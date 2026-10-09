//! In-memory ports for tests of the layers above the domain, and the
//! contract every implementation of a port is held to.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use super::document::{Document, State};
use super::draft::Draft;
use super::id::{DocumentId, VersionId};
use super::ports::{Drafts, Ids, Store, StoreError};
use super::version::{Envelope, Fields, Kind, Links, Stamp, Version};

#[derive(Default)]
pub struct MemoryStore {
    versions: RefCell<BTreeMap<DocumentId, Vec<Version>>>,
}

impl MemoryStore {
    fn matching(&self, wanted: impl Fn(&Document) -> bool) -> Vec<DocumentId> {
        self.versions
            .borrow()
            .iter()
            .filter(|(id, versions)| {
                wanted(&Document::new((*id).clone(), (*versions).clone(), vec![]))
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl Store for MemoryStore {
    fn document(&self, id: &DocumentId) -> Result<Document, StoreError> {
        let versions = self.versions.borrow().get(id).cloned().unwrap_or_default();
        Ok(Document::new(id.clone(), versions, vec![]))
    }

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let mut all = self.versions.borrow_mut();
        let versions = all.entry(version.envelope.document.clone()).or_default();
        if !versions.iter().any(|held| held.id == version.id) {
            versions.push(version.clone());
        }
        Ok(())
    }

    fn of_kind(&self, kind: &Kind) -> Result<Vec<DocumentId>, StoreError> {
        Ok(self.matching(|document| document.kind() == Some(kind)))
    }

    fn holding(&self, key: &str, value: &str) -> Result<Vec<DocumentId>, StoreError> {
        Ok(self.matching(|document| document.holds(key, value)))
    }

    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        Ok(self
            .versions
            .borrow()
            .keys()
            .filter(|id| !prefix.is_empty() && id.as_str().starts_with(prefix))
            .cloned()
            .collect())
    }

    fn versions_under(&self, prefix: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError> {
        let mut found: Vec<(DocumentId, VersionId)> = self
            .versions
            .borrow()
            .iter()
            .flat_map(|(document, versions)| {
                versions
                    .iter()
                    .filter(|version| version.id.starts_with(prefix))
                    .map(|version| (document.clone(), version.id.clone()))
            })
            .collect();
        found.sort();
        Ok(found)
    }
}

#[derive(Default)]
pub struct MemoryDrafts {
    drafts: RefCell<BTreeMap<DocumentId, Draft>>,
}

impl Drafts for MemoryDrafts {
    fn read(&self, document: &DocumentId) -> Result<Option<Draft>, StoreError> {
        Ok(self.drafts.borrow().get(document).cloned())
    }

    fn write(&self, draft: &Draft) -> Result<String, StoreError> {
        self.drafts
            .borrow_mut()
            .insert(draft.document.clone(), draft.clone());
        Ok(self.location(&draft.document))
    }

    fn delete(&self, document: &DocumentId) -> Result<(), StoreError> {
        self.drafts.borrow_mut().remove(document);
        Ok(())
    }

    fn list(&self) -> Result<Vec<Draft>, StoreError> {
        Ok(self.drafts.borrow().values().cloned().collect())
    }

    fn location(&self, document: &DocumentId) -> String {
        format!("memory:{document}")
    }
}

#[derive(Default)]
pub struct SequenceIds {
    minted: Cell<u128>,
}

impl Ids for SequenceIds {
    fn mint(&self) -> Result<DocumentId, StoreError> {
        self.minted.set(self.minted.get() + 1);
        Ok(DocumentId::from_bytes(self.minted.get().to_be_bytes()))
    }
}

pub const LANTERN: &str = "7f3a91c05be2446d8a10c3f29b7e6d54";

#[must_use]
pub fn lantern() -> DocumentId {
    DocumentId::from_bytes(0x7f3a_91c0_5be2_446d_8a10_c3f2_9b7e_6d54_u128.to_be_bytes())
}

fn compose(envelope: Envelope, fields: &str, body: &str) -> Version {
    let fields: Fields = fields.parse().expect("fields in TOML");
    Version::compose(envelope, fields, Links::default(), body.to_owned()).expect("a version")
}

fn stamp() -> Stamp {
    Stamp::parse("2026-09-04T10:00:00+01:00").expect("a stamp")
}

fn desk() -> DocumentId {
    DocumentId::from_bytes([0xde; 16])
}

/// The first version of `document`, its fields given as TOML.
///
/// # Panics
///
/// When the parts do not make a version.
#[must_use]
pub fn first(document: &DocumentId, kind: &str, fields: &str, body: &str) -> Version {
    let kind = Kind::parse(kind).expect("a kind");
    let envelope = Envelope::first(document.clone(), kind, stamp(), desk(), "new");
    compose(envelope, fields, body)
}

/// A version written on top of `heads`, its fields given as TOML.
///
/// # Panics
///
/// When there is no head to follow or the parts do not make a version.
#[must_use]
pub fn after(heads: &[&Version], fields: &str, body: &str) -> Version {
    let envelope = Envelope::following(heads, stamp(), desk(), "save").expect("a head to follow");
    compose(envelope, fields, body)
}

/// What every `Store` answers, whatever it keeps its versions in.
///
/// # Panics
///
/// When the store answers otherwise.
pub fn store_contract(store: &dyn Store, ids: &dyn Ids) {
    let lantern = ids.mint().expect("an id");
    let atlas = ids.mint().expect("an id");
    assert_ne!(lantern, atlas);
    assert_eq!(
        store.document(&lantern).expect("a read").state(),
        State::Absent
    );

    let topics = "topics = [\"lantern\", \"phone\"]";
    let relay = first(
        &lantern,
        "fact",
        &format!("name = \"relay\"\n{topics}"),
        "first\n",
    );
    store.put(&relay).expect("a write");
    store
        .put(&relay)
        .expect("a second write of the same version");
    assert_eq!(
        store.document(&lantern).expect("a read").history(),
        [&relay]
    );

    let renamed = after(
        &[&relay],
        &format!("name = \"relay-pin\"\n{topics}"),
        "second\n",
    );
    store.put(&renamed).expect("a write");
    let held = store.document(&lantern).expect("a read");
    assert_eq!(held.state(), State::Live(&renamed));
    assert_eq!(held.id(), &lantern);

    store
        .put(&first(&atlas, "topic", "name = \"atlas\"", "\n"))
        .expect("a write");

    let kind = |word: &str| Kind::parse(word).expect("a kind");
    assert_eq!(
        store.of_kind(&kind("fact")).expect("a read"),
        std::slice::from_ref(&lantern)
    );
    assert_eq!(
        store.of_kind(&kind("topic")).expect("a read"),
        std::slice::from_ref(&atlas)
    );
    assert!(store.of_kind(&kind("entry")).expect("a read").is_empty());

    assert_eq!(
        store.holding("name", "relay-pin").expect("a read"),
        std::slice::from_ref(&lantern)
    );
    assert_eq!(
        store.holding("topics", "phone").expect("a read"),
        std::slice::from_ref(&lantern)
    );
    assert!(
        store.holding("name", "relay").expect("a read").is_empty(),
        "only an older version has it"
    );
    assert!(store.holding("name", "desk").expect("a read").is_empty());

    for length in [1, 2, 5] {
        let under = store
            .documents_under(&lantern.as_str()[..length])
            .expect("a read");
        assert!(under.contains(&lantern), "{length}: {under:?}");
    }
    assert_eq!(
        store.documents_under(lantern.as_str()).expect("a read"),
        std::slice::from_ref(&lantern)
    );
    assert!(store.documents_under("").expect("a read").is_empty());

    let found = [(lantern.clone(), renamed.id.clone())];
    for prefix in [renamed.id.short(), renamed.id.as_str()] {
        assert_eq!(store.versions_under(prefix).expect("a read"), found);
    }
    for all in ["", "b3-"] {
        assert!(
            store.versions_under(all).expect("a read").is_empty(),
            "{all}"
        );
    }
}

/// What every `Drafts` answers, wherever it keeps them.
///
/// # Panics
///
/// When the drafts answer otherwise.
pub fn drafts_contract(drafts: &dyn Drafts, ids: &dyn Ids) {
    let lantern = ids.mint().expect("an id");
    let atlas = ids.mint().expect("an id");
    assert_eq!(drafts.read(&lantern).expect("a read"), None);
    assert!(drafts.list().expect("a read").is_empty());
    drafts.delete(&lantern).expect("deleting no draft");

    let draft = Draft::of(&first(&lantern, "fact", "name = \"relay\"", "first\n"));
    assert_eq!(
        drafts.write(&draft).expect("a write"),
        drafts.location(&lantern)
    );
    assert_eq!(drafts.read(&lantern).expect("a read"), Some(draft.clone()));

    let edited = draft
        .with_text("+++\nname = \"relay-pin\"\n+++\nsecond\n")
        .expect("a draft");
    drafts.write(&edited).expect("a write over the draft");
    assert_eq!(drafts.read(&lantern).expect("a read"), Some(edited.clone()));

    let topic = Draft::first(
        atlas.clone(),
        Kind::parse("topic").expect("a kind"),
        Fields::new(),
        String::new(),
    );
    drafts.write(&topic).expect("a write");
    let mut expected = vec![edited, topic];
    expected.sort_by(|a, b| a.document.cmp(&b.document));
    assert_eq!(drafts.list().expect("a read"), expected);

    drafts.delete(&lantern).expect("a delete");
    assert_eq!(drafts.read(&lantern).expect("a read"), None);
    assert_eq!(drafts.list().expect("a read").len(), 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_store_keeps_the_contract() {
        store_contract(&MemoryStore::default(), &SequenceIds::default());
    }

    #[test]
    fn the_memory_drafts_keep_the_contract() {
        drafts_contract(&MemoryDrafts::default(), &SequenceIds::default());
    }

    #[test]
    fn sequence_ids_count_up() {
        let ids = SequenceIds::default();
        assert_eq!(
            ids.mint().unwrap().as_str(),
            "00000000000000000000000000000001"
        );
        assert_eq!(
            ids.mint().unwrap().as_str(),
            "00000000000000000000000000000002"
        );
    }
}
