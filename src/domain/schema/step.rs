//! The step from a document's parent version to its next: what a draft
//! may not carry, what never changes, and what the tool writes itself.

use super::ending::Ending;
use super::error::SchemaError;
use super::field::Date;
use super::kind::{Content, KindOf, Record};
use crate::domain::version::{Fields, Kind};

/// The fields the tool writes that every version of a document carries.
const CARRIED: [&str; 2] = ["created", "former_names"];

fn tool_owned(key: &str) -> bool {
    CARRIED.contains(&key) || Ending::KEYS.contains(&key)
}

/// A version's fields without the ones only the tool writes.
#[must_use]
pub fn shown(fields: &Fields) -> Fields {
    fields
        .iter()
        .filter(|(key, _)| !tool_owned(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn filled(
    draft: &Fields,
    parent: Option<&Fields>,
    kind: KindOf,
    today: Date,
) -> Result<Fields, SchemaError> {
    if let Some(key) = draft.keys().find(|key| tool_owned(key)) {
        return Err(SchemaError::ToolOwned(key.clone()));
    }
    let mut filled = draft.clone();
    match parent {
        Some(parent) => {
            for key in CARRIED {
                if let Some(value) = parent.get(key) {
                    filled.insert(key.to_owned(), value.clone());
                }
            }
        }
        None if kind.set_once().contains(&"created") => {
            filled.insert("created".to_owned(), today.value());
        }
        None => {}
    }
    Ok(filled)
}

/// The record a save stores for a draft: the draft's fields, the tool's
/// own carried from the parent's or set for a first version, the old
/// name kept on a rename, and a fact's `confirmed` moved to `today`
/// when its summary or body changed and the draft left the date alone.
/// Refuses a draft carrying a field only the tool writes, a change to
/// a field that never changes, and any step from an ended parent.
pub fn next(
    kind: &Kind,
    draft: &Fields,
    parent: Option<&Fields>,
    body_changed: bool,
    today: Date,
) -> Result<Record, SchemaError> {
    let kind_of = KindOf::of(kind)?;
    let was = parent
        .map(|fields| Record::read(kind, fields))
        .transpose()?;
    if was.as_ref().is_some_and(|was| was.ending.is_some()) {
        return Err(SchemaError::Ended);
    }
    let mut next = Record::read(kind, &filled(draft, parent, kind_of, today)?)?;
    let Some(was) = was else {
        return Ok(next);
    };
    let (before, after) = (was.fields(), next.fields());
    if let Some(key) = kind_of
        .set_once()
        .iter()
        .find(|key| before.get(**key) != after.get(**key))
    {
        return Err(SchemaError::SetOnce((*key).to_owned()));
    }
    if let Some((old, _)) = was.content.naming()
        && next.content.naming().is_some_and(|(new, _)| new != old)
    {
        keep_names(&mut next.content, std::slice::from_ref(&was));
    }
    if let (Content::Fact(was), Content::Fact(is)) = (&was.content, &mut next.content)
        && (body_changed || was.summary != is.summary)
        && was.confirmed == is.confirmed
    {
        is.confirmed = today;
    }
    Ok(next)
}

fn keep_names(content: &mut Content, held: &[Record]) {
    let Some(new) = content.naming().map(|(name, _)| name.clone()) else {
        return;
    };
    let Some(former) = content.former_names_mut() else {
        return;
    };
    former.retain(|name| *name != new);
    let names = held
        .iter()
        .filter_map(|head| head.content.naming())
        .flat_map(|(name, former)| former.iter().chain([name]));
    for name in names {
        if *name != new && !former.contains(name) {
            former.push(name.clone());
        }
    }
}

/// `next` for a version that joins several heads: `speaking` is the head
/// the draft was shown from and steps as the parent, an ended one as if
/// reopened, and `others` are the remaining heads in head order. The
/// former names kept are the parent's, the parent's name, then each other
/// head's former names and name, each once and never the saved name.
pub fn joined(
    kind: &Kind,
    draft: &Fields,
    speaking: &Fields,
    others: &[Record],
    body_changed: bool,
    today: Date,
) -> Result<Record, SchemaError> {
    let reopened = Record::read(kind, speaking)?
        .reopen()
        .map(|record| record.fields());
    let parent = reopened.as_ref().unwrap_or(speaking);
    let mut joined = next(kind, draft, Some(parent), body_changed, today)?;
    keep_names(&mut joined.content, others);
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::super::field::Name;
    use super::super::testing::{fields, record};
    use super::*;
    use crate::domain::testing::{ATLAS, LANTERN};

    fn day(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn today() -> Date {
        day("2026-10-09")
    }

    fn kind(word: &str) -> Kind {
        Kind::parse(word).unwrap()
    }

    fn fact_draft(name: &str, summary: &str, confirmed: &str) -> Fields {
        fields(&format!(
            "name = \"{name}\"\ntopic = \"{LANTERN}\"\nconfirmed = {confirmed}\nsummary = \"{summary}\"\n"
        ))
    }

    fn stored_fact() -> (Fields, Fields) {
        let draft = fact_draft("relay", "The relay pin is fixed", "2026-09-04");
        let stored = next(&kind("fact"), &draft, None, true, day("2026-09-04"))
            .unwrap()
            .fields();
        (stored, draft)
    }

    fn after(parent: &Fields, draft: &Fields, body_changed: bool) -> Result<Record, SchemaError> {
        next(&kind("fact"), draft, Some(parent), body_changed, today())
    }

    fn confirmed(record: &Record) -> Date {
        match &record.content {
            Content::Fact(fact) => fact.confirmed,
            _ => unreachable!(),
        }
    }

    fn former(record: &Record) -> Vec<&str> {
        let (_, former) = record.content.naming().unwrap();
        former.iter().map(Name::as_str).collect()
    }

    #[test]
    fn a_draft_shows_no_field_the_tool_owns_and_may_carry_none() {
        let ended = record(
            "fact",
            &format!(
                "name = \"relay\"\nformer_names = [\"relay-pin\"]\ntopic = \"{LANTERN}\"\n\
                 created = 2026-09-04\nconfirmed = 2026-09-04\nsummary = \"s\"\n\
                 ended = \"false\"\nended_on = 2026-10-09\nended_by = \"{ATLAS}\"\nnote = \"rewired\"\n"
            ),
        )
        .fields();
        let shown = shown(&ended);
        assert_eq!(
            shown.keys().map(String::as_str).collect::<Vec<_>>(),
            ["name", "topic", "confirmed", "summary"]
        );
        for key in ended.keys().filter(|key| !shown.contains_key(*key)) {
            let mut draft = shown.clone();
            draft.insert(key.clone(), "x".into());
            assert_eq!(
                next(&kind("fact"), &draft, None, false, today()),
                Err(SchemaError::ToolOwned(key.clone()))
            );
        }
    }

    #[test]
    fn a_first_version_is_created_today_where_its_kind_is_created() {
        let (stored, _) = stored_fact();
        assert_eq!(stored.get("created"), Some(&day("2026-09-04").value()));
        assert_eq!(stored.get("former_names"), None);
        let entry = fields(&format!(
            "name = \"lamp-driver\"\ndate = 2026-09-01\nmachine = \"{LANTERN}\"\nsummary = \"s\"\n"
        ));
        let stored = next(&kind("entry"), &entry, None, true, today()).unwrap();
        assert_eq!(stored.fields(), entry);
        assert_eq!(
            next(&kind("note"), &entry, None, true, today()),
            Err(SchemaError::UnknownKind("note".to_owned()))
        );
    }

    #[test]
    fn the_tools_fields_are_carried_from_the_parent() {
        let (stored, draft) = stored_fact();
        let same = after(&stored, &draft, false).unwrap();
        assert_eq!(same.fields(), stored);
    }

    #[test]
    fn an_ended_document_takes_no_step() {
        let (stored, draft) = stored_fact();
        let mut ended = stored;
        ended.insert("ended".to_owned(), "false".into());
        ended.insert("ended_on".to_owned(), today().value());
        ended.insert("note".to_owned(), "rewired".into());
        assert_eq!(after(&ended, &draft, false), Err(SchemaError::Ended));
    }

    #[test]
    fn a_field_that_never_changes_refuses_a_change() {
        let step = |word: &str, was: &str, is: &str| {
            let kind = kind(word);
            let stored = next(&kind, &fields(was), None, true, today())
                .unwrap()
                .fields();
            next(&kind, &fields(is), Some(&stored), false, today())
        };
        let entry = |date: &str, machine: &str| {
            format!(
                "name = \"lamp-driver\"\ndate = {date}\nmachine = \"{machine}\"\nsummary = \"s\"\n"
            )
        };
        let followup = |rest: &str| {
            format!("topics = [\"{LANTERN}\"]\n{rest}summary = \"Add the second relay\"\n")
        };
        let claim = |directory: &str| {
            format!("machine = \"{ATLAS}\"\ntopic = \"{LANTERN}\"\ndirectory = \"{directory}\"\n")
        };
        let about = |id: &str| followup(&format!("about = \"{id}\"\n"));
        for (word, was, is, key) in [
            (
                "entry",
                entry("2026-09-04", LANTERN),
                entry("2026-09-05", LANTERN),
                "date",
            ),
            (
                "entry",
                entry("2026-09-04", LANTERN),
                entry("2026-09-04", ATLAS),
                "machine",
            ),
            ("followup", about(LANTERN), about(ATLAS), "about"),
            ("followup", about(LANTERN), followup(""), "about"),
            (
                "followup",
                followup(""),
                followup(&format!("entry = \"{ATLAS}\"\n")),
                "entry",
            ),
            ("claim", claim("~/lantern"), claim("~/lamp"), "directory"),
        ] {
            assert_eq!(
                step(word, &was, &is),
                Err(SchemaError::SetOnce(key.to_owned())),
                "{word} {key}"
            );
        }
        assert!(
            step(
                "entry",
                &entry("2026-09-04", LANTERN),
                &entry("2026-09-04", LANTERN)
            )
            .is_ok()
        );
    }

    #[test]
    fn a_rename_keeps_the_old_name_and_a_name_taken_back_stops_being_former() {
        let (first, _) = stored_fact();
        let draft = |name: &str| fact_draft(name, "The relay pin is fixed", "2026-09-04");
        let second = after(&first, &draft("relay-pin"), false).unwrap();
        assert_eq!(former(&second), ["relay"]);
        assert_eq!(confirmed(&second), day("2026-09-04"));

        let third = after(&second.fields(), &draft("relay-gpio"), false).unwrap();
        assert_eq!(former(&third), ["relay", "relay-pin"]);

        let back = after(&third.fields(), &draft("relay"), false).unwrap();
        assert_eq!(former(&back), ["relay-pin", "relay-gpio"]);
    }

    #[test]
    fn an_edit_of_the_claim_moves_confirmed_unless_the_draft_set_it() {
        let (stored, same) = stored_fact();
        let reworded = fact_draft("relay", "The relay pin cannot move", "2026-09-04");
        assert_eq!(
            confirmed(&after(&stored, &reworded, false).unwrap()),
            today()
        );
        assert_eq!(confirmed(&after(&stored, &same, true).unwrap()), today());
        let stated = fact_draft("relay", "The relay pin cannot move", "2026-09-20");
        assert_eq!(
            confirmed(&after(&stored, &stated, true).unwrap()),
            day("2026-09-20")
        );
        let mut refiled = same;
        refiled.insert("topic".to_owned(), ATLAS.into());
        assert_eq!(
            confirmed(&after(&stored, &refiled, false).unwrap()),
            day("2026-09-04")
        );
    }

    fn topic(name: &str, former: &str) -> Fields {
        fields(&format!(
            "name = \"{name}\"\nformer_names = [{former}]\ncreated = 2026-09-04\nsummary = \"s\"\n"
        ))
    }

    fn former_after(base: &Fields, others: &[&Fields], saved: &str) -> Vec<String> {
        let kind = kind("topic");
        let others: Vec<Record> = others
            .iter()
            .map(|other| Record::read(&kind, other).unwrap())
            .collect();
        let draft = fields(&format!("name = \"{saved}\"\nsummary = \"s\"\n"));
        let stored = joined(&kind, &draft, base, &others, false, today()).unwrap();
        let (_, former) = stored.content.naming().unwrap();
        former.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_step_from_several_heads_keeps_what_each_was_called_but_the_saved_name() {
        let plain = topic("lantern", "");
        let renamed = topic("lamp-two", "\"lantern\", \"lamp\"");
        assert_eq!(
            former_after(&plain, &[&renamed], "lamp-two"),
            ["lantern", "lamp"]
        );
        assert_eq!(
            former_after(&renamed, &[&plain], "lamp-two"),
            ["lantern", "lamp"]
        );
        assert_eq!(
            former_after(&plain, &[&renamed], "beacon"),
            ["lantern", "lamp", "lamp-two"]
        );
        assert_eq!(
            former_after(&renamed, &[&plain], "beacon"),
            ["lantern", "lamp", "lamp-two"]
        );
        assert_eq!(
            former_after(&renamed, &[&plain], "lamp"),
            ["lantern", "lamp-two"]
        );
        assert_eq!(
            former_after(&renamed, &[&renamed], "lamp-two"),
            ["lantern", "lamp"]
        );
        assert_eq!(former_after(&renamed, &[], "lamp-two"), ["lantern", "lamp"]);
        assert_eq!(former_after(&plain, &[], "lamp"), ["lantern"]);
    }
}
