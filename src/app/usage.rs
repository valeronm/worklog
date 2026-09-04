//! Recording that a command ran, the one use case ending in no version.

use crate::domain::graph;
use crate::domain::usage::Invocation;

use super::{Deps, Failure, machine};

/// Appends the run to this machine's log.
pub fn record(
    deps: &Deps,
    command: &str,
    arguments: Vec<String>,
    directory: &str,
    exit: i32,
) -> Result<(), Failure> {
    Ok(deps.usage.record(&Invocation {
        written: deps.clock.now(),
        machine: machine(deps)?,
        command: command.to_owned(),
        exit,
        directory: graph::contract(directory, &deps.home),
        arguments,
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
            0,
        )
        .unwrap();
        let logged = w.usage.all().unwrap();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].directory, "~/projects/lantern");
        assert_eq!(logged[0].command, "facts");
        assert_eq!(logged[0].arguments, ["lantern"]);
    }

    #[test]
    fn a_host_without_a_machine_name_logs_nothing() {
        let w = World::unnamed();
        assert!(record(&w.deps(), "context", vec![], "/home/u", 0).is_err());
        assert_eq!(w.usage.all().unwrap(), []);
    }
}
