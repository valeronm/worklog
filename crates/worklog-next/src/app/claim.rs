use crate::app::lookup::Lookup;
use crate::app::rules::claims_of;
use crate::app::save::admit;
use crate::app::{Deps, Failure, Written, kind_named, text};
use crate::domain::id::DocumentId;
use crate::domain::schema::{Directory, Ending, KindOf, Reason, Record};
use crate::domain::version::Fields;

/// The host's home directory is stored as `~` and a directory under it as `~/rest`; a
/// directory no claim could carry is a usage failure. Refuses a claim this machine already
/// holds for the same topic and directory.
pub fn claim(deps: &Deps, topic: &str, directory: Option<&str>) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let topic_id = lookup.one(topic)?;
    let machine = deps.machine()?;
    let directory = directory.map(|given| deps.directory(given)).transpose()?;
    let directory = directory.as_ref();
    let place = place(topic, directory);
    let mut fields = Fields::new();
    fields.insert("machine".to_owned(), text(machine.as_str()));
    fields.insert("topic".to_owned(), text(topic_id.as_str()));
    if let Some(directory) = directory {
        fields.insert("directory".to_owned(), text(directory.as_str()));
    }
    let record = Record::read(&kind_named(KindOf::Claim), &fields)
        .map_err(|error| Failure::at(&place, error))?;
    if held(&lookup, &machine, &topic_id, directory)?.is_some() {
        return Err(Failure::at(&place, "is already claimed"));
    }
    admit(deps, &lookup, &deps.ids.mint()?, &record, "\n", "claim")
}

/// Ends the claim this machine holds for the topic and the directory, the directory read as
/// `claim` stores it and a usage failure where `claim` gives one; refuses when there is none.
pub fn unclaim(deps: &Deps, topic: &str, directory: Option<&str>) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let topic_id = lookup.one(topic)?;
    let machine = deps.machine()?;
    let directory = directory.map(|given| deps.directory(given)).transpose()?;
    let directory = directory.as_ref();
    let place = place(topic, directory);
    let Some(id) = held(&lookup, &machine, &topic_id, directory)? else {
        return Err(Failure::at(&place, "is not claimed"));
    };
    let (head, record) = lookup.only_head(&id, id.short())?;
    let ending = Ending {
        reason: Reason::Removed,
        on: deps.today()?,
        by: None,
        note: "unclaimed".to_owned(),
    };
    let ended = record
        .end(ending)
        .map_err(|error| Failure::at(&place, error))?;
    admit(deps, &lookup, &id, &ended, &head.body, "unclaim")
}

fn place(topic: &str, directory: Option<&Directory>) -> String {
    match directory {
        Some(directory) => format!("{topic} at {directory}"),
        None => topic.to_owned(),
    }
}

fn held(
    lookup: &Lookup,
    machine: &DocumentId,
    topic: &DocumentId,
    directory: Option<&Directory>,
) -> Result<Option<DocumentId>, Failure> {
    Ok(claims_of(lookup, machine)?
        .into_iter()
        .find(|(_, claim)| claim.topic == *topic && claim.directory.as_ref() == directory)
        .map(|(document, _)| document.id().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::heads::read_or_skip;
    use crate::app::testing::{ENDED, TOPIC, World, claim_fields, refused, topic};
    use crate::domain::ports::Store;
    use crate::domain::schema::Content;
    use crate::domain::version::{Kind, Version};

    fn world() -> World {
        let world = World::new();
        topic(&world, "desk", "");
        topic(&world, "lantern", "");
        world
    }

    fn stored(world: &World, written: &Written) -> (Version, Record) {
        let document = world.store.document(&written.document).unwrap();
        let head = document.heads()[0].clone();
        let record = read_or_skip(&head).unwrap();
        (head, record)
    }

    #[test]
    fn a_claim_places_the_topic_on_this_machine() {
        let world = world();
        let lantern = world.store.of_kind(&Kind::parse("topic").unwrap()).unwrap();
        let written = claim(&world.deps(), "lantern", Some("~/lantern")).unwrap();
        let (head, record) = stored(&world, &written);
        assert_eq!(head.envelope.kind.as_str(), "claim");
        assert_eq!(head.envelope.change, "claim");
        assert_eq!(head.id, written.version);
        let Content::Claim(claim) = record.content else {
            panic!("a claim was stored");
        };
        assert_eq!(Some(&claim.machine), world.host.0.as_ref());
        assert!(lantern.iter().any(|topic| topic.id() == &claim.topic));
        assert_eq!(
            claim.directory.as_ref().map(Directory::as_str),
            Some("~/lantern")
        );
        assert_eq!(record.ending, None);
    }

    #[test]
    fn the_same_machine_topic_and_directory_is_claimed_once() {
        let world = world();
        let deps = world.deps();
        claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let text = refused(claim(&deps, "lantern", Some("~/lantern")));
        assert!(text.contains("is already claimed"), "{text}");
        claim(&deps, "lantern", Some("~/atlas")).unwrap();
        claim(&deps, "lantern", None).unwrap();
        let text = refused(claim(&deps, "lantern", None));
        assert!(text.contains("is already claimed"), "{text}");
    }

    fn usage<T>(result: Result<T, Failure>) -> String {
        match result {
            Err(Failure::Usage(text)) => text,
            Err(other) => panic!("expected a usage failure, got {other}"),
            Ok(_) => panic!("expected a usage failure"),
        }
    }

    #[test]
    fn a_claim_wants_a_live_topic_and_a_directory_the_schema_accepts() {
        let world = world();
        let deps = world.deps();
        let text = refused(claim(&deps, "unknown", None));
        assert!(text.contains("names no document"), "{text}");
        let text = usage(claim(&deps, "lantern", Some("lantern")));
        assert!(text.contains("absolute path"), "{text}");
        assert!(
            world
                .store
                .of_kind(&Kind::parse("claim").unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_unclaim_ends_the_claim_with_the_reason_removed() {
        let world = world();
        let deps = world.deps();
        let placed = claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let text = refused(unclaim(&deps, "lantern", Some("~/atlas")));
        assert!(text.contains("is not claimed"), "{text}");
        let text = refused(unclaim(&deps, "lantern", None));
        assert!(text.contains("is not claimed"), "{text}");

        let written = unclaim(&deps, "lantern", Some("~/lantern")).unwrap();
        assert_eq!(written.document, placed.document);
        let (head, record) = stored(&world, &written);
        assert_eq!(head.envelope.change, "unclaim");
        let ending = record.ending.expect("an ending");
        assert_eq!(ending.reason, Reason::Removed);
        assert_eq!(ending.note, "unclaimed");
        assert_eq!(ending.on.to_string(), "2026-10-09");

        let text = refused(unclaim(&deps, "lantern", Some("~/lantern")));
        assert!(text.contains("is not claimed"), "{text}");
        claim(&deps, "lantern", Some("~/lantern")).unwrap();
    }

    #[test]
    fn an_unclaim_of_a_claim_with_no_directory_wants_no_directory() {
        let world = world();
        let deps = world.deps();
        claim(&deps, "lantern", None).unwrap();
        let text = refused(unclaim(&deps, "lantern", Some("~/lantern")));
        assert!(text.contains("is not claimed"), "{text}");
        unclaim(&deps, "lantern", None).unwrap();
    }

    #[test]
    fn another_machines_claim_neither_blocks_nor_is_ended() {
        let world = world();
        let deps = world.deps();
        let phone = topic(&world, "phone", "");
        let lantern = world
            .store
            .of_kind(&Kind::parse("topic").unwrap())
            .unwrap()
            .into_iter()
            .map(|document| document.id().clone())
            .find(|id| world.lookup().label(id).unwrap() == "lantern")
            .unwrap();
        let theirs = world.put(
            "claim",
            &format!("machine = \"{phone}\"\ntopic = \"{lantern}\"\ndirectory = \"~/lantern\"\n"),
            "\n",
        );
        let text = refused(unclaim(&deps, "lantern", Some("~/lantern")));
        assert!(text.contains("is not claimed"), "{text}");
        let ours = claim(&deps, "lantern", Some("~/lantern")).unwrap();
        assert_ne!(ours.document, theirs);
        let ended = unclaim(&deps, "lantern", Some("~/lantern")).unwrap();
        assert_eq!(ended.document, ours.document);
        let other = world.store.document(&theirs).unwrap();
        let head = other.heads()[0].clone();
        assert_eq!(head.envelope.change, "new");
        assert_eq!(read_or_skip(&head).unwrap().ending, None);
    }

    fn at_home() -> World {
        let mut world = world();
        world.host.1 = Some("/home/desk".to_owned());
        world
    }

    fn directory_of(world: &World, written: &Written) -> Option<String> {
        match stored(world, written).1.content {
            Content::Claim(claim) => claim.directory.map(|directory| directory.to_string()),
            other => panic!("expected a claim, got {other:?}"),
        }
    }

    #[test]
    fn a_directory_under_home_is_stored_in_the_home_form() {
        let world = at_home();
        let deps = world.deps();
        let written = claim(&deps, "lantern", Some("/home/desk/lantern")).unwrap();
        assert_eq!(directory_of(&world, &written).as_deref(), Some("~/lantern"));
        let text = refused(claim(&deps, "lantern", Some("~/lantern")));
        assert!(text.contains("is already claimed"), "{text}");
        let text = refused(claim(&deps, "lantern", Some("/home/desk/lantern")));
        assert!(text.contains("~/lantern: is already claimed"), "{text}");

        let whole = claim(&deps, "lantern", Some("/home/desk")).unwrap();
        assert_eq!(directory_of(&world, &whole).as_deref(), Some("~"));
        let beside = claim(&deps, "lantern", Some("/home/desktop/x")).unwrap();
        assert_eq!(
            directory_of(&world, &beside).as_deref(),
            Some("/home/desktop/x")
        );
    }

    #[test]
    fn an_empty_directory_is_refused_whatever_the_home() {
        let mut world = world();
        world.host.1 = Some("/".to_owned());
        let text = usage(claim(&world.deps(), "lantern", Some("")));
        assert!(text.contains("absolute path"), "{text}");
        let Err(Failure::Usage(text)) = crate::app::context::context(&world.deps(), "") else {
            panic!("an empty directory must be a usage failure");
        };
        assert!(text.contains("absolute path"), "{text}");
    }

    #[test]
    fn a_directory_no_claim_could_carry_is_the_same_usage_failure_everywhere() {
        let world = world();
        let deps = world.deps();
        claim(&deps, "lantern", Some("~/lantern")).unwrap();
        for bad in ["", "lantern", "~lantern", "./lantern"] {
            let claimed = usage(claim(&deps, "lantern", Some(bad)));
            assert!(claimed.contains("absolute path"), "{claimed}");
            assert_eq!(usage(unclaim(&deps, "lantern", Some(bad))), claimed);
            assert_eq!(usage(crate::app::context::context(&deps, bad)), claimed);
        }
    }

    #[test]
    fn a_host_with_no_home_stores_a_directory_as_given() {
        let world = world();
        let written = claim(&world.deps(), "lantern", Some("/home/desk/lantern")).unwrap();
        assert_eq!(
            directory_of(&world, &written).as_deref(),
            Some("/home/desk/lantern")
        );
    }

    #[test]
    fn an_unclaim_by_the_absolute_form_ends_the_claim_in_the_home_form() {
        let world = at_home();
        let deps = world.deps();
        let placed = claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let text = refused(unclaim(&deps, "lantern", Some("/home/desktop/lantern")));
        assert!(text.contains("is not claimed"), "{text}");
        let written = unclaim(&deps, "lantern", Some("/home/desk/lantern")).unwrap();
        assert_eq!(written.document, placed.document);
        assert!(stored(&world, &written).1.ending.is_some());
    }

    fn stored_as(world: &World, directory: &str) -> DocumentId {
        let lantern = named(world, "lantern");
        world.put(
            "claim",
            &claim_fields(&world.machine(), &lantern, directory),
            "\n",
        )
    }

    #[test]
    fn a_claim_with_a_trailing_slash_is_the_claim_without_it() {
        let world = world();
        let deps = world.deps();
        claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let text = refused(claim(&deps, "lantern", Some("~/lantern/")));
        assert_eq!(text, "lantern at ~/lantern: is already claimed");
        let written = claim(&deps, "lantern", Some("~/atlas/")).unwrap();
        assert_eq!(directory_of(&world, &written).as_deref(), Some("~/atlas"));
        assert_eq!(
            stored(&world, &written).0.fields.get("directory"),
            Some(&"~/atlas".into())
        );
    }

    #[test]
    fn a_claim_stored_with_a_trailing_slash_is_already_claimed_without_it() {
        let world = world();
        stored_as(&world, "~/lantern/");
        let text = refused(claim(&world.deps(), "lantern", Some("~/lantern")));
        assert_eq!(text, "lantern at ~/lantern: is already claimed");
    }

    #[test]
    fn an_unclaim_with_a_trailing_slash_ends_the_claim_stored_without_it() {
        let world = world();
        let deps = world.deps();
        let placed = claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let written = unclaim(&deps, "lantern", Some("~/lantern/")).unwrap();
        assert_eq!(written.document, placed.document);
        assert!(stored(&world, &written).1.ending.is_some());
    }

    #[test]
    fn an_unclaim_ends_a_claim_stored_with_a_trailing_slash() {
        let world = world();
        let deps = world.deps();
        let placed = stored_as(&world, "~/lantern/");
        let written = unclaim(&deps, "lantern", Some("~/lantern")).unwrap();
        assert_eq!(written.document, placed);
        let (head, record) = stored(&world, &written);
        assert_eq!(
            record.ending.map(|ending| ending.reason),
            Some(Reason::Removed)
        );
        assert_eq!(head.fields.get("directory"), Some(&"~/lantern".into()));
        let text = refused(unclaim(&deps, "lantern", Some("~/lantern/")));
        assert!(text.contains("is not claimed"), "{text}");
    }

    fn named(world: &World, name: &str) -> DocumentId {
        world.lookup().one(name).unwrap()
    }

    #[test]
    fn a_claim_wants_this_hosts_machine_topic_live() {
        let world = world();
        let desk = named(&world, "desk");
        world.amend(&desk, &format!("name = \"desk\"\n{TOPIC}{ENDED}"), "\n");
        let text = refused(claim(&world.deps(), "lantern", Some("~/lantern")));
        assert_eq!(text, "new claim: machine: desk is ended");
        assert!(
            world
                .store
                .of_kind(&Kind::parse("claim").unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_claim_on_an_ended_topic_is_still_removed() {
        let world = world();
        let deps = world.deps();
        let placed = claim(&deps, "lantern", Some("~/lantern")).unwrap();
        let lantern = named(&world, "lantern");
        world.amend(
            &lantern,
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "\n",
        );
        let written = unclaim(&deps, "lantern", Some("~/lantern")).unwrap();
        assert_eq!(written.document, placed.document);
        assert!(stored(&world, &written).1.ending.is_some());
    }

    #[test]
    fn a_claim_on_an_ended_topic_is_refused() {
        let world = world();
        let lantern = named(&world, "lantern");
        world.amend(
            &lantern,
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "\n",
        );
        let text = refused(claim(&world.deps(), "lantern", Some("~/lantern")));
        assert_eq!(text, "new claim: topic: lantern is ended");
        let text = refused(claim(&world.deps(), "lantern", None));
        assert_eq!(text, "new claim: topic: lantern is ended");
        assert!(
            world
                .store
                .of_kind(&Kind::parse("claim").unwrap())
                .unwrap()
                .is_empty()
        );
    }
}
