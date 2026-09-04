//! The ports implemented on a directory tree.

pub mod clock;
pub mod config;
pub mod drafts;
pub mod legacy;
pub mod paths;
pub mod store;

pub use clock::SystemClock;
pub use config::{Config, FileIdentity};
pub use drafts::FsDrafts;
pub use paths::Paths;
pub use store::FsStore;

use std::path::Path;

use crate::domain::ports::StoreError;

pub(crate) fn io_error(location: &Path, error: &std::io::Error) -> StoreError {
    StoreError::Io {
        location: location.display().to_string(),
        reason: error.to_string(),
    }
}

pub(crate) fn corrupt(location: &Path, reason: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        location: location.display().to_string(),
        reason: reason.to_string(),
    }
}
