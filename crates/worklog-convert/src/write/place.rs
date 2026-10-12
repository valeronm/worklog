use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::compose::Composed;
use super::verify::verify;
use super::{Failure, Leftover};
use crate::old::Kind;

const STAGING_SUFFIX: &str = ".partial";

// Only `checked` makes one, so a store is staged and placed nowhere that touches the store
// being converted.
pub(crate) struct Place {
    target: PathBuf,
    staging: PathBuf,
}

fn unusable(path: &Path, why: impl Into<String>) -> Failure {
    Failure::Target {
        path: path.display().to_string(),
        why: why.into(),
    }
}

fn io(path: &Path, error: &std::io::Error) -> Failure {
    Failure::Io {
        path: path.display().to_string(),
        reason: error.to_string(),
    }
}

fn first_in(directory: &Path) -> std::io::Result<Option<OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        names.push(entry?.file_name());
    }
    names.sort();
    Ok(names.into_iter().next())
}

// The permissions of a target that is there, which is a directory holding nothing.
fn held(target: &Path) -> Result<Option<fs::Permissions>, Failure> {
    let permissions = match fs::symlink_metadata(target) {
        Ok(found) if found.is_dir() => found.permissions(),
        Ok(found) if found.is_symlink() => {
            let why = "is a symbolic link; give the directory it leads to";
            return Err(unusable(target, why));
        }
        Ok(_) => return Err(unusable(target, "is not a directory")),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(target, &error)),
    };
    match first_in(target).map_err(|error| io(target, &error))? {
        Some(held) => {
            let held = held.to_string_lossy();
            Err(unusable(target, format!("is not empty: it holds {held}")))
        }
        None => Ok(Some(permissions)),
    }
}

fn parent_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

// A path that is not there yet resolves as its name under its directory.
fn resolved(path: &Path) -> std::io::Result<PathBuf> {
    match (fs::canonicalize(path), path.file_name()) {
        (Ok(whole), _) => Ok(whole),
        (Err(_), Some(name)) => Ok(fs::canonicalize(parent_of(path))?.join(name)),
        (Err(error), None) => Err(error),
    }
}

fn touching(path: &Path, old: &Path) -> Option<&'static str> {
    if path == old {
        Some("is")
    } else if path.starts_with(old) {
        Some("is inside")
    } else if old.starts_with(path) {
        Some("holds")
    } else {
        None
    }
}

impl Place {
    pub(crate) fn target(&self) -> &Path {
        &self.target
    }

    // The target is absent or a directory holding nothing. Neither it nor the directory the
    // store is staged in is, holds or is inside `old`, the store being converted, or a
    // kind's directory that `old` keeps through a symbolic link.
    pub(crate) fn checked(target: &Path, old: &Path) -> Result<Place, Failure> {
        let Some(name) = target.file_name() else {
            return Err(unusable(target, "names no directory"));
        };
        let parent = parent_of(target);
        if !parent.is_dir() {
            return Err(unusable(parent, "is not a directory"));
        }
        let mut staging = name.to_owned();
        staging.push(STAGING_SUFFIX);
        let place = Place {
            target: target.to_path_buf(),
            staging: parent.join(staging),
        };
        place.apart_from(old)?;
        held(target)?;
        Ok(place)
    }

    fn apart_from(&self, old: &Path) -> Result<(), Failure> {
        const NEVER: &str = "the store being converted, which is never written to";
        let old = fs::canonicalize(old).map_err(|error| io(old, &error))?;
        let target = resolved(&self.target).map_err(|error| io(&self.target, &error))?;
        let staging = resolved(&self.staging).map_err(|error| io(&self.staging, &error))?;
        let mut parts = vec![(old.clone(), NEVER.to_owned())];
        // The store's reader opens a kind's directory through a link.
        for kind in Kind::ALL {
            let linked = fs::canonicalize(old.join(kind.word()));
            if let Some(linked) = linked.ok().filter(|linked| !linked.starts_with(&old)) {
                parts.push((linked, format!("the {kind} directory of {NEVER}")));
            }
        }
        for (part, never) in &parts {
            if let Some(how) = touching(&target, part) {
                return Err(unusable(&self.target, format!("{how} {never}")));
            }
        }
        for (part, never) in &parts {
            if let Some(how) = touching(&staging, part) {
                let why = format!(
                    "is staged at {} before it is moved to its place, and that {how} {never}",
                    self.staging.display()
                );
                return Err(unusable(&self.target, why));
            }
        }
        Ok(())
    }

    fn in_the_way(&self, composed: &[Composed]) -> Failure {
        let staging = &self.staging;
        let is_directory = fs::symlink_metadata(staging).is_ok_and(|found| found.is_dir());
        let left = if !is_directory {
            Leftover::Other
        } else if verify(composed, staging).is_ok() {
            Leftover::Whole {
                target_stands: self.target.exists(),
            }
        } else {
            Leftover::Part
        };
        Failure::InTheWay {
            path: staging.display().to_string(),
            target: self.target.display().to_string(),
            left,
        }
    }

    pub(super) fn stage(&self, composed: &[Composed]) -> Result<Staging<'_>, Failure> {
        match fs::create_dir(&self.staging) {
            Ok(()) => Ok(Staging {
                place: self,
                removed_on_drop: true,
            }),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                Err(self.in_the_way(composed))
            }
            Err(error) => Err(io(&self.staging, &error)),
        }
    }
}

pub(super) struct Staging<'p> {
    place: &'p Place,
    removed_on_drop: bool,
}

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        if self.removed_on_drop {
            let _ = fs::remove_dir_all(&self.place.staging);
        }
    }
}

impl Staging<'_> {
    pub(super) fn path(&self) -> &Path {
        &self.place.staging
    }

    // A rename puts a directory in the place of an empty one in one step, so the target
    // holds nothing or the whole store at every moment.
    fn swap(&self) -> Result<(), Failure> {
        let (staging, target) = (&self.place.staging, &self.place.target);
        if let Some(permissions) = held(target)? {
            fs::set_permissions(staging, permissions).map_err(|error| io(staging, &error))?;
        }
        fs::rename(staging, target).map_err(|error| io(target, &error))
    }

    pub(super) fn settle(mut self) -> Result<(), Failure> {
        match self.swap() {
            Ok(()) => {
                self.removed_on_drop = false;
                Ok(())
            }
            Err(cause) => Err(self.abandon(cause)),
        }
    }

    pub(super) fn abandon(mut self, cause: Failure) -> Failure {
        self.removed_on_drop = false;
        match fs::remove_dir_all(&self.place.staging) {
            Ok(()) => cause,
            Err(error) if error.kind() == ErrorKind::NotFound => cause,
            Err(error) => Failure::Left {
                path: self.place.staging.display().to_string(),
                reason: error.to_string(),
                cause: Box::new(cause),
            },
        }
    }
}
