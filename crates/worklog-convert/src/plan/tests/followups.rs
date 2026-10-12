use super::*;

fn with_entry() -> (OldStore, Stored) {
    let old = with_topics();
    let entry = old.entry(LAMP_DRIVER, "Drove the lamp", &[], desk(DAY_1));
    (old, entry)
}

fn recheck(old: &OldStore, parent: &Stored, recheck: &str, written: &str) -> Stored {
    old.changed(
        parent,
        "recheck",
        &[("recheck", scalar(recheck))],
        desk(written),
    )
}

fn versions_of<'p, 'a>(planned: &'p Planned<'a>, origin: &str) -> &'p [Version<'a>] {
    &of(planned, Kind::Followup, origin).versions
}

fn port_key() -> DocumentKey {
    key(Kind::Followup, PORT)
}

const TAGS: &str = "[tags]\nrelay = \"atlas\"\nmaps = \"\"\nlamp = \"lantern\"\n";

#[test]
fn a_followup_carries_its_fields_in_every_version() {
    let (old, entry) = with_entry();
    let tags = ["relay", "maps"];
    let port = old.followup(PORT, &entry, "Port the driver", &tags, desk(DAY_2));
    let dated = recheck(&old, &port, "2026-10-01 The board ships", DAY_3);
    let touching = recheck(&old, &dated, "touching Lantern", DAY_4);
    let wire = "2026-09-02-wire";
    let wired = old.followup(wire, &entry, "Wire it", &["lamp"], desk(DAY_2));
    recheck(&old, &wired, "touching lantern", DAY_3);
    let store = read(&old);
    let decisions = answers(TAGS);
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let document = of(&planned, Kind::Followup, PORT);
    assert_eq!(document.kind, KindOf::Followup);
    let versions = &document.versions;
    assert_eq!(versions.len(), 3);
    let first = Followup {
        created: date("2026-09-01"),
        topics: vec![topic_id("atlas")],
        entry: Some(document_id(Kind::Entry, LAMP_DRIVER)),
        about: None,
        trigger: None,
        summary: "Port the driver".to_owned(),
    };
    assert_eq!(followup(&versions[0]), first);
    assert_eq!(
        followup(&versions[1]),
        Followup {
            trigger: Some(Trigger::LookAgain {
                on: date("2026-10-01"),
                why: "The board ships".to_owned(),
            }),
            ..first.clone()
        }
    );
    assert_eq!(
        followup(&versions[2]),
        Followup {
            topics: vec![topic_id("atlas"), topic_id("lantern")],
            trigger: Some(Trigger::Touching(topic_id("lantern"))),
            ..first
        }
    );
    let changes: Vec<&str> = versions
        .iter()
        .map(|version| version.change.as_str())
        .collect();
    assert_eq!(changes, ["new", "trigger", "trigger"]);
    assert_eq!(versions[2].key, touching.id.to_string());
    assert_eq!(versions[2].parents, [dated.id.to_string()]);
    assert_eq!(versions[2].stamp, Some(Stamp::parse(DAY_4).unwrap()));
    assert_eq!(versions[2].machine, Some(topic_id("desk")));
    assert_eq!(versions[2].body, "\n");
    for version in versions {
        let fields = version.record.as_ref().unwrap().fields();
        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        for gone in ["name", "former_names", "state", "tags", "recheck"] {
            assert!(!keys.contains(&gone), "{gone}");
        }
        assert!(!format!("{fields:?}").contains(PORT));
        assert_eq!(ending(version), None);
    }
    assert_eq!(
        followup(&versions_of(&planned, wire)[1]).topics,
        [topic_id("lantern")]
    );
}

#[test]
fn a_followup_filed_under_no_topic_is_asked_about_and_the_file_answers() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["maps"], desk(DAY_2));
    old.saved(&port, "Port the whole driver", "\n", desk(DAY_3));
    let wire = "2026-09-02-wire";
    let wired = old.followup(wire, &entry, "Wire it", &[], desk(DAY_2));
    recheck(&old, &wired, "touching atlas", DAY_3);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    let unfiled = Open::NoTopic {
        document: port_key(),
        summary: "Port the whole driver".to_owned(),
    };
    let no_topic = Open::NoTopic {
        document: key(Kind::Followup, wire),
        summary: "Wire it".to_owned(),
    };
    assert_eq!(opens(&planned), [unfiled, no_topic.clone()]);
    assert_eq!(versions_of(&planned, PORT)[0].record, None);
    assert_eq!(versions_of(&planned, PORT)[1].record, None);
    assert_eq!(versions_of(&planned, wire)[0].record, None);
    assert_eq!(
        followup(&versions_of(&planned, wire)[1]).topics,
        [topic_id("atlas")]
    );

    let file = |topics: &str| {
        answers(&format!(
            "{TAGS}[documents.\"followup/{PORT}\"]\ntopics = {topics}\n"
        ))
    };
    let decisions = file("[\"lantern\"]");
    let planned = plan(&store, &decisions);
    assert_eq!(opens(&planned), std::slice::from_ref(&no_topic));
    assert_eq!(planned.overridden, []);
    assert_eq!(
        followup(&versions_of(&planned, PORT)[0]).topics,
        [topic_id("lantern")]
    );
    assert_eq!(
        followup(&versions_of(&planned, PORT)[1]).topics,
        [topic_id("lantern")]
    );

    let planned = plan(&store, &file("[]"));
    let empty = Open::Answer {
        key: Key::document(&port_key(), DocumentField::Topics),
        why: "names no topic, and a followup is filed under one".to_owned(),
    };
    assert_eq!(opens(&planned), [no_topic, empty]);
}

#[test]
fn a_tag_with_no_answer_is_one_question_over_entries_and_followups() {
    let (old, entry) = with_entry();
    let wiring = "2026-09/2026-09-05-wiring";
    old.entry(wiring, "Wired the lamp", &["Relay", "lamp"], desk(DAY_2));
    old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let wire = "2026-09-02-wire";
    let wired = old.followup(wire, &entry, "Wire it", &["RELAY"], desk(DAY_2));
    old.changed(&wired, "save", &[("tags", list(&["lamp"]))], desk(DAY_3));
    let own = "2026-09-03-solder";
    old.followup(own, &entry, "Solder it", &["relay"], desk(DAY_2));
    let store = read(&old);
    let file = format!("[documents.\"followup/{own}\"]\ntags.relay = \"atlas\"\n");
    let planned = plan(&store, &answers(&file));

    let mut carriers = vec![
        key(Kind::Entry, wiring),
        port_key(),
        key(Kind::Followup, wire),
    ];
    carriers.sort();
    let lamp = [key(Kind::Entry, wiring), key(Kind::Followup, wire)];
    assert_eq!(
        opens(&planned),
        [
            Open::Tag {
                tag: Folded::new("lamp"),
                carriers: lamp.to_vec(),
            },
            Open::Tag {
                tag: Folded::new("relay"),
                carriers,
            },
        ]
    );
    assert_eq!(
        followup(&versions_of(&planned, own)[0]).topics,
        [topic_id("atlas")]
    );
}

#[test]
fn a_recheck_touching_no_topic_is_asked_about_and_the_file_answers() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    recheck(&old, &port, "touching ghost", DAY_3);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(
        opens(&planned),
        [Open::Touching {
            document: port_key(),
            topic: "ghost".to_owned(),
        }]
    );
    assert!(versions_of(&planned, PORT)[0].record.is_some());
    assert_eq!(versions_of(&planned, PORT)[1].record, None);

    let file = |touching: &str| {
        answers(&format!(
            "{TAGS}[documents.\"followup/{PORT}\"]\ntouching = \"{touching}\"\n"
        ))
    };
    let decisions = file("Lantern");
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), [Key::tag("lamp"), Key::tag("maps")]);
    let touched = followup(&versions_of(&planned, PORT)[1]);
    assert_eq!(
        touched.trigger,
        Some(Trigger::Touching(topic_id("lantern")))
    );
    assert_eq!(touched.topics, [topic_id("atlas"), topic_id("lantern")]);

    let planned = plan(&store, &file(""));
    assert_eq!(planned.questions, []);
    let untouched = followup(&versions_of(&planned, PORT)[1]);
    assert_eq!(untouched.trigger, None);
    assert_eq!(untouched.topics, [topic_id("atlas")]);

    let planned = plan(&store, &file("spirit"));
    assert_eq!(
        opens(&planned),
        [Open::Answer {
            key: Key::document(&port_key(), DocumentField::Touching),
            why: "spirit names no topic".to_owned(),
        }]
    );
}

#[test]
fn a_recheck_touching_an_ended_topic_is_asked_about_on_an_open_followup() {
    let (old, entry) = with_entry();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_5));
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    recheck(&old, &port, "touching phone", DAY_3);
    let closed = "2026-09-02-wire";
    let wired = old.followup(closed, &entry, "Wire it", &["relay"], desk(DAY_2));
    let touching = recheck(&old, &wired, "touching phone", DAY_3);
    old.done(&touching, "Wired", desk(DAY_4));
    let moved_on = "2026-09-03-solder";
    let solder = old.followup(moved_on, &entry, "Solder it", &["relay"], desk(DAY_2));
    let touching = recheck(&old, &solder, "touching phone", DAY_3);
    recheck(&old, &touching, "touching atlas", DAY_4);
    let store = read(&old);
    let file = format!("{ENDED_PHONE}{TAGS}");
    let planned = plan(&store, &answers(&file));

    assert_eq!(
        opens(&planned),
        [Open::EndedTopic {
            document: port_key(),
            through: Through::Touching,
            topic: "phone".to_owned(),
        }]
    );
    assert_eq!(versions_of(&planned, PORT)[1].record, None);
    let phone = Some(Trigger::Touching(topic_id("phone")));
    let kept = followup(&versions_of(&planned, closed)[2]);
    assert_eq!(kept.trigger, phone);
    assert_eq!(kept.topics, [topic_id("atlas"), topic_id("phone")]);
    assert_eq!(
        ending(&versions_of(&planned, closed)[2]).unwrap().reason,
        Reason::Done
    );
    assert_eq!(followup(&versions_of(&planned, moved_on)[1]).trigger, phone);

    let answered = |touching: &str| {
        answers(&format!(
            "{file}[documents.\"followup/{PORT}\"]\ntouching = \"{touching}\"\n"
        ))
    };
    let planned = plan(&store, &answered("lantern"));
    assert_eq!(planned.questions, []);
    assert_eq!(
        followup(&versions_of(&planned, PORT)[1]).trigger,
        Some(Trigger::Touching(topic_id("lantern")))
    );

    let planned = plan(&store, &answered("phone"));
    assert_eq!(
        opens(&planned),
        [Open::Answer {
            key: Key::document(&port_key(), DocumentField::Touching),
            why: "phone is ended".to_owned(),
        }]
    );
}

#[test]
fn a_touching_answered_away_from_an_ended_topic_stands_in_every_version_that_stores_it() {
    let (old, entry) = with_entry();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    let handset = old.topic("handset", "Another phone", &[], desk(DAY_1));
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let other = recheck(&old, &port, "touching handset", DAY_2);
    let first = recheck(&old, &other, "touching phone", DAY_3);
    let saved = old.saved(&first, "Port the driver again", "\n", desk(DAY_4));
    old.changed(
        &saved,
        "save",
        &[("tags", list(&["relay", "maps"]))],
        desk(DAY_4),
    );
    old.removed(&phone, "Sold", desk(DAY_5));
    old.removed(&handset, "Sold too", desk(DAY_5));
    let store = read(&old);
    let file = format!("{ENDED_PHONE}[documents.\"topic/handset\"]\nended = \"retired\"\n{TAGS}");
    let answered = |touching: &str| {
        answers(&format!(
            "{file}[documents.\"followup/{PORT}\"]\ntouching = \"{touching}\"\n"
        ))
    };

    let planned = plan(&store, &answered("lantern"));
    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    let handset = followup(&versions[1]);
    assert_eq!(
        handset.trigger,
        Some(Trigger::Touching(topic_id("handset")))
    );
    assert_eq!(handset.topics, [topic_id("atlas"), topic_id("handset")]);
    for version in &versions[2..] {
        let touching = followup(version);
        let lantern = Some(Trigger::Touching(topic_id("lantern")));
        assert_eq!(touching.trigger, lantern, "{}", version.key);
        assert_eq!(touching.topics, [topic_id("atlas"), topic_id("lantern")]);
    }
    assert_eq!(versions.len(), 5);

    let planned = plan(&store, &answered(""));
    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    assert_eq!(followup(&versions[1]).trigger, handset.trigger);
    for version in &versions[2..] {
        assert_eq!(followup(version).trigger, None, "{}", version.key);
        assert_eq!(followup(version).topics, [topic_id("atlas")]);
    }
}

#[test]
fn the_entry_of_a_followup_is_resolved_through_its_renames_or_answered() {
    let (old, entry) = with_entry();
    let (_, moved) = old.renamed(&entry, "2026-09/2026-09-04-lamp", desk(DAY_2));
    old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let after = "2026-09-02-wire";
    old.followup(after, &moved, "Wire it", &["relay"], desk(DAY_3));
    let elsewhere = OldStore::new();
    let gone = "2026-09/2026-09-03-gone";
    let unheld = elsewhere.entry(gone, "Never synced", &[], desk(DAY_1));
    let lost = "2026-09-03-solder";
    old.followup(lost, &unheld, "Solder it", &["relay"], desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(
        opens(&planned),
        [Open::Entry {
            followup: key(Kind::Followup, lost),
            entry: gone.to_owned(),
        }]
    );
    let lamp = Some(document_id(Kind::Entry, LAMP_DRIVER));
    assert_eq!(followup(&versions_of(&planned, PORT)[0]).entry, lamp);
    assert_eq!(followup(&versions_of(&planned, after)[0]).entry, lamp);
    assert_eq!(versions_of(&planned, lost)[0].record, None);

    let file = |entry: &str| {
        answers(&format!(
            "{TAGS}[documents.\"followup/{lost}\"]\nentry = \"{entry}\"\n"
        ))
    };
    let planned = plan(&store, &file(""));
    assert_eq!(planned.questions, []);
    assert_eq!(followup(&versions_of(&planned, lost)[0]).entry, None);

    let planned = plan(&store, &file("2026-09/2026-09-04-lamp"));
    assert_eq!(planned.questions, []);
    assert_eq!(followup(&versions_of(&planned, lost)[0]).entry, lamp);

    let planned = plan(&store, &file("2026-09/2026-09-09-ghost"));
    assert_eq!(
        opens(&planned),
        [Open::Answer {
            key: Key::document(&key(Kind::Followup, lost), DocumentField::Entry),
            why: "2026-09/2026-09-09-ghost names no entry".to_owned(),
        }]
    );
}

fn closed_on(reason: Reason, on: &str, note: &str) -> Ending {
    Ending {
        reason,
        on: date(on),
        by: None,
        note: note.to_owned(),
    }
}

fn ended(version: &Version) -> Ending {
    ending(version).unwrap()
}

fn open_with_body(old: &OldStore, entry: &Stored, slug: &str, body: &str) -> Stored {
    let first = old.followup(slug, entry, "Port the driver", &["relay"], desk(DAY_1));
    old.saved(&first, "Port the driver", body, desk(DAY_2))
}

#[test]
fn a_closed_followup_ends_with_the_note_its_closing_added() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    old.done(&port, "  Ported on the bench ", desk(DAY_3));
    let wire = open_with_body(&old, &entry, "2026-09-02-wire", "\nNotes so far.\n");
    old.dropped(&wire, "No longer needed\n\nThe board changed", desk(DAY_4));
    let plain = old.followup(
        "2026-09-03-solder",
        &entry,
        "Solder",
        &["relay"],
        desk(DAY_2),
    );
    old.changed(&plain, "done", &[("state", scalar("done"))], desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    assert_eq!(ending(&versions[0]), None);
    assert_eq!(
        ended(&versions[1]),
        closed_on(Reason::Done, "2026-09-06", "Ported on the bench")
    );
    assert_eq!(versions[1].body, "\n");
    assert_eq!(versions[1].change, "done");
    assert_eq!(followup(&versions[1]), followup(&versions[0]));
    let versions = versions_of(&planned, "2026-09-02-wire");
    assert_eq!(
        ended(&versions[2]),
        closed_on(
            Reason::Dropped,
            "2026-09-08",
            "No longer needed\n\nThe board changed"
        )
    );
    assert_eq!(versions[2].body, "\nNotes so far.\n");
    assert_eq!(versions[2].change, "dropped");
    let versions = versions_of(&planned, "2026-09-03-solder");
    assert_eq!(
        ended(&versions[1]),
        closed_on(Reason::Done, "2026-09-06", "")
    );
    assert_eq!(versions[1].body, "\n");
}

#[test]
fn a_closing_that_did_not_append_to_the_body_has_no_note() {
    let (old, entry) = with_entry();
    let done = [("state", scalar("done"))];
    let close = |parent: &Stored, body: &str| {
        let closed = following(parent, desk(DAY_3), "done", &done, Some(body));
        old.write(&closed);
    };
    let edited = open_with_body(&old, &entry, PORT, "\nNotes so far.\n");
    close(&edited, "\nRewritten.\n");
    let empty = open_with_body(&old, &entry, "2026-09-02-wire", "");
    close(&empty, "Wired on the bench\n");
    let unended = open_with_body(&old, &entry, "2026-09-03-solder", "\nNotes so far.\n");
    close(&unended, "\nNotes so far.\nSoldered");
    let spaced = open_with_body(&old, &entry, "2026-09-04-glue", "\nNotes so far.\n");
    close(&spaced, "\nNotes so far.\n\nGlued\n");
    let blank = open_with_body(&old, &entry, "2026-09-05-paint", "\nNotes so far.\n");
    close(&blank, "\nNotes so far.\n\n");
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let last = |origin: &str| versions_of(&planned, origin).last().unwrap().clone();
    let none = closed_on(Reason::Done, "2026-09-06", "");
    assert_eq!(ended(&last(PORT)), none);
    assert_eq!(last(PORT).body, "\nRewritten.\n");
    assert_eq!(
        ended(&last("2026-09-02-wire")),
        closed_on(Reason::Done, "2026-09-06", "Wired on the bench")
    );
    assert_eq!(last("2026-09-02-wire").body, "");
    assert_eq!(ended(&last("2026-09-03-solder")), none);
    assert_eq!(last("2026-09-03-solder").body, "\nNotes so far.\nSoldered");
    assert_eq!(ended(&last("2026-09-04-glue")), none);
    assert_eq!(last("2026-09-04-glue").body, "\nNotes so far.\n\nGlued\n");
    assert_eq!(ended(&last("2026-09-05-paint")), none);
    assert_eq!(last("2026-09-05-paint").body, "\nNotes so far.\n\n");
}

#[test]
fn a_version_after_the_closing_keeps_the_ending() {
    let (old, entry) = with_entry();
    let port = open_with_body(&old, &entry, PORT, "\nNotes so far.\n");
    let done = old.done(&port, "Ported", desk(DAY_3));
    let same = old.saved(&done, "Port the driver, at last", &done.body, desk(DAY_4));
    old.saved(
        &same,
        "Port the driver",
        "\nNotes so far.\nPorted\nTwice.\n",
        desk(DAY_5),
    );
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    let ported = Some(closed_on(Reason::Done, "2026-09-06", "Ported"));
    let endings: Vec<Option<Ending>> = versions.iter().map(ending).collect();
    assert_eq!(
        endings,
        [None, None, ported.clone(), ported.clone(), ported]
    );
    let bodies: Vec<&str> = versions
        .iter()
        .map(|version| version.body.as_str())
        .collect();
    assert_eq!(
        bodies,
        [
            "\n",
            "\nNotes so far.\n",
            "\nNotes so far.\n",
            "\nNotes so far.\n",
            "\nNotes so far.\nPorted\nTwice.\n",
        ]
    );
    assert_eq!(followup(&versions[3]).summary, "Port the driver, at last");
    assert_eq!(versions[3].change, "save");
}

#[test]
fn a_removed_followup_ends_for_the_reason_the_file_gives() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    old.removed(&port, "Opened twice", desk(DAY_3));
    let wire = "2026-09-02-wire";
    let wired = old.followup(wire, &entry, "Wire it", &["relay"], desk(DAY_2));
    let done = old.done(&wired, "Wired", desk(DAY_3));
    old.removed(&done, "Was never ours", desk(DAY_4));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    let asked = |slug: &str, note: &str, already: Option<&str>| Open::Ended {
        document: key(Kind::Followup, slug),
        idea: false,
        note: note.to_owned(),
        already: already.map(str::to_owned),
    };
    assert_eq!(
        opens(&planned),
        [
            asked(PORT, "Opened twice", None),
            asked(wire, "Was never ours", Some("done on 2026-09-06")),
        ]
    );
    assert_eq!(versions_of(&planned, PORT)[1].record, None);
    assert_eq!(
        ended(&versions_of(&planned, wire)[1]),
        closed_on(Reason::Done, "2026-09-06", "Wired")
    );
    assert_eq!(versions_of(&planned, wire)[2].record, None);

    let untagged = plan(&store, &Decisions::default());
    let reasons: Vec<Open> = (opens(&untagged).into_iter())
        .filter(|open| matches!(open, Open::Ended { .. }))
        .collect();
    assert_eq!(
        reasons,
        [
            asked(PORT, "Opened twice", None),
            asked(wire, "Was never ours", Some("done")),
        ]
    );

    let decisions = answers(&format!(
        "{TAGS}[documents.\"followup/{PORT}\"]\nended = \"dropped\"\n\
         [documents.\"followup/{wire}\"]\nended = \"dropped\"\n"
    ));
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    assert_eq!(
        ended(&versions[1]),
        closed_on(Reason::Dropped, "2026-09-06", "Opened twice")
    );
    assert_eq!(followup(&versions[1]), followup(&versions[0]));
    assert_eq!(versions[1].body, "\n");
    assert_eq!(versions[1].change, "dropped");
    let versions = versions_of(&planned, wire);
    assert_eq!(
        ended(&versions[1]),
        closed_on(Reason::Done, "2026-09-06", "Wired")
    );
    assert_eq!(
        ended(&versions[2]),
        closed_on(Reason::Dropped, "2026-09-08", "Was never ours")
    );
    assert_eq!(versions[2].body, "\n");
}

#[test]
fn what_stands_before_a_followup_is_asked_as_for_any_document() {
    let (old, entry) = with_entry();
    let port = old.followup(
        PORT,
        &entry,
        "Port the driver",
        &["relay"],
        at("soon", "phone"),
    );
    let with = |parent: &Stored, written: &str, key: &str, value: &str| {
        let mut fields = parent.fields.clone();
        fields.set(key, scalar(value));
        let version = composed(PORT, desk(written), "save", &[parent], fields, "\n");
        old.write(&version);
        version
    };
    let colored = with(&port, DAY_2, "color", "amber");
    let paused = with(&port, DAY_3, "state", "paused");
    let vague = with(&port, DAY_4, "recheck", "soon");
    let never = with(&port, DAY_5, "recheck", "2026-02-31 The board ships");
    let undated = "2026-02-31-wire";
    let wired = old.followup(undated, &entry, "Wire it", &["relay"], desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    let refused =
        |slug: &str, version: &Stored, field: &str, value: &str, why: &str| Blocked::Value {
            document: key(Kind::Followup, slug),
            version: version.id.to_string(),
            field: field.to_owned(),
            value: value.to_owned(),
            why: why.to_owned(),
        };
    let mut expected = vec![
        Blocked::WriteTime {
            document: port_key(),
            version: port.id.to_string(),
            text: "soon".to_owned(),
        },
        Blocked::ForeignKeys {
            document: port_key(),
            version: colored.id.to_string(),
            keys: vec!["color".to_owned()],
        },
        refused(
            PORT,
            &paused,
            "state",
            "paused",
            "is not open, done or dropped",
        ),
        refused(
            PORT,
            &vague,
            "recheck",
            "soon",
            "is neither a day with a reason nor `touching` with a topic",
        ),
        refused(
            PORT,
            &never,
            "recheck",
            "2026-02-31 The board ships",
            "does not open with a day",
        ),
        refused(
            undated,
            &wired,
            "slug",
            "2026-02-31-wire",
            "opens with no calendar day",
        ),
    ];
    expected.sort();
    let mut blocked = blocks(&planned);
    blocked.retain(|blocked| !matches!(blocked, Blocked::Forked { .. }));
    assert_eq!(blocked, expected);
    assert_eq!(
        opens(&planned),
        [Open::Machine {
            name: "phone".to_owned(),
            versions: 1,
            documents: Vec::new(),
        }]
    );
    for version in versions_of(&planned, PORT) {
        let stored = version.origin.source().unwrap().version;
        let read = [&port, &colored]
            .iter()
            .any(|held| held.id.to_string() == stored.id);
        assert_eq!(version.record.is_some(), read, "{}", version.key);
    }
    assert_eq!(versions_of(&planned, PORT)[0].stamp, None);
    assert_eq!(versions_of(&planned, undated)[0].record, None);
}

#[test]
fn a_tag_answered_with_an_ended_topic_is_asked_about_on_an_open_followup() {
    let (old, entry) = with_entry();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_5));
    old.followup(PORT, &entry, "Port the driver", &["calls"], desk(DAY_2));
    let closed = "2026-09-02-wire";
    let wired = old.followup(closed, &entry, "Wire it", &["calls"], desk(DAY_2));
    old.done(&wired, "Wired", desk(DAY_3));
    let earlier = "2026-09-03-solder";
    let solder = old.followup(earlier, &entry, "Solder it", &["calls"], desk(DAY_2));
    old.changed(&solder, "save", &[("tags", list(&["relay"]))], desk(DAY_3));
    let store = read(&old);
    let file = format!("{TAGS}calls = \"phone\"\n{ENDED_PHONE}");
    let planned = plan(&store, &answers(&file));

    assert_eq!(
        opens(&planned),
        [Open::EndedTopic {
            document: port_key(),
            through: Through::Tag(Folded::new("calls")),
            topic: "phone".to_owned(),
        }]
    );
    assert_eq!(versions_of(&planned, PORT)[0].record, None);
    assert_eq!(
        followup(&versions_of(&planned, closed)[1]).topics,
        [topic_id("phone")]
    );
    assert_eq!(
        followup(&versions_of(&planned, earlier)[0]).topics,
        [topic_id("phone")]
    );

    let listed = format!("{file}[documents.\"followup/{PORT}\"]\ntopics = [\"phone\"]\n");
    let planned = plan(&store, &answers(&listed));
    assert_eq!(
        opens(&planned),
        [Open::Answer {
            key: Key::document(&port_key(), DocumentField::Topics),
            why: "phone is ended".to_owned(),
        }]
    );
}

fn written(
    old: &OldStore,
    parent: &Stored,
    operation: &str,
    state: &str,
    body: &str,
    day: &str,
) -> Stored {
    let changes = [("state", scalar(state))];
    let version = following(parent, desk(day), operation, &changes, Some(body));
    old.write(&version);
    version
}

#[test]
fn a_note_is_found_after_a_body_with_no_final_line_break() {
    let (old, entry) = with_entry();
    let port = open_with_body(&old, &entry, PORT, "\nNotes so far");
    old.done(&port, "Ported", desk(DAY_3));
    let glued = open_with_body(&old, &entry, "2026-09-02-wire", "\nNotes so far");
    written(&old, &glued, "done", "done", "\nNotes so farWired\n", DAY_3);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let closed = versions_of(&planned, PORT).last().unwrap().clone();
    assert_eq!(
        ended(&closed),
        closed_on(Reason::Done, "2026-09-06", "Ported")
    );
    assert_eq!(closed.body, "\nNotes so far");
    let glued = versions_of(&planned, "2026-09-02-wire")
        .last()
        .unwrap()
        .clone();
    assert_eq!(ended(&glued), closed_on(Reason::Done, "2026-09-06", ""));
    assert_eq!(glued.body, "\nNotes so farWired\n");
}

#[test]
fn a_followup_saved_open_after_its_closing_is_not_ended() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let done = old.done(&port, "Ported", desk(DAY_3));
    let reopened = written(&old, &done, "save", "open", &done.body, DAY_4);
    old.dropped(&reopened, "Not needed", desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    assert_eq!(
        ended(&versions[1]),
        closed_on(Reason::Done, "2026-09-06", "Ported")
    );
    assert_eq!(versions[1].body, "\n");
    assert_eq!(ending(&versions[2]), None);
    assert_eq!(versions[2].body, "\nPorted\n");
    assert_eq!(versions[2].change, "save");
    assert_eq!(
        ended(&versions[3]),
        closed_on(Reason::Dropped, "2026-09-10", "Not needed")
    );
    assert_eq!(versions[3].body, "\nPorted\n");
}

#[test]
fn a_followup_closed_again_for_the_other_reason_keeps_its_note_and_its_body() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let done = old.done(&port, "Ported", desk(DAY_3));
    written(&old, &done, "save", "dropped", &done.body, DAY_4);
    let wire = "2026-09-02-wire";
    let wired = old.followup(wire, &entry, "Wire it", &["relay"], desk(DAY_2));
    let done = old.done(&wired, "Wired", desk(DAY_3));
    written(
        &old,
        &done,
        "drop",
        "dropped",
        "\nWired\nNot needed\n",
        DAY_4,
    );
    let solder = "2026-09-03-solder";
    let soldered = old.followup(solder, &entry, "Solder it", &["relay"], desk(DAY_2));
    let done = old.done(&soldered, "Soldered", desk(DAY_3));
    written(&old, &done, "save", "dropped", "\nSoldered\nBadly\n", DAY_4);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let last = |origin: &str| versions_of(&planned, origin)[2].clone();
    assert_eq!(
        ended(&last(PORT)),
        closed_on(Reason::Dropped, "2026-09-08", "Ported")
    );
    assert_eq!(last(PORT).body, "\n");
    assert_eq!(
        ended(&last(wire)),
        closed_on(Reason::Dropped, "2026-09-08", "Not needed")
    );
    assert_eq!(last(wire).body, "\n");
    assert_eq!(
        ended(&last(solder)),
        closed_on(Reason::Dropped, "2026-09-08", "Soldered")
    );
    assert_eq!(last(solder).body, "\nSoldered\nBadly\n");
}

#[test]
fn a_save_that_set_the_state_ends_the_followup_with_no_note() {
    let (old, entry) = with_entry();
    let port = open_with_body(&old, &entry, PORT, "\nNotes so far.\n");
    written(
        &old,
        &port,
        "save",
        "done",
        "\nNotes so far.\nPorted\n",
        DAY_3,
    );
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let closed = versions_of(&planned, PORT).last().unwrap().clone();
    assert_eq!(ended(&closed), closed_on(Reason::Done, "2026-09-06", ""));
    assert_eq!(closed.body, "\nNotes so far.\nPorted\n");
}

#[test]
fn a_renamed_followup_was_created_on_the_day_of_its_first_slug() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let done = old.done(&port, "Ported", desk(DAY_3));
    let (_, moved) = old.renamed(&done, "2026-09-09-port-driver", desk(DAY_4));
    old.saved(&moved, "Port the whole driver", &moved.body, desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    assert_eq!(versions.len(), 4);
    for version in versions {
        assert_eq!(
            followup(version).created,
            date("2026-09-01"),
            "{}",
            version.key
        );
    }
    let ported = closed_on(Reason::Done, "2026-09-06", "Ported");
    assert_eq!(ended(&versions[2]), ported);
    assert_eq!(ended(&versions[3]), ported);
    assert_eq!(versions[2].change, "rename");
    assert_eq!(versions[3].body, "\n");
}

#[test]
fn a_followup_of_a_removed_entry_still_names_it() {
    let (old, entry) = with_entry();
    old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    old.removed(&entry, "Never happened", desk(DAY_3));
    let store = read(&old);
    let file = format!("{TAGS}[documents.\"entry/{LAMP_DRIVER}\"]\nended = \"removed\"\n");
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(
        followup(&versions_of(&planned, PORT)[0]).entry,
        Some(document_id(Kind::Entry, LAMP_DRIVER))
    );
}

fn resolved(old: &OldStore, parents: &[&Stored], state: &str, body: &str) -> Stored {
    let mut fields = parents[0].fields.clone();
    fields.set("state", scalar(state));
    let slug = parents[0].slug.path();
    let both = composed(slug, desk(DAY_5), "resolve", parents, fields, body);
    old.write(&both);
    both
}

#[test]
fn a_version_that_joins_a_fork_ends_the_followup_as_its_own_state_says() {
    let (old, entry) = with_entry();
    let fork = |slug: &str| old.followup(slug, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let port = fork(PORT);
    let done = old.done(&port, "Ported", desk(DAY_3));
    let edited = old.saved(&port, "Port the whole driver", "\nNotes.\n", desk(DAY_4));
    resolved(&old, &[&done, &edited], "done", &done.body);
    let wire = "2026-09-02-wire";
    let wired = fork(wire);
    let done = old.done(&wired, "Wired", desk(DAY_3));
    let dropped = old.dropped(&wired, "Not needed", desk(DAY_4));
    resolved(&old, &[&done, &dropped], "dropped", "\nWired\nNot needed\n");
    let solder = "2026-09-03-solder";
    let soldered = fork(solder);
    let once = old.done(&soldered, "Soldered", desk(DAY_3));
    let twice = old.done(&soldered, "Soldered again", desk(DAY_4));
    resolved(&old, &[&once, &twice], "done", &twice.body);
    let glue = "2026-09-04-glue";
    let glued = fork(glue);
    let done = old.done(&glued, "Glued", desk(DAY_3));
    let edited = old.saved(&glued, "Glue it all", "\n", desk(DAY_4));
    resolved(&old, &[&done, &edited], "open", &done.body);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let last = |origin: &str| versions_of(&planned, origin)[3].clone();
    assert_eq!(
        ended(&last(PORT)),
        closed_on(Reason::Done, "2026-09-06", "Ported")
    );
    assert_eq!(last(PORT).body, "\n");
    assert_eq!(last(PORT).parents.len(), 2);
    assert_eq!(
        ended(&last(wire)),
        closed_on(Reason::Dropped, "2026-09-08", "Not needed")
    );
    assert_eq!(last(wire).body, "\nWired\nNot needed\n");
    assert_eq!(
        ended(&last(solder)),
        closed_on(Reason::Done, "2026-09-10", "")
    );
    assert_eq!(last(solder).body, "\nSoldered again\n");
    let earlier = versions_of(&planned, solder);
    let notes: Vec<String> = (earlier[1..3].iter())
        .map(|version| ended(version).note)
        .collect();
    assert_eq!(notes.len(), 2);
    assert!(notes.contains(&"Soldered".to_owned()));
    assert!(notes.contains(&"Soldered again".to_owned()));
    assert_eq!(ending(&last(glue)), None);
    assert_eq!(last(glue).body, "\nGlued\n");
}

#[test]
fn a_fork_joined_closed_with_the_body_of_its_open_side_keeps_that_body_and_the_closing() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let done = old.done(&port, "Ported", desk(DAY_3));
    let edited = old.saved(&port, "Port the whole driver", "\nNotes.\n", desk(DAY_4));
    resolved(&old, &[&done, &edited], "done", &edited.body);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let last = versions_of(&planned, PORT)[3].clone();
    assert_eq!(
        ended(&last),
        closed_on(Reason::Done, "2026-09-06", "Ported")
    );
    assert_eq!(last.body, "\nNotes.\n");
}

#[test]
fn a_version_that_joins_one_ending_carried_down_both_sides_keeps_it() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let done = old.done(&port, "Ported", desk(DAY_3));
    let (one, other) = old.forked(&done, desk(DAY_4), desk(DAY_4));
    resolved(&old, &[&one, &other], "done", &one.body);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let last = versions_of(&planned, PORT)[4].clone();
    assert_eq!(
        ended(&last),
        closed_on(Reason::Done, "2026-09-06", "Ported")
    );
    assert_eq!(last.body, versions_of(&planned, PORT)[2].body);
}

#[test]
fn a_closing_that_follows_no_one_version_ends_the_followup_on_its_own_day() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let (one, other) = old.forked(&port, desk(DAY_3), desk(DAY_4));
    let mut fields = one.fields.clone();
    fields.set("state", scalar("done"));
    let both = composed(
        PORT,
        desk(DAY_5),
        "done",
        &[&one, &other],
        fields.clone(),
        "\nOne.\nPorted\n",
    );
    old.write(&both);
    fields.set("state", scalar("dropped"));
    let alone = composed("2026-09-02-wire", desk(DAY_5), "drop", &[], fields, "\n");
    old.write(&alone);
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let joined = versions_of(&planned, PORT)[3].clone();
    assert_eq!(ended(&joined), closed_on(Reason::Done, "2026-09-10", ""));
    assert_eq!(joined.body, "\nOne.\nPorted\n");
    let alone = versions_of(&planned, "2026-09-02-wire")[0].clone();
    assert_eq!(ended(&alone), closed_on(Reason::Dropped, "2026-09-10", ""));
    assert_eq!(alone.body, "\n");
}

#[test]
fn a_version_that_joins_a_removed_side_is_planned_as_the_store_holds_it() {
    let (old, entry) = with_entry();
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_2));
    let gone = old.removed(&port, "Duplicate", desk(DAY_3));
    let edited = old.saved(&port, "Port the whole driver", "\nNotes.\n", desk(DAY_4));
    let back = resolved(&old, &[&gone, &edited], "open", "\nNotes.\n");
    let lamp = old.topic("lamp", "A lamp", &[], desk(DAY_1));
    let sold = old.removed(&lamp, "Sold", desk(DAY_3));
    let kept = old.saved(&lamp, "A lamp on a desk", "\n", desk(DAY_4));
    let both = composed(
        "lamp",
        desk(DAY_5),
        "resolve",
        &[&sold, &kept],
        kept.fields.clone(),
        "\nKept.\n",
    );
    old.write(&both);
    let store = read(&old);
    let file = format!(
        "{TAGS}[documents.\"followup/{PORT}\"]\nended = \"dropped\"\n\
         [documents.\"topic/lamp\"]\nended = \"retired\"\n"
    );
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    let versions = versions_of(&planned, PORT);
    let removed = versions
        .iter()
        .find(|version| version.key == gone.id.to_string());
    assert_eq!(
        ended(removed.unwrap()),
        closed_on(Reason::Dropped, "2026-09-06", "Duplicate")
    );
    let last = versions.last().unwrap();
    assert_eq!(last.key, back.id.to_string());
    assert_eq!(last.parents.len(), 2);
    assert_eq!(ending(last), None);
    assert_eq!(last.body, "\nNotes.\n");
    let versions = &of(&planned, Kind::Topic, "lamp").versions;
    let removed = versions
        .iter()
        .find(|version| version.key == sold.id.to_string());
    assert_eq!(ended(removed.unwrap()).reason, Reason::Retired);
    let last = versions.last().unwrap();
    assert_eq!(last.key, both.id.to_string());
    assert_eq!(last.parents.len(), 2);
    assert_eq!(ending(last), None);
    assert_eq!(topic(last).summary, "A lamp on a desk");
    assert_eq!(last.body, "\nKept.\n");
}
