use std::cell::RefCell;
use std::fmt;

use toml::Value;

use crate::domain::id::{DocumentId, VersionId};
use crate::domain::ports::{Clock, Drafts, Host, Ids, Store, StoreError};
use crate::domain::schema::{Date, KindOf, SchemaError};
use crate::domain::version::Kind;

pub mod amend;
pub mod bulk;
pub mod claim;
pub mod draft;
pub mod followup;
pub mod fork;
mod live;
pub mod lookup;
pub mod save;
#[cfg(test)]
mod testing;

pub struct Deps<'a> {
    pub store: &'a dyn Store,
    pub drafts: &'a dyn Drafts,
    pub ids: &'a dyn Ids,
    pub clock: &'a dyn Clock,
    pub host: &'a dyn Host,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The store and the arguments are fine and the operation is not allowed on them.
    Refused(String),
    /// The arguments name no valid operation.
    Usage(String),
    Store(StoreError),
}

impl Failure {
    pub fn at(what: impl fmt::Display, why: impl fmt::Display) -> Failure {
        Failure::Refused(format!("{what}: {why}"))
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Refused(text) | Failure::Usage(text) => f.write_str(text),
            Failure::Store(error) => error.fmt(f),
        }
    }
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Failure {
        Failure::Store(error)
    }
}

impl From<SchemaError> for Failure {
    fn from(error: SchemaError) -> Failure {
        Failure::Refused(error.to_string())
    }
}

fn kind_named(kind: KindOf) -> Kind {
    Kind::parse(kind.word()).expect("a kind word is a kind")
}

pub(super) fn text(value: impl Into<String>) -> Value {
    Value::String(value.into())
}

/// Keeps the first failure of a closure that cannot return one.
#[derive(Default)]
pub(super) struct Failed(RefCell<Option<Failure>>);

impl Failed {
    pub(super) fn keep(&self, failure: Failure) {
        self.0.borrow_mut().get_or_insert(failure);
    }

    pub(super) fn done(self) -> Result<(), Failure> {
        self.0.into_inner().map_or(Ok(()), Err)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub document: DocumentId,
    pub version: VersionId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftRef {
    pub document: DocumentId,
    pub location: String,
}

impl Deps<'_> {
    pub fn today(&self) -> Result<Date, Failure> {
        Ok(Date::parse(&self.clock.now().day())?)
    }

    /// Refuses when the host has no machine topic.
    pub fn machine(&self) -> Result<DocumentId, Failure> {
        self.host.machine()?.ok_or_else(|| {
            Failure::Refused("this host has no machine topic; set the host up first".to_owned())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{
        FixedClock, FixedHost, MemoryDrafts, MemoryStore, SequenceIds, atlas,
    };

    #[test]
    fn a_failure_reads_as_its_text() {
        assert_eq!(
            Failure::at("lantern", "is ended").to_string(),
            "lantern: is ended"
        );
        assert_eq!(
            Failure::from(SchemaError::Ended),
            Failure::Refused(SchemaError::Ended.to_string())
        );
        let error = StoreError::io("desk", "gone");
        assert_eq!(Failure::from(error.clone()), Failure::Store(error.clone()));
        assert_eq!(Failure::Store(error.clone()).to_string(), error.to_string());
        assert_eq!(Failure::Usage("no".to_owned()).to_string(), "no");
    }

    #[test]
    fn today_is_the_clock_s_day_and_the_machine_needs_a_host() {
        let store = MemoryStore::default();
        let drafts = MemoryDrafts::default();
        let ids = SequenceIds::default();
        let clock = FixedClock::at("2026-10-09T18:22:41.118204+01:00");
        let bare = FixedHost(None);
        let set_up = FixedHost(Some(atlas()));
        let deps = |host| Deps {
            store: &store,
            drafts: &drafts,
            ids: &ids,
            clock: &clock,
            host,
        };

        assert_eq!(
            deps(&bare).today().unwrap(),
            Date::parse("2026-10-09").unwrap()
        );
        let Err(Failure::Refused(text)) = deps(&bare).machine() else {
            panic!("a host with no machine topic must be refused");
        };
        assert!(text.contains("has no machine topic"), "{text}");
        assert_eq!(deps(&set_up).machine(), Ok(atlas()));
    }
}
