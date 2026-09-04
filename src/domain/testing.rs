//! In-memory ports for tests of the layers above the domain.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::draft::Draft;
use super::machine::MachineName;
use super::ports::{Clock, Drafts, Host, Identity, Store, StoreError};
use super::slug::{Kind, Slug};
use super::version::{Document, Version};

#[derive(Default)]
pub struct MemoryStore {
    versions: RefCell<BTreeMap<Slug, Vec<Version>>>,
}

impl Store for MemoryStore {
    fn slugs(&self, kind: Kind) -> Result<Vec<Slug>, StoreError> {
        Ok(self
            .versions
            .borrow()
            .keys()
            .filter(|s| s.kind() == kind)
            .cloned()
            .collect())
    }

    fn document(&self, slug: &Slug) -> Result<Document, StoreError> {
        Ok(Document::new(
            self.versions
                .borrow()
                .get(slug)
                .cloned()
                .unwrap_or_default(),
        ))
    }

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let mut all = self.versions.borrow_mut();
        let versions = all.entry(version.slug.clone()).or_default();
        if !versions.iter().any(|v| v.id == version.id) {
            versions.push(version.clone());
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct MemoryDrafts {
    drafts: RefCell<BTreeMap<Slug, Draft>>,
}

impl Drafts for MemoryDrafts {
    fn read(&self, slug: &Slug) -> Result<Option<Draft>, StoreError> {
        Ok(self.drafts.borrow().get(slug).cloned())
    }

    fn write(&self, draft: &Draft) -> Result<String, StoreError> {
        self.drafts
            .borrow_mut()
            .insert(draft.slug.clone(), draft.clone());
        Ok(self.location(&draft.slug))
    }

    fn delete(&self, slug: &Slug) -> Result<(), StoreError> {
        self.drafts.borrow_mut().remove(slug);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Draft>, StoreError> {
        Ok(self.drafts.borrow().values().cloned().collect())
    }

    fn location(&self, slug: &Slug) -> String {
        format!("memory:{}/{}", slug.kind(), slug)
    }
}

pub struct FixedIdentity(pub RefCell<Option<MachineName>>);

impl FixedIdentity {
    /// # Panics
    /// On a name `MachineName` refuses; a test names its machine.
    #[must_use]
    pub fn named(name: &str) -> FixedIdentity {
        FixedIdentity(RefCell::new(Some(
            MachineName::parse(name).expect("a valid test machine name"),
        )))
    }

    #[must_use]
    pub fn unset() -> FixedIdentity {
        FixedIdentity(RefCell::new(None))
    }
}

impl Identity for FixedIdentity {
    fn machine(&self) -> Result<Option<MachineName>, StoreError> {
        Ok(self.0.borrow().clone())
    }
}

/// A host on which the named directories exist and no others.
pub struct FixedHost(pub RefCell<Vec<String>>);

impl FixedHost {
    #[must_use]
    pub fn with(dirs: &[&str]) -> FixedHost {
        FixedHost(RefCell::new(dirs.iter().map(|d| (*d).to_owned()).collect()))
    }
}

impl Host for FixedHost {
    fn dir_exists(&self, path: &str) -> bool {
        self.0.borrow().iter().any(|d| d == path)
    }
}

pub struct FixedClock {
    pub today: String,
    pub now: String,
}

impl FixedClock {
    #[must_use]
    pub fn on(today: &str) -> FixedClock {
        FixedClock {
            today: today.to_owned(),
            now: format!("{today}T12:00:00+00:00"),
        }
    }
}

impl Clock for FixedClock {
    fn today(&self) -> String {
        self.today.clone()
    }

    fn now(&self) -> String {
        self.now.clone()
    }
}
