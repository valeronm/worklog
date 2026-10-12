use std::collections::{BTreeMap, BTreeSet};

use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{Date, Ending, KindOf, Reason, Record};
use worklog_next::domain::version::Links;

use super::chain::{Settled, key_of};
use super::followup::reason_of;
use super::name::{Carrier, Holder, Scope};
use super::resolve::{Resolved, Unsettled};
use super::stamp::{Written, written};
use super::{BODY, Frame, Origin, Source, Stored, Version, entry, fact, topic};
use crate::decisions::{Answer, Unasked};
use crate::key::{DocumentKey, Key, Placement};
use crate::old::{self, FollowupState, Kind, Operation, Recheck, Value};
use crate::question::{Blocked, Open, reasons};

// A key of the new schema under which a document of a kind names a topic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    kind: KindOf,
    key: &'static str,
}

const fn asked(kind: KindOf, key: &'static str) -> Asked {
    Asked { kind, key }
}

impl Asked {
    pub(crate) const FACT_TOPIC: Asked = asked(KindOf::Fact, "topic");
    pub(crate) const PART_OF: Asked = asked(KindOf::Topic, "part_of");
    pub(crate) const USES: Asked = asked(KindOf::Topic, "uses");
    pub(crate) const ENTRY_TOPICS: Asked = asked(KindOf::Entry, "topics");
    pub(crate) const FOLLOWUP_TOPICS: Asked = asked(KindOf::Followup, "topics");
    pub(crate) const TOUCHING: Asked = asked(KindOf::Followup, "touching");
    pub(crate) const CLAIM_TOPIC: Asked = asked(KindOf::Claim, "topic");

    pub(crate) fn edge(placement: Placement) -> Option<Asked> {
        match placement {
            Placement::PartOf => Some(Asked::PART_OF),
            Placement::Uses => Some(Asked::USES),
            Placement::Drop => None,
        }
    }
    #[cfg(test)]
    pub(crate) const ALL: [Asked; 7] = [
        Asked::FACT_TOPIC,
        Asked::PART_OF,
        Asked::USES,
        Asked::ENTRY_TOPICS,
        Asked::FOLLOWUP_TOPICS,
        Asked::TOUCHING,
        Asked::CLAIM_TOPIC,
    ];

    #[cfg(test)]
    pub(crate) fn is_a_reference(self) -> bool {
        let mut references = self.kind.references().iter();
        references.any(|reference| reference.key == self.key)
    }
}

// The new store holds a topic open for a document that names it under such a key.
pub(crate) fn holds_open(asked: Asked) -> bool {
    let mut references = asked.kind.references().iter();
    references.any(|reference| reference.holds_open && reference.key == asked.key)
}

/// Whether the last record of a document keeps a topic it names under `asked` from being
/// ended: the document is not ended, a followup being ended where it is closed, and the new
/// store holds the topic open under the key. `None` is a document the conversion makes of
/// no document of the store, which is not ended.
pub(crate) fn holds(own: Option<Settled<'_, '_>>, asked: Asked) -> bool {
    let open = |head: &Step<'_>| scalar(head.source.version, "state") == Some("open");
    let unended = own.is_none_or(|own| {
        !own.ended && (own.chain.kind != Kind::Followup || own.heads().all(open))
    });
    unended && holds_open(asked)
}

pub(crate) fn refuses(own: Option<Settled<'_, '_>>, asked: Asked, target: &Resolved) -> bool {
    target.ended && holds(own, asked)
}

pub(crate) fn push_once<T: PartialEq>(held: &mut Vec<T>, one: T) {
    if !held.contains(&one) {
        held.push(one);
    }
}

// The store's readers take an empty scalar as a field that is not set.
pub(crate) fn scalar<'a>(version: &'a old::Version, key: &str) -> Option<&'a str> {
    (version.fields.iter()).find_map(|(held, value)| match value {
        Value::Scalar(text) if held == key && !text.is_empty() => Some(text.as_str()),
        _ => None,
    })
}

// The store's readers take a field that holds no list as an empty one.
pub(crate) fn list<'a>(version: &'a old::Version, key: &str) -> &'a [String] {
    let found = (version.fields.iter()).find_map(|(held, value)| match value {
        Value::List(items) if held == key => Some(items.as_slice()),
        _ => None,
    });
    found.unwrap_or_default()
}

pub(crate) trait Follows {
    fn key(&self) -> &str;
    fn parents(&self) -> impl Iterator<Item = &str>;
}

impl Follows for Version<'_> {
    fn key(&self) -> &str {
        &self.key
    }

    fn parents(&self) -> impl Iterator<Item = &str> {
        self.parents.iter().map(String::as_str)
    }
}

impl Follows for Step<'_> {
    fn key(&self) -> &str {
        &self.source.version.id
    }

    fn parents(&self) -> impl Iterator<Item = &str> {
        self.parents.iter().map(String::as_str)
    }
}

impl Follows for worklog_next::domain::version::Version {
    fn key(&self) -> &str {
        self.id.as_str()
    }

    fn parents(&self) -> impl Iterator<Item = &str> {
        (self.envelope.parents.iter()).map(worklog_next::domain::id::VersionId::as_str)
    }
}

pub(crate) fn heads<T: Follows>(of: &[T]) -> impl Iterator<Item = &T> {
    let named: BTreeSet<&str> = of.iter().flat_map(Follows::parents).collect();
    (of.iter()).filter(move |one| !named.contains(one.key()))
}

/// The head of a document that ends with one version.
pub(crate) fn head<T: Follows>(of: &[T]) -> Option<&T> {
    let mut heads = heads(of);
    heads.next().filter(|_| heads.next().is_none())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Plain,
    Renamed,
    Removed,
    CutShort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step<'a> {
    pub(crate) source: Source<'a>,
    pub(crate) role: Role,
    pub(crate) place: usize,
    pub(crate) head: bool,
    pub(crate) parents: Vec<String>,
    pub(crate) outside: Vec<String>,
}

impl<'a> Step<'a> {
    pub(crate) fn key(&self) -> &'a str {
        &self.source.version.id
    }

    pub(crate) fn slug(&self) -> &'a str {
        &self.source.document.slug
    }

    // A rename is stamped by its tombstone, the first of its two versions to be written.
    fn stamped(&self) -> Stored<'a> {
        let Source {
            document,
            version,
            stone,
        } = self.source;
        stone.unwrap_or(Stored { document, version })
    }
}

/// The versions the steps of a chain became, each with the step it was made from.
#[derive(Clone, Debug)]
pub(crate) struct Walked<'c, 'a> {
    steps: &'c [Step<'a>],
    pub(crate) versions: Vec<Version<'a>>,
}

impl<'c, 'a> Walked<'c, 'a> {
    pub(crate) fn each(&self) -> impl Iterator<Item = (&'c Step<'a>, &Version<'a>)> {
        self.steps.iter().zip(&self.versions)
    }

    pub(crate) fn head(&self) -> Option<(&'c Step<'a>, &Version<'a>)> {
        let mut heads = self.each().filter(|(step, _)| step.head);
        heads.next().filter(|_| heads.next().is_none())
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Dates {
    pub(crate) on: Option<Date>,
    pub(crate) created: Option<Date>,
}

#[derive(Clone, Debug)]
pub(crate) struct Mapped<S> {
    // `None` when a question stands before a field the kind requires.
    pub(crate) record: Option<Record>,
    pub(crate) body: String,
    pub(crate) state: S,
}

pub(crate) trait Mapper {
    type State: Clone;

    // Called for every version that is no tombstone and for a tombstone that follows no one
    // version, whose record is not kept. The frame ends a version only at a tombstone, so an
    // ending that a later version still has is the mapping's to set again.
    fn map(
        &mut self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        dates: Dates,
        parents: &[&Mapped<Self::State>],
    ) -> Mapped<Self::State>;
}

pub(crate) const OPERATIONS: [Operation; 11] = [
    Operation::New,
    Operation::Save,
    Operation::Done,
    Operation::Drop,
    Operation::Recheck,
    Operation::Verify,
    Operation::Tombstone,
    Operation::Rename,
    Operation::Resolve,
    Operation::Claim,
    Operation::Unclaim,
];

// An importer wrote the first version of a document under this word, which no command
// writes.
const MIGRATE: &str = "migrate";

// The new store labels an ending with its reason and a new trigger with `trigger`.
fn label(operation: Operation) -> &'static str {
    match operation {
        Operation::New
        | Operation::Save
        | Operation::Resolve
        | Operation::Verify
        | Operation::Claim
        | Operation::Unclaim
        | Operation::Rename
        | Operation::Tombstone => operation.as_str(),
        Operation::Done => Reason::Done.word(),
        Operation::Drop => Reason::Dropped.word(),
        Operation::Recheck => "trigger",
    }
}

pub(crate) fn change_of(operation: &str) -> Option<&'static str> {
    if operation == MIGRATE {
        return Some(MIGRATE);
    }
    let mut written = OPERATIONS.into_iter();
    let known = written.find(|known| known.as_str() == operation)?;
    Some(label(known))
}

// A tombstone copies the fields of the version it follows, so its own `state` says how a
// followup had been closed where no record of that version is planned.
fn closed(version: &old::Version) -> Option<String> {
    let state = FollowupState::parse(scalar(version, "state")?)?;
    Some(reason_of(state)?.word().to_owned())
}

/// A version the conversion makes beside `at`, a version made from `step` of another
/// document, whose stamp and machine it takes.
pub(crate) fn generated<'a>(
    (step, at): (&Step<'a>, &Version<'a>),
    change: &str,
    parents: Vec<String>,
    record: Option<Record>,
    answered: Option<Key>,
) -> Version<'a> {
    let cause = Stored {
        document: step.source.document,
        version: step.source.version,
    };
    Version {
        key: at.key.clone(),
        parents,
        outside: Vec::new(),
        stamp: at.stamp.clone(),
        machine: at.machine.clone(),
        change: change.to_owned(),
        record,
        body: BODY.to_owned(),
        links: Links::default(),
        origin: Origin::Generated { cause, answered },
    }
}

/// A recheck as the store's grammar reads it, with the day of the dated form read.
pub(crate) enum Rechecked {
    On { on: Date, why: String },
    Touching(String),
}

// A document two machines made before they synced has several versions that follow none,
// and it was created when the earliest of them was written.
fn created(steps: &[Step<'_>]) -> Option<Date> {
    fn read<'a>(step: &Step<'a>) -> Option<(Written, &'a str)> {
        Some((written(&step.stamped().version.written)?, step.key()))
    }
    let first = |step: &&Step<'_>| step.parents.is_empty() && step.outside.is_empty();
    let firsts = steps.iter().filter(first).filter_map(read);
    let earliest = firsts.min_by(|(one, one_key), (other, other_key)| {
        let instants = one.stamp.instant().cmp(&other.stamp.instant());
        instants.then_with(|| one_key.cmp(other_key))
    });
    let (written, _) = earliest.or_else(|| read(steps.first()?))?;
    Some(written.day)
}

fn sources<'a>(
    store: &'a old::Store,
    documents: &[usize],
    renames: &[(String, String)],
) -> Vec<(usize, Source<'a>)> {
    let moved: BTreeMap<&str, &str> = (renames.iter())
        .map(|(stone, moved)| (stone.as_str(), moved.as_str()))
        .collect();
    let mut stones: BTreeMap<&str, Stored<'a>> = BTreeMap::new();
    let mut versions = Vec::new();
    for (place, document) in documents.iter().enumerate() {
        let document = &store.documents[*document];
        for version in &document.versions {
            match moved.get(version.id.as_str()) {
                Some(to) => drop(stones.insert(to, Stored { document, version })),
                None => versions.push((place, document, version)),
            }
        }
    }
    let source = |(place, document, version): (usize, _, &'a old::Version)| {
        let source = Source {
            document,
            version,
            stone: stones.get(version.id.as_str()).copied(),
        };
        (place, source)
    };
    versions.into_iter().map(source).collect()
}

/// The versions the documents of one rename chain become, a parent before its child.
///
/// A rename's version follows what its tombstone followed, and what the moved version
/// names beside the tombstone; a version that names the tombstone follows the rename's
/// version, which the store may hold under a later slug than its own.
pub(crate) fn steps<'a>(
    store: &'a old::Store,
    documents: &[usize],
    renames: &[(String, String)],
) -> Vec<Step<'a>> {
    let became: BTreeMap<&str, &str> = (renames.iter())
        .map(|(stone, moved)| (stone.as_str(), moved.as_str()))
        .collect();
    let named = |source: &Source<'a>| -> Vec<String> {
        let left = source.stone.map(|stone| stone.version);
        let beside = (source.version.parents.iter())
            .filter(|parent| left.is_none_or(|left| left.id != **parent));
        let named = left
            .into_iter()
            .flat_map(|left| &left.parents)
            .chain(beside);
        named
            .map(|parent| {
                became
                    .get(parent.as_str())
                    .map_or(parent.as_str(), |moved| *moved)
            })
            .map(str::to_owned)
            .collect()
    };
    let mut waiting: Vec<(usize, Source<'a>, Vec<String>)> = (sources(store, documents, renames)
        .into_iter())
    .map(|(place, source)| (place, source, named(&source)))
    .collect();
    let held: BTreeSet<&'a str> = (waiting.iter())
        .map(|(_, source, _)| source.version.id.as_str())
        .collect();
    let mut earlier: BTreeSet<&str> = BTreeSet::new();
    let mut steps = Vec::new();
    while !waiting.is_empty() {
        let ready = waiting.iter().position(|(_, _, named)| {
            let follows = |parent: &String| !held.contains(parent.as_str());
            named
                .iter()
                .all(|parent| follows(parent) || earlier.contains(parent.as_str()))
        });
        let (place, source, named) = waiting.remove(ready.unwrap_or_default());
        let version = source.version;
        let role = match (source.stone, version.tombstone, &version.superseded_by) {
            (Some(_), _, _) => Role::Renamed,
            (None, true, None) => Role::Removed,
            (None, true, Some(_)) => Role::CutShort,
            (None, false, _) => Role::Plain,
        };
        let (parents, outside) = named
            .into_iter()
            .partition(|parent| earlier.contains(parent.as_str()));
        earlier.insert(&version.id);
        steps.push(Step {
            source,
            role,
            place,
            head: false,
            parents,
            outside,
        });
    }
    let last: BTreeSet<&'a str> = heads(&steps).map(Step::key).collect();
    for step in &mut steps {
        step.head = last.contains(step.key());
    }
    steps
}

impl<'a> Frame<'a, '_> {
    pub(crate) fn refused(&self, source: &Source<'_>, field: &str, value: &str, why: &str) {
        self.questions.ask(Blocked::Value {
            document: key_of(source.document),
            version: source.version.id.clone(),
            field: field.to_owned(),
            value: value.to_owned(),
            why: why.to_owned(),
        });
    }

    pub(crate) fn missing(&self, source: &Source<'_>, field: &str) {
        self.refused(source, field, "", "is missing, and the store requires it");
    }

    // A recheck the new schema cannot take is refused at the version that holds it.
    pub(crate) fn recheck(&self, source: &Source<'_>, text: &str) -> Option<Rechecked> {
        let why = match Recheck::parse(text) {
            Ok(Recheck::Touching(topic)) => return Some(Rechecked::Touching(topic)),
            Ok(Recheck::On { date, why }) => match Date::parse(&date) {
                Ok(on) => return Some(Rechecked::On { on, why }),
                Err(_) => "does not open with a day",
            },
            Err(_) => "is neither a day with a reason nor `touching` with a topic",
        };
        self.refused(source, "recheck", text, why);
        None
    }

    // The store checks a date by its shape alone.
    pub(crate) fn day(&self, source: &Source<'_>, field: &str) -> Option<Date> {
        let Some(day) = scalar(source.version, field) else {
            self.missing(source, field);
            return None;
        };
        let read = Date::parse(day).ok();
        if read.is_none() {
            self.refused(source, field, day, "is not a day");
        }
        read
    }

    fn foreign(&self, document: &old::Document, version: &old::Version) {
        let keys = document.foreign_keys(version);
        if !keys.is_empty() {
            self.questions.ask(Blocked::ForeignKeys {
                document: key_of(document),
                version: version.id.clone(),
                keys,
            });
        }
    }

    fn written(&self, step: &Step<'_>) -> Option<Written> {
        let Stored { document, version } = step.stamped();
        let read = written(&version.written);
        if read.is_none() {
            self.questions.ask(Blocked::WriteTime {
                document: key_of(document),
                version: version.id.clone(),
                text: version.written.clone(),
            });
        }
        read
    }

    // One answer of the file is the summary of every version of the document that has none.
    pub(crate) fn summary(&self, key: &DocumentKey, version: &old::Version) -> Option<String> {
        if let Some(own) = scalar(version, "summary").filter(|own| !own.trim().is_empty()) {
            return Some(own.to_owned());
        }
        let document = key.clone();
        let answered = self.text(Open::Summary { document });
        answered.map(|(_, answer)| answer.to_owned())
    }

    fn ended_by(&self, at: Option<Answer<'_>>) -> Result<Option<DocumentId>, ()> {
        let Some((at, by)) = at.and_then(|at| Some((at, at.document()?))) else {
            return Ok(None);
        };
        match self.resolver.document(by.kind, &by.slug) {
            Ok(Some(found)) => return Ok(Some(found.id.clone())),
            Ok(None) => (self.questions).ask(at.refused(format!("{by} names no document"))),
            Err(Unsettled) => {}
        }
        Err(())
    }

    // The old writer copies the fields of the version before into a tombstone, so the
    // tombstone's own `idea` says which reasons the document ends for.
    fn ending(
        &self,
        key: &DocumentKey,
        step: &Step<'_>,
        on: Option<Date>,
        before: Option<&Ending>,
    ) -> Option<Ending> {
        let version = step.source.version;
        let idea = key.kind == Kind::Fact && scalar(version, "idea") == Some("true");
        let note = version.body.trim();
        let already = before
            .map(|ending| format!("{} on {}", ending.reason.word(), ending.on))
            .or_else(|| closed(version));
        let answered = self.text(Open::Ended {
            document: key.clone(),
            idea,
            note: note.to_owned(),
            already,
        });
        let (reason, by) = match answered {
            Some((answer, _)) => {
                let by = self.decisions.unasked(Unasked::EndedBy(key)).pop();
                match answer.one_of(&reasons(key.kind, idea)) {
                    Ok(reason) => (Some(Reason::parse(reason)), self.ended_by(by)),
                    Err(open) => {
                        self.questions.ask(open);
                        (None, Ok(None))
                    }
                }
            }
            None => (None, Ok(None)),
        };
        Some(Ending {
            reason: reason?,
            on: on?,
            by: by.ok()?,
            note: note.to_owned(),
        })
    }

    // A tombstone carries the fields and body of the one version it follows; one that
    // follows none or several has no such version, and one whose one parent the store
    // lacks is waiting for it.
    fn before<S: Clone>(&self, step: &Step<'_>, parents: &[&Mapped<S>]) -> Option<Mapped<S>> {
        match (parents, step.outside.as_slice()) {
            ([before], []) => return Some((*before).clone()),
            ([], [_]) => return None,
            _ => {}
        }
        let follows = step.parents.iter().chain(&step.outside);
        self.questions.ask(Blocked::Tombstone {
            document: key_of(step.source.document),
            version: step.source.version.id.clone(),
            follows: follows.cloned().collect(),
        });
        None
    }

    pub(crate) fn walk<'c, M: Mapper>(
        &self,
        own: Settled<'c, 'a>,
        mapper: &mut M,
    ) -> Walked<'c, 'a> {
        let key = own.key;
        let mut done: BTreeMap<&str, Mapped<M::State>> = BTreeMap::new();
        let mut versions = Vec::new();
        let steps = own.chain.steps.as_slice();
        let created = created(steps);
        for step in steps {
            self.foreign(step.source.document, step.source.version);
            if let Some(stone) = step.source.stone {
                self.foreign(stone.document, stone.version);
            }
            let written = self.written(step);
            let on = written.as_ref().map(|written| written.day);
            let dates = Dates { on, created };
            let machine = self.machine(&step.stamped().version.machine);
            let parents: Vec<&Mapped<M::State>> = (step.parents.iter())
                .filter_map(|parent| done.get(parent.as_str()))
                .collect();
            let follows = match step.role {
                Role::Plain | Role::Renamed => None,
                Role::Removed | Role::CutShort => Some(self.before(step, &parents)),
            };
            let own = match (step.role, follows) {
                (Role::Removed, Some(Some(before))) => {
                    let ended = before
                        .record
                        .as_ref()
                        .and_then(|record| record.ending.as_ref());
                    let ending = self.ending(key, step, on, ended);
                    let record = before.record.zip(ending).map(|(record, ending)| Record {
                        content: record.content,
                        ending: Some(ending),
                    });
                    Mapped { record, ..before }
                }
                (_, Some(Some(before))) => before,
                (_, Some(None)) => Mapped {
                    record: None,
                    ..mapper.map(self, step, dates, &parents)
                },
                (_, None) => mapper.map(self, step, dates, &parents),
            };
            let ended = own
                .record
                .as_ref()
                .and_then(|record| record.ending.as_ref());
            let stored = step.source.version.operation.as_str();
            let known = change_of(stored);
            if known.is_none() {
                let why = "is no operation a command of the store writes";
                self.refused(&step.source, "operation", stored, why);
            }
            let label = match (step.role, ended) {
                (Role::Removed, Some(ending)) => ending.reason.word(),
                (Role::Renamed | Role::CutShort, _) => Operation::Rename.as_str(),
                (Role::Plain | Role::Removed, _) => known.unwrap_or(stored),
            };
            let label = label.to_owned();
            versions.push(Version {
                key: step.key().to_owned(),
                parents: step.parents.clone(),
                outside: step.outside.clone(),
                stamp: written.map(|written| written.stamp),
                machine,
                change: label,
                record: own.record.clone(),
                body: own.body.clone(),
                links: Links::default(),
                origin: Origin::Converted(step.source),
            });
            done.insert(step.key(), own);
        }
        Walked { steps, versions }
    }

    pub(crate) fn forked(&self, own: Settled<'_, '_>) {
        let mut heads: Vec<String> = own.heads().map(|head| head.key().to_owned()).collect();
        heads.sort();
        self.questions.ask(Blocked::Forked {
            document: own.key.clone(),
            heads,
        });
    }

    pub(crate) fn carriers(&self) -> Vec<Carrier> {
        let mut carriers = Vec::new();
        for own in self.chains.each_settled() {
            let found = match own.chain.kind {
                Kind::Topic => Some(topic::carrier(self, own)),
                Kind::Fact => Some(fact::carrier(self, own)),
                Kind::Entry => Some(entry::carrier(self, own)),
                Kind::Followup => None,
            };
            carriers.extend(found);
        }
        carriers.extend(self.resolver.created().map(|table| Carrier {
            holder: Holder::Created(table.to_owned()),
            scope: Some(Scope::Topics),
            segments: vec![table.to_owned()],
            head: 0,
        }));
        carriers
    }
}
