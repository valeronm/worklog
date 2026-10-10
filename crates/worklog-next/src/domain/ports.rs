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
    fn of_kind(&self, kind: &Kind) -> Result<Vec<Document>, StoreError>;
    /// The documents a head of which carries `key` as one of `values`, or as a
    /// list with one of them in it, by id; none for no values.
    fn holding(&self, key: &str, values: &[&str]) -> Result<Vec<Document>, StoreError>;
    /// The documents holding a file that does not read as a version, by id.
    fn unreadable(&self) -> Result<Vec<Document>, StoreError>;
    /// The documents with more than one head, by id.
    fn forked(&self) -> Result<Vec<Document>, StoreError>;
    /// The kinds of the documents held, each once, sorted; a document none of whose files
    /// read has no kind.
    fn kinds(&self) -> Result<Vec<Kind>, StoreError>;
    /// The documents whose id starts with `prefix`, by id; none for an
    /// empty prefix.
    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError>;
    /// The versions whose id starts with `prefix`, with or without the
    /// algorithm before it, each with its document; none for an empty
    /// prefix. A file named as a version counts whether or not it reads.
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
    /// The home directory as an absolute path; `None` for a host with none.
    fn home(&self) -> Result<Option<String>, StoreError>;
    /// Makes `machine` this host's machine topic, refusing a host that already has one.
    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError>;
    /// `absolute` with the links followed through the longest part of it that exists on this
    /// host's file system, the rest kept as given.
    fn resolve(&self, absolute: &str) -> Result<String, StoreError>;
}
