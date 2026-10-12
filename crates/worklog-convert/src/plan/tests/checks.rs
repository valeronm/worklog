use super::*;

fn cycle(topics: &[&str]) -> Question {
    Question::Open(Open::Cycle {
        topics: topics.iter().map(|topic| (*topic).to_owned()).collect(),
    })
}

#[test]
fn a_cycle_the_placements_make_is_asked_about_once() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &["atlas"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &["beacon"], desk(DAY_1));
    old.topic(
        "beacon",
        "A light on a hill",
        &["lantern", "phone"],
        desk(DAY_1),
    );
    old.topic("phone", "A phone", &[], desk(DAY_1));
    let store = read(&old);
    let placed = "[topics.lantern]\nuses = [\"atlas\"]\n[topics.atlas]\npart_of = [\"beacon\"]\n\
                  [topics.beacon]\nuses = [\"phone\"]\n";
    let decisions = answers(&format!("{placed}part_of = [\"lantern\"]\n"));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, [cycle(&["atlas", "beacon", "lantern"])]);

    let open = answers(&format!("{placed}drop = [\"lantern\"]\n"));
    assert_eq!(plan(&store, &open).questions, []);
}

#[test]
fn a_cycle_an_added_edge_makes_is_asked_about() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &["atlas"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[topics.lantern]\npart_of = [\"atlas\"]\n\
         [topics.atlas]\nuses = [\"lantern\", \"atlas\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(
        planned.questions,
        [cycle(&["atlas"]), cycle(&["atlas", "lantern"])]
    );
    assert_eq!(of(&planned, Kind::Topic, "atlas").versions.len(), 2);
}

#[test]
fn an_edge_only_an_ended_topic_holds_or_one_the_file_drops_closes_no_cycle() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &["lantern"], desk(DAY_1));
    old.topic("lantern", "A lamp", &["phone", "atlas"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &["lantern"], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}{ENDED_PHONE}[topics.phone]\npart_of = [\"lantern\"]\n\
         [topics.lantern]\npart_of = [\"phone\"]\ndrop = [\"phone\", \"atlas\"]\n\
         [topics.atlas]\nuses = [\"lantern\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
}

#[test]
fn an_edge_of_an_ended_topic_closes_no_cycle_while_the_edge_to_it_stands() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &["lantern"], desk(DAY_1));
    old.topic("lantern", "A lamp", &["phone"], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let decisions = answers(&format!(
        "{ENDED_PHONE}[topics.phone]\npart_of = [\"lantern\"]\n\
         [topics.lantern]\npart_of = [\"phone\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(
        planned.questions,
        [Question::Open(Open::EndedTopic {
            document: key(Kind::Topic, "lantern"),
            through: Through::Includes,
            topic: "phone".to_owned(),
        })]
    );
    let lantern = topic(&of(&planned, Kind::Topic, "lantern").versions[0]);
    assert_eq!(lantern.part_of, [topic_id("phone")]);
}

#[test]
fn a_machine_with_no_topic_is_one_question_whoever_names_it() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], at(DAY_2, "bench"));
    old.entry(
        "2026-09/2026-09-04-lamp",
        "Drove it",
        &[],
        at(DAY_2, "bench"),
    );
    let store = read(&old);
    let file = "[conversion]\nwritten = 2026-10-20T09:30:00+01:00\nmachine = \"bench\"\n\
                [topics.firmware]\nsummary = \"The firmware\"\n[topics.atlas]\nuses = [\"lantern\"]\n";
    let planned = plan(&store, &answers(file));

    assert_eq!(
        planned.questions,
        [Question::Open(Open::Machine {
            name: "bench".to_owned(),
            versions: 4,
        })]
    );
}

#[test]
fn a_conversion_written_no_later_than_the_store_is_an_answer_that_is_wrong() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_5));
    old.topic("phone", "A phone", &[], desk("someday"));
    let store = read(&old);
    let file = |written: &str| {
        format!(
            "[conversion]\nwritten = {written}\nmachine = \"desk\"\n\
             [topics.atlas]\nuses = [\"lantern\"]\n"
        )
    };
    let early = |written: &str| {
        let planned = plan(&store, &answers(&file(written)));
        opens(&planned)
    };
    let wrong = |written: &str| {
        vec![Open::Answer {
            key: Key::Conversion(crate::key::ConversionKey::Written),
            why: format!(
                "{written} is not later than {DAY_5}, the latest write time the store holds"
            ),
        }]
    };

    assert_eq!(early("2026-09-09T23:00:00-12:00"), []);
    assert_eq!(
        early("2026-09-10T10:00:00+01:00"),
        wrong("2026-09-10T10:00:00+01:00")
    );
    assert_eq!(early("2026-09-08T12:00:00Z"), wrong("2026-09-08T12:00:00Z"));
}

#[test]
fn a_version_wanted_after_a_document_with_several_heads_blocks_whoever_read_it() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let atlas = old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let (one, other) = old.forked(&atlas, desk(DAY_2), desk(DAY_3));
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    let battery = old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    let (left, right) = old.forked(&battery, desk(DAY_2), desk(DAY_3));
    old.removed(&phone, "Sold", desk(DAY_4));
    let mut store = read(&old);
    store.problems.clear();
    let decisions = answers(&format!(
        "{CONVERSION}{ENDED_PHONE}[topics.atlas]\nuses = [\"lantern\"]\n\
         [documents.\"fact/phone/battery\"]\ntopics = [\"lantern\"]\n"
    ));
    let planned = plan(&store, &decisions);

    let forked = |kind, slug: &str, heads: [&Stored; 2]| {
        let mut heads: Vec<String> = heads.iter().map(|head| head.id.to_string()).collect();
        heads.sort();
        Blocked::Forked {
            document: key(kind, slug),
            heads,
        }
    };
    assert_eq!(
        blocks(&planned),
        [
            forked(Kind::Fact, "phone/battery", [&left, &right]),
            forked(Kind::Topic, "atlas", [&one, &other]),
        ]
    );
    assert_eq!(planned.authored, []);
}

#[test]
fn a_decision_no_part_of_the_plan_asks_for_is_unused() {
    let old = OldStore::new();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let claimed = [("ghost", &["~/ghost"][..])];
    old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    let pin = old.fact("lantern/relay-pin", "Pin 5", &[], desk(DAY_1));
    let touching = [("recheck", scalar("touching lantern"))];
    old.changed(&pin, "recheck", &touching, desk(DAY_2));
    let store = read(&old);
    let used = format!(
        "{CONVERSION}[topics.firmware]\nsummary = \"The firmware\"\nuses = [\"lantern\"]\n\
         [topics.desk]\nunclaim = [\"ghost\"]\n[topics.atlas]\npart_of = [\"lantern\"]\n\
         [documents.\"fact/lantern/relay-pin\"]\nrecheck_summary = \"Look again\"\n"
    );
    let planned = plan(&store, &answers(&used));
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    assert_eq!(planned.authored.len(), 2);

    let spare = format!(
        "{used}summary = \"Pin five\"\n[tags]\nrust = \"\"\n[topics.lantern]\ndrop = [\"atlas\"]\n\
         [machines]\nbench = \"desk\"\n[documents.\"fact/lantern/dimmer\"]\n\
         recheck_summary = \"Look at it\"\n"
    );
    let planned = plan(&store, &answers(&spare));
    assert_eq!(planned.questions, []);
    let unused: Vec<String> = planned.unused.iter().map(Key::to_string).collect();
    assert_eq!(
        unused,
        [
            "tags.rust",
            "topics.lantern.drop[\"atlas\"]",
            "machines.bench",
            "documents.\"fact/lantern/dimmer\".recheck_summary",
            "documents.\"fact/lantern/relay-pin\".summary",
        ]
    );
}

fn planned_as<'a>(id: &DocumentId, kind: KindOf, content: Content, ended: bool) -> Document<'a> {
    let ending = ended.then(|| Ending {
        reason: Reason::Retired,
        on: date("2026-09-05"),
        by: None,
        note: "Gone".to_owned(),
    });
    Document {
        id: id.clone(),
        kind,
        versions: vec![Version {
            key: AUTHORED.to_owned(),
            parents: Vec::new(),
            outside: Vec::new(),
            stamp: Stamp::parse(DAY_1).ok(),
            machine: Some(DocumentId::from_bytes([1; 16])),
            change: "new".to_owned(),
            record: Some(Record { content, ending }),
            body: "\n".to_owned(),
            links: Links::default(),
            origin: Origin::Authored { answered: None },
        }],
    }
}

fn a_topic(name: &str) -> Content {
    Content::Topic(Topic {
        name: names(&[name])[0].clone(),
        former_names: Vec::new(),
        created: date("2026-09-04"),
        summary: "A topic".to_owned(),
        part_of: Vec::new(),
        uses: Vec::new(),
    })
}

fn a_fact(name: &str, topic: &DocumentId) -> Content {
    Content::Fact(Fact {
        name: names(&[name])[0].clone(),
        former_names: Vec::new(),
        topic: topic.clone(),
        created: date("2026-09-04"),
        confirmed: date("2026-09-04"),
        idea: false,
        summary: "A fact".to_owned(),
    })
}

fn id(byte: u8) -> DocumentId {
    DocumentId::from_bytes([byte; 16])
}

fn unheld(document: &DocumentId, kind: &str, field: &str, target: &str, why: &str) -> Question {
    Question::Blocked(Blocked::Unheld {
        kind: kind.to_owned(),
        document: document.to_string(),
        field: field.to_owned(),
        target: target.to_owned(),
        why: why.to_owned(),
    })
}

fn found(documents: &[Document<'_>]) -> Vec<Question> {
    let questions = Questions::default();
    check::unheld(documents, &questions);
    questions.asked()
}

#[test]
fn a_reference_the_plan_does_not_hold_blocks_when_nothing_else_was_asked() {
    let (lantern, phone, missing) = (id(1), id(2), id(3));
    let (pin, battery, charger, stray, wire) = (id(4), id(5), id(6), id(7), id(8));
    let by = |ended_by: &DocumentId| {
        let mut ended = planned_as(&charger, KindOf::Fact, a_fact("charger", &phone), true);
        let record = ended.versions[0].record.as_mut().unwrap();
        record.ending.as_mut().unwrap().by = Some(ended_by.clone());
        ended
    };
    let followup = Content::Followup(worklog_next::domain::schema::Followup {
        created: date("2026-09-04"),
        topics: vec![lantern.clone(), missing.clone(), pin.clone()],
        entry: Some(pin.clone()),
        about: Some(pin.clone()),
        trigger: None,
        summary: "Wire it".to_owned(),
    });
    let documents = [
        planned_as(&lantern, KindOf::Topic, a_topic("lantern"), false),
        planned_as(&phone, KindOf::Topic, a_topic("phone"), true),
        planned_as(&pin, KindOf::Fact, a_fact("pin", &lantern), false),
        planned_as(&battery, KindOf::Fact, a_fact("battery", &phone), false),
        by(&missing),
        planned_as(&stray, KindOf::Fact, a_fact("stray", &missing), false),
        planned_as(&wire, KindOf::Followup, followup, false),
    ];

    let (phone, missing, pin) = (phone.as_str(), missing.as_str(), pin.as_str());
    let mut expected = vec![
        unheld(&battery, "fact", "topic", phone, "is ended"),
        unheld(&charger, "fact", "ended_by", missing, "is not planned"),
        unheld(&stray, "fact", "topic", missing, "is not planned"),
        unheld(
            &wire,
            "followup",
            "entry",
            pin,
            "is planned as fact, not as entry",
        ),
        unheld(&wire, "followup", "topics", missing, "is not planned"),
        unheld(
            &wire,
            "followup",
            "topics",
            pin,
            "is planned as fact, not as topic",
        ),
    ];
    expected.sort();
    assert_eq!(found(&documents), expected);

    let asked = Questions::default();
    asked.ask(Open::Cycle {
        topics: vec!["lantern".to_owned()],
    });
    check::unheld(&documents, &asked);
    assert_eq!(asked.asked().len(), 1);
}

#[test]
fn a_planned_version_that_lacks_what_is_written_blocks_when_nothing_else_was_asked() {
    let (lantern, pin, atlas, missing) = (id(1), id(4), id(9), id(3));
    let sound = || planned_as(&lantern, KindOf::Topic, a_topic("lantern"), false);
    assert_eq!(found(&[sound()]), []);
    let holed = |hole: fn(&mut Document<'_>)| {
        let mut document = planned_as(&atlas, KindOf::Topic, a_topic("atlas"), false);
        hole(&mut document);
        let fact = planned_as(&pin, KindOf::Fact, a_fact("pin", &lantern), false);
        found(&[sound(), document, fact])
    };
    let of =
        |field: &str, target: &str, why: &str| vec![unheld(&atlas, "topic", field, target, why)];
    let missing_text = missing.to_string();

    let none = holed(|document| document.versions[0].record = None);
    assert_eq!(none, of("record", AUTHORED, "is absent"));
    let unstamped = holed(|document| document.versions[0].stamp = None);
    assert_eq!(unstamped, of("written", AUTHORED, "is absent"));
    let unmade = holed(|document| document.versions[0].machine = None);
    assert_eq!(unmade, of("machine", AUTHORED, "is absent"));
    let elsewhere = holed(|document| document.versions[0].machine = Some(id(3)));
    assert_eq!(elsewhere, of("machine", &missing_text, "is not planned"));
    let by_a_fact = holed(|document| document.versions[0].machine = Some(id(4)));
    let fact = id(4).to_string();
    assert_eq!(
        by_a_fact,
        of("machine", &fact, "is planned as fact, not as topic")
    );
    let orphan = holed(|document| document.versions[0].parents = vec!["aa11".to_owned()]);
    assert_eq!(
        orphan,
        of("parents", "aa11", "is no earlier version of the document")
    );
    let outside = holed(|document| document.versions[0].outside = vec!["bb22".to_owned()]);
    assert_eq!(
        outside,
        of("parents", "bb22", "is no version of the document")
    );
    let linked = holed(|document| {
        let rows = [
            ("lantern".to_owned(), Some(id(3))),
            ("ghost".to_owned(), None),
        ];
        document.versions[0].links = Links(rows.into_iter().collect());
    });
    assert_eq!(linked, of("links", &missing_text, "is not planned"));
    let forked = holed(|document| {
        let mut again = document.versions[0].clone();
        again.key = "cc33".to_owned();
        document.versions.push(again);
    });
    assert_eq!(forked, of("versions", "2", "end with no one version"));
    let empty = holed(|document| document.versions.clear());
    assert_eq!(empty, of("versions", "0", "end with no one version"));
}

#[test]
fn a_stamp_the_file_gives_is_held_to_the_store_before_anything_takes_it() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_5));
    let store = read(&old);
    let written = "2026-09-01T00:00:00Z";
    let file = format!("[conversion]\nwritten = {written}\nmachine = \"bench\"\n");
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.authored, []);
    assert_eq!(
        opens(&planned),
        [
            Open::Machine {
                name: "bench".to_owned(),
                versions: 0,
            },
            Open::Answer {
                key: Key::Conversion(crate::key::ConversionKey::Written),
                why: format!(
                    "{written} is not later than {DAY_5}, the latest write time the store holds"
                ),
            },
        ]
    );

    let held = "[conversion]\nwritten = \"?\"\nmachine = \"bench\"\n";
    let planned = plan(&store, &answers(held));
    assert_eq!(
        opens(&planned),
        [Open::Machine {
            name: "bench".to_owned(),
            versions: 0,
        }]
    );
}
