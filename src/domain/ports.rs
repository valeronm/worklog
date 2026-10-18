//! What a use case needs from outside the domain, as traits the adapters under `wiring`
//! implement and the tests stub.

use std::fmt;
use std::rc::Rc;

use super::document::Document;
use super::draft::Draft;
use super::id::{DocumentId, VersionId};
use super::usage::{Invocation, Kept, Session};
use super::version::{Kind, ReadError, Stamp, Version};

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

/// Why a version's file gave no version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotRead {
    /// No file is there.
    Missing,
    Unreadable(ReadError),
}

impl fmt::Display for NotRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotRead::Missing => f.write_str("its file is not there"),
            NotRead::Unreadable(error) => error.fmt(f),
        }
    }
}

/// Every version of every document, by their headers across the store and one body at a
/// time; no call returns the whole store.
pub trait StoreRead {
    /// The headers of the versions held of the document, none for one never stored: from the
    /// one reading of the tree this instance makes, or from the document alone before one.
    fn headers(&self, id: &DocumentId) -> Result<Rc<Document>, StoreError>;
    /// The version with its body, as its file reads now, or why the file gives none.
    fn version(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<Version, NotRead>, StoreError>;
    /// The body of the version, as its file reads now, or why the file gives none.
    fn body(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<String, NotRead>, StoreError>;
    /// The documents of the kind, by id.
    fn of_kind(&self, kind: &Kind) -> Result<Vec<Rc<Document>>, StoreError>;
    /// The documents a head of which carries `key` as one of `values`, or as a
    /// list with one of them in it, by id; none for no values.
    fn holding(&self, key: &str, values: &[&str]) -> Result<Vec<Rc<Document>>, StoreError>;
    /// The documents holding a file that does not read as a version, by id.
    fn unreadable(&self) -> Result<Vec<Rc<Document>>, StoreError>;
    /// The documents with more than one head, by id.
    fn forked(&self) -> Result<Vec<Rc<Document>>, StoreError>;
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
    /// Reads every version again from what holds the store, past anything remembered of
    /// an earlier reading.
    fn reread(&self) -> Result<(), StoreError>;
}

pub trait Store: StoreRead {
    /// Adds a version; a version already present is not an error.
    fn put(&self, version: &Version) -> Result<(), StoreError>;
}

/// Where a new document's id comes from.
pub trait Ids {
    fn mint(&self) -> Result<DocumentId, StoreError>;
}

/// Drafts being edited on this machine, never synced.
pub trait DraftsRead {
    fn read(&self, document: &DocumentId) -> Result<Option<Draft>, StoreError>;
    /// Every draft, by its document's id.
    fn list(&self) -> Result<Vec<Draft>, StoreError>;
    /// Where the draft of the document is or would be.
    fn location(&self, document: &DocumentId) -> String;
}

pub trait Drafts: DraftsRead {
    /// Writes the draft and returns where a person or an editor finds it.
    fn write(&self, draft: &Draft) -> Result<String, StoreError>;
    fn delete(&self, document: &DocumentId) -> Result<(), StoreError>;
}

/// The runs every machine logged.
pub trait UsageRead {
    /// The lines that read and that `kept` keeps; a line filed under another machine or
    /// month than its own may be left out.
    fn read(&self, kept: &Kept) -> Result<Vec<Invocation>, StoreError>;
}

pub trait Usage: UsageRead {
    fn record(&self, invocation: &Invocation) -> Result<(), StoreError>;
}

/// Where releases of this binary are published.
pub trait Releases {
    /// The tag of the latest release.
    fn latest(&self) -> Result<String, StoreError>;
    /// An asset of the release the tag names, whole.
    fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, StoreError>;
}

/// What a binary's refresh of its host wrote and had to note, with why it stopped where it did
/// not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refreshed {
    pub written: Vec<String>,
    pub notes: Vec<String>,
    pub ended: Result<(), StoreError>,
}

/// The executable this process runs from.
pub trait Binary {
    /// The release asset built for the target this binary runs on; `None` where no release
    /// is built for it.
    fn asset(&self) -> Option<String>;
    /// Puts `bytes` in the place of the running binary and returns where that is.
    fn replace(&self, bytes: &[u8]) -> Result<String, StoreError>;
    /// Has the binary now in place, which after `replace` is not this process, bring what
    /// this host takes from it up to itself.
    fn refresh(&self) -> Refreshed;
}

pub trait Clock {
    fn now(&self) -> Stamp;
}

pub trait HostRead {
    /// The id of this machine's topic; `None` before the host is set up.
    fn machine(&self) -> Result<Option<DocumentId>, StoreError>;
    /// The home directory as an absolute path; `None` for a host with none.
    fn home(&self) -> Result<Option<String>, StoreError>;
    /// `absolute` with the links followed through the longest part of it that exists on this
    /// host's file system, the rest kept as given.
    fn resolve(&self, absolute: &str) -> Result<String, StoreError>;
    /// The agent session this process runs in; `None` at a terminal.
    fn session(&self) -> Option<Session>;
    /// The name the operating system has for this host, as it gives it; `None` for one that
    /// does not read as text.
    fn name(&self) -> Option<String>;
}

pub trait Host: HostRead {
    /// Makes `machine` this host's machine topic, refusing a host that already has one.
    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError>;
}
