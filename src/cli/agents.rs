use std::path::{Path, PathBuf};

use serde::Serialize;

use super::args::{AgentsAction, Shell};
use super::render::{Output, Render, Stopped, output};
use super::skill::SKILL;
use super::{Environment, NAME, complete, hook};
use crate::app::Failure;
use crate::app::setup::Bound;
use crate::domain::id::DocumentId;
use crate::wiring::{self, Agent};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Placed {
    pub written: Vec<String>,
}

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

pub(super) fn lines(written: &[String]) -> String {
    written
        .iter()
        .flat_map(|path| [path.as_str(), "\n"])
        .collect()
}

impl Render for Placed {
    fn text(&self) -> String {
        lines(&self.written)
    }
}

impl Render for Initialized {
    fn text(&self) -> String {
        let how = if self.created { "created" } else { "bound" };
        format!("{} {how}\n{}", self.label, lines(&self.written))
    }
}

#[derive(Default)]
pub(super) struct Done {
    pub(super) written: Vec<String>,
    pub(super) notes: Vec<String>,
}

impl Done {
    fn wrote(&mut self, path: &Path) {
        self.written.push(path.display().to_string());
    }

    fn hooks(&mut self, agent: &Agent, planned: Option<&str>) -> Result<bool, Failure> {
        if let Some(text) = planned {
            agent.write_hooks(text)?;
            self.wrote(agent.hooks());
        }
        Ok(planned.is_some())
    }

    /// The notes are given under `json` too, since the value printed there carries none.
    pub(super) fn output(self, bound: Option<Bound>, json: bool) -> Output {
        let mut made = match bound {
            Some(bound) => output(&Initialized::new(bound, self.written), json),
            None => output(
                &Placed {
                    written: self.written,
                },
                json,
            ),
        };
        made.notes = self.notes;
        made
    }

    fn ended(self, ended: Result<(), Failure>, json: bool) -> Result<Output, Stopped> {
        let done = self.output(None, json);
        match ended {
            Ok(()) => Ok(done),
            Err(failure) => Err(Stopped {
                done: Some(done),
                failure,
            }),
        }
    }
}

fn present(known: Vec<Agent>) -> Result<Vec<Agent>, Failure> {
    let names: Vec<&str> = known.iter().map(Agent::name).collect();
    let present: Vec<Agent> = known.into_iter().filter(Agent::is_present).collect();
    if present.is_empty() {
        let (last, first) = names.split_last().expect("agents the tool knows");
        let joined = if first.is_empty() {
            (*last).to_owned()
        } else {
            format!("{} or {last}", first.join(", "))
        };
        return Err(Failure::Refused(format!("no {joined} home on this host")));
    }
    Ok(present)
}

/// The agents present on this host, by name; none on a host with no home directory.
pub(super) fn present_names(environment: &Environment) -> Result<Vec<&'static str>, Failure> {
    if wiring::homes(environment.variable)?.is_empty() {
        return Ok(Vec::new());
    }
    let known = wiring::agents(environment.variable)?;
    let present = known.iter().filter(|agent| agent.is_present());
    Ok(present.map(Agent::name).collect())
}

type Planned = Vec<(Agent, Option<String>)>;

// Every agent's hooks file is read and edited in memory before the first write.
fn planned(
    agents: Vec<Agent>,
    hooks: impl Fn(&Agent) -> Result<Option<String>, Failure>,
) -> Result<Planned, Failure> {
    agents
        .into_iter()
        .map(|agent| hooks(&agent).map(|text| (agent, text)))
        .collect()
}

/// An install with every refusal behind it: the binary's path is known and makes a command,
/// an agent is present, and each hooks file was edited in memory.
pub(super) struct Placing(Planned);

impl Placing {
    pub(super) fn on_this_host(environment: &Environment) -> Result<Placing, Failure> {
        let binary = environment.binary()?;
        let agents = present(wiring::agents(environment.variable)?)?;
        planned(agents, |agent| hook::installed(agent, binary)).map(Placing)
    }

    fn install_into(&self, done: &mut Done) -> Result<(), Failure> {
        for (agent, hooks) in &self.0 {
            done.wrote(&agent.write_skill(SKILL)?);
            if !done.hooks(agent, hooks.as_deref())? {
                done.notes.push(format!(
                    "{}: a SessionStart hook already runs {NAME} context",
                    agent.hooks().display()
                ));
            }
        }
        Ok(())
    }

    pub(super) fn install(&self) -> Result<Done, Failure> {
        let mut done = Done::default();
        self.install_into(&mut done)?;
        Ok(done)
    }
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
    Ok(done.output(Some(bound), json))
}

fn fish_completions(
    environment: &Environment,
    homes: &[&Path],
    done: &mut Done,
) -> Result<Option<(PathBuf, String)>, Failure> {
    let Some(file) = wiring::fish_completions(environment.variable, NAME)? else {
        return Ok(None);
    };
    let binary = environment.binary()?;
    if !complete::runs_as_the_command(binary) {
        done.notes.push(format!(
            "{}: fish's completions are not written, since the binary's file name is not {NAME}",
            binary.display()
        ));
        return Ok(None);
    }
    let line = complete::registration(Shell::Fish, binary, homes)?;
    Ok(Some((file, line)))
}

pub(super) fn refresh_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let agents = wiring::agents(environment.variable)?;
    let binary = || environment.binary();
    let agents = planned(agents, |agent| hook::refreshed(agent, binary))?;
    // A binary whose path is unknown or cannot be quoted is refused before the first write.
    let homes = agents.first().map(|(agent, _)| agent.homes());
    let completions = fish_completions(environment, &homes.unwrap_or_default(), done)?;
    for (agent, hooks) in &agents {
        if agent.read_skill()?.is_some_and(|held| held != SKILL) {
            done.wrote(&agent.write_skill(SKILL)?);
        }
        done.hooks(agent, hooks.as_deref())?;
    }
    if let Some((file, line)) = completions {
        wiring::write_completions(&file, &line)?;
        done.wrote(&file);
    }
    Ok(())
}

fn uninstall_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let agents = wiring::agents(environment.variable)?;
    for (agent, hooks) in planned(agents, hook::uninstalled)? {
        if let Some(dir) = agent.remove_skill()? {
            done.wrote(&dir);
        }
        done.hooks(&agent, hooks.as_deref())?;
    }
    Ok(())
}

pub(super) fn run(
    action: AgentsAction,
    environment: &Environment,
    json: bool,
) -> Result<Output, Stopped> {
    let mut done = Done::default();
    let ended = match action {
        AgentsAction::Install => {
            Placing::on_this_host(environment).and_then(|placing| placing.install_into(&mut done))
        }
        AgentsAction::Refresh => refresh_into(environment, &mut done),
        AgentsAction::Uninstall => uninstall_into(environment, &mut done),
    };
    done.ended(ended, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::atlas;

    #[test]
    fn a_result_is_its_paths_one_to_a_line_and_init_puts_them_after_its_own_line() {
        let written = vec!["/home/desk/a".to_owned(), "/home/desk/b".to_owned()];
        assert_eq!(
            Placed {
                written: written.clone()
            }
            .text(),
            "/home/desk/a\n/home/desk/b\n"
        );
        assert_eq!(
            Placed {
                written: Vec::new()
            }
            .text(),
            ""
        );
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
