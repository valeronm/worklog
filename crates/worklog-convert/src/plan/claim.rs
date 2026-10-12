use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{
    Claim, Content, Date, Directory, Ending, KindOf, Reason, Record,
};

use super::authored::{Authoring, Draft, Recorded};
use super::chain::Settled;
use super::id::generated_id;
use super::resolve::Resolved;
use super::stamp::day_of;
use super::walk::{Asked, Role, Step, Walked, generated, list, refuses};
use super::{BODY, Document, Frame, Version};
use crate::key::{DocumentKey, Folded};
use crate::old::Value;
use crate::question::{Authored, Open, Through};

// A topic and the directory it is claimed for, empty for a topic claimed for none.
type Pair = (DocumentId, String);

struct Held {
    topic: Resolved,
    directory: Option<Directory>,
}

struct Claims<'f, 'a, 'd> {
    frame: &'f Frame<'a, 'd>,
    key: DocumentKey,
    machine: DocumentId,
    origin: &'a str,
    of_a_machine: bool,
    // What was refused, so that a value many versions carry is asked about at the first.
    refused: RefCell<BTreeSet<(String, String)>>,
}

// The new store's own `unclaim` ends a claim with this note.
const NOTE: &str = "unclaimed";
const CLAIMS: &str = "claims";
const UNCLAIMED: &str = "unclaimed";

fn ended(record: &Record, on: Date) -> Record {
    Record {
        content: record.content.clone(),
        ending: Some(Ending {
            reason: Reason::Removed,
            on,
            by: None,
            note: NOTE.to_owned(),
        }),
    }
}

fn claimed<'a>(step: &Step<'a>) -> Option<&'a Value> {
    let mut fields = step.source.version.fields.iter();
    fields.find_map(|(key, value)| (key == CLAIMS).then_some(value))
}

impl<'a> Claims<'_, 'a, '_> {
    fn refuse(&self, step: &Step<'_>, field: &str, value: &str, why: &str) {
        let once = (value.to_owned(), why.to_owned());
        if self.refused.borrow_mut().insert(once) {
            self.frame.refused(&step.source, field, value, why);
        }
    }

    fn topic(&self, name: &str) -> Option<Resolved> {
        let found = self.frame.listed(name).ok()?;
        if found.is_none() {
            let (machine_topic, item) = (Folded::new(&self.key.slug), Folded::new(name));
            let _ = self.frame.answer(Open::Claim {
                machine_topic,
                item,
            });
        }
        found
    }

    // The store reads the claims of a machine topic alone, so those of another topic are
    // nothing it ever acted on, asked about once for the topic.
    fn of_no_machine(&self, step: &Step<'_>) {
        let keys = match claimed(step) {
            Some(Value::Map(entries)) => entries.iter().map(|(name, _)| name.as_str()).collect(),
            _ => Vec::new(),
        };
        let unclaimed = list(step.source.version, UNCLAIMED);
        let field = if keys.is_empty() { UNCLAIMED } else { CLAIMS };
        let names: Vec<&str> = (keys.into_iter())
            .chain(unclaimed.iter().map(String::as_str))
            .collect();
        if !names.is_empty() && self.refused.borrow().is_empty() {
            let why = "is claimed by a topic that is no machine's";
            self.refuse(step, field, &names.join(", "), why);
        }
    }

    fn claimed(&self, step: &Step<'_>, pairs: &mut BTreeMap<Pair, Held>) {
        let entries = match claimed(step) {
            Some(Value::Map(entries)) => entries.as_slice(),
            Some(Value::Scalar(text)) if text.is_empty() => &[],
            Some(Value::Scalar(text)) => {
                self.refuse(step, CLAIMS, text, "is not a map of lists");
                &[]
            }
            Some(Value::List(items)) => {
                self.refuse(step, CLAIMS, &items.join(", "), "is not a map of lists");
                &[]
            }
            None => &[],
        };
        for (name, paths) in entries {
            let Value::List(paths) = paths else {
                self.refuse(step, CLAIMS, name, "holds no list of directories");
                continue;
            };
            let Some(topic) = self.topic(name) else {
                continue;
            };
            for path in paths {
                match Directory::parse(path) {
                    Ok(directory) => {
                        let pair = (topic.id.clone(), directory.as_str().to_owned());
                        let topic = topic.clone();
                        let directory = Some(directory);
                        pairs.insert(pair, Held { topic, directory });
                    }
                    Err(refusal) => self.refuse(step, CLAIMS, path, &refusal.to_string()),
                }
            }
        }
    }

    // A tombstone copies the fields of the version before it and ends what they claim.
    fn pairs(&self, step: &Step<'_>) -> BTreeMap<Pair, Held> {
        let mut pairs = BTreeMap::new();
        if step.role == Role::Removed {
            return pairs;
        }
        if !self.of_a_machine {
            self.of_no_machine(step);
            return pairs;
        }
        self.claimed(step, &mut pairs);
        // The store claims nothing for an item its reader leaves out of the list.
        let unclaimed = list(step.source.version, UNCLAIMED);
        let out = self.frame.resolver.left_out(unclaimed);
        let read = (unclaimed.iter().enumerate()).filter(|(place, _)| !out.contains(place));
        for (_, name) in read {
            if let Some(topic) = self.topic(name) {
                let pair = (topic.id.clone(), String::new());
                let directory = None;
                pairs.insert(pair, Held { topic, directory });
            }
        }
        pairs
    }

    fn id(&self, held: &Held, again: usize) -> DocumentId {
        let documents = &self.frame.store.documents;
        let topic = match held.topic.chain {
            Some(chain) => &documents[self.frame.chains.chains[chain].origin].slug,
            None => &held.topic.name,
        };
        let directory = held.directory.as_ref().map_or("", Directory::as_str);
        let machine = self.origin;
        generated_id(&format!("claim\0{machine}\0{topic}\0{directory}\0{again}"))
    }

    fn made(&self, step: &Step<'a>, at: &Version<'a>, held: &Held, again: usize) -> Document<'a> {
        let content = Content::Claim(Claim {
            machine: self.machine.clone(),
            topic: held.topic.id.clone(),
            directory: held.directory.clone(),
        });
        let record = Record {
            content,
            ending: None,
        };
        Document {
            id: self.id(held, again),
            kind: KindOf::Claim,
            versions: vec![generated(
                (step, at),
                "claim",
                Vec::new(),
                Some(record),
                None,
            )],
        }
    }

    // The new store holds the topic of a claim open, so a claim that outlives its topic
    // ends in a version the conversion authors.
    fn outlived(&self, document: &Document<'_>, held: &Held, authoring: &mut Authoring) {
        if !refuses(None, Asked::CLAIM_TOPIC, &held.topic) {
            return;
        }
        let dropped = self.frame.answer(Open::EndedTopic {
            document: self.key.clone(),
            through: Through::Claim,
            topic: held.topic.name.clone(),
        });
        if dropped.is_none() {
            return;
        }
        let Some(first) = document.versions.first() else {
            return;
        };
        let record = first.record.clone();
        let on = move |on: Date| record.as_ref().map(|record| ended(record, on));
        let draft = Draft {
            kind: KindOf::Claim,
            parents: vec![first.key.clone()],
            change: "unclaim",
            record: Recorded::On(Box::new(on)),
            body: BODY.to_owned(),
            answered: None,
        };
        let directory = held.directory.as_ref().map_or("", Directory::as_str);
        let listed = Authored::Unclaimed {
            machine_topic: self.key.slug.clone(),
            topic: held.topic.name.clone(),
            directory: directory.to_owned(),
        };
        authoring.begin(document.id.clone(), draft, listed);
    }
}

struct Made<'a> {
    document: Document<'a>,
    held: Held,
    // The key of the version that ended the claim.
    ended: Option<String>,
}

struct History<'c, 'f, 'a, 'd> {
    claims: &'c Claims<'f, 'a, 'd>,
    made: Vec<Made<'a>>,
    times: BTreeMap<Pair, usize>,
    // What each version holds, and every version behind it.
    live: BTreeMap<String, BTreeMap<Pair, usize>>,
    behind: BTreeMap<String, BTreeSet<String>>,
}

impl<'a> History<'_, '_, 'a, '_> {
    fn blocked(&self, step: &Step<'_>, held: &Held, why: &str) {
        let directory = held.directory.as_ref().map(Directory::as_str);
        let place = directory.map_or_else(|| "anywhere".to_owned(), |at| format!("at {at}"));
        let claim = format!("{} {place}", held.topic.name);
        self.claims.refuse(step, CLAIMS, &claim, why);
    }

    // A claim is one line of versions, so one that two versions with no order between
    // them made is not planned on.
    fn carried(&self, step: &Step<'_>, held: &Held, by: &BTreeSet<usize>) {
        let mut keys: Vec<&str> = (by.iter())
            .map(|made| self.made[*made].document.versions[0].key.as_str())
            .collect();
        keys.sort_unstable();
        if let [one, other, ..] = keys.as_slice() {
            let why = format!(
                "is claimed at {one} and at {other}, neither after the other, and this version \
                 follows both"
            );
            self.blocked(step, held, &why);
        }
    }

    // A claim a version behind this one ended is ended for good, whatever parent still
    // holds it.
    fn ended_behind(&self, made: usize, behind: &BTreeSet<String>) -> bool {
        (self.made[made].ended.as_ref()).is_some_and(|at| behind.contains(at))
    }

    fn gone(&mut self, step: &Step<'a>, at: &Version<'a>, made: usize, behind: &BTreeSet<String>) {
        if let Some(ended) = &self.made[made].ended {
            if !behind.contains(ended) {
                let why =
                    format!("is unclaimed at {ended} and at this version, neither after the other");
                self.blocked(step, &self.made[made].held, &why);
            }
            return;
        }
        let on = at.stamp.as_ref().and_then(day_of);
        let document = &mut self.made[made].document;
        let first = &document.versions[0];
        let record = first.record.as_ref().zip(on);
        let record = record.map(|(record, on)| ended(record, on));
        let parents = vec![first.key.clone()];
        let version = generated((step, at), "unclaim", parents, record, None);
        document.versions.push(version);
        self.made[made].ended = Some(at.key.clone());
    }

    // A claim appears at a version that holds it when none of its parents holds it
    // unended, and is gone at a version that lacks it when one of its parents holds it.
    fn step(&mut self, step: &Step<'a>, at: &Version<'a>) {
        let mut behind: BTreeSet<String> = step.parents.iter().cloned().collect();
        let mut before: BTreeMap<Pair, BTreeSet<usize>> = BTreeMap::new();
        for parent in &step.parents {
            behind.extend(self.behind.get(parent).into_iter().flatten().cloned());
            for (pair, made) in self.live.get(parent).into_iter().flatten() {
                before.entry(pair.clone()).or_default().insert(*made);
            }
        }
        let mut own = BTreeMap::new();
        for (pair, held) in self.claims.pairs(step) {
            let mut by = before.remove(&pair).unwrap_or_default();
            by.retain(|made| !self.ended_behind(*made, &behind));
            if let Some(first) = by.first() {
                self.carried(step, &held, &by);
                own.insert(pair, *first);
                continue;
            }
            let again = self.times.entry(pair.clone()).or_default();
            let document = self.claims.made(step, at, &held, *again);
            *again += 1;
            own.insert(pair, self.made.len());
            self.made.push(Made {
                document,
                held,
                ended: None,
            });
        }
        let gone: BTreeSet<usize> = before.into_values().flatten().collect();
        for made in gone {
            self.gone(step, at, made, &behind);
        }
        self.live.insert(at.key.clone(), own);
        self.behind.insert(at.key.clone(), behind);
    }
}

pub(crate) fn documents<'a>(
    frame: &Frame<'a, '_>,
    own: Settled<'_, 'a>,
    walked: &Walked<'_, 'a>,
    authoring: &mut Authoring,
) -> Vec<Document<'a>> {
    let claims = Claims {
        frame,
        key: own.key.clone(),
        machine: own.chain.id.clone(),
        origin: &frame.store.documents[own.chain.origin].slug,
        of_a_machine: frame.machines.has(own.index),
        refused: RefCell::default(),
    };
    let mut history = History {
        claims: &claims,
        made: Vec::new(),
        times: BTreeMap::new(),
        live: BTreeMap::new(),
        behind: BTreeMap::new(),
    };
    for (step, at) in walked.each() {
        history.step(step, at);
    }
    let last = (walked.head()).and_then(|(_, last)| history.live.get(&last.key));
    for made in last.into_iter().flat_map(BTreeMap::values) {
        let Made { document, held, .. } = &history.made[*made];
        claims.outlived(document, held, authoring);
    }
    (history.made.into_iter())
        .map(|made| made.document)
        .collect()
}
