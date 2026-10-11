//! The ports implemented on a directory tree.

mod agent;
mod clock;
mod config;
mod drafts;
mod host;
mod ids;
mod paths;
mod store;

pub use agent::{FsAgent, SKILL_DIRECTORY};
pub use clock::SystemClock;
pub use config::Config;
pub use drafts::FsDrafts;
pub use host::FsHost;
pub use ids::RandomIds;
pub use paths::Paths;
pub use store::FsStore;

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(at, &e)),
    }
}

// A file renamed before its bytes reach the disk can be empty after a crash.
fn staged(file: std::io::Result<File>, text: &str) -> std::io::Result<()> {
    let mut file = file?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// Replaces `target` whole with `text`, through a file beside it that `create` opens and that
/// is gone again when the write fails.
fn replace(
    target: &Path,
    text: &str,
    create: impl FnOnce(&Path) -> std::io::Result<File>,
) -> Result<(), StoreError> {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(StoreError::io(target.display(), "is not a file's path"));
    };
    std::fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;
    // A rename replaces the file whole only from the same file system.
    let staging = dir.join(format!("{STAGING_PREFIX}{}", name.to_string_lossy()));
    let replaced = staged(create(&staging), text)
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
