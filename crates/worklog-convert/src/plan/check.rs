use std::collections::BTreeMap;

use toml::{Table, Value};
use worklog::domain::id::DocumentId;
use worklog::domain::schema::graph::{Edges, closes_cycle, reach_from};
use worklog::domain::schema::{Content, KindOf, Record};
use worklog::domain::version::{Links, Stamp};

use super::{Document, Questions, Version};
use crate::question::{Blocked, Open};

fn last<'d>(document: &'d Document<'_>) -> Option<&'d Record> {
    document.head()?.record.as_ref()
}

fn around(topic: &DocumentId, own: &Edges, edges: &BTreeMap<DocumentId, Edges>) -> Vec<DocumentId> {
    let targets = [own.part_of.as_slice(), own.uses.as_slice()].concat();
    let reached = reach_from(&targets, |from| edges.get(from));
    let from = |at: &DocumentId| {
        let met = reached.iter().find(|(met, _)| met == at);
        met.and_then(|(_, from)| from.clone())
    };
    let mut back = Vec::new();
    let mut at = from(topic);
    while let Some(before) = at {
        at = from(&before);
        back.push(before);
    }
    back.push(topic.clone());
    back.reverse();
    back
}

// A cycle is what the new store calls one: a topic that is not ended reaching itself
// through the `part_of` and `uses` of topics that are not ended.
pub(crate) fn cycles(documents: &[Document<'_>], questions: &Questions) {
    let mut names = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for document in documents {
        if let Some(Record {
            content: Content::Topic(topic),
            ending: None,
        }) = last(document)
        {
            names.insert(document.id.clone(), topic.name.as_str().to_owned());
            let (part_of, uses) = (topic.part_of.clone(), topic.uses.clone());
            edges.insert(document.id.clone(), Edges { part_of, uses });
        }
    }
    for (topic, own) in &edges {
        if !closes_cycle(topic, own, |from| edges.get(from)) {
            continue;
        }
        let mut topics: Vec<String> = (around(topic, own, &edges).iter())
            .filter_map(|on| names.get(on).cloned())
            .collect();
        let first = (0..topics.len()).min_by_key(|place| &topics[*place]);
        topics.rotate_left(first.unwrap_or_default());
        questions.ask(Open::Cycle { topics });
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Referred<'f> {
    pub(crate) key: &'static str,
    /// The kind the schema wants of the document named; `None` for any.
    pub(crate) wanted: Option<KindOf>,
    pub(crate) holds_open: bool,
    pub(crate) id: &'f str,
}

pub(crate) fn referenced(kind: KindOf, fields: &Table) -> Vec<Referred<'_>> {
    let references = kind.references();
    let mut referred = Vec::new();
    for key in kind.reference_keys() {
        let reference = references.iter().find(|reference| reference.key == key);
        let ids = match fields.get(key) {
            Some(Value::String(id)) => vec![id.as_str()],
            Some(Value::Array(ids)) => ids.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        referred.extend(ids.into_iter().map(|id| Referred {
            key,
            wanted: reference.and_then(|reference| reference.target),
            holds_open: reference.is_some_and(|reference| reference.holds_open),
            id,
        }));
    }
    referred
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Written {
    pub(crate) key: String,
    parents: Vec<usize>,
    pub(crate) stamp: Stamp,
    pub(crate) machine: DocumentId,
    pub(crate) change: String,
    pub(crate) record: Record,
    pub(crate) body: String,
    pub(crate) links: Links,
}

impl Written {
    /// Where the versions it follows stand among the versions of its document, each
    /// before it.
    pub(crate) fn parents(&self) -> &[usize] {
        &self.parents
    }
}

/// A document of a complete plan, which has a version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Whole {
    pub(crate) id: DocumentId,
    pub(crate) kind: KindOf,
    versions: Vec<Written>,
}

impl Whole {
    /// A parent before its child.
    pub(crate) fn versions(&self) -> &[Written] {
        &self.versions
    }
}

// The field, the target and the reason of what a planned document lacks of a written one.
type Lack = (&'static str, String, &'static str);

pub(crate) fn whole(document: &Document<'_>) -> Result<Whole, Vec<Lack>> {
    let mut lacks: Vec<Lack> = Vec::new();
    let mut earlier: BTreeMap<&str, usize> = BTreeMap::new();
    let mut versions = Vec::new();
    for (at, version) in document.versions.iter().enumerate() {
        let absent = [
            ("record", version.record.is_none()),
            ("written", version.stamp.is_none()),
            ("machine", version.machine.is_none()),
        ];
        for (field, _) in absent.iter().filter(|(_, absent)| *absent) {
            lacks.push((field, version.key.clone(), "is absent"));
        }
        let mut parents = Vec::new();
        for parent in &version.parents {
            if let Some(before) = earlier.get(parent.as_str()) {
                parents.push(*before);
            } else {
                let why = "is no earlier version of the document";
                lacks.push(("parents", parent.clone(), why));
            }
        }
        for parent in &version.outside {
            lacks.push(("parents", parent.clone(), "is no version of the document"));
        }
        earlier.insert(&version.key, at);
        let parts = (version.stamp.clone()).zip(version.machine.clone());
        if let Some(((stamp, machine), record)) = parts.zip(version.record.clone()) {
            versions.push(Written {
                key: version.key.clone(),
                parents,
                stamp,
                machine,
                change: version.change.clone(),
                record,
                body: version.body.clone(),
                links: version.links.clone(),
            });
        }
    }
    if document.versions.is_empty() {
        lacks.push(("versions", "0".to_owned(), "end with no one version"));
    }
    if lacks.is_empty() {
        let (id, kind) = (document.id.clone(), document.kind);
        Ok(Whole { id, kind, versions })
    } else {
        Err(lacks)
    }
}

struct Guard<'d, 'a> {
    documents: &'d [Document<'a>],
    // Each planned document's kind, and whether its last record is ended.
    planned: BTreeMap<&'d str, (KindOf, bool)>,
    questions: &'d Questions,
}

impl Guard<'_, '_> {
    fn ask(&self, document: &Document<'_>, field: &str, target: &str, why: &str) {
        self.questions.ask(Blocked::Unheld {
            kind: document.kind.word().to_owned(),
            document: document.id.to_string(),
            field: field.to_owned(),
            target: target.to_owned(),
            why: why.to_owned(),
        });
    }

    // `Some` for a target that is not planned, or planned as another kind than `wanted`.
    fn misses(&self, target: &str, wanted: Option<KindOf>) -> Option<String> {
        match self.planned.get(target) {
            None => Some("is not planned".to_owned()),
            Some((kind, _)) => {
                let wanted = wanted.filter(|wanted| wanted != kind)?;
                let (kind, wanted) = (kind.word(), wanted.word());
                Some(format!("is planned as {kind}, not as {wanted}"))
            }
        }
    }

    fn version(&self, document: &Document<'_>, version: &Version<'_>) {
        if let Some(machine) = &version.machine
            && let Some(why) = self.misses(machine.as_str(), Some(KindOf::Topic))
        {
            self.ask(document, "machine", machine.as_str(), &why);
        }
        for linked in version.links.0.values().flatten() {
            if let Some(why) = self.misses(linked.as_str(), None) {
                self.ask(document, "links", linked.as_str(), &why);
            }
        }
    }

    // The references of a record are those the new schema lists for its kind.
    fn references(&self, document: &Document<'_>, record: &Record) {
        let fields = record.fields();
        for referred in referenced(document.kind, &fields) {
            let ended = matches!(self.planned.get(referred.id), Some((_, true)));
            let held = referred.holds_open && record.ending.is_none();
            let why = match self.misses(referred.id, referred.wanted) {
                Some(why) => why,
                None if ended && held => "is ended".to_owned(),
                None => continue,
            };
            self.ask(document, referred.key, referred.id, &why);
        }
    }

    fn all(&self) -> Vec<Whole> {
        let mut held = Vec::new();
        for document in self.documents {
            match whole(document) {
                Ok(whole) => held.push(whole),
                Err(lacks) => {
                    for (field, target, why) in lacks {
                        self.ask(document, field, &target, why);
                    }
                }
            }
            for version in &document.versions {
                self.version(document, version);
            }
            if !document.versions.is_empty() && document.head().is_none() {
                let count = document.versions.len().to_string();
                self.ask(document, "versions", &count, "end with no one version");
            }
            if let Some(record) = last(document) {
                self.references(document, record);
            }
        }
        held
    }
}

/// A plan with no question is written as it stands, so every version of it is held here to
/// what a written version has and to the documents the plan holds; `Some` with the
/// documents to write where nothing was asked before and nothing is asked here.
pub(crate) fn unheld<'d>(
    documents: &'d [Document<'_>],
    questions: &'d Questions,
) -> Option<Vec<Whole>> {
    if !questions.asked().is_empty() {
        return None;
    }
    let planned = (documents.iter())
        .map(|document| {
            let ended = last(document).is_some_and(|record| record.ending.is_some());
            (document.id.as_str(), (document.kind, ended))
        })
        .collect();
    let guard = Guard {
        documents,
        planned,
        questions,
    };
    let held = guard.all();
    questions.asked().is_empty().then_some(held)
}
