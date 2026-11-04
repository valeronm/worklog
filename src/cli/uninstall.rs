use super::agents::{self, Done, Placed};
use super::render::{Output, Stopped};
use super::{Environment, NAME};
use worklog_app::Failure;
use worklog_wiring::HostFiles;

fn uninstall_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let host = HostFiles::closed(environment.variable, NAME)?;
    agents::uninstall_into(environment, done)?;
    let uninstalled = host.remove()?;
    for path in &uninstalled.removed {
        done.wrote(path);
    }
    for dir in &uninstalled.left {
        done.notes.push(format!(
            "{}: left in place, since it is not empty",
            dir.display()
        ));
    }
    if let Ok(binary) = environment.binary() {
        done.notes.push(format!(
            "{}: this binary stays; delete it to finish",
            binary.display()
        ));
    }
    Ok(())
}

pub(super) fn run(environment: &Environment, json: bool) -> Result<Output, Stopped> {
    let mut done = Done::default();
    let ended = uninstall_into(environment, &mut done);
    done.ended(ended, json, |written| Placed { written })
}
