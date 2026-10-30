use std::path::{Path, PathBuf};

use super::agents::{Done, Placed, Refreshing};
use super::args::Shell;
use super::render::{Output, Stopped};
use super::{Environment, NAME, complete, said, wired};
use crate::app::Failure;
use crate::wiring;

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
    let homes = wiring::homes(environment.variable)?;
    let homes: Vec<&Path> = homes.iter().map(PathBuf::as_path).collect();
    // A binary whose path is unknown or cannot be quoted is refused before the first write.
    let completions = fish_completions(environment, &homes, done)?;
    let agents = Refreshing::on_this_host(environment)?;
    agents.write_into(done)?;
    if let Some((file, line)) = completions {
        wiring::write_completions(&file, &line)?;
        done.wrote(&file);
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
