use worklog::domain::schema::Entry;

use super::*;

fn entry(version: &Version) -> Entry {
    match version.record.as_ref().map(|record| &record.content) {
        Some(Content::Entry(entry)) => entry.clone(),
        other => panic!("not an entry: {other:?}"),
    }
}

fn topics_of(planned: &Planned, origin: &str) -> Vec<Vec<DocumentId>> {
    let versions = &of(planned, Kind::Entry, origin).versions;
    versions
        .iter()
        .map(|version| entry(version).topics)
        .collect()
}

#[test]
fn an_entry_carries_its_fields_in_every_version() {
    let old = with_topics();
    let tags = ["maps", "Relay", "lantern", "rust", "RELAY"];
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &tags, desk(DAY_1));
    let touched = [
        ("files_touched", list(&["~/lantern/driver.c"])),
        ("tags", list(&["rust", "maps"])),
        ("machine", scalar("phone")),
    ];
    let saved = following(&first, desk(DAY_3), "save", &touched, Some("\nTwice.\n"));
    old.write(&saved);
    let store = read(&old);
    let tags =
        "[tags]\nmaps = \"atlas\"\nrelay = \"Lantern\"\nlantern = \"lantern\"\nrust = \"\"\n";
    let unnamed = plan(&store, &answers(tags));
    assert_eq!(
        opens(&unnamed),
        [Open::Machine {
            name: "phone".to_owned(),
            versions: 1,
            documents: vec![key(Kind::Entry, LAMP_DRIVER)],
        }]
    );
    let decisions = answers(&format!("{tags}[machines]\nphone = \"atlas\"\n"));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let document = of(&planned, Kind::Entry, LAMP_DRIVER);
    assert_eq!(document.kind, KindOf::Entry);
    let versions = &document.versions;
    assert_eq!(versions.len(), 2);
    assert_eq!(
        entry(&versions[0]),
        Entry {
            name: names(&["lamp-driver"]).remove(0),
            former_names: Vec::new(),
            date: date("2026-09-04"),
            machine: topic_id("desk"),
            topics: vec![topic_id("atlas"), topic_id("lantern")],
            summary: "Drove the lamp".to_owned(),
        }
    );
    assert_eq!(
        entry(&versions[1]),
        Entry {
            machine: topic_id("atlas"),
            topics: vec![topic_id("atlas")],
            ..entry(&versions[0])
        }
    );
    assert_eq!(versions[0].body, "\n## What\nDrove the lamp\n");
    assert_eq!(versions[1].body, "\nTwice.\n");
    assert_eq!(versions[0].change, "new");
    assert_eq!(versions[1].change, "save");
    assert_eq!(versions[1].key, saved.id.to_string());
    assert_eq!(versions[1].parents, [first.id.to_string()]);
    assert_eq!(versions[1].machine, Some(topic_id("desk")));
    assert_eq!(versions[1].stamp, Some(Stamp::parse(DAY_3).unwrap()));
    assert_eq!(ending(&versions[1]), None);
    let fields = versions[1].record.as_ref().unwrap().fields();
    assert!(!fields.contains_key("files_touched"));
    assert!(!fields.contains_key("tags"));
}

#[test]
fn a_tag_is_answered_for_the_document_before_the_store() {
    let old = with_topics();
    old.entry(
        LAMP_DRIVER,
        "Drove the lamp",
        &["relay", "maps"],
        desk(DAY_1),
    );
    let other = "2026-09/2026-09-05-wiring";
    old.entry(other, "Wired the lamp", &["Relay", "maps"], desk(DAY_2));
    let store = read(&old);
    let decisions = answers(
        "[tags]\nrelay = \"lantern\"\nmaps = \"atlas\"\n\
         [documents.\"entry/2026-09/2026-09-05-wiring\"]\ntags = { relay = \"atlas\", maps = \"\" }\n",
    );
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    assert_eq!(
        topics_of(&planned, LAMP_DRIVER),
        [[topic_id("lantern"), topic_id("atlas")]]
    );
    assert_eq!(topics_of(&planned, other), [[topic_id("atlas")]]);
}

#[test]
fn a_tag_is_answered_with_a_topic_the_file_creates() {
    let old = with_topics();
    old.entry(LAMP_DRIVER, "Drove the lamp", &["firmware"], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(&format!(
        "{CONVERSION}[tags]\nfirmware = \"Lantern-Firmware\"\n\
         [topics.lantern-firmware]\nsummary = \"The firmware of the lantern\"\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let created = by_id(&planned, &created_topic_id("lantern-firmware"));
    assert_eq!(created.kind, KindOf::Topic);
    assert_eq!(
        topics_of(&planned, LAMP_DRIVER),
        [[created_topic_id("lantern-firmware")]]
    );
}

#[test]
fn a_tag_answered_with_no_topic_is_asked_about_where_the_answer_is() {
    let old = with_topics();
    old.entry(LAMP_DRIVER, "Drove the lamp", &["relay"], desk(DAY_1));
    let other = "2026-09/2026-09-05-wiring";
    old.entry(other, "Wired the lamp", &["relay"], desk(DAY_2));
    let store = read(&old);
    let decisions = answers(
        "[tags]\nrelay = \"ghost\"\n\
         [documents.\"entry/2026-09/2026-09-05-wiring\"]\ntags.relay = \"spirit\"\n",
    );
    let planned = plan(&store, &decisions);

    let own = DocumentField::Tag(Folded::new("relay"));
    assert_eq!(
        opens(&planned),
        [
            Open::Answer {
                key: Key::tag("relay"),
                why: "ghost names no topic".to_owned(),
            },
            Open::Answer {
                key: Key::document(&key(Kind::Entry, other), own),
                why: "spirit names no topic".to_owned(),
            },
        ]
    );
    assert_eq!(
        of(&planned, Kind::Entry, LAMP_DRIVER).versions[0].record,
        None
    );
    assert_eq!(of(&planned, Kind::Entry, other).versions[0].record, None);
}

#[test]
fn an_entry_stays_filed_under_a_topic_that_ended() {
    let old = with_topics();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_4));
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &["calls"], desk(DAY_1));
    let both = [("tags", list(&["calls", "maps"]))];
    old.changed(&first, "save", &both, desk(DAY_2));
    let listed = "2026-09/2026-09-05-wiring";
    old.entry(listed, "Wired the lamp", &[], desk(DAY_2));
    let store = read(&old);
    let file = format!(
        "{ENDED_PHONE}[tags]\ncalls = \"phone\"\nmaps = \"atlas\"\n\
         [documents.\"entry/{listed}\"]\ntopics = [\"phone\"]\n"
    );
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(
        topics_of(&planned, LAMP_DRIVER),
        [
            vec![topic_id("phone")],
            vec![topic_id("phone"), topic_id("atlas")]
        ]
    );
    assert_eq!(topics_of(&planned, listed), [[topic_id("phone")]]);
}

#[test]
fn the_topics_of_the_file_replace_what_the_tags_give_in_every_version() {
    let old = with_topics();
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &["relay"], desk(DAY_1));
    old.changed(&first, "save", &[("tags", list(&["maps"]))], desk(DAY_2));
    let other = "2026-09/2026-09-05-wiring";
    old.entry(other, "Wired the lamp", &["relay"], desk(DAY_2));
    let store = read(&old);
    let file = |topics: &str, other_topics: &str| {
        answers(&format!(
            "[documents.\"entry/{LAMP_DRIVER}\"]\ntopics = {topics}\n\
             [documents.\"entry/{other}\"]\ntopics = {other_topics}\n"
        ))
    };
    let decisions = file("[\"Atlas\", \"lantern\"]", "[]");
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let both = vec![topic_id("atlas"), topic_id("lantern")];
    assert_eq!(topics_of(&planned, LAMP_DRIVER), [both.clone(), both]);
    assert_eq!(topics_of(&planned, other), [Vec::new()]);

    let planned = plan(&store, &file("[\"ghost\"]", "[]"));
    assert_eq!(
        opens(&planned),
        [Open::Answer {
            key: Key::document(&key(Kind::Entry, LAMP_DRIVER), DocumentField::Topics),
            why: "ghost names no topic".to_owned(),
        }]
    );
}

#[test]
fn a_renamed_entry_is_one_document_under_its_last_name() {
    let old = with_topics();
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &[], desk(DAY_1));
    let (tombstone, moved) = old.renamed(&first, "2026-09/2026-09-04-lamp", desk(DAY_2));
    old.saved(&moved, "Drove it", "\n", desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let entries: Vec<&Document> = (planned.documents.iter())
        .filter(|document| document.kind == KindOf::Entry)
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, document_id(Kind::Entry, LAMP_DRIVER));
    let versions = &entries[0].versions;
    assert_eq!(versions.len(), 3);
    assert_eq!(entry(&versions[0]).name, names(&["lamp-driver"])[0]);
    assert_eq!(entry(&versions[1]).name, names(&["lamp"])[0]);
    assert_eq!(entry(&versions[1]).former_names, names(&["lamp-driver"]));
    assert_eq!(entry(&versions[2]).former_names, names(&["lamp-driver"]));
    assert_eq!(versions[1].change, "rename");
    assert_eq!(versions[1].key, moved.id.to_string());
    assert_eq!(versions[1].parents, [first.id.to_string()]);
    assert_eq!(
        versions[1]
            .origin
            .source()
            .unwrap()
            .stone
            .unwrap()
            .version
            .id,
        tombstone.id.to_string()
    );
}

#[test]
fn an_entry_name_the_new_store_refuses_is_asked_about_and_the_file_answers() {
    let old = with_topics();
    let slug = "2026-09/2026-09-04-Lamp_Driver";
    old.entry(slug, "Drove the lamp", &[], desk(DAY_1));
    old.entry(
        "2026-09/2026-09-05-2026-09-04-again",
        "Again",
        &[],
        desk(DAY_2),
    );
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        opens(&planned),
        [
            Open::Name {
                document: key(Kind::Entry, slug),
                name: "Lamp_Driver".to_owned(),
                why: "holds an uppercase letter".to_owned(),
            },
            Open::Name {
                document: key(Kind::Entry, "2026-09/2026-09-05-2026-09-04-again"),
                name: "2026-09-04-again".to_owned(),
                why: "reads as a date or as a document id".to_owned(),
            },
        ]
    );
    assert_eq!(of(&planned, Kind::Entry, slug).versions[0].record, None);

    let decisions = answers(
        "[documents.\"entry/2026-09/2026-09-04-Lamp_Driver\"]\nname = \"lamp-driver\"\n\
         [documents.\"entry/2026-09/2026-09-05-2026-09-04-again\"]\nname = \"again\"\n",
    );
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    let version = &of(&planned, Kind::Entry, slug).versions[0];
    assert_eq!(entry(version).name, names(&["lamp-driver"])[0]);
}

#[test]
fn two_entries_of_one_date_with_one_name_are_asked_about() {
    let old = with_topics();
    old.entry(
        "2026-09/2026-09-04-lamp",
        "Drove the lamp",
        &[],
        desk(DAY_1),
    );
    old.entry(
        "2026-09/2026-09-04-Lamp",
        "Drove it again",
        &[],
        desk(DAY_1),
    );
    old.entry("2026-09/2026-09-05-lamp", "Another day", &[], desk(DAY_2));
    let gone = old.entry("2026-09/2026-09-04-LAMP", "Never", &[], desk(DAY_1));
    old.removed(&gone, "Never happened", desk(DAY_2));
    let store = read(&old);
    let file = "[documents.\"entry/2026-09/2026-09-04-Lamp\"]\nname = \"lamp\"\n\
                [documents.\"entry/2026-09/2026-09-04-LAMP\"]\nname = \"lamp\"\nended = \"removed\"\n";
    let planned = plan(&store, &answers(file));

    assert_eq!(
        opens(&planned),
        [
            Open::Name {
                document: key(Kind::Entry, "2026-09/2026-09-04-lamp"),
                name: "lamp".to_owned(),
                why: "is held by several documents".to_owned(),
            },
            Open::Answer {
                key: Key::document(
                    &key(Kind::Entry, "2026-09/2026-09-04-Lamp"),
                    DocumentField::Name
                ),
                why: "lamp is held by several documents".to_owned(),
            },
        ]
    );

    let renamed = format!(
        "{CONVERSION}{file}[documents.\"entry/2026-09/2026-09-04-lamp\"]\nname = \"lamp-first\"\n"
    );
    let planned = plan(&store, &answers(&renamed));
    assert_eq!(planned.questions, []);
    let name = |origin: &str| entry(&of(&planned, Kind::Entry, origin).versions[0]).name;
    assert_eq!(name("2026-09/2026-09-04-lamp"), names(&["lamp"])[0]);
    let first = of(&planned, Kind::Entry, "2026-09/2026-09-04-lamp");
    assert_eq!(first.versions.len(), 2);
    assert_eq!(first.versions[1].key, AUTHORED);
    assert_eq!(first.versions[1].change, "rename");
    assert_eq!(entry(&first.versions[1]).name, names(&["lamp-first"])[0]);
    assert_eq!(entry(&first.versions[1]).former_names, names(&["lamp"]));
    assert_eq!(name("2026-09/2026-09-04-Lamp"), names(&["lamp"])[0]);
    assert_eq!(name("2026-09/2026-09-05-lamp"), names(&["lamp"])[0]);
    assert_eq!(name("2026-09/2026-09-04-LAMP"), names(&["lamp"])[0]);
}

#[test]
fn a_list_of_topics_no_question_asked_for_is_returned_with_the_plan() {
    let old = with_topics();
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &["relay"], desk(DAY_1));
    old.changed(&first, "save", &[("tags", list(&["maps"]))], desk(DAY_2));
    let filed = "2026-09-01-port";
    old.followup(filed, &first, "Port the driver", &["relay"], desk(DAY_2));
    let untagged = "2026-09-02-wire";
    let wired = old.followup(untagged, &first, "Wire it", &[], desk(DAY_2));
    old.changed(&wired, "save", &[("tags", list(&["relay"]))], desk(DAY_3));
    let dropped = "2026-09-03-solder";
    old.followup(dropped, &first, "Solder it", &["rust"], desk(DAY_2));
    let touching = "2026-09-04-glue";
    let draft = OldStore::new().followup(touching, &first, "Glue it", &[], desk(DAY_2));
    let mut fields = draft.fields.clone();
    fields.set("recheck", scalar("touching atlas"));
    old.write(&composed(touching, desk(DAY_2), "new", &[], fields, "\n"));
    old.entry(
        "2026-09/2026-09-05-wiring",
        "Wired",
        &["relay"],
        desk(DAY_2),
    );
    let store = read(&old);
    let lists = |slugs: &[&str]| -> String {
        let table = |slug: &&str| format!("[documents.\"{slug}\"]\ntopics = [\"lantern\"]\n");
        slugs.iter().map(table).collect()
    };
    let every = [
        format!("entry/{LAMP_DRIVER}"),
        format!("followup/{filed}"),
        format!("followup/{untagged}"),
        format!("followup/{dropped}"),
        format!("followup/{touching}"),
    ];
    let every: Vec<&str> = every.iter().map(String::as_str).collect();
    let file = format!("[tags]\nrelay = \"atlas\"\nrust = \"\"\n{}", lists(&every));
    let decisions = answers(&file);
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, [Key::tag("rust")]);
    let listed = |document: &str| Overridden {
        document: document.to_owned(),
        topics: vec!["lantern".to_owned()],
    };
    assert_eq!(
        planned.overridden,
        [listed(every[0]), listed(every[1]), listed(every[4]),]
    );
    assert_eq!(
        topics_of(&planned, LAMP_DRIVER),
        [[topic_id("lantern")], [topic_id("lantern")]]
    );
}

#[test]
fn a_removed_entry_ends_for_the_reason_the_file_gives() {
    let old = with_topics();
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &[], desk(DAY_1));
    old.removed(&first, "Folded into the wiring", desk(DAY_3));
    let other = "2026-09/2026-09-05-wiring";
    old.entry(other, "Wired the lamp", &[], desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        opens(&planned),
        [Open::Ended {
            document: key(Kind::Entry, LAMP_DRIVER),
            idea: false,
            note: "Folded into the wiring".to_owned(),
            already: None,
        }]
    );
    assert_eq!(
        of(&planned, Kind::Entry, LAMP_DRIVER).versions[1].record,
        None
    );

    let decisions = answers(&format!(
        "[documents.\"entry/{LAMP_DRIVER}\"]\nended = \"merged\"\nended_by = \"entry/{other}\"\n"
    ));
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let versions = &of(&planned, Kind::Entry, LAMP_DRIVER).versions;
    assert_eq!(
        ending(&versions[1]),
        Some(Ending {
            reason: Reason::Merged,
            on: date("2026-09-06"),
            by: Some(document_id(Kind::Entry, other)),
            note: "Folded into the wiring".to_owned(),
        })
    );
    assert_eq!(entry(&versions[1]), entry(&versions[0]));
    assert_eq!(versions[1].body, versions[0].body);
    assert_eq!(versions[1].change, "merged");
}

#[test]
fn what_stands_before_an_entry_is_asked_as_for_any_document() {
    let old = with_topics();
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &[], at("yesterday", "phone"));
    let mut fields = first.fields.clone();
    fields.set("color", scalar("amber"));
    fields.set("date", scalar("2026-02-31"));
    let colored = composed(LAMP_DRIVER, desk(DAY_2), "save", &[&first], fields, "\n");
    old.write(&colored);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let document = key(Kind::Entry, LAMP_DRIVER);
    let mut expected = vec![
        Blocked::WriteTime {
            document: document.clone(),
            version: first.id.to_string(),
            text: "yesterday".to_owned(),
        },
        Blocked::ForeignKeys {
            document: document.clone(),
            version: colored.id.to_string(),
            keys: vec!["color".to_owned()],
        },
        Blocked::Value {
            document,
            version: colored.id.to_string(),
            field: "date".to_owned(),
            value: "2026-02-31".to_owned(),
            why: "is not a day".to_owned(),
        },
    ];
    expected.sort();
    assert_eq!(blocks(&planned), expected);
    assert_eq!(
        opens(&planned),
        [Open::Machine {
            name: "phone".to_owned(),
            versions: 2,
            documents: vec![key(Kind::Entry, LAMP_DRIVER)],
        }]
    );
    let versions = &of(&planned, Kind::Entry, LAMP_DRIVER).versions;
    assert_eq!(versions[0].stamp, None);
    assert_eq!(versions[0].machine, None);
    assert_eq!(versions[0].record, None);
    assert_eq!(versions[1].record, None);
    assert_eq!(versions[1].machine, Some(topic_id("desk")));
}

#[test]
fn a_field_the_store_requires_and_a_version_lacks_blocks() {
    let old = with_topics();
    let first = old.entry(LAMP_DRIVER, "Drove the lamp", &[], desk(DAY_1));
    let mut fields = worklog_old::domain::frontmatter::Fields::default();
    fields.set("summary", scalar("Drove the lamp"));
    let bare = composed(
        LAMP_DRIVER,
        desk(DAY_2),
        "save",
        &[&first],
        fields.clone(),
        "\n",
    );
    old.write(&bare);
    let port = composed("2026-09-01-port", desk(DAY_2), "new", &[], fields, "\n");
    old.write(&port);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let missing = |kind: Kind, slug: &str, version: &Stored, field: &str| Blocked::Value {
        document: key(kind, slug),
        version: version.id.to_string(),
        field: field.to_owned(),
        value: String::new(),
        why: "is missing, and the store requires it".to_owned(),
    };
    let mut expected = vec![
        missing(Kind::Entry, LAMP_DRIVER, &bare, "date"),
        missing(Kind::Entry, LAMP_DRIVER, &bare, "machine"),
        missing(Kind::Followup, "2026-09-01-port", &port, "entry"),
        missing(Kind::Followup, "2026-09-01-port", &port, "state"),
    ];
    expected.sort();
    assert_eq!(blocks(&planned), expected);
}
