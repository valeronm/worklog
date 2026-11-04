use std::path::{Path, PathBuf};

use super::agents::{Done, Fish, Placed, Refreshing};
use super::args::Shell;
use super::render::{Output, Stopped};
use super::{Environment, NAME, complete, said, wired};
use worklog_app::Failure;

fn fish_completions(
    environment: &Environment,
    file: PathBuf,
    homes: &[&Path],
    done: &mut Done,
) -> Result<Fish, Failure> {
    let binary = environment.binary()?;
    if !complete::runs_as_the_command(binary) {
        done.notes.push(format!(
            "{}: fish's completions are not written, since the binary's file name is not {NAME}",
            binary.display()
        ));
        return Ok(Fish { file, line: None });
    }
    let line = complete::registration(Shell::Fish, binary, homes)?;
    Ok(Fish {
        file,
        line: Some(line),
    })
}

pub(super) fn refresh_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let homes = worklog_wiring::homes(environment.variable)?;
    let homes: Vec<&Path> = homes.iter().map(PathBuf::as_path).collect();
    let file = worklog_wiring::fish_completions(environment.variable, NAME)?;
    // A binary whose path is unknown or cannot be quoted is refused before the first write.
    let fish = match file {
        Some(file) => Some(fish_completions(environment, file, &homes, done)?),
        None => None,
    };
    if let Some(refreshing) = Refreshing::on_this_host(environment, fish, done)? {
        refreshing.write_into(environment, done)?;
    }
    if let Err(failure) = wired(environment) {
        done.notes.push(said(&failure));
    }
    Ok(())
}

pub(super) fn run(environment: &Environment, json: bool) -> Result<Output, Stopped> {
    let mut done = Done::default();
    let ended = refresh_into(environment, &mut done);
    done.ended(ended, json, |written| Placed { written })
}
