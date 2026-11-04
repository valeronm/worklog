use super::agents::{Done, Placed, Refreshing};
use super::render::{Output, Stopped};
use super::{Environment, NAME, said, wired};
use worklog_app::Failure;

pub(super) fn refresh_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let fish = worklog_wiring::fish_completions(environment.variable, NAME)?;
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
