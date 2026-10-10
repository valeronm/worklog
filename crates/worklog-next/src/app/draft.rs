use toml::Value;

use crate::app::lookup::{Lookup, ended, unreadable};
use crate::app::save::name_is_free;
use crate::app::{Deps, DraftRef, Failed, Failure, kind_named, text};
use crate::domain::draft::Draft;
use crate::domain::id::{DocumentId, is_id_prefix};
use crate::domain::schema::address::{Address, Found};
use crate::domain::schema::{Content, Date, Entry, Fact, KindOf, Name, Topic, step, translate};
use crate::domain::version::Fields;

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

fn usage(error: impl std::fmt::Display) -> Failure {
    Failure::Usage(error.to_string())
}

pub(super) fn named(lookup: &Lookup, id: &DocumentId) -> Result<String, Failure> {
    Ok(lookup.address(id)?.unwrap_or_else(|| id.to_string()))
}

/// Refuses a name taken in its scope and an address that already has a draft; a malformed
/// name or date is a usage failure.
pub fn new(deps: &Deps, what: &New) -> Result<DraftRef, Failure> {
    let lookup = Lookup::new(deps.store);
    let today = deps.today()?;
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
            let topic_id = lookup.referencable(topic.as_str(), Some(KindOf::Topic))?;
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
            let machine = deps.machine()?;
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
    for open in deps.drafts.list()? {
        if spelled(&open).as_deref() == Some(address.as_str()) {
            return Err(draft_open(&address));
        }
    }
    let draft = Draft::first(id, kind_named(content.kind()), fields, body.to_owned());
    let location = deps.drafts.write(&draft)?;
    Ok(DraftRef {
        document: draft.document,
        location,
    })
}

fn draft_open(what: &str) -> Failure {
    Failure::Refused(format!("a draft of {what} is open"))
}

pub(super) fn refuse_open(deps: &Deps, id: &DocumentId, label: &str) -> Result<(), Failure> {
    if deps.drafts.read(id)?.is_some() {
        return Err(draft_open(label));
    }
    Ok(())
}

/// Refuses a claim, an ended, forked or unreadable document, and one that has a draft open.
pub fn checkout(deps: &Deps, address: &str) -> Result<DraftRef, Failure> {
    let lookup = Lookup::new(deps.store);
    let id = lookup.one(address)?;
    let label = lookup.label(&id)?;
    refuse_open(deps, &id, &label)?;
    let document = lookup.document(&id)?;
    if !document.unreadable().is_empty() {
        return Err(unreadable(&label));
    }
    let (head, record) = lookup.writable(&id, id.short())?;
    let kind = KindOf::of(&head.envelope.kind)?;
    if kind == KindOf::Claim {
        return Err(Failure::at(&label, "a claim is never edited"));
    }
    if record.ending.is_some() {
        return Err(ended(&label));
    }
    let mut draft = Draft::of(&head);
    draft.fields = names(&lookup, &step::shown(&head.fields), kind)?;
    let location = deps.drafts.write(&draft)?;
    Ok(DraftRef {
        document: id,
        location,
    })
}

pub(super) fn names(lookup: &Lookup, fields: &Fields, kind: KindOf) -> Result<Fields, Failure> {
    let failed = Failed::default();
    let shown = translate::to_names(fields, kind, |id| {
        lookup.address(id).unwrap_or_else(|failure| {
            failed.keep(failure);
            None
        })
    });
    failed.done()?;
    Ok(shown)
}

pub fn discard(deps: &Deps, address: &str) -> Result<(), Failure> {
    let lookup = Lookup::new(deps.store);
    let draft = draft_for(deps, &lookup, address)?;
    deps.drafts.delete(&draft.document)?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftRow {
    pub document: DocumentId,
    pub kind: String,
    pub label: String,
    pub location: String,
}

pub fn drafts(deps: &Deps) -> Result<Vec<DraftRow>, Failure> {
    let mut open = deps.drafts.list()?;
    open.sort_by(|a, b| a.document.cmp(&b.document));
    Ok(open
        .iter()
        .map(|draft| DraftRow {
            document: draft.document.clone(),
            kind: draft.kind.to_string(),
            label: spelled(draft).unwrap_or_else(|| draft.document.short().to_owned()),
            location: deps.drafts.location(&draft.document),
        })
        .collect())
}

/// The draft of the stored document the address finds, else the one draft whose document's id
/// starts with the address or whose fields spell it; refuses none and several.
pub(super) fn draft_for(deps: &Deps, lookup: &Lookup, address: &str) -> Result<Draft, Failure> {
    match lookup.find(address) {
        Ok(Found::One(id)) => {
            if let Some(draft) = deps.drafts.read(&id)? {
                return Ok(draft);
            }
        }
        Ok(Found::Collision(_)) => {
            lookup.one(address)?;
        }
        Ok(Found::None) | Err(Failure::Usage(_)) => {}
        Err(other) => return Err(other),
    }
    let mut matching: Vec<Draft> = deps
        .drafts
        .list()?
        .into_iter()
        .filter(|draft| {
            (is_id_prefix(address) && draft.document.as_str().starts_with(address))
                || spelled(draft).as_deref() == Some(address)
        })
        .collect();
    match matching.len() {
        0 => Err(Failure::at(address, "no draft")),
        1 => Ok(matching.remove(0)),
        _ => Err(Failure::at(address, "several drafts")),
    }
}

/// None for a kind without a name and for a draft lacking the fields that spell its address.
pub(super) fn spelled(draft: &Draft) -> Option<String> {
    let field = |key: &str| draft.fields.get(key);
    let name = field("name")?.as_str()?;
    match draft.kind.as_str() {
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
    use crate::app::testing::{ENDED, TOPIC, World, fact, refused, topic, world};
    use crate::domain::ports::{Drafts, Store};
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
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
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
        let head = world.store.document(&relay).unwrap().heads()[0].id.clone();
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
        fact(
            &world,
            &lantern,
            "old-pin",
            "ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n",
        );
        let text = refused(checkout(&world.deps(), "lantern/old-pin"));
        assert!(text.contains("is ended; reopen it first"), "{text}");

        let phone = topic(&world, "phone", "");
        let root = world.store.document(&phone).unwrap().heads()[0].clone();
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
            &format!(
                "machine = \"{}\"\ntopic = \"{lantern}\"\n",
                world.host.0.clone().unwrap()
            ),
            "\n",
        );
        let text = refused(checkout(&world.deps(), claim.as_str()));
        assert!(text.contains("a claim is never edited"), "{text}");

        let atlas = topic(&world, "atlas", "");
        let planted = crate::domain::testing::first(&atlas, "topic", "name = \"x\"", "\n").id;
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
        let lookup = Lookup::new(&world.store);
        let relay = fact(&world, &lantern, "relay-pin", "");
        checkout(&deps, "lantern/relay-pin").unwrap();
        let fresh = new(&deps, &New::Topic { name: "phone" }).unwrap();

        let by_address = draft_for(&deps, &lookup, "lantern/relay-pin").unwrap();
        assert_eq!(by_address.document, relay);
        let remote = Draft::first(
            DocumentId::from_bytes([0xab; 16]),
            Kind::parse("followup").unwrap(),
            Fields::new(),
            String::new(),
        );
        world.drafts.write(&remote).unwrap();
        let by_id = draft_for(&deps, &lookup, remote.document.short()).unwrap();
        assert_eq!(by_id.document, remote.document);
        let by_spelling = draft_for(&deps, &lookup, "phone").unwrap();
        assert_eq!(by_spelling.document, fresh.document);

        let text = refused(draft_for(&deps, &lookup, "unknown"));
        assert!(text.contains("no draft"), "{text}");
        let other = Draft::first(
            DocumentId::from_bytes([0x77; 16]),
            by_spelling.kind.clone(),
            by_spelling.fields.clone(),
            String::new(),
        );
        world.drafts.write(&other).unwrap();
        let text = refused(draft_for(&deps, &lookup, "phone"));
        assert!(text.contains("several drafts"), "{text}");
    }

    #[test]
    fn a_name_held_by_several_documents_is_a_collision_not_a_scan() {
        let (world, _) = world();
        topic(&world, "lantern", "");
        let deps = world.deps();
        let lookup = Lookup::new(&world.store);
        let text = refused(draft_for(&deps, &lookup, "lantern"));
        assert!(text.contains("several documents"), "{text}");
    }

    #[test]
    fn a_stored_document_without_a_draft_is_no_draft() {
        let (world, _) = world();
        let deps = world.deps();
        let lookup = Lookup::new(&world.store);
        let text = refused(draft_for(&deps, &lookup, "lantern"));
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
        let rows = drafts(&deps).unwrap();
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
            .map(|row| (row.document.clone(), row.kind.as_str(), row.label.clone()))
            .collect();
        assert_eq!(got, expected);
        for row in &rows {
            assert_eq!(row.location, world.drafts.location(&row.document));
        }
    }

    #[test]
    fn a_checkout_of_an_entry_shows_machine_and_topics_as_names() {
        let (world, lantern) = world();
        let driver = world.put(
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\n\
                 topics = [\"{lantern}\"]\nsummary = \"s\"\n",
                world.host.0.clone().unwrap()
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
        let lookup = Lookup::new(&world.store);
        let draft = draft_for(&deps, &lookup, "lantern").unwrap();
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
