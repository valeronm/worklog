//! The store being converted, as its files hold it: every version of every document, with
//! nothing renamed, resolved or parsed past the grammar.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod read;
#[cfg(test)]
mod tests;

pub(crate) use read::{dated, read, shaped};
// The grammars of an operation word, a followup's state, a recheck, a closing note and a
// link are the functions of the crate that wrote the store.
pub(crate) use worklog::app::operation::Operation;
pub(crate) use worklog::domain::followup::FollowupState;
pub(crate) use worklog::domain::links::linked;
pub(crate) use worklog::domain::recheck::Recheck;
pub(crate) use worklog::domain::version::with_note;

/// The kinds in the order a store's documents are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Entry,
    Fact,
    Topic,
    Followup,
}

impl Kind {
    pub(crate) const ALL: [Kind; 4] = [Kind::Entry, Kind::Fact, Kind::Topic, Kind::Followup];

    /// The kind as a version file and the kind's directory spell it.
    pub(crate) fn word(self) -> &'static str {
        match self {
            Kind::Entry => "entry",
            Kind::Fact => "fact",
            Kind::Topic => "topic",
            Kind::Followup => "followup",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.word())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Value {
    Scalar(String),
    List(Vec<String>),
    Map(Vec<(String, Value)>),
}

/// A head is a version no other version of the document names as a parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum State {
    Live,
    /// The one head is a tombstone that names no successor.
    Removed,
    /// The one head is a tombstone naming the slug the document moved to.
    Renamed,
    /// Several heads, each a descendant of every parent the document names and lacks.
    Forked,
    /// Several heads, one of which may precede a version the document lacks.
    Waiting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Version {
    pub(crate) id: String,
    pub(crate) parents: Vec<String>,
    /// As stored; the store accepts any text here.
    pub(crate) written: String,
    pub(crate) machine: String,
    pub(crate) operation: String,
    /// The kind's own fields in the order the file holds them.
    pub(crate) fields: Vec<(String, Value)>,
    pub(crate) body: String,
    /// On the tombstone a rename leaves: the slug the document moved to.
    pub(crate) superseded_by: Option<String>,
    /// On the first version under a new slug: the slug the document moved from, whose
    /// document holds this version's parents.
    pub(crate) renamed_from: Option<String>,
    /// Whether the version ends its document when it is the head.
    pub(crate) tombstone: bool,
    /// The whole version block as the file holds it, kept only when it carries a key the
    /// store's grammar does not know.
    pub(crate) foreign_block: Option<Vec<(String, Value)>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Document {
    pub(crate) kind: Kind,
    pub(crate) slug: String,
    pub(crate) state: State,
    /// A parent before its child, and versions with no order between them by id.
    pub(crate) versions: Vec<Version>,
}

const BLOCK_KEYS: [&str; 6] = [
    "parents",
    "written",
    "machine",
    "operation",
    "superseded_by",
    "renamed_from",
];

impl Document {
    pub(crate) fn foreign_keys(&self, version: &Version) -> Vec<String> {
        let known = read::field_keys(self.kind);
        let fields = (version.fields.iter()).filter(|(key, _)| !known.contains(&key.as_str()));
        let fields = fields.map(|(key, _)| key.as_str());
        let keys = foreign_block_keys(version).into_iter().chain(fields);
        keys.map(str::to_owned).collect()
    }

    pub(crate) fn heads(&self) -> Vec<&Version> {
        let named: BTreeSet<&str> = self.versions.iter().flat_map(parents_of).collect();
        let mut heads: Vec<&Version> = (self.versions.iter())
            .filter(|version| !named.contains(version.id.as_str()))
            .collect();
        heads.sort_by(|a, b| a.id.cmp(&b.id));
        heads
    }
}

/// Every path is relative to the store.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Problem {
    Forked {
        kind: Kind,
        slug: String,
        /// By id.
        heads: Vec<String>,
    },
    /// A version names a parent the store does not hold, whatever the number of heads.
    Waiting {
        kind: Kind,
        slug: String,
        /// The parents named and not held, by id.
        missing: Vec<String>,
    },
    /// A file that is not a version in the writer's shape, or a directory whose versions
    /// make no document; none of the versions of the document at or above `path` are read.
    Unreadable { path: String, reason: String },
    /// A file or a directory no reader of the store looks at.
    Skipped { path: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Store {
    /// By kind, then slug.
    pub(crate) documents: Vec<Document>,
    pub(crate) problems: Vec<Problem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    Io {
        path: String,
        reason: String,
    },
    /// The directory holds none of the kinds' directories.
    NotAStore {
        path: String,
    },
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Io { path, reason } => write!(f, "{path}: {reason}"),
            Failure::NotAStore { path } => {
                let [first @ .., last] = Kind::ALL.map(Kind::word);
                write!(
                    f,
                    "{path}: holds no store: none of {} and {last} is a directory in it",
                    first.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for Failure {}

fn foreign_block_keys(version: &Version) -> Vec<&str> {
    let block = version.foreign_block.iter().flatten();
    let foreign = block.filter(|(key, _)| !BLOCK_KEYS.contains(&key.as_str()));
    foreign.map(|(key, _)| key.as_str()).collect()
}

fn parents_of(version: &Version) -> impl Iterator<Item = &str> {
    version.parents.iter().map(String::as_str)
}

// The versions that cannot be placed, by id, when some name each other as parents.
fn parent_first(mut versions: Vec<Version>) -> Result<Vec<Version>, Vec<String>> {
    versions.sort_by(|a, b| a.id.cmp(&b.id));
    let held: BTreeSet<String> = versions.iter().map(|version| version.id.clone()).collect();
    let mut placed: BTreeSet<String> = BTreeSet::new();
    let mut ordered = Vec::with_capacity(versions.len());
    while !versions.is_empty() {
        let ready = versions.iter().position(|version| {
            parents_of(version).all(|parent| !held.contains(parent) || placed.contains(parent))
        });
        let Some(ready) = ready else {
            return Err(versions.into_iter().map(|version| version.id).collect());
        };
        let next = versions.remove(ready);
        placed.insert(next.id.clone());
        ordered.push(next);
    }
    Ok(ordered)
}

fn unreadable(path: String, reason: impl fmt::Display) -> Problem {
    Problem::Unreadable {
        path,
        reason: reason.to_string(),
    }
}

// The walk and the store's own reader each decide which directories are documents.
fn unmatched(kind: Kind, walked: &BTreeSet<String>, listed: &BTreeSet<String>) -> Vec<Problem> {
    let only =
        |slugs: &BTreeSet<String>, others: &BTreeSet<String>, reason: &str| -> Vec<Problem> {
            (slugs.difference(others))
                .map(|slug| unreadable(format!("{kind}/{slug}"), reason))
                .collect()
        };
    let mut problems = only(
        walked,
        listed,
        "holds version files and the store's reader does not list it",
    );
    problems.extend(only(
        listed,
        walked,
        "the store's reader lists it and no version file was found in it",
    ));
    problems.sort();
    problems
}

#[derive(Default)]
struct Found {
    documents: Vec<Document>,
    // The directories whose versions are not known, each covering every slug under it.
    unread: BTreeSet<(Kind, String)>,
    problems: Vec<Problem>,
}

type Held<'a> = BTreeMap<(Kind, &'a str), BTreeSet<&'a str>>;

impl Found {
    fn store(self) -> Store {
        let Found {
            documents: found,
            mut unread,
            mut problems,
        } = self;
        let mut documents = Vec::with_capacity(found.len());
        for mut document in found {
            match parent_first(std::mem::take(&mut document.versions)) {
                Ok(versions) => {
                    document.versions = versions;
                    documents.push(document);
                }
                Err(knotted) => {
                    let reason = format!(
                        "versions name each other as parents: {}",
                        knotted.join(", ")
                    );
                    problems.push(unreadable(
                        format!("{}/{}", document.kind, document.slug),
                        reason,
                    ));
                    unread.insert((document.kind, document.slug));
                }
            }
        }
        documents.sort_by(|a, b| (a.kind, &a.slug).cmp(&(b.kind, &b.slug)));
        let held: Held = (documents.iter())
            .map(|document| {
                let ids = document.versions.iter().map(|version| version.id.as_str());
                ((document.kind, document.slug.as_str()), ids.collect())
            })
            .collect();
        for document in &documents {
            let (kind, slug) = (document.kind, document.slug.clone());
            if document.state == State::Forked {
                let heads = document.heads();
                problems.push(Problem::Forked {
                    kind,
                    slug: slug.clone(),
                    heads: heads.iter().map(|head| head.id.clone()).collect(),
                });
            }
            let missing = missing(document, &held, &unread);
            if !missing.is_empty() {
                problems.push(Problem::Waiting {
                    kind,
                    slug,
                    missing,
                });
            }
        }
        problems.sort();
        problems.dedup();
        Store {
            documents,
            problems,
        }
    }
}

fn is_unread(unread: &BTreeSet<(Kind, String)>, kind: Kind, slug: &str) -> bool {
    unread.iter().any(|(of, directory)| {
        let under = (slug.strip_prefix(directory.as_str()))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        *of == kind && (directory.is_empty() || under)
    })
}

// A version a rename moved has its parents under the slug it moved from.
fn missing(document: &Document, held: &Held, unread: &BTreeSet<(Kind, String)>) -> Vec<String> {
    let mut missing = BTreeSet::new();
    for version in &document.versions {
        let home = version.renamed_from.as_deref().unwrap_or(&document.slug);
        if is_unread(unread, document.kind, home) {
            continue;
        }
        let there = held.get(&(document.kind, home));
        for parent in parents_of(version) {
            if !there.is_some_and(|ids| ids.contains(parent)) {
                missing.insert(parent.to_owned());
            }
        }
    }
    missing.into_iter().collect()
}
