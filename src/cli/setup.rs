//! What runs before a store exists: naming the host, and placing the skill
//! and the session hook where each coding agent reads them.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::app::Failure;
use crate::domain::machine::MachineName;
use crate::fs::{Agent, Config, Paths};

use super::Rendered;
use super::args::{AgentsWhat, SetupCommand};
use super::hook;

/// The agent skill, compiled in so it always describes this binary's
/// commands; `tests/skill.rs` holds it to that.
pub const SKILL: &str = include_str!("../../skill/SKILL.md");

/// One answer from the terminal, or the default on an empty line.
fn ask(prompt: &str, default: &str) -> Result<String, Failure> {
    eprint!("{prompt} [{default}]: ");
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Failure::Refused(format!("cannot read the answer: {e}")))?;
    let answer = line.trim();
    Ok(if answer.is_empty() {
        default.to_owned()
    } else {
        answer.to_owned()
    })
}

/// A yes or no: the flag when one was given, the terminal's answer when
/// asking is allowed, and no otherwise.
fn consent(flag: Option<bool>, may_ask: bool, prompt: &str) -> Result<bool, Failure> {
    match flag {
        Some(choice) => Ok(choice),
        None if may_ask => {
            let answer = ask(prompt, "y")?;
            Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
        }
        None => Ok(false),
    }
}

/// The hostname as a default to offer, never as the identity itself.
fn hostname() -> Option<String> {
    let out = std::process::Command::new("uname")
        .arg("-n")
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?;
    let name = name.trim().trim_end_matches(".local").to_ascii_lowercase();
    (!name.is_empty()).then_some(name)
}

fn line(path: &Path) -> String {
    format!("{}\n", path.display())
}

/// Each of the agent operations returns what it touched, one path per
/// line, which is what the command prints.
fn install_agent(agent: &Agent) -> Result<String, Failure> {
    let skill = line(&agent.write_skill(SKILL)?);
    let hooks = if hook::install(agent)? {
        line(&agent.hooks)
    } else {
        format!(
            "{}: a SessionStart hook already runs worklog context\n",
            agent.hooks.display()
        )
    };
    Ok(skill + &hooks)
}

fn uninstall_agent(agent: &Agent) -> Result<String, Failure> {
    let mut removed = String::new();
    if let Some(dir) = agent.remove_skill()? {
        removed.push_str(&line(&dir));
    }
    if hook::uninstall(agent)? {
        removed.push_str(&line(&agent.hooks));
    }
    Ok(removed)
}

/// Brings what an agent already has up to this binary: the skill's text
/// and the hook's path.
fn refresh_agent(agent: &Agent) -> Result<String, Failure> {
    let mut written = String::new();
    if agent.read_skill()?.is_some_and(|text| text != SKILL) {
        written.push_str(&line(&agent.write_skill(SKILL)?));
    }
    if hook::refresh(agent)? {
        written.push_str(&line(&agent.hooks));
    }
    Ok(written)
}

/// Brings everything on this host that comes from the binary up to it:
/// each agent's skill and hook, and each present shell's completions.
pub(super) fn refresh(paths: &Paths) -> Result<String, Failure> {
    let mut written = each_agent(&paths.agents, refresh_agent)?;
    for shell in paths.present_shells() {
        let file = shell.write_completions(&completions(shell.kind)?)?;
        written.push_str(&line(&file));
    }
    Ok(written)
}

fn completions(shell: clap_complete::Shell) -> Result<String, Failure> {
    super::complete::registration(shell, &super::this_binary()?)
}

fn each_agent<'a>(
    agents: impl IntoIterator<Item = &'a Agent>,
    change: impl Fn(&Agent) -> Result<String, Failure>,
) -> Result<String, Failure> {
    agents.into_iter().map(change).collect()
}

fn agent_names<'a>(agents: impl IntoIterator<Item = &'a Agent>, joint: &str) -> String {
    agents
        .into_iter()
        .map(|agent| agent.name)
        .collect::<Vec<_>>()
        .join(joint)
}

/// The agents given, or the refusal an install with none of them gets.
fn any_or_refuse<'a>(paths: &Paths, agents: Vec<&'a Agent>) -> Result<Vec<&'a Agent>, Failure> {
    if agents.is_empty() {
        return Err(Failure::Refused(format!(
            "no {} home on this host",
            agent_names(&paths.agents, " or ")
        )));
    }
    Ok(agents)
}

/// Records the machine name and the store directory, once per host, and
/// places the skill and the hook when told to. Returns what was written,
/// one path per line.
fn init(
    paths: &Paths,
    machine: Option<&str>,
    store: Option<&str>,
    agents: Option<bool>,
) -> Result<String, Failure> {
    if let Some(existing) = Config::read(&paths.config)? {
        return Err(Failure::Refused(format!(
            "this machine is already named {} with its store at {}",
            existing.machine,
            existing.store.display()
        )));
    }
    let interactive = machine.is_none();
    let (machine, store) = match machine {
        Some(machine) => (machine.to_owned(), store.map(str::to_owned)),
        None if std::io::stdin().is_terminal() => {
            let machine = ask("Machine name", &hostname().unwrap_or_default())?;
            let store = ask(
                "Store directory",
                &paths.default_store.display().to_string(),
            )?;
            (machine, Some(store))
        }
        None => {
            return Err(Failure::Usage(
                "init needs a machine name when not run from a terminal".into(),
            ));
        }
    };
    let present = paths.present_agents();
    let install_agents_too = consent(
        agents,
        interactive && !present.is_empty(),
        &format!(
            "Install the skill and the SessionStart hook for {}?",
            agent_names(present.iter().copied(), " and ")
        ),
    )?;
    // An install with nowhere to go refuses before the config is written.
    let agents = if install_agents_too {
        any_or_refuse(paths, present)?
    } else {
        Vec::new()
    };
    let machine = MachineName::parse(&machine)?;
    let store = match store.as_deref() {
        Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
        Some(dir) => std::env::current_dir()
            .map_err(|e| Failure::Refused(format!("no working directory: {e}")))?
            .join(dir),
        None => paths.default_store.clone(),
    };
    Config { machine, store }.write(&paths.config)?;
    Ok(line(&paths.config) + &each_agent(agents, install_agent)?)
}

/// Runs a setup command and returns what it prints.
pub(super) fn run(paths: &Paths, command: &SetupCommand) -> Result<Rendered, Failure> {
    let text = match command {
        SetupCommand::Upgrade { check } => return super::upgrade::run(paths, *check),
        SetupCommand::Init {
            machine,
            store,
            agents,
            no_agents,
        } => {
            let choice = match (agents, no_agents) {
                (true, _) => Some(true),
                (_, true) => Some(false),
                _ => None,
            };
            init(paths, machine.as_deref(), store.as_deref(), choice)?
        }
        SetupCommand::Agents { what } => match what {
            AgentsWhat::Install => {
                each_agent(any_or_refuse(paths, paths.present_agents())?, install_agent)?
            }
            AgentsWhat::Refresh => refresh(paths)?,
            AgentsWhat::Uninstall => each_agent(&paths.agents, uninstall_agent)?,
        },
        SetupCommand::Completions { shell } => completions(*shell)?,
    };
    Ok(Rendered { text, exit: 0 })
}
