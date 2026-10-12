use super::parse::Refused;
use super::{Decisions, EntryAnswer, FormerAnswer, TagAnswer, TouchingAnswer, Unasked};
use crate::fixture::{LAMP_DRIVER, entry_key as entry, fact_key as fact, followup_key as followup};
use crate::key::{ConversionKey, DocumentField, DocumentKey, Edges, Folded, Key, Placement};
use crate::old::Kind;
use crate::question::{Open, Through, reasons};

fn said(decisions: &Decisions, open: Open) -> Result<&str, Open> {
    let read = decisions.answer(&open).and_then(|answer| answer.text());
    read.ok_or(open)
}

fn held(decisions: &Decisions, open: Open) -> Result<(), Open> {
    decisions.answer(&open).map(|_| ()).ok_or(open)
}

fn placed(decisions: &Decisions, open: Open) -> Result<Placement, Open> {
    let placed = decisions
        .answer(&open)
        .and_then(|answer| answer.placement());
    placed.ok_or(open)
}

fn one_topic(decisions: &Decisions, open: Open) -> Result<&str, Open> {
    decisions.answer(&open).ok_or(open)?.one_topic()
}

fn one_reason<'d>(decisions: &'d Decisions, open: Open, allowed: &[&str]) -> Result<&'d str, Open> {
    decisions.answer(&open).ok_or(open)?.one_of(allowed)
}

fn conversion(decisions: &Decisions, key: ConversionKey) -> Option<&str> {
    let read = decisions.unasked(Unasked::Conversion(key)).pop();
    read.and_then(|answer| answer.text())
}

fn own_tag<'d>(
    decisions: &'d Decisions,
    carrier: &DocumentKey,
    tag: &str,
) -> Option<(Key, TagAnswer<'d>)> {
    let own = decisions.unasked(Unasked::Tag(carrier, tag)).pop()?;
    Some((own.key().clone(), TagAnswer::of(own.text()?)))
}

fn tag<'d>(
    decisions: &'d Decisions,
    carrier: &DocumentKey,
    tag: &str,
) -> Result<TagAnswer<'d>, Open> {
    let open = Open::tag(tag, std::slice::from_ref(carrier));
    said(decisions, open).map(TagAnswer::of)
}

fn ended_by<'d>(decisions: &'d Decisions, document: &DocumentKey) -> Option<&'d DocumentKey> {
    let by = decisions.unasked(Unasked::EndedBy(document)).pop();
    by.and_then(|by| by.document())
}

fn topics_override<'d>(decisions: &'d Decisions, document: &DocumentKey) -> Option<&'d [String]> {
    let listed = decisions.unasked(Unasked::Topics(document)).pop();
    listed.and_then(|listed| listed.topics())
}

fn listed<'d>(decisions: &'d Decisions, topic: &str, placement: Placement) -> Vec<&'d str> {
    let listed = decisions.unasked(Unasked::Listed(topic, placement));
    let items = listed.iter().filter_map(|answer| answer.key().item());
    items.map(Folded::as_str).collect()
}

fn edges<'d>(decisions: &'d Decisions, topic: &str) -> Edges<&'d str> {
    Edges {
        part_of: listed(decisions, topic, Placement::PartOf),
        uses: listed(decisions, topic, Placement::Uses),
    }
}

fn drops<'d>(decisions: &'d Decisions, topic: &str) -> Vec<&'d str> {
    listed(decisions, topic, Placement::Drop)
}

fn include(topic: &str, item: &str, missing: bool) -> Open {
    Open::Include {
        topic: Folded::new(topic),
        item: Folded::new(item),
        missing,
        ended: false,
        unread: false,
    }
}

fn topic_summary(topic: &str) -> Open {
    let topic = Folded::new(topic);
    Open::TopicSummary { topic }
}

fn ended_topic(holder: &DocumentKey, through: Through, ended: &str) -> Open {
    Open::EndedTopic {
        document: holder.clone(),
        through,
        topic: ended.to_owned(),
    }
}

fn machine(name: &str, versions: usize) -> Open {
    let name = name.to_owned();
    Open::Machine { name, versions }
}

fn machine_among(name: &str, topics: &[String]) -> Open {
    let (name, topics) = (name.to_owned(), topics.to_vec());
    Open::MachineTopics { name, topics }
}

fn claim(machine_topic: &str, item: &str) -> Open {
    let (machine_topic, item) = (Folded::new(machine_topic), Folded::new(item));
    Open::Claim {
        machine_topic,
        item,
    }
}

fn fact_topic(fact: &DocumentKey, topic: &str) -> Open {
    let (fact, topic) = (fact.clone(), topic.to_owned());
    Open::FactTopic { fact, topic }
}

fn name(document: &DocumentKey, name: &str, why: &str) -> Open {
    let (document, name, why) = (document.clone(), name.to_owned(), why.to_owned());
    Open::Name {
        document,
        name,
        why,
    }
}

fn former_name(document: &DocumentKey, name: &str, why: &str) -> Open {
    let (document, name, why) = (document.clone(), name.to_owned(), why.to_owned());
    Open::FormerName {
        document,
        name,
        why,
    }
}

fn summary(document: &DocumentKey) -> Open {
    let document = document.clone();
    Open::Summary { document }
}

fn recheck_summary(fact: &DocumentKey, touching: &str) -> Open {
    let (fact, touching) = (fact.clone(), touching.to_owned());
    Open::RecheckSummary { fact, touching }
}

fn entry_of(followup: &DocumentKey, entry: &str) -> Open {
    let (followup, entry) = (followup.clone(), entry.to_owned());
    Open::Entry { followup, entry }
}

fn touching(document: &DocumentKey, topic: &str) -> Open {
    let (document, topic) = (document.clone(), topic.to_owned());
    Open::Touching { document, topic }
}

const EXAMPLE: &str = r#"
[conversion]
written = 2026-10-20T09:30:00+01:00
machine = "desk"

[tags]
relay = "lantern"
firmware = "lantern-firmware"
rust = ""

[topics.lantern-firmware]
summary = "The firmware of the lantern"
part_of = ["lantern"]
uses = []

[topics.lantern]
part_of = []
uses = ["atlas"]
drop = ["phone"]

[topics.desk]
unclaim = ["phone"]

[machines]
desk-old = "desk"

[documents."entry/2026-09/2026-09-04-lamp-driver"]
tags = { relay = "" }
topics = ["lantern"]
name = "lamp-driver"
summary = "One line"
ended = "merged"
ended_by = "fact/lantern/relay-pin"

[documents."fact/lantern/relay-pin"]
recheck_summary = "Look at the pin again"

[documents."followup/2026-09-04-port"]
entry = ""
touching = ""

[documents."followup/2026-09-05-wire"]
entry = "2026-09/2026-09-04-lamp-driver"
touching = "atlas"
"#;

fn read(text: &str) -> Decisions {
    Decisions::parse(text).unwrap_or_else(|refused| panic!("refused: {refused:?}"))
}

fn refused(text: &str) -> Vec<String> {
    let refused = Decisions::parse(text).err().expect("the file was read");
    refused.iter().map(Refused::to_string).collect()
}

fn unused(decisions: &Decisions) -> Vec<String> {
    decisions.unused().iter().map(Key::to_string).collect()
}

#[test]
fn the_example_answers_each_question() {
    let decisions = read(EXAMPLE);
    let lamp = entry(LAMP_DRIVER);
    let (port, wire) = (followup("2026-09-04-port"), followup("2026-09-05-wire"));

    assert_eq!(
        conversion(&decisions, ConversionKey::Written),
        Some("2026-10-20T09:30:00+01:00")
    );
    assert_eq!(conversion(&decisions, ConversionKey::Machine), Some("desk"));
    assert_eq!(
        tag(&decisions, &port, "relay"),
        Ok(TagAnswer::Topic("lantern"))
    );
    assert_eq!(
        tag(&decisions, &port, "firmware"),
        Ok(TagAnswer::Topic("lantern-firmware"))
    );
    assert_eq!(tag(&decisions, &port, "rust"), Ok(TagAnswer::Dropped));
    let asked = Open::tag("relay", std::slice::from_ref(&port));
    let read = decisions.answer(&asked).map(|answer| answer.key().clone());
    assert_eq!(read, Some(Key::tag("relay")));
    assert_eq!(
        own_tag(&decisions, &lamp, "Relay"),
        Some((
            Key::document(&lamp, DocumentField::Tag(Folded::new("relay"))),
            TagAnswer::Dropped
        ))
    );
    assert_eq!(own_tag(&decisions, &port, "relay"), None);
    assert_eq!(
        tag(&decisions, &lamp, "ghost"),
        Err(Open::tag("ghost", std::slice::from_ref(&lamp)))
    );

    let names: Vec<&str> = decisions.topics().collect();
    assert_eq!(names, ["desk", "lantern", "lantern-firmware"]);
    assert_eq!(
        said(&decisions, topic_summary("lantern-firmware")),
        Ok("The firmware of the lantern")
    );
    assert_eq!(
        edges(&decisions, "lantern-firmware"),
        Edges {
            part_of: vec!["lantern"],
            uses: vec![],
        }
    );
    assert_eq!(
        placed(&decisions, include("lantern", "atlas", false)),
        Ok(Placement::Uses)
    );
    assert_eq!(
        placed(&decisions, include("lantern", "phone", true)),
        Ok(Placement::Drop)
    );

    assert_eq!(said(&decisions, machine("desk-old", 3)), Ok("desk"));
    assert_eq!(
        said(&decisions, machine_among("desk-old", &["atlas".to_owned()])),
        Ok("desk")
    );

    assert_eq!(
        topics_override(&decisions, &lamp),
        Some(&["lantern".to_owned()][..])
    );
    assert_eq!(
        said(&decisions, name(&lamp, "Lamp", "is refused")),
        Ok("lamp-driver")
    );
    assert_eq!(said(&decisions, summary(&lamp)), Ok("One line"));
    let by = fact("lantern/relay-pin");
    let merged = one_reason(
        &decisions,
        Open::ended(&lamp, false, "a note"),
        &reasons(Kind::Entry, false),
    );
    assert_eq!(merged, Ok("merged"));
    assert_eq!(ended_by(&decisions, &lamp), Some(&by));
    assert_eq!(
        said(
            &decisions,
            recheck_summary(&fact("lantern/relay-pin"), "lantern")
        ),
        Ok("Look at the pin again")
    );
    assert_eq!(held(&decisions, claim("desk", "Phone")), Ok(()));
    assert_eq!(
        said(&decisions, entry_of(&port, "gone")).map(EntryAnswer::of),
        Ok(EntryAnswer::NoEntry)
    );
    assert_eq!(
        said(&decisions, entry_of(&wire, "gone")).map(EntryAnswer::of),
        Ok(EntryAnswer::Entry(LAMP_DRIVER))
    );
    assert_eq!(
        said(&decisions, touching(&port, "gone")).map(TouchingAnswer::of),
        Ok(TouchingAnswer::NoTrigger)
    );
    assert_eq!(
        said(&decisions, touching(&wire, "gone")).map(TouchingAnswer::of),
        Ok(TouchingAnswer::Topic("atlas"))
    );
    assert!(unused(&decisions).is_empty(), "{:?}", unused(&decisions));
}

#[test]
fn an_empty_file_hands_each_question_back() {
    let decisions = read("");
    let lamp = entry(LAMP_DRIVER);

    assert_eq!(conversion(&decisions, ConversionKey::Written), None);
    assert_eq!(conversion(&decisions, ConversionKey::Machine), None);
    assert_eq!(
        tag(&decisions, &lamp, "Relay"),
        Err(Open::tag("relay", std::slice::from_ref(&lamp)))
    );
    assert_eq!(
        placed(&decisions, include("Lantern", "Atlas", true)),
        Err(Open::Include {
            topic: Folded::new("lantern"),
            item: Folded::new("atlas"),
            missing: true,
            ended: false,
            unread: false,
        })
    );
    assert_eq!(
        said(&decisions, topic_summary("Atlas")),
        Err(Open::TopicSummary {
            topic: Folded::new("atlas"),
        })
    );
    assert_eq!(edges(&decisions, "lantern"), Edges::default());
    assert_eq!(
        said(&decisions, machine("desk-old", 3)),
        Err(Open::Machine {
            name: "desk-old".to_owned(),
            versions: 3,
        })
    );
    assert_eq!(
        said(
            &decisions,
            machine_among("desk", &["atlas".to_owned(), "desk".to_owned()])
        ),
        Err(Open::MachineTopics {
            name: "desk".to_owned(),
            topics: vec!["atlas".to_owned(), "desk".to_owned()],
        })
    );
    assert_eq!(
        held(&decisions, claim("Desk", "Phone")),
        Err(Open::Claim {
            machine_topic: Folded::new("desk"),
            item: Folded::new("phone"),
        })
    );
    assert!(decisions.unused().is_empty());
}

#[test]
fn an_empty_file_hands_each_question_of_a_document_back() {
    let decisions = read("");
    let lamp = entry(LAMP_DRIVER);
    let port = followup("2026-09-04-port");
    let relay_pin = fact("lantern/relay-pin");

    assert_eq!(
        said(&decisions, Open::ended(&relay_pin, true, "a note")),
        Err(Open::Ended {
            document: relay_pin.clone(),
            idea: true,
            note: "a note".to_owned(),
            already: None,
        })
    );
    assert_eq!(
        said(&decisions, Open::ended(&lamp, true, "")),
        Err(Open::Ended {
            document: lamp.clone(),
            idea: false,
            note: String::new(),
            already: None,
        })
    );
    assert_eq!(topics_override(&decisions, &lamp), None);
    assert_eq!(
        decisions.unfiled(&port, "Port the driver"),
        (Open::NoTopic {
            document: port.clone(),
            summary: "Port the driver".to_owned(),
        })
    );
    assert_eq!(
        one_topic(&decisions, fact_topic(&relay_pin, "lantern")),
        Err(Open::FactTopic {
            fact: relay_pin.clone(),
            topic: "lantern".to_owned(),
        })
    );
    assert_eq!(
        said(&decisions, name(&lamp, "Lamp", "holds an uppercase letter")),
        Err(Open::Name {
            document: lamp.clone(),
            name: "Lamp".to_owned(),
            why: "holds an uppercase letter".to_owned(),
        })
    );
    assert_eq!(
        said(&decisions, summary(&lamp)),
        Err(Open::Summary {
            document: lamp.clone(),
        })
    );
    assert_eq!(
        said(&decisions, recheck_summary(&relay_pin, "lantern")),
        Err(Open::RecheckSummary {
            fact: relay_pin,
            touching: "lantern".to_owned(),
        })
    );
    assert_eq!(
        said(&decisions, entry_of(&port, "2026-09/gone")).map(EntryAnswer::of),
        Err(Open::Entry {
            followup: port.clone(),
            entry: "2026-09/gone".to_owned(),
        })
    );
    assert_eq!(
        said(&decisions, touching(&port, "gone")).map(TouchingAnswer::of),
        Err(Open::Touching {
            document: port,
            topic: "gone".to_owned(),
        })
    );
    assert!(decisions.unused().is_empty());
}

#[test]
fn tags_and_topic_names_are_found_in_any_ascii_case() {
    let decisions = read(
        "[tags]\nrelay = \"Lantern\"\n\
         [topics.Lantern]\nuses = [\"Atlas\"]\nunclaim = [\"PHONE\"]\n\
         [topics.Atlas]\nsummary = \"The atlas\"\n\
         [documents.\"entry/a\"]\ntags.pin = \"\"\n",
    );
    let a = entry("a");

    assert_eq!(
        tag(&decisions, &a, "Relay"),
        Ok(TagAnswer::Topic("Lantern"))
    );
    let own = own_tag(&decisions, &a, "PIN").map(|(_, answer)| answer);
    assert_eq!(own, Some(TagAnswer::Dropped));
    assert_eq!(
        placed(&decisions, include("lantern", "ATLAS", false)),
        Ok(Placement::Uses)
    );
    assert_eq!(held(&decisions, claim("LANTERN", "phone")), Ok(()));
    assert!(placed(&decisions, include("lantern", "phone", false)).is_err());
    assert_eq!(said(&decisions, topic_summary("ATLAS")), Ok("The atlas"));
    let names: Vec<&str> = decisions.topics().collect();
    assert_eq!(names, ["atlas", "lantern"]);
    assert_eq!(edges(&decisions, "LANTERN").uses, ["atlas"]);
    assert!(decisions.unused().is_empty());
}

#[test]
fn a_document_with_its_own_answer_leaves_the_tags_one_unused() {
    let decisions =
        read("[tags]\nrelay = \"lantern\"\n[documents.\"entry/a\"]\ntags.relay = \"\"\n");

    let own = own_tag(&decisions, &entry("a"), "relay").map(|(_, answer)| answer);
    assert_eq!(own, Some(TagAnswer::Dropped));
    assert_eq!(unused(&decisions), ["tags.relay"]);
}

#[test]
fn a_reason_the_kind_does_not_have_is_an_answer_that_names_nothing() {
    let decisions = read(
        "[documents.\"fact/lantern/relay-pin\"]\nended = \"built\"\n\
         [documents.\"entry/a\"]\nended = \"moved\"\n",
    );
    let relay_pin = fact("lantern/relay-pin");

    let reason = |document: &DocumentKey, idea: bool| {
        let allowed = reasons(document.kind, idea);
        one_reason(&decisions, Open::ended(document, idea, ""), &allowed)
    };
    assert_eq!(reason(&relay_pin, true), Ok("built"));
    assert_eq!(ended_by(&decisions, &relay_pin), None);
    assert_eq!(
        reason(&relay_pin, false),
        Err(Open::Answer {
            key: Key::document(&relay_pin, DocumentField::Ended),
            why: "built is not one of: false, moved, superseded".to_owned(),
        })
    );
    assert_eq!(
        reason(&entry("a"), true),
        Err(Open::Answer {
            key: Key::document(&entry("a"), DocumentField::Ended),
            why: "moved is not one of: removed, merged".to_owned(),
        })
    );
    assert!(unused(&decisions).is_empty(), "{:?}", unused(&decisions));
}

#[test]
fn a_list_of_the_wrong_length_is_an_answer_that_names_nothing() {
    let decisions = read(
        "[documents.\"followup/a\"]\ntopics = []\n\
         [documents.\"fact/lantrn/pin\"]\ntopics = [\"lantern\", \"atlas\"]\n\
         [documents.\"fact/lantrn/relay\"]\ntopics = [\"lantern\"]\n\
         [documents.\"followup/b\"]\ntopics = [\"lantern\", \"atlas\"]\n",
    );

    assert_eq!(
        decisions.unfiled(&followup("a"), "Port"),
        (Open::Answer {
            key: Key::document(&followup("a"), DocumentField::Topics),
            why: "names no topic, and a followup is filed under one".to_owned(),
        })
    );
    assert_eq!(
        one_topic(&decisions, fact_topic(&fact("lantrn/pin"), "lantrn")),
        Err(Open::Answer {
            key: Key::document(&fact("lantrn/pin"), DocumentField::Topics),
            why: "is not one name, and a fact is filed under one topic".to_owned(),
        })
    );
    assert_eq!(
        one_topic(&decisions, fact_topic(&fact("lantrn/relay"), "lantrn")),
        Ok("lantern")
    );
    assert_eq!(
        topics_override(&decisions, &followup("b")),
        Some(&["lantern".to_owned(), "atlas".to_owned()][..])
    );
    assert!(unused(&decisions).is_empty(), "{:?}", unused(&decisions));
}

#[test]
fn an_unknown_table_is_refused() {
    assert_eq!(
        refused("[colors]\nred = 1\n"),
        ["colors: is not a table of a decision file"]
    );
    assert_eq!(
        refused("colors = 1\n"),
        ["colors: is not a table of a decision file"]
    );
}

#[test]
fn an_unknown_key_is_refused() {
    assert_eq!(
        refused("[topics.lantern]\ncolor = \"red\"\n"),
        ["topics.lantern.color: is not a key of a topic's table"]
    );
    assert_eq!(
        refused("[documents.\"fact/lantern/x\"]\nreason = \"moved\"\n"),
        ["documents.\"fact/lantern/x\".reason: is not a key of a document's table"]
    );
    assert_eq!(
        refused(
            "[conversion]\nwritten = 2026-10-20T09:30:00Z\nmachine = \"desk\"\nby = \"desk\"\n"
        ),
        ["conversion.by: is not a key of the conversion's table"]
    );
}

#[test]
fn a_tag_key_that_is_not_its_own_lowercase_is_refused() {
    assert_eq!(
        refused("[tags]\nRelay = \"lantern\"\n"),
        ["tags.Relay: holds an uppercase letter, and a tag is keyed in lowercase"]
    );
    assert_eq!(
        refused("[documents.\"entry/a\"]\ntags = { Pin = \"\" }\n"),
        [
            "documents.\"entry/a\".tags.Pin: holds an uppercase letter, and a tag is keyed in lowercase"
        ]
    );
    read("[tags]\n\"\u{1c5}x\" = \"\"\n");
}

#[test]
fn a_wrong_type_is_refused() {
    assert_eq!(refused("[tags]\nrelay = 1\n"), ["tags.relay: is not text"]);
    assert_eq!(refused("tags = \"relay\"\n"), ["tags: is not a table"]);
    assert_eq!(
        refused("[topics]\nlantern = \"atlas\"\n"),
        ["topics.lantern: is not a table"]
    );
    assert_eq!(
        refused("[topics.lantern]\nuses = \"atlas\"\n"),
        ["topics.lantern.uses: is not a list of text"]
    );
    assert_eq!(
        refused("[topics.lantern]\nuses = [\"atlas\", 2]\n"),
        ["topics.lantern.uses: is not a list of text"]
    );
    assert_eq!(
        refused("[machines]\ndesk-old = [\"desk\"]\n"),
        ["machines.desk-old: is not text"]
    );
    assert_eq!(
        refused("[documents.\"entry/a\"]\ntags = [\"relay\"]\n"),
        ["documents.\"entry/a\".tags: is not a table"]
    );
    assert_eq!(
        refused("[documents.\"entry/a\"]\nended = true\n"),
        ["documents.\"entry/a\".ended: is not text"]
    );
}

#[test]
fn a_document_key_with_no_kind_is_refused() {
    let reason = "does not open with entry/, fact/, topic/ or followup/ before a slug";
    assert_eq!(
        refused("[documents.\"2026-09/2026-09-04-lamp-driver\"]\nname = \"lamp\"\n"),
        [format!(
            "documents.\"2026-09/2026-09-04-lamp-driver\": {reason}"
        )]
    );
    assert_eq!(
        refused("[documents.lantern]\nname = \"lamp\"\n"),
        [format!("documents.lantern: {reason}")]
    );
    assert_eq!(
        refused("[documents.\"topic/\"]\nname = \"lamp\"\n"),
        [format!("documents.\"topic/\": {reason}")]
    );
    assert_eq!(
        refused("[documents.\"entry/a\"]\nended_by = \"lantern/relay-pin\"\n"),
        [format!("documents.\"entry/a\".ended_by: {reason}")]
    );
}

#[test]
fn an_item_in_both_lists_of_edges_is_refused_in_any_case() {
    assert_eq!(
        refused("[topics.lantern]\npart_of = [\"atlas\"]\nuses = [\"Atlas\"]\n"),
        ["topics.lantern.uses: holds \"atlas\", which part_of holds too"]
    );
    assert_eq!(
        refused("[topics.lantern]\nuses = [\"atlas\"]\npart_of = [\"Atlas\"]\n"),
        ["topics.lantern.part_of: holds \"atlas\", which uses holds too"]
    );
}

#[test]
fn an_item_placed_in_a_list_of_edges_can_be_dropped_too() {
    let lantern = DocumentKey::new(Kind::Topic, "Lantern");
    let decisions = read("[topics.lantern]\ndrop = [\"Atlas\", \"phone\"]\nuses = [\"atlas\"]\n");

    assert_eq!(
        placed(&decisions, include("lantern", "atlas", false)),
        Ok(Placement::Uses)
    );
    assert_eq!(
        placed(&decisions, include("lantern", "phone", false)),
        Ok(Placement::Drop)
    );
    assert_eq!(unused(&decisions), ["topics.lantern.drop[\"atlas\"]"]);
    assert_eq!(
        held(
            &decisions,
            ended_topic(&lantern, Through::Includes, "ATLAS")
        ),
        Ok(())
    );
    assert_eq!(unused(&decisions), [] as [&str; 0]);

    let kept = read("[topics.lantern]\nuses = [\"atlas\"]\n");
    assert_eq!(
        held(&kept, ended_topic(&lantern, Through::Claim, "atlas")),
        Err(Open::EndedTopic {
            document: lantern,
            through: Through::Claim,
            topic: "atlas".to_owned(),
        })
    );
}

#[test]
fn what_a_table_drops_is_read_whole_for_a_topic_the_file_creates() {
    let decisions = read(
        "[topics.lantern-firmware]\nsummary = \"The firmware\"\nuses = [\"desk\"]\n\
         drop = [\"phone\", \"Atlas\"]\n[topics.lantern]\ndrop = [\"desk\"]\n",
    );

    assert_eq!(drops(&decisions, "Lantern-Firmware"), ["atlas", "phone"]);
    assert_eq!(
        unused(&decisions),
        [
            "topics.lantern.drop[\"desk\"]",
            "topics.lantern-firmware.summary",
            "topics.lantern-firmware.uses[\"desk\"]",
        ]
    );
    assert_eq!(drops(&decisions, "atlas"), [] as [&str; 0]);
}

#[test]
fn an_item_twice_in_a_list_or_empty_is_refused() {
    assert_eq!(
        refused("[topics.lantern]\nuses = [\"atlas\", \"Atlas\"]\n"),
        ["topics.lantern.uses: holds \"Atlas\" twice"]
    );
    assert_eq!(
        refused("[documents.\"entry/a\"]\ntopics = [\"atlas\", \" \"]\n"),
        ["documents.\"entry/a\".topics: holds an empty item"]
    );
    assert_eq!(
        refused("[topics.lantern]\ndrop = [\"\"]\n"),
        ["topics.lantern.drop: holds an empty item"]
    );
}

#[test]
fn two_tables_of_one_topic_in_two_cases_are_refused() {
    assert_eq!(
        refused("[topics.lantern]\nuses = []\n[topics.Lantern]\nuses = []\n"),
        ["topics.Lantern: is the topic of topics.lantern in another case"]
    );
}

#[test]
fn text_that_must_be_one_line_is_refused_empty_blank_or_broken() {
    let keys = [
        ("[topics.lantern]", "summary", "topics.lantern.summary"),
        ("[machines]", "desk-old", "machines.desk-old"),
        (
            "[documents.\"fact/a/b\"]",
            "summary",
            "documents.\"fact/a/b\".summary",
        ),
        (
            "[documents.\"fact/a/b\"]",
            "name",
            "documents.\"fact/a/b\".name",
        ),
        (
            "[documents.\"fact/a/b\"]",
            "ended",
            "documents.\"fact/a/b\".ended",
        ),
        (
            "[documents.\"fact/a/b\"]",
            "recheck_summary",
            "documents.\"fact/a/b\".recheck_summary",
        ),
    ];
    for (table, key, path) in keys {
        let file = |value: &str| format!("{table}\n{key} = {value}\n");
        assert_eq!(refused(&file("\"\"")), [format!("{path}: is empty")]);
        assert_eq!(refused(&file("\"  \"")), [format!("{path}: is empty")]);
        assert_eq!(
            refused(&file("\"one\\ntwo\"")),
            [format!("{path}: is not one line")]
        );
    }
    assert_eq!(
        refused("[conversion]\nwritten = 2026-10-20T09:30:00Z\nmachine = \"\"\n"),
        ["conversion.machine: is empty"]
    );
    assert_eq!(
        refused("[tags]\nrelay = \"a\\nb\"\n"),
        ["tags.relay: is not one line"]
    );
}

#[test]
fn the_placeholder_of_a_stub_answers_nothing_and_the_rest_of_the_file_applies() {
    let text = r#"
[conversion]
written = "?"
machine = "desk"

[tags]
relay = "?"
rust = ""

[topics.lantern]
summary = "?"
part_of = ["?", "?"]
uses = ["atlas", "?"]
unclaim = ["?"]

[machines]
desk-old = "?"
bench = "desk"

[documents."followup/a"]
tags.pin = "?"
topics = ["lantern", "?"]
name = "?"
former_names.b = "?"
summary = "?"
recheck_summary = "?"
entry = "?"
touching = "?"
ended = "?"
ended_by = "?"
"#;
    let decisions = read(text);
    let a = followup("a");

    let unanswered: Vec<String> = (decisions.unanswered().iter())
        .map(Key::to_string)
        .collect();
    assert_eq!(
        unanswered,
        [
            "conversion.written",
            "tags.relay",
            "topics.lantern.summary",
            "topics.lantern.part_of[\"\"]",
            "topics.lantern.uses[\"\"]",
            "topics.lantern.unclaim[\"\"]",
            "machines.desk-old",
            "documents.\"followup/a\".tags.pin",
            "documents.\"followup/a\".topics",
            "documents.\"followup/a\".name",
            "documents.\"followup/a\".former_names.b",
            "documents.\"followup/a\".summary",
            "documents.\"followup/a\".recheck_summary",
            "documents.\"followup/a\".entry",
            "documents.\"followup/a\".touching",
            "documents.\"followup/a\".ended",
            "documents.\"followup/a\".ended_by",
        ]
    );
    assert_eq!(
        unused(&decisions),
        [
            "tags.rust",
            "topics.lantern.uses[\"atlas\"]",
            "machines.bench",
            "documents.\"followup/a\"",
        ]
    );

    assert_eq!(conversion(&decisions, ConversionKey::Written), None);
    assert_eq!(conversion(&decisions, ConversionKey::Machine), Some("desk"));
    assert!(tag(&decisions, &a, "relay").is_err());
    assert!(own_tag(&decisions, &a, "pin").is_none());
    assert_eq!(tag(&decisions, &a, "rust"), Ok(TagAnswer::Dropped));
    assert!(said(&decisions, topic_summary("lantern")).is_err());
    assert!(placed(&decisions, include("lantern", "phone", false)).is_err());
    assert_eq!(
        placed(&decisions, include("lantern", "atlas", false)),
        Ok(Placement::Uses)
    );
    assert!(held(&decisions, claim("lantern", "phone")).is_err());
    assert!(said(&decisions, machine("desk-old", 1)).is_err());
    assert_eq!(said(&decisions, machine("bench", 1)), Ok("desk"));
    assert_eq!(topics_override(&decisions, &a), None);
    assert!(matches!(
        decisions.unfiled(&a, "Port it"),
        Open::NoTopic { .. }
    ));
    assert!(said(&decisions, name(&a, "A", "holds an uppercase letter")).is_err());
    assert!(said(&decisions, former_name(&a, "b", "is short")).is_err());
    assert!(said(&decisions, summary(&a)).is_err());
    assert!(said(&decisions, recheck_summary(&a, "atlas")).is_err());
    assert!(said(&decisions, entry_of(&a, "gone")).is_err());
    assert!(said(&decisions, touching(&a, "gone")).is_err());
    assert!(said(&decisions, Open::ended(&a, false, "")).is_err());
    assert_eq!(ended_by(&decisions, &a), None);
    assert_eq!(unused(&decisions), [""; 0]);
}

#[test]
fn a_conversion_needs_a_time_with_an_offset_and_a_machine() {
    let reason = "is not a date and time with seconds and an offset";
    assert_eq!(
        refused("[conversion]\nwritten = 2026-10-20T09:30:00\nmachine = \"desk\"\n"),
        [format!("conversion.written: {reason}")]
    );
    assert_eq!(
        refused("[conversion]\nwritten = 2026-10-20\nmachine = \"desk\"\n"),
        [format!("conversion.written: {reason}")]
    );
    assert_eq!(
        refused("[conversion]\nwritten = 2026-10-20T09:30+01:00\nmachine = \"desk\"\n"),
        [format!("conversion.written: {reason}")]
    );
    assert_eq!(
        refused("[conversion]\nwritten = \"2026-10-20T09:30:00+01:00\"\nmachine = \"desk\"\n"),
        ["conversion.written: is text: write the date and time without quotes"]
    );
    assert_eq!(
        refused("[conversion]\nmachine = \"desk\"\n"),
        ["conversion.written: is missing"]
    );
    assert_eq!(
        refused("[conversion]\n"),
        [
            "conversion.written: is missing",
            "conversion.machine: is missing"
        ]
    );
}

#[test]
fn every_refusal_of_a_file_is_reported() {
    let text = r#"
[tags]
Relay = "lantern"
pin = 3

[colors]
red = 1

[topics.lantern]
color = "red"
uses = "atlas"

[documents."lantern"]
name = "x"

[documents."fact/lantern/x"]
ended = 1
reason = "moved"
summary = ""
"#;
    assert_eq!(
        refused(text),
        [
            "tags.Relay: holds an uppercase letter, and a tag is keyed in lowercase",
            "tags.pin: is not text",
            "colors: is not a table of a decision file",
            "topics.lantern.color: is not a key of a topic's table",
            "topics.lantern.uses: is not a list of text",
            "documents.lantern: does not open with entry/, fact/, topic/ or followup/ before a slug",
            "documents.\"fact/lantern/x\".ended: is not text",
            "documents.\"fact/lantern/x\".reason: is not a key of a document's table",
            "documents.\"fact/lantern/x\".summary: is empty",
        ]
    );
}

#[test]
fn a_file_that_is_not_toml_is_refused_with_its_line() {
    let refused = (Decisions::parse("[tags]\nrelay = \n").err()).expect("the file was read");

    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].key, "");
    assert!(refused[0].reason.starts_with("line 2: "), "{refused:?}");
}

#[test]
fn a_table_or_a_key_written_twice_is_refused_with_its_name() {
    assert_eq!(
        refused("[tags]\nrelay = \"\"\n\n[machines]\n\n[tags]\npin = \"\"\n"),
        ["line 6: duplicate key at tags"]
    );
    assert_eq!(
        refused("[topics.lantern]\nuses = []\nuses = [\"atlas\"]\n"),
        ["line 3: duplicate key at uses"]
    );
}

#[test]
fn every_decision_is_unused_until_its_question_is_asked() {
    let decisions = read(EXAMPLE);

    assert_eq!(
        unused(&decisions),
        [
            "tags.firmware",
            "tags.relay",
            "tags.rust",
            "topics.desk.unclaim[\"phone\"]",
            "topics.lantern.uses[\"atlas\"]",
            "topics.lantern.drop[\"phone\"]",
            "topics.lantern-firmware.summary",
            "topics.lantern-firmware.part_of[\"lantern\"]",
            "machines.desk-old",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".tags.relay",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".topics",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".name",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".summary",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".ended",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".ended_by",
            "documents.\"fact/lantern/relay-pin\".recheck_summary",
            "documents.\"followup/2026-09-04-port\".entry",
            "documents.\"followup/2026-09-04-port\".touching",
            "documents.\"followup/2026-09-05-wire\".entry",
            "documents.\"followup/2026-09-05-wire\".touching",
        ]
    );
}

#[test]
fn a_question_asked_uses_the_one_answer_it_finds() {
    let decisions = read(EXAMPLE);
    let lamp = entry(LAMP_DRIVER);

    let _ = own_tag(&decisions, &lamp, "relay");
    let _ = tag(&decisions, &lamp, "rust");
    let _ = tag(&decisions, &lamp, "atlas");
    let _ = placed(&decisions, include("lantern", "atlas", false));
    let _ = placed(&decisions, include("lantern", "desk", false));
    let _ = edges(&decisions, "lantern-firmware");
    let _ = said(&decisions, summary(&lamp));
    let _ = said(&decisions, summary(&fact(LAMP_DRIVER)));
    let _ = said(&decisions, entry_of(&followup("2026-09-04-port"), "gone"));

    assert_eq!(
        unused(&decisions),
        [
            "tags.firmware",
            "tags.relay",
            "topics.desk.unclaim[\"phone\"]",
            "topics.lantern.drop[\"phone\"]",
            "topics.lantern-firmware.summary",
            "machines.desk-old",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".topics",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".name",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".ended",
            "documents.\"entry/2026-09/2026-09-04-lamp-driver\".ended_by",
            "documents.\"fact/lantern/relay-pin\".recheck_summary",
            "documents.\"followup/2026-09-04-port\".touching",
            "documents.\"followup/2026-09-05-wire\".entry",
            "documents.\"followup/2026-09-05-wire\".touching",
        ]
    );
}

#[test]
fn a_table_that_holds_nothing_is_unused_until_something_is_asked_under_it() {
    let text = "[topics.lantrn]\nuses = []\n\
                [topics.atlas]\n\
                [documents.\"entry/a\"]\ntags = {}\n\
                [documents.\"entry/b\"]\n";
    let decisions = read(text);

    assert_eq!(
        unused(&decisions),
        [
            "topics.atlas",
            "topics.lantrn",
            "documents.\"entry/a\"",
            "documents.\"entry/b\"",
        ]
    );

    let _ = said(&decisions, topic_summary("atlas"));
    let _ = said(&decisions, summary(&entry("b")));

    assert_eq!(
        unused(&decisions),
        ["topics.lantrn", "documents.\"entry/a\""]
    );
}

#[test]
fn a_key_that_is_no_bare_key_is_a_basic_string_in_its_path() {
    let text = "[tags]\n\"relay pin\" = \"\"\n[machines]\n\"desk.old\" = \"desk\"\n\
                [documents.\"fact/lantern/it's\"]\nname = \"a\"\n\
                [documents.\"fact/lantern/it's \\\"x\\\"\"]\nname = \"b\"\n";
    let decisions = read(text);

    assert_eq!(
        unused(&decisions),
        [
            "tags.\"relay pin\"",
            "machines.\"desk.old\"",
            "documents.\"fact/lantern/it's\".name",
            "documents.\"fact/lantern/it's \\\"x\\\"\".name",
        ]
    );
    for key in decisions.unused() {
        let again = format!(
            "[{}]\n{} = \"x\"\n",
            key.table(),
            key.line().unwrap_or_default()
        );
        let reread = read(&again);
        assert_eq!(reread.unused(), [key]);
    }
}

#[test]
fn the_time_of_a_conversion_is_handed_on_as_rfc_3339() {
    use worklog_next::domain::version::Stamp;

    for (typed, handed) in [
        ("2026-10-20T09:30:00+01:00", "2026-10-20T09:30:00+01:00"),
        ("2026-10-20 09:30:00.25z", "2026-10-20T09:30:00.25Z"),
        ("2026-10-20t09:30:00-07:30", "2026-10-20T09:30:00-07:30"),
    ] {
        let decisions = read(&format!(
            "[conversion]\nwritten = {typed}\nmachine = \"desk\"\n"
        ));
        let written = conversion(&decisions, ConversionKey::Written).expect("the file answers");
        assert_eq!(written, handed);
        assert!(Stamp::parse(written).is_ok(), "{handed}");
    }
}

#[test]
fn the_document_that_ended_another_is_unused_until_it_is_read() {
    let text = "[documents.\"entry/a\"]\nended_by = \"entry/b\"\n\
                [documents.\"entry/c\"]\nended = \"moved\"\nended_by = \"entry/b\"\n";
    let decisions = read(text);
    let allowed = reasons(Kind::Entry, false);

    assert_eq!(
        said(&decisions, Open::ended(&entry("a"), false, "")),
        Err(Open::ended(&entry("a"), false, ""))
    );
    assert!(matches!(
        one_reason(&decisions, Open::ended(&entry("c"), false, ""), &allowed),
        Err(Open::Answer { .. })
    ));
    assert_eq!(ended_by(&decisions, &entry("c")), Some(&entry("b")));
    assert_eq!(unused(&decisions), ["documents.\"entry/a\".ended_by"]);
}

#[test]
fn every_key_a_question_is_answered_under_is_read_when_all_are_asked_for() {
    let decisions = read("[topics.lantern]\nuses = [\"atlas\"]\ndrop = [\"Atlas\"]\n");
    let asked = include("lantern", "atlas", true);

    let all = decisions.answers(&asked);
    let placements: Vec<Option<Placement>> = all.iter().map(super::Answer::placement).collect();
    assert_eq!(placements, [Some(Placement::Uses), Some(Placement::Drop)]);
    assert_eq!(unused(&decisions), [""; 0]);
}

#[test]
fn a_list_of_topics_replaces_tags_on_an_entry_or_a_followup_alone() {
    let text = "[documents.\"entry/a\"]\ntopics = [\"lantern\"]\n\
                [documents.\"fact/lantern/pin\"]\ntopics = [\"lantern\"]\n\
                [documents.\"topic/lantern\"]\ntopics = [\"atlas\"]\n\
                [documents.\"followup/a\"]\ntopics = []\n";
    let decisions = read(text);
    let lantern = DocumentKey::new(Kind::Topic, "lantern");

    assert_eq!(
        topics_override(&decisions, &entry("a")),
        Some(&["lantern".to_owned()][..])
    );
    assert_eq!(topics_override(&decisions, &followup("a")), Some(&[][..]));
    assert_eq!(topics_override(&decisions, &fact("lantern/pin")), None);
    assert_eq!(topics_override(&decisions, &lantern), None);
    assert_eq!(
        unused(&decisions),
        [
            "documents.\"fact/lantern/pin\".topics",
            "documents.\"topic/lantern\".topics",
        ]
    );
}

#[test]
fn the_edges_of_a_topic_leave_what_it_drops_alone() {
    let decisions = read(EXAMPLE);

    assert_eq!(
        edges(&decisions, "lantern"),
        Edges {
            part_of: vec![],
            uses: vec!["atlas"],
        }
    );
    assert!(unused(&decisions).contains(&"topics.lantern.drop[\"phone\"]".to_owned()));
}

#[test]
fn a_claim_is_answered_by_unclaim_alone() {
    let desk = DocumentKey::new(Kind::Topic, "desk");
    let decisions = read(
        "[topics.desk]\npart_of = [\"phone\"]\nuses = [\"atlas\"]\n\
         drop = [\"beacon\", \"lantern\"]\nunclaim = [\"Lantern\", \"ghost\"]\n",
    );

    assert!(held(&decisions, claim("desk", "phone")).is_err());
    assert!(held(&decisions, claim("desk", "atlas")).is_err());
    assert!(held(&decisions, claim("desk", "beacon")).is_err());
    let ended = |topic: &str| held(&decisions, ended_topic(&desk, Through::Claim, topic));
    assert!(ended("beacon").is_err());
    assert_eq!(
        unused(&decisions),
        [
            "topics.desk.part_of[\"phone\"]",
            "topics.desk.uses[\"atlas\"]",
            "topics.desk.drop[\"beacon\"]",
            "topics.desk.drop[\"lantern\"]",
            "topics.desk.unclaim[\"ghost\"]",
            "topics.desk.unclaim[\"lantern\"]",
        ]
    );
    assert_eq!(held(&decisions, claim("Desk", "GHOST")), Ok(()));
    assert_eq!(ended("lantern"), Ok(()));
    let edge = held(&decisions, ended_topic(&desk, Through::Includes, "lantern"));
    assert_eq!(edge, Ok(()));
    assert_eq!(
        unused(&decisions),
        [
            "topics.desk.part_of[\"phone\"]",
            "topics.desk.uses[\"atlas\"]",
            "topics.desk.drop[\"beacon\"]",
        ]
    );
    let refusals = refused("[topics.desk]\nunclaim = [\"phone\", \"?\", \"Phone\"]\n");
    assert_eq!(refusals, ["topics.desk.unclaim: holds \"Phone\" twice"]);
}

#[test]
fn the_tables_a_file_holds_are_listed_as_a_stub_writes_them() {
    let decisions = read("[tags]\n[machines]\n[topics.Lantern]\n[documents.\"entry/a\"]\n");
    let tables: Vec<&str> = decisions.tables().iter().map(String::as_str).collect();

    assert_eq!(
        tables,
        [
            "documents.\"entry/a\"",
            "machines",
            "tags",
            "topics.lantern"
        ]
    );
}

#[test]
fn a_former_name_is_answered_by_a_name_or_left_out() {
    let lantern = DocumentKey::new(Kind::Topic, "lantern");
    let decisions = read(
        "[documents.\"topic/lantern\"]\nname = \"lamp\"\n\
         former_names = { Lantern_Old = \"lantern-old\", \"Lamp.1\" = \"\", unasked = \"x\" }\n",
    );
    let why = "holds an uppercase letter";

    assert_eq!(
        said(&decisions, former_name(&lantern, "Lantern_Old", why)).map(FormerAnswer::of),
        Ok(FormerAnswer::Name("lantern-old"))
    );
    assert_eq!(
        said(&decisions, former_name(&lantern, "Lamp.1", why)).map(FormerAnswer::of),
        Ok(FormerAnswer::LeftOut)
    );
    assert_eq!(
        said(&decisions, former_name(&lantern, "lantern_old", why)).map(FormerAnswer::of),
        Err(Open::FormerName {
            document: lantern.clone(),
            name: "lantern_old".to_owned(),
            why: why.to_owned(),
        })
    );
    assert_eq!(
        unused(&decisions),
        [
            "documents.\"topic/lantern\".name",
            "documents.\"topic/lantern\".former_names.unasked",
        ]
    );
}

#[test]
fn a_former_name_that_is_no_line_is_refused() {
    assert_eq!(
        refused(
            "[documents.\"topic/lantern\"]\nformer_names = { a = \"?\", b = \" \", c = 3 }\n\
             [documents.\"topic/atlas\"]\nformer_names = \"lamp\"\n"
        ),
        [
            "documents.\"topic/lantern\".former_names.b: is empty",
            "documents.\"topic/lantern\".former_names.c: is not text",
            "documents.\"topic/atlas\".former_names: is not a table",
        ]
    );
}

#[test]
fn the_example_of_the_readme_is_a_decision_file_with_a_line_for_every_table() {
    let readme = include_str!("../../README.md");
    let (_, example) = readme.split_once("\n```toml\n").unwrap();
    let (example, _) = example.split_once("\n```\n").unwrap();
    let decisions = read(example);
    let lamp = entry("2026-09/2026-09-04-lantern");

    assert_eq!(
        decisions.unanswered().len(),
        2,
        "{:?}",
        decisions.unanswered()
    );
    assert_eq!(
        conversion(&decisions, ConversionKey::Written),
        Some("2026-10-20T09:30:00+01:00")
    );
    assert_eq!(conversion(&decisions, ConversionKey::Machine), Some("desk"));
    let own = own_tag(&decisions, &lamp, "lantern").map(|(_, answer)| answer);
    assert_eq!(own, Some(TagAnswer::Topic("atlas")));
    assert_eq!(tag(&decisions, &lamp, "phone"), Ok(TagAnswer::Dropped));
    assert_eq!(
        said(&decisions, topic_summary("atlas")),
        Ok("A book of maps")
    );
    let _ = edges(&decisions, "atlas");
    assert_eq!(
        placed(&decisions, include("lantern", "desk", false)),
        Ok(Placement::Uses)
    );
    assert_eq!(
        placed(&decisions, include("lantern", "phone", false)),
        Ok(Placement::Drop)
    );
    assert_eq!(held(&decisions, claim("desk", "phone")), Ok(()));
    assert_eq!(said(&decisions, machine("phone", 1)), Ok("desk"));
    assert_eq!(
        topics_override(&decisions, &lamp),
        Some(&["lantern".to_owned()][..])
    );
    assert_eq!(
        said(&decisions, name(&lamp, "Lantern", "is refused")),
        Ok("lantern")
    );
    assert_eq!(
        said(
            &decisions,
            former_name(&lamp, "Lantern_Atlas", "is refused")
        )
        .map(FormerAnswer::of),
        Ok(FormerAnswer::LeftOut)
    );
    assert_eq!(said(&decisions, summary(&lamp)), Ok("One line"));
    let by = entry("2026-09/2026-09-05-atlas");
    assert_eq!(
        said(&decisions, Open::ended(&lamp, false, "")),
        Ok("merged")
    );
    assert_eq!(ended_by(&decisions, &lamp), Some(&by));
    let pin = fact("lantern/desk");
    assert_eq!(
        said(&decisions, recheck_summary(&pin, "atlas")),
        Ok("Look again")
    );
    let port = followup("2026-09-04-phone");
    assert_eq!(
        said(&decisions, entry_of(&port, "gone")).map(EntryAnswer::of),
        Ok(EntryAnswer::NoEntry)
    );
    assert_eq!(
        said(&decisions, touching(&port, "gone")).map(TouchingAnswer::of),
        Ok(TouchingAnswer::Topic("atlas"))
    );
    assert_eq!(
        tag(&decisions, &port, "lantern"),
        Ok(TagAnswer::Topic("lantern"))
    );
    assert_eq!(unused(&decisions), [""; 0]);
}
