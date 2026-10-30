use std::io::Write;
use std::path::Path;

use super::args::{HostCommand, InitArgs};
use super::render::Output;
use super::{Environment, agents, prompt};
use crate::app::setup::{self, Bound, Joining};
use crate::app::{Deps, Failure};
use crate::wiring;

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
        .then(|| agents::Placing::on_this_host(environment))
        .transpose()?;
    agents::initialized(placing, json, || bind(&deps))
}
