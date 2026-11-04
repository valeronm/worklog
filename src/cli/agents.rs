use std::path::{Path, PathBuf};

use serde::Serialize;

use super::args::AgentsAction;
use super::json::Pinned;
use super::render::{Output, Render, Stopped, output};
use super::sandbox::Edit;
use super::skill::SKILL;
use super::{Environment, NAME, hook, sandbox};
use worklog_app::Failure;
use worklog_wiring::Agent;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Placed {
    pub written: Vec<String>,
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

#[derive(Default)]
pub(super) struct Done {
    pub(super) written: Vec<String>,
    pub(super) notes: Vec<String>,
}

impl Done {
    pub(super) fn wrote(&mut self, path: &Path) {
        self.written.push(path.display().to_string());
    }

    // An edit is made from the file as it stands just before its write.
    fn hooks(
        &mut self,
        agent: &Agent,
        edit: impl FnOnce(&Agent) -> Result<Option<String>, Failure>,
    ) -> Result<bool, Failure> {
        let Some(text) = edit(agent)? else {
            return Ok(false);
        };
        self.wrote(&agent.write_hooks(&text)?);
        Ok(true)
    }

    fn config(
        &mut self,
        agent: &Agent,
        edit: impl FnOnce(&Agent) -> Result<Option<Edit>, Failure>,
    ) -> Result<(), Failure> {
        let Some(Edit {
            config,
            text,
            grants,
        }) = edit(agent)?
        else {
            return Ok(());
        };
        if let Some(root) = grants {
            worklog_wiring::make_drafts(&root)?;
        }
        self.wrote(&agent.write_config(&config, &text)?);
        Ok(())
    }

    pub(super) fn ended<T: Render + Serialize + Pinned>(
        self,
        ended: Result<(), Failure>,
        json: bool,
        value: impl FnOnce(Vec<String>) -> T,
    ) -> Result<Output, Stopped> {
        // The notes are given under `json` too, since the value printed there carries none.
        let mut done = output(&value(self.written), json);
        done.notes.extend(self.notes);
        match ended {
            Ok(()) => Ok(done),
            Err(failure) => Err(Stopped {
                done: Some(done),
                failure,
            }),
        }
    }
}

fn unfound(environment: &Environment) -> Result<Option<String>, Failure> {
    let binary = environment.binary()?;
    Ok((!(environment.found_on_path)(binary)).then(|| {
        format!(
            "{}: not the {NAME} that PATH finds, which is the one an agent runs",
            binary.display()
        )
    }))
}

pub(super) fn unoffered(environment: &Environment) -> Option<String> {
    let why = unfound(environment).ok().flatten()?;
    Some(format!(
        "{why}; `{NAME} agents install` sets the agents up once it is"
    ))
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
        return Err(Failure::Refused(format!(
            "no {joined} home on this machine"
        )));
    }
    Ok(present)
}

/// The agents present on this host, by name; none on a host with no home directory.
pub(super) fn present_names(environment: &Environment) -> Result<Vec<&'static str>, Failure> {
    if worklog_wiring::homes(environment.variable)?.is_empty() {
        return Ok(Vec::new());
    }
    let known = worklog_wiring::agents(environment.variable)?;
    let present = known.iter().filter(|agent| agent.is_present());
    Ok(present.map(Agent::name).collect())
}

// Every agent's hooks file and config is read and edited in memory before the first write.
fn checked(
    agents: &[Agent],
    hooks: impl Fn(&Agent) -> Result<Option<String>, Failure>,
    config: impl Fn(&Agent) -> Result<Option<Edit>, Failure>,
) -> Result<(), Failure> {
    for agent in agents {
        hooks(agent)?;
        config(agent)?;
    }
    Ok(())
}

/// An install with every refusal behind it: the binary's path is known, is the one `PATH` finds
/// and makes a command, an agent is present, and each hooks file and config was edited in
/// memory.
pub(super) struct Placing {
    agents: Vec<Agent>,
    binary: PathBuf,
    drafts: PathBuf,
}

impl Placing {
    pub(super) fn on_this_host(environment: &Environment) -> Result<Placing, Failure> {
        if let Some(why) = unfound(environment)? {
            return Err(Failure::Refused(why));
        }
        let binary = environment.binary()?.to_owned();
        let agents = present(worklog_wiring::agents(environment.variable)?)?;
        let drafts = worklog_wiring::drafts(environment.variable)?;
        let placing = Placing {
            agents,
            binary,
            drafts,
        };
        checked(
            &placing.agents,
            |agent| placing.hooks(agent),
            |agent| placing.config(agent),
        )?;
        Ok(placing)
    }

    fn hooks(&self, agent: &Agent) -> Result<Option<String>, Failure> {
        hook::installed(agent, &self.binary)
    }

    fn config(&self, agent: &Agent) -> Result<Option<Edit>, Failure> {
        sandbox::installed(agent, &self.drafts)
    }

    pub(super) fn install_into(&self, done: &mut Done) -> Result<(), Failure> {
        for agent in &self.agents {
            done.wrote(&agent.write_skill(SKILL)?);
            if let Some(rule) = agent.write_rule()? {
                done.wrote(&rule);
            }
            done.config(agent, |agent| self.config(agent))?;
            if !done.hooks(agent, |agent| self.hooks(agent))? {
                done.notes.push(format!(
                    "{}: a SessionStart hook already runs {NAME} context",
                    agent.hooks().display()
                ));
            }
        }
        Ok(())
    }
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
}

impl Refreshed {
    fn config(&self, drafts: &Path) -> Result<Option<Edit>, Failure> {
        if self.installed {
            sandbox::installed(&self.agent, drafts)
        } else {
            Ok(None)
        }
    }
}

impl Refreshing {
    pub(super) fn on_this_host(environment: &Environment) -> Result<Refreshing, Failure> {
        let agents = worklog_wiring::agents(environment.variable)?;
        let drafts = worklog_wiring::drafts(environment.variable)?;
        let refreshed = |agent: Agent| {
            let skill = agent.read_skill()?;
            let (holds_hook, _) = hook::refreshed(&agent, || environment.binary())?;
            let refreshed = Refreshed {
                installed: skill.is_some() || holds_hook,
                agent,
                skill,
            };
            refreshed.config(&drafts)?;
            Ok(refreshed)
        };
        let refreshed = agents
            .into_iter()
            .map(refreshed)
            .collect::<Result<_, Failure>>()?;
        Ok(Refreshing { refreshed, drafts })
    }

    /// Where an agent holds the skill or the hook, or `also_there` is set, a binary `PATH` does
    /// not find gets a note and no write.
    pub(super) fn goes_ahead(
        &self,
        environment: &Environment,
        also_there: bool,
        done: &mut Done,
    ) -> Result<bool, Failure> {
        let there = also_there || self.refreshed.iter().any(|refreshed| refreshed.installed);
        let Some(why) = there.then(|| unfound(environment)).transpose()?.flatten() else {
            return Ok(true);
        };
        done.notes.push(format!("{why}; nothing is refreshed"));
        Ok(false)
    }

    pub(super) fn write_into(
        &self,
        environment: &Environment,
        done: &mut Done,
    ) -> Result<(), Failure> {
        for refreshed in &self.refreshed {
            let agent = &refreshed.agent;
            if (refreshed.skill.as_deref()).is_some_and(|held| held != SKILL) {
                done.wrote(&agent.write_skill(SKILL)?);
            }
            if let Some(rule) = agent.refresh_rule(refreshed.installed)? {
                done.wrote(&rule);
            }
            done.config(agent, |_| refreshed.config(&self.drafts))?;
            done.hooks(agent, |agent| {
                Ok(hook::refreshed(agent, || environment.binary())?.1)
            })?;
        }
        Ok(())
    }
}

pub(super) fn uninstall_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let agents = worklog_wiring::agents(environment.variable)?;
    let drafts = worklog_wiring::drafts(environment.variable)?;
    let uninstalled = |agent: &Agent| sandbox::uninstalled(agent, &drafts);
    checked(&agents, hook::uninstalled, uninstalled)?;
    for agent in &agents {
        if let Some(dir) = agent.remove_skill()? {
            done.wrote(&dir);
        }
        if let Some(rule) = agent.remove_rule()? {
            done.wrote(&rule);
        }
        done.config(agent, uninstalled)?;
        done.hooks(agent, hook::uninstalled)?;
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
        AgentsAction::Refresh => Refreshing::on_this_host(environment).and_then(|agents| {
            if agents.goes_ahead(environment, false, &mut done)? {
                agents.write_into(environment, &mut done)?;
            }
            Ok(())
        }),
        AgentsAction::Uninstall => uninstall_into(environment, &mut done),
    };
    done.ended(ended, json, |written| Placed { written })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;

    use super::*;
    use crate::cli::tests::environment;

    #[test]
    fn an_edit_to_a_hooks_file_after_the_install_was_planned_is_kept() {
        let kept = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(kept.path()).unwrap();
        let home = root.join("home");
        fs::create_dir_all(home.join(".claude")).unwrap();
        let variable = |name: &str| (name == "HOME").then(|| OsString::from(&home));
        let binary = root.join("bin").join(NAME);
        let environment = Environment {
            binary: Some(binary.clone()),
            ..environment(&variable)
        };
        let placing = Placing::on_this_host(&environment).unwrap();
        let hooks = home.join(".claude").join("settings.json");
        fs::write(&hooks, "{\"theme\": \"dark\"}\n").unwrap();
        placing.install_into(&mut Done::default()).unwrap();
        themed_with_a_hook_running(&hooks, &binary);
    }

    #[test]
    fn an_edit_to_a_hooks_file_after_the_refresh_was_planned_is_kept() {
        let kept = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(kept.path()).unwrap();
        let home = root.join("home");
        fs::create_dir_all(home.join(".claude")).unwrap();
        let variable = |name: &str| (name == "HOME").then(|| OsString::from(&home));
        let installed = Environment {
            binary: Some(root.join("bin").join(NAME)),
            ..environment(&variable)
        };
        let placing = Placing::on_this_host(&installed).unwrap();
        placing.install_into(&mut Done::default()).unwrap();
        let moved = root.join("bench").join(NAME);
        let environment = Environment {
            binary: Some(moved.clone()),
            ..environment(&variable)
        };
        let refreshing = Refreshing::on_this_host(&environment).unwrap();
        let hooks = home.join(".claude").join("settings.json");
        let mut edited: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&hooks).unwrap()).unwrap();
        edited["theme"] = "dark".into();
        fs::write(&hooks, edited.to_string()).unwrap();
        refreshing
            .write_into(&environment, &mut Done::default())
            .unwrap();
        themed_with_a_hook_running(&hooks, &moved);
    }

    fn themed_with_a_hook_running(hooks: &Path, binary: &Path) {
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(hooks).unwrap()).unwrap();
        assert_eq!(written["theme"], "dark");
        let command = &written["hooks"]["SessionStart"][0]["hooks"][0]["command"];
        let runs = format!("\"{}\" context", binary.display());
        assert!(
            command
                .as_str()
                .is_some_and(|command| command.starts_with(&runs)),
            "{written}"
        );
    }

    #[test]
    fn a_result_is_its_paths_one_to_a_line() {
        let written = vec!["/home/desk/a".to_owned(), "/home/desk/b".to_owned()];
        assert_eq!(Placed { written }.text(), "/home/desk/a\n/home/desk/b\n");
        assert_eq!(
            Placed {
                written: Vec::new()
            }
            .text(),
            ""
        );
    }
}
