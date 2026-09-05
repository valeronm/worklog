//! The current version of every document, parsed by kind.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::draft::Draft;
use crate::domain::entry::Entry;
use crate::domain::fact::Fact;
use crate::domain::followup::Followup;
use crate::domain::frontmatter::{FieldError, Fields};
use crate::domain::kind_keys;
use crate::domain::ports::Store;
use crate::domain::slug::{Kind, Slug};
use crate::domain::topic::Topic;
use crate::domain::version::{Document, State, Tombstone, Version, VersionId};

use super::{Deps, Failure};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Doc<T> {
    pub slug: Slug,
    pub version: Version,
    pub data: T,
    /// What a newer worklog wrote into it, which this one cannot amend.
    pub foreign: Option<String>,
}

impl<T> Doc<T> {
    /// The day the current version was written.
    #[must_use]
    pub fn written_day(&self) -> &str {
        let written = &self.version.block.written;
        written.get(..10).unwrap_or(written)
    }
}

/// A tombstone's content, owned so the index outlives the documents.
enum Stone {
    RenamedTo(Slug),
    Removed { note: Option<String> },
}

/// Where a link lands, through any renames.
pub enum Landing<'a> {
    Present,
    Removed(&'a Slug),
    Missing,
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
    /// What the tombstone at each tombstoned slug says.
    tombstones: BTreeMap<Slug, Stone>,
    /// Indexes into `facts`, by topic.
    facts_by_topic: BTreeMap<String, Vec<usize>>,
}

/// What in the version a newer worklog wrote: grammar in its block, or a
/// field its kind does not know.
#[must_use]
pub fn foreign(version: &Version) -> Option<String> {
    version
        .foreign_grammar()
        .or_else(|| unknown_key(version.slug.kind(), &version.fields))
}

fn unknown_key(kind: Kind, fields: &Fields) -> Option<String> {
    match fields.reject_unknown(kind_keys(kind)) {
        Err(FieldError::Unknown(key)) => Some(format!("field `{key}`")),
        _ => None,
    }
}

/// Without a grammar version the file could as well be damaged, so the
/// sentence says both.
#[must_use]
pub fn foreign_reason(what: &str) -> String {
    format!(
        "carries grammar this worklog does not know ({what}): written by a newer worklog, or corrupted"
    )
}

/// The note a read prints for a foreign version.
#[must_use]
pub fn foreign_note(version: &Version) -> Option<String> {
    foreign(version).map(|what| format!("{} {}", version.slug, foreign_reason(&what)))
}

/// A version a newer worklog wrote cannot be amended here.
pub fn refuse_foreign(version: &Version) -> Result<(), Failure> {
    match foreign_note(version) {
        Some(note) => Err(Failure::Refused(format!("{note}; upgrade to change it"))),
        None => Ok(()),
    }
}

/// The live version a command is about to amend.
pub fn live_to_amend(store: &dyn Store, slug: &Slug) -> Result<Version, Failure> {
    let version = live(store, slug)?;
    refuse_foreign(&version)?;
    Ok(version)
}

/// The refusal for a document that has no single live head.
#[must_use]
pub fn not_live(slug: &Slug, document: &Document) -> Failure {
    match document.state() {
        State::Absent => Failure::Refused(format!("no {}: {slug}", slug.kind())),
        State::Live(_) => unreachable!("a live document needs no refusal"),
        State::Tombstoned(_) => Failure::Refused(match document.tombstone() {
            Some(Tombstone::RenamedTo(new)) => format!("{slug} was renamed to {new}"),
            Some(Tombstone::Removed { note: Some(why) }) => {
                let first = why.lines().next().unwrap_or_default();
                format!("{slug} was removed: {first}")
            }
            _ => format!("{slug} was removed"),
        }),
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
#[allow(
    clippy::large_enum_variant,
    reason = "one lives per lookup and none is stored, so a box would only add an allocation"
)]
pub enum Named {
    Version(Version),
    Slug(Slug, Document),
}

pub fn named(deps: &Deps, text: &str, kind: Option<Kind>) -> Result<Named, Failure> {
    let mut found = None;
    if VersionId::is_prefix(text) {
        let hits = deps.store.by_id_prefix(text)?;
        if hits.len() > 1 {
            return Err(Failure::Refused(format!(
                "{text} is a prefix of more than one version; give more of it"
            )));
        }
        // A hit the document then lacks was written between the two
        // reads, and reads as nothing named.
        if let [(slug, id)] = &hits[..] {
            found = deps.store.document(slug)?.get(id).cloned();
        }
    }
    let document = match super::slug_arg(text, kind) {
        Ok(slug) => {
            let document = deps.store.document(&slug)?;
            let named = !document.versions.is_empty() || deps.drafts.read(&slug)?.is_some();
            named.then_some((slug, document))
        }
        Err(e) if found.is_none() => return Err(e),
        Err(_) => None,
    };
    match (found, document) {
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
            State::Tombstoned(_) => {
                let what = match document.tombstone() {
                    Some(Tombstone::RenamedTo(to)) => Stone::RenamedTo(to.clone()),
                    Some(Tombstone::Removed { note }) => Stone::Removed {
                        note: note.map(str::to_owned),
                    },
                    None => unreachable!("a tombstoned document has a tombstone"),
                };
                loaded.tombstones.insert(slug, what);
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
            Ok(data) => {
                let foreign = foreign(&version);
                docs.push(Doc {
                    slug,
                    version,
                    data,
                    foreign,
                });
            }
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
    /// Every document a newer worklog wrote into, with what it wrote.
    pub fn foreign(&self) -> impl Iterator<Item = (&Slug, &str)> {
        let docs = self
            .entries
            .iter()
            .map(|d| (&d.slug, &d.foreign))
            .chain(self.facts.iter().map(|d| (&d.slug, &d.foreign)))
            .chain(self.topics.values().map(|d| (&d.slug, &d.foreign)))
            .chain(self.followups.iter().map(|d| (&d.slug, &d.foreign)));
        docs.filter_map(|(slug, what)| what.as_deref().map(|w| (slug, w)))
    }

    /// A document with a live head or a fork, whatever its kind.
    #[must_use]
    pub fn is_present(&self, slug: &Slug) -> bool {
        self.present.contains(slug)
    }

    /// A slug that reaches a present document, through any renames.
    #[must_use]
    pub fn reaches(&self, slug: &Slug) -> bool {
        matches!(self.landing(slug), Landing::Present)
    }

    #[must_use]
    pub fn landing(&self, slug: &Slug) -> Landing<'_> {
        // Slugs are never reused, so a rename chain cannot loop.
        let mut at = slug;
        loop {
            if self.present.contains(at) {
                return Landing::Present;
            }
            match self.tombstones.get_key_value(at) {
                Some((_, Stone::RenamedTo(to))) => at = to,
                Some((removed, Stone::Removed { .. })) => return Landing::Removed(removed),
                None => return Landing::Missing,
            }
        }
    }

    /// The removed documents with the note each tombstone carries.
    pub fn removed(&self) -> impl Iterator<Item = (&Slug, Option<&str>)> {
        self.tombstones
            .iter()
            .filter_map(|(slug, stone)| match stone {
                Stone::Removed { note } => Some((slug, note.as_deref())),
                Stone::RenamedTo(_) => None,
            })
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
