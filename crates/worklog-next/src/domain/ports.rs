//! What a use case needs from outside the domain, as traits the `fs` layer
//! implements and the tests stub.

use std::fmt;

use super::document::Document;
use super::draft::Draft;
use super::id::{DocumentId, VersionId};
use super::version::{Kind, Stamp, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Io { location: String, reason: String },
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
        }
    }
}

/// Every version of every document; no call returns the whole store.
pub trait Store {
    /// The versions held of the document; none for one never stored.
    fn document(&self, id: &DocumentId) -> Result<Document, StoreError>;
    /// Adds a version; a version already present is not an error.
    fn put(&self, version: &Version) -> Result<(), StoreError>;
    /// The documents of the kind, by id.
    fn of_kind(&self, kind: &Kind) -> Result<Vec<DocumentId>, StoreError>;
    /// The documents a head of which carries `key` as `value`, or as a
    /// list with `value` in it, by id.
    fn holding(&self, key: &str, value: &str) -> Result<Vec<DocumentId>, StoreError>;
    /// The documents whose id starts with `prefix`, by id; none for an
    /// empty prefix.
    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError>;
    /// The versions whose id starts with `prefix`, with or without the
    /// algorithm before it, each with its document; none for an empty
    /// prefix.
    fn versions_under(&self, prefix: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError>;
}

/// Where a new document's id comes from.
pub trait Ids {
    fn mint(&self) -> Result<DocumentId, StoreError>;
}

/// Drafts being edited on this machine, never synced.
pub trait Drafts {
    fn read(&self, document: &DocumentId) -> Result<Option<Draft>, StoreError>;
    /// Writes the draft and returns where a person or an editor finds it.
    fn write(&self, draft: &Draft) -> Result<String, StoreError>;
    fn delete(&self, document: &DocumentId) -> Result<(), StoreError>;
    /// Every draft, by its document's id.
    fn list(&self) -> Result<Vec<Draft>, StoreError>;
    /// Where the draft of the document is or would be.
    fn location(&self, document: &DocumentId) -> String;
}

pub trait Clock {
    fn now(&self) -> Stamp;
}

pub trait Host {
    /// The id of this machine's topic; `None` before the host is set up.
    fn machine(&self) -> Result<Option<DocumentId>, StoreError>;
}
