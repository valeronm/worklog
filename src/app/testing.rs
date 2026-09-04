//! Every port at once, for a use-case test.

use crate::domain::testing::{
    FixedClock, FixedHost, FixedIdentity, MemoryDrafts, MemoryStore, MemoryUsage,
};

use super::Deps;

pub struct World {
    pub store: MemoryStore,
    pub drafts: MemoryDrafts,
    pub identity: FixedIdentity,
    pub clock: FixedClock,
    pub host: FixedHost,
    pub usage: MemoryUsage,
}

impl World {
    #[must_use]
    pub fn new(machine: &str) -> World {
        World {
            identity: FixedIdentity::named(machine),
            ..World::unnamed()
        }
    }

    #[must_use]
    pub fn unnamed() -> World {
        World {
            store: MemoryStore::default(),
            drafts: MemoryDrafts::default(),
            identity: FixedIdentity::unset(),
            clock: FixedClock::on("2026-09-04"),
            host: FixedHost::with(&["/home/u/projects/lantern"]),
            usage: MemoryUsage::default(),
        }
    }

    #[must_use]
    pub fn deps(&self) -> Deps<'_> {
        Deps {
            store: &self.store,
            drafts: &self.drafts,
            identity: &self.identity,
            clock: &self.clock,
            host: &self.host,
            usage: &self.usage,
            home: "/home/u".into(),
        }
    }
}
