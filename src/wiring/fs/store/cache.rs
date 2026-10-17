//! What a host keeps of a store's parsed tree, outside the store, so a reading reuses the
//! parse of every document directory that did not change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::domain::document::Document;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::version::{Envelope, FORMAT, Fields, Kind, Links, Stamp, Version};

use super::super::replace;

/// Raised with any change to the records below, since a build of the same version reads
/// the file the last one wrote.
const CACHE_FORMAT: u32 = 1;

/// A directory changed this close to the walk that recorded it may have changed again
/// within the granularity of its modification time, and is read again.
const SETTLING: Duration = Duration::from_secs(2);

pub struct Cache {
    file: PathBuf,
    writing: bool,
}

/// A document as its directory held it when the directory's modification time was `modified`.
#[derive(Clone, Debug, PartialEq)]
pub struct Remembered {
    pub modified: SystemTime,
    pub document: Document,
}

#[derive(Serialize, Deserialize)]
struct Held {
    cache_format: u32,
    store_format: u32,
    binary: String,
    documents: Vec<DocumentRecord>,
}

#[derive(Serialize, Deserialize)]
struct DocumentRecord {
    id: String,
    modified: SystemTime,
    versions: Vec<VersionRecord>,
}

#[derive(Serialize, Deserialize)]
struct VersionRecord {
    id: String,
    kind: String,
    parents: Vec<String>,
    ancestors: Vec<String>,
    written: String,
    machine: String,
    change: String,
    fields: Vec<(String, Value)>,
    links: Vec<(String, Option<String>)>,
    text: String,
}

// postcard reads no self-describing value, which a TOML value is.
#[derive(Serialize, Deserialize)]
enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Datetime(String),
    Array(Vec<Value>),
    Table(Vec<(String, Value)>),
}

/// Whether a directory last changed at `modified` is recorded by a walk begun at `taken`.
pub fn settled(taken: SystemTime, modified: SystemTime) -> bool {
    taken
        .checked_sub(SETTLING)
        .is_some_and(|before| modified < before)
}

fn value_of(value: &toml::Value) -> Value {
    match value {
        toml::Value::String(text) => Value::String(text.clone()),
        toml::Value::Integer(number) => Value::Integer(*number),
        toml::Value::Float(number) => Value::Float(*number),
        toml::Value::Boolean(flag) => Value::Boolean(*flag),
        toml::Value::Datetime(datetime) => Value::Datetime(datetime.to_string()),
        toml::Value::Array(items) => Value::Array(items.iter().map(value_of).collect()),
        toml::Value::Table(table) => Value::Table(table_of(table)),
    }
}

fn table_of(table: &toml::Table) -> Vec<(String, Value)> {
    table
        .iter()
        .map(|(key, value)| (key.clone(), value_of(value)))
        .collect()
}

fn toml_of(value: Value) -> Option<toml::Value> {
    Some(match value {
        Value::String(text) => toml::Value::String(text),
        Value::Integer(number) => toml::Value::Integer(number),
        Value::Float(number) => toml::Value::Float(number),
        Value::Boolean(flag) => toml::Value::Boolean(flag),
        Value::Datetime(text) => toml::Value::Datetime(text.parse().ok()?),
        Value::Array(items) => {
            toml::Value::Array(items.into_iter().map(toml_of).collect::<Option<_>>()?)
        }
        Value::Table(pairs) => toml::Value::Table(fields_of(pairs)?),
    })
}

fn fields_of(pairs: Vec<(String, Value)>) -> Option<Fields> {
    pairs
        .into_iter()
        .map(|(key, value)| Some((key, toml_of(value)?)))
        .collect()
}

fn version_record(version: &Version) -> VersionRecord {
    let envelope = &version.envelope;
    let ids = |ids: &[VersionId]| ids.iter().map(|id| id.as_str().to_owned()).collect();
    VersionRecord {
        id: version.id.as_str().to_owned(),
        kind: envelope.kind.as_str().to_owned(),
        parents: ids(&envelope.parents),
        ancestors: ids(&envelope.ancestors),
        written: envelope.written.to_string(),
        machine: envelope.machine.as_str().to_owned(),
        change: envelope.change.clone(),
        fields: table_of(&version.fields),
        links: version
            .links
            .0
            .iter()
            .map(|(name, id)| (name.clone(), id.as_ref().map(|id| id.as_str().to_owned())))
            .collect(),
        text: version.text().to_owned(),
    }
}

fn version_of(document: &DocumentId, record: VersionRecord) -> Option<Version> {
    let ids = |ids: Vec<String>| {
        ids.iter()
            .map(|id| VersionId::parse(id).ok())
            .collect::<Option<Vec<_>>>()
    };
    let envelope = Envelope {
        document: document.clone(),
        kind: Kind::parse(&record.kind).ok()?,
        parents: ids(record.parents)?,
        ancestors: ids(record.ancestors)?,
        written: Stamp::parse(&record.written).ok()?,
        machine: DocumentId::parse(&record.machine).ok()?,
        change: record.change,
    };
    let links = record
        .links
        .into_iter()
        .map(|(name, id)| {
            let id = match id {
                Some(id) => Some(DocumentId::parse(&id).ok()?),
                None => None,
            };
            Some((name, id))
        })
        .collect::<Option<_>>()?;
    Version::recalled(
        VersionId::parse(&record.id).ok()?,
        envelope,
        fields_of(record.fields)?,
        Links(links),
        record.text,
    )
}

fn document_of(record: DocumentRecord) -> Option<(DocumentId, Remembered)> {
    let id = DocumentId::parse(&record.id).ok()?;
    let versions = record
        .versions
        .into_iter()
        .map(|version| version_of(&id, version))
        .collect::<Option<Vec<_>>>()?;
    let document = Document::new(id.clone(), versions, Vec::new());
    Some((
        id,
        Remembered {
            modified: record.modified,
            document,
        },
    ))
}

impl Cache {
    /// The cache of the store at `store`, among those under `dir`.
    #[must_use]
    pub fn at(dir: &Path, store: &Path) -> Cache {
        let name = blake3::hash(store.as_os_str().as_encoded_bytes()).to_hex();
        Cache {
            file: dir.join(format!("{}.store", &name[..16])),
            writing: true,
        }
    }

    /// The same cache, read and never written.
    #[must_use]
    pub fn reading_only(mut self) -> Cache {
        self.writing = false;
        self
    }

    /// What was written: nothing at all for a file that is not there, does not read, or was
    /// written by another binary.
    #[must_use]
    pub fn read(&self) -> BTreeMap<DocumentId, Remembered> {
        self.recalled().unwrap_or_default()
    }

    fn recalled(&self) -> Option<BTreeMap<DocumentId, Remembered>> {
        let bytes = std::fs::read(&self.file).ok()?;
        let held: Held = postcard::from_bytes(&bytes).ok()?;
        if (held.cache_format, held.store_format, held.binary.as_str())
            != (CACHE_FORMAT, FORMAT, env!("CARGO_PKG_VERSION"))
        {
            return None;
        }
        held.documents.into_iter().map(document_of).collect()
    }

    /// Replaces the file with `documents` as a walk begun at `taken` found them, leaving out
    /// a document holding a file that does not read and one whose directory is not settled.
    /// A file that cannot be written is left as it is, and a cache that only reads writes
    /// nothing.
    pub fn write(&self, taken: SystemTime, documents: &BTreeMap<DocumentId, Remembered>) {
        if !self.writing {
            return;
        }
        let held = Held {
            cache_format: CACHE_FORMAT,
            store_format: FORMAT,
            binary: env!("CARGO_PKG_VERSION").to_owned(),
            documents: documents
                .iter()
                .filter(|(_, held)| {
                    held.document.unreadable().is_empty() && settled(taken, held.modified)
                })
                .map(|(id, held)| DocumentRecord {
                    id: id.as_str().to_owned(),
                    modified: held.modified,
                    versions: held
                        .document
                        .history()
                        .into_iter()
                        .map(version_record)
                        .collect(),
                })
                .collect(),
        };
        // A file torn by a crash does not read, and reads as no cache.
        if let Ok(bytes) = postcard::to_allocvec(&held) {
            let _ = replace(&self.file, &bytes, None, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::domain::document::Unreadable;
    use crate::domain::testing::{after, atlas, desk, first, lantern, stamp};
    use crate::domain::version::ReadError;

    fn scratch() -> (tempfile::TempDir, Cache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(&dir.path().join("cache"), Path::new("/atlas/store"));
        (dir, cache)
    }

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_hours(500_000)
    }

    fn settled() -> SystemTime {
        now() - Duration::from_hours(1)
    }

    fn relay() -> Version {
        let fields: Fields = "name = \"relay\"\ncount = 3\nratio = 0.5\nidea = true\n\
                              when = 2026-01-01T10:00:00+01:00\ntags = [\"a\", \"b\"]\n\
                              nested = { x = 1, y = [false] }\n"
            .parse()
            .unwrap();
        let envelope = Envelope::first(
            lantern(),
            Kind::parse("fact").unwrap(),
            stamp(),
            desk(),
            "new",
        );
        let links = Links(BTreeMap::from([
            ("atlas".to_owned(), Some(atlas())),
            ("unwritten".to_owned(), None),
        ]));
        Version::compose(envelope, fields, links, "The relay.\n".to_owned()).unwrap()
    }

    fn remembered(document: Document, modified: SystemTime) -> Remembered {
        Remembered { modified, document }
    }

    fn held(id: DocumentId, versions: Vec<Version>) -> Document {
        Document::new(id, versions, Vec::new())
    }

    fn topic() -> Document {
        held(
            atlas(),
            vec![first(&atlas(), "topic", "name = \"atlas\"", "\n")],
        )
    }

    #[test]
    fn what_was_written_is_read_back_once_its_directories_settled() {
        let (_dir, cache) = scratch();
        let relay = relay();
        let renamed = after(&[&relay], "name = \"relay-pin\"", "second\n");
        let documents = BTreeMap::from([
            (
                lantern(),
                remembered(held(lantern(), vec![relay, renamed]), settled()),
            ),
            (
                atlas(),
                remembered(topic(), settled() - Duration::from_secs(1)),
            ),
        ]);
        cache.write(now(), &documents);
        assert_eq!(cache.read(), documents);
    }

    #[test]
    fn a_directory_changed_within_the_settling_time_before_the_walk_is_not_kept() {
        let (_dir, cache) = scratch();
        let just_settled = now() - SETTLING - Duration::from_nanos(1);
        let documents = BTreeMap::from([
            (
                lantern(),
                remembered(held(lantern(), vec![relay()]), now() - SETTLING),
            ),
            (atlas(), remembered(topic(), just_settled)),
        ]);
        cache.write(now(), &documents);
        assert_eq!(
            cache.read(),
            BTreeMap::from([(atlas(), remembered(topic(), just_settled))])
        );
    }

    #[test]
    fn a_document_holding_a_file_that_does_not_read_is_not_kept() {
        let (_dir, cache) = scratch();
        let torn = Unreadable {
            version: VersionId::of(b"torn"),
            why: ReadError::Corrupt,
        };
        let torn = Document::new(lantern(), vec![relay()], vec![torn]);
        let documents = BTreeMap::from([
            (lantern(), remembered(torn, settled())),
            (atlas(), remembered(topic(), settled())),
        ]);
        cache.write(now(), &documents);
        assert_eq!(
            cache.read(),
            BTreeMap::from([(atlas(), remembered(topic(), settled()))])
        );
    }

    #[test]
    fn a_file_that_does_not_read_or_another_binary_s_is_nothing() {
        let (_dir, cache) = scratch();
        assert!(cache.read().is_empty());
        fs::create_dir_all(cache.file.parent().unwrap()).unwrap();
        fs::write(&cache.file, b"damaged").unwrap();
        assert!(cache.read().is_empty());
        let documents = BTreeMap::from([(
            lantern(),
            remembered(held(lantern(), vec![relay()]), settled()),
        )]);
        cache.write(now(), &documents);
        assert_eq!(cache.read(), documents);
        let bytes = fs::read(&cache.file).unwrap();
        let mut other: Held = postcard::from_bytes(&bytes).unwrap();
        other.binary = "0.0.0".to_owned();
        fs::write(&cache.file, postcard::to_allocvec(&other).unwrap()).unwrap();
        assert!(cache.read().is_empty());
    }

    #[test]
    fn a_file_that_cannot_be_written_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("cache"),
            b"a file where the directory would be",
        )
        .unwrap();
        let cache = Cache::at(&dir.path().join("cache"), Path::new("/atlas/store"));
        cache.write(
            now(),
            &BTreeMap::from([(
                lantern(),
                remembered(held(lantern(), vec![relay()]), settled()),
            )]),
        );
        assert!(cache.read().is_empty());
        assert_eq!(
            fs::read(dir.path().join("cache")).unwrap(),
            b"a file where the directory would be"
        );
    }

    #[test]
    fn a_cache_that_only_reads_writes_nothing() {
        let (_dir, cache) = scratch();
        let documents = BTreeMap::from([(
            lantern(),
            remembered(held(lantern(), vec![relay()]), settled()),
        )]);
        let reading =
            Cache::at(cache.file.parent().unwrap(), Path::new("/atlas/store")).reading_only();
        reading.write(now(), &documents);
        assert!(!cache.file.exists());
        cache.write(now(), &documents);
        assert_eq!(reading.read(), documents);
        reading.write(now(), &BTreeMap::new());
        assert_eq!(cache.read(), documents);
    }

    #[test]
    fn each_store_has_a_file_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(dir.path(), Path::new("/atlas/store"));
        let other = Cache::at(dir.path(), Path::new("/lantern/store"));
        let documents = BTreeMap::from([(
            lantern(),
            remembered(held(lantern(), vec![relay()]), settled()),
        )]);
        cache.write(now(), &documents);
        assert_eq!(cache.read(), documents);
        assert!(other.read().is_empty());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
