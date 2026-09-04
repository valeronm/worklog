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

pub fn draft(deps: &Deps, slug: &Slug) -> Result<Draft, Failure> {
    deps.drafts
        .read(slug)?
        .ok_or_else(|| Failure::Refused(format!("no draft: {slug}")))
}

/// The head moved out of the document, if it has exactly one and it lives.
fn take_head(mut document: Document) -> Option<Version> {
    let id = document.current()?.id.clone();
    let index = document.versions.iter().position(|v| v.id == id)?;
    Some(document.versions.swap_remove(index))
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
        if let State::Forked(heads) = document.state() {
            // A fork is still a document a link or a claim can name; only
            // its content is undecided.
            loaded.present.insert(slug.clone());
            loaded
                .forks
                .push((slug, heads.iter().map(|h| h.id.clone()).collect()));
            continue;
        }
        let Some(version) = take_head(document) else {
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
