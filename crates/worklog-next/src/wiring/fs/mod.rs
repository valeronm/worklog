//! The ports implemented on a directory tree.

mod agent;
mod binary;
mod clock;
mod config;
mod drafts;
mod host;
mod ids;
mod paths;
mod releases;
mod store;
mod usage;

pub use agent::{FsAgent, SKILL_DIRECTORY, replace_under};
pub use binary::FsBinary;
pub use clock::SystemClock;
pub use config::Config;
pub use drafts::FsDrafts;
pub use host::FsHost;
pub use ids::RandomIds;
pub use paths::{Paths, set};
pub use releases::DirReleases;
pub use store::FsStore;
pub use usage::{FsUsage, USAGE_DIRECTORY};

use std::fs::{File, OpenOptions, Permissions};
use std::io::{ErrorKind, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use crate::domain::ports::StoreError;

// The store's ignore file carries the same prefix.
const STAGING_PREFIX: &str = ".tmp-";

fn io_error(location: &Path, error: &std::io::Error) -> StoreError {
    StoreError::io(location.display(), error)
}

// Every file and directory here is created by its first write.
fn optional<T>(at: &Path, result: std::io::Result<T>) -> Result<Option<T>, StoreError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(at, &e)),
    }
}

// Two processes replacing one file must not write through one staged file.
fn staging_of(dir: &Path, name: &std::ffi::OsStr) -> PathBuf {
    let name = name.to_string_lossy();
    dir.join(format!("{STAGING_PREFIX}{name}-{}", std::process::id()))
}

// A link planted under the staged name is not followed, and a `private` file is its owner's
// alone until its bytes are whole.
fn open_staged(staging: &Path, private: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    if private {
        options.mode(0o600);
    }
    options.open(staging)
}

fn staged(staging: &Path, bytes: &[u8], permissions: Option<&Permissions>) -> std::io::Result<()> {
    let mut file = match open_staged(staging, permissions.is_some()) {
        // A run that was killed left its file, and its process id has been given out again.
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            std::fs::remove_file(staging)?;
            open_staged(staging, permissions.is_some())?
        }
        opened => opened?,
    };
    file.write_all(bytes)?;
    // A file renamed before its bytes reach the disk can be empty after a crash.
    file.sync_all()?;
    match permissions {
        Some(permissions) => file.set_permissions(permissions.clone()),
        None => Ok(()),
    }
}

/// Replaces `target` whole with `bytes`, through a file beside it that is gone again when the
/// write fails. The file ends with `permissions`, or without them with those of a new file.
fn replace(
    target: &Path,
    bytes: &[u8],
    permissions: Option<&Permissions>,
) -> Result<(), StoreError> {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(StoreError::io(target.display(), "is not a file's path"));
    };
    std::fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;
    // A rename replaces the file whole only from the same file system.
    let staging = staging_of(dir, name);
    let replaced = staged(&staging, bytes, permissions)
        .map_err(|e| io_error(&staging, &e))
        .and_then(|()| std::fs::rename(&staging, target).map_err(|e| io_error(target, &e)));
    if replaced.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    replaced
}

/// Sorted, and empty for a directory that is not there.
fn listed(dir: &Path, directories: bool) -> Result<Vec<String>, StoreError> {
    let mut names = Vec::new();
    for entry in optional(dir, std::fs::read_dir(dir))?.into_iter().flatten() {
        let entry = entry.map_err(|e| io_error(dir, &e))?;
        if entry.file_type().map_err(|e| io_error(dir, &e))?.is_dir() == directories {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

fn directories(dir: &Path) -> Result<Vec<String>, StoreError> {
    listed(dir, true)
}

fn files(dir: &Path) -> Result<Vec<String>, StoreError> {
    listed(dir, false)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn leftover(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!(".tmp-{name}-{}", std::process::id()))
    }

    #[test]
    fn a_file_is_staged_under_a_name_of_this_process_and_ends_with_the_mode_asked_for() {
        let kept = tempfile::tempdir().unwrap();
        let name = std::ffi::OsStr::new("lantern");
        assert_eq!(
            staging_of(kept.path(), name),
            leftover(kept.path(), "lantern")
        );
        let staging = kept.path().join("staged");
        let file = open_staged(&staging, true).unwrap();
        assert_eq!(file.metadata().unwrap().len(), 0);
        assert_eq!(mode(&staging), 0o600);
        assert!(open_staged(&staging, true).is_err());
        fs::remove_file(&staging).unwrap();

        for asked in [0o600, 0o664, 0o755] {
            let staging = kept.path().join(format!("staged-{asked:o}"));
            staged(&staging, b"atlas", Some(&Permissions::from_mode(asked))).unwrap();
            assert_eq!(mode(&staging), asked);
            assert_eq!(fs::read(&staging).unwrap(), b"atlas");
        }
    }

    #[test]
    fn a_file_left_staged_by_a_run_that_was_killed_does_not_block_the_next() {
        let kept = tempfile::tempdir().unwrap();
        let target = kept.path().join("lantern");
        fs::write(&target, "old").unwrap();
        let left = leftover(kept.path(), "lantern");
        fs::write(&left, "half").unwrap();
        fs::set_permissions(&left, Permissions::from_mode(0o444)).unwrap();
        replace(&target, b"new", Some(&Permissions::from_mode(0o640))).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(mode(&target), 0o640);
        assert_eq!(files(kept.path()).unwrap(), ["lantern"]);
    }

    #[test]
    fn a_link_planted_under_the_staged_name_is_not_written_through() {
        let kept = tempfile::tempdir().unwrap();
        let target = kept.path().join("lantern");
        fs::write(&target, "old").unwrap();
        let elsewhere = kept.path().join("atlas");
        fs::write(&elsewhere, "kept").unwrap();
        std::os::unix::fs::symlink(&elsewhere, leftover(kept.path(), "lantern")).unwrap();
        for permissions in [None, Some(Permissions::from_mode(0o640))] {
            replace(&target, b"new", permissions.as_ref()).unwrap();
            assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "kept");
            assert!(fs::symlink_metadata(&target).unwrap().is_file());
            assert_eq!(fs::read_to_string(&target).unwrap(), "new");
            assert_eq!(files(kept.path()).unwrap(), ["atlas", "lantern"]);
            let planted = leftover(kept.path(), "lantern");
            std::os::unix::fs::symlink(&elsewhere, planted).unwrap();
            fs::write(&target, "old").unwrap();
        }
    }

    #[test]
    fn a_rename_that_fails_leaves_the_target_as_it_was_and_no_staged_file() {
        let kept = tempfile::tempdir().unwrap();
        let target = kept.path().join("lantern");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("relay"), "old").unwrap();
        let refused = replace(&target, b"new", None).unwrap_err().to_string();
        assert!(
            refused.starts_with(&target.display().to_string()),
            "{refused}"
        );
        assert_eq!(fs::read_to_string(target.join("relay")).unwrap(), "old");
        assert_eq!(files(kept.path()).unwrap(), [""; 0]);
        assert_eq!(directories(kept.path()).unwrap(), ["lantern"]);
    }
}
