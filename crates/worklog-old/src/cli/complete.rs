//! The values a shell offers for an argument, served by the binary itself
//! when asked with `COMPLETE=<shell>`.

use std::path::Path;

use clap::CommandFactory as _;
use clap::builder::StyledStr;
use clap_complete::{ArgValueCandidates, CompleteEnv, CompletionCandidate};

use crate::app::{Deps, Failure, read, write};
use crate::fs::Paths;

use super::args::Cli;
use super::{Opened, help};

/// Answers a shell's completion request when there is one, with the exit
/// code to end on; `None` when the process runs a command.
#[must_use]
pub fn answer() -> Option<i32> {
    let cwd = std::env::current_dir().ok();
    match CompleteEnv::with_factory(|| help::grouped(Cli::command()))
        .try_complete(std::env::args_os(), cwd.as_deref())
    {
        Ok(true) => Some(0),
        Ok(false) => None,
        Err(e) => {
            eprintln!("worklog: {e}");
            Some(1)
        }
    }
}

/// The line a shell's startup file holds to take completions from the
/// binary at this path.
pub fn registration(shell: clap_complete::Shell, exe: &Path) -> Result<String, Failure> {
    let exe = exe.display();
    match shell {
        clap_complete::Shell::Fish => Ok(format!("COMPLETE=fish \"{exe}\" | source\n")),
        clap_complete::Shell::Bash | clap_complete::Shell::Zsh => {
            Ok(format!("source <(COMPLETE={shell} \"{exe}\")\n"))
        }
        other => Err(Failure::Refused(format!("no completions for {other}"))),
    }
}

/// Every live document.
#[must_use]
pub fn slugs() -> ArgValueCandidates {
    from_store(|deps| {
        Ok(read::all(deps)?
            .rows
            .into_iter()
            .map(|row| candidate(row.slug, row.summary))
            .collect())
    })
}

/// Every draft on this machine.
#[must_use]
pub fn drafts() -> ArgValueCandidates {
    from_store(|deps| {
        Ok(write::drafts(deps)?
            .drafts
            .into_iter()
            .map(|draft| candidate(draft.slug, draft.kind))
            .collect())
    })
}

#[must_use]
pub fn topics() -> ArgValueCandidates {
    from_store(|deps| {
        Ok(read::topics(deps)?
            .topics
            .into_iter()
            .map(|t| candidate(t.slug, t.summary))
            .collect())
    })
}

/// The followups still open.
#[must_use]
pub fn followups() -> ArgValueCandidates {
    from_store(|deps| {
        Ok(read::followups(deps, None, false)?
            .items
            .into_iter()
            .filter(|item| item.source == "followup")
            .map(|item| candidate(item.slug, item.summary))
            .collect())
    })
}

#[must_use]
pub fn facts() -> ArgValueCandidates {
    from_store(|deps| {
        Ok(read::facts(deps, None, false)?
            .facts
            .into_iter()
            .map(|row| candidate(row.slug, row.summary))
            .collect())
    })
}

#[must_use]
pub fn tags() -> ArgValueCandidates {
    from_store(|deps| {
        Ok(read::tags(deps)?
            .tags
            .into_iter()
            .map(|t| candidate(t.name, t.count.to_string()))
            .collect())
    })
}

fn candidate(value: String, help: impl Into<StyledStr>) -> CompletionCandidate {
    CompletionCandidate::new(value).help(Some(help.into()))
}

/// Candidates listed from the store when a shell asks, and none rather
/// than a diagnosis when it cannot be read.
fn from_store(
    list: impl Fn(&Deps) -> Result<Vec<CompletionCandidate>, Failure> + Send + Sync + 'static,
) -> ArgValueCandidates {
    ArgValueCandidates::new(move || {
        let Ok(paths) = Paths::from_env() else {
            return Vec::new();
        };
        let Ok(Some(opened)) = Opened::open(paths) else {
            return Vec::new();
        };
        list(&opened.deps()).unwrap_or_default()
    })
}
