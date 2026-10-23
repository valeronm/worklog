use serde::Serialize;
use toml::Value;

use crate::app::heads::label_or_short;
use crate::app::live::open;
use crate::app::lookup::{Lookup, draft_open};
use crate::app::rules::{name_is_free, referencable};
use crate::app::{Deps, DraftRef, Failed, Failure, ReadDeps, kind_named, text};
use crate::domain::draft::{Draft, Listed};
use crate::domain::id::{DocumentId, is_id_prefix};
use crate::domain::ports::BrokenDraft;
use crate::domain::schema::address::{Address, Found};
use crate::domain::schema::{Content, Date, Entry, Fact, KindOf, Name, Topic, step, translate};
use crate::domain::session::Session;
use crate::domain::version::{Fields, Kind, Stamp, Version};

pub enum New<'a> {
    Topic {
        name: &'a str,
    },
    /// `address` is `topic/name`.
    Fact {
        address: &'a str,
        idea: bool,
    },
    /// `date` is `YYYY-MM-DD`; today when absent.
    Entry {
        name: &'a str,
        date: Option<&'a str>,
    },
}

const FACT_BODY: &str = "\n\n**Why:** \n\n**How to apply:** \n";
const ENTRY_BODY: &str = "\n## What\n\n## Why\n\n## Changes\n\n## Notes\n";

pub(super) fn usage(error: impl std::fmt::Display) -> Failure {
    Failure::Usage(error.to_string())
}

pub(super) fn named(lookup: &Lookup, id: &DocumentId) -> Result<String, Failure> {
    Ok(lookup.address(id)?.unwrap_or_else(|| id.to_string()))
}

/// Refuses a name taken in its scope and an address that already has a draft; a malformed
/// name or date is a usage failure.
pub fn new(deps: &Deps, what: &New) -> Result<DraftRef, Failure> {
    let lookup = Lookup::new(deps.read().store);
    let today = deps.read().today()?;
    let mut fields = Fields::new();
    let (content, address, body) = match what {
        New::Topic { name } => {
            let name = Name::parse(name).map_err(usage)?;
            fields.insert("name".to_owned(), text(name.as_str()));
            fields.insert("summary".to_owned(), text(""));
            let address = name.to_string();
            let topic = Topic {
                name,
                former_names: Vec::new(),
                created: today,
                summary: String::new(),
                part_of: Vec::new(),
                uses: Vec::new(),
            };
            (Content::Topic(topic), address, "\n")
        }
        New::Fact { address, idea } => {
            let Address::Fact { topic, name } = Address::parse(address).map_err(usage)? else {
                return Err(usage(format!("{address}: a fact is addressed topic/name")));
            };
            let topic_id = referencable(&lookup, topic.as_str())?;
            let topic_name = named(&lookup, &topic_id)?;
            fields.insert("name".to_owned(), text(name.as_str()));
            fields.insert("topic".to_owned(), text(&topic_name));
            fields.insert("confirmed".to_owned(), today.value());
            if *idea {
                fields.insert("idea".to_owned(), Value::Boolean(true));
            }
            fields.insert("summary".to_owned(), text(""));
            let address = format!("{topic_name}/{name}");
            let fact = Fact {
                name,
                former_names: Vec::new(),
                topic: topic_id,
                created: today,
                confirmed: today,
                idea: *idea,
                summary: String::new(),
            };
            (Content::Fact(fact), address, FACT_BODY)
        }
        New::Entry { name, date } => {
            let name = Name::parse(name).map_err(usage)?;
            let date = match date {
                Some(date) => Date::parse(date).map_err(usage)?,
                None => today,
            };
            let machine = deps.read().machine()?;
            let machine_name = named(&lookup, &machine)?;
            fields.insert("name".to_owned(), text(name.as_str()));
            fields.insert("date".to_owned(), date.value());
            fields.insert("machine".to_owned(), text(&machine_name));
            fields.insert("topics".to_owned(), Value::Array(Vec::new()));
            fields.insert("summary".to_owned(), text(""));
            let address = format!("{date}-{name}");
            let entry = Entry {
                name,
                former_names: Vec::new(),
                date,
                machine,
                topics: Vec::new(),
                summary: String::new(),
            };
            (Content::Entry(entry), address, ENTRY_BODY)
        }
    };
    let id = deps.ids.mint()?;
    name_is_free(&lookup, &id, &content)?;
    for Listed { draft: open, .. } in deps.drafts.list()?.into_iter().flatten() {
        if spelled(&open.kind, &open.fields).as_deref() == Some(address.as_str()) {
            return Err(draft_open(&address));
        }
    }
    open_draft(
        deps,
        Draft::first(id, kind_named(content.kind()), fields, body.to_owned()),
    )
}

/// Writes `draft` as opened in this process's session.
pub(super) fn open_draft(deps: &Deps, mut draft: Draft) -> Result<DraftRef, Failure> {
    draft.opened_in = deps.host.session();
    let location = deps.drafts.write(&draft)?;
    Ok(DraftRef {
        document: draft.document,
        location,
    })
}

pub(super) fn is_drafted(deps: &ReadDeps, id: &DocumentId) -> Result<bool, Failure> {
    Ok(deps.drafts.documents()?.contains(id))
}

pub(super) fn refuse_open(deps: &ReadDeps, id: &DocumentId, label: &str) -> Result<(), Failure> {
    if is_drafted(deps, id)? {
        return Err(draft_open(label));
    }
    Ok(())
}

/// Refuses a claim, an ended, forked or unreadable document, and one that has a draft open.
pub fn checkout(deps: &Deps, address: &str) -> Result<DraftRef, Failure> {
    let lookup = Lookup::new(deps.read().store);
    let target = open(deps, &lookup, address)?;
    let kind = KindOf::of(&target.head.envelope.kind)?;
    if kind == KindOf::Claim {
        return Err(Failure::at(&target.label, "a claim is never edited"));
    }
    target.unended()?;
    open_draft(deps, opened(&lookup, &target.head, kind)?)
}

pub(super) fn opened(lookup: &Lookup, version: &Version, kind: KindOf) -> Result<Draft, Failure> {
    let mut draft = Draft::of(version);
    draft.fields = names(lookup, &step::shown(&version.fields), kind)?;
    Ok(draft)
}

pub(super) fn names(lookup: &Lookup, fields: &Fields, kind: KindOf) -> Result<Fields, Failure> {
    shown_as(fields, kind, |id| lookup.address(id))
}

pub(super) fn shown_as(
    fields: &Fields,
    kind: KindOf,
    name_of: impl Fn(&DocumentId) -> Result<Option<String>, Failure>,
) -> Result<Fields, Failure> {
    let failed = Failed::default();
    let shown = translate::to_names(fields, kind, |id| {
        name_of(id).unwrap_or_else(|failure| {
            failed.keep(failure);
            None
        })
    });
    failed.done()?;
    Ok(shown)
}

pub fn discard(deps: &Deps, address: &str) -> Result<(), Failure> {
    let lookup = Lookup::new(deps.read().store);
    let document = drafted_document(&deps.read(), &lookup, address)?;
    deps.drafts.delete(&document)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DraftRow {
    Open(OpenDraft),
    Broken(BrokenRow),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BrokenRow {
    pub document: DocumentId,
    #[serde(rename = "path")]
    pub location: String,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenDraft {
    pub document: DocumentId,
    pub kind: String,
    pub label: String,
    #[serde(rename = "path")]
    pub location: String,
    pub opener: Opener,
    pub touched: Stamp,
    /// Whole seconds from `touched` to now; 0 for a draft touched after now.
    pub age_seconds: u64,
}

/// Whose a draft is, by the agent session it was opened in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Opener {
    ThisSession,
    OtherSession { agent: String, session: String },
    NoSession,
}

pub fn drafts(deps: &ReadDeps) -> Result<Vec<DraftRow>, Failure> {
    let held = deps.drafts.list()?;
    let now = deps.clock.now().instant();
    let this = deps.host.session();
    let row = |held: Result<Listed, BrokenDraft>| {
        let Listed { draft, touched } = match held {
            Ok(listed) => listed,
            Err(BrokenDraft { document, error }) => {
                return DraftRow::Broken(BrokenRow {
                    location: deps.drafts.location(&document),
                    document,
                    error: error.to_string(),
                });
            }
        };
        let (label, location) = (draft_label(&draft), deps.drafts.location(&draft.document));
        let opener = match draft.opened_in {
            None => Opener::NoSession,
            Some(session) if this.as_ref() == Some(&session) => Opener::ThisSession,
            Some(Session { agent, id }) => Opener::OtherSession { agent, session: id },
        };
        let age_seconds = u64::try_from((now - touched.instant()) / 1_000_000_000).unwrap_or(0);
        DraftRow::Open(OpenDraft {
            document: draft.document,
            kind: draft.kind.to_string(),
            label,
            location,
            opener,
            touched,
            age_seconds,
        })
    };
    Ok(held.into_iter().map(row).collect())
}

/// Refuses an address that finds no draft or several, and one whose draft does not read.
pub(super) fn draft_for(deps: &ReadDeps, lookup: &Lookup, address: &str) -> Result<Draft, Failure> {
    let document = drafted_document(deps, lookup, address)?;
    deps.drafts
        .read(&document)?
        .ok_or_else(|| Failure::at(address, "no draft"))
}

// The draft of a stored document the address finds is tried before a draft whose document's
// id starts with the address or whose fields spell it. A draft that does not read has no fields
// to spell an address.
fn drafted_document(
    deps: &ReadDeps,
    lookup: &Lookup,
    address: &str,
) -> Result<DocumentId, Failure> {
    match lookup.find(address) {
        Ok(Found::One(id)) => {
            if deps.drafts.documents()?.contains(&id) {
                return Ok(id);
            }
        }
        Ok(Found::Collision(_)) => {
            lookup.one(address)?;
        }
        Ok(Found::None) | Err(Failure::Usage(_)) => {}
        Err(other) => return Err(other),
    }
    let by_id =
        |document: &DocumentId| is_id_prefix(address) && document.as_str().starts_with(address);
    let mut matching: Vec<DocumentId> = (deps.drafts.list()?.into_iter())
        .filter_map(|held| match held {
            Ok(Listed { draft, .. }) => (by_id(&draft.document)
                || spelled(&draft.kind, &draft.fields).as_deref() == Some(address))
            .then_some(draft.document),
            Err(broken) => by_id(&broken.document).then_some(broken.document),
        })
        .collect();
    match matching.len() {
        0 => Err(Failure::at(address, "no draft")),
        1 => Ok(matching.remove(0)),
        _ => Err(Failure::at(address, "several drafts")),
    }
}

pub(super) fn draft_label(draft: &Draft) -> String {
    label_or_short(spelled(&draft.kind, &draft.fields), &draft.document)
}

pub(super) fn spelled(kind: &Kind, fields: &Fields) -> Option<String> {
    let field = |key: &str| fields.get(key);
    let name = field("name")?.as_str()?;
    match kind.as_str() {
        "topic" => Some(name.to_owned()),
        "fact" => Some(format!("{}/{name}", field("topic")?.as_str()?)),
        "entry" => match field("date")? {
            Value::Datetime(date) => Some(format!("{date}-{name}")),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{
        ENDED, ENDED_FACT, TOPIC, World, fact, head, head_id, open_row, refused, topic, world,
    };
    use crate::domain::ports::{Drafts, DraftsRead, Store};
    use crate::domain::testing::lamp_session;
    use crate::domain::version::{Kind, ReadError};

    fn stored(world: &World, id: &DocumentId) -> Draft {
        world.drafts.read(id).unwrap().expect("a draft")
    }

    #[test]
    fn a_new_topic_is_a_draft_of_a_name_and_a_summary() {
        let (world, _) = world();
        let deps = world.deps();
        let opened = new(&deps, &New::Topic { name: "phone" }).unwrap();
        let draft = stored(&world, &opened.document);
        assert_eq!(opened.location, world.drafts.location(&opened.document));
        assert_eq!(draft.kind.as_str(), "topic");
        assert!(draft.parents.is_empty());
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            "name = \"phone\"\nsummary = \"\"\n"
        );
        assert_eq!(draft.body, "\n");
    }

    #[test]
    fn a_new_fact_names_its_topic_and_confirms_today() {
        let (world, _) = world();
        let deps = world.deps();
        let opened = new(
            &deps,
            &New::Fact {
                address: "lantern/relay-pin",
                idea: false,
            },
        )
        .unwrap();
        let draft = stored(&world, &opened.document);
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            "name = \"relay-pin\"\ntopic = \"lantern\"\nconfirmed = 2026-10-09\nsummary = \"\"\n"
        );
        assert_eq!(draft.body, "\n\n**Why:** \n\n**How to apply:** \n");

        let idea = new(
            &deps,
            &New::Fact {
                address: "lantern/spare",
                idea: true,
            },
        )
        .unwrap();
        assert_eq!(
            toml::to_string(&stored(&world, &idea.document).fields).unwrap(),
            "name = \"spare\"\ntopic = \"lantern\"\nconfirmed = 2026-10-09\nidea = true\nsummary = \"\"\n"
        );

        let text = refused(new(
            &deps,
            &New::Fact {
                address: "unknown/relay-pin",
                idea: false,
            },
        ));
        assert!(text.contains("names no document"), "{text}");
    }

    #[test]
    fn a_new_fact_goes_under_a_forked_topic_with_a_head_that_is_not_ended() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        for (rest, body) in [("", "left\n"), (ENDED, "right\n")] {
            let fork = crate::domain::testing::after(
                &[&root],
                &format!("name = \"lantern\"\n{TOPIC}{rest}"),
                body,
            );
            world.store.put(&fork).unwrap();
        }
        let what = New::Fact {
            address: "lantern/relay-pin",
            idea: false,
        };
        let opened = new(&world.deps(), &what).unwrap();
        let draft = stored(&world, &opened.document);
        assert_eq!(draft.fields.get("topic"), Some(&text("lantern")));
    }

    #[test]
    fn a_new_fact_refuses_an_ended_topic() {
        let (world, lantern) = world();
        world.amend(
            &lantern,
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "\n",
        );
        let text = refused(new(
            &world.deps(),
            &New::Fact {
                address: "lantern/relay-pin",
                idea: false,
            },
        ));
        assert!(text.contains("lantern: is ended"), "{text}");
    }

    #[test]
    fn a_new_entry_is_dated_and_made_on_this_machine() {
        let (world, _) = world();
        let deps = world.deps();
        let today = new(
            &deps,
            &New::Entry {
                name: "lamp-driver",
                date: None,
            },
        )
        .unwrap();
        let draft = stored(&world, &today.document);
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            "name = \"lamp-driver\"\ndate = 2026-10-09\nmachine = \"desk\"\ntopics = []\nsummary = \"\"\n"
        );
        assert_eq!(
            draft.body,
            "\n## What\n\n## Why\n\n## Changes\n\n## Notes\n"
        );

        let earlier = new(
            &deps,
            &New::Entry {
                name: "lamp-driver",
                date: Some("2026-10-01"),
            },
        )
        .unwrap();
        assert_eq!(
            stored(&world, &earlier.document).fields.get("date"),
            Some(&"2026-10-01".parse::<toml::Value>().unwrap())
        );

        let result = new(
            &deps,
            &New::Entry {
                name: "lamp-driver",
                date: Some("soon"),
            },
        );
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    #[test]
    fn a_new_document_refuses_a_taken_address_a_second_draft_and_a_bad_name() {
        let (world, _) = world();
        let deps = world.deps();
        let text = refused(new(&deps, &New::Topic { name: "lantern" }));
        assert!(
            text.contains("lantern: name: lantern is taken by lantern"),
            "{text}"
        );

        new(&deps, &New::Topic { name: "phone" }).unwrap();
        let text = refused(new(&deps, &New::Topic { name: "phone" }));
        assert!(text.contains("a draft of phone is open"), "{text}");

        let result = new(&deps, &New::Topic { name: "Lantern" });
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    #[test]
    fn an_ended_holder_does_not_block_a_new_document() {
        let (world, lantern) = world();
        world.amend(
            &lantern,
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "\n",
        );
        assert!(new(&world.deps(), &New::Topic { name: "lantern" }).is_ok());
    }

    #[test]
    fn a_checkout_shows_names_and_hides_the_tools_fields() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "former_names = [\"old\"]\n");
        let head = head_id(&world, &relay);
        let opened = checkout(&world.deps(), "lantern/relay-pin").unwrap();
        assert_eq!(opened.document, relay);
        let draft = stored(&world, &relay);
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            "name = \"relay-pin\"\ntopic = \"lantern\"\nconfirmed = 2026-09-04\nsummary = \"s\"\n"
        );
        assert_eq!(draft.parents, vec![head]);
        assert_eq!(draft.body, "The relay.\n");

        let text = refused(checkout(&world.deps(), "lantern/relay-pin"));
        assert!(
            text.contains("a draft of lantern/relay-pin is open"),
            "{text}"
        );
    }

    #[test]
    fn a_checkout_refuses_what_cannot_be_edited() {
        let (world, lantern) = world();
        fact(&world, &lantern, "old-pin", ENDED_FACT);
        let text = refused(checkout(&world.deps(), "lantern/old-pin"));
        assert!(text.contains("is ended; reopen it first"), "{text}");

        let phone = topic(&world, "phone", "");
        let root = head(&world, &phone);
        for body in ["left\n", "right\n"] {
            let fork = crate::domain::testing::after(
                &[&root],
                &format!("name = \"phone\"\n{TOPIC}"),
                body,
            );
            world.store.put(&fork).unwrap();
        }
        let text = refused(checkout(&world.deps(), "phone"));
        assert!(text.contains("is forked"), "{text}");

        let claim = world.put(
            "claim",
            &format!("machine = \"{}\"\ntopic = \"{lantern}\"\n", world.machine()),
            "\n",
        );
        let text = refused(checkout(&world.deps(), claim.as_str()));
        assert!(text.contains("a claim is never edited"), "{text}");

        let atlas = topic(&world, "atlas", "");
        let planted = crate::domain::testing::first(&atlas, "topic", "name = \"x\"", "\n")
            .id
            .clone();
        world
            .store
            .plant_unreadable(&atlas, planted, ReadError::Corrupt);
        let text = refused(checkout(&world.deps(), "atlas"));
        assert!(
            text.contains("holds a version this worklog cannot read"),
            "{text}"
        );
    }

    #[test]
    fn an_address_means_a_draft_by_document_by_id_or_by_what_it_spells() {
        let (world, lantern) = world();
        let deps = world.deps();
        let lookup = world.lookup();
        let relay = fact(&world, &lantern, "relay-pin", "");
        checkout(&deps, "lantern/relay-pin").unwrap();
        let fresh = new(&deps, &New::Topic { name: "phone" }).unwrap();

        let by_address = draft_for(&deps.read(), &lookup, "lantern/relay-pin").unwrap();
        assert_eq!(by_address.document, relay);
        let remote = Draft::first(
            DocumentId::from_bytes([0xab; 16]),
            Kind::parse("followup").unwrap(),
            Fields::new(),
            String::new(),
        );
        world.drafts.write(&remote).unwrap();
        let by_id = draft_for(&deps.read(), &lookup, remote.document.short()).unwrap();
        assert_eq!(by_id.document, remote.document);
        let by_spelling = draft_for(&deps.read(), &lookup, "phone").unwrap();
        assert_eq!(by_spelling.document, fresh.document);

        let text = refused(draft_for(&deps.read(), &lookup, "unknown"));
        assert!(text.contains("no draft"), "{text}");
        let other = Draft::first(
            DocumentId::from_bytes([0x77; 16]),
            by_spelling.kind.clone(),
            by_spelling.fields.clone(),
            String::new(),
        );
        world.drafts.write(&other).unwrap();
        let text = refused(draft_for(&deps.read(), &lookup, "phone"));
        assert!(text.contains("several drafts"), "{text}");
    }

    #[test]
    fn a_name_held_by_several_documents_is_a_collision_not_a_scan() {
        let (world, _) = world();
        topic(&world, "lantern", "");
        let deps = world.deps();
        let lookup = world.lookup();
        let text = refused(draft_for(&deps.read(), &lookup, "lantern"));
        assert!(text.contains("several documents"), "{text}");
    }

    #[test]
    fn a_stored_document_without_a_draft_is_no_draft() {
        let (world, _) = world();
        let deps = world.deps();
        let lookup = world.lookup();
        let text = refused(draft_for(&deps.read(), &lookup, "lantern"));
        assert!(text.contains("no draft"), "{text}");
    }

    #[test]
    fn discarding_removes_the_draft_once() {
        let (world, _) = world();
        let deps = world.deps();
        let opened = new(&deps, &New::Topic { name: "phone" }).unwrap();
        discard(&deps, "phone").unwrap();
        assert_eq!(world.drafts.read(&opened.document).unwrap(), None);
        let text = refused(discard(&deps, "phone"));
        assert!(text.contains("no draft"), "{text}");
    }

    #[test]
    fn a_draft_that_does_not_read_is_listed_counted_and_discarded_by_id_and_by_its_document() {
        let (world, lantern) = world();
        let deps = world.deps();
        checkout(&deps, "lantern").unwrap();
        world.drafts.break_draft(&lantern);
        let phone = new(&deps, &New::Topic { name: "phone" }).unwrap();
        world.drafts.break_draft(&phone.document);
        new(&deps, &New::Topic { name: "atlas" }).unwrap();

        let rows = drafts(&deps.read()).unwrap();
        let broken: Vec<&DocumentId> = (rows.iter())
            .filter_map(|row| match row {
                DraftRow::Broken(broken) => Some(&broken.document),
                DraftRow::Open(_) => None,
            })
            .collect();
        let mut expected = vec![&lantern, &phone.document];
        expected.sort();
        assert_eq!(broken, expected);
        let opened = crate::app::context::context(&deps.read(), "/work").unwrap();
        assert_eq!(opened.draft_count, 3);
        let text = refused(checkout(&deps, "lantern"));
        assert!(text.contains("a draft of lantern is open"), "{text}");
        let Err(failure) = crate::app::save::save(&deps, "lantern") else {
            panic!("a draft that does not read must not save");
        };
        assert!(
            failure.to_string().contains("no opening fence"),
            "{failure}"
        );

        discard(&deps, "lantern").unwrap();
        discard(&deps, phone.document.short()).unwrap();
        assert_eq!(world.drafts.documents().unwrap().len(), 1);
    }

    #[test]
    fn drafts_are_listed_by_document_id_with_their_labels() {
        let (world, lantern) = world();
        let deps = world.deps();
        fact(&world, &lantern, "relay-pin", "");
        let phone = new(&deps, &New::Topic { name: "phone" }).unwrap();
        let relay = checkout(&deps, "lantern/relay-pin").unwrap();
        let nameless = Draft::first(
            DocumentId::from_bytes([0xab; 16]),
            Kind::parse("followup").unwrap(),
            Fields::new(),
            String::new(),
        );
        world.drafts.write(&nameless).unwrap();
        let rows = drafts(&deps.read()).unwrap();
        let mut expected = vec![
            (phone.document.clone(), "topic", "phone".to_owned()),
            (
                relay.document.clone(),
                "fact",
                "lantern/relay-pin".to_owned(),
            ),
            (
                nameless.document.clone(),
                "followup",
                nameless.document.short().to_owned(),
            ),
        ];
        expected.sort();
        let got: Vec<_> = rows
            .iter()
            .map(open_row)
            .map(|row| (row.document.clone(), row.kind.as_str(), row.label.clone()))
            .collect();
        assert_eq!(got, expected);
        for row in rows.iter().map(open_row) {
            assert_eq!(row.location, world.drafts.location(&row.document));
        }
    }

    #[test]
    fn a_draft_keeps_the_session_it_was_opened_in() {
        let (mut world, _) = world();
        world.host.set_session(Some(lamp_session()));
        let lantern = checkout(&world.deps(), "lantern").unwrap();
        world.host.set_session(None);
        let phone = new(&world.deps(), &New::Topic { name: "phone" }).unwrap();
        let opened_in = |opened: &DraftRef| {
            world
                .drafts
                .read(&opened.document)
                .unwrap()
                .unwrap()
                .opened_in
        };
        assert_eq!(opened_in(&lantern), Some(lamp_session()));
        assert_eq!(opened_in(&phone), None);
    }

    #[test]
    fn a_listed_draft_s_age_runs_from_its_last_touch_to_now() {
        let (world, _) = world();
        let opened = checkout(&world.deps(), "lantern").unwrap();
        let age = || open_row(&drafts(&world.deps().read()).unwrap()[0]).age_seconds;
        assert_eq!(age(), 12 * 60 + 41);
        world
            .drafts
            .touch(&opened.document, "2026-10-09T18:30:00+01:00");
        assert_eq!(age(), 0);
    }

    #[test]
    fn a_checkout_of_an_entry_shows_machine_and_topics_as_names() {
        let (world, lantern) = world();
        let driver = world.put(
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\n\
                 topics = [\"{lantern}\"]\nsummary = \"s\"\n",
                world.machine()
            ),
            "\n",
        );
        checkout(&world.deps(), "2026-10-08-lamp-driver").unwrap();
        assert_eq!(
            toml::to_string(&stored(&world, &driver).fields).unwrap(),
            "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"desk\"\n\
             topics = [\"lantern\"]\nsummary = \"s\"\n"
        );
    }

    #[test]
    fn a_checkout_keeps_a_reference_to_an_unnamed_document_as_its_id() {
        let (world, lantern) = world();
        let other = world.put(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"o\"\n"),
            "\n",
        );
        let followup = world.put(
            "followup",
            &format!(
                "created = 2026-09-04\ntopics = [\"{lantern}\"]\nabout = \"{other}\"\n\
                 summary = \"f\"\n"
            ),
            "\n",
        );
        checkout(&world.deps(), followup.as_str()).unwrap();
        assert_eq!(
            toml::to_string(&stored(&world, &followup).fields).unwrap(),
            format!("topics = [\"lantern\"]\nabout = \"{other}\"\nsummary = \"f\"\n")
        );
    }

    #[test]
    fn a_draft_is_reached_by_the_address_an_ended_document_holds() {
        let (world, lantern) = world();
        let deps = world.deps();
        world.amend(
            &lantern,
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "\n",
        );
        let opened = new(&deps, &New::Topic { name: "lantern" }).unwrap();
        let lookup = world.lookup();
        let draft = draft_for(&deps.read(), &lookup, "lantern").unwrap();
        assert_eq!(draft.document, opened.document);
        discard(&deps, "lantern").unwrap();
        assert_eq!(world.drafts.read(&opened.document).unwrap(), None);
    }

    #[test]
    fn a_former_name_of_a_live_document_is_free_for_a_new_one() {
        let (world, lantern) = world();
        let deps = world.deps();
        world.amend(
            &lantern,
            &format!("name = \"lantern\"\nformer_names = [\"lamp\"]\n{TOPIC}"),
            "\n",
        );
        new(&deps, &New::Topic { name: "lamp" }).unwrap();
    }
}
