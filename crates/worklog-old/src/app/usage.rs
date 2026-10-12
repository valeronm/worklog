//! Recording that a command ran, the one use case ending in no version.

use crate::domain::graph;
use crate::domain::release;
use crate::domain::usage::Invocation;

use super::{Deps, Failure, machine};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub exit: i32,
    pub refusal: Option<String>,
    pub hits: Option<usize>,
}

impl Outcome {
    #[must_use]
    pub fn failed(failure: &Failure) -> Outcome {
        Outcome {
            exit: failure.exit_code(),
            refusal: Some(failure.to_string()),
            hits: None,
        }
    }
}

/// Appends the run to this machine's log.
pub fn record(
    deps: &Deps,
    command: &str,
    arguments: Vec<String>,
    directory: &str,
    outcome: Outcome,
) -> Result<(), Failure> {
    Ok(deps.usage.record(&Invocation {
        written: deps.clock.now(),
        machine: machine(deps)?,
        command: command.to_owned(),
        exit: outcome.exit,
        refusal: outcome.refusal,
        hits: outcome.hits,
        directory: graph::contract(directory, &deps.home),
        arguments,
        version: Some(release::current().to_string()),
        session: deps.host.session(),
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::World;
    use crate::domain::ports::Usage as _;

    #[test]
    fn a_run_is_logged_with_its_directory_under_home() {
        let w = World::new("m1");
        record(
            &w.deps(),
            "facts",
            vec!["lantern".to_owned()],
            "/home/u/projects/lantern",
            Outcome::default(),
        )
        .unwrap();
        let logged = w.usage.all().unwrap();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].directory, "~/projects/lantern");
        assert_eq!(logged[0].command, "facts");
        assert_eq!(logged[0].arguments, ["lantern"]);
        assert_eq!(
            logged[0].version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn a_host_without_a_machine_name_logs_nothing() {
        let w = World::unnamed();
        assert!(record(&w.deps(), "context", vec![], "/home/u", Outcome::default()).is_err());
        assert_eq!(w.usage.all().unwrap(), []);
    }
}
