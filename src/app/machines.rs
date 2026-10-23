//! What is read of machine topics: by a host about to be bound, of the store it was given,
//! and by any reader, of the summary a topic made for a host carries until one is written.

use std::collections::BTreeSet;

use crate::app::draft::usage;
use crate::app::heads::{kind_of, unended_heads};
use crate::app::lookup::{Lookup, held_by_several};
use crate::app::{Failure, ReadDeps};
use crate::domain::id::DocumentId;
use crate::domain::schema::{Content, KindOf, Name};

/// The summary of a machine topic made before anyone said what the machine is. A topic
/// stored with it is recognized by this text alone.
pub const PLACEHOLDER: &str = "(summary not yet written)";

/// The name the operating system has for this host, cut at its first `.` and lowercased.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostName {
    /// One topic that is not ended holds it.
    Held(String),
    /// It is a topic's name and no topic that is not ended holds it.
    Free(String),
    /// The host has no name, or one that is no topic's name, or one several topics hold.
    Unusable,
}

/// The topic that is not ended and named `name`; refuses a name several such topics hold.
pub(super) fn topic_named(lookup: &Lookup, name: &Name) -> Result<Option<DocumentId>, Failure> {
    let holders = lookup.holders(KindOf::Topic, "name", &[name.as_str()], false)?;
    match holders.as_slice() {
        [] => Ok(None),
        [only] => Ok(Some(only.id().clone())),
        several => {
            let ids = several.iter().map(|holder| holder.id());
            Err(held_by_several(name.as_str(), ids))
        }
    }
}

/// Whether a topic that is not ended is named `name`. Refuses a name several such topics
/// hold; a malformed name is a usage failure.
pub fn held(deps: &ReadDeps, name: &str) -> Result<bool, Failure> {
    let name = Name::parse(name).map_err(usage)?;
    Ok(topic_named(&Lookup::new(deps.store), &name)?.is_some())
}

pub fn host_name(deps: &ReadDeps) -> Result<HostName, Failure> {
    let given = deps.host.name().unwrap_or_default();
    let cut = given.split('.').next().unwrap_or_default().to_lowercase();
    let Ok(name) = Name::parse(&cut) else {
        return Ok(HostName::Unusable);
    };
    match topic_named(&Lookup::new(deps.store), &name) {
        Ok(Some(_)) => Ok(HostName::Held(cut)),
        Ok(None) => Ok(HostName::Free(cut)),
        Err(Failure::Refused(_)) => Ok(HostName::Unusable),
        Err(failure) => Err(failure),
    }
}

/// The names of the topics that are not ended and that a stored version carries as its
/// machine, sorted. Reads every version of the store.
pub fn machines(deps: &ReadDeps) -> Result<Vec<String>, Failure> {
    let lookup = Lookup::new(deps.store);
    let mut names = BTreeSet::new();
    for id in machine_topics(&lookup)? {
        for (_, record) in unended_heads(&*lookup.headers(&id)?) {
            if let Content::Topic(topic) = record.content {
                names.insert(topic.name.to_string());
            }
        }
    }
    Ok(names.into_iter().collect())
}

/// The topics a stored version carries as its machine, ended or not. Reads every version of
/// the store.
pub(super) fn machine_topics(lookup: &Lookup) -> Result<BTreeSet<DocumentId>, Failure> {
    let mut documents = lookup.everything()?;
    for unknown in lookup.unknown()? {
        documents.extend(unknown.documents);
    }
    let writers: BTreeSet<&DocumentId> = (documents.iter())
        .flat_map(|document| document.headers())
        .map(|version| &version.envelope.machine)
        .collect();
    let mut topics = BTreeSet::new();
    for id in writers {
        if kind_of(&*lookup.headers(id)?) == Some(KindOf::Topic) {
            topics.insert(id.clone());
        }
    }
    Ok(topics)
}

/// Whether the store holds no document, of a kind this worklog knows or not, that reads or
/// not.
pub fn holds_nothing(deps: &ReadDeps) -> Result<bool, Failure> {
    let lookup = Lookup::new(deps.store);
    Ok(lookup.everything()?.is_empty()
        && lookup.unknown()?.is_empty()
        && lookup.holding_unreadable()?.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{
        ENDED, World, fact, head, refused, topic, topic_fields, world, written_at,
    };
    use crate::domain::ports::Store;
    use crate::domain::testing::{first, lantern};

    fn named(world: &mut World, name: Option<&str>) -> HostName {
        world.host.set_name(name.map(str::to_owned));
        host_name(&world.deps().read()).unwrap()
    }

    #[test]
    fn a_host_s_name_is_cut_at_its_first_dot_and_lowercased() {
        let (mut world, _) = world();
        assert_eq!(
            named(&mut world, Some("Desk.local")),
            HostName::Held("desk".to_owned())
        );
        assert_eq!(
            named(&mut world, Some("LANTERN")),
            HostName::Held("lantern".to_owned())
        );
        assert_eq!(
            named(&mut world, Some("Atlas.case.example")),
            HostName::Free("atlas".to_owned())
        );
    }

    #[test]
    fn a_host_with_no_name_or_one_that_is_no_topic_s_name_has_none_to_offer() {
        let (mut world, _) = world();
        for unusable in [
            None,
            Some(""),
            Some(".local"),
            Some("the_desk"),
            Some("a b"),
        ] {
            assert_eq!(named(&mut world, unusable), HostName::Unusable);
        }
    }

    #[test]
    fn a_host_s_name_is_held_by_the_one_topic_that_is_not_ended() {
        let (mut world, _) = world();
        topic(&world, "phone", ENDED);
        assert_eq!(
            named(&mut world, Some("phone")),
            HostName::Free("phone".to_owned())
        );
        topic(&world, "phone", "");
        assert_eq!(
            named(&mut world, Some("phone")),
            HostName::Held("phone".to_owned())
        );
        topic(&world, "phone", "");
        assert_eq!(named(&mut world, Some("phone")), HostName::Unusable);
    }

    #[test]
    fn the_placeholder_is_a_summary_a_topic_may_carry() {
        let world = World::new();
        let fields = topic_fields("desk", "").replace("\"s\"", &format!("{PLACEHOLDER:?}"));
        let id = world.put("topic", &fields, "\n");
        assert_eq!(PLACEHOLDER, "(summary not yet written)");
        assert_eq!(
            crate::app::testing::record(&world, &id).content.kind(),
            KindOf::Topic
        );
    }

    #[test]
    fn a_name_is_held_by_a_topic_that_is_not_ended() {
        let (world, _) = world();
        topic(&world, "phone", ENDED);
        let deps = world.deps();
        assert_eq!(held(&deps.read(), "lantern"), Ok(true));
        assert_eq!(held(&deps.read(), "atlas"), Ok(false));
        assert_eq!(held(&deps.read(), "phone"), Ok(false));
        assert!(matches!(held(&deps.read(), "Desk"), Err(Failure::Usage(_))));
        topic(&world, "lantern", "");
        let text = refused(held(&world.deps().read(), "lantern"));
        assert!(text.contains("is held by several documents"), "{text}");
    }

    #[test]
    fn the_machines_are_the_topics_not_ended_that_a_version_was_written_on() {
        let (world, lantern_topic) = world();
        assert_eq!(machines(&world.deps().read()), Ok(vec!["desk".to_owned()]));

        let phone = topic(&world, "phone", "");
        let atlas = topic(&world, "atlas", "");
        let retired = topic(&world, "case", ENDED);
        let relay = fact(&world, &lantern_topic, "relay", "");
        let root = head(&world, &relay);
        for (machine, stamp) in [
            (&phone, "2026-10-09T19:00:00+01:00"),
            (&retired, "2026-10-09T20:00:00+01:00"),
            (&relay, "2026-10-09T21:00:00+01:00"),
        ] {
            let fields = toml::to_string(&root.fields).unwrap();
            written_at(&world, &root, &fields, stamp, "The relay.\n", Some(machine));
        }
        assert_eq!(
            machines(&world.deps().read()),
            Ok(vec!["desk".to_owned(), "phone".to_owned()])
        );

        let renamed = topic_fields("lamp", "");
        let phone_head = head(&world, &phone);
        let stamp = "2026-10-09T22:00:00+01:00";
        written_at(&world, &phone_head, &renamed, stamp, "\n", Some(&atlas));
        assert_eq!(
            machines(&world.deps().read()),
            Ok(vec![
                "atlas".to_owned(),
                "desk".to_owned(),
                "lamp".to_owned()
            ])
        );
    }

    #[test]
    fn a_store_holds_nothing_until_a_document_is_in_it() {
        let (stocked, _) = world();
        assert_eq!(holds_nothing(&stocked.deps().read()), Ok(false));

        let bare = World::new();
        assert_eq!(holds_nothing(&bare.deps().read()), Ok(true));
        let sketch = first(&lantern(), "sketch", "name = \"lantern\"", "\n");
        bare.store.put(&sketch).unwrap();
        assert_eq!(holds_nothing(&bare.deps().read()), Ok(false));
    }
}
