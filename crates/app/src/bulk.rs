use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec::Vec;

use serde::Serialize;
use toml::Value;

use crate::heads::texts;
use crate::live::{Open, opened};
use crate::lookup::{Lookup, only};
use crate::rules::referencable;
use crate::save::Admitted;
use crate::{Deps, Failure, Reference, Written, text};
use worklog_domain::id::DocumentId;
use worklog_domain::version::Fields;
use worklog_schema::Content;
use worklog_schema::address::{Address, Found};
use worklog_schema::kind::Role;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Moved {
    pub to: Reference,
    pub moved: Vec<Written>,
    /// The documents left as they stood, being already under `to`.
    pub already: Vec<Reference>,
}

/// A failed move; `done` holds the versions it stored before failing, `None` when it stored none.
#[derive(Debug)]
pub struct Halted {
    pub done: Option<Box<Moved>>,
    pub failure: Failure,
}

impl From<Failure> for Halted {
    fn from(failure: Failure) -> Halted {
        Halted {
            done: None,
            failure,
        }
    }
}

/// Writes nothing unless every document passes its checks.
pub fn move_to(deps: &Deps, documents: &[&str], from: &str, to: &str) -> Result<Moved, Halted> {
    let lookup = Lookup::new(deps.read().store);
    let from_id = referencable(&lookup, from)?;
    let to_id = referencable(&lookup, to)?;
    if from_id == to_id {
        return Err(Failure::Usage(format!("{from} and {to} are one topic")).into());
    }
    let mut listed: Vec<DocumentId> = Vec::new();
    let mut moves: Vec<Admitted> = Vec::new();
    let mut done = Moved {
        to: lookup.reference(&to_id)?,
        moved: Vec::new(),
        already: Vec::new(),
    };
    for address in documents {
        let target = opened(deps, &lookup, resolved(&lookup, address, &from_id, &to_id)?)?;
        if listed.contains(&target.id) {
            return Err(Failure::at(&target.label, "is listed twice").into());
        }
        target.unended()?;
        match filing(&target, &from_id, &to_id) {
            Filing::Under(fields) => {
                moves.push(target.checked(deps, &lookup, &fields, "move")?);
            }
            Filing::Already => done.already.push(lookup.reference(&target.id)?),
            Filing::Neither => {
                return Err(Failure::at(&target.label, format!("is not under {from}")).into());
            }
        }
        listed.push(target.id);
    }
    for admitted in moves {
        match admitted.store(deps) {
            Ok(written) => done.moved.push(written),
            Err(failure) => {
                let done = (!done.moved.is_empty()).then(|| Box::new(done));
                return Err(Halted { done, failure });
            }
        }
    }
    Ok(done)
}

// A fact's address carries its topic, so a fact already moved is not found at the address it
// was given.
fn resolved(
    lookup: &Lookup,
    address: &str,
    from: &DocumentId,
    to: &DocumentId,
) -> Result<DocumentId, Failure> {
    let mut found = lookup.find(address)?;
    if found == Found::None
        && let Ok(Address::Fact { topic, name }) = Address::parse(address)
        && lookup.find(topic.as_str())? == Found::One(from.clone())
    {
        found = lookup.fact_named(to, &name)?;
    }
    only(found, address)
}

enum Filing {
    Under(Fields),
    Already,
    Neither,
}

fn filing(target: &Open, from: &DocumentId, to: &DocumentId) -> Filing {
    let content = &target.record.content;
    let filed_under = match content {
        Content::Fact(_) | Content::Entry(_) | Content::Followup(_) => {
            (content.kind()).key_where(|reference| reference.role == Role::Filed)
        }
        Content::Topic(_) | Content::Claim(_) => None,
    };
    let Some(filed_under) = filed_under else {
        return Filing::Neither;
    };
    let topics = texts(&target.head.fields, filed_under);
    if !topics.contains(&from.as_str()) {
        return if topics.contains(&to.as_str()) {
            Filing::Already
        } else {
            Filing::Neither
        };
    }
    let mut fields = target.shown();
    let is_from = |value: &Value| value.as_str() == Some(from.as_str());
    let to_text = || text(to.as_str());
    let filed = match fields.get(filed_under) {
        Some(Value::Array(topics)) => {
            let mut replaced: Vec<Value> = Vec::new();
            for topic in topics {
                let topic = if is_from(topic) {
                    to_text()
                } else {
                    topic.clone()
                };
                if !replaced.contains(&topic) {
                    replaced.push(topic);
                }
            }
            Value::Array(replaced)
        }
        _ => to_text(),
    };
    fields.insert(filed_under.to_owned(), filed);
    if fields.get("touching").is_some_and(is_from) {
        fields.insert("touching".to_owned(), to_text());
    }
    Filing::Under(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{World, fact, head, head_id, referred, refused, world_with_atlas};
    use alloc::string::{String, ToString};
    use worklog_domain::id::DocumentId;
    use worklog_domain::ports::Store;

    fn move_to(deps: &Deps, documents: &[&str], from: &str, to: &str) -> Result<Moved, Failure> {
        super::move_to(deps, documents, from, to).map_err(|halted| halted.failure)
    }

    fn followup(
        world: &World,
        topics: &[&DocumentId],
        touching: Option<&DocumentId>,
    ) -> DocumentId {
        let list: Vec<String> = topics.iter().map(|id| format!("\"{id}\"")).collect();
        let touching = touching.map_or_else(String::new, |id| format!("touching = \"{id}\"\n"));
        world.put(
            "followup",
            &format!(
                "created = 2026-09-04\ntopics = [{}]\n{touching}summary = \"s\"\n",
                list.join(", ")
            ),
            "\n",
        )
    }

    #[test]
    fn facts_and_a_followup_move_to_the_other_topic() {
        let (world, lantern, atlas) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let both = followup(&world, &[&lantern, &atlas], Some(&lantern));
        let only = followup(&world, &[&lantern], None);
        let addresses = [
            "lantern/relay-pin",
            "lantern/lamp-pin",
            both.as_str(),
            only.as_str(),
        ];
        let Moved {
            to,
            moved: written,
            already,
        } = move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap();
        assert_eq!((&to.document, to.label.as_str()), (&atlas, "atlas"));
        assert_eq!(written.len(), 4);
        assert_eq!(already, []);

        for (id, written) in [&relay, &lamp].into_iter().zip(&written) {
            assert_eq!(&written.document, id);
            let moved = head(&world, id);
            assert_eq!(moved.id, written.version);
            assert_eq!(moved.envelope.change, "move");
            assert_eq!(moved.fields["topic"].as_str(), Some(atlas.as_str()));
            assert_eq!(moved.fields["confirmed"].to_string(), "2026-09-04");
        }
        let moved = head(&world, &both);
        let topics = moved.fields["topics"].as_array().unwrap();
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].as_str(), Some(atlas.as_str()));
        assert_eq!(moved.fields["touching"].as_str(), Some(atlas.as_str()));
        let moved = head(&world, &only);
        assert_eq!(
            moved.fields["topics"].as_array().unwrap()[0].as_str(),
            Some(atlas.as_str())
        );
    }

    fn stored(done: &Moved) -> Vec<&str> {
        (done.moved.iter())
            .map(|written| written.label.as_str())
            .collect()
    }

    #[test]
    fn a_document_already_under_the_other_topic_is_left_as_it_stands() {
        let (world, lantern, atlas) = world_with_atlas();
        fact(&world, &lantern, "relay-pin", "");
        let map = fact(&world, &atlas, "map", "");
        let filed = followup(&world, &[&atlas], None);
        let before = names_of(&world, &[&map, &filed]);
        let addresses = ["atlas/map", "lantern/relay-pin", filed.as_str()];
        let done = move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap();
        assert_eq!(stored(&done), ["atlas/relay-pin"]);
        assert_eq!(referred(&done.already), ["atlas/map", filed.short()]);
        assert_eq!(names_of(&world, &[&map, &filed]), before);
    }

    #[test]
    fn a_fact_already_moved_is_found_by_the_address_it_had_under_the_first_topic() {
        let (world, lantern, _) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        fact(&world, &lantern, "lamp-pin", "");
        move_to(&world.deps(), &["lantern/relay-pin"], "lantern", "atlas").unwrap();
        let before = head_id(&world, &relay);
        let addresses = ["lantern/relay-pin", "lantern/lamp-pin"];
        let done = move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap();
        assert_eq!(stored(&done), ["atlas/lamp-pin"]);
        assert_eq!(referred(&done.already), ["atlas/relay-pin"]);
        assert_eq!(head_id(&world, &relay), before);

        let text = refused(move_to(
            &world.deps(),
            &["lantern/fuse"],
            "lantern",
            "atlas",
        ));
        assert_eq!(text, "lantern/fuse: names no document");
    }

    #[test]
    fn a_move_a_store_problem_stopped_gives_what_it_stored_and_runs_again() {
        let (world, lantern, _) = world_with_atlas();
        fact(&world, &lantern, "relay-pin", "");
        fact(&world, &lantern, "lamp-pin", "");
        let world = world.counted();
        let addresses = ["lantern/relay-pin", "lantern/lamp-pin"];
        world.store.puts_left.set(Some(1));
        let halted = super::move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap_err();
        assert!(matches!(halted.failure, Failure::Store(_)), "{halted:?}");
        let stopped = halted.done.unwrap();
        assert_eq!(stored(&stopped), ["atlas/relay-pin"]);

        world.store.puts_left.set(Some(0));
        let halted = super::move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap_err();
        assert!(matches!(halted.failure, Failure::Store(_)), "{halted:?}");
        assert_eq!(halted.done, None);

        world.store.puts_left.set(None);
        let done = move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap();
        assert_eq!(stored(&done), ["atlas/lamp-pin"]);
        assert_eq!(referred(&done.already), ["atlas/relay-pin"]);
    }

    #[test]
    fn a_refusal_for_any_document_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let phone = crate::testing::topic(&world, "phone", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let map = fact(&world, &phone, "map", "");
        let before = (head_id(&world, &relay), head_id(&world, &lamp));
        let text = refused(move_to(
            &world.deps(),
            &["lantern/relay-pin", "lantern/lamp-pin", "phone/map"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("is not under lantern"), "{text}");
        assert_eq!(head(&world, &relay).id, before.0);
        assert_eq!(head(&world, &lamp).id, before.1);
        assert_eq!(head(&world, &map).envelope.change, "new");
    }

    #[test]
    fn an_ended_document_is_refused() {
        let (world, lantern, _) = world_with_atlas();
        let old = world.put(
            "fact",
            &format!(
                "name = \"old-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\nended = \"false\"\n\
                 ended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "\n",
        );
        let text = refused(move_to(&world.deps(), &[old.as_str()], "lantern", "atlas"));
        assert!(text.contains("is ended"), "{text}");
    }

    #[test]
    fn a_topic_moved_to_itself_is_usage() {
        let (world, lantern, _) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let result = move_to(&world.deps(), &[relay.as_str()], "lantern", "lantern");
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    fn names_of(world: &World, ids: &[&DocumentId]) -> Vec<worklog_domain::id::VersionId> {
        ids.iter().map(|id| head_id(world, id)).collect()
    }

    #[test]
    fn a_fact_name_taken_under_the_target_topic_writes_nothing() {
        let (world, lantern, atlas) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        fact(&world, &atlas, "relay-pin", "");
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(
            text.contains("lantern/relay-pin: name: relay-pin is taken by atlas/relay-pin"),
            "{text}"
        );
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_document_listed_twice_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin", "lantern/lamp-pin"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("lantern/lamp-pin: is listed twice"), "{text}");
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_document_with_a_draft_open_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        crate::draft::checkout(&world.deps(), "lantern/relay-pin").unwrap();
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(
            text.contains("a draft of lantern/relay-pin is open"),
            "{text}"
        );
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_topic_in_the_list_is_not_under_the_topic() {
        let (world, lantern, atlas) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let before = names_of(&world, &[&lamp, &atlas]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "atlas"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("atlas: is not under lantern"), "{text}");
        assert_eq!(names_of(&world, &[&lamp, &atlas]), before);
    }

    #[test]
    fn a_document_holding_an_unreadable_version_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let planted = worklog_domain::testing::first(&relay, "fact", "name = \"x\"", "\n")
            .id
            .clone();
        world
            .store
            .plant_unreadable(&relay, planted, worklog_domain::version::ReadError::Corrupt);
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(
            text.contains("lantern/relay-pin: holds a version this worklog cannot read"),
            "{text}"
        );
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_fact_name_a_forked_fact_holds_under_the_target_topic_writes_nothing() {
        let (world, lantern, atlas) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let forked = fact(&world, &atlas, "relay-pin", "");
        let root = head(&world, &forked);
        for body in ["left\n", "right\n"] {
            let fields = format!(
                "name = \"relay-pin\"\ntopic = \"{atlas}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n"
            );
            let version = worklog_domain::testing::after(&[&root], &fields, body);
            world.store.put(&version).unwrap();
        }
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("name: relay-pin is taken by"), "{text}");
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }
}
