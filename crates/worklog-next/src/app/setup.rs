use serde::Serialize;

use crate::app::draft::usage;
use crate::app::lookup::{Lookup, held_by_several};
use crate::app::save::stepped;
use crate::app::{Deps, Failure, kind_named, text};
use crate::domain::id::DocumentId;
use crate::domain::ports::{Host, StoreError};
use crate::domain::schema::{KindOf, Name};
use crate::domain::version::Fields;

const INIT: &str = "init";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Bound {
    /// The machine topic the host is bound to.
    pub document: DocumentId,
    pub label: String,
    pub created: bool,
}

struct Becoming<'a> {
    host: &'a dyn Host,
    machine: &'a DocumentId,
}

impl Host for Becoming<'_> {
    fn machine(&self) -> Result<Option<DocumentId>, StoreError> {
        Ok(Some(self.machine.clone()))
    }

    fn home(&self) -> Result<Option<String>, StoreError> {
        self.host.home()
    }

    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError> {
        self.host.bind(machine)
    }

    fn resolve(&self, absolute: &str) -> Result<String, StoreError> {
        self.host.resolve(absolute)
    }
}

/// Binds the host to the topic that is not ended and named `name`, making it with `summary`
/// when there is none. Refuses a host already bound and a name several such topics hold; a
/// malformed name, or a topic to make without a summary, is a usage failure.
pub fn init(deps: &Deps, name: &str, summary: Option<&str>) -> Result<Bound, Failure> {
    let lookup = Lookup::new(deps.store);
    if let Some(machine) = deps.host.machine()? {
        return Err(Failure::Refused(format!(
            "this host is already set up as {}",
            lookup.label(&machine)?
        )));
    }
    let name = Name::parse(name).map_err(usage)?;
    let holders = lookup.holders(KindOf::Topic, "name", &[name.as_str()], false)?;
    let held = match holders.as_slice() {
        [] => None,
        [only] => Some(only.id().clone()),
        several => {
            let ids = several.iter().map(|holder| holder.id());
            return Err(held_by_several(name.as_str(), ids));
        }
    };
    if let Some(machine) = held {
        let label = lookup.label(&machine)?;
        deps.host.bind(&machine)?;
        return Ok(Bound {
            document: machine,
            label,
            created: false,
        });
    }
    let Some(summary) = summary else {
        return Err(Failure::Usage(format!(
            "no topic is named {name}; making it takes `--summary`"
        )));
    };
    let machine = deps.ids.mint()?;
    let mut fields = Fields::new();
    fields.insert("name".to_owned(), text(name.as_str()));
    fields.insert("summary".to_owned(), text(summary));
    // A first version carries the machine topic of its writer, and this writer has none
    // until the version is stored.
    let as_itself = Deps {
        host: &Becoming {
            host: deps.host,
            machine: &machine,
        },
        ..*deps
    };
    let kind = kind_named(KindOf::Topic);
    stepped(&as_itself, &lookup, &machine, &kind, &fields, "\n", INIT)?.store(&as_itself)?;
    deps.host.bind(&machine)?;
    Ok(Bound {
        document: machine,
        label: name.to_string(),
        created: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{
        ENDED, World, fork, found, head, refused, topic, topic_fields, world,
    };
    use crate::domain::ports::Store;
    use crate::domain::schema::SchemaError;
    use crate::domain::schema::address::Found;
    use crate::domain::testing::{first, first_minted, lantern};
    use crate::domain::version::Kind;

    fn bare() -> World {
        let mut world = World::new();
        world.host.set_machine(None);
        world
    }

    fn topics(world: &World) -> usize {
        let topic = Kind::parse("topic").unwrap();
        world.store.of_kind(&topic).unwrap().len()
    }

    #[test]
    fn a_new_machine_topic_is_written_by_itself_and_the_host_is_bound_to_it() {
        let world = bare();
        let bound = init(&world.deps(), "desk", Some("The desk")).unwrap();
        assert_eq!(
            bound,
            Bound {
                document: first_minted(),
                label: "desk".to_owned(),
                created: true,
            }
        );
        let stored = head(&world, &bound.document);
        assert_eq!(
            toml::to_string(&stored.fields).unwrap(),
            "name = \"desk\"\ncreated = 2026-10-09\nsummary = \"The desk\"\n"
        );
        assert_eq!(stored.envelope.machine, bound.document);
        assert_eq!(stored.envelope.change, "init");
        assert!(stored.envelope.parents.is_empty());
        assert_eq!(stored.envelope.written, world.clock.0);
        assert_eq!(stored.body, "\n");
        assert_eq!(world.host.machine(), Ok(Some(bound.document)));
    }

    #[test]
    fn a_host_is_bound_to_the_topic_that_holds_the_name() {
        let (mut world, lantern) = world();
        world.host.set_machine(None);
        let before = topics(&world);
        let bound = init(&world.deps(), "lantern", Some("ignored")).unwrap();
        assert_eq!(
            bound,
            Bound {
                document: lantern.clone(),
                label: "lantern".to_owned(),
                created: false,
            }
        );
        assert_eq!(world.host.machine(), Ok(Some(lantern.clone())));
        assert_eq!(topics(&world), before);
        let held = world.store.document(&lantern).unwrap();
        assert_eq!(held.history().len(), 1);
    }

    #[test]
    fn a_host_is_bound_to_a_forked_topic_without_a_summary() {
        let (mut world, lantern) = world();
        fork(&world, &lantern, &topic_fields("lantern", ""));
        world.host.set_machine(None);
        let bound = init(&world.deps(), "lantern", None).unwrap();
        assert_eq!(bound.document, lantern);
        assert!(!bound.created);
        assert_eq!(world.host.machine(), Ok(Some(lantern)));
    }

    #[test]
    fn a_host_already_set_up_is_refused() {
        let (world, _) = world();
        let before = topics(&world);
        let text = refused(init(&world.deps(), "atlas", Some("The atlas")));
        assert_eq!(text, "this host is already set up as desk");
        assert_eq!(topics(&world), before);
        assert_eq!(world.host.machine(), Ok(Some(first_minted())));

        let world = bare();
        init(&world.deps(), "desk", Some("The desk")).unwrap();
        let text = refused(init(&world.deps(), "desk", None));
        assert_eq!(text, "this host is already set up as desk");
    }

    #[test]
    fn a_topic_to_make_takes_a_summary() {
        let world = bare();
        let Err(Failure::Usage(text)) = init(&world.deps(), "desk", None) else {
            panic!("a new machine topic without a summary is a usage failure");
        };
        assert!(text.contains("--summary"), "{text}");
        assert_eq!(topics(&world), 0);
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_name_only_an_ended_topic_holds_makes_a_new_topic() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let ended = topic(&world, "phone", ENDED);
        world.host.set_machine(None);
        let Err(Failure::Usage(text)) = init(&world.deps(), "phone", None) else {
            panic!("an ended topic is not one to bind to");
        };
        assert!(text.contains("--summary"), "{text}");

        let bound = init(&world.deps(), "phone", Some("The phone")).unwrap();
        assert!(bound.created);
        assert_ne!(bound.document, ended);
        assert_eq!(bound.label, "phone");
        assert_eq!(
            head(&world, &bound.document).envelope.machine,
            bound.document
        );
        assert_eq!(world.host.machine(), Ok(Some(bound.document)));
    }

    #[test]
    fn a_name_several_topics_hold_binds_to_none_of_them() {
        let (mut world, _) = world();
        topic(&world, "lantern", "");
        world.host.set_machine(None);
        let text = refused(init(&world.deps(), "lantern", Some("The lantern")));
        assert!(text.contains("is held by several documents"), "{text}");
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_name_that_is_none_is_a_usage_failure() {
        let world = bare();
        let Err(Failure::Usage(text)) = init(&world.deps(), "Desk", Some("The desk")) else {
            panic!("a name the schema refuses is a usage failure");
        };
        assert_eq!(text, SchemaError::NotAName("Desk".to_owned()).to_string());
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_name_two_ended_topics_hold_makes_a_new_topic() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let ended = [topic(&world, "phone", ENDED), topic(&world, "phone", ENDED)];
        world.host.set_machine(None);
        let bound = init(&world.deps(), "phone", Some("The phone")).unwrap();
        assert!(bound.created);
        assert!(!ended.contains(&bound.document));
        assert_eq!(world.host.machine(), Ok(Some(bound.document)));
    }

    #[test]
    fn a_topic_that_held_the_name_before_is_not_bound_to() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let renamed = topic(&world, "atlas", "former_names = [\"lantern\"]\n");
        world.host.set_machine(None);
        let Err(Failure::Usage(text)) = init(&world.deps(), "lantern", None) else {
            panic!("a former name is not a topic to bind to");
        };
        assert!(text.contains("--summary"), "{text}");

        let bound = init(&world.deps(), "lantern", Some("The lantern")).unwrap();
        assert!(bound.created);
        assert_ne!(bound.document, renamed);
        assert_eq!(bound.label, "lantern");
    }

    #[test]
    fn the_start_of_a_topic_s_id_is_a_name_and_not_that_topic() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let held = first(&lantern(), "topic", &topic_fields("lantern", ""), "\n");
        world.store.put(&held).unwrap();
        assert_eq!(found(&world, "7f3a"), Found::One(lantern()));
        world.host.set_machine(None);
        let bound = init(&world.deps(), "7f3a", Some("The phone")).unwrap();
        assert!(bound.created);
        assert_ne!(bound.document, lantern());
        assert_eq!(bound.label, "7f3a");
    }

    #[test]
    fn a_topic_stored_by_a_host_that_did_not_bind_is_bound_to_the_next_time() {
        let mut world = bare();
        world.host.set_binds(false);
        let result = init(&world.deps(), "desk", Some("The desk"));
        assert!(matches!(result, Err(Failure::Store(_))), "{result:?}");
        assert_eq!(topics(&world), 1);
        assert_eq!(world.host.machine(), Ok(None));

        world.host.set_binds(true);
        let bound = init(&world.deps(), "desk", None).unwrap();
        assert_eq!(
            bound,
            Bound {
                document: first_minted(),
                label: "desk".to_owned(),
                created: false,
            }
        );
        assert_eq!(topics(&world), 1);
        assert_eq!(world.host.machine(), Ok(Some(first_minted())));
    }

    #[test]
    fn a_summary_that_is_none_is_refused_and_the_host_stays_unbound() {
        let world = bare();
        let text = refused(init(&world.deps(), "desk", Some("")));
        assert!(text.contains("summary"), "{text}");
        assert_eq!(topics(&world), 0);
        assert_eq!(world.host.machine(), Ok(None));
    }
}
