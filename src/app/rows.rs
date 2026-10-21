use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::Serialize;

use crate::app::heads::{
    ended_for, filed_on, held_under, is_forked, label_of, labeling_topic, row_head,
};
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::{Failure, ReadDeps};
use crate::domain::document::Document;
use crate::domain::id::DocumentId;
use crate::domain::schema::kind::Role;
use crate::domain::schema::{Content, Date, KindOf, Record, Trigger};
use crate::domain::version::Header;

/// Another document as a result refers to it: the id the referring document stores, the
/// label the document referred to has now, or its short id, and the reason it ended with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub document: DocumentId,
    pub label: String,
    // Private so that no reference is made without saying whether its document has ended.
    ended: Option<String>,
}

impl Reference {
    pub(super) fn to(document: &DocumentId, label: String, ended: Option<String>) -> Reference {
        Reference {
            document: document.clone(),
            label,
            ended,
        }
    }

    /// The reason the document ended with, when every head of it that reads has ended; none
    /// where the reference is to a topic and the document does not read as one.
    #[must_use]
    pub fn ended(&self) -> Option<&str> {
        self.ended.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub document: DocumentId,
    pub kind: KindOf,
    pub label: String,
    /// The document's own name, which a followup and a claim have none of.
    pub name: Option<String>,
    /// Empty for a claim.
    pub summary: String,
    /// `topic` or `topics` in stored order; empty for a topic.
    pub topics: Vec<Reference>,
    pub forked: bool,
    pub ended: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shown<'d> {
    pub(super) row: Row,
    pub(super) head: &'d Header,
    pub(super) record: Record,
}

pub(super) fn row<'d>(topics: &Topics, document: &'d Document) -> Option<Shown<'d>> {
    let (head, record) = row_head(document)?;
    let id = document.id().clone();
    let shown = topics.references(filed_on(&record.content));
    let summary = record.content.summary().unwrap_or_default().to_owned();
    let topic_name = labeling_topic(&record.content).and_then(|topic| topics.name(topic));
    let row = Row {
        kind: record.content.kind(),
        label: label_of(&record.content, topic_name, &id),
        name: (record.content.naming()).map(|(name, _)| name.to_string()),
        document: id,
        summary,
        topics: shown,
        forked: is_forked(document),
        ended: ended_for(&record),
    };
    Some(Shown { row, head, record })
}

pub(super) fn members(
    lookup: &Lookup,
    kind: KindOf,
    topics: &BTreeSet<DocumentId>,
    ended: bool,
) -> Result<Vec<Rc<Document>>, Failure> {
    let Some(key) = membership_key(kind) else {
        return Ok(Vec::new());
    };
    let topics: Vec<&DocumentId> = topics.iter().collect();
    lookup.holders(kind, key, &topics, ended)
}

fn memberships(document: &Document, kind: KindOf) -> BTreeSet<DocumentId> {
    let held = membership_key(kind).map(|key| held_under(document, kind, key, false));
    (held.into_iter().flatten())
        .filter_map(|text| DocumentId::parse(text).ok())
        .collect()
}

pub(super) type Filed = BTreeMap<DocumentId, Vec<Rc<Document>>>;

pub(super) fn filed_by_topic(
    lookup: &Lookup,
    kind: KindOf,
    topics: &BTreeSet<DocumentId>,
) -> Result<Filed, Failure> {
    let mut grouped = Filed::new();
    for document in members(lookup, kind, topics, false)? {
        for topic in memberships(&document, kind) {
            grouped.entry(topic).or_default().push(Rc::clone(&document));
        }
    }
    Ok(grouped)
}

pub(super) fn filed(grouped: &Filed, topics: &BTreeSet<DocumentId>) -> Vec<Rc<Document>> {
    let mut found: BTreeMap<&DocumentId, &Rc<Document>> = BTreeMap::new();
    for topic in topics {
        for document in grouped.get(topic).into_iter().flatten() {
            found.insert(document.id(), document);
        }
    }
    found.into_values().cloned().collect()
}

pub(super) fn is_idea(content: &Content) -> bool {
    matches!(content, Content::Fact(fact) if fact.idea)
}

pub(super) struct Counts {
    pub(super) facts: usize,
    pub(super) ideas: usize,
    pub(super) open: usize,
}

pub(super) struct Filings {
    pub(super) facts: Filed,
    pub(super) followups: Filed,
}

impl Filings {
    pub(super) fn of(lookup: &Lookup, topics: &BTreeSet<DocumentId>) -> Result<Filings, Failure> {
        Ok(Filings {
            facts: filed_by_topic(lookup, KindOf::Fact, topics)?,
            followups: filed_by_topic(lookup, KindOf::Followup, topics)?,
        })
    }

    pub(super) fn counts(&self, topics: &BTreeSet<DocumentId>) -> Counts {
        let held = filed(&self.facts, topics);
        let ideas = (held.iter().filter_map(|fact| row_head(fact)))
            .filter(|(_, record)| is_idea(&record.content))
            .count();
        Counts {
            facts: held.len() - ideas,
            ideas,
            open: filed(&self.followups, topics).len(),
        }
    }
}

fn membership_key(kind: KindOf) -> Option<&'static str> {
    kind.key_where(|reference| reference.role == Role::Filed)
}

pub(super) fn reading<'a>(
    deps: &ReadDeps<'a>,
    address: Option<&str>,
) -> Result<(Lookup<'a>, Topics, Option<DocumentId>), Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let topic = address
        .map(|address| lookup.one_topic(address))
        .transpose()?;
    Ok((lookup, topics, topic))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerShown {
    LookAgain { on: Date, why: String },
    Touching { topic: Reference },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FollowupRow {
    pub row: Row,
    pub trigger: Option<TriggerShown>,
    pub due: bool,
    pub entry: Option<Reference>,
    pub about: Option<Reference>,
}

pub(super) fn urgency(trigger: Option<&Trigger>, today: Date) -> (u8, Option<Date>) {
    match trigger {
        Some(Trigger::LookAgain { on, .. }) => (u8::from(*on > today), Some(*on)),
        Some(Trigger::Touching(_)) => (2, None),
        None => (3, None),
    }
}

pub(super) fn followup_rows(
    lookup: &Lookup,
    topics: &Topics,
    shown: Vec<Shown>,
    today: Date,
    counted: &BTreeSet<DocumentId>,
) -> Result<Vec<FollowupRow>, Failure> {
    let mut rows = Vec::new();
    for Shown { row, record, .. } in shown {
        let Content::Followup(followup) = record.content else {
            continue;
        };
        let label =
            |id: &Option<DocumentId>| id.as_ref().map(|id| lookup.reference(id)).transpose();
        let urgency = urgency(followup.trigger.as_ref(), today);
        let (trigger, is_due) = match followup.trigger {
            Some(Trigger::LookAgain { on, why }) => {
                (Some(TriggerShown::LookAgain { on, why }), on <= today)
            }
            Some(Trigger::Touching(topic)) => (
                Some(TriggerShown::Touching {
                    topic: topics.reference(&topic),
                }),
                counted.contains(&topic),
            ),
            None => (None, false),
        };
        let row = FollowupRow {
            row,
            trigger,
            due: is_due,
            entry: label(&followup.entry)?,
            about: label(&followup.about)?,
        };
        rows.push((urgency, row));
    }
    rows.sort_by_cached_key(|(urgency, row)| (*urgency, row.row.document.short().to_owned()));
    Ok(rows.into_iter().map(|(_, row)| row).collect())
}

pub(super) fn by_label<T>(rows: &mut [T], row_of: impl Fn(&T) -> &Row) {
    rows.sort_by(|a, b| {
        let (a, b) = (row_of(a), row_of(b));
        (&a.label, &a.document).cmp(&(&b.label, &b.document))
    });
}

pub(super) fn documents(
    lookup: &Lookup,
    topics: &Topics,
    kind: KindOf,
    topic: Option<&DocumentId>,
    ended: bool,
) -> Result<Vec<Rc<Document>>, Failure> {
    match (topic, kind) {
        (None, _) => lookup.of_kind(kind),
        (Some(id), KindOf::Topic) => {
            let covered = topics.covered(id);
            Ok(lookup
                .of_kind(kind)?
                .into_iter()
                .filter(|document| covered.contains(document.id()))
                .collect())
        }
        (Some(id), _) => members(lookup, kind, &topics.covered(id), ended),
    }
}

pub(super) fn shown<'d>(
    topics: &Topics,
    documents: &'d [Rc<Document>],
    ended: bool,
) -> Vec<Shown<'d>> {
    documents
        .iter()
        .filter_map(|document| row(topics, document))
        .filter(|shown| ended || shown.row.ended.is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::lookup::Stored;
    use crate::app::testing::{
        Counting, ENDED, ENDED_FACT, REMOVED, TOPIC, World, entry, fact, fact_fields, followup,
        head, part_of, referred, to, to_ended, topic, topic_fields, world_with_atlas, written_at,
    };
    use crate::domain::ports::{Store, StoreRead};
    use crate::domain::testing::first;

    fn row_of(world: &World, id: &DocumentId) -> Option<Row> {
        let topics = Topics::load(&world.lookup()).unwrap();
        row(&topics, &world.store.headers(id).unwrap()).map(|shown| shown.row)
    }

    #[test]
    fn a_reference_carries_the_reason_when_every_head_that_reads_has_ended() {
        let (world, lantern, atlas) = world_with_atlas();
        let fuse = fact(&world, &lantern, "fuse", ENDED_FACT);
        let relay = fact(&world, &lantern, "relay-pin", "");
        let unheld = DocumentId::from_bytes([0x42; 16]);
        let references = |world: &World, id: &DocumentId| {
            let lookup = world.lookup();
            let topics = Topics::load(&lookup).unwrap();
            let any = lookup.reference(id).unwrap();
            let as_topic = lookup.topic_reference(id).unwrap();
            assert_eq!(topics.reference(id), as_topic);
            (any, as_topic)
        };
        let both = |reference: Reference| (reference.clone(), reference);
        assert_eq!(references(&world, &atlas), both(to(&atlas, "atlas")));
        assert_eq!(
            references(&world, &relay),
            (to(&relay, "lantern/relay-pin"), to(&relay, relay.short()))
        );
        assert_eq!(
            references(&world, &fuse),
            (
                to_ended(&fuse, "lantern/fuse", "false"),
                to(&fuse, fuse.short())
            )
        );
        assert_eq!(
            references(&world, &unheld),
            both(to(&unheld, unheld.short()))
        );

        let root = head(&world, &atlas);
        let live = written_at(
            &world,
            &root,
            &topic_fields("atlas", ""),
            "2026-10-09T19:00:00+01:00",
            "\n",
            None,
        );
        let fields = topic_fields("atlas", REMOVED);
        written_at(
            &world,
            &root,
            &fields,
            "2026-10-09T19:30:00+01:00",
            "\n",
            None,
        );
        assert_eq!(references(&world, &atlas), both(to(&atlas, "atlas")));
        let fields = topic_fields("atlas", ENDED);
        written_at(
            &world,
            &live,
            &fields,
            "2026-10-09T20:00:00+01:00",
            "\n",
            None,
        );
        assert_eq!(
            references(&world, &atlas),
            both(to_ended(&atlas, "atlas", "retired"))
        );
    }

    #[test]
    fn a_row_shows_what_each_kind_carries() {
        let world = World::new();
        let desk = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", &part_of(&[&desk]));
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&lantern, &atlas]);
        let followup = followup(&world, &[&atlas, &lantern], "check", "");
        let claim = world.put(
            "claim",
            &format!("machine = \"{desk}\"\ntopic = \"{lantern}\"\n"),
            "\n",
        );
        let shown = |id: &DocumentId| row_of(&world, id).unwrap();

        let topic_row = shown(&lantern);
        assert_eq!(
            (
                topic_row.kind,
                topic_row.label.as_str(),
                topic_row.summary.as_str()
            ),
            (KindOf::Topic, "lantern", "s")
        );
        assert!(topic_row.topics.is_empty());
        assert!(!topic_row.forked);
        assert_eq!(topic_row.ended, None);

        let fact_row = shown(&relay);
        assert_eq!(fact_row.kind, KindOf::Fact);
        assert_eq!(fact_row.label, "lantern/relay-pin");
        assert_eq!(fact_row.topics, [to(&lantern, "lantern")]);
        assert_eq!(fact_row.document, relay);

        let entry_row = shown(&driver);
        assert_eq!(entry_row.label, "2026-10-08-lamp-driver");
        assert_eq!(referred(&entry_row.topics), ["lantern", "atlas"]);

        let followup_row = shown(&followup);
        assert_eq!(followup_row.label, followup.short());
        assert_eq!(followup_row.summary, "check");
        assert_eq!(referred(&followup_row.topics), ["atlas", "lantern"]);

        let claim_row = shown(&claim);
        assert_eq!(claim_row.kind, KindOf::Claim);
        assert_eq!(claim_row.label, "lantern anywhere");
        assert_eq!(claim_row.document, claim);
        assert_eq!(claim_row.summary, "");
        assert_eq!(referred(&claim_row.topics), ["lantern"]);

        let names = [&lantern, &relay, &driver, &followup, &claim].map(|id| shown(id).name);
        let own = |name: &str| Some(name.to_owned());
        assert_eq!(
            names,
            [
                own("lantern"),
                own("relay-pin"),
                own("lamp-driver"),
                None,
                None
            ]
        );
    }

    #[test]
    fn a_row_names_a_topic_that_is_absent_by_its_short_id() {
        let world = World::new();
        topic(&world, "desk", "");
        let gone = DocumentId::from_bytes([0x42; 16]);
        let orphan = fact(&world, &gone, "relay-pin", "");
        let row = row_of(&world, &orphan).unwrap();
        assert_eq!(row.topics, [to(&gone, gone.short())]);
        assert_eq!(row.label, orphan.short());
    }

    #[test]
    fn a_row_shows_the_reason_an_ended_document_ended_with() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", ENDED_FACT);
        assert_eq!(
            row_of(&world, &relay).unwrap().ended.as_deref(),
            Some("false")
        );
    }

    #[test]
    fn a_document_with_no_readable_head_has_no_row() {
        let world = World::new();
        topic(&world, "desk", "");
        let broken = world.put("topic", "name = \"atlas\"\n", "\n");
        assert_eq!(row_of(&world, &broken), None);
    }

    #[test]
    fn a_fork_is_one_row_from_its_latest_written_unended_head() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let fields = |summary: &str, rest: &str| fact_fields(&lantern, "relay-pin", summary, rest);
        written_at(
            &world,
            &root,
            &fields("older", ""),
            "2026-10-09T09:00:00+00:00",
            "a\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fields("newer", ""),
            "2026-10-09T11:30:00+02:00",
            "b\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fields("latest but ended", ENDED_FACT),
            "2026-10-09T23:00:00+00:00",
            "c\n",
            None,
        );
        let row = row_of(&world, &relay).unwrap();
        assert!(row.forked);
        assert_eq!(row.summary, "newer");
        assert_eq!(row.ended, None);
    }

    #[test]
    fn a_fork_with_equal_stamps_is_shown_from_the_greater_version_id() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let mut heads = Vec::new();
        for summary in ["first", "second"] {
            let fields = fact_fields(&lantern, "relay-pin", summary, "");
            heads.push((
                written_at(
                    &world,
                    &root,
                    &fields,
                    "2026-10-09T09:00:00+00:00",
                    summary,
                    None,
                )
                .id
                .clone(),
                summary,
            ));
        }
        heads.sort();
        let row = row_of(&world, &relay).unwrap();
        assert_eq!(row.summary, heads[1].1);
    }

    #[test]
    fn a_fork_whose_heads_are_all_ended_is_shown_from_the_latest_of_them() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let ended = |summary: &str, reason: &str| {
            fact_fields(
                &lantern,
                "relay-pin",
                summary,
                &format!("ended = \"{reason}\"\nended_on = 2026-10-09\nnote = \"n\"\n"),
            )
        };
        written_at(
            &world,
            &root,
            &ended("early", "moved"),
            "2026-10-09T09:00:00Z",
            "a\n",
            None,
        );
        written_at(
            &world,
            &root,
            &ended("late", "superseded"),
            "2026-10-09T10:00:00Z",
            "b\n",
            None,
        );
        let row = row_of(&world, &relay).unwrap();
        assert!(row.forked);
        assert_eq!(row.summary, "late");
        assert_eq!(row.ended.as_deref(), Some("superseded"));
    }

    fn belonging(
        world: &World,
        topics: &[&DocumentId],
        kind: KindOf,
        ended: bool,
    ) -> Vec<DocumentId> {
        let lookup = world.lookup();
        let set: BTreeSet<DocumentId> = topics.iter().map(|id| (*id).clone()).collect();
        members(&lookup, kind, &set, ended)
            .unwrap()
            .iter()
            .map(|document| document.id().clone())
            .collect()
    }

    #[test]
    fn members_are_the_documents_filed_under_any_of_the_topics() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let phone = topic(&world, "phone", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let map = fact(&world, &atlas, "map", "");
        fact(&world, &phone, "dial", "");
        let old = fact(&world, &lantern, "old-pin", ENDED_FACT);
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&atlas]);

        assert_eq!(
            belonging(&world, &[&lantern, &atlas], KindOf::Fact, false),
            [relay.clone(), map.clone()]
        );
        let mut everything = vec![relay, old, map];
        everything.sort();
        assert_eq!(
            belonging(&world, &[&lantern, &atlas], KindOf::Fact, true),
            everything
        );
        assert_eq!(belonging(&world, &[&atlas], KindOf::Entry, false), [driver]);
        assert!(belonging(&world, &[&atlas], KindOf::Topic, true).is_empty());
        assert!(belonging(&world, &[], KindOf::Fact, true).is_empty());
    }

    #[test]
    fn members_of_a_set_of_topics_cost_one_store_question() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let phone = topic(&world, "phone", "");
        fact(&world, &lantern, "relay-pin", "");
        fact(&world, &atlas, "map", "");
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));
        let set = BTreeSet::from([lantern, atlas, phone]);
        assert_eq!(members(&lookup, KindOf::Fact, &set, true).unwrap().len(), 2);
        assert_eq!(counting.questions(), 1);
        assert_eq!(counting.holdings.get(), 1);
    }

    #[test]
    fn a_fork_belongs_by_its_unended_heads_alone() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        written_at(
            &world,
            &root,
            &fact_fields(&lantern, "relay-pin", "kept", ENDED_FACT),
            "2026-10-09T09:00:00Z",
            "a\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fact_fields(&atlas, "relay-pin", "moved", ""),
            "2026-10-09T10:00:00Z",
            "b\n",
            None,
        );
        assert_eq!(belonging(&world, &[&lantern], KindOf::Fact, false), []);
        assert_eq!(belonging(&world, &[&lantern], KindOf::Fact, true), []);
        assert_eq!(belonging(&world, &[&atlas], KindOf::Fact, false), [relay]);
    }

    #[test]
    fn an_ended_document_belongs_by_its_row_head() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        written_at(
            &world,
            &root,
            &fact_fields(&lantern, "relay-pin", "early", ENDED_FACT),
            "2026-10-09T09:00:00Z",
            "a\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fact_fields(&atlas, "relay-pin", "late", ENDED_FACT),
            "2026-10-09T10:00:00Z",
            "b\n",
            None,
        );
        assert_eq!(belonging(&world, &[&atlas], KindOf::Fact, true), [relay]);
        assert_eq!(belonging(&world, &[&lantern], KindOf::Fact, true), []);
        assert_eq!(belonging(&world, &[&atlas], KindOf::Fact, false), []);
    }

    #[test]
    fn one_instant_written_in_two_offsets_is_a_tie_decided_by_the_version_id() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let mut heads = Vec::new();
        for (summary, stamp) in [
            ("zulu", "2026-10-09T10:00:00Z"),
            ("shifted", "2026-10-09T11:00:00+01:00"),
        ] {
            let fields = fact_fields(&lantern, "relay-pin", summary, "");
            heads.push((
                written_at(&world, &root, &fields, stamp, summary, None)
                    .id
                    .clone(),
                summary,
            ));
        }
        heads.sort();
        assert_eq!(row_of(&world, &relay).unwrap().summary, heads[1].1);
    }

    #[test]
    fn a_forked_document_is_labeled_from_its_latest_written_head() {
        let world = World::new();
        topic(&world, "desk", "");
        let id = topic(&world, "lantern", "");
        let root = head(&world, &id);
        let fields = |name: &str| format!("name = \"{name}\"\n{TOPIC}");
        written_at(
            &world,
            &root,
            &fields("atlas"),
            "2026-10-09T09:00:00Z",
            "a\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fields("phone"),
            "2026-10-09T11:00:00Z",
            "b\n",
            None,
        );
        let lookup = world.lookup();
        assert_eq!(lookup.label(&id).unwrap(), "phone");
        assert_eq!(row_of(&world, &id).unwrap().label, "phone");
    }

    #[test]
    fn the_index_and_its_rows_cost_the_one_question_that_loads_the_topics() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let relay = fact(&world, &atlas, "relay-pin", "");
        let document = world.store.headers(&relay).unwrap();
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));
        let topics = Topics::load(&lookup).unwrap();
        assert_eq!(topics.covered(&lantern).len(), 2);
        assert_eq!(topics.reach_from(std::slice::from_ref(&atlas)).len(), 2);
        assert_eq!(topics.label(&atlas), "atlas");
        let row = row(&topics, &document).unwrap().row;
        assert_eq!(row.label, "atlas/relay-pin");
        assert_eq!(counting.questions(), 1);
        assert_eq!(counting.kinds.get(), 1);
    }

    #[test]
    fn followup_rows_with_equal_triggers_come_in_short_id_order_whatever_order_they_are_given_in() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        for byte in [0xa1, 0xb1, 0xc1] {
            let fields = format!(
                "created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n\
                 look_again = 2026-10-20\nwhy = \"w\"\n"
            );
            let id = DocumentId::from_bytes([byte; 16]);
            world
                .store
                .put(&first(&id, "followup", &fields, "\n"))
                .unwrap();
        }
        let lookup = world.lookup();
        let topics = Topics::load(&lookup).unwrap();
        let mut documents = lookup.of_kind(KindOf::Followup).unwrap();
        documents.reverse();
        let given = shown(&topics, &documents, false);
        let given_shorts: Vec<&str> = given
            .iter()
            .map(|shown| shown.row.document.short())
            .collect();
        assert_eq!(given_shorts, ["c1c1c1c1", "b1b1b1b1", "a1a1a1a1"]);

        let today = Date::parse("2026-10-09").unwrap();
        let rows = followup_rows(&lookup, &topics, given, today, &BTreeSet::new()).unwrap();
        let shorts: Vec<&str> = rows.iter().map(|row| row.row.document.short()).collect();
        assert_eq!(shorts, ["a1a1a1a1", "b1b1b1b1", "c1c1c1c1"]);
    }
}
