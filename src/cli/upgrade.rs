//! `upgrade`, wired to the release source the paths select and rendered
//! under the output contract: what was written on stdout, what was
//! decided on stderr.

use crate::app::Failure;
use crate::app::upgrade::{self, Outcome};
use crate::domain::ports::Releases;
use crate::domain::release;
use crate::fs::{DirReleases, FsBinary, Paths};
use crate::net::GitHubReleases;

use super::{Rendered, setup};

pub(super) fn run(paths: &Paths, check: bool) -> Result<Rendered, Failure> {
    let releases: Box<dyn Releases> = match &paths.releases {
        Some(dir) => Box::new(DirReleases { dir: dir.clone() }),
        None => Box::new(GitHubReleases::new()),
    };
    let current = release::current();
    if check {
        let latest = upgrade::check(releases.as_ref())?;
        return Ok(Rendered {
            text: format!("current: {current}\nlatest: {latest}\n"),
            exit: i32::from(latest > current),
        });
    }
    let binary = FsBinary::running()?;
    let (outcome, written) = upgrade::run(releases.as_ref(), &binary, current, release::asset())?;
    // A binary that stays refreshes the host itself; a replaced one had
    // its successor do it, since only that one knows its own skill.
    let text = match outcome {
        Outcome::Current => {
            eprintln!("worklog: already at {current}");
            setup::refresh(paths)?
        }
        Outcome::Ahead(latest) => {
            eprintln!("worklog: {current} is newer than the latest release {latest}");
            setup::refresh(paths)?
        }
        Outcome::Upgraded(to) => {
            eprintln!("worklog: upgraded {current} to {to}");
            written
        }
    };
    Ok(Rendered { text, exit: 0 })
}
