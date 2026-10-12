use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use worklog::domain::frontmatter::Value as StoredValue;
use worklog::domain::ports::{Store as _, StoreError};
use worklog::domain::slug::{Kind as StoredKind, Slug};
use worklog::domain::version::{
    Document as StoredDocument, State as StoredState, Version as StoredVersion, VersionId,
};
use worklog::fs::FsStore;

use super::{
    Document, Failure, Found, Kind, Problem, State, Store, Value, Version, unmatched, unreadable,
};

const USAGE: &str = "usage";

struct Listed {
    name: String,
    directory: bool,
}

// The store's reader tells what is under a kind's directory apart without following a
// link.
fn listing(directory: &Path) -> std::io::Result<Vec<Listed>> {
    let mut listed = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        listed.push(Listed {
            name: entry.file_name().to_string_lossy().into_owned(),
            directory: entry.file_type()?.is_dir(),
        });
    }
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(listed)
}

// The store's reader opens a kind's directory by its path, through a link.
fn root_listing(root: &Path) -> std::io::Result<Vec<Listed>> {
    let mut listed = listing(root)?;
    for entry in &mut listed {
        entry.directory = root.join(&entry.name).is_dir();
    }
    Ok(listed)
}

fn names_a_version(name: &str) -> bool {
    (name.strip_suffix(".md")).is_some_and(|id| VersionId::parse(id).is_ok())
}

fn kind_of(kind: StoredKind) -> Kind {
    match kind {
        StoredKind::Entry => Kind::Entry,
        StoredKind::Fact => Kind::Fact,
        StoredKind::Topic => Kind::Topic,
        StoredKind::Followup => Kind::Followup,
    }
}

fn stored_kind(kind: Kind) -> StoredKind {
    match kind {
        Kind::Entry => StoredKind::Entry,
        Kind::Fact => StoredKind::Fact,
        Kind::Topic => StoredKind::Topic,
        Kind::Followup => StoredKind::Followup,
    }
}

pub(crate) fn dated(kind: Kind, slug: &str) -> Option<(String, String)> {
    let slug = Slug::of_kind(stored_kind(kind), slug).ok()?;
    let date = slug.date()?;
    let name = slug.name().strip_prefix(date)?.strip_prefix('-')?;
    Some((date.to_owned(), name.to_owned()))
}

pub(crate) fn shaped(slug: &str) -> Option<Kind> {
    Slug::parse(slug).ok().map(|slug| kind_of(slug.kind()))
}

pub(super) fn field_keys(kind: Kind) -> &'static [&'static str] {
    worklog::domain::kind_keys(stored_kind(kind))
}

fn value_of(value: &StoredValue) -> Value {
    match value {
        StoredValue::Scalar(text) => Value::Scalar(text.clone()),
        StoredValue::List(items) => Value::List(items.clone()),
        StoredValue::Map(entries) => Value::Map(entries_of(entries)),
    }
}

fn entries_of(entries: &[(String, StoredValue)]) -> Vec<(String, Value)> {
    (entries.iter())
        .map(|(key, value)| (key.clone(), value_of(value)))
        .collect()
}

pub(super) fn version_of(version: &StoredVersion) -> Version {
    let block = &version.block;
    Version {
        id: version.id.to_string(),
        parents: block.parents.iter().map(ToString::to_string).collect(),
        written: block.written.clone(),
        machine: block.machine.as_str().to_owned(),
        operation: block.operation.clone(),
        fields: (version.fields.iter())
            .map(|(key, value)| (key.to_owned(), value_of(value)))
            .collect(),
        body: version.body.clone(),
        superseded_by: block.superseded_by.as_ref().map(ToString::to_string),
        renamed_from: block.renamed_from.as_ref().map(ToString::to_string),
        tombstone: version.is_tombstone(),
        foreign_block: block.raw.as_deref().map(entries_of),
    }
}

pub(super) fn document_of(
    kind: Kind,
    slug: &str,
    stored: &StoredDocument,
) -> Result<Document, &'static str> {
    let state = match stored.state() {
        StoredState::Absent => return Err("no version of it is a head"),
        StoredState::Live(_) => State::Live,
        StoredState::Tombstoned(head) if head.block.superseded_by.is_some() => State::Renamed,
        StoredState::Tombstoned(_) => State::Removed,
        StoredState::Forked(_) => State::Forked,
        StoredState::Waiting(_) => State::Waiting,
    };
    Ok(Document {
        kind,
        slug: slug.to_owned(),
        state,
        versions: stored.history().into_iter().map(version_of).collect(),
    })
}

fn version_at(path: &Path, name: &str, slug: &Slug) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let id = name.strip_suffix(".md").unwrap_or(name);
    let version = StoredVersion::from_named_text(id, &text).map_err(|error| error.to_string())?;
    if version.slug != *slug {
        return Err(format!("names slug {} but sits under {slug}", version.slug));
    }
    Ok(())
}

struct Walk<'a> {
    root: &'a Path,
    store: FsStore,
    found: Found,
    walked: BTreeSet<(Kind, String)>,
}

impl Walk<'_> {
    fn unreadable(&mut self, path: &str, reason: &dyn fmt::Display) {
        self.found
            .problems
            .push(unreadable(path.to_owned(), reason));
    }

    fn skipped(&mut self, path: String) {
        self.found.problems.push(Problem::Skipped { path });
    }

    fn unread(&mut self, kind: StoredKind, relative: &str) {
        self.found
            .unread
            .insert((kind_of(kind), relative.to_owned()));
    }

    fn document(&mut self, kind: StoredKind, relative: &str, path: &str, names: &[String]) {
        let slug = match Slug::of_kind(kind, relative) {
            Ok(slug) => slug,
            Err(refused) => return self.unreadable(path, &refused),
        };
        self.walked.insert((kind_of(kind), relative.to_owned()));
        let refused = match self.store.document(&slug) {
            Ok(stored) => match document_of(kind_of(kind), relative, &stored) {
                Ok(document) => return self.found.documents.push(document),
                Err(reason) => reason.to_owned(),
            },
            Err(StoreError::Io { location, reason } | StoreError::Corrupt { location, reason }) => {
                format!("{}: {reason}", self.under_root(&location))
            }
        };
        self.unread(kind, relative);
        let mut failed = false;
        for name in names {
            let file = format!("{path}/{name}");
            if let Err(reason) = version_at(&self.root.join(&file), name, &slug) {
                failed = true;
                self.unreadable(&file, &reason);
            }
        }
        if !failed {
            self.unreadable(path, &refused);
        }
    }

    fn under_root(&self, location: &str) -> String {
        let under = Path::new(location).strip_prefix(self.root);
        under.map_or(location.to_owned(), |path| path.display().to_string())
    }

    fn directory(&mut self, kind: StoredKind, relative: &str) {
        let path = if relative.is_empty() {
            kind.dir().to_owned()
        } else {
            format!("{}/{relative}", kind.dir())
        };
        let listed = match listing(&self.root.join(&path)) {
            Ok(listed) => listed,
            Err(error) => {
                self.unread(kind, relative);
                return self.unreadable(&path, &error);
            }
        };
        let mut versions = Vec::new();
        let mut directories = Vec::new();
        for entry in listed {
            if entry.name.starts_with('.') {
                continue;
            }
            if entry.directory {
                directories.push(entry.name);
            } else if names_a_version(&entry.name) {
                versions.push(entry.name);
            } else {
                self.skipped(format!("{path}/{}", entry.name));
            }
        }
        if !versions.is_empty() {
            self.document(kind, relative, &path, &versions);
        } else if directories.is_empty() && !relative.is_empty() {
            self.skipped(path);
        }
        for name in directories {
            let below = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            self.directory(kind, &below);
        }
    }

    fn held_against(&mut self) {
        for kind in StoredKind::ALL {
            match self.store.slugs(kind) {
                Ok(slugs) => {
                    let of = kind_of(kind);
                    let listed = slugs.iter().map(|slug| slug.path().to_owned()).collect();
                    let walked = (self.walked.iter())
                        .filter(|(walked, _)| *walked == of)
                        .map(|(_, slug)| slug.clone())
                        .collect();
                    (self.found.problems).extend(unmatched(of, &walked, &listed));
                }
                Err(
                    StoreError::Io { location, reason } | StoreError::Corrupt { location, reason },
                ) => {
                    let path = self.under_root(&location);
                    self.unreadable(&path, &reason);
                }
            }
        }
    }
}

/// What cannot be converted is collected beside the documents read, so one damaged document
/// does not hide the rest.
pub(crate) fn read(root: &Path) -> Result<Store, Failure> {
    let at = || root.display().to_string();
    let listed = root_listing(root).map_err(|error| Failure::Io {
        path: at(),
        reason: error.to_string(),
    })?;
    let mut walk = Walk {
        root,
        store: FsStore::new(root.to_path_buf()),
        found: Found::default(),
        walked: BTreeSet::new(),
    };
    let mut is_a_store = false;
    for entry in listed {
        let kind = StoredKind::from_dir(&entry.name).filter(|_| entry.directory);
        let expected = entry.name.starts_with('.') || (entry.directory && entry.name == USAGE);
        if let Some(kind) = kind {
            is_a_store = true;
            walk.directory(kind, "");
        } else if !expected {
            walk.skipped(entry.name);
        }
    }
    if !is_a_store {
        return Err(Failure::NotAStore { path: at() });
    }
    walk.held_against();
    Ok(walk.found.store())
}
