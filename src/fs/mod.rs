//! The ports implemented on a directory tree.

pub mod agent;
pub mod clock;
pub mod config;
pub mod drafts;
pub mod legacy;
pub mod paths;
pub mod store;
pub mod usage;

pub use agent::Agent;
pub use clock::SystemClock;
pub use config::{Config, FileIdentity};
pub use drafts::FsDrafts;
pub use paths::Paths;
pub use store::FsStore;
pub use usage::FsUsage;

use std::path::Path;

use crate::domain::ports::{Host, StoreError};

pub struct FsHost;

impl Host for FsHost {
    fn dir_exists(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }
}

pub(crate) fn io_error(location: &Path, error: &std::io::Error) -> StoreError {
    StoreError::Io {
        location: location.display().to_string(),
        reason: error.to_string(),
    }
}

/// An outcome at a path that is not there reads as `None` rather than as
/// a failure, since every file and directory here is created by its first
/// write.
pub(crate) fn optional<T>(at: &Path, result: std::io::Result<T>) -> Result<Option<T>, StoreError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(at, &e)),
    }
}

pub(crate) fn read_optional(path: &Path) -> Result<Option<String>, StoreError> {
    optional(path, std::fs::read_to_string(path))
}

pub(crate) fn entries(
    dir: &Path,
) -> Result<impl Iterator<Item = std::io::Result<std::fs::DirEntry>>, StoreError> {
    optional(dir, std::fs::read_dir(dir)).map(|found| found.into_iter().flatten())
}

/// Writes a whole file, creating its directory, with the path in any
/// refusal.
fn write_file(path: &Path, text: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
    }
    std::fs::write(path, text).map_err(|e| io_error(path, &e))
}

pub(crate) fn corrupt(location: &Path, reason: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        location: location.display().to_string(),
        reason: reason.to_string(),
    }
}
