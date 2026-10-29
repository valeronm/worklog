//! Recording that a command ran, the one write that stores no version.

use crate::app::lookup::Lookup;
use crate::app::{Deps, Failure};
use crate::domain::release::Built;
use crate::domain::usage::Invocation;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub exit: i32,
    pub refusal: Option<String>,
    pub hits: Option<usize>,
}

pub struct Run<'a> {
    pub version: Built,
    pub command: &'a str,
    pub arguments: &'a [String],
    /// An absolute path.
    pub working_directory: &'a str,
}

/// Appends the run to this machine's log where the store holds the machine's topic. Refuses on
/// a host bound to no machine topic.
pub fn record(deps: &Deps, run: &Run, outcome: Outcome) -> Result<(), Failure> {
    let machine = deps.read().machine()?;
    // A directory that has not synced or mounted holds no file of the machine's topic, and a
    // line appended there creates files at the path the store belongs at.
    let machine_topic = Lookup::new(deps.read().store).headers(&machine)?;
    if machine_topic.history().is_empty() && machine_topic.unreadable().is_empty() {
        return Ok(());
    }
    Ok(deps.usage.record(&Invocation {
        written: deps.clock.now(),
        machine,
        version: Some(run.version.to_string()),
        session: deps.host.session(),
        command: run.command.to_owned(),
        exit: outcome.exit,
        refusal: outcome.refusal,
        hits: outcome.hits,
        directory: deps.read().directory(run.working_directory)?.to_string(),
        arguments: run.arguments.to_vec(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{NOW, World, refused, topic};
    use crate::domain::id::VersionId;
    use crate::domain::ports::UsageRead as _;
    use crate::domain::testing::{lamp_session, version};
    use crate::domain::usage::RunFilter;
    use crate::domain::version::{ReadError, Stamp};

    fn run<'a>(command: &'a str, working_directory: &'a str) -> Run<'a> {
        Run {
            version: version("1.2.3").into(),
            command,
            arguments: &[],
            working_directory,
        }
    }

    #[test]
    fn a_run_is_logged_with_its_version_its_directory_as_a_claim_spells_it_and_how_it_ended() {
        let mut world = World::new();
        topic(&world, "desk", "");
        world.host.set_home(Some("/home/desk".to_owned()));
        world
            .host
            .link("/home/desk/link", "/home/desk/projects/lantern");
        let outcome = Outcome {
            exit: 1,
            refusal: Some("lantern/relay: names no document".to_owned()),
            hits: Some(0),
        };
        let arguments = vec!["lantern/relay".to_owned(), "--json".to_owned()];
        let run = Run {
            version: version("1.2.3").into(),
            command: "show",
            arguments: &arguments,
            working_directory: "/home/desk/link/case",
        };
        record(&world.deps(), &run, outcome).unwrap();
        let logged = Invocation {
            written: Stamp::parse(NOW).unwrap(),
            machine: world.machine(),
            version: Some("1.2.3".to_owned()),
            session: None,
            command: "show".to_owned(),
            exit: 1,
            refusal: Some("lantern/relay: names no document".to_owned()),
            hits: Some(0),
            directory: "~/projects/lantern/case".to_owned(),
            arguments,
        };
        assert_eq!(world.usage.read(&RunFilter::default()), Ok(vec![logged]));
    }

    #[test]
    fn a_run_in_an_agent_s_session_is_logged_with_it() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let session = lamp_session();
        world.host.set_session(Some(session.clone()));
        let ran = |command: &str| {
            let run = run(command, "/srv");
            record(&world.deps(), &run, Outcome::default())
        };
        ran("topics").unwrap();
        ran("facts").unwrap();
        let logged = world.usage.read(&RunFilter::default()).unwrap();
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[1].session, Some(session));
        assert_eq!(logged[1].directory, "/srv");
        assert_eq!((logged[1].exit, logged[1].hits), (0, None));
    }

    #[test]
    fn a_host_bound_to_no_machine_and_a_directory_not_from_the_root_log_nothing() {
        let mut world = World::new();
        topic(&world, "desk", "");
        let ran = |world: &World, directory: &str| {
            let run = run("topics", directory);
            record(&world.deps(), &run, Outcome::default())
        };
        assert!(matches!(ran(&world, "projects"), Err(Failure::Usage(_))));
        world.host.set_machine(None);
        assert_eq!(
            refused(ran(&world, "/srv")),
            "this host is not set up; run `worklog init`"
        );
        assert_eq!(world.usage.read(&RunFilter::default()), Ok(Vec::new()));
    }

    #[test]
    fn a_run_is_logged_only_into_a_store_that_holds_the_machine_s_topic() {
        let world = World::new();
        let ran = |world: &World| record(&world.deps(), &run("topics", "/srv"), Outcome::default());
        assert_eq!(ran(&world), Ok(()));
        assert_eq!(world.usage.read(&RunFilter::default()), Ok(Vec::new()));
        assert_eq!(topic(&world, "desk", ""), world.machine());
        assert_eq!(ran(&world), Ok(()));
        assert_eq!(world.usage.read(&RunFilter::default()).unwrap().len(), 1);
    }

    #[test]
    fn a_run_is_logged_into_a_store_whose_files_of_the_machine_s_topic_do_not_read() {
        let world = World::new();
        let damaged = VersionId::of(b"damaged");
        (world.store).plant_unreadable(&world.machine(), damaged, ReadError::Corrupt);
        record(&world.deps(), &run("topics", "/srv"), Outcome::default()).unwrap();
        assert_eq!(world.usage.read(&RunFilter::default()).unwrap().len(), 1);
    }

    #[test]
    fn a_run_asks_the_store_for_the_machine_s_topic_and_reads_nothing_else_of_it() {
        let world = World::new();
        topic(&world, "desk", "");
        let world = world.counted();
        record(&world.deps(), &run("topics", "/srv"), Outcome::default()).unwrap();
        assert_eq!((world.store.headers.get(), world.store.scans()), (1, 0));
    }
}
