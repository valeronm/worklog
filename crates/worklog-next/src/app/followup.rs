use toml::Value;

use crate::app::draft::named;
use crate::app::live::open;
use crate::app::lookup::Lookup;
use crate::app::rules::referencable;
use crate::app::save::stepped;
use crate::app::{Deps, DraftRef, Failure, Written, kind_named, text};
use crate::domain::draft::Draft;
use crate::domain::id::DocumentId;
use crate::domain::schema::{Content, Date, KindOf};
use crate::domain::version::Fields;

pub enum TriggerArg<'a> {
    LookAgain { on: &'a str, why: &'a str },
    Touching(&'a str),
}

pub struct NewFollowup<'a> {
    pub topics: &'a [&'a str],
    pub summary: Option<&'a str>,
    pub entry: Option<&'a str>,
    pub about: Option<&'a str>,
    pub trigger: Option<TriggerArg<'a>>,
}

pub enum Made {
    Written(Written),
    Draft(DraftRef),
}

const TRIGGER_KEYS: [&str; 3] = ["look_again", "why", "touching"];

fn trigger_fields(
    trigger: &TriggerArg,
    touching: impl Fn(&str) -> Result<String, Failure>,
) -> Result<Vec<(&'static str, Value)>, Failure> {
    match trigger {
        TriggerArg::LookAgain { on, why } => {
            let on = Date::parse(on).map_err(|error| Failure::Usage(format!("on: {error}")))?;
            let why = why.trim();
            if why.is_empty() {
                return Err(Failure::Usage("why: a look-again date says why".to_owned()));
            }
            Ok(vec![("look_again", on.value()), ("why", text(why))])
        }
        TriggerArg::Touching(address) => Ok(vec![("touching", text(touching(address)?))]),
    }
}

fn touched(lookup: &Lookup, address: &str, topics: &[DocumentId]) -> Result<String, Failure> {
    let topic = referencable(lookup, address)?;
    if !topics.contains(&topic) {
        return Err(Failure::at(address, "is not one of the followup's topics"));
    }
    Ok(topic.to_string())
}

/// Stored at once when a summary is given, otherwise opened as a draft.
pub fn new_followup(deps: &Deps, what: &NewFollowup) -> Result<Made, Failure> {
    if what.topics.is_empty() {
        return Err(Failure::Usage("a followup names a topic".to_owned()));
    }
    let lookup = Lookup::new(deps.store);
    let topics = what
        .topics
        .iter()
        .map(|topic| lookup.one(topic))
        .collect::<Result<Vec<_>, _>>()?;
    let entry = what.entry.map(|entry| lookup.one(entry)).transpose()?;
    let about = what.about.map(|about| lookup.one(about)).transpose()?;
    let shown = what.summary.is_none();
    let reference = |id: &DocumentId| {
        if shown {
            named(&lookup, id)
        } else {
            Ok(id.to_string())
        }
    };
    let mut fields = Fields::new();
    let held = topics
        .iter()
        .map(&reference)
        .collect::<Result<Vec<_>, _>>()?;
    fields.insert(
        "topics".to_owned(),
        Value::Array(held.into_iter().map(text).collect()),
    );
    if let Some(entry) = &entry {
        fields.insert("entry".to_owned(), text(reference(entry)?));
    }
    if let Some(about) = &about {
        fields.insert("about".to_owned(), text(reference(about)?));
    }
    if let Some(trigger) = &what.trigger {
        let touching = |address: &str| {
            if shown {
                named(&lookup, &lookup.one(address)?)
            } else {
                touched(&lookup, address, &topics)
            }
        };
        for (key, value) in trigger_fields(trigger, touching)? {
            fields.insert(key.to_owned(), value);
        }
    }
    fields.insert("summary".to_owned(), text(what.summary.unwrap_or_default()));
    let kind = kind_named(KindOf::Followup);
    let id = deps.ids.mint()?;
    if what.summary.is_none() {
        let draft = Draft::first(id, kind, fields, "\n".to_owned());
        let location = deps.drafts.write(&draft)?;
        return Ok(Made::Draft(DraftRef {
            document: draft.document,
            location,
        }));
    }
    let admitted = stepped(deps, &lookup, &id, &kind, &fields, "\n", "new")?;
    Ok(Made::Written(admitted.store(deps)?))
}

/// `None` clears the trigger.
pub fn set_trigger(
    deps: &Deps,
    address: &str,
    trigger: Option<&TriggerArg>,
) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let target = open(deps, &lookup, address)?;
    let Content::Followup(followup) = &target.record.content else {
        return Err(Failure::at(&target.label, "is not a followup"));
    };
    target.unended()?;
    let mut fields = target.shown();
    for key in TRIGGER_KEYS {
        fields.remove(key);
    }
    if let Some(trigger) = trigger {
        let touching = |address: &str| touched(&lookup, address, &followup.topics);
        for (key, value) in trigger_fields(trigger, touching)? {
            fields.insert(key.to_owned(), value);
        }
    }
    target.store(deps, &lookup, &fields, "trigger")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::heads::read_or_skip;
    use crate::app::save::save;
    use crate::app::testing::{World, head, refused, world_with_atlas};
    use crate::domain::id::DocumentId;
    use crate::domain::ports::Drafts;
    use crate::domain::schema::{Content, Trigger};

    fn followup(world: &World, id: &DocumentId) -> crate::domain::schema::Followup {
        let head = head(world, id);
        match read_or_skip(&head).unwrap().content {
            Content::Followup(followup) => followup,
            other => panic!("expected a followup, got {other:?}"),
        }
    }

    fn written(made: Result<Made, Failure>) -> Written {
        match made {
            Ok(Made::Written(written)) => written,
            other => panic!("expected a stored followup, got {:?}", other.map(|_| ())),
        }
    }

    fn plain<'a>(topics: &'a [&'a str], summary: &'a str) -> NewFollowup<'a> {
        NewFollowup {
            topics,
            summary: Some(summary),
            entry: None,
            about: None,
            trigger: None,
        }
    }

    #[test]
    fn a_followup_with_a_summary_is_stored_at_once() {
        let (world, lantern, atlas) = world_with_atlas();
        let made = written(new_followup(
            &world.deps(),
            &plain(&["lantern", "atlas"], "Check the driver"),
        ));
        let stored = followup(&world, &made.document);
        assert_eq!(stored.created.to_string(), "2026-10-09");
        assert_eq!(stored.topics, [lantern, atlas]);
        assert_eq!(stored.summary, "Check the driver");
        assert_eq!(head(&world, &made.document).envelope.change, "new");
        assert_eq!(head(&world, &made.document).body, "\n");
    }

    #[test]
    fn a_followup_keeps_its_entry_about_and_trigger() {
        let (world, lantern, _) = world_with_atlas();
        let entry = world.put(
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\n\
                 topics = [\"{lantern}\"]\nsummary = \"s\"\n",
                world.machine()
            ),
            "\n",
        );
        let relay = world.put(
            "fact",
            &format!(
                "name = \"relay\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n"
            ),
            "\n",
        );
        let deps = world.deps();
        let mut what = plain(&["lantern"], "Check");
        what.entry = Some("2026-10-08-lamp-driver");
        what.about = Some("lantern/relay");
        what.trigger = Some(TriggerArg::LookAgain {
            on: "2026-11-01",
            why: "after the shipment",
        });
        let stored = followup(&world, &written(new_followup(&deps, &what)).document);
        assert_eq!(stored.entry, Some(entry));
        assert_eq!(stored.about, Some(relay));
        let Some(Trigger::LookAgain { on, why }) = stored.trigger else {
            panic!("a look-again trigger");
        };
        assert_eq!(
            (on.to_string().as_str(), why.as_str()),
            ("2026-11-01", "after the shipment")
        );

        what.entry = None;
        what.about = None;
        what.trigger = Some(TriggerArg::Touching("lantern"));
        let stored = followup(&world, &written(new_followup(&deps, &what)).document);
        assert_eq!(stored.trigger, Some(Trigger::Touching(lantern)));
    }

    #[test]
    fn a_followup_is_refused_for_what_it_cannot_name() {
        let (world, lantern, _) = world_with_atlas();
        world.put(
            "fact",
            &format!(
                "name = \"relay\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n"
            ),
            "\n",
        );
        let deps = world.deps();
        assert!(matches!(
            new_followup(&deps, &plain(&[], "Check")),
            Err(Failure::Usage(_))
        ));
        let text = refused(new_followup(&deps, &plain(&["nowhere"], "Check")));
        assert!(text.contains("names no document"), "{text}");
        let mut what = plain(&["lantern"], "Check");
        what.trigger = Some(TriggerArg::Touching("atlas"));
        let text = refused(new_followup(&deps, &what));
        assert!(text.contains("not one of the followup's topics"), "{text}");
        what.trigger = None;
        what.entry = Some("lantern/relay");
        let text = refused(new_followup(&deps, &what));
        assert_eq!(text, "new followup: entry: lantern/relay is not an entry");
    }

    #[test]
    fn a_followup_without_a_summary_opens_a_draft_that_saves_once_filled() {
        let (world, lantern, _) = world_with_atlas();
        let deps = world.deps();
        let mut what = plain(&["lantern"], "");
        what.summary = None;
        what.trigger = Some(TriggerArg::Touching("lantern"));
        let Ok(Made::Draft(opened)) = new_followup(&deps, &what) else {
            panic!("a draft");
        };
        let mut draft = world.drafts.read(&opened.document).unwrap().unwrap();
        assert_eq!(draft.fields["topics"].to_string(), "[\"lantern\"]");
        assert_eq!(draft.fields["touching"].as_str(), Some("lantern"));
        assert_eq!(draft.fields["summary"].as_str(), Some(""));
        draft.fields.insert("summary".to_owned(), "Check".into());
        world.drafts.write(&draft).unwrap();
        let saved = save(&deps, opened.document.as_str()).unwrap();
        assert_eq!(followup(&world, &saved.document).topics, [lantern]);
    }

    #[test]
    fn a_draft_opens_whatever_it_touches_and_the_save_refuses() {
        let (world, _, _) = world_with_atlas();
        let deps = world.deps();
        let filled = |topics: &[&str]| {
            let mut what = plain(topics, "");
            what.summary = None;
            what.trigger = Some(TriggerArg::Touching("atlas"));
            let Ok(Made::Draft(opened)) = new_followup(&deps, &what) else {
                panic!("a draft");
            };
            let mut draft = world.drafts.read(&opened.document).unwrap().unwrap();
            assert_eq!(draft.fields["touching"].as_str(), Some("atlas"));
            draft.fields.insert("summary".to_owned(), "Check".into());
            world.drafts.write(&draft).unwrap();
            opened.document
        };

        let apart = filled(&["lantern"]);
        let text = refused(save(&deps, apart.as_str()));
        assert!(text.contains("is not among `topics`"), "{text}");

        crate::app::amend::end(&deps, "atlas", "retired", "n", None).unwrap();
        let ended = filled(&["atlas"]);
        let text = refused(save(&deps, ended.as_str()));
        assert!(text.contains("topics: atlas is ended"), "{text}");
        assert!(world.drafts.read(&ended).unwrap().is_some());
    }

    #[test]
    fn a_trigger_is_set_replaced_and_cleared() {
        let (world, lantern, _) = world_with_atlas();
        let deps = world.deps();
        let id = written(new_followup(&deps, &plain(&["lantern", "atlas"], "Check"))).document;
        let at = id.as_str();
        set_trigger(
            &deps,
            at,
            Some(&TriggerArg::LookAgain {
                on: "2026-11-01",
                why: "later",
            }),
        )
        .unwrap();
        assert!(matches!(
            followup(&world, &id).trigger,
            Some(Trigger::LookAgain { .. })
        ));
        assert_eq!(head(&world, &id).envelope.change, "trigger");
        set_trigger(&deps, at, Some(&TriggerArg::Touching("lantern"))).unwrap();
        assert_eq!(
            followup(&world, &id).trigger,
            Some(Trigger::Touching(lantern))
        );
        set_trigger(&deps, at, None).unwrap();
        assert_eq!(followup(&world, &id).trigger, None);
        assert!(matches!(
            set_trigger(
                &deps,
                at,
                Some(&TriggerArg::LookAgain {
                    on: "soon",
                    why: "x"
                })
            ),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn a_trigger_is_refused_on_a_fact_and_on_an_ended_followup() {
        let (world, lantern, _) = world_with_atlas();
        let deps = world.deps();
        world.put(
            "fact",
            &format!(
                "name = \"relay\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n"
            ),
            "\n",
        );
        let text = refused(set_trigger(&deps, "lantern/relay", None));
        assert!(text.contains("is not a followup"), "{text}");
        let id = written(new_followup(&deps, &plain(&["lantern"], "Check"))).document;
        crate::app::amend::end(&deps, id.as_str(), "done", "", None).unwrap();
        let text = refused(set_trigger(
            &deps,
            id.as_str(),
            Some(&TriggerArg::Touching("lantern")),
        ));
        assert!(text.contains("is ended; reopen it first"), "{text}");
    }

    #[test]
    fn a_touching_trigger_must_be_a_topic_of_the_followup() {
        let (world, _, _) = world_with_atlas();
        let deps = world.deps();
        let id = written(new_followup(&deps, &plain(&["lantern"], "Check"))).document;
        let text = refused(set_trigger(
            &deps,
            id.as_str(),
            Some(&TriggerArg::Touching("atlas")),
        ));
        assert!(text.contains("not one of the followup's topics"), "{text}");
    }

    #[test]
    fn a_draft_open_blocks_a_trigger() {
        let (world, _, _) = world_with_atlas();
        let deps = world.deps();
        let id = written(new_followup(&deps, &plain(&["lantern"], "Check"))).document;
        crate::app::draft::checkout(&deps, id.as_str()).unwrap();
        let text = refused(set_trigger(&deps, id.as_str(), None));
        assert!(text.contains("is open"), "{text}");
    }

    #[test]
    fn a_followup_may_be_about_an_ended_entry_but_not_name_an_ended_topic() {
        let (world, lantern, _) = world_with_atlas();
        let deps = world.deps();
        let entry = world.put(
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\n\
                 topics = [\"{lantern}\"]\nsummary = \"s\"\nended = \"removed\"\n\
                 ended_on = 2026-10-09\nnote = \"n\"\n",
                world.machine()
            ),
            "\n",
        );
        let mut what = plain(&["lantern"], "Check");
        what.entry = Some("2026-10-08-lamp-driver");
        let stored = followup(&world, &written(new_followup(&deps, &what)).document);
        assert_eq!(stored.entry, Some(entry));
        crate::app::amend::end(&deps, "atlas", "retired", "n", None).unwrap();
        let text = refused(new_followup(&deps, &plain(&["atlas"], "Check")));
        assert!(text.contains("is ended"), "{text}");
    }

    #[test]
    fn a_followup_stored_at_once_is_refused_as_a_new_followup() {
        let (world, _, _) = world_with_atlas();
        let text = refused(new_followup(&world.deps(), &plain(&["lantern"], "")));
        assert!(text.contains("new followup: `summary` is empty"), "{text}");
    }
}
