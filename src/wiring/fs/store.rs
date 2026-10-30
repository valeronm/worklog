use std::cell::{OnceCell, Ref, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use crate::domain::document::{Document, State, Unreadable};
use crate::domain::id::{DocumentId, VersionId, is_hex};
use crate::domain::ports::{NotRead, Store, StoreError, StoreRead};
use crate::domain::version::{Header, Kind, ReadError, Version, read_body};

use super::{STAGING_PREFIX, USAGE_DIRECTORY, directories, files, io_error, make_dir, optional};

mod cache;

use cache::{Cache, Remembered};

// A version's file is never rewritten. One instance is one reading of the tree, and a body
// is read from its file as it stands.
pub struct FsStore {
    root: PathBuf,
    cache: Option<Cache>,
    reading: Option<Reading>,
    walked: OnceCell<RefCell<Walked>>,
}

/// The last walk a process made through a store's cache, which a store built `continuing` it
/// starts from in place of the cache's file. Cloning shares the one reading.
#[derive(Clone, Default)]
pub struct Reading(Rc<RefCell<Option<BTreeMap<DocumentId, Remembered>>>>);

type Walked = BTreeMap<DocumentId, Rc<Document>>;

fn file_name(id: &VersionId) -> String {
    format!("{id}.md")
}

fn version_named(name: &str) -> Option<VersionId> {
    VersionId::parse(name.strip_suffix(".md")?).ok()
}

fn text_of(bytes: Vec<u8>) -> Result<String, ReadError> {
    String::from_utf8(bytes).map_err(|_| ReadError::Corrupt)
}

fn empty(id: &DocumentId) -> Rc<Document> {
    Rc::new(Document::new(id.clone(), Vec::new(), Vec::new()))
}

impl FsStore {
    // A `root` that is not there reads as a store holding nothing, and the first version
    // put makes it.
    #[must_use]
    pub fn new(root: PathBuf) -> FsStore {
        FsStore {
            root,
            cache: None,
            reading: None,
            walked: OnceCell::new(),
        }
    }

    /// Walks the tree through the cache kept under `dir`: a document directory whose
    /// modification time the cache recorded is taken as the cache holds it, so a file
    /// rewritten in place under an unchanged directory is read as it was.
    #[must_use]
    pub fn caching(mut self, dir: &Path) -> FsStore {
        self.cache = Some(Cache::at(dir, &self.root));
        self
    }

    /// Walks the tree through its cache as `caching` does, and leaves the cache as it was.
    #[must_use]
    pub fn recalling(mut self) -> FsStore {
        self.cache = self.cache.take().map(Cache::reading_only);
        self
    }

    /// Walks the tree through its cache as `caching` does, starting from the walk `reading`
    /// holds where it holds one, and leaves this walk in it.
    #[must_use]
    pub fn continuing(mut self, reading: &Reading) -> FsStore {
        self.reading = Some(reading.clone());
        self
    }

    pub fn chosen(root: PathBuf) -> Result<FsStore, StoreError> {
        let store = FsStore::new(root);
        let root = &store.root;
        let mut entries = directories(root)?;
        // A store holds its usage log before its first document.
        entries.retain(|name| name != USAGE_DIRECTORY);
        entries.extend(files(root)?);
        let other = entries.iter().any(|name| !name.starts_with('.'));
        if other && store.documents("")?.is_empty() {
            return Err(StoreError::io(
                root.display(),
                "is not empty and holds no store",
            ));
        }
        Ok(store)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
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

    fn read_headers(&self, id: &DocumentId) -> Result<Rc<Document>, StoreError> {
        let listed = self.version_ids(id)?;
        self.headers_of(id, listed)
    }

    // A sync removes a version's file while a walk is on.
    fn headers_of(
        &self,
        id: &DocumentId,
        listed: Vec<VersionId>,
    ) -> Result<Rc<Document>, StoreError> {
        let mut headers = Vec::new();
        let mut unreadable = Vec::new();
        for name in listed {
            let read = match self.text(id, &name)? {
                Ok(text) => Header::read(&name, &text),
                Err(NotRead::Missing) => continue,
                Err(NotRead::Unreadable(why)) => Err(why),
            };
            match read {
                Ok(header) => headers.push(header),
                Err(why) => unreadable.push(Unreadable { version: name, why }),
            }
        }
        Ok(Rc::new(Document::new(id.clone(), headers, unreadable)))
    }

    fn walked(&self) -> Result<Ref<'_, Walked>, StoreError> {
        if let Some(walked) = self.walked.get() {
            return Ok(walked.borrow());
        }
        let held = match &self.cache {
            Some(cache) => self.walk_through(cache, self.documents("")?)?,
            None => self.walk()?,
        };
        Ok(self.walked.get_or_init(|| RefCell::new(held)).borrow())
    }

    fn walk(&self) -> Result<Walked, StoreError> {
        let mut held = Walked::new();
        for id in self.documents("")? {
            let document = self.read_headers(&id)?;
            held.insert(id, document);
        }
        Ok(held)
    }

    // `taken` is the walk's start, so a directory changed after it is recorded within the
    // settling time and read again. A sync removes a document directory while a walk is on.
    fn walk_through(&self, cache: &Cache, listed: Vec<DocumentId>) -> Result<Walked, StoreError> {
        let taken = SystemTime::now();
        let kept = self
            .reading
            .as_ref()
            .and_then(|reading| reading.0.borrow().clone());
        let mut remembered = kept.unwrap_or_else(|| cache.read());
        let mut found = BTreeMap::new();
        let mut changed = false;
        for id in listed {
            let dir = self.document_dir(&id);
            let modified = fs::metadata(&dir).and_then(|metadata| metadata.modified());
            let Some(modified) = optional(&dir, modified)? else {
                continue;
            };
            let document = match remembered.remove(&id) {
                Some(kept) if kept.modified == modified => kept.document,
                _ => {
                    let document = self.read_headers(&id)?;
                    changed |= document.unreadable().is_empty() && cache::settled(taken, modified);
                    document
                }
            };
            found.insert(id, Remembered { modified, document });
        }
        if changed || !remembered.is_empty() {
            cache.write(taken, &found);
        }
        if let Some(reading) = &self.reading {
            let kept = cache::recorded(taken, &found)
                .map(|(id, held)| (id.clone(), held.clone()))
                .collect();
            *reading.0.borrow_mut() = Some(kept);
        }
        Ok(found
            .into_iter()
            .map(|(id, kept)| (id, kept.document))
            .collect())
    }

    /// Reads the tree now unless it was read, so every later header read of this instance
    /// comes from one reading of it.
    pub fn settle(&self) -> Result<(), StoreError> {
        self.walked().map(drop)
    }

    fn matching(
        &self,
        wanted: impl Fn(&Document) -> bool,
    ) -> Result<Vec<Rc<Document>>, StoreError> {
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

    fn text(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<String, NotRead>, StoreError> {
        let path = self.version_path(document, version);
        let Some(bytes) = optional(&path, fs::read(&path))? else {
            return Ok(Err(NotRead::Missing));
        };
        Ok(text_of(bytes).map_err(NotRead::Unreadable))
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

impl StoreRead for FsStore {
    fn headers(&self, id: &DocumentId) -> Result<Rc<Document>, StoreError> {
        match self.walked.get() {
            Some(walked) => Ok(walked.borrow().get(id).map_or_else(|| empty(id), Rc::clone)),
            None => self.read_headers(id),
        }
    }

    fn version(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<Version, NotRead>, StoreError> {
        let text = self.text(document, version)?;
        Ok(text.and_then(|text| Version::read(version, &text).map_err(NotRead::Unreadable)))
    }

    fn body(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<String, NotRead>, StoreError> {
        let text = self.text(document, version)?;
        Ok(text.and_then(|text| {
            read_body(version, &text)
                .map(str::to_owned)
                .map_err(NotRead::Unreadable)
        }))
    }

    fn of_kind(&self, kind: &Kind) -> Result<Vec<Rc<Document>>, StoreError> {
        self.matching(|document| document.kind() == Some(kind))
    }

    fn holding(&self, key: &str, values: &[&str]) -> Result<Vec<Rc<Document>>, StoreError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let asked: BTreeSet<&str> = values.iter().copied().collect();
        self.matching(|document| document.holds_any(key, &asked))
    }

    fn unreadable(&self) -> Result<Vec<Rc<Document>>, StoreError> {
        self.matching(|document| !document.unreadable().is_empty())
    }

    fn forked(&self) -> Result<Vec<Rc<Document>>, StoreError> {
        self.matching(|document| matches!(document.state(), State::Forked(_)))
    }

    fn kinds(&self) -> Result<Vec<Kind>, StoreError> {
        let walked = self.walked()?;
        let held: BTreeSet<&Kind> = walked.values().filter_map(|held| held.kind()).collect();
        Ok(held.into_iter().cloned().collect())
    }

    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        if let Some(walked) = self.walked.get() {
            let walked = walked.borrow();
            return Ok(walked
                .iter()
                .filter(|(id, held)| {
                    id.as_str().starts_with(prefix)
                        && !(held.headers().is_empty() && held.unreadable().is_empty())
                })
                .map(|(id, _)| id.clone())
                .collect());
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
        if let Some(walked) = self.walked.get() {
            for (document, held) in walked.borrow().iter() {
                // A version put over its file that did not read is a header and an unreadable
                // file both.
                let unread = (held.unreadable().iter().map(|file| &file.version))
                    .filter(|version| held.get(version).is_none());
                let named = (held.headers().iter().map(|header| &header.id))
                    .chain(unread)
                    .filter(|version| version.starts_with(prefix));
                found.extend(named.map(|version| (document.clone(), version.clone())));
            }
            return Ok(found);
        }
        for document in self.documents("")? {
            for version in self.version_ids(&document)? {
                if version.starts_with(prefix) {
                    found.push((document.clone(), version));
                }
            }
        }
        Ok(found)
    }

    fn reread(&self) -> Result<(), StoreError> {
        let held = self.walk()?;
        match self.walked.get() {
            Some(walked) => *walked.borrow_mut() = held,
            None => drop(self.walked.set(RefCell::new(held))),
        }
        Ok(())
    }
}

impl Store for FsStore {
    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let document = &version.envelope.document;
        let dir = self.document_dir(document);
        let target = dir.join(file_name(&version.id));
        if !target.exists() {
            self.keep_sync_clean()?;
            make_dir(&dir)?;
            // Syncthing picks up a file as soon as it appears.
            let staging = dir.join(format!("{STAGING_PREFIX}{}", version.id));
            fs::write(&staging, version.text()).map_err(|e| io_error(&staging, &e))?;
            fs::rename(&staging, &target).map_err(|e| io_error(&target, &e))?;
        }
        if let Some(walked) = self.walked.get() {
            let mut walked = walked.borrow_mut();
            let held = walked
                .entry(document.clone())
                .or_insert_with(|| empty(document));
            *held = Rc::new(held.with_version(version.header().clone()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::domain::testing::{LANTERN, after, first, lantern, store_contract};
    use crate::wiring::fs::RandomIds;

    const ATLAS: &str = "7fd0000000000000000000000000000c";
    const RELAY: &str = "name = \"relay\"";

    fn scratch() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    fn reread(store: &FsStore, id: &DocumentId) {
        if let Some(walked) = store.walked.get() {
            let document = store.read_headers(id).unwrap();
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
        for digit in "0123456789abcdef".chars() {
            let prefix = digit.to_string();
            assert_eq!(
                afresh.documents_under(&prefix).unwrap(),
                store.documents_under(&prefix).unwrap(),
                "{prefix}"
            );
        }
        let fact = Kind::parse("fact").unwrap();
        let known = store.of_kind(&fact).unwrap()[0].heads()[0].id.clone();
        for prefix in [&known.as_str()[..11], known.short()] {
            let found = store.versions_under(prefix).unwrap();
            assert_eq!(found.len(), 1, "{prefix}");
            assert_eq!(found[0].1, known, "{prefix}");
            assert_eq!(afresh.versions_under(prefix).unwrap(), found, "{prefix}");
        }
        assert_eq!(afresh.unreadable().unwrap(), store.unreadable().unwrap());
        assert_eq!(afresh.kinds().unwrap(), store.kinds().unwrap());
        assert_eq!(afresh.forked().unwrap(), store.forked().unwrap());
    }

    #[test]
    fn a_root_that_is_not_there_reads_as_a_store_holding_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            FsStore::new(dir.path().join("atlas"))
                .kinds()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_store_is_chosen_where_nothing_is_or_a_store_is() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        assert!(FsStore::chosen(root.clone()).is_ok());
        assert!(!root.exists());
        fs::create_dir(&root).unwrap();
        assert!(FsStore::chosen(root.clone()).is_ok());

        let store = FsStore::new(root.clone());
        store.put(&first(&lantern(), "fact", RELAY, "\n")).unwrap();
        assert!(FsStore::chosen(root.clone()).is_ok());
        fs::create_dir(root.join(".stfolder")).unwrap();
        fs::write(root.join(".stignore"), ".DS_Store\n").unwrap();
        fs::write(root.join(".DS_Store"), "x").unwrap();
        let chosen = FsStore::chosen(root).unwrap();
        assert_eq!(chosen.documents_under("7f").unwrap(), [lantern()]);
    }

    #[test]
    fn a_directory_of_dot_named_entries_alone_is_chosen_as_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let synced = dir.path().join("atlas");
        fs::create_dir_all(synced.join(".stfolder")).unwrap();
        assert!(FsStore::chosen(synced.clone()).is_ok());
        let ignoring = dir.path().join("phone");
        fs::create_dir(&ignoring).unwrap();
        fs::write(ignoring.join(".stignore"), ".DS_Store\n").unwrap();
        assert!(FsStore::chosen(ignoring).is_ok());

        fs::write(synced.join("notes.txt"), "a person's own").unwrap();
        let Err(refused) = FsStore::chosen(synced.clone()) else {
            panic!("a directory that holds no store must be refused");
        };
        assert_eq!(
            refused.to_string(),
            format!("{}: is not empty and holds no store", synced.display())
        );
    }

    #[test]
    fn a_directory_holding_the_usage_log_alone_is_chosen_as_an_empty_one() {
        let dir = tempfile::tempdir().unwrap();
        let logged = dir.path().join("atlas");
        fs::create_dir_all(logged.join(USAGE_DIRECTORY)).unwrap();
        let line = "{}\n";
        fs::write(
            logged.join(USAGE_DIRECTORY).join("desk-2026-09.jsonl"),
            line,
        )
        .unwrap();
        fs::create_dir(logged.join(".stfolder")).unwrap();
        assert!(FsStore::chosen(logged.clone()).is_ok());
        fs::write(logged.join("notes.txt"), "a person's own").unwrap();
        assert!(FsStore::chosen(logged).is_err());

        let named = dir.path().join("phone");
        fs::create_dir(&named).unwrap();
        fs::write(named.join(USAGE_DIRECTORY), "a file").unwrap();
        let Err(refused) = FsStore::chosen(named.clone()) else {
            panic!("a file named as the log is no log");
        };
        assert_eq!(
            refused.to_string(),
            format!("{}: is not empty and holds no store", named.display())
        );
    }

    #[test]
    fn a_directory_of_other_files_is_not_chosen_for_a_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("atlas");
        fs::create_dir_all(root.join("7f").join("not-a-document")).unwrap();
        fs::create_dir(root.join("ab")).unwrap();
        fs::write(root.join("7f").join(LANTERN), "a file named as a document").unwrap();
        fs::write(root.join("notes.md"), "a person's own").unwrap();
        fs::write(root.join(".stignore"), ".DS_Store\n").unwrap();
        let Err(refused) = FsStore::chosen(root.clone()) else {
            panic!("a directory that holds no store must be refused");
        };
        assert_eq!(
            refused.to_string(),
            format!("{}: is not empty and holds no store", root.display())
        );

        let file = dir.path().join("phone");
        fs::write(&file, "a file").unwrap();
        assert!(FsStore::chosen(file).is_err());
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
        let held = store.headers(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(rewired.header()));
        assert_eq!(
            held.unreadable(),
            [Unreadable {
                version: relay.id.clone(),
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
        let held = store.headers(&lantern()).unwrap();
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
        let held = store.headers(&lantern()).unwrap();
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
        let held = store.headers(&lantern()).unwrap();
        assert_eq!(held.history(), [relay.header()]);
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
        let held = store.headers(&lantern()).unwrap();
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
        let damaged = store.headers(&atlas).unwrap();
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
        assert_eq!(store.headers(&atlas).unwrap(), damaged);
        let absent = DocumentId::from_bytes([0x42; 16]);
        assert_eq!(store.headers(&absent).unwrap().state(), State::Absent);
    }

    #[test]
    fn a_settled_store_reads_no_file_for_a_document_or_a_prefix() {
        let (dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        store.settle().unwrap();
        store.settle().unwrap();

        fs::remove_dir_all(dir.path()).unwrap();
        let held = store.headers(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(relay.header()));
        assert_eq!(store.documents_under(LANTERN).unwrap(), [lantern()]);
        assert_eq!(
            store.versions_under(relay.id.short()).unwrap(),
            [(lantern(), relay.id.clone())]
        );
        let unsettled = FsStore::new(dir.path().to_path_buf());
        assert_eq!(
            unsettled.headers(&lantern()).unwrap().state(),
            State::Absent
        );
    }

    #[test]
    fn headers_before_a_walk_read_the_one_directory_and_walk_nothing() {
        let (dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        store
            .put(&first(&atlas, "topic", "name = \"atlas\"", "\n"))
            .unwrap();
        let unwalked = FsStore::new(dir.path().to_path_buf());
        let held = unwalked.headers(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(relay.header()));
        assert!(unwalked.walked.get().is_none());
        fs::remove_dir_all(unwalked.document_dir(&atlas)).unwrap();
        assert_eq!(unwalked.headers(&atlas).unwrap().state(), State::Absent);
    }

    #[test]
    fn a_version_and_its_body_read_from_the_file_as_it_stands() {
        let (_dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        store.settle().unwrap();
        assert_eq!(
            store.version(&lantern(), &relay.id).unwrap(),
            Ok(relay.clone())
        );
        assert_eq!(
            store.body(&lantern(), &relay.id).unwrap().as_deref(),
            Ok("first\n")
        );
        fs::write(path(&store, &relay), "rewritten in place").unwrap();
        assert_eq!(
            store.headers(&lantern()).unwrap().state(),
            State::Live(relay.header())
        );
        let corrupt = NotRead::Unreadable(ReadError::Corrupt);
        assert_eq!(
            store.version(&lantern(), &relay.id).unwrap(),
            Err(corrupt.clone())
        );
        assert_eq!(
            store.body(&lantern(), &relay.id).unwrap(),
            Err(corrupt.clone())
        );
        fs::write(path(&store, &relay), [0xff, 0xfe]).unwrap();
        assert_eq!(store.body(&lantern(), &relay.id).unwrap(), Err(corrupt));
        fs::remove_file(path(&store, &relay)).unwrap();
        assert_eq!(
            store.version(&lantern(), &relay.id).unwrap(),
            Err(NotRead::Missing)
        );
    }

    fn cached(dir: &Path) -> FsStore {
        FsStore::new(dir.join("store")).caching(&dir.join("cache"))
    }

    // The relay fact put, its directory settled and the cache written for it.
    fn stocked(dir: &Path) -> (FsStore, Version) {
        let store = cached(dir);
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        settled(&store, &lantern());
        store.settle().unwrap();
        (store, relay)
    }

    // A directory changed within the cache's settling time is read again whatever the
    // cache holds, so a test of what the cache answers backdates the directory first.
    fn settled(store: &FsStore, id: &DocumentId) {
        let dir = store.document_dir(id);
        let earlier = SystemTime::now() - Duration::from_hours(1);
        fs::File::open(&dir).unwrap().set_modified(earlier).unwrap();
    }

    #[test]
    fn a_tree_that_did_not_change_is_answered_from_the_cache_until_it_is_reread() {
        let dir = tempfile::tempdir().unwrap();
        let (store, relay) = stocked(dir.path());

        fs::write(path(&store, &relay), "damaged").unwrap();
        let afresh = cached(dir.path());
        afresh.settle().unwrap();
        assert_eq!(
            afresh.headers(&lantern()).unwrap().state(),
            State::Live(relay.header())
        );
        assert_eq!(afresh.of_kind(&relay.envelope.kind).unwrap().len(), 1);
        assert!(afresh.version(&lantern(), &relay.id).unwrap().is_err());
        afresh.reread().unwrap();
        assert_eq!(afresh.headers(&lantern()).unwrap().state(), State::Absent);
        assert_eq!(afresh.unreadable().unwrap().len(), 1);
        let unwalked = cached(dir.path());
        unwalked.reread().unwrap();
        assert_eq!(unwalked.unreadable().unwrap().len(), 1);
    }

    #[test]
    fn a_directory_changed_within_the_settling_time_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let store = cached(dir.path());
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        store.settle().unwrap();
        assert!(!dir.path().join("cache").exists());

        settled(&store, &lantern());
        cached(dir.path()).settle().unwrap();
        assert!(dir.path().join("cache").exists());
    }

    #[test]
    fn a_version_that_landed_in_a_known_directory_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let (_, relay) = stocked(dir.path());

        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        cached(dir.path()).put(&renamed).unwrap();
        let afresh = cached(dir.path());
        afresh.settle().unwrap();
        let held = afresh.headers(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(renamed.header()));
        assert_eq!(held.history(), [renamed.header(), relay.header()]);
        assert_eq!(afresh.holding("name", &["relay-pin"]).unwrap(), [held]);
    }

    #[test]
    fn a_recalling_store_reads_what_changed_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (_, relay) = stocked(dir.path());
        let file = cache_file(dir.path());
        let written = fs::read(&file).unwrap();

        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        cached(dir.path()).put(&renamed).unwrap();
        let recalling = cached(dir.path()).recalling();
        recalling.settle().unwrap();
        assert_eq!(
            recalling.headers(&lantern()).unwrap().state(),
            State::Live(renamed.header())
        );
        assert_eq!(fs::read(&file).unwrap(), written);
    }

    #[test]
    fn a_document_that_appeared_is_found_and_one_that_is_gone_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = stocked(dir.path());

        let atlas = DocumentId::parse(ATLAS).unwrap();
        let topic = first(&atlas, "topic", "name = \"atlas\"", "\n");
        cached(dir.path()).put(&topic).unwrap();
        fs::remove_dir_all(store.document_dir(&lantern())).unwrap();
        let afresh = cached(dir.path());
        afresh.settle().unwrap();
        assert_eq!(
            afresh.headers(&atlas).unwrap().state(),
            State::Live(topic.header())
        );
        assert_eq!(afresh.headers(&lantern()).unwrap().state(), State::Absent);
        assert_eq!(
            afresh.kinds().unwrap(),
            std::slice::from_ref(&topic.envelope.kind)
        );
        assert_eq!(afresh.documents_under("7").unwrap(), [atlas]);
    }

    fn cache_file(dir: &Path) -> PathBuf {
        fs::read_dir(dir.join("cache"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .next()
            .unwrap()
    }

    #[test]
    fn a_held_reading_carries_each_walk_to_the_next_in_place_of_the_cache_file() {
        let dir = tempfile::tempdir().unwrap();
        let (store, relay) = stocked(dir.path());
        let reading = Reading::default();
        cached(dir.path()).continuing(&reading).settle().unwrap();

        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        store.put(&renamed).unwrap();
        settled(&store, &lantern());
        let next = cached(dir.path()).continuing(&reading);
        next.settle().unwrap();
        assert_eq!(
            next.headers(&lantern()).unwrap().state(),
            State::Live(renamed.header())
        );

        fs::write(cache_file(dir.path()), "not a cache").unwrap();
        fs::write(path(&store, &renamed), "damaged").unwrap();
        let later = cached(dir.path()).continuing(&reading);
        later.settle().unwrap();
        assert_eq!(
            later.headers(&lantern()).unwrap().state(),
            State::Live(renamed.header())
        );
        assert_eq!(
            cached(dir.path()).headers(&lantern()).unwrap().state(),
            State::Live(relay.header())
        );
    }

    #[test]
    fn a_document_directory_gone_since_the_listing_is_passed_over() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = stocked(dir.path());
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let topic = first(&atlas, "topic", "name = \"atlas\"", "\n");
        store.put(&topic).unwrap();

        let afresh = cached(dir.path());
        let listed = afresh.documents("").unwrap();
        assert!(listed.contains(&lantern()));
        fs::remove_dir_all(afresh.document_dir(&lantern())).unwrap();
        let walked = afresh
            .walk_through(afresh.cache.as_ref().unwrap(), listed)
            .unwrap();
        assert_eq!(walked.keys().collect::<Vec<_>>(), [&atlas]);
    }

    #[test]
    fn a_version_file_gone_since_its_listing_is_passed_over() {
        let (_dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        store.put(&renamed).unwrap();

        let listed = store.version_ids(&lantern()).unwrap();
        assert!(listed.contains(&renamed.id));
        fs::remove_file(path(&store, &renamed)).unwrap();
        let held = store.headers_of(&lantern(), listed).unwrap();
        assert_eq!(held.state(), State::Live(relay.header()));
        assert!(held.unreadable().is_empty());
    }

    #[test]
    fn a_document_holding_a_file_that_does_not_read_is_read_every_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = cached(dir.path());
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let torn = VersionId::of(b"torn");
        fs::write(store.version_path(&lantern(), &torn), "damaged").unwrap();
        settled(&store, &lantern());
        store.settle().unwrap();
        assert_eq!(
            store.headers(&lantern()).unwrap().state(),
            State::Live(relay.header())
        );

        fs::write(path(&store, &relay), "damaged too").unwrap();
        let afresh = cached(dir.path());
        afresh.settle().unwrap();
        let held = afresh.headers(&lantern()).unwrap();
        assert_eq!(held.state(), State::Absent);
        assert_eq!(held.unreadable().len(), 2);
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
        let held = afresh.headers(&lantern()).unwrap();
        assert_eq!(held.state(), State::Live(renamed.header()));
        assert_eq!(store.headers(&lantern()).unwrap(), held);
        assert_eq!(store.of_kind(&fact).unwrap(), std::slice::from_ref(&held));
        assert_eq!(store.holding("name", &["relay-pin"]).unwrap(), [held]);
        assert!(store.holding("name", &["relay"]).unwrap().is_empty());
        assert_eq!(store.kinds().unwrap(), [fact, topic.envelope.kind.clone()]);
        assert_eq!(
            store.of_kind(&topic.envelope.kind).unwrap(),
            [afresh.headers(&atlas).unwrap()]
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

        let held = store.headers(&lantern()).unwrap();
        assert_eq!(held.history(), [renamed.header(), relay.header()]);
        assert_eq!(held.state(), State::Live(renamed.header()));
        assert!(held.unreadable().is_empty());
        let named = store.headers(&atlas).unwrap();
        assert_eq!(named.state(), State::Live(topic.header()));
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
    fn a_prefix_lookup_after_a_walk_reads_no_directory() {
        let (dir, store) = scratch();
        let atlas = DocumentId::parse(ATLAS).unwrap();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        store.put(&relay).unwrap();
        let torn = VersionId::of(b"torn");
        fs::write(store.version_path(&lantern(), &torn), "damaged").unwrap();
        let empty = DocumentId::from_bytes([0x7f; 16]);
        fs::create_dir_all(store.document_dir(&empty)).unwrap();
        assert_eq!(store.kinds().unwrap().len(), 1);

        let topic = first(&atlas, "topic", "name = \"atlas\"", "\n");
        store.put(&topic).unwrap();
        fs::remove_dir_all(dir.path()).unwrap();

        assert_eq!(
            store.documents_under("7").unwrap(),
            [lantern(), atlas.clone()]
        );
        assert_eq!(
            store.documents_under("7fd").unwrap(),
            std::slice::from_ref(&atlas)
        );
        assert!(store.documents_under(empty.as_str()).unwrap().is_empty());
        assert!(store.documents_under("").unwrap().is_empty());
        assert_eq!(
            store.versions_under(relay.id.short()).unwrap(),
            [(lantern(), relay.id.clone())]
        );
        assert_eq!(
            store.versions_under(torn.short()).unwrap(),
            [(lantern(), torn)]
        );
        assert_eq!(
            store.versions_under(topic.id.short()).unwrap(),
            [(atlas, topic.id.clone())]
        );
    }

    #[test]
    fn a_version_put_over_its_file_that_did_not_read_is_found_once_by_its_prefix() {
        let (_dir, store) = scratch();
        let relay = first(&lantern(), "fact", RELAY, "first\n");
        let path = path(&store, &relay);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "damaged").unwrap();
        assert_eq!(store.unreadable().unwrap().len(), 1);

        store.put(&relay).unwrap();
        assert_eq!(
            store.versions_under(relay.id.short()).unwrap(),
            [(lantern(), relay.id.clone())]
        );
        assert_eq!(store.documents_under(LANTERN).unwrap(), [lantern()]);
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
        let mut heads = vec![left.header(), right.header()];
        heads.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(forked[0].state(), State::Forked(heads));
        assert_eq!(forked, [store.headers(&lantern()).unwrap()]);
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
