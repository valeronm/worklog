use std::fs;
use std::path::PathBuf;

use crate::domain::document::{Document, Unreadable};
use crate::domain::id::{DocumentId, VersionId, is_hex};
use crate::domain::ports::{Store, StoreError};
use crate::domain::version::{Kind, ReadError, Version};

use super::{directories, files, io_error};

/// The folder's ignore file names the same prefix.
const STAGING_PREFIX: &str = ".tmp-";

/// A version's file is never rewritten.
pub struct FsStore {
    root: PathBuf,
}

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
        FsStore { root }
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

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let dir = self.document_dir(&version.envelope.document);
        let target = dir.join(file_name(&version.id));
        if target.exists() {
            return Ok(());
        }
        self.keep_sync_clean()?;
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
        // Syncthing picks up a file as soon as it appears.
        let staging = dir.join(format!("{STAGING_PREFIX}{}", version.id));
        fs::write(&staging, version.text()).map_err(|e| io_error(&staging, &e))?;
        fs::rename(&staging, &target).map_err(|e| io_error(&target, &e))?;
        Ok(())
    }

    fn of_kind(&self, kind: &Kind) -> Result<Vec<DocumentId>, StoreError> {
        let mut found = Vec::new();
        for id in self.documents("")? {
            // Every version of a document is of one kind.
            for name in self.version_ids(&id)? {
                let Ok(version) = parse(self.bytes(&id, &name)?, &name) else {
                    continue;
                };
                let one = Document::new(id.clone(), vec![version], vec![]);
                if let Some(held) = one.kind() {
                    if held == kind {
                        found.push(id);
                    }
                    break;
                }
            }
        }
        Ok(found)
    }

    fn holding(&self, key: &str, value: &str) -> Result<Vec<DocumentId>, StoreError> {
        let mut found = Vec::new();
        for id in self.documents("")? {
            if self.document(&id)?.holds(key, value) {
                found.push(id);
            }
        }
        Ok(found)
    }

    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        self.documents(prefix)
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
    use crate::domain::document::State;
    use crate::domain::testing::{LANTERN, after, first, lantern, store_contract};
    use crate::fs::RandomIds;

    const ATLAS: &str = "7fd0000000000000000000000000000c";
    const RELAY: &str = "name = \"relay\"";

    fn scratch() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    fn path(store: &FsStore, version: &Version) -> PathBuf {
        store.version_path(&version.envelope.document, &version.id)
    }

    #[test]
    fn the_directory_store_keeps_the_contract() {
        let (_dir, store) = scratch();
        store_contract(&store, &RandomIds);
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
        assert_eq!(store.of_kind(&fact).unwrap(), [lantern()]);
        fs::write(path(&store, &rewired), [0xff, 0xfe]).unwrap();
        assert_eq!(store.document(&lantern()).unwrap().unreadable().len(), 2);
        assert!(store.of_kind(&fact).unwrap().is_empty());
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
        assert_eq!(store.holding("name", "relay").unwrap(), [lantern()]);
        assert_eq!(
            store.versions_under(relay.id.short()).unwrap(),
            [(lantern(), relay.id.clone())]
        );
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
