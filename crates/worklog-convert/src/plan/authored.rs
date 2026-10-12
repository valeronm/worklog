use std::collections::{BTreeMap, BTreeSet};

use worklog::domain::id::DocumentId;
use worklog::domain::schema::{Content, Date, Fact, KindOf, Name, Record, Topic};
use worklog::domain::version::Links;

use super::chain::{Chain, Settled};
use super::fact::{self, Refiled};
use super::id::created_topic_id;
use super::name::{Naming, naming};
use super::resolve::{Named, Resolved};
use super::stamp::{NamedBy, Written, written};
use super::walk::{Asked, Step, Walked, list, push_once, refuses};
use super::{AUTHORED, BODY, Document, Frame, Origin, Version};
use crate::decisions::{Answer, Unasked};
use crate::key::{ConversionKey, Edges, Folded, Key, Placement, TopicKey};
use crate::question::{Authored, Open, Through, kind_of};

pub(crate) enum Recorded {
    // `None` where a question stands before a field the kind requires.
    Whole(Option<Record>),
    // A record that holds the day the conversion is written on.
    On(Box<dyn Fn(Date) -> Option<Record>>),
}

pub(crate) struct Draft {
    pub(crate) kind: KindOf,
    pub(crate) parents: Vec<String>,
    pub(crate) change: &'static str,
    pub(crate) record: Recorded,
    pub(crate) body: String,
    pub(crate) answered: Option<Key>,
}

const RENAME: &str = "rename";

/// The conversion authors at most one version a document, after every version the store
/// holds of it.
#[derive(Default)]
pub(crate) struct Authoring {
    drafts: BTreeMap<DocumentId, Draft>,
    pub(crate) listed: BTreeSet<Authored>,
}

// `holder` is the topic of the store the table is of, and `None` for one the file creates.
fn listed(
    frame: &Frame<'_, '_>,
    (table, holder): (&str, Option<Settled<'_, '_>>),
    mut adds: impl FnMut(&Answer<'_>, &Resolved) -> bool,
) -> (Edges<Resolved>, bool) {
    let mut ends = Edges::default();
    let mut whole = true;
    for placement in [Placement::PartOf, Placement::Uses] {
        for at in frame.decisions.unasked(Unasked::Listed(table, placement)) {
            let Some(item) = at.key().item() else {
                continue;
            };
            let found = match frame.topic(item.as_str()) {
                Ok(Some(found)) => found,
                Ok(None) => {
                    (frame.questions).ask(at.refused(format!("{item} names no topic")));
                    whole = false;
                    continue;
                }
                Err(_) => {
                    whole = false;
                    continue;
                }
            };
            if !adds(&at, &found) {
                continue;
            }
            let asked = Asked::edge(placement);
            if asked.is_some_and(|asked| refuses(holder, asked, &found)) {
                frame.ended_answer(&at, item.as_str());
                whole = false;
                continue;
            }
            if let Some(ends) = ends.of(placement) {
                ends.push(found);
            }
        }
    }
    (ends, whole)
}

fn without(ids: &[DocumentId], dropped: &[DocumentId], added: Vec<DocumentId>) -> Vec<DocumentId> {
    let kept = ids.iter().filter(|id| !dropped.contains(id));
    let mut ids: Vec<DocumentId> = kept.cloned().collect();
    for id in added {
        push_once(&mut ids, id);
    }
    ids
}

fn renamed(content: &Content, to: &Name) -> Content {
    let mut content = content.clone();
    let held = match &mut content {
        Content::Topic(topic) => Some((&mut topic.name, &mut topic.former_names)),
        Content::Fact(fact) => Some((&mut fact.name, &mut fact.former_names)),
        Content::Entry(entry) => Some((&mut entry.name, &mut entry.former_names)),
        Content::Followup(_) | Content::Claim(_) => None,
    };
    if let Some((name, former)) = held {
        let before = Naming {
            name: name.clone(),
            former: std::mem::take(former),
        };
        *former =
            naming(Some(to.clone()), &[Some(&before)]).map_or(before.former, |now| now.former);
        *name = to.clone();
    }
    content
}

// An item of the file that names a topic the `includes` of some version held places that
// item, and adds no edge.
fn included(frame: &Frame<'_, '_>, steps: &[Step<'_>]) -> BTreeSet<DocumentId> {
    let items = steps
        .iter()
        .flat_map(|step| list(step.source.version, "includes"));
    let named = items.filter_map(|item| match frame.included(item) {
        Named::One(found) => Some(found.id),
        Named::None | Named::Several(_) | Named::Unsettled => None,
    });
    named.collect()
}

// Only the last version of a topic that is not ended is held to edges to topics that are
// not ended.
fn dropped(
    frame: &Frame<'_, '_>,
    own: Settled<'_, '_>,
    items: &[String],
    last: &Topic,
) -> Edges<Resolved> {
    let mut dropped = Edges::default();
    let mut seen = BTreeSet::new();
    for item in items {
        let Named::One(found) = frame.included(item) else {
            continue;
        };
        let lists = [
            (Placement::PartOf, Asked::PART_OF, &last.part_of),
            (Placement::Uses, Asked::USES, &last.uses),
        ];
        let placed = (lists.into_iter())
            .find(|(_, _, ids)| ids.contains(&found.id))
            .map(|(placement, asked, _)| (placement, asked));
        let Some((placement, asked)) = placed else {
            continue;
        };
        let judged = refuses(Some(own), asked, &found);
        if !judged || !seen.insert(found.id.clone()) {
            continue;
        }
        let answered = frame.answer(Open::EndedTopic {
            document: own.key.clone(),
            through: Through::Includes,
            topic: found.name.clone(),
        });
        if let (Some(_), Some(ends)) = (answered, dropped.of(placement)) {
            ends.push(found);
        }
    }
    dropped
}

fn names(ends: &Edges<Resolved>) -> Edges<String> {
    ends.map(|end| end.name.clone())
}

impl Authoring {
    // A change to the last record of a document of the store goes into the one version
    // the conversion authors of it, which is labeled as a rename where it renames, whatever
    // else it carries.
    fn amend(
        &mut self,
        chain: &Chain<'_>,
        head: &Version<'_>,
        change: &'static str,
        to: impl Fn(&Record) -> Record,
    ) {
        let draft = self
            .drafts
            .entry(chain.id.clone())
            .or_insert_with(|| Draft {
                kind: kind_of(chain.kind),
                parents: vec![head.key.clone()],
                change,
                record: Recorded::Whole(head.record.clone()),
                body: head.body.clone(),
                answered: None,
            });
        if let Recorded::Whole(Some(record)) = &mut draft.record {
            *record = to(record);
        }
        if change == RENAME {
            draft.change = RENAME;
        }
    }

    /// The one version the conversion authors of a document the store holds no version
    /// of, or of a claim.
    pub(crate) fn begin(&mut self, document: DocumentId, draft: Draft, listed: Authored) {
        self.drafts.entry(document).or_insert(draft);
        self.listed.insert(listed);
    }

    // One version after the last carries every edge the file adds to a topic and lacks
    // every edge it drops.
    pub(crate) fn edges(
        &mut self,
        frame: &Frame<'_, '_>,
        own: Settled<'_, '_>,
        walked: &Walked<'_, '_>,
    ) {
        let (chain, key, ended) = (own.chain, own.key, own.ended);
        let placed = included(frame, &chain.steps);
        let (added, _) = listed(frame, (&key.slug, Some(own)), |at, found| {
            let adds = !placed.contains(&found.id);
            let asked = Folded::new(&found.name);
            if let Some(item) = at.key().item().filter(|item| !adds && **item != asked) {
                let why = format!(
                    "{item} is {}, an item of includes that is placed under that name",
                    found.name
                );
                frame.questions.ask(at.refused(why));
            }
            if adds && ended {
                let why = format!("{} is ended, and an ended topic takes no edge", key.slug);
                frame.questions.ask(at.refused(why));
            }
            adds && !ended
        });
        let last = walked.head();
        let before = last.and_then(|(_, last)| match &last.record.as_ref()?.content {
            Content::Topic(topic) => Some((last, topic)),
            _ => None,
        });
        let gone = match (last, before) {
            (Some((step, _)), Some((_, topic))) if !ended => {
                dropped(frame, own, list(step.source.version, "includes"), topic)
            }
            _ => Edges::default(),
        };
        if added.is_empty() && gone.is_empty() {
            return;
        }
        let Some((last, _)) = before else {
            if last.is_none() {
                frame.forked(own);
            }
            return;
        };
        let (out, new) = (
            gone.map(|end| end.id.clone()),
            added.map(|end| end.id.clone()),
        );
        self.amend(chain, last, "save", |record| match &record.content {
            Content::Topic(topic) => Record {
                content: Content::Topic(Topic {
                    part_of: without(&topic.part_of, &out.part_of, new.part_of.clone()),
                    uses: without(&topic.uses, &out.uses, new.uses.clone()),
                    ..topic.clone()
                }),
                ending: record.ending.clone(),
            },
            _ => record.clone(),
        });
        let topic = &key.slug;
        if !added.is_empty() {
            let Edges { part_of, uses } = names(&added);
            self.listed.insert(Authored::Edges {
                topic: topic.clone(),
                part_of,
                uses,
            });
        }
        if !gone.is_empty() {
            let Edges { part_of, uses } = names(&gone);
            self.listed.insert(Authored::Dropped {
                topic: topic.clone(),
                part_of,
                uses,
            });
        }
    }

    // The new store's own move changes the topic and leaves `confirmed` alone.
    pub(crate) fn refile(
        &mut self,
        frame: &Frame<'_, '_>,
        own: Settled<'_, '_>,
        walked: &Walked<'_, '_>,
    ) -> Option<DocumentId> {
        let chain = own.chain;
        let head = walked.head().map(|(_, head)| head);
        let fact = head.and_then(|head| match &head.record.as_ref()?.content {
            Content::Fact(fact) => Some((head, fact)),
            _ => None,
        });
        let (from, to) = match fact::refiled(frame, own) {
            Refiled::Stays => return fact.map(|(_, fact)| fact.topic.clone()),
            Refiled::Asked => return None,
            Refiled::To { from, to } => (from, to),
        };
        let Some((head, _)) = fact else {
            if head.is_none() {
                frame.forked(own);
            }
            return None;
        };
        self.amend(chain, head, "move", |record| match &record.content {
            Content::Fact(fact) => Record {
                content: Content::Fact(Fact {
                    topic: to.id.clone(),
                    ..fact.clone()
                }),
                ending: record.ending.clone(),
            },
            _ => record.clone(),
        });
        self.listed.insert(Authored::Refiled {
            fact: own.key.slug.clone(),
            from: from.name,
            to: to.name,
        });
        Some(to.id)
    }

    pub(crate) fn rename(
        &mut self,
        frame: &Frame<'_, '_>,
        own: Settled<'_, '_>,
        walked: &Walked<'_, '_>,
    ) {
        let Some(to) = frame.names.renamed(own.index) else {
            return;
        };
        let chain = own.chain;
        let Some((_, last)) = walked.head() else {
            frame.forked(own);
            return;
        };
        self.amend(chain, last, RENAME, |record| Record {
            content: renamed(&record.content, to),
            ending: record.ending.clone(),
        });
        let from = frame.names.name(own.index, own.place());
        self.listed.insert(Authored::Renamed {
            document: own.key.to_string(),
            from: from
                .map(|from| from.as_str().to_owned())
                .unwrap_or_default(),
            to: to.as_str().to_owned(),
        });
    }

    pub(crate) fn created(&mut self, frame: &Frame<'_, '_>) {
        let tables: Vec<String> = frame.resolver.created().map(str::to_owned).collect();
        for table in tables {
            let name = frame.names.created(&table);
            let topic = Folded::new(&table);
            let summary = frame.text(Open::TopicSummary { topic });
            let summary = summary.map(|(_, summary)| summary.to_owned());
            let (ends, mut whole) = listed(frame, (&table, None), |_, _| true);
            for at in (frame.decisions).unasked(Unasked::Listed(&table, Placement::Drop)) {
                let why = format!("{table} is created by the file and drops nothing");
                frame.questions.ask(at.refused(why));
                whole = false;
            }
            let ids = ends.map(|end| end.id.clone());
            let parts = name.zip(summary.clone()).filter(|_| whole);
            let record = move |created: Date| {
                let (name, summary) = parts.clone()?;
                let content = Content::Topic(Topic {
                    name,
                    former_names: Vec::new(),
                    created,
                    summary,
                    part_of: ids.part_of.clone(),
                    uses: ids.uses.clone(),
                });
                Some(Record {
                    content,
                    ending: None,
                })
            };
            let Edges { part_of, uses } = names(&ends);
            let draft = Draft {
                kind: KindOf::Topic,
                parents: Vec::new(),
                change: "new",
                record: Recorded::On(Box::new(record)),
                body: BODY.to_owned(),
                answered: Some(Key::topic(&table, TopicKey::Summary)),
            };
            let listed = Authored::Topic {
                name: table.clone(),
                summary: summary.unwrap_or_default(),
                part_of,
                uses,
            };
            self.begin(created_topic_id(&table), draft, listed);
        }
    }

    // The file's stamp is held to the store whenever the file gives it, and asked for,
    // with the count of the versions that carry it, once the conversion authors one.
    fn stamp<'d>(&self, frame: &Frame<'_, 'd>) -> (Option<Written>, Option<DocumentId>) {
        let key = |key| frame.decisions.unasked(Unasked::Conversion(key)).pop();
        let read = |at: Answer<'d>| Some((at, at.text()?));
        let written_at = key(ConversionKey::Written).and_then(read);
        let machine = key(ConversionKey::Machine).and_then(read);
        let versions = self.drafts.len();
        if versions > 0 && (written_at.is_none() || machine.is_none()) {
            frame.questions.ask(Open::Conversion { versions });
        }
        let read = written_at.and_then(|(at, text)| {
            let read = written(text);
            let latest = frame.latest();
            let follows = (read.as_ref().zip(latest.as_ref()))
                .is_none_or(|(read, latest)| read.stamp.instant() > latest.instant());
            if read.is_none() {
                let why = format!("{text} is no write time the store keeps");
                frame.questions.ask(at.refused(why));
            } else if let (false, Some(latest)) = (follows, latest) {
                let why = format!(
                    "{text} is not later than {latest}, the latest write time the store holds"
                );
                frame.questions.ask(at.refused(why));
            }
            read
        });
        let machine =
            machine.and_then(|(_, machine)| frame.machine_of(machine, NamedBy::Authored(versions)));
        (read, machine)
    }

    // Every version the conversion authors carries the one stamp the file gives.
    pub(crate) fn write<'a>(self, frame: &Frame<'a, '_>, documents: &mut Vec<Document<'a>>) {
        let (read, machine) = self.stamp(frame);
        let mut held: BTreeMap<DocumentId, usize> = (documents.iter().enumerate())
            .map(|(at, document)| (document.id.clone(), at))
            .collect();
        for (id, draft) in self.drafts {
            let record = match draft.record {
                Recorded::Whole(record) => record,
                Recorded::On(record) => read.as_ref().and_then(|read| record(read.day)),
            };
            let version = Version {
                key: AUTHORED.to_owned(),
                parents: draft.parents,
                outside: Vec::new(),
                stamp: read.as_ref().map(|read| read.stamp.clone()),
                machine: machine.clone(),
                change: draft.change.to_owned(),
                record,
                body: draft.body,
                links: Links::default(),
                origin: Origin::Authored {
                    answered: draft.answered,
                },
            };
            let at = *held.entry(id.clone()).or_insert_with(|| {
                documents.push(Document {
                    id,
                    kind: draft.kind,
                    versions: Vec::new(),
                });
                documents.len() - 1
            });
            documents[at].versions.push(version);
        }
    }
}
