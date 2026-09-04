//! What runs before a store exists: naming the host, and placing the skill
//! and the session hook where the agent reads them.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::app::Failure;
use crate::domain::machine::MachineName;
use crate::fs::{Config, Paths, write_file};

use super::args::{HookWhat, SetupCommand, SkillWhat};
use super::hook::{self, Merged};

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

/// Writes the skill into a skills directory and returns the file.
pub fn install_skill(skills: &Path) -> Result<PathBuf, Failure> {
    let file = skills.join("worklog").join("SKILL.md");
    write_file(&file, SKILL)?;
    Ok(file)
}

/// Merges the hook into a settings file and says what happened.
fn install_hook(settings: &Path) -> Result<String, Failure> {
    Ok(match hook::install(settings)? {
        Merged::Added(_) => format!("{}\n", settings.display()),
        Merged::Present => format!(
            "{}: a SessionStart hook already runs worklog context\n",
            settings.display()
        ),
    })
}

/// Records the machine name and the store directory, once per host, and
/// places the skill and the hook when told to. Returns what was written,
/// one path per line.
fn init(
    paths: &Paths,
    machine: Option<&str>,
    store: Option<&str>,
    skill: Option<bool>,
    hook: Option<bool>,
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
    let install_skill_too = consent(
        skill,
        interactive,
        "Install the agent skill for Claude Code?",
    )?;
    let install_hook_too = consent(
        hook,
        interactive,
        "Add the SessionStart hook to Claude Code's settings?",
    )?;
    let machine = MachineName::parse(&machine)?;
    let store = match store.as_deref() {
        Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
        Some(dir) => std::env::current_dir()
            .map_err(|e| Failure::Refused(format!("no working directory: {e}")))?
            .join(dir),
        None => paths.default_store.clone(),
    };
    Config { machine, store }.write(&paths.config)?;
    let mut written = format!("{}\n", paths.config.display());
    if install_skill_too {
        let file = install_skill(&paths.agent_skills)?;
        written.push_str(&file.display().to_string());
        written.push('\n');
    }
    if install_hook_too {
        written.push_str(&install_hook(&paths.agent_settings)?);
    }
    Ok(written)
}

/// Runs a setup command and returns what it prints.
pub fn run(paths: &Paths, command: &SetupCommand) -> Result<String, Failure> {
    // A flag pair as a choice: set, unset, or nothing said.
    let choice = |yes: bool, no: bool| match (yes, no) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    };
    match command {
        SetupCommand::Init {
            machine,
            store,
            skill,
            no_skill,
            hook,
            no_hook,
        } => init(
            paths,
            machine.as_deref(),
            store.as_deref(),
            choice(*skill, *no_skill),
            choice(*hook, *no_hook),
        ),
        SetupCommand::Skill { what } => match what {
            SkillWhat::Show => Ok(SKILL.to_owned()),
            SkillWhat::Install { dir } => {
                let file = install_skill(dir.as_deref().unwrap_or(&paths.agent_skills))?;
                Ok(format!("{}\n", file.display()))
            }
        },
        SetupCommand::Hook { what } => match what {
            HookWhat::Show => super::pretty_json(&hook::entry()?),
            HookWhat::Install { settings } => {
                install_hook(settings.as_deref().unwrap_or(&paths.agent_settings))
            }
        },
    }
}
