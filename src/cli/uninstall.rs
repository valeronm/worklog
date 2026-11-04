use std::path::Path;

use super::agents::{Done, Placed, Uninstalling};
use super::render::{Output, Stopped};
use super::{Environment, NAME, wired};
use worklog_app::Failure;
use worklog_wiring::HostFiles;

// `drafts`, `save` and `discard` run only on a machine `wired` takes.
fn open_drafts(environment: &Environment, drafts: &Path) -> Failure {
    let why = match wired(environment) {
        Ok(_) => {
            format!("holds open drafts, which `{NAME} drafts` lists; save or discard each first")
        }
        Err(Failure::NotSetUp) => "holds open drafts this machine is no longer set up to save \
                                   or discard; delete them there first"
            .to_owned(),
        Err(failure) => format!("holds open drafts, and `{NAME} drafts` refuses: {failure}"),
    };
    Failure::at(drafts.display(), why)
}

fn uninstall_into(environment: &Environment, done: &mut Done) -> Result<(), Failure> {
    let agents = Uninstalling::on_this_host(environment)?;
    let open = |drafts: &Path| open_drafts(environment, drafts);
    let host = HostFiles::closed(environment.variable, NAME, open)?;
    agents.uninstall_into(done)?;
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
