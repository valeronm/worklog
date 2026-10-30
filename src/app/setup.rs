use std::fmt;

use crate::app::draft::usage;
use crate::app::lookup::Lookup;
use crate::app::machines::{PLACEHOLDER, topic_named};
use crate::app::save::stepped;
use crate::app::{Deps, Failure, kind_named, text};
use crate::domain::id::DocumentId;
use crate::domain::ports::{Host, HostRead, StoreError};
use crate::domain::schema::{KindOf, Name};
use crate::domain::session::Session;
use crate::domain::version::Fields;

const INIT: &str = "init";
const JOIN: &str = "join";

#[derive(Clone, Debug, PartialEq, Eq)]
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

impl HostRead for Becoming<'_> {
    fn machine(&self) -> Result<Option<DocumentId>, StoreError> {
        Ok(Some(self.machine.clone()))
    }

    fn home(&self) -> Result<Option<String>, StoreError> {
        self.host.home()
    }

    fn resolve(&self, absolute: &str) -> Result<String, StoreError> {
        self.host.resolve(absolute)
    }

    fn is_directory(&self, absolute: &str) -> Result<bool, StoreError> {
        self.host.is_directory(absolute)
    }

    fn session(&self) -> Option<Session> {
        self.host.session()
    }

    fn name(&self) -> Option<String> {
        self.host.name()
    }
}

impl Host for Becoming<'_> {
    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError> {
        self.host.bind(machine)
    }
}

/// Refuses a host that has a machine topic.
pub fn refuse_bound(deps: &Deps) -> Result<(), Failure> {
    match deps.host.machine()? {
        Some(machine) => Err(Failure::Refused(format!(
            "this host is already set up as {}",
            Lookup::new(deps.read().store).label(&machine)?
        ))),
        None => Ok(()),
    }
}

/// What a host joining a store states of its machine topic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joining<'a> {
    /// The store holds the topic.
    Known,
    /// The topic is new to the store; without a summary it carries `PLACEHOLDER`.
    New { summary: Option<&'a str> },
}

/// A store a setup refuses to bind the host to; its text holds no command.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    HoldsDocuments,
    HoldsNothing,
    LacksTopic(Name),
    HoldsTopic(Name),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::HoldsDocuments => f.write_str("the store holds documents"),
            Refusal::HoldsNothing => {
                f.write_str("the store holds no documents; a shared store may not have synced yet")
            }
            Refusal::LacksTopic(name) => write!(
                f,
                "the store holds no topic named {name}; it may not have synced yet"
            ),
            Refusal::HoldsTopic(name) => write!(f, "{name} is already in the store"),
        }
    }
}

/// Makes the machine topic `name` of a new store, with `summary` or `PLACEHOLDER`, and binds the
/// host to it. Refuses a host already bound and a store that holds anything; a malformed name
/// is a usage failure.
pub fn start(deps: &Deps, name: &str, summary: Option<&str>) -> Result<Bound, Failure> {
    refuse_bound(deps)?;
    let name = Name::parse(name).map_err(usage)?;
    let lookup = Lookup::new(deps.read().store);
    if !lookup.holds_nothing()? {
        return Err(Failure::Setup(Refusal::HoldsDocuments));
    }
    made(deps, &lookup, &name, summary, INIT)
}

/// Binds the host to a store that holds documents: to the topic `name` it holds for
/// `Joining::Known`, or to one it makes for `Joining::New`. Refuses a host already bound, a
/// store holding nothing, a known machine whose topic the store lacks, a new machine whose
/// topic it holds, and a name several topics hold; a malformed name is a usage failure.
pub fn join(deps: &Deps, name: &str, joining: Joining) -> Result<Bound, Failure> {
    refuse_bound(deps)?;
    let name = Name::parse(name).map_err(usage)?;
    let lookup = Lookup::new(deps.read().store);
    if lookup.holds_nothing()? {
        return Err(Failure::Setup(Refusal::HoldsNothing));
    }
    match (topic_named(&lookup, &name)?, joining) {
        (Some(held), Joining::Known) => bound_to(deps, &lookup, held),
        (None, Joining::Known) => Err(Failure::Setup(Refusal::LacksTopic(name))),
        (Some(_), Joining::New { .. }) => Err(Failure::Setup(Refusal::HoldsTopic(name))),
        (None, Joining::New { summary }) => made(deps, &lookup, &name, summary, JOIN),
    }
}

fn bound_to(deps: &Deps, lookup: &Lookup, machine: DocumentId) -> Result<Bound, Failure> {
    let label = lookup.label(&machine)?;
    deps.host.bind(&machine)?;
    Ok(Bound {
        document: machine,
        label,
        created: false,
    })
}

fn made(
    deps: &Deps,
    lookup: &Lookup,
    name: &Name,
    summary: Option<&str>,
    change: &str,
) -> Result<Bound, Failure> {
    let machine = deps.ids.mint()?;
    let mut fields = Fields::new();
    fields.insert("name".to_owned(), text(name.as_str()));
    fields.insert("summary".to_owned(), text(summary.unwrap_or(PLACEHOLDER)));
    // A first version carries the machine topic of its writer, and this writer has none
    // until the version is stored.
    let host = Becoming {
        host: deps.host,
        machine: &machine,
    };
    let as_itself = Deps {
        host: &host,
        ..*deps
    };
    let kind = kind_named(KindOf::Topic);
    stepped(&as_itself, lookup, &machine, &kind, &fields, "\n", change)?.store(&as_itself)?;
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
        ENDED, World, fork, found, head, record, refused, topic, topic_fields, world,
    };
    use crate::domain::ports::{Store, StoreRead};
    use crate::domain::schema::SchemaError;
    use crate::domain::schema::address::Found;
    use crate::domain::testing::{first, first_minted, lantern};
    use crate::domain::version::Kind;

    fn bare() -> World {
        let mut world = World::new();
        world.host.set_machine(None);
        world
    }

    fn lacks(name: &str) -> Result<Bound, Failure> {
        Err(Failure::Setup(Refusal::LacksTopic(
            Name::parse(name).unwrap(),
        )))
    }

    fn topics(world: &World) -> usize {
        let topic = Kind::parse("topic").unwrap();
        world.store.of_kind(&topic).unwrap().len()
    }

    fn sharing() -> (World, DocumentId) {
        let (mut world, lantern) = world();
        world.host.set_machine(None);
        (world, lantern)
    }

    #[test]
    fn a_started_store_s_machine_topic_is_written_by_itself_and_the_host_is_bound_to_it() {
        let world = bare();
        let bound = start(&world.deps(), "desk", Some("The desk")).unwrap();
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
    fn a_machine_topic_made_without_a_summary_carries_the_placeholder() {
        let world = bare();
        let started = start(&world.deps(), "desk", None).unwrap();
        assert!(started.created);
        assert_eq!(
            record(&world, &started.document).content.summary(),
            Some(PLACEHOLDER)
        );

        let (world, _) = sharing();
        let joined = join(&world.deps(), "atlas", Joining::New { summary: None }).unwrap();
        assert!(joined.created);
        assert_eq!(
            record(&world, &joined.document).content.summary(),
            Some(PLACEHOLDER)
        );
        assert_eq!(world.host.machine(), Ok(Some(joined.document)));
    }

    #[test]
    fn a_store_holding_documents_is_not_started() {
        let (world, _) = sharing();
        let before = topics(&world);
        assert_eq!(
            start(&world.deps(), "atlas", Some("The atlas")),
            Err(Failure::Setup(Refusal::HoldsDocuments))
        );
        assert_eq!(topics(&world), before);
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_store_holding_nothing_is_not_joined() {
        for machine in [
            Joining::Known,
            Joining::New {
                summary: Some("The desk"),
            },
        ] {
            let world = bare();
            assert_eq!(
                join(&world.deps(), "desk", machine),
                Err(Failure::Setup(Refusal::HoldsNothing))
            );
            assert_eq!(topics(&world), 0);
            assert_eq!(world.host.machine(), Ok(None));
        }
    }

    #[test]
    fn a_known_machine_is_bound_to_the_topic_that_holds_the_name_and_keeps_it() {
        let (world, lantern) = sharing();
        let before = topics(&world);
        let bound = join(&world.deps(), "lantern", Joining::Known).unwrap();
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
        assert_eq!(world.store.headers(&lantern).unwrap().history().len(), 1);
        assert_eq!(record(&world, &lantern).content.summary(), Some("s"));
    }

    #[test]
    fn a_known_machine_is_bound_to_a_forked_topic() {
        let (world, lantern) = sharing();
        fork(&world, &lantern, &topic_fields("lantern", ""));
        let bound = join(&world.deps(), "lantern", Joining::Known).unwrap();
        assert_eq!(
            (bound.document.clone(), bound.created),
            (lantern.clone(), false)
        );
        assert_eq!(world.host.machine(), Ok(Some(lantern)));
    }

    #[test]
    fn a_known_machine_the_store_lacks_is_refused_as_not_synced() {
        let (world, _) = sharing();
        let before = topics(&world);
        assert_eq!(join(&world.deps(), "atlas", Joining::Known), lacks("atlas"));
        assert_eq!(topics(&world), before);
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_new_machine_the_store_holds_is_refused() {
        let (world, _) = sharing();
        let before = topics(&world);
        let new = Joining::New {
            summary: Some("The lantern"),
        };
        assert_eq!(
            join(&world.deps(), "lantern", new),
            Err(Failure::Setup(Refusal::HoldsTopic(
                Name::parse("lantern").unwrap()
            )))
        );
        assert_eq!(topics(&world), before);
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_host_that_is_bound_is_refused_before_anything_is_asked_of_it() {
        let (world, _) = world();
        let text = refused(refuse_bound(&world.deps()));
        assert_eq!(text, "this host is already set up as desk");
        assert_eq!(refuse_bound(&bare().deps()), Ok(()));
    }

    #[test]
    fn a_host_already_set_up_is_refused_by_both() {
        let (world, _) = world();
        let before = topics(&world);
        let refusals = [
            refused(start(&world.deps(), "atlas", Some("The atlas"))),
            refused(join(&world.deps(), "lantern", Joining::Known)),
            refused(join(&world.deps(), "atlas", Joining::New { summary: None })),
        ];
        for text in refusals {
            assert_eq!(text, "this host is already set up as desk");
        }
        assert_eq!(topics(&world), before);
        assert_eq!(world.host.machine(), Ok(Some(first_minted())));
    }

    #[test]
    fn a_name_only_ended_topics_hold_is_a_new_machine() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let ended = [topic(&world, "phone", ENDED), topic(&world, "phone", ENDED)];
        world.host.set_machine(None);
        assert_eq!(join(&world.deps(), "phone", Joining::Known), lacks("phone"));

        let new = Joining::New {
            summary: Some("The phone"),
        };
        let bound = join(&world.deps(), "phone", new).unwrap();
        assert!(bound.created);
        assert!(!ended.contains(&bound.document));
        assert_eq!(bound.label, "phone");
        let stored = head(&world, &bound.document);
        assert_eq!(stored.envelope.machine, bound.document);
        assert_eq!(stored.envelope.change, "join");
        assert_eq!(world.host.machine(), Ok(Some(bound.document)));
    }

    #[test]
    fn a_name_several_topics_hold_binds_to_none_of_them() {
        let (mut world, _) = world();
        topic(&world, "lantern", "");
        world.host.set_machine(None);
        for machine in [
            Joining::Known,
            Joining::New {
                summary: Some("The lantern"),
            },
        ] {
            let text = refused(join(&world.deps(), "lantern", machine));
            assert!(text.contains("is held by several documents"), "{text}");
            assert_eq!(world.host.machine(), Ok(None));
        }
    }

    #[test]
    fn a_name_that_is_none_is_a_usage_failure_to_both() {
        let world = bare();
        let not_a_name = SchemaError::NotAName("Desk".to_owned()).to_string();
        let Err(Failure::Usage(text)) = start(&world.deps(), "Desk", Some("The desk")) else {
            panic!("a name the schema refuses is a usage failure");
        };
        assert_eq!(text, not_a_name);
        let (world, _) = sharing();
        let Err(Failure::Usage(text)) = join(&world.deps(), "Desk", Joining::Known) else {
            panic!("a name the schema refuses is a usage failure");
        };
        assert_eq!(text, not_a_name);
        assert_eq!(world.host.machine(), Ok(None));
    }

    #[test]
    fn a_topic_that_held_the_name_before_is_not_a_known_machine() {
        let (mut world, _) = world();
        let renamed = topic(&world, "atlas", "former_names = [\"phone\"]\n");
        world.host.set_machine(None);
        assert_eq!(join(&world.deps(), "phone", Joining::Known), lacks("phone"));

        let new = Joining::New {
            summary: Some("The phone"),
        };
        let bound = join(&world.deps(), "phone", new).unwrap();
        assert!(bound.created);
        assert_ne!(bound.document, renamed);
        assert_eq!(bound.label, "phone");
    }

    #[test]
    fn the_start_of_a_topic_s_id_is_a_name_and_not_that_topic() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let held = first(&lantern(), "topic", &topic_fields("lantern", ""), "\n");
        world.store.put(&held).unwrap();
        assert_eq!(found(&world, "7f3a"), Found::One(lantern()));
        world.host.set_machine(None);
        let new = Joining::New {
            summary: Some("The phone"),
        };
        let bound = join(&world.deps(), "7f3a", new).unwrap();
        assert!(bound.created);
        assert_ne!(bound.document, lantern());
        assert_eq!(bound.label, "7f3a");
    }

    #[test]
    fn a_start_whose_host_did_not_bind_is_finished_by_a_join() {
        let mut world = bare();
        world.host.set_binds(false);
        let result = start(&world.deps(), "desk", Some("The desk"));
        assert!(matches!(result, Err(Failure::Store(_))), "{result:?}");
        assert_eq!(topics(&world), 1);
        assert_eq!(world.host.machine(), Ok(None));

        world.host.set_binds(true);
        assert_eq!(
            start(&world.deps(), "desk", Some("The desk")),
            Err(Failure::Setup(Refusal::HoldsDocuments))
        );
        let bound = join(&world.deps(), "desk", Joining::Known).unwrap();
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
        let text = refused(start(&world.deps(), "desk", Some("")));
        assert!(text.contains("summary"), "{text}");
        assert_eq!(topics(&world), 0);
        assert_eq!(world.host.machine(), Ok(None));
    }
}
