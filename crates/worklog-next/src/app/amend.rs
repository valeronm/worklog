use crate::app::live::open;
use crate::app::lookup::Lookup;
use crate::app::save::admit;
use crate::app::{Deps, Failure, Written, text};
use crate::domain::schema::{Content, Ending, Name, Reason};

pub fn rename(deps: &Deps, address: &str, name: &str) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let target = open(deps, &lookup, address)?;
    if !matches!(
        target.record.content,
        Content::Topic(_) | Content::Fact(_) | Content::Entry(_)
    ) {
        return Err(Failure::at(&target.label, "has no name"));
    }
    let name = Name::parse(name).map_err(|error| Failure::Usage(error.to_string()))?;
    target.unended()?;
    let mut fields = target.shown();
    fields.insert("name".to_owned(), text(name.as_str()));
    target.store(deps, &lookup, &fields, "rename")
}

pub fn verify(deps: &Deps, address: &str) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let target = open(deps, &lookup, address)?;
    let Content::Fact(fact) = &target.record.content else {
        return Err(Failure::at(&target.label, "is not a fact"));
    };
    target.unended()?;
    let today = deps.today()?;
    if fact.confirmed == today {
        return Err(Failure::at(&target.label, "is already confirmed today"));
    }
    let mut fields = target.shown();
    fields.insert("confirmed".to_owned(), today.value());
    target.store(deps, &lookup, &fields, "verify")
}

pub fn end(
    deps: &Deps,
    address: &str,
    reason: &str,
    note: &str,
    by: Option<&str>,
) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let target = open(deps, &lookup, address)?;
    let by = by.map(|by| lookup.one(by)).transpose()?;
    if by.as_ref() == Some(&target.id) {
        return Err(Failure::at(&target.label, "cannot be ended by itself"));
    }
    let ending = Ending {
        reason: Reason::parse(reason),
        on: deps.today()?,
        by,
        note: note.to_owned(),
    };
    let ended = target
        .record
        .clone()
        .end(ending)
        .map_err(|error| Failure::at(&target.label, error))?;
    admit(deps, &lookup, &target.id, &ended, &target.head.body, reason)
}

pub fn reopen(deps: &Deps, address: &str, why: &str) -> Result<Written, Failure> {
    let why = why.trim();
    if why.is_empty() {
        return Err(Failure::Usage("a reopening says why".to_owned()));
    }
    let lookup = Lookup::new(deps.store);
    let target = open(deps, &lookup, address)?;
    if let Content::Claim(_) = target.record.content {
        return Err(Failure::at(
            &target.label,
            "a claim is never reopened; claim again",
        ));
    }
    let reopened = target
        .record
        .clone()
        .reopen()
        .map_err(|error| Failure::at(&target.label, error))?;
    admit(
        deps,
        &lookup,
        &target.id,
        &reopened,
        &target.head.body,
        &format!("reopen: {why}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{
        entry, fact, fact_fields, fork, found, head, record, refused, topic, world,
    };
    use crate::app::{claim, draft};
    use crate::domain::draft::Draft;
    use crate::domain::ports::Drafts;
    use crate::domain::schema::address::Found;
    use crate::domain::schema::{Content, Name, Reason};

    #[test]
    fn a_renamed_fact_is_found_by_both_names() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let written = rename(&world.deps(), "lantern/relay", "switch").unwrap();
        assert_eq!(written.document, relay);
        assert_eq!(found(&world, "lantern/switch"), Found::One(relay.clone()));
        assert_eq!(found(&world, "lantern/relay"), Found::One(relay.clone()));
        let Content::Fact(fact) = record(&world, &relay).content else {
            panic!("a fact");
        };
        assert_eq!(fact.name.as_str(), "switch");
        assert_eq!(
            fact.former_names
                .iter()
                .map(Name::as_str)
                .collect::<Vec<_>>(),
            ["relay"]
        );
        assert_eq!(head(&world, &relay).envelope.change, "rename");
    }

    #[test]
    fn a_rename_is_refused_where_it_cannot_apply() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        fact(&world, &lantern, "bulb", "");
        let followup = world.put(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n"),
            "\n",
        );
        let deps = world.deps();
        let text = refused(rename(&deps, followup.as_str(), "switch"));
        assert!(text.contains("has no name"), "{text}");
        let text = refused(rename(&deps, "lantern/relay", "bulb"));
        assert!(
            text.contains("lantern/relay: name: bulb is taken by lantern/bulb"),
            "{text}"
        );
        let text = refused(rename(&deps, "lantern/relay", "relay"));
        assert!(
            text.contains("lantern/relay: nothing would change"),
            "{text}"
        );
        assert!(matches!(
            rename(&deps, "lantern/relay", "Not A Name"),
            Err(Failure::Usage(_))
        ));
        draft::checkout(&deps, "lantern/relay").unwrap();
        let text = refused(rename(&deps, "lantern/relay", "switch"));
        assert!(text.contains("a draft of lantern/relay is open"), "{text}");
        world.drafts.delete(&relay).unwrap();
        world.amend(
            &relay,
            &format!(
                "{}ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n",
                fact_fields(&lantern, "relay", "s", "")
            ),
            "The relay.\n",
        );
        let text = refused(rename(&deps, "lantern/relay", "switch"));
        assert!(text.contains("is ended; reopen it first"), "{text}");
    }

    #[test]
    fn ending_a_fact_records_who_when_and_why() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let by = entry(&world, "2026-10-08", "lamp-driver", &[&lantern]);
        end(
            &world.deps(),
            "lantern/relay",
            "false",
            "the board has none",
            Some("2026-10-08-lamp-driver"),
        )
        .unwrap();
        let ending = record(&world, &relay).ending.unwrap();
        assert_eq!(ending.reason, Reason::False);
        assert_eq!(ending.on.to_string(), "2026-10-09");
        assert_eq!(ending.by, Some(by));
        assert_eq!(ending.note, "the board has none");
        assert_eq!(head(&world, &relay).envelope.change, "false");
    }

    #[test]
    fn an_ending_is_refused_for_a_reason_of_another_kind_a_blank_note_or_a_second_ending() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        let text = refused(end(&deps, "lantern/relay", "built", "n", None));
        assert!(text.contains("does not end as built"), "{text}");
        let text = refused(end(&deps, "lantern/relay", "false", "  ", None));
        assert!(text.contains("says why in its note"), "{text}");
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        let text = refused(end(&deps, "lantern/relay", "moved", "n", None));
        assert!(text.contains("is ended"), "{text}");
    }

    #[test]
    fn a_topic_with_live_dependents_does_not_end() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay", "");
        entry(&world, "2026-10-08", "lamp-driver", &[&lantern]);
        let deps = world.deps();
        let text = refused(end(&deps, "lantern", "retired", "n", None));
        assert!(text.ends_with("still has 1 fact"), "{text}");
        assert!(!text.contains("topics"), "{text}");
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        end(&deps, "lantern", "retired", "n", None).unwrap();
        assert!(record(&world, &lantern).ending.is_some());
    }

    #[test]
    fn a_topic_another_topic_is_part_of_does_not_end() {
        let (world, lantern) = world();
        topic(&world, "atlas", &format!("part_of = [\"{lantern}\"]\n"));
        let text = refused(end(&world.deps(), "lantern", "retired", "n", None));
        assert!(text.ends_with("still has 1 topic"), "{text}");
    }

    #[test]
    fn a_reopened_document_has_no_ending_and_says_why() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        let text = refused(reopen(&deps, "lantern/relay", "still true"));
        assert!(text.contains("is not ended"), "{text}");
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        assert!(matches!(
            reopen(&deps, "lantern/relay", " "),
            Err(Failure::Usage(_))
        ));
        reopen(&deps, "lantern/relay", "still true on the desk").unwrap();
        assert!(record(&world, &relay).ending.is_none());
        assert_eq!(
            head(&world, &relay).envelope.change,
            "reopen: still true on the desk"
        );
    }

    #[test]
    fn a_name_taken_since_blocks_a_reopening() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        fact(&world, &lantern, "relay", "");
        let text = refused(reopen(&deps, relay.as_str(), "back"));
        assert!(
            text.contains("lantern/relay: name: relay is taken by lantern/relay"),
            "{text}"
        );
    }

    #[test]
    fn verifying_confirms_a_fact_once_a_day() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        entry(&world, "2026-10-08", "lamp-driver", &[&lantern]);
        let deps = world.deps();
        verify(&deps, "lantern/relay").unwrap();
        let Content::Fact(fact) = record(&world, &relay).content else {
            panic!("a fact");
        };
        assert_eq!(fact.confirmed.to_string(), "2026-10-09");
        assert_eq!(head(&world, &relay).envelope.change, "verify");
        let text = refused(verify(&deps, "lantern/relay"));
        assert!(text.contains("is already confirmed today"), "{text}");
        let text = refused(verify(&deps, "2026-10-08-lamp-driver"));
        assert!(text.contains("is not a fact"), "{text}");
    }

    #[test]
    fn a_reopened_fact_needs_its_topic_live() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        end(&deps, "lantern", "retired", "n", None).unwrap();
        let text = refused(reopen(&deps, relay.as_str(), "back"));
        assert!(
            text.contains("lantern/relay: topic: lantern is ended"),
            "{text}"
        );
        reopen(&deps, lantern.as_str(), "back").unwrap();
        reopen(&deps, relay.as_str(), "back").unwrap();
        assert!(record(&world, &relay).ending.is_none());
    }

    #[test]
    fn a_name_a_forked_document_holds_blocks_a_reopening() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        let other = fact(&world, &lantern, "relay", "");
        fork(&world, &other, &fact_fields(&lantern, "relay", "s", ""));
        let text = refused(reopen(&deps, relay.as_str(), "back"));
        assert!(
            text.contains("lantern/relay: name: relay is taken by lantern/relay"),
            "{text}"
        );
    }

    #[test]
    fn a_topic_with_every_kind_of_dependent_names_each_count() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay", "");
        world.put(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n"),
            "\n",
        );
        world.put(
            "claim",
            &format!("machine = \"{}\"\ntopic = \"{lantern}\"\n", world.machine()),
            "\n",
        );
        topic(&world, "atlas", &format!("uses = [\"{lantern}\"]\n"));
        let text = refused(end(&world.deps(), "lantern", "retired", "n", None));
        assert!(
            text.contains("still has 1 fact, 1 followup, 1 claim, 1 topic"),
            "{text}"
        );
    }

    #[test]
    fn a_topic_holding_another_twice_counts_once() {
        let (world, lantern) = world();
        topic(
            &world,
            "atlas",
            &format!("part_of = [\"{lantern}\"]\nuses = [\"{lantern}\"]\n"),
        );
        let text = refused(end(&world.deps(), "lantern", "retired", "n", None));
        assert!(text.ends_with("still has 1 topic"), "{text}");
    }

    #[test]
    fn verifying_an_ended_fact_is_refused() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        let text = refused(verify(&deps, "lantern/relay"));
        assert!(text.contains("is ended; reopen it first"), "{text}");
    }

    #[test]
    fn a_draft_open_blocks_an_ending_and_a_reopening() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let deps = world.deps();
        draft::checkout(&deps, "lantern/relay").unwrap();
        let text = refused(end(&deps, "lantern/relay", "false", "n", None));
        assert!(text.contains("a draft of lantern/relay is open"), "{text}");
        world.drafts.delete(&relay).unwrap();
        end(&deps, "lantern/relay", "false", "n", None).unwrap();
        world
            .drafts
            .write(&Draft::of(&head(&world, &relay)))
            .unwrap();
        let text = refused(reopen(&deps, relay.as_str(), "back"));
        assert!(text.contains("is open"), "{text}");
    }

    #[test]
    fn a_forked_fact_with_a_live_head_keeps_its_topic_from_ending() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        fork(&world, &relay, &fact_fields(&lantern, "relay-pin", "s", ""));
        let text = refused(end(&world.deps(), "lantern", "retired", "n", None));
        assert!(text.ends_with("lantern: still has 1 fact"), "{text}");
    }

    #[test]
    fn a_forked_fact_whose_heads_are_all_ended_lets_its_topic_end() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        fork(
            &world,
            &relay,
            &format!(
                "{}ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n",
                fact_fields(&lantern, "relay-pin", "s", "")
            ),
        );
        end(&world.deps(), "lantern", "retired", "n", None).unwrap();
    }

    #[test]
    fn a_machine_topic_with_live_claims_does_not_end() {
        let (world, lantern) = world();
        let desk = world.machine();
        world.put(
            "claim",
            &format!("machine = \"{desk}\"\ntopic = \"{lantern}\"\ndirectory = \"~/lantern\"\n"),
            "\n",
        );
        world.put(
            "claim",
            &format!("machine = \"{desk}\"\ntopic = \"{desk}\"\n"),
            "\n",
        );
        let text = refused(end(&world.deps(), "desk", "retired", "n", None));
        assert!(text.ends_with("desk: still has 2 claims"), "{text}");
    }

    #[test]
    fn a_claim_is_never_reopened() {
        let (world, _) = world();
        let deps = world.deps();
        let placed = claim::claim(&deps, "lantern", Some("~/lantern")).unwrap();
        claim::unclaim(&deps, "lantern", Some("~/lantern")).unwrap();
        claim::claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let text = refused(reopen(&deps, placed.document.as_str(), "again"));
        assert!(
            text.contains(&format!(
                "{}: a claim is never reopened; claim again",
                placed.document.short()
            )),
            "{text}"
        );
    }

    #[test]
    fn a_document_is_not_ended_by_itself() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let text = refused(end(
            &world.deps(),
            "lantern/relay",
            "superseded",
            "n",
            Some(relay.as_str()),
        ));
        assert!(
            text.contains("lantern/relay: cannot be ended by itself"),
            "{text}"
        );
        assert!(record(&world, &relay).ending.is_none());
    }
}
