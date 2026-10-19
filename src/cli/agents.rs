use std::path::{Path, PathBuf};

use serde::Serialize;

use super::args::AgentsAction;
use super::render::{Output, Render, Stopped, output};
use super::skill::SKILL;
use super::{Environment, NAME, hook, sandbox};
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
    pub(super) fn wrote(&mut self, path: &Path) {
        self.written.push(path.display().to_string());
    }

    fn hooks(&mut self, agent: &Agent, planned: Option<&str>) -> Result<bool, Failure> {
        if let Some(text) = planned {
            agent.write_hooks(text)?;
            self.wrote(agent.hooks());
        }
        Ok(planned.is_some())
    }

    fn config(&mut self, agent: &Agent, planned: Option<&str>) -> Result<(), Failure> {
        if let Some(text) = planned
            && let Some(file) = agent.write_config(text)?
        {
            self.wrote(&file);
        }
        Ok(())
    }

    fn granted(
        &mut self,
        agent: &Agent,
        planned: Option<&str>,
        drafts: &Path,
    ) -> Result<(), Failure> {
        if planned.is_some() {
            wiring::make_drafts(drafts)?;
        }
        self.config(agent, planned)
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

    pub(super) fn ended(self, ended: Result<(), Failure>, json: bool) -> Result<Output, Stopped> {
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

/// An agent's hooks file and config, each as it is to be written, or `None` where it stays.
struct Planned {
    agent: Agent,
    hooks: Option<String>,
    config: Option<String>,
}

// Every agent's hooks file and config is read and edited in memory before the first write.
fn planned(
    agents: Vec<Agent>,
    hooks: impl Fn(&Agent) -> Result<Option<String>, Failure>,
    config: impl Fn(&Agent) -> Result<Option<String>, Failure>,
) -> Result<Vec<Planned>, Failure> {
    agents
        .into_iter()
        .map(|agent| {
            Ok(Planned {
                hooks: hooks(&agent)?,
                config: config(&agent)?,
                agent,
            })
        })
        .collect()
}

/// An install with every refusal behind it: the binary's path is known and makes a command,
/// an agent is present, and each hooks file and config was edited in memory.
pub(super) struct Placing {
    planned: Vec<Planned>,
    drafts: PathBuf,
}

impl Placing {
    pub(super) fn on_this_host(environment: &Environment) -> Result<Placing, Failure> {
        let binary = environment.binary()?;
        let agents = present(wiring::agents(environment.variable)?)?;
        let drafts = wiring::drafts(environment.variable)?;
        let planned = planned(
            agents,
            |agent| hook::installed(agent, binary),
            |agent| sandbox::installed(agent, &drafts),
        )?;
        Ok(Placing { planned, drafts })
    }

    fn install_into(&self, done: &mut Done) -> Result<(), Failure> {
        for Planned {
            agent,
            hooks,
            config,
        } in &self.planned
        {
            done.wrote(&agent.write_skill(SKILL)?);
            if let Some(rule) = agent.write_rule()? {
                done.wrote(&rule);
            }
            done.granted(agent, config.as_deref(), &self.drafts)?;
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

/// A refresh planned and not yet written: each hooks file and config was edited in memory.
pub(super) struct Refreshing {
    refreshed: Vec<Refreshed>,
    drafts: PathBuf,
}

struct Refreshed {
    agent: Agent,
    skill: Option<String>,
    /// Whether the skill or the hook is there, as on a host set up before the rule or the root
    /// existed.
    installed: bool,
    hooks: Option<String>,
    config: Option<String>,
}

impl Refreshing {
    pub(super) fn on_this_host(environment: &Environment) -> Result<Refreshing, Failure> {
        let agents = wiring::agents(environment.variable)?;
        let drafts = wiring::drafts(environment.variable)?;
        let refreshed = |agent: Agent| {
            let skill = agent.read_skill()?;
            let (holds_hook, hooks) = hook::refreshed(&agent, || environment.binary())?;
            let installed = skill.is_some() || holds_hook;
            let config = if installed {
                sandbox::installed(&agent, &drafts)?
            } else {
                None
            };
            Ok(Refreshed {
                agent,
                skill,
                installed,
                hooks,
                config,
            })
        };
        let refreshed = agents
            .into_iter()
            .map(refreshed)
            .collect::<Result<_, Failure>>()?;
        Ok(Refreshing { refreshed, drafts })
    }

    pub(super) fn write_into(&self, done: &mut Done) -> Result<(), Failure> {
        for refreshed in &self.refreshed {
            let agent = &refreshed.agent;
            if (refreshed.skill.as_deref()).is_some_and(|held| held != SKILL) {
                done.wrote(&agent.write_skill(SKILL)?);
            }
            if let Some(rule) = agent.refresh_rule(refreshed.installed)? {
                done.wrote(&rule);
            }
            done.granted(agent, refreshed.config.as_deref(), &self.drafts)?;
            done.hooks(agent, refreshed.hooks.as_deref())?;
        }
        Ok(())
    }
}

fn uninstall_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let agents = wiring::agents(environment.variable)?;
    let drafts = wiring::drafts(environment.variable)?;
    let uninstalled = |agent: &Agent| sandbox::uninstalled(agent, &drafts);
    for Planned {
        agent,
        hooks,
        config,
    } in planned(agents, hook::uninstalled, uninstalled)?
    {
        if let Some(dir) = agent.remove_skill()? {
            done.wrote(&dir);
        }
        if let Some(rule) = agent.remove_rule()? {
            done.wrote(&rule);
        }
        done.config(&agent, config.as_deref())?;
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
    let ended =
        match action {
            AgentsAction::Install => Placing::on_this_host(environment)
                .and_then(|placing| placing.install_into(&mut done)),
            AgentsAction::Refresh => Refreshing::on_this_host(environment)
                .and_then(|agents| agents.write_into(&mut done)),
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
