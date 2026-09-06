//! In-memory ports for tests of the layers above the domain.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use super::draft::Draft;
use super::machine::MachineName;
use super::ports::{Binary, Clock, Drafts, Host, Identity, Releases, Store, StoreError, Usage};
use super::slug::{Kind, Slug};
use super::usage::Invocation;
use super::version::{Document, Version, VersionId};

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

    fn by_id_prefix(&self, prefix: &str) -> Result<Vec<(Slug, VersionId)>, StoreError> {
        Ok(self
            .versions
            .borrow()
            .iter()
            .flat_map(|(slug, versions)| {
                versions
                    .iter()
                    .filter(|v| v.id.as_str().starts_with(prefix))
                    .map(|v| (slug.clone(), v.id.clone()))
            })
            .collect())
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

#[derive(Default)]
pub struct MemoryUsage {
    lines: RefCell<Vec<Invocation>>,
}

impl Usage for MemoryUsage {
    fn record(&self, invocation: &Invocation) -> Result<(), StoreError> {
        self.lines.borrow_mut().push(invocation.clone());
        Ok(())
    }

    fn all(&self) -> Result<Vec<Invocation>, StoreError> {
        Ok(self.lines.borrow().clone())
    }
}

/// One published release, its assets by name.
pub struct MemoryReleases {
    pub latest: String,
    pub assets: BTreeMap<String, Vec<u8>>,
}

impl Releases for MemoryReleases {
    fn latest(&self) -> Result<String, StoreError> {
        Ok(self.latest.clone())
    }

    fn fetch(&self, _tag: &str, asset: &str) -> Result<Vec<u8>, StoreError> {
        self.assets
            .get(asset)
            .cloned()
            .ok_or_else(|| StoreError::io(asset, "no such asset"))
    }
}

/// A binary that remembers what replaced it and whether it was refreshed.
#[derive(Default)]
pub struct MemoryBinary {
    pub replaced_with: RefCell<Option<Vec<u8>>>,
    pub refreshed: Cell<bool>,
}

impl Binary for MemoryBinary {
    fn replace(&self, bytes: &[u8]) -> Result<String, StoreError> {
        *self.replaced_with.borrow_mut() = Some(bytes.to_vec());
        Ok("/home/u/.local/bin/worklog".into())
    }

    fn refresh(&self) -> Result<String, StoreError> {
        self.refreshed.set(true);
        Ok("/home/u/.config/fish/completions/worklog.fish\n".into())
    }
}
