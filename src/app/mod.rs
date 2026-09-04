//! One use case per command, over the ports the domain declares. A use
//! case returns a typed output and never prints.

pub mod load;
pub mod migrate;
pub mod output;
pub mod read;
pub mod usage;
pub mod write;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::fmt;

use crate::domain::machine::{MachineName, MachineNameError};
use crate::domain::ports::{Clock, Drafts, Host, Identity, Store, StoreError, Usage};
use crate::domain::recheck::RecheckError;
use crate::domain::slug::{Kind, Slug, SlugError};

/// Everything a use case reaches outside the domain.
pub struct Deps<'a> {
    pub store: &'a dyn Store,
    pub drafts: &'a dyn Drafts,
    pub identity: &'a dyn Identity,
    pub clock: &'a dyn Clock,
    pub host: &'a dyn Host,
    pub usage: &'a dyn Usage,
    /// The user's home, for `~/` in claims.
    pub home: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The store or the arguments are fine and the operation is not allowed
    /// on them; exit 1.
    Refused(String),
    /// The arguments do not name a valid operation; exit 2.
    Usage(String),
    Store(StoreError),
}

impl Failure {
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Refused(_) | Failure::Store(_) => 1,
            Failure::Usage(_) => 2,
        }
    }

    /// A refusal about one document.
    #[must_use]
    pub fn at(slug: &Slug, reason: impl fmt::Display) -> Failure {
        Failure::Refused(format!("{slug}: {reason}"))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Refused(text) | Failure::Usage(text) => f.write_str(text),
            Failure::Store(e) => e.fmt(f),
        }
    }
}

impl From<StoreError> for Failure {
    fn from(e: StoreError) -> Self {
        Failure::Store(e)
    }
}

// A value the arguments could not spell is a usage error, whichever
// domain type refused it.
impl From<SlugError> for Failure {
    fn from(e: SlugError) -> Self {
        Failure::Usage(e.to_string())
    }
}

impl From<RecheckError> for Failure {
    fn from(e: RecheckError) -> Self {
        Failure::Usage(e.to_string())
    }
}

impl From<MachineNameError> for Failure {
    fn from(e: MachineNameError) -> Self {
        Failure::Usage(e.to_string())
    }
}

/// A slug argument, by shape unless the caller named the kind.
pub fn slug_arg(text: &str, kind: Option<Kind>) -> Result<Slug, Failure> {
    Ok(match kind {
        Some(kind) => Slug::of_kind(kind, text)?,
        None => Slug::parse(text)?,
    })
}

/// This host's name, which every write and every placement needs.
pub fn machine(deps: &Deps) -> Result<MachineName, Failure> {
    deps.identity
        .machine()?
        .ok_or_else(|| Failure::Refused("no machine name: run `worklog init` first".into()))
}
