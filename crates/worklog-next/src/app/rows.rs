use std::collections::BTreeSet;
use std::rc::Rc;

use crate::app::heads::{ended_for, held_under, is_forked, label_or_short, row_head};
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::id::DocumentId;
use crate::domain::schema::address::displayed;
use crate::domain::schema::{Content, Date, KindOf, Record, Trigger};
use crate::domain::version::Version;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: DocumentId,
    pub kind: KindOf,
    pub label: String,
    /// Empty for a claim.
    pub summary: String,
    /// The labels of `topic` or `topics` in stored order; empty for a topic.
    pub topics: Vec<String>,
    pub forked: bool,
    pub ended: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shown<'d> {
    pub(super) row: Row,
    pub(super) head: &'d Version,
    pub(super) record: Record,
}

pub(super) fn row<'d>(topics: &Topics, document: &'d Document) -> Option<Shown<'d>> {
    let (head, record) = row_head(document)?;
    let id = document.id().clone();
    let labels = |ids: &[DocumentId]| ids.iter().map(|id| topics.label(id)).collect();
    let (summary, shown) = match &record.content {
        Content::Topic(topic) => (topic.summary.clone(), Vec::new()),
        Content::Fact(fact) => (
            fact.summary.clone(),
            labels(std::slice::from_ref(&fact.topic)),
        ),
        Content::Entry(entry) => (entry.summary.clone(), labels(&entry.topics)),
        Content::Followup(followup) => (followup.summary.clone(), labels(&followup.topics)),
        Content::Claim(claim) => (String::new(), labels(std::slice::from_ref(&claim.topic))),
    };
    let topic_name = match &record.content {
        Content::Fact(fact) => topics.name(&fact.topic),
        _ => None,
    };
    let row = Row {
        kind: record.content.kind(),
        label: label_or_short(displayed(&record.content, topic_name), &id),
        id,
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

pub(super) fn memberships(document: &Document, kind: KindOf) -> BTreeSet<DocumentId> {
    let held = membership_key(kind).map(|key| held_under(document, kind, key, false));
    (held.into_iter().flatten())
        .filter_map(|text| DocumentId::parse(text).ok())
        .collect()
}

fn membership_key(kind: KindOf) -> Option<&'static str> {
    kind.key_where(|reference| reference.membership)
}

pub(super) fn reading<'a>(
    deps: &Deps<'a>,
    address: Option<&str>,
) -> Result<(Lookup<'a>, Topics, Option<DocumentId>), Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let topic = address
        .map(|address| lookup.one_topic(address))
        .transpose()?;
    Ok((lookup, topics, topic))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriggerShown {
    LookAgain { on: Date, why: String },
    Touching(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FollowupRow {
    pub row: Row,
    pub trigger: Option<TriggerShown>,
    pub due: bool,
    pub entry: Option<String>,
    pub about: Option<String>,
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
        let label = |id: &Option<DocumentId>| id.as_ref().map(|id| lookup.label(id)).transpose();
        let (trigger, is_due) = match followup.trigger {
            Some(Trigger::LookAgain { on, why }) => {
                (Some(TriggerShown::LookAgain { on, why }), on <= today)
            }
            Some(Trigger::Touching(topic)) => (
                Some(TriggerShown::Touching(topics.label(&topic))),
                counted.contains(&topic),
            ),
            None => (None, false),
        };
        rows.push(FollowupRow {
            row,
            trigger,
            due: is_due,
            entry: label(&followup.entry)?,
            about: label(&followup.about)?,
        });
    }
    rows.sort_by_cached_key(|row| {
        let (rank, on) = match (&row.trigger, row.due) {
            (Some(TriggerShown::LookAgain { on, .. }), true) => (0, Some(*on)),
            (Some(TriggerShown::LookAgain { on, .. }), false) => (1, Some(*on)),
            (Some(TriggerShown::Touching(_)), _) => (2, None),
            (None, _) => (3, None),
        };
        (rank, on, row.row.id.short().to_owned())
    });
    Ok(rows)
}

pub(super) fn by_label<T>(rows: &mut [T], row_of: impl Fn(&T) -> &Row) {
    rows.sort_by(|a, b| {
        let (a, b) = (row_of(a), row_of(b));
        (&a.label, &a.id).cmp(&(&b.label, &b.id))
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
        Counting, ENDED_FACT, TOPIC, World, entry, fact, fact_fields, followup, head, part_of,
        topic, written_at,
    };
    use crate::domain::ports::Store;
    use crate::domain::testing::first;

    fn row_of(world: &World, id: &DocumentId) -> Option<Row> {
        let topics = Topics::load(&world.lookup()).unwrap();
        row(&topics, &world.store.document(id).unwrap()).map(|shown| shown.row)
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
        assert_eq!(fact_row.topics, ["lantern"]);
        assert_eq!(fact_row.id, relay);

        let entry_row = shown(&driver);
        assert_eq!(entry_row.label, "2026-10-08-lamp-driver");
        assert_eq!(entry_row.topics, ["lantern", "atlas"]);

        let followup_row = shown(&followup);
        assert_eq!(followup_row.label, followup.short());
        assert_eq!(followup_row.summary, "check");
        assert_eq!(followup_row.topics, ["atlas", "lantern"]);

        let claim_row = shown(&claim);
        assert_eq!(claim_row.kind, KindOf::Claim);
        assert_eq!(claim_row.label, claim.short());
        assert_eq!(claim_row.summary, "");
        assert_eq!(claim_row.topics, ["lantern"]);
    }

    #[test]
    fn a_row_names_a_topic_that_is_absent_by_its_short_id() {
        let world = World::new();
        topic(&world, "desk", "");
        let gone = DocumentId::from_bytes([0x42; 16]);
        let orphan = fact(&world, &gone, "relay-pin", "");
        let row = row_of(&world, &orphan).unwrap();
        assert_eq!(row.topics, [gone.short()]);
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
                .id,
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
                written_at(&world, &root, &fields, stamp, summary, None).id,
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
        let document = world.store.document(&relay).unwrap();
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));
        let topics = Topics::load(&lookup).unwrap();
        assert_eq!(topics.covered(&lantern).len(), 2);
        assert_eq!(topics.reach(std::slice::from_ref(&atlas)).len(), 2);
        assert_eq!(topics.label(&atlas), "atlas");
        let row = row(&topics, &document).unwrap().row;
        assert_eq!(row.label, "atlas/relay-pin");
        assert_eq!(counting.questions(), 1);
        assert_eq!(counting.kinds.get(), 1);
        assert_eq!(counting.documents.get(), 0);
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
        let given_shorts: Vec<&str> = given.iter().map(|shown| shown.row.id.short()).collect();
        assert_eq!(given_shorts, ["c1c1c1c1", "b1b1b1b1", "a1a1a1a1"]);

        let today = Date::parse("2026-10-09").unwrap();
        let rows = followup_rows(&lookup, &topics, given, today, &BTreeSet::new()).unwrap();
        let shorts: Vec<&str> = rows.iter().map(|row| row.row.id.short()).collect();
        assert_eq!(shorts, ["a1a1a1a1", "b1b1b1b1", "c1c1c1c1"]);
    }
}
