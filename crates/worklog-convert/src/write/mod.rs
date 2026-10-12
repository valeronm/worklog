//! A complete plan written as a store of the new format, and held to that store's own check
//! before it stands at its place.

use std::fmt;
use std::path::Path;

use worklog_next::app::check::Finding;
use worklog_next::wiring::write_version;

use crate::old;
use crate::plan::Complete;

mod compose;
mod counts;
mod place;
mod rules;
#[cfg(test)]
mod tests;
mod verify;

pub(crate) use counts::Counts;
pub(crate) use place::Place;

use compose::{Composed, Defect, compose};
use counts::{Miscount, counted};
use place::Staging;
use verify::{Fault, verify};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The target cannot take a store, and is as it was.
    Target {
        path: String,
        why: String,
    },
    /// The directory the store is staged in is there already, and is left as found.
    InTheWay {
        path: String,
        target: String,
        left: Leftover,
    },
    Io {
        path: String,
        reason: String,
    },
    Defect(Defect),
    Miscount(Box<Miscount>),
    /// What was staged is not the planned store, or the new store's check faults it.
    Unsound(Vec<Fault>),
    // The staging directory is still there after `cause`.
    Left {
        path: String,
        reason: String,
        cause: Box<Failure>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Leftover {
    // Every version of this conversion and nothing else, as its verification reads it.
    Whole { target_stands: bool },
    Part,
    // No directory.
    Other,
}

fn in_the_way(
    f: &mut fmt::Formatter<'_>,
    path: &str,
    target: &str,
    left: &Leftover,
) -> fmt::Result {
    write!(
        f,
        "{path}: is in the way, where the store is staged before it is moved to its place. "
    )?;
    match left {
        Leftover::Whole { target_stands } => {
            let emptied = if *target_stands {
                format!("rmdir {target} && ")
            } else {
                String::new()
            };
            write!(
                f,
                "It holds the whole store this conversion writes, as the converter reads it \
                 back, left by a run that was interrupted. To put it in its place: \
                 {emptied}mv {path} {target}"
            )
        }
        Leftover::Part => write!(
            f,
            "It does not hold the whole store this conversion writes; a run that was \
             interrupted may have left it. Remove it and run again; it is not removed here"
        ),
        Leftover::Other => write!(f, "It is no directory, and is left as it is"),
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DEFECT: &str = "which is a defect of the converter and no question to answer";
        match self {
            Failure::Target { path, why } => write!(f, "{path}: {why}"),
            Failure::InTheWay { path, target, left } => in_the_way(f, path, target, left),
            Failure::Io { path, reason } => write!(f, "{path}: {reason}"),
            Failure::Defect(defect) => {
                write!(f, "the plan cannot be written, {DEFECT}: {defect}")
            }
            Failure::Miscount(miscount) => write!(
                f,
                "the plan does not account for the store, {DEFECT}: {miscount}"
            ),
            Failure::Unsound(faults) => {
                write!(f, "the store as planned is not sound, {DEFECT}:")?;
                faults.iter().try_for_each(|fault| write!(f, "\n  {fault}"))
            }
            Failure::Left {
                path,
                reason,
                cause,
            } => write!(f, "{cause}\n{path}: was not removed: {reason}"),
        }
    }
}

impl std::error::Error for Failure {}

pub(crate) struct Ready {
    composed: Vec<Composed>,
    counts: Counts,
}

// Reads no file and writes none.
pub(crate) fn ready(plan: &Complete, old: &old::Store) -> Result<Ready, Failure> {
    let composed = compose(plan).map_err(Failure::Defect)?;
    let counts = counted(old, plan).map_err(Failure::Miscount)?;
    Ok(Ready { composed, counts })
}

impl Ready {
    pub(crate) fn counts(&self) -> &Counts {
        &self.counts
    }

    // The target is as it was unless every version was written beside it, read back and
    // passed by the new store's check; the staging directory is gone after a failure that
    // does not name it.
    pub(crate) fn write(&self, place: &Place) -> Result<Vec<Finding>, Failure> {
        let staging = place.stage(&self.composed)?;
        stored(&self.composed, staging)
    }
}

fn stored(composed: &[Composed], staging: Staging) -> Result<Vec<Finding>, Failure> {
    match filled(composed, staging.path()) {
        Ok(notices) => staging.settle().map(|()| notices),
        Err(cause) => Err(staging.abandon(cause)),
    }
}

fn filled(composed: &[Composed], staging: &Path) -> Result<Vec<Finding>, Failure> {
    for version in composed.iter().flat_map(|document| &document.versions) {
        write_version(staging, version).map_err(|error| Failure::Io {
            path: staging.display().to_string(),
            reason: error.to_string(),
        })?;
    }
    verify(composed, staging).map_err(Failure::Unsound)
}
