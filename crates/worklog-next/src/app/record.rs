//! Recording that a command ran, the one write that stores no version.

use crate::app::{Deps, Failure};
use crate::domain::release::ReleaseVersion;
use crate::domain::usage::Invocation;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub exit: i32,
    pub refusal: Option<String>,
    pub hits: Option<usize>,
}

pub struct Run<'a> {
    pub version: ReleaseVersion,
    pub command: &'a str,
    pub arguments: &'a [String],
    /// The working directory, as an absolute path.
    pub directory: &'a str,
}

/// Appends the run to this machine's log. Refuses on a host bound to no machine topic.
pub fn record(deps: &Deps, run: &Run, outcome: Outcome) -> Result<(), Failure> {
    Ok(deps.usage.record(&Invocation {
        written: deps.clock.now(),
        machine: deps.machine()?,
        version: Some(run.version.to_string()),
        session: deps.host.session(),
        command: run.command.to_owned(),
        exit: outcome.exit,
        refusal: outcome.refusal,
        hits: outcome.hits,
        directory: deps.directory(run.directory)?.to_string(),
        arguments: run.arguments.to_vec(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{NOW, World, refused};
    use crate::domain::ports::Usage as _;
    use crate::domain::testing::{lamp_session, version};
    use crate::domain::usage::Kept;
    use crate::domain::version::Stamp;

    fn run<'a>(command: &'a str, directory: &'a str) -> Run<'a> {
        Run {
            version: version("1.2.3"),
            command,
            arguments: &[],
            directory,
        }
    }

    #[test]
    fn a_run_is_logged_with_its_version_its_directory_as_a_claim_spells_it_and_how_it_ended() {
        let mut world = World::new();
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
            version: version("1.2.3"),
            command: "show",
            arguments: &arguments,
            directory: "/home/desk/link/case",
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
        assert_eq!(world.usage.read(&Kept::default()), Ok(vec![logged]));
    }

    #[test]
    fn a_run_in_an_agent_s_session_is_logged_with_it() {
        let mut world = World::new();
        let session = lamp_session();
        world.host.set_session(Some(session.clone()));
        let ran = |command: &str| {
            let run = run(command, "/srv");
            record(&world.deps(), &run, Outcome::default())
        };
        ran("topics").unwrap();
        ran("facts").unwrap();
        let logged = world.usage.read(&Kept::default()).unwrap();
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[1].session, Some(session));
        assert_eq!(logged[1].directory, "/srv");
        assert_eq!((logged[1].exit, logged[1].hits), (0, None));
    }

    #[test]
    fn a_host_bound_to_no_machine_and_a_directory_not_from_the_root_log_nothing() {
        let mut world = World::new();
        let ran = |world: &World, directory: &str| {
            let run = run("topics", directory);
            record(&world.deps(), &run, Outcome::default())
        };
        assert!(matches!(ran(&world, "projects"), Err(Failure::Usage(_))));
        world.host.set_machine(None);
        assert_eq!(
            refused(ran(&world, "/srv")),
            "this host is not set up; run `worklog-next init`"
        );
        assert_eq!(world.usage.read(&Kept::default()), Ok(Vec::new()));
    }
}
