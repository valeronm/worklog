//! The ports implemented on a directory tree.

pub mod clock;
pub mod config;
pub mod drafts;
pub mod host;
pub mod ids;
pub mod paths;
pub mod store;

pub use clock::SystemClock;
pub use config::Config;
pub use drafts::FsDrafts;
pub use host::FsHost;
pub use ids::RandomIds;
pub use paths::Paths;
pub use store::FsStore;

use std::path::Path;

use crate::domain::ports::StoreError;

pub(crate) fn io_error(location: &Path, error: &std::io::Error) -> StoreError {
    StoreError::io(location.display(), error)
}

/// Every file and directory here is created by its first write.
pub(crate) fn optional<T>(at: &Path, result: std::io::Result<T>) -> Result<Option<T>, StoreError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(at, &e)),
    }
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

pub(crate) fn directories(dir: &Path) -> Result<Vec<String>, StoreError> {
    listed(dir, true)
}

pub(crate) fn files(dir: &Path) -> Result<Vec<String>, StoreError> {
    listed(dir, false)
}
