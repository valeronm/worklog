use std::collections::BTreeSet;

use worklog::app::load::{Loaded, load};
use worklog::app::operation::Operation;
use worklog::domain::recheck::Recheck;
use worklog::domain::version::Version as Stored;
use worklog::fs::FsStore;
use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{
    Content, Date, Ending, Fact, Followup, KindOf, Name, Reason, Record, Topic, Trigger,
};
use worklog_next::domain::version::Kind as NewKind;
use worklog_next::domain::version::{Links, Stamp};

use super::chain::Chains;
use super::id::created_topic_id;
use super::name::refusal;
use super::resolve::{Named, Resolver};
use super::stamp::written;
use super::walk::{Asked, Dates, Mapped, Mapper, OPERATIONS, Step, change_of, holds_open};
use super::{
    AUTHORED, Document, Frame, Origin, Outcome, Planned, Questions, Source, Stored as Held,
    Version, check, document_id, generated_id, outside_unasked,
};
use crate::decisions::Decisions;
use crate::fixture::{
    At, CONVERSION, DAY_1, DAY_2, DAY_3, DAY_4, DAY_5, DAY_6, ENDED_PHONE, FILE, LAMP_DRIVER,
    OldStore, PIN, PORT, at, claims, composed, desk, following, in_another_order, list, read, rich,
    scalar, with_desk, with_topics,
};
use crate::key::{DocumentField, DocumentKey, Folded, Key, Placement, TopicKey};
use crate::old::{self, Kind, Store};
use crate::question::{Authored, Overridden};
use crate::question::{Blocked, Open, Question, Through, Unjoined};

fn answers(text: &str) -> Decisions {
    Decisions::parse(text).unwrap()
}

const CONVERTED: &str = "2026-10-20T09:30:00+01:00";

fn plan<'a>(store: &'a Store, decisions: &Decisions) -> Planned<'a> {
    let outcome = super::plan(store, decisions);
    let complete = matches!(outcome, Outcome::Complete(_));
    let planned = outcome.into_planned();
    assert_eq!(
        complete,
        planned.questions.is_empty() && planned.unused.is_empty()
    );
    let mut sorted = planned.authored.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(planned.authored, sorted);
    let authors = |version: &Version| matches!(version.origin, Origin::Authored { .. });
    let authored = planned
        .documents
        .iter()
        .flat_map(|document| &document.versions)
        .any(authors);
    assert_eq!(authored, !planned.authored.is_empty());
    let blocked = planned
        .questions
        .iter()
        .any(|question| matches!(question, Question::Blocked(_)));
    for document in &planned.documents {
        let kind = NewKind::parse(document.kind.word()).unwrap();
        let mut earlier = BTreeSet::new();
        for version in &document.versions {
            if let Some(record) = &version.record {
                let read = Record::read(&kind, &record.fields());
                assert_eq!(read.as_ref(), Ok(record), "{}", version.key);
            }
            for parent in &version.parents {
                assert!(earlier.contains(parent), "{parent} of {}", version.key);
            }
            assert!(blocked || version.outside.is_empty(), "{}", version.key);
            let summary = version.record.as_ref().map(Record::fields);
            let summary = summary
                .as_ref()
                .and_then(|fields| fields.get("summary")?.as_str());
            let summary = summary.unwrap_or_default();
            assert!(!summary.contains('\n'), "{}", version.key);
            for (text, id) in &version.links.0 {
                let link = format!("[[{text}]]");
                let held = version.body.contains(&link) || summary.contains(&link);
                assert!(held, "{link} of {}", version.key);
                let unsettled = id.is_some() && old::shaped(text) == Some(Kind::Entry);
                assert!(!unsettled || !planned.questions.is_empty(), "{link}");
            }
            assert!(earlier.insert(&version.key), "{} twice", version.key);
            assert_eq!(authors(version), version.key == AUTHORED);
            let converted = matches!(version.origin, Origin::Converted(_));
            assert!(converted || version.outside.is_empty(), "{}", version.key);
        }
        let last = document.versions.iter().position(authors);
        assert!(last.is_none_or(|last| last + 1 == document.versions.len()));
    }
    planned
}

fn by_id<'p, 'a>(planned: &'p Planned<'a>, id: &DocumentId) -> &'p Document<'a> {
    (planned.documents.iter())
        .find(|document| document.id == *id)
        .unwrap_or_else(|| panic!("no document {id} was planned"))
}

fn of_kind<'p, 'a>(planned: &'p Planned<'a>, kind: KindOf) -> Vec<&'p Document<'a>> {
    let of_kind = |document: &&Document| document.kind == kind;
    planned.documents.iter().filter(of_kind).collect()
}

fn held<'a>(store: &'a Store, kind: Kind, slug: &str, version: &Stored) -> Held<'a> {
    let document = (store.documents.iter())
        .find(|document| document.kind == kind && document.slug == slug)
        .unwrap();
    let id = version.id.to_string();
    let version = (document.versions.iter())
        .find(|version| version.id == id)
        .unwrap();
    Held { document, version }
}

fn stamped(version: &Version) -> String {
    version.stamp.as_ref().unwrap().to_string()
}

fn authored_by_desk(version: &Version, parents: &[&Stored], change: &str) {
    assert_eq!(version.key, AUTHORED);
    let parents: Vec<String> = parents.iter().map(|parent| parent.id.to_string()).collect();
    assert_eq!(version.parents, parents);
    assert_eq!(stamped(version), CONVERTED);
    assert_eq!(version.machine, Some(topic_id("desk")));
    assert_eq!(version.change, change);
}

fn topic_id(origin: &str) -> DocumentId {
    document_id(Kind::Topic, origin)
}

fn of<'p, 'a>(planned: &'p Planned<'a>, kind: Kind, origin: &str) -> &'p Document<'a> {
    by_id(planned, &document_id(kind, origin))
}

fn topic(version: &Version) -> Topic {
    match version.record.as_ref().map(|record| &record.content) {
        Some(Content::Topic(topic)) => topic.clone(),
        other => panic!("not a topic: {other:?}"),
    }
}

fn fact(version: &Version) -> Fact {
    match version.record.as_ref().map(|record| &record.content) {
        Some(Content::Fact(fact)) => fact.clone(),
        other => panic!("not a fact: {other:?}"),
    }
}

fn followup(version: &Version) -> Followup {
    match version.record.as_ref().map(|record| &record.content) {
        Some(Content::Followup(followup)) => followup.clone(),
        other => panic!("not a followup: {other:?}"),
    }
}

fn ending(version: &Version) -> Option<Ending> {
    version.record.as_ref().unwrap().ending.clone()
}

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}

fn names(names: &[&str]) -> Vec<Name> {
    names
        .iter()
        .map(|name| Name::parse(name).unwrap())
        .collect()
}

fn opens(planned: &Planned) -> Vec<Open> {
    let open = |question: &Question| match question {
        Question::Open(open) => Some(open.clone()),
        Question::Blocked(_) => None,
    };
    planned.questions.iter().filter_map(open).collect()
}

fn blocks(planned: &Planned) -> Vec<Blocked> {
    let blocked = |question: &Question| match question {
        Question::Blocked(blocked) => Some(blocked.clone()),
        Question::Open(_) => None,
    };
    planned.questions.iter().filter_map(blocked).collect()
}

fn key(kind: Kind, slug: &str) -> DocumentKey {
    DocumentKey::new(kind, slug)
}

fn first_bytes(text: &str) -> DocumentId {
    let hash = blake3::hash(text.as_bytes());
    DocumentId::from_bytes(hash.as_bytes()[..16].try_into().unwrap())
}

#[test]
fn an_id_is_the_hash_of_what_the_document_is() {
    assert_eq!(
        document_id(Kind::Fact, "lantern/relay-pin"),
        first_bytes("worklog-convert\0document\0fact\0lantern/relay-pin")
    );
    assert_eq!(
        generated_id("topic\0lantern-firmware"),
        first_bytes("worklog-convert\0generated\0topic\0lantern-firmware")
    );
    assert_ne!(
        document_id(Kind::Topic, "2026-09-04-port"),
        document_id(Kind::Followup, "2026-09-04-port")
    );
}

#[test]
fn an_origin_slug_gives_one_id_in_any_run_and_any_order_of_files() {
    let old = with_topics();
    old.fact(PIN, "Pin 5", &[], desk(DAY_2));
    let (one, other) = (read(&old), read(&in_another_order(&old)));
    let none = Decisions::default();
    let (first, again, reversed) = (plan(&one, &none), plan(&one, &none), plan(&other, &none));

    assert_eq!(first, again);
    assert_eq!(first, reversed);
    assert_eq!(first.questions, []);
    let ids: Vec<&DocumentId> = first
        .documents
        .iter()
        .map(|document| &document.id)
        .collect();
    let mut topics = [topic_id("atlas"), topic_id("desk"), topic_id("lantern")];
    topics.sort();
    let mut expected: Vec<&DocumentId> = topics.iter().collect();
    let relay = document_id(Kind::Fact, "lantern/relay-pin");
    expected.push(&relay);
    assert_eq!(ids, expected);
    let kinds: Vec<KindOf> = first
        .documents
        .iter()
        .map(|document| document.kind)
        .collect();
    assert_eq!(
        kinds,
        [KindOf::Topic, KindOf::Topic, KindOf::Topic, KindOf::Fact]
    );
}

#[test]
fn a_rename_is_one_version_of_the_document_its_origin_made() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (left, moved) = old.renamed(&first, "lamp", desk(DAY_3));
    let saved = old.saved(&moved, "A brighter lamp", "\nBrighter.\n", desk(DAY_4));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    assert_eq!(planned.documents.len(), 2);
    let lantern = of(&planned, Kind::Topic, "lantern");
    let keys: Vec<&str> = lantern
        .versions
        .iter()
        .map(|version| version.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [first.id.as_str(), moved.id.as_str(), saved.id.as_str()]
    );
    let renamed = &lantern.versions[1];
    assert_eq!(renamed.parents, [first.id.to_string()]);
    assert_eq!(renamed.stamp.as_ref().unwrap().to_string(), DAY_3);
    assert_eq!(renamed.change, "rename");
    assert_eq!(renamed.body, "\n");
    assert_eq!(
        renamed.origin.source().unwrap().version.id,
        moved.id.to_string()
    );
    assert_eq!(
        renamed.origin.source().unwrap().stone.unwrap().version.id,
        left.id.to_string()
    );
    assert_eq!(lantern.versions[2].parents, [moved.id.to_string()]);
    assert_eq!(topic(&lantern.versions[0]).name, names(&["lantern"])[0]);
    assert_eq!(topic(&lantern.versions[0]).former_names, []);
    assert_eq!(topic(renamed).name, names(&["lamp"])[0]);
    assert_eq!(topic(renamed).former_names, names(&["lantern"]));
    assert_eq!(
        topic(&lantern.versions[2]).former_names,
        names(&["lantern"])
    );
    assert_eq!(topic(&lantern.versions[2]).created, date("2026-09-04"));
}

#[test]
fn renames_in_a_row_join_into_one_document() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (_, lamp) = old.renamed(&first, "lamp", desk(DAY_2));
    let (_, beacon) = old.renamed(&lamp, "beacon", desk(DAY_3));
    let (_, back) = old.renamed(&beacon, "lantern-two", desk(DAY_4));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let lantern = of(&planned, Kind::Topic, "lantern");
    assert_eq!(lantern.versions.len(), 4);
    assert_eq!(lantern.versions[3].key, back.id.to_string());
    assert_eq!(lantern.versions[3].parents, [beacon.id.to_string()]);
    assert_eq!(
        topic(&lantern.versions[2]).former_names,
        names(&["lantern", "lamp"])
    );
    assert_eq!(
        topic(&lantern.versions[3]).former_names,
        names(&["lantern", "lamp", "beacon"])
    );
    assert_eq!(topic(&lantern.versions[3]).name, names(&["lantern-two"])[0]);
}

#[test]
fn a_rename_cut_short_blocks_and_its_document_is_still_planned() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let left = old.rename_cut_short(&first, "lamp", desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::RenameCutShort {
            document: key(Kind::Topic, "lantern"),
            target: "lamp".to_owned(),
        })]
    );
    let lantern = of(&planned, Kind::Topic, "lantern");
    assert_eq!(lantern.versions.len(), 2);
    assert_eq!(lantern.versions[1].key, left.id.to_string());
    assert_eq!(lantern.versions[1].change, "rename");
    assert_eq!(topic(&lantern.versions[1]), topic(&lantern.versions[0]));
    assert_eq!(ending(&lantern.versions[1]), None);
}

#[test]
fn a_rename_whose_moved_version_another_rename_wrote_is_cut_short() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let atlas = old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    old.rename_cut_short(&atlas, "lamp", desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::RenameCutShort {
            document: key(Kind::Topic, "atlas"),
            target: "lamp".to_owned(),
        })]
    );
    assert_eq!(of(&planned, Kind::Topic, "lantern").versions.len(), 2);
    assert_eq!(of(&planned, Kind::Topic, "atlas").versions.len(), 2);
}

#[test]
fn a_moved_version_no_rename_of_its_old_slug_wrote_is_not_joined() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (_, moved) = old.renamed(&lantern, "lamp", desk(DAY_2));
    let astray = moved.to_text().replace("slug: lamp\n", "slug: beacon\n");
    old.planted("topic/beacon", &astray);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::RenameUnjoined {
            document: DocumentKey::new(Kind::Topic, "beacon"),
            others: vec!["lantern".to_owned()],
            case: Unjoined::NoTombstone,
        })]
    );
    assert_eq!(of(&planned, Kind::Topic, "lantern").versions.len(), 2);
    let beacon = &of(&planned, Kind::Topic, "beacon").versions;
    assert_eq!(beacon.len(), 1);
    assert_eq!(beacon[0].parents, [""; 0]);
    let named: Vec<String> = moved
        .block
        .parents
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(beacon[0].outside, named);
}

#[test]
fn renames_that_meet_or_part_join_nothing() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let atlas = old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    old.renamed(&atlas, "lamp", desk(DAY_3));
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.renamed(&phone, "handset", desk(DAY_2));
    old.renamed(&phone, "mobile", desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let unjoined = |slug: &str, other: &str, case| Blocked::RenameUnjoined {
        document: DocumentKey::new(Kind::Topic, slug),
        others: vec![other.to_owned()],
        case,
    };
    let mut expected = vec![
        unjoined("lamp", "lantern", Unjoined::Met),
        unjoined("lamp", "atlas", Unjoined::Met),
        unjoined("phone", "handset", Unjoined::Parted),
        unjoined("phone", "mobile", Unjoined::Parted),
    ];
    expected.sort();
    let unreadable: Vec<Blocked> = (blocks(&planned).into_iter())
        .filter(|blocked| matches!(blocked, Blocked::RenameUnjoined { .. }))
        .collect();
    assert_eq!(unreadable, expected);
    assert_eq!(blocks(&planned).len(), 6);
    assert_eq!(planned.documents.len(), 7);
    let counts = [
        ("lantern", 2),
        ("atlas", 2),
        ("lamp", 2),
        ("phone", 3),
        ("handset", 1),
        ("mobile", 1),
    ];
    for (origin, count) in counts {
        let versions = &of(&planned, Kind::Topic, origin).versions;
        assert_eq!(versions.len(), count, "{origin}");
    }
}

#[test]
fn a_fact_renamed_into_another_topic_changes_its_topic_and_keeps_its_names() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let first = old.fact("lantern/relay-pin", "Pin 5", &["lantern"], desk(DAY_1));
    let (_, moved) = old.renamed(&first, "atlas/relay-pin", desk(DAY_2));
    old.renamed(&moved, "atlas/relay", desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let relay = of(&planned, Kind::Fact, "lantern/relay-pin");
    assert_eq!(relay.kind, KindOf::Fact);
    let facts: Vec<Fact> = relay.versions.iter().map(fact).collect();
    assert_eq!(facts[0].topic, topic_id("lantern"));
    assert_eq!(facts[1].topic, topic_id("atlas"));
    assert_eq!(facts[1].name, names(&["relay-pin"])[0]);
    assert_eq!(facts[1].former_names, []);
    assert_eq!(facts[2].topic, topic_id("atlas"));
    assert_eq!(facts[2].name, names(&["relay"])[0]);
    assert_eq!(facts[2].former_names, names(&["relay-pin"]));
    assert_eq!(facts[2].confirmed, date("2026-09-04"));
}

#[test]
fn a_name_resolves_through_renames_and_in_any_case() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    old.fact("lantern/dimmer", "Dims by half", &[], desk(DAY_3));
    old.topic("atlas", "A book of maps", &["LANTERN", "Lamp"], desk(DAY_3));
    let store = read(&old);
    let decisions = answers("[topics.atlas]\nuses = [\"lamp\"]\n");
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let pin = fact(&of(&planned, Kind::Fact, "lantern/relay-pin").versions[0]);
    let dimmer = fact(&of(&planned, Kind::Fact, "lantern/dimmer").versions[0]);
    assert_eq!(pin.topic, topic_id("lantern"));
    assert_eq!(dimmer.topic, topic_id("lantern"));
    let atlas = topic(&of(&planned, Kind::Topic, "atlas").versions[0]);
    assert_eq!(atlas.uses, [topic_id("lantern")]);
    assert_eq!(atlas.part_of, []);
}

#[test]
fn a_name_of_nothing_is_asked_about_once_in_each_place() {
    let old = with_desk();
    let atlas = old.topic("atlas", "A book of maps", &["ghost"], desk(DAY_1));
    old.saved(&atlas, "A bigger book of maps", "\n", desk(DAY_2));
    old.topic("lantern", "A lamp", &["Ghost"], desk(DAY_1));
    old.fact("ghost/relay-pin", "Pin 5", &[], desk(DAY_1));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let missing = |topic: &str| Open::Include {
        topic: Folded::new(topic),
        item: Folded::new("ghost"),
        missing: true,
        ended: false,
        unread: false,
    };
    assert_eq!(
        opens(&planned),
        [
            missing("atlas"),
            missing("lantern"),
            Open::FactTopic {
                fact: key(Kind::Fact, "ghost/relay-pin"),
                topic: "ghost".to_owned(),
            },
        ]
    );
    assert_eq!(blocks(&planned), []);
    assert_eq!(of(&planned, Kind::Topic, "atlas").versions[1].record, None);
    assert_eq!(
        of(&planned, Kind::Fact, "ghost/relay-pin").versions[0].record,
        None
    );
}

#[test]
fn a_fact_under_no_topic_takes_the_one_the_file_gives() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.fact("ghost/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.fact("ghost/dimmer", "Dims by half", &[], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(
        "[documents.\"fact/ghost/relay-pin\"]\ntopics = [\"Lantern\"]\n\
         [documents.\"fact/ghost/dimmer\"]\ntopics = [\"nowhere\"]\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(
        planned.questions,
        [Question::Open(Open::Answer {
            key: Key::document(&key(Kind::Fact, "ghost/dimmer"), DocumentField::Topics),
            why: "nowhere names no topic".to_owned(),
        })]
    );
    let pin = fact(&of(&planned, Kind::Fact, "ghost/relay-pin").versions[0]);
    assert_eq!(pin.topic, topic_id("lantern"));
}

#[test]
fn a_write_time_is_read_with_or_without_a_fraction_in_its_own_offset() {
    let read = |text: &str| written(text).map(|written| written.stamp.to_string());
    assert_eq!(read(DAY_1).as_deref(), Some(DAY_1));
    assert_eq!(read(DAY_2).as_deref(), Some(DAY_2));
    assert_eq!(read(DAY_3).as_deref(), Some(DAY_3));
    assert_eq!(read(DAY_5).as_deref(), Some(DAY_5));
    assert_eq!(
        read("2026-09-04t10:15:07z").as_deref(),
        Some("2026-09-04T10:15:07Z")
    );
    assert_eq!(
        read("2026-09-04t10:15:07.25-07:00").as_deref(),
        Some("2026-09-04T10:15:07.25-07:00")
    );
    assert_eq!(written(DAY_3).unwrap().day, date("2026-09-06"));
    for refused in [
        "yesterday",
        "2026-09-04",
        "2026-09-04T10:15:07",
        "2026-09-04 10:15:07+01:00",
        "2026-09-04T10:15+01:00",
        "2026-09-04T10:15:07.+01:00",
        "2026-09-04T10:15:07+0100",
        "2026-13-04T10:15:07+01:00",
        " 2026-09-04T10:15:07+01:00",
    ] {
        assert_eq!(read(refused), None, "{refused}");
    }
}

#[test]
fn a_date_the_tool_sets_is_the_stored_date_of_the_write_time() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_3));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_3));
    old.removed(&pin, "Gone", desk(DAY_3));
    old.saved(&lantern, "A brighter lamp", "\n", desk(DAY_4));
    let store = read(&old);
    let decisions = answers("[documents.\"fact/lantern/relay-pin\"]\nended = \"false\"\n");
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    let lantern = of(&planned, Kind::Topic, "lantern");
    assert_eq!(
        lantern.versions[0].stamp.as_ref().unwrap().to_string(),
        DAY_3
    );
    assert_eq!(
        lantern.versions[1].stamp.as_ref().unwrap().to_string(),
        DAY_4
    );
    assert_eq!(topic(&lantern.versions[1]).created, date("2026-09-06"));
    let pin = of(&planned, Kind::Fact, "lantern/relay-pin");
    assert_eq!(fact(&pin.versions[0]).created, date("2026-09-06"));
    assert_eq!(fact(&pin.versions[0]).confirmed, date("2026-09-06"));
    assert_eq!(ending(&pin.versions[1]).unwrap().on, date("2026-09-06"));
}

#[test]
fn a_write_time_that_is_not_read_blocks() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk("yesterday"));
    let saved = old.saved(&lantern, "A brighter lamp", "\n", desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::WriteTime {
            document: key(Kind::Topic, "lantern"),
            version: lantern.id.to_string(),
            text: "yesterday".to_owned(),
        })]
    );
    let versions = &of(&planned, Kind::Topic, "lantern").versions;
    assert_eq!(versions[0].stamp, None);
    assert_eq!(versions[0].record, None);
    assert_eq!(versions[1].key, saved.id.to_string());
    assert_eq!(versions[1].record, None);
    assert!(versions[1].stamp.is_some());
}

#[test]
fn what_blocks_a_renames_tombstone_is_asked_under_the_slug_it_left() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (left, _) = old.renamed(&first, "lamp", desk("yesterday"));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::WriteTime {
            document: key(Kind::Topic, "lantern"),
            version: left.id.to_string(),
            text: "yesterday".to_owned(),
        })]
    );
}

#[test]
fn a_slug_resolves_to_the_document_its_renames_made_and_says_whether_it_ended() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    let phone = old.topic("Phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_2));
    old.topic("PHONE", "Another phone", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.renamed(&pin, "lamp/relay", desk(DAY_3));
    let store = read(&old);
    let decisions = answers("[topics.lantern-firmware]\nsummary = \"The firmware\"\n");
    let questions = Questions::default();
    let chains = Chains::build(&store, &questions);
    let resolver = Resolver::build(&store, &chains, &decisions);

    let found = |name: &str| match resolver.topic(name) {
        Named::One(found) => Some((found.id, found.name, found.ended)),
        Named::None => None,
        Named::Several(topics) => panic!("{name} names {topics:?}"),
        Named::Unsettled => panic!("{name} names an unsettled topic"),
    };
    let lamp = Some((topic_id("lantern"), "lamp".to_owned(), false));
    assert_eq!(found("lantern"), lamp);
    assert_eq!(found("LAMP"), lamp);
    assert_eq!(
        found("Phone"),
        Some((topic_id("Phone"), "Phone".to_owned(), true))
    );
    assert_eq!(
        found("PHONE"),
        Some((topic_id("PHONE"), "PHONE".to_owned(), false))
    );
    let both = vec!["PHONE".to_owned(), "Phone".to_owned()];
    assert_eq!(resolver.topic("phone"), Named::Several(both));
    assert_eq!(found("ghost"), None);
    let firmware = created_topic_id("lantern-firmware");
    assert_eq!(firmware, generated_id("topic\0lantern-firmware"));
    assert_eq!(
        found("Lantern-Firmware"),
        Some((firmware, "lantern-firmware".to_owned(), false))
    );
    let relay = resolver.document(Kind::Fact, "lantern/relay-pin");
    let relay = relay.unwrap().unwrap();
    assert_eq!(relay.id, document_id(Kind::Fact, "lantern/relay-pin"));
    assert_eq!(relay.name, "lamp/relay");
    assert_eq!(resolver.document(Kind::Fact, "lamp/relay"), Ok(Some(relay)));
    assert_eq!(resolver.document(Kind::Topic, "lamp/relay"), Ok(None));
    assert_eq!(
        resolver.document(Kind::Entry, "2026-09/2026-09-04-lamp"),
        Ok(None)
    );
    assert_eq!(decisions.unused().len(), 1);
}

#[test]
fn a_machine_with_no_topic_is_asked_about_and_the_file_answers() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], at(DAY_1, "phone"));
    old.saved(&lantern, "A brighter lamp", "\n", at(DAY_2, "phone"));
    old.topic("atlas", "A book of maps", &[], at(DAY_2, "tablet"));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    let machine = |name: &str, versions| Open::Machine {
        name: name.to_owned(),
        versions,
        documents: Vec::new(),
    };
    assert_eq!(opens(&asked), [machine("phone", 2), machine("tablet", 1)]);
    let versions = &of(&asked, Kind::Topic, "lantern").versions;
    assert_eq!(versions[0].machine, None);
    assert!(versions[0].record.is_some());
    assert_eq!(
        of(&asked, Kind::Topic, "desk").versions[0].machine,
        Some(topic_id("desk"))
    );

    let decisions = answers("[machines]\nphone = \"Desk\"\ntablet = \"slate\"\n");
    let answered = plan(&store, &decisions);
    assert_eq!(
        answered.questions,
        [Question::Open(Open::Answer {
            key: Key::Machine("tablet".to_owned()),
            why: "slate names no topic".to_owned(),
        })]
    );
    let versions = &of(&answered, Kind::Topic, "lantern").versions;
    assert_eq!(versions[0].machine, Some(topic_id("desk")));
    assert_eq!(versions[1].machine, Some(topic_id("desk")));
}

#[test]
fn a_machine_of_two_topics_is_asked_about_and_the_file_picks_one() {
    let old = OldStore::new();
    let desk_topic = old.machine_topic("desk", &[], &[], desk(DAY_1));
    old.renamed(&desk_topic, "bench", desk(DAY_2));
    old.machine_topic("workshop", &[], &[], desk(DAY_2));
    old.topic("lantern", "A lamp", &[], desk(DAY_3));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    assert_eq!(
        asked.questions,
        [Question::Open(Open::MachineTopics {
            name: "desk".to_owned(),
            topics: vec!["bench".to_owned(), "workshop".to_owned()],
        })]
    );

    let picked = plan(&store, &answers("[machines]\ndesk = \"desk\"\n"));
    assert_eq!(picked.questions, []);
    assert_eq!(
        of(&picked, Kind::Topic, "lantern").versions[0].machine,
        Some(topic_id("desk"))
    );

    let other = plan(&store, &answers("[machines]\ndesk = \"lantern\"\n"));
    assert_eq!(
        other.questions,
        [Question::Open(Open::Answer {
            key: Key::Machine("desk".to_owned()),
            why: "lantern is not a topic of the machine desk".to_owned(),
        })]
    );
}

#[test]
fn a_topic_carries_its_fields_in_every_version() {
    let old = with_desk();
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("phone", "A phone", &[], desk(DAY_1));
    let first = old.topic("lantern", "A lamp", &["atlas"], desk(DAY_1));
    let edited = old.saved(&first, "A brighter lamp", "\nBrighter.\n", desk(DAY_2));
    let (_, moved) = old.renamed(&edited, "lamp", desk(DAY_3));
    let includes = [("includes", list(&["phone", "atlas"]))];
    old.changed(&moved, "save", &includes, desk(DAY_4));
    let store = read(&old);
    let decisions = answers("[topics.lamp]\npart_of = [\"atlas\"]\nuses = [\"phone\"]\n");
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let lantern = of(&planned, Kind::Topic, "lantern");
    let (atlas, phone) = (topic_id("atlas"), topic_id("phone"));
    let expected = |name: &str, former: &[&str], summary: &str, uses: &[DocumentId]| Topic {
        name: names(&[name]).remove(0),
        former_names: names(former),
        created: date("2026-09-04"),
        summary: summary.to_owned(),
        part_of: vec![atlas.clone()],
        uses: uses.to_vec(),
    };
    let topics: Vec<Topic> = lantern.versions.iter().map(topic).collect();
    assert_eq!(
        topics,
        [
            expected("lantern", &[], "A lamp", &[]),
            expected("lantern", &[], "A brighter lamp", &[]),
            expected("lamp", &["lantern"], "A brighter lamp", &[]),
            expected("lamp", &["lantern"], "A brighter lamp", &[phone]),
        ]
    );
    let bodies: Vec<&str> = lantern
        .versions
        .iter()
        .map(|version| version.body.as_str())
        .collect();
    assert_eq!(
        bodies,
        ["\n", "\nBrighter.\n", "\nBrighter.\n", "\nBrighter.\n"]
    );
    let changes: Vec<&str> = (lantern.versions.iter())
        .map(|version| version.change.as_str())
        .collect();
    assert_eq!(changes, ["new", "save", "rename", "save"]);
    assert!(
        lantern
            .versions
            .iter()
            .all(|version| version.links.0.is_empty())
    );
    assert!((lantern.versions.iter()).all(|version| version.machine == Some(topic_id("desk"))));
}

#[test]
fn each_item_of_includes_is_placed_dropped_or_asked_about() {
    let old = with_desk();
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("beacon", "A light on a hill", &[], desk(DAY_1));
    old.topic(
        "lantern",
        "A lamp",
        &[
            "atlas",
            "phone",
            "beacon",
            "ghost",
            "gone",
            "lost",
            "lantern-firmware",
        ],
        desk(DAY_1),
    );
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[topics.lantern]\npart_of = [\"atlas\", \"lost\", \"lantern-firmware\"]\n\
         drop = [\"phone\", \"gone\"]\nuses = [\"desk\"]\n\
         [topics.lantern-firmware]\nsummary = \"The firmware of the lantern\"\n"
    ));
    let planned = plan(&store, &decisions);

    let include = |item: &str, missing| Open::Include {
        topic: Folded::new("lantern"),
        item: Folded::new(item),
        missing,
        ended: false,
        unread: false,
    };
    assert_eq!(
        opens(&planned),
        [
            include("beacon", false),
            include("ghost", true),
            Open::Answer {
                key: Key::placed("lantern", Placement::PartOf, "lost"),
                why: "lost names no topic".to_owned(),
            },
        ]
    );
    assert_eq!(
        of(&planned, Kind::Topic, "lantern").versions[0].record,
        None
    );
    assert_eq!(planned.unused, []);

    let settled = answers(&format!(
        "{CONVERSION}[topics.lantern]\npart_of = [\"atlas\", \"lantern-firmware\"]\n\
         uses = [\"beacon\"]\ndrop = [\"phone\", \"gone\", \"ghost\", \"lost\"]\n\
         [topics.lantern-firmware]\nsummary = \"The firmware of the lantern\"\n"
    ));
    let planned = plan(&store, &settled);
    assert_eq!(planned.questions, []);
    let lantern = topic(&of(&planned, Kind::Topic, "lantern").versions[0]);
    assert_eq!(
        lantern.part_of,
        [topic_id("atlas"), created_topic_id("lantern-firmware")]
    );
    assert_eq!(lantern.uses, [topic_id("beacon")]);
}

#[test]
fn a_fact_is_confirmed_when_verified_and_else_when_its_text_last_changed() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let first = old.fact("lantern/relay-pin", "Pin 5", &["lantern"], desk(DAY_1));
    let tags = [("tags", list(&["lantern", "relay"]))];
    let tagged = old.changed(&first, "save", &tags, desk(DAY_2));
    let saved = old.saved(&tagged, "Pin 6", "\nWhat holds now.\n", desk(DAY_3));
    let verified = [("verified", scalar("2026-09-07"))];
    let checked = old.changed(&saved, "verify", &verified, desk(DAY_4));
    let rechecked = [("recheck", scalar("2026-12-01 Look again"))];
    old.changed(&checked, "recheck", &rechecked, desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let relay = of(&planned, Kind::Fact, "lantern/relay-pin");
    let facts: Vec<Fact> = relay.versions.iter().map(fact).collect();
    let confirmed: Vec<Date> = facts.iter().map(|fact| fact.confirmed).collect();
    assert_eq!(
        confirmed,
        [
            date("2026-09-04"),
            date("2026-09-04"),
            date("2026-09-06"),
            date("2026-09-07"),
            date("2026-09-07"),
        ]
    );
    assert_eq!(
        facts[4],
        Fact {
            name: names(&["relay-pin"]).remove(0),
            former_names: Vec::new(),
            topic: topic_id("lantern"),
            created: date("2026-09-04"),
            confirmed: date("2026-09-07"),
            idea: false,
            summary: "Pin 6".to_owned(),
        }
    );
    let changes: Vec<&str> = (relay.versions.iter())
        .map(|version| version.change.as_str())
        .collect();
    assert_eq!(changes, ["new", "save", "save", "verify", "trigger"]);
    let source = relay.versions[4].origin.source().unwrap();
    assert_eq!(source.document.slug, "lantern/relay-pin");
    assert!(
        source
            .version
            .fields
            .iter()
            .any(|(key, _)| key == "recheck")
    );
    assert!(source.version.fields.iter().any(|(key, _)| key == "tags"));
}

#[test]
fn a_version_that_joins_two_heads_follows_both() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let (one, other) = old.forked(&pin, desk(DAY_2), desk(DAY_3));
    let heads = [&one, &other];
    let fields = one.fields.clone();
    let slug = "lantern/relay-pin";
    let joined = composed(slug, desk(DAY_5), "resolve", &heads, fields, &one.body);
    old.write(&joined);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let relay = of(&planned, Kind::Fact, slug);
    assert_eq!(relay.versions.len(), 4);
    let last = &relay.versions[3];
    assert_eq!(last.key, joined.id.to_string());
    assert_eq!(last.parents, [one.id.to_string(), other.id.to_string()]);
    assert_eq!(last.change, "resolve");
    assert_eq!(fact(last).summary, "As one machine has it");
    assert_eq!(fact(last).confirmed, date("2026-09-06"));
}

#[test]
fn a_document_first_written_twice_was_created_when_the_earlier_was_written() {
    let old = with_desk();
    let first = |summary: &str, written: &str| {
        OldStore::new().topic("lantern", summary, &[], desk(written))
    };
    let made = (0..64).find_map(|turn| {
        let earlier = first(&format!("A lamp, told the {turn}. time"), DAY_2);
        let later = first("A lamp", DAY_3);
        (later.id.to_string() < earlier.id.to_string()).then_some((earlier, later))
    });
    let (earlier, later) = made.unwrap();
    old.write(&earlier);
    old.write(&later);
    let fields = later.fields.clone();
    let both = composed(
        "lantern",
        desk(DAY_4),
        "resolve",
        &[&earlier, &later],
        fields,
        "\n",
    );
    old.write(&both);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let versions = &of(&planned, Kind::Topic, "lantern").versions;
    assert_eq!(versions[0].key, later.id.to_string());
    for version in versions {
        assert_eq!(
            topic(version).created,
            date("2026-09-05"),
            "{}",
            version.key
        );
    }
}

#[test]
fn an_idea_is_a_fact_flagged_as_one() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let idea = old.idea("lantern/solar-panel", "Charge by day", &[], desk(DAY_1));
    let built = [("idea", scalar(""))];
    old.changed(&idea, "save", &built, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let panel = of(&planned, Kind::Fact, "lantern/solar-panel");
    assert_eq!(panel.kind, KindOf::Fact);
    assert!(fact(&panel.versions[0]).idea);
    assert!(!fact(&panel.versions[1]).idea);
}

#[test]
fn a_tombstone_ends_the_document_for_the_reason_the_file_gives() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.removed(&pin, "Moved to the atlas", desk(DAY_3));
    let idea = old.idea("lantern/solar-panel", "Charge by day", &[], desk(DAY_1));
    let ended_once = old.removed(&idea, "Too dim", desk(DAY_2));
    let again = old.removed(&ended_once, "Too dim in winter", desk(DAY_4));
    let store = read(&old);
    let decisions = answers(
        "[documents.\"fact/lantern/relay-pin\"]\nended = \"moved\"\nended_by = \"topic/atlas\"\n\
         [documents.\"fact/lantern/solar-panel\"]\nended = \"abandoned\"\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let pin = of(&planned, Kind::Fact, "lantern/relay-pin");
    assert_eq!(
        ending(&pin.versions[1]),
        Some(Ending {
            reason: Reason::Moved,
            on: date("2026-09-06"),
            by: Some(topic_id("atlas")),
            note: "Moved to the atlas".to_owned(),
        })
    );
    assert_eq!(pin.versions[1].body, "\nWhat holds.\n");
    assert_eq!(pin.versions[1].change, "moved");
    assert_eq!(fact(&pin.versions[1]), fact(&pin.versions[0]));
    assert_eq!(ending(&pin.versions[0]), None);

    let panel = of(&planned, Kind::Fact, "lantern/solar-panel");
    assert_eq!(panel.versions.len(), 3);
    let ended = |on: &str, note: &str| {
        Some(Ending {
            reason: Reason::Abandoned,
            on: date(on),
            by: None,
            note: note.to_owned(),
        })
    };
    assert_eq!(ending(&panel.versions[1]), ended("2026-09-05", "Too dim"));
    assert_eq!(
        ending(&panel.versions[2]),
        ended("2026-09-08", "Too dim in winter")
    );
    assert_eq!(panel.versions[2].key, again.id.to_string());
    assert_eq!(panel.versions[2].parents, [ended_once.id.to_string()]);
    assert_eq!(panel.versions[2].body, "\nWhat holds.\n");
    assert_eq!(fact(&panel.versions[2]), fact(&panel.versions[0]));
}

#[test]
fn the_document_that_ended_another_is_read_where_the_file_gives_a_reason() {
    let old = with_desk();
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let pin = old.fact("atlas/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.removed(&pin, "Wrong pin", desk(DAY_2));
    let scale = old.fact("atlas/scale", "One to ten", &[], desk(DAY_1));
    old.removed(&scale, "Redrawn", desk(DAY_2));
    old.fact("atlas/legend", "Blue is water", &[], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(
        "[documents.\"fact/atlas/relay-pin\"]\nended_by = \"fact/atlas/legend\"\n\
         [documents.\"fact/atlas/scale\"]\nended = \"abandoned\"\n\
         ended_by = \"fact/atlas/legend\"\n",
    );
    let planned = plan(&store, &decisions);

    let (pin, scale) = (
        key(Kind::Fact, "atlas/relay-pin"),
        key(Kind::Fact, "atlas/scale"),
    );
    assert_eq!(
        opens(&planned),
        [
            Open::Ended {
                document: pin.clone(),
                idea: false,
                note: "Wrong pin".to_owned(),
                already: None,
            },
            Open::Answer {
                key: Key::document(&scale, DocumentField::Ended),
                why: "abandoned is not one of: false, moved, superseded".to_owned(),
            },
        ]
    );
    assert_eq!(
        planned.unused,
        [Key::document(&pin, DocumentField::EndedBy)]
    );
}

#[test]
fn an_ending_with_no_reason_or_a_reason_of_another_kind_is_open() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let pin = old.fact("atlas/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.removed(&pin, "Wrong pin", desk(DAY_2));
    let idea = old.idea("atlas/solar-panel", "Charge by day", &[], desk(DAY_1));
    old.removed(&idea, "Too dim", desk(DAY_2));
    old.removed(&lantern, "Sold", desk(DAY_2));
    let store = read(&old);
    let decisions = answers(
        "[documents.\"fact/atlas/relay-pin\"]\nended = \"abandoned\"\n\
         [documents.\"topic/lantern\"]\nended = \"retired\"\nended_by = \"topic/ghost\"\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(
        opens(&planned),
        [
            Open::Ended {
                document: key(Kind::Fact, "atlas/solar-panel"),
                idea: true,
                note: "Too dim".to_owned(),
                already: None,
            },
            Open::Answer {
                key: Key::document(&key(Kind::Fact, "atlas/relay-pin"), DocumentField::Ended),
                why: "abandoned is not one of: false, moved, superseded".to_owned(),
            },
            Open::Answer {
                key: Key::document(&key(Kind::Topic, "lantern"), DocumentField::EndedBy),
                why: "topic/ghost names no document".to_owned(),
            },
        ]
    );
    let offered: Vec<Option<String>> = (opens(&planned)[0].offers().into_iter())
        .map(|offer| offer.evidence)
        .collect();
    assert_eq!(
        offered,
        [Some("one of: built, abandoned; note: Too dim".to_owned())]
    );
    for (kind, origin) in [
        (Kind::Fact, "atlas/relay-pin"),
        (Kind::Fact, "atlas/solar-panel"),
        (Kind::Topic, "lantern"),
    ] {
        let versions = &of(&planned, kind, origin).versions;
        assert!(versions[0].record.is_some());
        assert_eq!(versions[1].record, None, "{origin}");
        assert_eq!(versions[1].body, versions[0].body);
    }
    assert_eq!(
        of(&planned, Kind::Fact, "atlas/solar-panel").versions[1].change,
        "tombstone"
    );
}

#[test]
fn a_name_the_new_store_refuses_is_asked_about_and_the_file_answers() {
    let old = with_desk();
    let first = old.topic("Lantern_Old", "A lamp", &[], desk(DAY_1));
    old.renamed(&first, "lantern", desk(DAY_2));
    old.topic("atlas-", "A book of maps", &[], desk(DAY_1));
    old.fact("lantern/Relay.Pin", "Pin 5", &[], desk(DAY_2));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    let name = |kind, slug: &str, name: &str, why: &str| Open::Name {
        document: key(kind, slug),
        name: name.to_owned(),
        why: why.to_owned(),
    };
    assert_eq!(
        opens(&asked),
        [
            name(
                Kind::Fact,
                "lantern/Relay.Pin",
                "Relay.Pin",
                "holds an uppercase letter"
            ),
            name(
                Kind::Topic,
                "atlas-",
                "atlas-",
                "opens or closes with a hyphen"
            ),
            Open::FormerName {
                document: key(Kind::Topic, "lantern"),
                name: "Lantern_Old".to_owned(),
                why: "holds an uppercase letter".to_owned(),
            },
        ]
    );
    let versions = &of(&asked, Kind::Topic, "Lantern_Old").versions;
    assert_eq!(versions[0].record, None);
    assert_eq!(versions[1].record, None);

    let decisions = answers(
        "[documents.\"topic/lantern\"]\nformer_names = { Lantern_Old = \"lantern-old\" }\n\
         [documents.\"topic/atlas-\"]\nname = \"Year\"\n\
         [documents.\"fact/lantern/Relay.Pin\"]\nname = \"relay-pin\"\n",
    );
    let answered = plan(&store, &decisions);
    assert_eq!(
        answered.questions,
        [Question::Open(Open::Answer {
            key: Key::document(&key(Kind::Topic, "atlas-"), DocumentField::Name),
            why: "Year holds an uppercase letter".to_owned(),
        })]
    );
    let versions = &of(&answered, Kind::Topic, "Lantern_Old").versions;
    assert_eq!(topic(&versions[0]).name, names(&["lantern-old"])[0]);
    assert_eq!(topic(&versions[1]).name, names(&["lantern"])[0]);
    assert_eq!(topic(&versions[1]).former_names, names(&["lantern-old"]));
    let pin = fact(&of(&answered, Kind::Fact, "lantern/Relay.Pin").versions[0]);
    assert_eq!(pin.name, names(&["relay-pin"])[0]);
    assert_eq!(pin.topic, topic_id("Lantern_Old"));
}

#[test]
fn why_a_name_is_refused_says_which_rule_it_breaks() {
    assert_eq!(refusal("relay-pin"), None);
    assert_eq!(refusal("1password"), None);
    let why = |text: &str| refusal(text).unwrap();
    assert_eq!(why("Relay"), "holds an uppercase letter");
    assert_eq!(
        why("relay_pin"),
        "holds `_`, and a name is lowercase letters, digits and hyphens"
    );
    assert_eq!(why("2026"), "holds no letter");
    assert_eq!(why("relay-"), "opens or closes with a hyphen");
    assert_eq!(
        why("2026-09-04-relay"),
        "reads as a date or as a document id"
    );
    assert_eq!(why(&"ab".repeat(16)), "reads as a date or as a document id");
}

#[test]
fn two_facts_that_end_under_one_topic_with_one_name_are_asked_about() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.fact("lantern/relay", "Pin 5", &[], desk(DAY_1));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    let newer = old.fact("lamp/relay", "Pin 6", &[], desk(DAY_3));
    let saved = old.saved(&newer, "Pin 7", "\nWhat holds.\n", desk(DAY_4));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.fact("atlas/relay", "A relay station", &[], desk(DAY_1));
    let gone = old.fact("lamp/dimmer", "Dims by half", &[], desk(DAY_3));
    old.removed(&gone, "Never built", desk(DAY_4));
    old.fact("lantern/dimmer", "Dims by a third", &[], desk(DAY_1));
    let store = read(&old);
    let ended = "[documents.\"fact/lamp/dimmer\"]\nended = \"false\"\n";

    let asked = plan(&store, &answers(ended));
    let shared = |slug: &str| Open::Name {
        document: key(Kind::Fact, slug),
        name: "relay".to_owned(),
        why: "is held by several documents".to_owned(),
    };
    assert_eq!(
        opens(&asked),
        [shared("lamp/relay"), shared("lantern/relay")]
    );

    let named = format!("{ended}[documents.\"fact/lamp/relay\"]\nname = \"relay-two\"\n");
    let unstamped = plan(&store, &answers(&named));
    assert_eq!(opens(&unstamped), [Open::Conversion { versions: 1 }]);

    let decisions = answers(&format!("{CONVERSION}{named}"));
    let answered = plan(&store, &decisions);
    assert_eq!(answered.questions, []);
    assert_eq!(decisions.unused(), []);
    let versions = &of(&answered, Kind::Fact, "lamp/relay").versions;
    assert_eq!(versions.len(), 3);
    for converted in &versions[..2] {
        assert_eq!(fact(converted).name, names(&["relay"])[0]);
        assert_eq!(fact(converted).former_names, []);
    }
    authored_by_desk(&versions[2], &[&saved], "rename");
    let renamed = Fact {
        name: names(&["relay-two"])[0].clone(),
        former_names: names(&["relay"]),
        ..fact(&versions[1])
    };
    assert_eq!(fact(&versions[2]), renamed);
    assert_eq!(versions[2].body, versions[1].body);
    assert_eq!(
        answered.authored,
        [Authored::Renamed {
            document: "fact/lamp/relay".to_owned(),
            from: "relay".to_owned(),
            to: "relay-two".to_owned(),
        }]
    );
    let older = of(&answered, Kind::Fact, "lantern/relay");
    assert_eq!(older.versions.len(), 1);
    assert_eq!(fact(&older.versions[0]).name, names(&["relay"])[0]);

    let taken = answers(&format!(
        "{CONVERSION}{ended}[documents.\"fact/lamp/relay\"]\nname = \"dimmer\"\n"
    ));
    assert_eq!(
        opens(&plan(&store, &taken)),
        [
            Open::Name {
                document: key(Kind::Fact, "lantern/dimmer"),
                name: "dimmer".to_owned(),
                why: "is held by several documents".to_owned(),
            },
            Open::Answer {
                key: Key::document(&key(Kind::Fact, "lamp/relay"), DocumentField::Name),
                why: "dimmer is held by several documents".to_owned(),
            },
        ]
    );
}

#[test]
fn a_summary_the_store_lacks_is_asked_about_and_the_file_answers() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let mut fields = lantern.fields.clone();
    fields.set("summary", scalar(""));
    let blank = composed("lantern", desk(DAY_2), "save", &[&lantern], fields, "\n");
    old.write(&blank);
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    assert_eq!(
        asked.questions,
        [Question::Open(Open::Summary {
            document: key(Kind::Topic, "lantern"),
        })]
    );
    let versions = &of(&asked, Kind::Topic, "lantern").versions;
    assert!(versions[0].record.is_some());
    assert_eq!(versions[1].record, None);

    let decisions = answers("[documents.\"topic/lantern\"]\nsummary = \"A lamp, still\"\n");
    let answered = plan(&store, &decisions);
    assert_eq!(answered.questions, []);
    let versions = &of(&answered, Kind::Topic, "lantern").versions;
    assert_eq!(topic(&versions[0]).summary, "A lamp");
    assert_eq!(topic(&versions[1]).summary, "A lamp, still");
}

#[test]
fn a_version_with_keys_outside_the_grammar_blocks_and_is_still_planned() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let signed = lantern
        .to_text()
        .replace(
            "  operation: new\n",
            "  operation: new\n  signed: desk-key\n",
        )
        .replace("slug: lantern\n", "slug: atlas\n");
    let path = old.planted("topic/atlas", &signed);
    let mut fields = lantern.fields.clone();
    fields.set("color", scalar("amber"));
    fields.set("weight", scalar("light"));
    let colored = composed("lantern", desk(DAY_2), "save", &[&lantern], fields, "\n");
    old.write(&colored);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let atlas = path.rsplit('/').next().unwrap().trim_end_matches(".md");
    let foreign = |slug: &str, version: &str, keys: &[&str]| Blocked::ForeignKeys {
        document: key(Kind::Topic, slug),
        version: version.to_owned(),
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
    };
    assert_eq!(
        blocks(&planned),
        [
            foreign("atlas", atlas, &["signed"]),
            foreign("lantern", &colored.id.to_string(), &["color", "weight"]),
        ]
    );
    assert_eq!(opens(&planned), []);
    assert_eq!(
        topic(&of(&planned, Kind::Topic, "lantern").versions[1]).summary,
        "A lamp"
    );
    assert_eq!(
        topic(&of(&planned, Kind::Topic, "atlas").versions[0]).name,
        names(&["atlas"])[0]
    );
}

#[test]
fn a_value_no_field_of_the_new_fact_takes_blocks() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let mut fields = pin.fields.clone();
    fields.set("idea", scalar("maybe"));
    let maybe = composed(
        "lantern/relay-pin",
        desk(DAY_2),
        "save",
        &[&pin],
        fields,
        "\n",
    );
    old.write(&maybe);
    let mut fields = pin.fields.clone();
    fields.set("verified", scalar("2026-02-31"));
    let never = composed(
        "lantern/relay-pin",
        desk(DAY_3),
        "verify",
        &[&maybe],
        fields,
        "\n",
    );
    old.write(&never);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let refused = |version: &Stored, field: &str, value: &str, why: &str| Blocked::Value {
        document: DocumentKey::new(Kind::Fact, "lantern/relay-pin"),
        version: version.id.as_str().to_owned(),
        field: field.to_owned(),
        value: value.to_owned(),
        why: why.to_owned(),
    };
    let mut expected = vec![
        refused(&maybe, "idea", "maybe", "is not `true`"),
        refused(&never, "verified", "2026-02-31", "is not a day"),
    ];
    expected.sort();
    assert_eq!(blocks(&planned), expected);
    let versions = &of(&planned, Kind::Fact, "lantern/relay-pin").versions;
    assert!(versions[0].record.is_some());
    assert_eq!(versions[1].record, None);
    assert_eq!(versions[2].record, None);
}

#[test]
fn what_stood_between_the_store_and_its_reading_is_among_the_questions() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (one, other) = old.forked(&lantern, desk(DAY_2), desk(DAY_3));
    old.stray_file("notes.txt", "a note\n");
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let mut heads = vec![one.id.to_string(), other.id.to_string()];
    heads.sort();
    assert_eq!(
        blocks(&planned),
        [
            Blocked::Forked {
                document: key(Kind::Topic, "lantern"),
                heads,
            },
            Blocked::Skipped {
                path: "notes.txt".to_owned(),
            },
        ]
    );
    assert_eq!(of(&planned, Kind::Topic, "lantern").versions.len(), 3);
}

#[test]
fn a_topic_that_includes_itself_is_asked_to_drop_the_item() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &["Lantern"], desk(DAY_1));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    let store = read(&old);
    let itself = [Question::Open(Open::Answer {
        key: Key::placed("lamp", Placement::Drop, "lamp"),
        why: "lamp includes itself: write it in drop".to_owned(),
    })];

    assert_eq!(plan(&store, &Decisions::default()).questions, itself);
    let placed = answers("[topics.lamp]\nuses = [\"lamp\"]\n");
    assert_eq!(plan(&store, &placed).questions, itself);
    let dropped = answers("[topics.lamp]\ndrop = [\"lamp\"]\n");
    let planned = plan(&store, &dropped);
    assert_eq!(planned.questions, []);
    assert_eq!(dropped.unused(), []);
    assert_eq!(
        topic(&of(&planned, Kind::Topic, "lantern").versions[0]).uses,
        []
    );
}

#[test]
fn a_fact_is_confirmed_on_the_later_of_its_check_and_its_last_edit() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let first = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let verified = [("verified", scalar("2026-09-05"))];
    let checked = old.changed(&first, "verify", &verified, desk(DAY_2));
    let edited = old.saved(&checked, "Pin 6", "\nWhat holds now.\n", desk(DAY_3));
    let verified = [("verified", scalar("2026-09-09"))];
    let rechecked = old.changed(&edited, "verify", &verified, desk(DAY_4));
    let unchecked = [("verified", scalar(""))];
    let cleared = old.changed(&rechecked, "save", &unchecked, desk(DAY_5));
    old.changed(&cleared, "save", &[("tags", list(&["relay"]))], desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let relay = of(&planned, Kind::Fact, "lantern/relay-pin");
    let confirmed: Vec<Date> = (relay.versions.iter())
        .map(|version| fact(version).confirmed)
        .collect();
    let checked_then_edited = date("2026-09-06");
    let edited_then_checked = date("2026-09-09");
    let check_dropped = date("2026-09-06");
    assert_eq!(
        confirmed,
        [
            date("2026-09-04"),
            date("2026-09-05"),
            checked_then_edited,
            edited_then_checked,
            check_dropped,
            check_dropped,
        ]
    );
}

#[test]
fn a_change_of_the_summary_alone_or_the_body_alone_confirms_a_fact() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let first = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let reworded = old.changed(&first, "save", &[("summary", scalar("Pin 6"))], desk(DAY_2));
    let body = following(
        &reworded,
        desk(DAY_3),
        "save",
        &[],
        Some("\nWhat holds now.\n"),
    );
    old.write(&body);
    old.changed(&body, "save", &[("tags", list(&["relay"]))], desk(DAY_4));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let relay = of(&planned, Kind::Fact, "lantern/relay-pin");
    let confirmed: Vec<Date> = (relay.versions.iter())
        .map(|version| fact(version).confirmed)
        .collect();
    assert_eq!(
        confirmed,
        [
            date("2026-09-04"),
            date("2026-09-05"),
            date("2026-09-06"),
            date("2026-09-06"),
        ]
    );
}

#[test]
fn a_rename_written_as_two_halves_apart_takes_the_stamp_of_its_tombstone() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (left, moved) = old.renamed_apart(&first, "lamp", desk(DAY_3), at(DAY_4, "phone"));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let renamed = &of(&planned, Kind::Topic, "lantern").versions[1];
    assert_eq!(renamed.key, moved.id.to_string());
    assert_eq!(renamed.stamp.as_ref().unwrap().to_string(), DAY_3);
    assert_eq!(renamed.machine, Some(topic_id("desk")));
    assert_eq!(
        renamed.origin.source().unwrap().stone.unwrap().version.id,
        left.id.to_string()
    );
    assert_eq!(renamed.origin.source().unwrap().version.written, DAY_4);
    assert!(
        planned
            .documents
            .iter()
            .all(|document| document.id != topic_id("lamp"))
    );
}

#[test]
fn a_name_used_before_between_and_after_three_renames_is_one_topic() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.fact("lantern/wick", "Cotton", &[], desk(DAY_1));
    let (_, lamp) = old.renamed(&lantern, "lamp", desk(DAY_2));
    old.fact("lamp/oil", "Paraffin", &[], desk(DAY_2));
    let (_, beacon) = old.renamed(&lamp, "beacon", desk(DAY_3));
    old.fact("beacon/glass", "Clear", &[], desk(DAY_3));
    old.renamed(&beacon, "torch", desk(DAY_4));
    old.fact("torch/switch", "On the side", &[], desk(DAY_5));
    old.fact("lantern/handle", "Brass", &[], desk(DAY_5));
    old.topic(
        "atlas",
        "A book of maps",
        &["lamp", "Beacon", "lantern"],
        desk(DAY_5),
    );
    let store = read(&old);
    let decisions = answers("[topics.atlas]\nuses = [\"torch\"]\n");
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    for slug in [
        "lantern/wick",
        "lamp/oil",
        "beacon/glass",
        "torch/switch",
        "lantern/handle",
    ] {
        let under = fact(&of(&planned, Kind::Fact, slug).versions[0]).topic;
        assert_eq!(under, topic_id("lantern"), "{slug}");
    }
    let atlas = topic(&of(&planned, Kind::Topic, "atlas").versions[0]);
    assert_eq!(atlas.uses, [topic_id("lantern")]);
    let last = topic(&of(&planned, Kind::Topic, "lantern").versions[3]);
    assert_eq!(last.former_names, names(&["lantern", "lamp", "beacon"]));
    for id in ["lamp", "beacon", "torch"] {
        assert!(
            planned
                .documents
                .iter()
                .all(|document| document.id != topic_id(id))
        );
    }
}

#[test]
fn a_name_that_comes_back_is_no_former_name_of_its_own() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let pin = old.fact("lantern/pin", "Pin 5", &[], desk(DAY_1));
    let (_, contact) = old.renamed(&pin, "lantern/contact", desk(DAY_2));
    old.renamed(&contact, "atlas/pin", desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let facts: Vec<Fact> = (of(&planned, Kind::Fact, "lantern/pin").versions.iter())
        .map(fact)
        .collect();
    assert_eq!(facts[1].former_names, names(&["pin"]));
    assert_eq!(facts[2].name, names(&["pin"])[0]);
    assert_eq!(facts[2].former_names, names(&["contact"]));
    assert_eq!(facts[2].topic, topic_id("atlas"));
}

#[test]
fn a_tombstone_with_no_note_ends_the_document_with_none() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    old.removed_with_no_note(&pin, desk(DAY_2));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    assert_eq!(
        asked.questions,
        [Question::Open(Open::Ended {
            document: key(Kind::Fact, "lantern/relay-pin"),
            idea: false,
            note: String::new(),
            already: None,
        })]
    );
    let decisions = answers("[documents.\"fact/lantern/relay-pin\"]\nended = \"false\"\n");
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    let ended = &of(&planned, Kind::Fact, "lantern/relay-pin").versions[1];
    assert_eq!(ending(ended).unwrap().note, "");
    assert!(!ended.record.as_ref().unwrap().fields().contains_key("note"));
    assert_eq!(ended.body, "\nWhat holds.\n");
}

#[test]
fn a_document_ended_twice_is_ended_by_the_same_document_both_times() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.fact("lantern/fuse", "Two amps", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let once = old.removed(&pin, "Replaced", desk(DAY_2));
    old.removed(&once, "Replaced by the fuse", desk(DAY_3));
    let store = read(&old);
    let decisions = answers(
        "[documents.\"fact/lantern/relay-pin\"]\nended = \"superseded\"\n\
         ended_by = \"fact/lantern/fuse\"\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    let versions = &of(&planned, Kind::Fact, "lantern/relay-pin").versions;
    let fuse = Some(document_id(Kind::Fact, "lantern/fuse"));
    assert_eq!(ending(&versions[1]).unwrap().by, fuse);
    assert_eq!(ending(&versions[2]).unwrap().by, fuse);
    assert_eq!(ending(&versions[2]).unwrap().note, "Replaced by the fuse");
    assert_eq!(ending(&versions[2]).unwrap().on, date("2026-09-06"));
}

struct Closing;

impl Mapper for Closing {
    type State = ();

    fn map(
        &mut self,
        _: &Frame<'_, '_>,
        step: &Step<'_>,
        dates: Dates,
        parents: &[&Mapped<()>],
    ) -> Mapped<()> {
        let version = step.source.version;
        let before = parents.first().and_then(|parent| parent.record.clone());
        let closes = version.operation == "done";
        let ending = closes.then(|| Ending {
            reason: Reason::Retired,
            on: dates.on.unwrap(),
            by: None,
            note: "Closed".to_owned(),
        });
        let content = Content::Topic(Topic {
            name: names(&[step.slug()]).remove(0),
            former_names: Vec::new(),
            created: dates.created.unwrap(),
            summary: "A lamp".to_owned(),
            part_of: Vec::new(),
            uses: Vec::new(),
        });
        Mapped {
            record: Some(Record {
                content,
                ending: ending.or_else(|| before.and_then(|before| before.ending)),
            }),
            body: version.body.clone(),
            state: (),
        }
    }
}

#[test]
fn a_mapping_ends_any_version_and_what_follows_keeps_the_ending_it_is_handed() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let closed = old.changed(&first, "done", &[("summary", scalar("Over"))], desk(DAY_2));
    let later = old.saved(&closed, "Still over", "\n", desk(DAY_3));
    old.rename_cut_short(&later, "lamp", desk(DAY_4));
    let store = read(&old);
    let decisions = Decisions::default();
    let frame = Frame::build(&store, &decisions);
    let lantern = (store.documents.iter())
        .position(|document| document.slug == "lantern")
        .unwrap();
    let own = frame.chains.settled(frame.chains.of[lantern]).unwrap();
    let versions = frame.walk(own, &mut Closing).versions;

    let closed_on = Some(Ending {
        reason: Reason::Retired,
        on: date("2026-09-05"),
        by: None,
        note: "Closed".to_owned(),
    });
    let endings: Vec<Option<Ending>> = versions.iter().map(ending).collect();
    assert_eq!(
        endings,
        [None, closed_on.clone(), closed_on.clone(), closed_on]
    );
}

#[test]
fn a_tombstone_that_follows_no_one_version_blocks() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (one, other) = old.forked(&lantern, desk(DAY_2), desk(DAY_3));
    let heads = [&one, &other];
    let fields = one.fields.clone();
    let both = composed(
        "lantern",
        desk(DAY_4),
        "tombstone",
        &heads,
        fields,
        "Gone\n",
    );
    old.write(&both);
    let fields = lantern.fields.clone();
    let alone = composed(
        "beacon",
        desk(DAY_4),
        "tombstone",
        &[],
        fields,
        "Never lit\n",
    );
    old.write(&alone);
    let store = read(&old);
    let decisions = answers(
        "[documents.\"topic/lantern\"]\nended = \"retired\"\n\
         [documents.\"topic/beacon\"]\nended = \"retired\"\n",
    );
    let planned = plan(&store, &decisions);

    let follows = |slug: &str, version: &Stored, follows: &[&Stored]| Blocked::Tombstone {
        document: key(Kind::Topic, slug),
        version: version.id.to_string(),
        follows: follows
            .iter()
            .map(|version| version.id.to_string())
            .collect(),
    };
    assert_eq!(
        planned.questions,
        [
            Question::Blocked(follows("beacon", &alone, &[])),
            Question::Blocked(follows("lantern", &both, &heads)),
        ]
    );
    assert_eq!(of(&planned, Kind::Topic, "beacon").versions[0].record, None);
    let last = &of(&planned, Kind::Topic, "lantern").versions[3];
    assert_eq!(last.record, None);
    assert_eq!(last.parents, [one.id.to_string(), other.id.to_string()]);
}

fn several(name: &str) -> String {
    format!("{name} is PHONE, Phone in another case: spell one as the store does")
}

#[test]
fn a_name_several_topics_hold_blocks_where_no_answer_can_spell_one() {
    let old = with_desk();
    old.topic("Phone", "A phone", &[], desk(DAY_1));
    old.topic("PHONE", "Another phone", &[], desk(DAY_1));
    old.topic("lantern", "A lamp", &["phone", "Phone"], desk(DAY_1));
    old.fact("PHONE/charger", "Five volts", &[], desk(DAY_1));
    let store = read(&old);
    let decisions = answers("[topics.lantern]\nuses = [\"phone\"]\n");
    let planned = plan(&store, &decisions);

    assert_eq!(
        blocks(&planned),
        [Blocked::TopicCase {
            name: "phone".to_owned(),
            topics: vec!["PHONE".to_owned(), "Phone".to_owned()],
        }]
    );
    assert_eq!(
        of(&planned, Kind::Topic, "lantern").versions[0].record,
        None
    );
    let charger = fact(&of(&planned, Kind::Fact, "PHONE/charger").versions[0]);
    assert_eq!(charger.topic, topic_id("PHONE"));
}

#[test]
fn a_name_several_topics_hold_is_open_where_an_answer_can_spell_one() {
    let old = with_desk();
    old.topic("Phone", "A phone", &[], desk(DAY_1));
    old.topic("PHONE", "Another phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], at(DAY_1, "tablet"));
    old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    let entry = old.entry("2026-09/2026-09-04-call", "Called", &["calls"], desk(DAY_1));
    let port = old.followup("2026-09-01-port", &entry, "Port", &["calls"], desk(DAY_1));
    let touching = [("recheck", scalar("touching phone"))];
    old.changed(&port, "recheck", &touching, desk(DAY_2));
    let store = read(&old);
    let battery = key(Kind::Fact, "phone/battery");
    let port = key(Kind::Followup, "2026-09-01-port");
    let both = vec!["PHONE".to_owned(), "Phone".to_owned()];
    let named = "[documents.\"topic/Phone\"]\nname = \"phone-one\"\n\
        [documents.\"topic/PHONE\"]\nname = \"phone-two\"\n";
    let file = format!("[tags]\ncalls = \"phone\"\n[machines]\ntablet = \"pHone\"\n{named}");
    let planned = plan(&store, &answers(&file));

    assert_eq!(blocks(&planned), []);
    assert_eq!(
        opens(&planned),
        [
            Open::TopicCase {
                document: battery.clone(),
                through: Through::FactTopic,
                name: "phone".to_owned(),
                topics: both.clone(),
            },
            Open::TopicCase {
                document: port.clone(),
                through: Through::Touching,
                name: "phone".to_owned(),
                topics: both,
            },
            Open::Answer {
                key: Key::tag("calls"),
                why: several("phone"),
            },
            Open::Answer {
                key: Key::Machine("tablet".to_owned()),
                why: several("pHone"),
            },
        ]
    );
    assert_eq!(
        of(&planned, Kind::Fact, "phone/battery").versions[0].record,
        None
    );

    let spelled = format!(
        "[tags]\ncalls = \"PHONE\"\n[machines]\ntablet = \"Phone\"\n{named}\
         [documents.\"fact/phone/battery\"]\ntopics = [\"Phone\"]\n\
         [documents.\"followup/2026-09-01-port\"]\ntouching = \"Phone\"\n"
    );
    let decisions = answers(&spelled);
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let battery_planned = fact(&of(&planned, Kind::Fact, "phone/battery").versions[0]);
    assert_eq!(battery_planned.topic, topic_id("Phone"));
    assert_eq!(
        of(&planned, Kind::Topic, "atlas").versions[0].machine,
        Some(topic_id("Phone"))
    );

    let unspelled = spelled
        .replace("[\"Phone\"]", "[\"phonE\"]")
        .replace("touching = \"Phone\"", "touching = \"phOne\"");
    let planned = plan(&store, &answers(&unspelled));
    assert_eq!(
        opens(&planned),
        [
            Open::Answer {
                key: Key::document(&battery, DocumentField::Topics),
                why: several("phonE"),
            },
            Open::Answer {
                key: Key::document(&port, DocumentField::Touching),
                why: several("phOne"),
            },
        ]
    );
}

fn read_by_the_store(old: &OldStore) -> Loaded {
    let store = FsStore::new(old.root().to_path_buf());
    let Ok(loaded) = load(&store) else {
        panic!("the store's own reader refuses the store");
    };
    loaded
}

fn touched_by(loaded: &Loaded, slug: &str) -> String {
    let followup = (loaded.followups.iter()).find(|followup| followup.slug.path() == slug);
    match &followup.unwrap().data.recheck {
        Some(Recheck::Touching(topic)) => topic.clone(),
        other => panic!("{slug} touches no topic: {other:?}"),
    }
}

fn home_of(loaded: &Loaded, slug: &str) -> String {
    let fact = loaded.facts.iter().find(|fact| fact.slug.path() == slug);
    loaded.home(fact.unwrap()).to_owned()
}

fn touching(planned: &Planned, origin: &str) -> DocumentId {
    let followup = of(planned, Kind::Followup, origin);
    let record = followup.versions.last().unwrap().record.as_ref().unwrap();
    match &record.content {
        Content::Followup(Followup {
            trigger: Some(Trigger::Touching(topic)),
            ..
        }) => topic.clone(),
        other => panic!("{origin} touches no topic: {other:?}"),
    }
}

fn followup_touching(old: &OldStore, entry: &Stored, slug: &str, topic: &str) {
    let followup = old.followup(slug, entry, "Port", &["desk-work"], desk(DAY_1));
    let touching = [("recheck", scalar(&format!("touching {topic}")))];
    old.changed(&followup, "recheck", &touching, desk(DAY_2));
}

const PHONES: &str = "[documents.\"topic/Phone\"]\nname = \"phone-one\"\n\
    [documents.\"topic/handset\"]\nformer_names = { PHONE = \"\" }\n";

fn edges_of(planned: &Planned, origin: &str) -> BTreeSet<DocumentId> {
    let last = topic(of(planned, Kind::Topic, origin).versions.last().unwrap());
    last.part_of.into_iter().chain(last.uses).collect()
}

fn claimed(planned: &Planned) -> BTreeSet<(DocumentId, String)> {
    let claim = |document: &&Document| match &document.versions[0].record.as_ref()?.content {
        Content::Claim(claim) => {
            let directory = claim.directory.as_ref();
            let directory = directory.map(|directory| directory.as_str().to_owned());
            Some((claim.topic.clone(), directory.unwrap_or_default()))
        }
        _ => None,
    };
    of_kind(planned, KindOf::Claim)
        .iter()
        .filter_map(claim)
        .collect()
}

const SHELVED: &str = "[topics.shelf]\nuses = [\"handset\"]\ndrop = [\"atlas\"]\n\
    [topics.desk]\nunclaim = [\"atlas\"]\n\
    [documents.\"fact/Atlas/scale\"]\ntopics = [\"atlas\"]\n";

const RACKED: &str = "[topics.rack]\nuses = [\"handset\"]\ndrop = [\"beam\", \"lantern\"]\n";
const LEFT_OUT: &str = "[topics.bench]\nunclaim = [\"beam\"]\n\
    [documents.\"topic/Lantern\"]\nname = \"lamp\"\n";

fn with_items_the_reader_leaves_out(old: &OldStore) {
    let left_out = ["torch", "Beam", "lantern", "Lantern"];
    let included = ["torch", "Beam", "lantern", "PHONE", "Lantern"];
    old.topic("rack", "A rack", &included, desk(DAY_1));
    old.machine_topic("bench", &[], &left_out, at(DAY_1, "bench"));
    let torch = old.topic("torch", "A torch", &[], desk(DAY_1));
    old.renamed(&torch, "beam", desk(DAY_3));
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.renamed(&lantern, "Lantern", desk(DAY_3));
}

#[test]
fn a_name_is_the_topic_the_reader_of_the_store_takes_it_for() {
    let old = OldStore::new();
    let claims: [(&str, &[&str]); 2] = [
        ("PHONE", &["~/projects/handset"]),
        ("Atlas", &["~/projects/atlas"]),
    ];
    old.machine_topic("desk", &claims, &["Phone", "ATLAS"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic(
        "shelf",
        "A shelf",
        &["Phone", "PHONE", "Atlas"],
        desk(DAY_1),
    );
    old.fact("Atlas/scale", "One to ten", &[], desk(DAY_1));
    let shouted = old.topic("PHONE", "A phone", &[], desk(DAY_1));
    old.topic("Phone", "Another phone", &[], desk(DAY_1));
    old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    old.fact("PHONE/charger", "Five volts", &[], desk(DAY_1));
    old.fact("Phone/case", "Leather", &[], desk(DAY_1));
    let entry = old.entry("2026-09/2026-09-04-call", "Called", &[], desk(DAY_1));
    followup_touching(&old, &entry, "2026-09-01-spelled", "Phone");
    followup_touching(&old, &entry, "2026-09-02-shouted", "PHONE");
    followup_touching(&old, &entry, "2026-09-03-lower", "phone");
    old.renamed(&shouted, "handset", desk(DAY_3));
    with_items_the_reader_leaves_out(&old);
    let store = read(&old);
    let tagged = format!("{PHONES}{SHELVED}{LEFT_OUT}[tags]\ndesk-work = \"desk\"\n");
    let file = format!("{tagged}{RACKED}");
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let loaded = read_by_the_store(&old);
    let now = |name: String| match name.as_str() {
        "handset" => topic_id("PHONE"),
        "Phone" => topic_id("Phone"),
        other => panic!("the store reads {other}"),
    };
    let held = |name: &String| loaded.lookup()(name).is_some();
    let shelf = &loaded.topics["shelf"].data;
    assert_eq!(shelf.includes, ["handset", "Atlas"]);
    let included = shelf.includes.iter().filter(|name| held(name));
    let included: BTreeSet<DocumentId> = included.map(|name| now(name.clone())).collect();
    assert_eq!(edges_of(&planned, "shelf"), included);
    let rack = &loaded.topics["rack"].data;
    assert_eq!(rack.includes, ["Beam", "handset"]);
    let included = rack.includes.iter().filter(|name| held(name));
    let included: BTreeSet<DocumentId> = included.map(|name| now(name.clone())).collect();
    assert_eq!(edges_of(&planned, "rack"), included);
    let unplaced = plan(&store, &answers(&tagged));
    let include = |item: &str, unread| Open::Include {
        topic: Folded::new("rack"),
        item: Folded::new(item),
        missing: false,
        ended: false,
        unread,
    };
    assert_eq!(
        opens(&unplaced),
        [
            include("beam", true),
            include("handset", false),
            include("lantern", true),
        ]
    );
    let bench = &loaded.topics["bench"].data;
    assert_eq!(bench.unclaimed, ["Beam"]);
    assert!(!held(&bench.unclaimed[0]));
    let desk_topic = &loaded.topics["desk"].data;
    assert_eq!(desk_topic.unclaimed, ["handset", "ATLAS"]);
    let mut read: BTreeSet<(DocumentId, String)> = BTreeSet::new();
    for (name, paths) in desk_topic.claims.iter().filter(|(name, _)| held(name)) {
        read.extend(paths.iter().map(|path| (now(name.clone()), path.clone())));
    }
    let unclaimed = desk_topic.unclaimed.iter().filter(|name| held(name));
    read.extend(unclaimed.map(|name| (now(name.clone()), String::new())));
    assert_eq!(read.len(), 2);
    assert_eq!(claimed(&planned), read);
    assert_eq!(home_of(&loaded, "Atlas/scale"), "Atlas");
    assert!(!loaded.has_topic("Atlas"));
    let scale = fact(&of(&planned, Kind::Fact, "Atlas/scale").versions[0]);
    assert_eq!(scale.topic, topic_id("atlas"));
    for slug in ["phone/battery", "PHONE/charger", "Phone/case"] {
        let planned = fact(&of(&planned, Kind::Fact, slug).versions[0]);
        assert_eq!(planned.topic, now(home_of(&loaded, slug)), "{slug}");
    }
    assert_eq!(home_of(&loaded, "phone/battery"), "handset");
    assert_eq!(home_of(&loaded, "Phone/case"), "Phone");
    for slug in [
        "2026-09-01-spelled",
        "2026-09-02-shouted",
        "2026-09-03-lower",
    ] {
        let read = now(touched_by(&loaded, slug));
        assert_eq!(touching(&planned, slug), read, "{slug}");
    }
    assert_eq!(touched_by(&loaded, "2026-09-01-spelled"), "handset");
}

#[test]
fn a_claim_on_a_topic_renamed_to_its_own_slug_in_another_case_stands_as_the_store_reads_it() {
    let old = OldStore::new();
    let claims: [(&str, &[&str]); 1] = [("lantern", &["~/projects/lantern"])];
    old.machine_topic("desk", &claims, &["lantern"], desk(DAY_1));
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.renamed(&lantern, "Lantern", desk(DAY_2));
    let store = read(&old);
    let named = answers("[documents.\"topic/Lantern\"]\nname = \"lamp\"\n");
    let planned = plan(&store, &named);

    assert_eq!(planned.questions, []);
    let loaded = read_by_the_store(&old);
    let desk_topic = &loaded.topics["desk"].data;
    assert_eq!(desk_topic.unclaimed, [""; 0]);
    let held = |name: &String| loaded.lookup()(name).is_some();
    let mut read: BTreeSet<(DocumentId, String)> = BTreeSet::new();
    for (name, paths) in desk_topic.claims.iter().filter(|(name, _)| held(name)) {
        assert_eq!(name, "Lantern");
        read.extend(paths.iter().map(|path| (topic_id("lantern"), path.clone())));
    }
    assert_eq!(read.len(), 1);
    assert_eq!(claimed(&planned), read);
}

#[test]
fn an_answer_spelled_as_a_topic_is_that_topic_whatever_was_renamed() {
    let old = with_desk();
    let shouted = old.topic("PHONE", "A phone", &[], desk(DAY_1));
    old.topic("Phone", "Another phone", &[], desk(DAY_1));
    old.entry("2026-09/2026-09-04-call", "Called", &["calls"], desk(DAY_1));
    old.renamed(&shouted, "handset", desk(DAY_3));
    let store = read(&old);
    let filed = |answer: &str| {
        let file = format!("{PHONES}[tags]\ncalls = \"{answer}\"\n");
        let planned = plan(&store, &answers(&file));
        assert_eq!(planned.questions, []);
        let entry = of(&planned, Kind::Entry, "2026-09/2026-09-04-call");
        match &entry.versions[0].record.as_ref().unwrap().content {
            Content::Entry(entry) => entry.topics.clone(),
            other => panic!("not an entry: {other:?}"),
        }
    };

    assert_eq!(filed("Phone"), [topic_id("Phone")]);
    assert_eq!(filed("PHONE"), [topic_id("PHONE")]);
    assert_eq!(filed("handset"), [topic_id("PHONE")]);
    assert_eq!(filed("phone"), [topic_id("PHONE")]);
}

#[test]
fn of_two_renamed_topics_that_differ_in_case_a_name_is_asked_about() {
    let old = OldStore::new();
    let claims: [(&str, &[&str]); 1] = [("phone", &["~/projects/phone"])];
    old.machine_topic("desk", &claims, &[], desk(DAY_1));
    let shouted = old.topic("PHONE", "A phone", &[], desk(DAY_1));
    let spelled = old.topic("Phone", "Another phone", &[], desk(DAY_1));
    old.topic("shelf", "A shelf", &["PHONE"], desk(DAY_1));
    old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    old.fact("PHONE/charger", "Five volts", &[], desk(DAY_1));
    let entry = old.entry("2026-09/2026-09-04-call", "Called", &[], desk(DAY_1));
    followup_touching(&old, &entry, "2026-09-01-spelled", "Phone");
    old.renamed(&shouted, "handset", desk(DAY_3));
    old.renamed(&spelled, "mobile", desk(DAY_3));
    let store = read(&old);
    let file = "[documents.\"topic/handset\"]\nformer_names = { PHONE = \"\" }\n\
        [documents.\"topic/mobile\"]\nformer_names = { Phone = \"\" }\n\
        [tags]\ndesk-work = \"desk\"\n";
    let planned = plan(&store, &answers(file));

    let loaded = read_by_the_store(&old);
    assert_eq!(home_of(&loaded, "phone/battery"), "mobile");
    assert_eq!(home_of(&loaded, "PHONE/charger"), "mobile");
    let both = || vec!["PHONE".to_owned(), "Phone".to_owned()];
    let of_a_fact = |slug: &str, name: &str| Open::TopicCase {
        document: key(Kind::Fact, slug),
        through: Through::FactTopic,
        name: name.to_owned(),
        topics: both(),
    };
    assert_eq!(
        opens(&planned),
        [
            of_a_fact("PHONE/charger", "PHONE"),
            of_a_fact("phone/battery", "phone"),
            Open::TopicCase {
                document: key(Kind::Followup, "2026-09-01-spelled"),
                through: Through::Touching,
                name: "Phone".to_owned(),
                topics: both(),
            },
        ]
    );
    let of_no_key = |name: &str| Blocked::TopicCase {
        name: name.to_owned(),
        topics: both(),
    };
    assert_eq!(blocks(&planned), [of_no_key("PHONE"), of_no_key("phone")]);

    let answered = format!(
        "{file}[documents.\"fact/PHONE/charger\"]\ntopics = [\"PHONE\"]\n\
         [documents.\"fact/phone/battery\"]\ntopics = [\"mobile\"]\n\
         [documents.\"followup/2026-09-01-spelled\"]\ntouching = \"Phone\"\n"
    );
    let planned = plan(&store, &answers(&answered));

    assert_eq!(opens(&planned), []);
    let topic_of = |slug: &str| fact(&of(&planned, Kind::Fact, slug).versions[0]).topic;
    assert_eq!(topic_of("PHONE/charger"), topic_id("PHONE"));
    assert_eq!(topic_of("phone/battery"), topic_id("Phone"));
    assert_eq!(touching(&planned, "2026-09-01-spelled"), topic_id("Phone"));
}

#[test]
fn a_name_in_another_case_than_the_one_topic_that_has_it_names_none_where_the_store_reads_so() {
    let old = OldStore::new();
    let claims: [(&str, &[&str]); 1] = [("Atlas", &["~/projects/atlas"])];
    old.machine_topic("desk", &claims, &["ATLAS"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("shelf", "A shelf", &["Atlas"], desk(DAY_1));
    old.fact("Atlas/scale", "One to ten", &[], desk(DAY_1));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let loaded = read_by_the_store(&old);
    assert!(loaded.lookup()("Atlas").is_none());
    assert_eq!(home_of(&loaded, "Atlas/scale"), "Atlas");
    assert_eq!(
        planned.questions,
        [
            Question::Open(Open::Include {
                topic: Folded::new("shelf"),
                item: Folded::new("atlas"),
                missing: true,
                ended: false,
                unread: false,
            }),
            Question::Open(Open::Claim {
                machine_topic: Folded::new("desk"),
                item: Folded::new("atlas"),
            }),
            Question::Open(Open::FactTopic {
                fact: key(Kind::Fact, "Atlas/scale"),
                topic: "Atlas".to_owned(),
            }),
        ]
    );
}

#[test]
fn a_tombstone_whose_one_parent_the_store_lacks_is_waiting_and_nothing_more() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.removed(&lantern, "Sold", desk(DAY_2));
    old.withhold(&lantern);
    let store = read(&old);
    let decisions = answers("[documents.\"topic/lantern\"]\nended = \"retired\"\n");
    let planned = plan(&store, &decisions);

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::Waiting {
            document: key(Kind::Topic, "lantern"),
            missing: vec![lantern.id.to_string()],
        })]
    );
    assert_eq!(
        of(&planned, Kind::Topic, "lantern").versions[0].record,
        None
    );
}

#[test]
fn a_version_after_an_ending_keeps_it_on_a_followup_and_is_live_on_the_rest() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let entry = old.entry("2026-09/2026-09-04-lamp", "Drove it", &[], desk(DAY_1));
    let port = old.followup("2026-09-01-port", &entry, "Port", &["relay"], desk(DAY_1));
    let wire = old.followup("2026-09-02-wire", &entry, "Wire", &["relay"], desk(DAY_1));
    let gone = old.removed(
        &old.topic("torch", "A torch", &[], desk(DAY_1)),
        "Sold",
        desk(DAY_2),
    );
    old.saved(&gone, "A torch again", "\n", desk(DAY_3));
    for first in [&pin, &entry, &wire] {
        let gone = old.removed(first, "Gone", desk(DAY_2));
        old.changed(&gone, "save", &[("summary", scalar("Back"))], desk(DAY_3));
    }
    let done = old.done(&port, "Ported", desk(DAY_2));
    let later = old.changed(
        &done,
        "save",
        &[("summary", scalar("Port it"))],
        desk(DAY_3),
    );
    old.changed(&later, "save", &[("state", scalar("open"))], desk(DAY_4));
    let store = read(&old);
    let decisions = answers(
        "[tags]\nrelay = \"lantern\"\n\
         [documents.\"topic/torch\"]\nended = \"retired\"\n\
         [documents.\"fact/lantern/relay-pin\"]\nended = \"false\"\n\
         [documents.\"entry/2026-09/2026-09-04-lamp\"]\nended = \"removed\"\n\
         [documents.\"followup/2026-09-02-wire\"]\nended = \"dropped\"\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    let endings = |kind: Kind, origin: &str| -> Vec<Option<Reason>> {
        let versions = &of(&planned, kind, origin).versions;
        let reason = |version: &Version| ending(version).map(|ending| ending.reason);
        versions.iter().map(reason).collect()
    };
    assert_eq!(
        endings(Kind::Topic, "torch"),
        [None, Some(Reason::Retired), None]
    );
    assert_eq!(
        endings(Kind::Fact, "lantern/relay-pin"),
        [None, Some(Reason::False), None]
    );
    assert_eq!(
        endings(Kind::Entry, "2026-09/2026-09-04-lamp"),
        [None, Some(Reason::Removed), None]
    );
    assert_eq!(
        endings(Kind::Followup, "2026-09-02-wire"),
        [None, Some(Reason::Dropped), None]
    );
    let done = Some(Reason::Done);
    assert_eq!(
        endings(Kind::Followup, "2026-09-01-port"),
        [None, done.clone(), done, None]
    );
    assert_eq!(lantern.slug.path(), "lantern");
}

#[test]
fn a_former_name_the_new_store_refuses_has_an_answer_of_its_own() {
    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let pin = old.fact("lantern/Relay.Pin", "Pin 5", &[], desk(DAY_1));
    old.renamed(&pin, "lantern/relay", desk(DAY_2));
    old.renamed(&lantern, "lamp", desk(DAY_2));
    old.fact("lamp/relay", "Pin 6", &[], desk(DAY_3));
    let store = read(&old);
    let relay = key(Kind::Fact, "lantern/relay");
    let former = Open::FormerName {
        document: relay.clone(),
        name: "Relay.Pin".to_owned(),
        why: "holds an uppercase letter".to_owned(),
    };
    let shared = |slug: &str| Open::Name {
        document: key(Kind::Fact, slug),
        name: "relay".to_owned(),
        why: "is held by several documents".to_owned(),
    };

    let asked = plan(&store, &Decisions::default());
    assert_eq!(
        opens(&asked),
        [
            shared("lamp/relay"),
            shared("lantern/relay"),
            former.clone()
        ]
    );

    let head = "[documents.\"fact/lantern/relay\"]\nname = \"relay-pin\"\n";
    let renamed = plan(&store, &answers(&format!("{CONVERSION}{head}")));
    assert_eq!(opens(&renamed), [former]);

    let newer = format!("{CONVERSION}[documents.\"fact/lamp/relay\"]\nname = \"relay-two\"\n");
    let table = "[documents.\"fact/lantern/relay\"]\nformer_names = ";
    let decisions = answers(&format!(
        "{newer}{table}{{ \"Relay.Pin\" = \"relay-pin\" }}\n"
    ));
    let answered = plan(&store, &decisions);
    assert_eq!(answered.questions, []);
    assert_eq!(decisions.unused(), []);
    let versions = &of(&answered, Kind::Fact, "lantern/Relay.Pin").versions;
    assert_eq!(fact(&versions[0]).name, names(&["relay-pin"])[0]);
    assert_eq!(fact(&versions[1]).name, names(&["relay"])[0]);
    assert_eq!(fact(&versions[1]).former_names, names(&["relay-pin"]));

    let left_out = answers(&format!("{newer}{table}{{ \"Relay.Pin\" = \"\" }}\n"));
    let without = plan(&store, &left_out);
    assert_eq!(without.questions, []);
    let versions = &of(&without, Kind::Fact, "lantern/Relay.Pin").versions;
    assert_eq!(fact(&versions[0]).name, names(&["relay"])[0]);
    assert_eq!(fact(&versions[1]).name, names(&["relay"])[0]);
    assert_eq!(fact(&versions[1]).former_names, []);

    let unfit = answers(&format!(
        "{newer}{table}{{ \"Relay.Pin\" = \"Relay Pin\" }}\n"
    ));
    assert_eq!(
        opens(&plan(&store, &unfit)),
        [Open::Answer {
            key: Key::document(&relay, DocumentField::FormerName("Relay.Pin".to_owned())),
            why: "Relay Pin holds an uppercase letter".to_owned(),
        }]
    );
}

#[test]
fn a_moved_version_keeps_the_parents_it_names_beside_its_tombstone() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let saved = old.saved(&first, "A brighter lamp", "\n", desk(DAY_2));
    let (_, moved) = old.renamed_with(&saved, "lamp", desk(DAY_3), |block| {
        block.parents.push(first.id.clone());
    });
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let renamed = &of(&planned, Kind::Topic, "lantern").versions[2];
    assert_eq!(renamed.key, moved.id.to_string());
    assert_eq!(
        renamed.parents,
        [saved.id.to_string(), first.id.to_string()]
    );
    assert_eq!(renamed.outside, [""; 0]);
}

#[test]
fn a_version_that_joins_a_rename_and_an_edit_follows_the_one_version_the_rename_became() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (tombstone, moved) = old.renamed(&first, "lamp", desk(DAY_2));
    let edited = old.saved(&first, "A brighter lamp", "\n", desk(DAY_3));
    let both = composed(
        "lantern",
        desk(DAY_4),
        "resolve",
        &[&tombstone, &edited],
        edited.fields.clone(),
        "\n",
    );
    old.write(&both);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let versions = &of(&planned, Kind::Topic, "lantern").versions;
    let keys: Vec<&str> = versions
        .iter()
        .map(|version| version.key.as_str())
        .collect();
    let (moved, edited) = (moved.id.to_string(), edited.id.to_string());
    assert_eq!(keys.len(), 4);
    assert_eq!(keys[0], first.id.to_string());
    assert_eq!(keys[3], both.id.to_string());
    assert_eq!(versions[3].parents, [moved.clone(), edited]);
    assert_eq!(versions[3].outside, [""; 0]);
    let renamed = versions.iter().find(|version| version.key == moved);
    assert_eq!(topic(renamed.unwrap()).name, names(&["lamp"])[0]);
    assert_eq!(topic(&versions[3]).name, names(&["lantern"])[0]);
    assert_eq!(topic(&versions[3]).former_names, names(&["lamp"]));
    assert_eq!(topic(&versions[3]).summary, "A brighter lamp");
}

#[test]
fn a_document_that_goes_on_under_both_slugs_of_a_rename_blocks() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (tombstone, moved) = old.renamed(&first, "lamp", desk(DAY_2));
    let edited = old.saved(&first, "A brighter lamp", "\n", desk(DAY_3));
    let both = composed(
        "lantern",
        desk(DAY_4),
        "resolve",
        &[&tombstone, &edited],
        edited.fields.clone(),
        "\n",
    );
    old.write(&both);
    old.saved(&moved, "A lamp on a desk", "\n", desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::Value {
            document: key(Kind::Topic, "lantern"),
            version: both.id.to_string(),
            field: "parents".to_owned(),
            value: tombstone.id.to_string(),
            why: "is the tombstone of a rename, and the document goes on under both slugs"
                .to_owned(),
        })]
    );
}

#[test]
fn a_parent_outside_the_document_is_blocked_whoever_else_asked() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let store = read(&old);
    let lantern = (store.documents.iter())
        .find(|document| document.slug == "lantern")
        .unwrap();
    let version = |key: &str, outside: &[&str]| Version {
        key: key.to_owned(),
        parents: Vec::new(),
        outside: outside.iter().map(|id| (*id).to_owned()).collect(),
        stamp: Stamp::parse(DAY_1).ok(),
        machine: None,
        change: "save".to_owned(),
        record: None,
        body: String::new(),
        links: Links::default(),
        origin: Origin::Converted(Source {
            document: lantern,
            version: &lantern.versions[0],
            stone: None,
        }),
    };
    let document = Document {
        id: topic_id("lantern"),
        kind: KindOf::Topic,
        versions: vec![version("aa11", &[]), version("bb22", &["cc33", "dd44"])],
    };
    let questions = Questions::default();
    outside_unasked(std::slice::from_ref(&document), &questions);
    assert_eq!(
        questions.asked(),
        [Question::Blocked(Blocked::Waiting {
            document: key(Kind::Topic, "lantern"),
            missing: vec!["cc33".to_owned(), "dd44".to_owned()],
        })]
    );

    let asked = Questions::default();
    asked.ask(Blocked::Skipped {
        path: "notes.txt".to_owned(),
    });
    outside_unasked(&[document], &asked);
    assert_eq!(asked.asked().len(), 1);
}

mod authored;
mod checks;
mod claims;
mod entries;
mod followups;
mod joined;
mod links;
mod machines;
mod placeholders;
mod rechecks;
mod whole;

#[test]
fn the_topics_a_file_lists_for_a_fact_under_a_live_topic_or_for_a_topic_are_unused() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(
        "[documents.\"fact/lantern/relay-pin\"]\ntopics = [\"atlas\"]\n\
         [documents.\"topic/lantern\"]\ntopics = [\"atlas\"]\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(
        planned.unused,
        [
            Key::document(&key(Kind::Fact, "lantern/relay-pin"), DocumentField::Topics),
            Key::document(&key(Kind::Topic, "lantern"), DocumentField::Topics),
        ]
    );
    let pin = fact(&of(&planned, Kind::Fact, "lantern/relay-pin").versions[0]);
    assert_eq!(pin.topic, topic_id("lantern"));
}

#[test]
fn every_key_a_topic_is_held_open_under_is_a_reference_of_its_kind() {
    for asked in Asked::ALL {
        assert!(asked.is_a_reference(), "{asked:?}");
        let held = asked != Asked::ENTRY_TOPICS;
        assert_eq!(holds_open(asked), held, "{asked:?}");
    }
}

#[test]
fn each_operation_of_the_store_becomes_a_change_on_purpose_and_any_other_word_blocks() {
    let becomes = [
        ("new", "new"),
        ("save", "save"),
        ("resolve", "resolve"),
        ("verify", "verify"),
        ("claim", "claim"),
        ("unclaim", "unclaim"),
        ("done", "done"),
        ("drop", "dropped"),
        ("recheck", "trigger"),
        ("rename", "rename"),
        ("tombstone", "tombstone"),
        ("migrate", "migrate"),
    ];
    for (operation, change) in becomes {
        assert!(Operation::is_known(operation), "{operation}");
        assert_eq!(change_of(operation), Some(change), "{operation}");
    }
    assert_eq!(becomes.len(), OPERATIONS.len() + 1);
    assert_eq!(change_of("import"), None);

    let old = with_desk();
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let imported = old.changed(&lantern, "import", &[], desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());
    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::Value {
            document: key(Kind::Topic, "lantern"),
            version: imported.id.to_string(),
            field: "operation".to_owned(),
            value: "import".to_owned(),
            why: "is no operation a command of the store writes".to_owned(),
        })]
    );
}
