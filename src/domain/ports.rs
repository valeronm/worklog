//! What a use case needs from outside the domain, as traits the `fs` layer
//! implements and the tests stub.

use std::fmt;

use super::draft::Draft;
use super::machine::MachineName;
use super::slug::{Kind, Slug};
use super::usage::Invocation;
use super::version::{Document, Version, VersionId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Io {
        location: String,
        reason: String,
    },
    /// A file that is not a version in the writer's shape.
    Corrupt {
        location: String,
        reason: String,
    },
}

impl StoreError {
    pub fn io(location: impl fmt::Display, reason: impl fmt::Display) -> StoreError {
        StoreError::Io {
            location: location.to_string(),
            reason: reason.to_string(),
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io { location, reason } => write!(f, "{location}: {reason}"),
            StoreError::Corrupt { location, reason } => {
                write!(f, "{location} is not a version: {reason}")
            }
        }
    }
}

/// The append-only store: every version of every document.
pub trait Store {
    fn slugs(&self, kind: Kind) -> Result<Vec<Slug>, StoreError>;
    /// Every version of the slug; empty for one the store never held.
    fn document(&self, slug: &Slug) -> Result<Document, StoreError>;
    /// Adds a version; a version already present is not an error.
    fn put(&self, version: &Version) -> Result<(), StoreError>;
    /// Every version whose id starts with `prefix`, as its slug and its
    /// full id, in no particular order. The caller passes a text
    /// `VersionId::is_prefix` accepts, since an empty one matches all.
    fn by_id_prefix(&self, prefix: &str) -> Result<Vec<(Slug, VersionId)>, StoreError>;
}

/// Drafts being edited on this machine, never synced.
pub trait Drafts {
    fn read(&self, slug: &Slug) -> Result<Option<Draft>, StoreError>;
    /// Writes the draft and returns where a person or an editor finds it.
    fn write(&self, draft: &Draft) -> Result<String, StoreError>;
    fn delete(&self, slug: &Slug) -> Result<(), StoreError>;
    fn list(&self) -> Result<Vec<Draft>, StoreError>;
    /// Where the draft for the slug is or would be.
    fn location(&self, slug: &Slug) -> String;
}

/// The log of commands run, one file per machine so no two writers share
/// one.
pub trait Usage {
    fn record(&self, invocation: &Invocation) -> Result<(), StoreError>;
    /// Every line every machine has written here, in no order.
    fn all(&self) -> Result<Vec<Invocation>, StoreError>;
}

/// The configured name of this machine, if `init` has run.
pub trait Identity {
    fn machine(&self) -> Result<Option<MachineName>, StoreError>;
}

/// What the host this runs on can answer about itself.
pub trait Host {
    fn dir_exists(&self, path: &str) -> bool;
}

pub trait Clock {
    /// `YYYY-MM-DD` in local time.
    fn today(&self) -> String;
    /// RFC 3339 with the local offset.
    fn now(&self) -> String;
}

/// Where releases of this binary are published.
pub trait Releases {
    /// The tag of the latest release.
    fn latest(&self) -> Result<String, StoreError>;
    /// An asset of the release the tag names, whole.
    fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, StoreError>;
}

/// The executable this process runs from.
pub trait Binary {
    /// Puts the bytes in the place of the running binary and returns where
    /// that is.
    fn replace(&self, bytes: &[u8]) -> Result<String, StoreError>;
    /// Has the binary now in place, which after `replace` is not this
    /// process, bring what this host takes from it up to itself, and
    /// returns what it wrote, one path per line.
    fn refresh(&self) -> Result<String, StoreError>;
}
