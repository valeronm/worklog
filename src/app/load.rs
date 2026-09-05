//! The current version of every document, parsed by kind.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::draft::Draft;
use crate::domain::entry::Entry;
use crate::domain::fact::Fact;
use crate::domain::followup::Followup;
use crate::domain::frontmatter::{FieldError, Fields};
use crate::domain::ports::Store;
use crate::domain::slug::{Kind, Slug};
use crate::domain::topic::Topic;
use crate::domain::version::{Document, State, Version, VersionId};

use super::{Deps, Failure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc<T> {
    pub slug: Slug,
    pub version: Version,
    pub data: T,
}

impl<T> Doc<T> {
    /// The day the current version was written.
    #[must_use]
    pub fn written_day(&self) -> &str {
        let written = &self.version.block.written;
        written.get(..10).unwrap_or(written)
    }
}

#[derive(Default)]
pub struct Loaded {
    /// Newest first.
    pub entries: Vec<Doc<Entry>>,
    pub facts: Vec<Doc<Fact>>,
    pub topics: BTreeMap<String, Doc<Topic>>,
    /// Oldest first.
    pub followups: Vec<Doc<Followup>>,
    pub forks: Vec<(Slug, Vec<VersionId>)>,
    /// A current version whose fields its kind refuses.
    pub broken: Vec<(Slug, String)>,
    /// Every document with a live head or a fork.
    present: BTreeSet<Slug>,
    /// Each renamed slug and the one its tombstone names.
    renamed: BTreeMap<Slug, Slug>,
    /// Indexes into `facts`, by topic.
    facts_by_topic: BTreeMap<String, Vec<usize>>,
}

/// The refusal for a document that has no single live head.
#[must_use]
pub fn not_live(slug: &Slug, document: &Document) -> Failure {
    match document.state() {
        State::Absent => Failure::Refused(format!("no {}: {slug}", slug.kind())),
        State::Live(_) => unreachable!("a live document needs no refusal"),
        State::Tombstoned(v) => match &v.block.superseded_by {
            Some(new) => Failure::Refused(format!("{slug} was renamed to {new}")),
            None => Failure::Refused(format!("{slug} was removed")),
        },
        State::Forked(heads) => Failure::Refused(format!(
            "{slug} is forked: {} — `worklog resolve {slug}`",
            heads
                .iter()
                .map(|h| h.id.short().to_owned())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The one live version, or the refusal that explains why there is none.
pub fn live(store: &dyn Store, slug: &Slug) -> Result<Version, Failure> {
    let document = store.document(slug)?;
    match document.current() {
        Some(v) => Ok(v.clone()),
        None => Err(not_live(slug, &document)),
    }
}

/// The document a slug reaches through any renames, and the slug it
/// landed on, whatever state that document is in.
pub fn follow(
    store: &dyn Store,
    mut slug: Slug,
    mut document: Document,
) -> Result<(Slug, Document), Failure> {
    // Slugs are never reused, so a rename chain cannot loop.
    while let Some(to) = document.renamed_to().cloned() {
        document = store.document(&to)?;
        slug = to;
    }
    Ok((slug, document))
}

/// A version's first parent, which sits in its own document or, for the
/// version a rename moved, in the old slug's.
pub fn parent(store: &dyn Store, version: &Version) -> Result<Option<Version>, Failure> {
    let Some(id) = version.block.parents.first() else {
        return Ok(None);
    };
    let holder = version.block.renamed_from.as_ref().unwrap_or(&version.slug);
    Ok(store.document(holder)?.get(id).cloned())
}

/// Every document in the store, whatever its kind.
pub fn documents(store: &dyn Store) -> Result<Vec<(Slug, Document)>, Failure> {
    let mut all = Vec::new();
    for kind in Kind::ALL {
        for slug in store.slugs(kind)? {
            let document = store.document(&slug)?;
            all.push((slug, document));
        }
    }
    Ok(all)
}

/// What a command-line word names: a stored version, by an id or a
/// prefix of one, or else a document by its slug, read along with it; a
/// slug with only a draft names a document with no versions. A word that
/// is both is refused rather than guessed.
pub enum Named {
    Version(Version),
    Slug(Slug, Document),
}

pub fn named(deps: &Deps, text: &str, kind: Option<Kind>) -> Result<Named, Failure> {
    let mut found = Vec::new();
    if VersionId::is_prefix(text) {
        for (_, document) in documents(deps.store)? {
            found.extend(
                document
                    .versions
                    .into_iter()
                    .filter(|v| v.id.as_str().starts_with(text)),
            );
            if found.len() > 1 {
                return Err(Failure::Refused(format!(
                    "{text} is a prefix of more than one version; give more of it"
                )));
            }
        }
    }
    let document = match super::slug_arg(text, kind) {
        Ok(slug) => {
            let document = deps.store.document(&slug)?;
            let named = !document.versions.is_empty() || deps.drafts.read(&slug)?.is_some();
            named.then_some((slug, document))
        }
        Err(e) if found.is_empty() => return Err(e),
        Err(_) => None,
    };
    match (found.pop(), document) {
        (Some(version), None) => Ok(Named::Version(version)),
        (None, Some((slug, document))) => Ok(Named::Slug(slug, document)),
        (None, None) => Err(Failure::Refused(format!("no version or document: {text}"))),
        (Some(_), Some(_)) => Err(Failure::Refused(format!(
            "{text} is both a version and a document; give more of the id or the slug's kind"
        ))),
    }
}

pub fn draft(deps: &Deps, slug: &Slug) -> Result<Draft, Failure> {
    deps.drafts
        .read(slug)?
        .ok_or_else(|| Failure::Refused(format!("no draft: {slug}")))
}

/// Every live document of a kind, with its forks and broken files noted.
fn load_kind<T>(
    store: &dyn Store,
    kind: Kind,
    parse: fn(&Fields) -> Result<T, FieldError>,
    loaded: &mut Loaded,
) -> Result<Vec<Doc<T>>, Failure> {
    let mut docs = Vec::new();
    for slug in store.slugs(kind)? {
        let document = store.document(&slug)?;
        let head = match document.state() {
            State::Forked(heads) => {
                // A fork is still a document a link or a claim can name;
                // only its content is undecided.
                loaded.present.insert(slug.clone());
                loaded
                    .forks
                    .push((slug, heads.iter().map(|h| h.id.clone()).collect()));
                continue;
            }
            State::Tombstoned(tombstone) => {
                if let Some(to) = &tombstone.block.superseded_by {
                    loaded.renamed.insert(slug, to.clone());
                }
                continue;
            }
            State::Absent => continue,
            State::Live(v) => v.id.clone(),
        };
        let Some(version) = document.versions.into_iter().find(|v| v.id == head) else {
            continue;
        };
        loaded.present.insert(slug.clone());
        match parse(&version.fields) {
            Ok(data) => docs.push(Doc {
                slug,
                version,
                data,
            }),
            Err(e) => loaded.broken.push((slug, e.to_string())),
        }
    }
    Ok(docs)
}

/// Every live followup, oldest first, without the rest of the store.
pub fn followups(store: &dyn Store) -> Result<Vec<Doc<Followup>>, Failure> {
    let mut scratch = Loaded::default();
    let mut docs = load_kind(store, Kind::Followup, Followup::from_fields, &mut scratch)?;
    docs.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(docs)
}

/// Every live topic, without the rest of the store.
pub fn topics(store: &dyn Store) -> Result<Vec<Doc<Topic>>, Failure> {
    let mut scratch = Loaded::default();
    load_kind(store, Kind::Topic, Topic::from_fields, &mut scratch)
}

/// The topic bound to this host among `topics`, or the refusal naming
/// what to create.
pub fn machine_topic<'a>(
    topics: impl IntoIterator<Item = &'a Doc<Topic>>,
    machine: &str,
) -> Result<&'a Doc<Topic>, Failure> {
    topics
        .into_iter()
        .find(|doc| {
            doc.data
                .machine
                .as_ref()
                .is_some_and(|m| m.as_str() == machine)
        })
        .ok_or_else(|| {
            Failure::Refused(format!(
                "no topic carries `machine: {machine}`; `worklog new topic <name>` with that line first"
            ))
        })
}

pub fn load(store: &dyn Store) -> Result<Loaded, Failure> {
    let mut loaded = Loaded::default();
    loaded.entries = load_kind(store, Kind::Entry, Entry::from_fields, &mut loaded)?;
    loaded.entries.sort_by(|a, b| b.slug.cmp(&a.slug));
    loaded.facts = load_kind(store, Kind::Fact, Fact::from_fields, &mut loaded)?;
    loaded.facts.sort_by(|a, b| a.slug.cmp(&b.slug));
    for (i, fact) in loaded.facts.iter().enumerate() {
        let topic = fact.slug.topic().unwrap_or_default().to_owned();
        loaded.facts_by_topic.entry(topic).or_default().push(i);
    }
    loaded.topics = load_kind(store, Kind::Topic, Topic::from_fields, &mut loaded)?
        .into_iter()
        .map(|doc| (doc.slug.path().to_owned(), doc))
        .collect();
    loaded.followups = load_kind(store, Kind::Followup, Followup::from_fields, &mut loaded)?;
    loaded.followups.sort_by(|a, b| a.slug.cmp(&b.slug));
    Ok(loaded)
}

impl Loaded {
    /// A document with a live head or a fork, whatever its kind.
    #[must_use]
    pub fn is_present(&self, slug: &Slug) -> bool {
        self.present.contains(slug)
    }

    /// A slug that reaches a present document, through any renames.
    #[must_use]
    pub fn reaches(&self, slug: &Slug) -> bool {
        std::iter::successors(Some(slug), |at| self.renamed.get(*at))
            .any(|at| self.present.contains(at))
    }

    /// A topic that exists, forked or not.
    #[must_use]
    pub fn has_topic(&self, name: &str) -> bool {
        Slug::of_kind(Kind::Topic, name).is_ok_and(|slug| self.present.contains(&slug))
    }

    /// The graph's view of the topics.
    pub fn lookup<'s>(&'s self) -> impl Fn(&str) -> Option<&'s Topic> + 's {
        move |name: &str| self.topics.get(name).map(|doc| &doc.data)
    }

    /// The topic bound to the machine name, if the store has one.
    #[must_use]
    pub fn machine_topic(&self, machine: &str) -> Option<(&str, &Topic)> {
        self.topics
            .iter()
            .find(|(_, doc)| {
                doc.data
                    .machine
                    .as_ref()
                    .is_some_and(|m| m.as_str() == machine)
            })
            .map(|(slug, doc)| (slug.as_str(), &doc.data))
    }

    /// Facts under a topic, by name.
    pub fn facts_of<'a>(&'a self, topic: &str) -> impl Iterator<Item = &'a Doc<Fact>> + 'a {
        self.facts_by_topic
            .get(topic)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|&i| &self.facts[i])
    }
}
