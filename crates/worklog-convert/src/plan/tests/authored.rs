use super::*;

fn answer(key: Key, why: &str) -> Open {
    Open::Answer {
        key,
        why: why.to_owned(),
    }
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_table_of_no_topic_creates_one_with_its_summary_and_edges() {
    let old = with_topics();
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[topics.Lantern-Firmware]\nsummary = \"The firmware\"\n\
         part_of = [\"Lantern\"]\nuses = [\"atlas\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let created = by_id(&planned, &created_topic_id("lantern-firmware"));
    assert_eq!(created.kind, KindOf::Topic);
    let [version] = created.versions.as_slice() else {
        panic!("one version: {:?}", created.versions);
    };
    authored_by_desk(version, &[], "new");
    let summary = Key::topic("lantern-firmware", TopicKey::Summary);
    assert_eq!(
        version.origin,
        Origin::Authored {
            answered: Some(summary)
        }
    );
    assert_eq!(version.body, "\n");
    assert_eq!(
        topic(version),
        Topic {
            name: names(&["lantern-firmware"])[0].clone(),
            former_names: Vec::new(),
            created: date("2026-10-20"),
            summary: "The firmware".to_owned(),
            part_of: vec![topic_id("lantern")],
            uses: vec![topic_id("atlas")],
        }
    );
    assert_eq!(
        planned.authored,
        [Authored::Topic {
            name: "lantern-firmware".to_owned(),
            summary: "The firmware".to_owned(),
            part_of: owned(&["lantern"]),
            uses: owned(&["atlas"]),
        }]
    );
}

#[test]
fn a_created_topic_with_no_summary_is_asked_about_and_listed() {
    let old = with_topics();
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[topics.lantrn]\nuses = [\"atlas\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(
        planned.questions,
        [Question::Open(Open::TopicSummary {
            topic: Folded::new("lantrn"),
        })]
    );
    let created = by_id(&planned, &created_topic_id("lantrn"));
    assert_eq!(created.versions.len(), 1);
    assert_eq!(created.versions[0].record, None);
    assert_eq!(stamped(&created.versions[0]), CONVERTED);
    assert_eq!(
        planned.authored,
        [Authored::Topic {
            name: "lantrn".to_owned(),
            summary: String::new(),
            part_of: Vec::new(),
            uses: owned(&["atlas"]),
        }]
    );
    assert_eq!(planned.unused, []);
}

#[test]
fn what_a_created_topic_cannot_take_is_asked_about_at_its_key() {
    let old = with_topics();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_2));
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}{ENDED_PHONE}[topics.firmware]\nsummary = \"The firmware\"\n\
         part_of = [\"ghost\"]\nuses = [\"phone\", \"atlas\"]\ndrop = [\"lantern\"]\n\
         [topics.x_y]\nsummary = \"Nothing\"\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(
        opens(&planned),
        [
            answer(
                Key::placed("firmware", Placement::PartOf, "ghost"),
                "ghost names no topic"
            ),
            answer(
                Key::placed("firmware", Placement::Uses, "phone"),
                "phone is ended"
            ),
            answer(
                Key::placed("firmware", Placement::Drop, "lantern"),
                "firmware is created by the file and drops nothing"
            ),
            answer(
                Key::topic("x_y", TopicKey::Table),
                "x_y holds `_`, and a name is lowercase letters, digits and hyphens"
            ),
        ]
    );
    assert_eq!(planned.unused, []);
    let firmware = by_id(&planned, &created_topic_id("firmware"));
    assert_eq!(firmware.versions[0].record, None);
    assert_eq!(
        planned.authored[0],
        Authored::Topic {
            name: "firmware".to_owned(),
            summary: "The firmware".to_owned(),
            part_of: Vec::new(),
            uses: owned(&["atlas"]),
        }
    );
}

#[test]
fn a_created_topic_and_a_topic_answered_its_name_are_both_asked_about() {
    let old = with_topics();
    old.topic("Lantern_Old", "An older lamp", &[], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[topics.lamp]\nsummary = \"A lamp\"\n\
         [documents.\"topic/Lantern_Old\"]\nname = \"lamp\"\n"
    ));
    let planned = plan(&store, &decisions);

    let old_lantern = key(Kind::Topic, "Lantern_Old");
    assert_eq!(
        opens(&planned),
        [
            answer(
                Key::topic("lamp", TopicKey::Table),
                "lamp is held by several documents"
            ),
            answer(
                Key::document(&old_lantern, DocumentField::Name),
                "lamp is held by several documents"
            ),
        ]
    );
}

#[test]
fn what_the_conversion_authors_with_no_stamp_is_one_question() {
    let old = with_topics();
    let lantern = old.topic("beacon", "A light on a hill", &[], desk(DAY_1));
    let store = read(&old);
    let file = "[topics.firmware]\nsummary = \"The firmware\"\n\
                [topics.beacon]\nuses = [\"atlas\"]\n";
    let planned = plan(&store, &answers(file));

    assert_eq!(
        planned.questions,
        [Question::Open(Open::Conversion { versions: 2 })]
    );
    let created = &by_id(&planned, &created_topic_id("firmware")).versions[0];
    assert_eq!((&created.stamp, &created.machine), (&None, &None));
    assert_eq!(created.record, None);
    let beacon = of(&planned, Kind::Topic, "beacon");
    assert_eq!(beacon.versions.len(), 2);
    let added = &beacon.versions[1];
    assert_eq!((&added.stamp, &added.machine), (&None, &None));
    assert_eq!(added.parents, [lantern.id.to_string()]);
    assert_eq!(topic(added).uses, [topic_id("atlas")]);
    assert_eq!(planned.authored.len(), 2);
    assert_eq!(planned.unused, []);

    let elsewhere = "[conversion]\nwritten = 2026-10-20T09:30:00+01:00\nmachine = \"bench\"\n";
    let planned = plan(&store, &answers(&format!("{elsewhere}{file}")));
    assert_eq!(
        planned.questions,
        [Question::Open(Open::Machine {
            name: "bench".to_owned(),
            versions: 2,
        })]
    );
    let created = &by_id(&planned, &created_topic_id("firmware")).versions[0];
    assert_eq!(stamped(created), CONVERTED);
    assert_eq!(created.machine, None);
    assert_eq!(topic(created).created, date("2026-10-20"));
}

#[test]
fn a_stamp_the_file_gives_before_anything_takes_it_stands_in_no_way() {
    let old = with_topics();
    let store = read(&old);
    let planned = plan(&store, &answers(CONVERSION));

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    assert_eq!(planned.authored, []);
}

#[test]
fn edges_the_store_never_held_are_one_version_after_the_last() {
    let old = with_topics();
    old.topic("phone", "A phone", &[], desk(DAY_1));
    let first = old.topic("beacon", "A light on a hill", &["atlas"], desk(DAY_1));
    let saved = old.saved(&first, "A light on a far hill", "\nFar.\n", desk(DAY_2));
    let handset = old.topic("handset", "Another phone", &["phone"], desk(DAY_1));
    old.changed(&handset, "save", &[("includes", list(&[]))], desk(DAY_2));
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[topics.beacon]\npart_of = [\"atlas\", \"lantern\"]\nuses = [\"PHONE\"]\n\
         [topics.handset]\nuses = [\"phone\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let beacon = of(&planned, Kind::Topic, "beacon");
    assert_eq!(beacon.versions.len(), 3);
    for converted in &beacon.versions[..2] {
        assert_eq!(topic(converted).part_of, [topic_id("atlas")]);
        assert_eq!(topic(converted).uses, []);
    }
    let added = &beacon.versions[2];
    authored_by_desk(added, &[&saved], "save");
    assert_eq!(added.origin, Origin::Authored { answered: None });
    assert_eq!(added.body, "\nFar.\n");
    assert_eq!(
        topic(added),
        Topic {
            part_of: vec![topic_id("atlas"), topic_id("lantern")],
            uses: vec![topic_id("phone")],
            ..topic(&beacon.versions[1])
        }
    );
    let handset = of(&planned, Kind::Topic, "handset");
    assert_eq!(handset.versions.len(), 2);
    assert_eq!(topic(&handset.versions[0]).uses, [topic_id("phone")]);
    assert_eq!(
        planned.authored,
        [Authored::Edges {
            topic: "beacon".to_owned(),
            part_of: owned(&["lantern"]),
            uses: owned(&["phone"]),
        }]
    );
}

#[test]
fn an_item_that_names_a_placed_topic_by_another_name_is_an_answer_that_is_wrong() {
    let old = with_desk();
    let atlas = old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("lantern", "A lamp", &["Atlas"], desk(DAY_1));
    old.renamed(&atlas, "world", desk(DAY_2));
    let store = read(&old);
    let wrong = answer(
        Key::placed("lantern", Placement::PartOf, "atlas"),
        "atlas is world, an item of includes that is placed under that name",
    );

    let both = "[topics.lantern]\npart_of = [\"atlas\"]\nuses = [\"world\"]\n";
    let decisions = answers(both);
    let planned = plan(&store, &decisions);
    assert_eq!(opens(&planned), std::slice::from_ref(&wrong));
    assert_eq!(planned.unused, []);
    assert_eq!(planned.authored, []);
    assert_eq!(
        topic(&of(&planned, Kind::Topic, "lantern").versions[0]).uses,
        [topic_id("atlas")]
    );

    let planned = plan(
        &store,
        &answers("[topics.lantern]\npart_of = [\"atlas\"]\n"),
    );
    let unplaced = Open::Include {
        topic: Folded::new("lantern"),
        item: Folded::new("world"),
        missing: false,
        ended: false,
        unread: false,
    };
    assert_eq!(opens(&planned), [unplaced, wrong]);

    let planned = plan(
        &store,
        &answers("[topics.lantern]\npart_of = [\"world\"]\n"),
    );
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
}

#[test]
fn an_edge_is_added_to_no_ended_topic_and_names_one_that_is_not_ended() {
    let old = with_topics();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_2));
    let store = read(&old);
    let decisions = answers(&format!(
        "{ENDED_PHONE}[topics.phone]\nuses = [\"atlas\"]\n\
         [topics.lantern]\npart_of = [\"ghost\"]\nuses = [\"phone\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(
        opens(&planned),
        [
            answer(
                Key::placed("lantern", Placement::PartOf, "ghost"),
                "ghost names no topic"
            ),
            answer(
                Key::placed("lantern", Placement::Uses, "phone"),
                "phone is ended"
            ),
            answer(
                Key::placed("phone", Placement::Uses, "atlas"),
                "phone is ended, and an ended topic takes no edge"
            ),
        ]
    );
    assert_eq!(of(&planned, Kind::Topic, "lantern").versions.len(), 1);
    assert_eq!(of(&planned, Kind::Topic, "phone").versions.len(), 2);
    assert_eq!(planned.authored, []);
    assert_eq!(planned.unused, []);
}

#[test]
fn a_fact_under_an_ended_topic_keeps_it_and_is_filed_again_in_one_more_version() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let battery = old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    let saved = old.saved(&battery, "Lasts two days", "\nTwo.\n", desk(DAY_2));
    let charger = old.fact("phone/charger", "Five volts", &[], desk(DAY_1));
    old.removed(&charger, "Lost", desk(DAY_2));
    let case = old.fact("phone/case", "Leather", &[], desk(DAY_1));
    old.renamed(&case, "atlas/case", desk(DAY_2));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let ended = format!("{ENDED_PHONE}[documents.\"fact/phone/charger\"]\nended = \"false\"\n");

    let asked = plan(&store, &answers(&ended));
    assert_eq!(
        asked.questions,
        [Question::Open(Open::EndedTopic {
            document: key(Kind::Fact, "phone/battery"),
            through: Through::FactTopic,
            topic: "phone".to_owned(),
        })]
    );
    let kept = of(&asked, Kind::Fact, "phone/battery");
    assert_eq!(kept.versions.len(), 2);
    assert_eq!(fact(&kept.versions[1]).topic, topic_id("phone"));
    let case = of(&asked, Kind::Fact, "phone/case");
    assert_eq!(fact(&case.versions[0]).topic, topic_id("phone"));
    assert_eq!(fact(&case.versions[1]).topic, topic_id("atlas"));
    let charger = of(&asked, Kind::Fact, "phone/charger");
    assert_eq!(fact(&charger.versions[1]).topic, topic_id("phone"));
    assert_eq!(charger.versions.len(), 2);

    let battery = "[documents.\"fact/phone/battery\"]\ntopics = ";
    let decisions = answers(&format!("{CONVERSION}{ended}{battery}[\"Atlas\"]\n"));
    let answered = plan(&store, &decisions);
    assert_eq!(answered.questions, []);
    assert_eq!(answered.unused, []);
    let moved = of(&answered, Kind::Fact, "phone/battery");
    assert_eq!(moved.versions.len(), 3);
    assert_eq!(fact(&moved.versions[0]).topic, topic_id("phone"));
    assert_eq!(fact(&moved.versions[1]).topic, topic_id("phone"));
    let again = &moved.versions[2];
    authored_by_desk(again, &[&saved], "move");
    assert_eq!(again.origin, Origin::Authored { answered: None });
    assert_eq!(again.body, "\nTwo.\n");
    let before = fact(&moved.versions[1]);
    assert_eq!(before.confirmed, date("2026-09-05"));
    assert_eq!(
        fact(again),
        Fact {
            topic: topic_id("atlas"),
            ..before
        }
    );
    assert_eq!(
        answered.authored,
        [Authored::Refiled {
            fact: "phone/battery".to_owned(),
            from: "phone".to_owned(),
            to: "atlas".to_owned(),
        }]
    );

    let still = plan(&store, &answers(&format!("{ended}{battery}[\"phone\"]\n")));
    assert_eq!(
        still.questions,
        [Question::Open(Open::Answer {
            key: Key::document(&key(Kind::Fact, "phone/battery"), DocumentField::Topics),
            why: "phone is ended".to_owned(),
        })]
    );
    assert_eq!(of(&still, Kind::Fact, "phone/battery").versions.len(), 2);
}

#[test]
fn a_fact_filed_again_holds_its_name_against_the_facts_of_its_new_topic() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let battery = old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    old.fact("atlas/battery", "A page on cells", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let filed = format!(
        "{CONVERSION}{ENDED_PHONE}[documents.\"fact/phone/battery\"]\ntopics = [\"atlas\"]\n"
    );
    let decisions = answers(&filed);
    let planned = plan(&store, &decisions);

    let shared = |slug: &str| Open::Name {
        document: key(Kind::Fact, slug),
        name: "battery".to_owned(),
        why: "is held by several documents".to_owned(),
    };
    assert_eq!(
        opens(&planned),
        [shared("atlas/battery"), shared("phone/battery")]
    );

    let decisions = answers(&format!("{filed}name = \"battery-life\"\n"));
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let moved = of(&planned, Kind::Fact, "phone/battery");
    assert_eq!(moved.versions.len(), 2);
    authored_by_desk(&moved.versions[1], &[&battery], "rename");
    let renamed = Fact {
        name: names(&["battery-life"])[0].clone(),
        former_names: names(&["battery"]),
        topic: topic_id("atlas"),
        ..fact(&moved.versions[0])
    };
    assert_eq!(fact(&moved.versions[1]), renamed);
    assert_eq!(
        planned.authored,
        [
            Authored::Refiled {
                fact: "phone/battery".to_owned(),
                from: "phone".to_owned(),
                to: "atlas".to_owned(),
            },
            Authored::Renamed {
                document: "fact/phone/battery".to_owned(),
                from: "battery".to_owned(),
                to: "battery-life".to_owned(),
            },
        ]
    );
}

#[test]
fn an_edge_to_an_ended_topic_stays_in_history_and_is_dropped_in_one_more_version() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    let lantern = old.topic("lantern", "A lamp", &["phone", "atlas"], desk(DAY_1));
    let beacon = old.topic("beacon", "A light on a hill", &["phone"], desk(DAY_1));
    old.changed(&beacon, "save", &[("includes", list(&[]))], desk(DAY_2));
    let handset = old.topic("handset", "Another phone", &["phone"], desk(DAY_1));
    old.removed(&handset, "Sold too", desk(DAY_2));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let rest = format!(
        "{ENDED_PHONE}[documents.\"topic/handset\"]\nended = \"retired\"\n\
         [topics.beacon]\npart_of = [\"phone\"]\n[topics.handset]\nuses = [\"phone\"]\n"
    );

    let placed = "[topics.lantern]\npart_of = [\"atlas\"]\nuses = [\"phone\"]\n";
    let asked = plan(&store, &answers(&format!("{rest}{placed}")));
    assert_eq!(
        asked.questions,
        [Question::Open(Open::EndedTopic {
            document: key(Kind::Topic, "lantern"),
            through: Through::Includes,
            topic: "phone".to_owned(),
        })]
    );
    let kept = of(&asked, Kind::Topic, "lantern");
    assert_eq!(kept.versions.len(), 1);
    assert_eq!(topic(&kept.versions[0]).uses, [topic_id("phone")]);
    let beacon = of(&asked, Kind::Topic, "beacon");
    assert_eq!(topic(&beacon.versions[0]).part_of, [topic_id("phone")]);
    assert_eq!(topic(&beacon.versions[1]).part_of, []);
    assert_eq!(beacon.versions.len(), 2);
    let handset = of(&asked, Kind::Topic, "handset");
    assert_eq!(topic(&handset.versions[0]).uses, [topic_id("phone")]);
    assert_eq!(handset.versions.len(), 2);

    let dropped = format!("{CONVERSION}{rest}{placed}drop = [\"Phone\"]\n");
    let answered = plan(&store, &answers(&dropped));
    assert_eq!(answered.questions, []);
    assert_eq!(answered.unused, []);
    let lantern_now = of(&answered, Kind::Topic, "lantern");
    assert_eq!(lantern_now.versions.len(), 2);
    let converted = topic(&lantern_now.versions[0]);
    assert_eq!(converted.uses, [topic_id("phone")]);
    let last = &lantern_now.versions[1];
    authored_by_desk(last, &[&lantern], "save");
    assert_eq!(
        topic(last),
        Topic {
            uses: Vec::new(),
            ..converted
        }
    );
    assert_eq!(topic(last).part_of, [topic_id("atlas")]);
    assert_eq!(
        answered.authored,
        [Authored::Dropped {
            topic: "lantern".to_owned(),
            part_of: Vec::new(),
            uses: owned(&["phone"]),
        }]
    );

    let never = "[topics.lantern]\npart_of = [\"atlas\"]\ndrop = [\"phone\"]\n";
    let unplaced = plan(&store, &answers(&format!("{rest}{never}")));
    assert_eq!(unplaced.questions, []);
    assert_eq!(unplaced.unused, []);
    let lantern_never = of(&unplaced, Kind::Topic, "lantern");
    assert_eq!(lantern_never.versions.len(), 1);
    assert_eq!(topic(&lantern_never.versions[0]).uses, []);

    let live = "[topics.lantern]\npart_of = [\"atlas\"]\nuses = [\"phone\"]\n\
                drop = [\"phone\", \"atlas\"]\n";
    let both = plan(&store, &answers(&format!("{CONVERSION}{rest}{live}")));
    let unused: Vec<String> = both.unused.iter().map(Key::to_string).collect();
    assert_eq!(unused, ["topics.lantern.drop[\"atlas\"]"]);
}

#[test]
fn an_item_naming_an_ended_topic_says_so_when_it_is_first_asked_about() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("lantern", "A lamp", &["phone", "atlas"], desk(DAY_1));
    let beacon = old.topic("beacon", "A light on a hill", &["phone"], desk(DAY_1));
    old.changed(&beacon, "save", &[("includes", list(&[]))], desk(DAY_2));
    let handset = old.topic("handset", "Another phone", &["phone"], desk(DAY_1));
    old.removed(&handset, "Sold too", desk(DAY_2));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let rest = format!("{ENDED_PHONE}[documents.\"topic/handset\"]\nended = \"retired\"\n");
    let include = |topic: &str, item: &str, ended| Open::Include {
        topic: Folded::new(topic),
        item: Folded::new(item),
        missing: false,
        ended,
        unread: false,
    };

    let asked = plan(&store, &answers(&rest));
    assert_eq!(
        opens(&asked),
        [
            include("beacon", "phone", false),
            include("handset", "phone", false),
            include("lantern", "atlas", false),
            include("lantern", "phone", true),
        ]
    );

    let both = format!(
        "{CONVERSION}{rest}[topics.lantern]\nuses = [\"phone\", \"atlas\"]\ndrop = [\"phone\"]\n\
         [topics.beacon]\nuses = [\"phone\"]\n[topics.handset]\nuses = [\"phone\"]\n"
    );
    let answered = plan(&store, &answers(&both));
    assert_eq!(answered.questions, []);
    assert_eq!(answered.unused, []);
}

#[test]
fn an_edge_added_and_an_edge_dropped_on_one_topic_are_one_version() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("lantern", "A lamp", &["phone"], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let decisions = answers(&format!(
        "{ENDED_PHONE}[topics.lantern]\npart_of = [\"atlas\"]\nuses = [\"phone\"]\n\
         drop = [\"phone\"]\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(
        planned.questions,
        [Question::Open(Open::Conversion { versions: 1 })]
    );
    let lantern = of(&planned, Kind::Topic, "lantern");
    assert_eq!(lantern.versions.len(), 2);
    assert_eq!(topic(&lantern.versions[1]).part_of, [topic_id("atlas")]);
    assert_eq!(topic(&lantern.versions[1]).uses, []);
    assert_eq!(
        planned.authored,
        [
            Authored::Edges {
                topic: "lantern".to_owned(),
                part_of: owned(&["atlas"]),
                uses: Vec::new(),
            },
            Authored::Dropped {
                topic: "lantern".to_owned(),
                part_of: Vec::new(),
                uses: owned(&["phone"]),
            },
        ]
    );
}
