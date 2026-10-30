use std::io::Write;
use std::path::Path;

use serde::Serialize;

use super::agents::{self, Placing};
use super::args::{HostCommand, InitArgs};
use super::render::{Output, Render};
use super::{Environment, prompt};
use crate::app::setup::{self, Bound, Joining};
use crate::app::{Deps, Failure};
use crate::domain::id::DocumentId;
use crate::wiring;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Initialized {
    pub document: DocumentId,
    pub label: String,
    pub created: bool,
    pub written: Vec<String>,
}

impl Initialized {
    pub(super) fn new(bound: Bound, written: Vec<String>) -> Initialized {
        let Bound {
            document,
            label,
            created,
        } = bound;
        Initialized {
            document,
            label,
            created,
            written,
        }
    }
}

impl Render for Initialized {
    fn text(&self) -> String {
        let how = if self.created { "created" } else { "bound" };
        format!("{} {how}\n{}", self.label, agents::lines(&self.written))
    }
}

pub(super) fn run(
    command: &HostCommand,
    json: bool,
    environment: &Environment,
    err: &mut dyn Write,
) -> Result<Output, Failure> {
    match command {
        HostCommand::Init { given: None } => prompt::asked(json, environment, err),
        HostCommand::Init {
            given:
                Some(InitArgs {
                    name,
                    store,
                    summary,
                    agents,
                }),
        } => bound_over(store, *agents, json, environment, |deps| {
            setup::start(deps, name, summary.as_deref())
        }),
        HostCommand::Join {
            name,
            store,
            summary,
            agents,
        } => {
            let joining = (summary.as_deref()).map_or(Joining::Known, |summary| Joining::New {
                summary: Some(summary),
            });
            bound_over(store, *agents, json, environment, |deps| {
                setup::join(deps, name, joining)
            })
        }
    }
}

fn bound_over(
    store: &Path,
    with_agents: bool,
    json: bool,
    environment: &Environment,
    bind: impl FnOnce(&Deps) -> Result<Bound, Failure>,
) -> Result<Output, Failure> {
    let adapters = wiring::wire_over(environment.variable, &environment.placed(store)?)?;
    let deps = adapters.deps();
    let placing = with_agents
        .then(|| Placing::on_this_host(environment))
        .transpose()?;
    initialized(placing, json, || bind(&deps))
}

// Everything that can refuse an agent's install is settled before the host is bound.
pub(super) fn initialized(
    placing: Option<Placing>,
    json: bool,
    bind: impl FnOnce() -> Result<Bound, Failure>,
) -> Result<Output, Failure> {
    let bound = bind()?;
    let done = placing
        .map(|placing| placing.install())
        .transpose()?
        .unwrap_or_default();
    Ok(done.output(json, |written| Initialized::new(bound, written)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::atlas;

    #[test]
    fn a_bound_host_is_its_line_and_then_the_paths_written() {
        let written = vec!["/home/desk/a".to_owned(), "/home/desk/b".to_owned()];
        let mut made = Initialized::new(
            Bound {
                document: atlas(),
                label: "atlas".to_owned(),
                created: true,
            },
            written,
        );
        assert_eq!(made.text(), "atlas created\n/home/desk/a\n/home/desk/b\n");
        made.created = false;
        made.written.clear();
        assert_eq!(made.text(), "atlas bound\n");
    }
}
