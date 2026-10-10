use std::cell::{OnceCell, Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use crate::domain::document::{Document, State, Unreadable};
use crate::domain::id::{DocumentId, VersionId, is_hex};
use crate::domain::ports::{Store, StoreError};
use crate::domain::version::{Kind, ReadError, Version};

use super::{directories, files, io_error};

/// The folder's ignore file names the same prefix.
const STAGING_PREFIX: &str = ".tmp-";

/// A version's file is never rewritten. One instance is one snapshot: the tree is walked
/// once and remembered.
pub struct FsStore {
    root: PathBuf,
    walked: OnceCell<RefCell<Walked>>,
}

type Walked = BTreeMap<DocumentId, Document>;

fn file_name(id: &VersionId) -> String {
    format!("{id}.md")
}

fn version_named(name: &str) -> Option<VersionId> {
    VersionId::parse(name.strip_suffix(".md")?).ok()
}

fn parse(bytes: Vec<u8>, name: &VersionId) -> Result<Version, ReadError> {
    let text = String::from_utf8(bytes).map_err(|_| ReadError::Corrupt)?;
    Version::read(name, &text)
}

impl FsStore {
    #[must_use]
    pub fn new(root: PathBuf) -> FsStore {
        FsStore {
            root,
            walked: OnceCell::new(),
        }
    }

    fn document_dir(&self, id: &DocumentId) -> PathBuf {
        self.root.join(id.bucket()).join(id.as_str())
    }

    fn version_path(&self, document: &DocumentId, version: &VersionId) -> PathBuf {
        self.document_dir(document).join(file_name(version))
    }

    fn documents(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        let mut found = Vec::new();
        for bucket in directories(&self.root)? {
            if !is_hex(&bucket, 2) || !(bucket.starts_with(prefix) || prefix.starts_with(&bucket)) {
                continue;
            }
            for name in directories(&self.root.join(&bucket))? {
                if name.starts_with(&bucket)
                    && name.starts_with(prefix)
                    && let Ok(id) = DocumentId::parse(&name)
                {
                    found.push(id);
                }
            }
        }
        Ok(found)
    }

    fn read(&self, id: &DocumentId) -> Result<Document, StoreError> {
        let mut versions = Vec::new();
        let mut unreadable = Vec::new();
        for name in self.version_ids(id)? {
            match parse(self.bytes(id, &name)?, &name) {
                Ok(version) => versions.push(version),
                Err(why) => unreadable.push(Unreadable { id: name, why }),
            }
        }
        Ok(Document::new(id.clone(), versions, unreadable))
    }

    fn walked(&self) -> Result<Ref<'_, Walked>, StoreError> {
        if let Some(walked) = self.walked.get() {
            return Ok(walked.borrow());
        }
        let mut held = Walked::new();
        for id in self.documents("")? {
            let document = self.read(&id)?;
            held.insert(id, document);
        }
        Ok(self.walked.get_or_init(|| RefCell::new(held)).borrow())
    }

    fn matching(&self, wanted: impl Fn(&Document) -> bool) -> Result<Vec<Document>, StoreError> {
        let walked = self.walked()?;
        Ok(walked
            .values()
            .filter(|document| wanted(document))
            .cloned()
            .collect())
    }

    fn version_ids(&self, document: &DocumentId) -> Result<Vec<VersionId>, StoreError> {
        Ok(files(&self.document_dir(document))?
            .iter()
            .filter_map(|name| version_named(name))
            .collect())
    }

    fn bytes(&self, document: &DocumentId, version: &VersionId) -> Result<Vec<u8>, StoreError> {
        let path = self.version_path(document, version);
        fs::read(&path).map_err(|e| io_error(&path, &e))
    }

    /// A Syncthing folder's ignore file is per host and never synced. One
    /// already there is the host's own.
    fn keep_sync_clean(&self) -> Result<(), StoreError> {
        if !self.root.join(".stfolder").exists() {
            return Ok(());
        }
        let ignore = self.root.join(".stignore");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ignore);
        match file {
            Ok(mut file) => {
                use std::io::Write as _;
                writeln!(file, ".DS_Store\n{STAGING_PREFIX}*").map_err(|e| io_error(&ignore, &e))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(io_error(&ignore, &e)),
        }
    }
}

impl Store for FsStore {
    fn document(&self, id: &DocumentId) -> Result<Document, StoreError> {
        let Some(walked) = self.walked.get() else {
            return self.read(id);
        };
        let held = walked.borrow().get(id).cloned();
        Ok(held.unwrap_or_else(|| Document::new(id.clone(), Vec::new(), Vec::new())))
    }

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let document = &version.envelope.document;
        let dir = self.document_dir(document);
        let target = dir.join(file_name(&version.id));
        if !target.exists() {
            self.keep_sync_clean()?;
            fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
            // Syncthing picks up a file as soon as it appears.
            let staging = dir.join(format!("{STAGING_PREFIX}{}", version.id));
            fs::write(&staging, version.text()).map_err(|e| io_error(&staging, &e))?;
            fs::rename(&staging, &target).map_err(|e| io_error(&target, &e))?;
        }
        if let Some(walked) = self.walked.get() {
            let mut walked = walked.borrow_mut();
            let held = walked
                .entry(document.clone())
                .or_insert_with(|| Document::new(document.clone(), Vec::new(), Vec::new()));
            *held = held.with_version(version.clone());
        }
        Ok(())
    }

    fn of_kind(&self, kind: &Kind) -> Result<Vec<Document>, StoreError> {
        self.matching(|document| document.kind() == Some(kind))
    }

    fn holding(&self, key: &str, values: &[&str]) -> Result<Vec<Document>, StoreError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        self.matching(|document| values.iter().any(|value| document.holds(key, value)))
    }

    fn unreadable(&self) -> Result<Vec<Document>, StoreError> {
        self.matching(|document| !document.unreadable().is_empty())
    }

    fn forked(&self) -> Result<Vec<Document>, StoreError> {
        self.matching(|document| matches!(document.state(), State::Forked(_)))
    }

    fn kinds(&self) -> Result<Vec<Kind>, StoreError> {
        let walked = self.walked()?;
        let held: BTreeSet<&Kind> = walked.values().filter_map(Document::kind).collect();
        Ok(held.into_iter().cloned().collect())
    }

    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let mut held = Vec::new();
        for id in self.documents(prefix)? {
            if !self.version_ids(&id)?.is_empty() {
                held.push(id);
            }
        }
        Ok(held)
    }

    fn versions_under(&self, prefix: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError> {
        let mut found = Vec::new();
        for document in self.documents("")? {
            for version in self.version_ids(&document)? {
                if version.starts_with(prefix) {
                    found.push((document.clone(), version));
                }
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{LANTERN, after, first, lantern, store_contract};
    use crate::fs::RandomIds;

    const ATLAS: &str = "7fd0000000000000000000000000000c";
    const RELAY: &str = "name = \"relay\"";

    fn scratch() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    fn reread(store: &FsStore, id: &DocumentId) {
        if let Some(walked) = store.walked.get() {
            let document = store.read(id).unwrap();
            walked.borrow_mut().insert(id.clone(), document);
        }
    }

    fn path(store: &FsStore, version: &Version) -> PathBuf {
        store.version_path(&version.envelope.document, &version.id)
    }

    #[test]
    fn the_directory_store_keeps_the_contract() {
        let (_dir, store) = scratch();
        store_contract(&store, &RandomIds, &|document, version| {
            let path = store.version_path(document, version);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "damaged").unwrap();
            reread(&store, document);
        });
        let afresh = FsStore::new(store.root.clone());
        for word in ["fact", "topic", "entry", "sketch", "followup", "claim"] {
            let kind = Kind::parse(word).unwrap();
            assert_eq!(
                afresh.of_kind(&kind).unwrap(),
                store.of_kind(&kind).unwrap(),
                "{word}"
            );
        }
        assert_eq!(afresh.unreadable().unwrap(), store.unreadable().unwrap());
        assert_eq!(afresh.kinds().unwrap(), store.kinds().unwrap());
        assert_eq!(afresh.forked().unwrap(), store.forked().unwrap());
    }

    #[test]
    fn a_version_is_one_file_under_its_documents_bucket() {
        let (dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let file = dir
            .path()
            .join("7f")
            .join(LANTERN)
            .join(format!("{}.md", relay.id));
        assert_eq!(fs::read_to_string(file).unwrap(), relay.text());
        assert_eq!(files(&store.document_dir(&lantern())).unwrap().len(), 1);
    }

    #[test]
    fn a_damaged_file_is_unreadable_and_its_sibling_still_loads() {
        let (_dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        let rewired = after(&[&relay], RELAY, "second\n");
        store.put(&relay).unwrap();
        store.put(&rewired).unwrap();
        fs::write(path(&store, &relay), "damaged").unwrap();
        let held = store.document(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(&rewired));
        assert_eq!(
            held.unreadable(),
            [Unreadable {
                id: relay.id.clone(),
                why: ReadError::Corrupt
            }]
        );
        assert_eq!(
            store.versions_under(relay.id.short()).unwrap(),
            [(lantern(), relay.id.clone())]
        );
        let fact = Kind::parse("fact").unwrap();
        assert_eq!(store.of_kind(&fact).unwrap(), [held]);
        fs::write(path(&store, &rewired), [0xff, 0xfe]).unwrap();
        reread(&store, &lantern());
        let held = store.document(&lantern()).unwrap();
        assert_eq!(held.unreadable().len(), 2);
        assert!(store.of_kind(&fact).unwrap().is_empty());
        assert_eq!(store.unreadable().unwrap(), [held]);
    }

    #[test]
    fn a_version_under_another_documents_directory_is_unreadable() {
        let (_dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let stray = first(&atlas, "topic", "name = \"atlas\"", "\n");
        let misplaced = store.version_path(&lantern(), &stray.id);
        fs::create_dir_all(misplaced.parent().unwrap()).unwrap();
        fs::write(misplaced, stray.text()).unwrap();
        let held = store.document(&lantern()).unwrap();
        assert_eq!(held.state(), State::Absent);
        match &held.unreadable()[0].why {
            ReadError::Malformed(why) => assert!(why.contains(ATLAS), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(
            store
                .of_kind(&Kind::parse("topic").unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn what_is_not_named_for_a_version_or_a_document_is_ignored() {
        let (dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let document = store.document_dir(&lantern());
        fs::write(
            document.join(format!("{STAGING_PREFIX}{}", relay.id)),
            "half",
        )
        .unwrap();
        fs::write(document.join("notes.md"), "stray").unwrap();
        fs::create_dir(document.join(file_name(&VersionId::of(b"a directory")))).unwrap();
        fs::write(dir.path().join(".stignore"), "x").unwrap();
        fs::write(dir.path().join("ab"), "a file named as a bucket is").unwrap();
        fs::write(dir.path().join("7f").join(ATLAS), "and one as a document").unwrap();
        fs::create_dir_all(dir.path().join("7f").join("not-a-document")).unwrap();
        fs::create_dir_all(dir.path().join(".stfolder")).unwrap();
        let held = store.document(&lantern()).unwrap();
        assert_eq!(held.history(), [&relay]);
        assert!(held.unreadable().is_empty());
        assert_eq!(store.documents_under("7").unwrap(), [lantern()]);
        assert!(store.documents_under("a").unwrap().is_empty());
        assert_eq!(store.holding("name", &["relay"]).unwrap(), [held]);
        assert_eq!(
            store.versions_under(relay.id.short()).unwrap(),
            [(lantern(), relay.id.clone())]
        );
    }

    #[test]
    fn a_directory_holding_no_version_is_no_document() {
        let (_dir, store) = scratch();
        let directory = store.document_dir(&lantern());
        fs::create_dir_all(&directory).unwrap();
        let staged = format!("{STAGING_PREFIX}{}", VersionId::of(b"half"));
        fs::write(directory.join(staged), "half").unwrap();
        for prefix in ["7", "7f", LANTERN] {
            assert!(
                store.documents_under(prefix).unwrap().is_empty(),
                "{prefix}"
            );
        }
        let held = store.document(&lantern()).unwrap();
        assert_eq!(held.state(), State::Absent);
        assert!(held.unreadable().is_empty());
        assert!(store.unreadable().unwrap().is_empty());
        assert!(store.kinds().unwrap().is_empty());
    }

    #[test]
    fn holding_none_of_no_values_reads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_directory = dir.path().join("store");
        fs::write(&not_a_directory, "a file").unwrap();
        let store = FsStore::new(not_a_directory);
        assert!(store.holding("name", &["relay"]).is_err());
        assert_eq!(store.holding("name", &[]), Ok(Vec::new()));
    }

    #[test]
    fn a_second_question_reads_nothing() {
        let (dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        for body in ["left\n", "right\n"] {
            store.put(&after(&[&relay], RELAY, body)).unwrap();
        }
        let topic = first(&atlas, "topic", "name = \"atlas\"", "\n");
        store.put(&topic).unwrap();
        let torn = VersionId::of(b"torn");
        fs::write(store.version_path(&atlas, &torn), "damaged").unwrap();
        let fact = Kind::parse("fact").unwrap();
        let forked = store.of_kind(&fact).unwrap();
        let damaged = store.document(&atlas).unwrap();
        assert_eq!(damaged.unreadable().len(), 1);

        fs::remove_dir_all(dir.path()).unwrap();
        assert_eq!(
            store.of_kind(&topic.envelope.kind).unwrap(),
            std::slice::from_ref(&damaged)
        );
        assert_eq!(store.kinds().unwrap(), [fact, topic.envelope.kind.clone()]);
        assert_eq!(store.holding("name", &["relay"]).unwrap(), forked);
        assert_eq!(store.forked().unwrap(), forked);
        assert_eq!(store.unreadable().unwrap(), std::slice::from_ref(&damaged));
        assert_eq!(store.document(&atlas).unwrap(), damaged);
        let absent = DocumentId::from_bytes([0x42; 16]);
        assert_eq!(store.document(&absent).unwrap().state(), State::Absent);
    }

    #[test]
    fn a_version_put_after_a_question_is_in_every_later_answer() {
        let (dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let fact = Kind::parse("fact").unwrap();
        assert_eq!(store.of_kind(&fact).unwrap().len(), 1);

        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        store.put(&renamed).unwrap();
        let topic = first(&atlas, "topic", "name = \"atlas\"", "\n");
        store.put(&topic).unwrap();
        let afresh = FsStore::new(dir.path().to_path_buf());
        let held = afresh.document(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(&renamed));
        assert_eq!(store.document(&lantern()).unwrap(), held);
        assert_eq!(store.of_kind(&fact).unwrap(), std::slice::from_ref(&held));
        assert_eq!(store.holding("name", &["relay-pin"]).unwrap(), [held]);
        assert!(store.holding("name", &["relay"]).unwrap().is_empty());
        assert_eq!(store.kinds().unwrap(), [fact, topic.envelope.kind.clone()]);
        assert_eq!(
            store.of_kind(&topic.envelope.kind).unwrap(),
            [afresh.document(&atlas).unwrap()]
        );
        assert_eq!(store.documents_under("7").unwrap(), [lantern(), atlas]);
    }

    #[test]
    fn a_version_put_after_a_walk_is_answered_without_reading() {
        let (dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let fact = Kind::parse("fact").unwrap();
        assert_eq!(store.of_kind(&fact).unwrap().len(), 1);

        let behind = VersionId::of(b"written behind the instance");
        fs::write(store.version_path(&lantern(), &behind), "damaged").unwrap();
        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        store.put(&renamed).unwrap();
        store.put(&renamed).unwrap();
        let topic = first(&atlas, "topic", "name = \"atlas\"", "\n");
        store.put(&topic).unwrap();
        fs::remove_dir_all(dir.path()).unwrap();

        let held = store.document(&lantern()).unwrap();
        assert_eq!(held.history(), [&renamed, &relay]);
        assert_eq!(held.state(), State::Live(&renamed));
        assert!(held.unreadable().is_empty());
        let named = store.document(&atlas).unwrap();
        assert_eq!(named.state(), State::Live(&topic));
        assert_eq!(store.of_kind(&fact).unwrap(), std::slice::from_ref(&held));
        assert_eq!(
            store.of_kind(&topic.envelope.kind).unwrap(),
            std::slice::from_ref(&named)
        );
        assert_eq!(store.holding("name", &["relay-pin"]).unwrap(), [held]);
        assert!(store.holding("name", &["relay"]).unwrap().is_empty());
        assert_eq!(store.kinds().unwrap(), [fact, topic.envelope.kind.clone()]);
        assert!(store.unreadable().unwrap().is_empty());
        assert!(store.forked().unwrap().is_empty());
    }

    #[test]
    fn a_second_head_put_after_a_walk_forks_the_document() {
        let (_dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let left = after(&[&relay], RELAY, "left\n");
        store.put(&left).unwrap();
        assert!(store.forked().unwrap().is_empty());

        let right = after(&[&relay], RELAY, "right\n");
        store.put(&right).unwrap();
        let forked = store.forked().unwrap();
        assert_eq!(forked.len(), 1);
        let mut heads = vec![&left, &right];
        heads.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(forked[0].state(), State::Forked(heads));
        assert_eq!(forked, [store.document(&lantern()).unwrap()]);
    }

    #[test]
    fn a_prefix_is_matched_across_and_within_buckets() {
        let (_dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        store.put(&first(&lantern(), "fact", RELAY, "\n")).unwrap();
        store
            .put(&first(&atlas, "topic", "name = \"atlas\"", "\n"))
            .unwrap();
        assert_eq!(
            store.documents_under("7").unwrap(),
            [lantern(), atlas.clone()]
        );
        assert_eq!(
            store.documents_under("7f").unwrap(),
            [lantern(), atlas.clone()]
        );
        assert_eq!(store.documents_under("7fd").unwrap(), [atlas]);
        assert!(store.documents_under("8").unwrap().is_empty());
        assert!(store.documents_under("7f3b").unwrap().is_empty());
    }

    #[test]
    fn a_synced_folder_gets_an_ignore_file_on_the_first_write() {
        let (dir, store) = scratch();
        fs::create_dir_all(dir.path().join(".stfolder")).unwrap();
        store.put(&first(&lantern(), "fact", RELAY, "\n")).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(".stignore")).unwrap(),
            ".DS_Store\n.tmp-*\n"
        );
    }
}
