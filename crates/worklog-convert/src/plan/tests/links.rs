use std::collections::BTreeMap;

use super::*;

struct Linking {
    old: OldStore,
    entry: Stored,
    port: Stored,
    pin: Stored,
}

fn linking() -> Linking {
    let old = with_topics();
    let entry = old.entry(LAMP_DRIVER, "Drove the lamp", &[], desk(DAY_1));
    let port = old.followup(PORT, &entry, "Port the driver", &["relay"], desk(DAY_1));
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    Linking {
        old,
        entry,
        port,
        pin,
    }
}

const TAGS: &str = "[tags]\nrelay = \"atlas\"\n";

fn table(rows: &[(&str, Option<DocumentId>)]) -> Links {
    let rows = rows
        .iter()
        .map(|(text, id)| ((*text).to_owned(), id.clone()));
    Links(rows.collect::<BTreeMap<_, _>>())
}

fn entry_id() -> DocumentId {
    document_id(Kind::Entry, LAMP_DRIVER)
}

fn pin_versions<'p, 'a>(planned: &'p Planned<'a>) -> &'p [Version<'a>] {
    &of(planned, Kind::Fact, PIN).versions
}

#[test]
fn a_link_to_an_entry_takes_the_name_the_entry_has_last() {
    let Linking {
        old, entry, pin, ..
    } = linking();
    let body = "\nSee [[2026-09/2026-09-04-lamp-driver]].\n";
    let linked = old.saved(&pin, "Pin 5", body, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = pin_versions(&planned);
    assert_eq!(versions[0].links, Links::default());
    assert_eq!(versions[1].body, "\nSee [[2026-09-04-lamp-driver]].\n");
    assert_eq!(
        versions[1].links,
        table(&[("2026-09-04-lamp-driver", Some(entry_id()))])
    );

    let (_, moved) = old.renamed(&entry, "2026-09/2026-09-04-Lamp_X", desk(DAY_3));
    let body = "\nSee [[2026-09/2026-09-04-lamp-driver]] or [[2026-09/2026-09-04-Lamp_X]].\n";
    old.saved(&linked, "Pin 5", body, desk(DAY_4));
    let store = read(&old);
    let file =
        format!("{TAGS}[documents.\"entry/2026-09/2026-09-04-Lamp_X\"]\nname = \"lamp-x\"\n");
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    let versions = pin_versions(&planned);
    let renamed = table(&[("2026-09-04-lamp-x", Some(entry_id()))]);
    assert_eq!(versions[1].body, "\nSee [[2026-09-04-lamp-x]].\n");
    assert_eq!(versions[1].links, renamed);
    assert_eq!(
        versions[2].body,
        "\nSee [[2026-09-04-lamp-x]] or [[2026-09-04-lamp-x]].\n"
    );
    assert_eq!(versions[2].links, renamed);
    assert_eq!(moved.slug.path(), "2026-09/2026-09-04-Lamp_X");

    let planned = plan(&store, &answers(TAGS));
    assert_eq!(opens(&planned).len(), 1);
    let versions = pin_versions(&planned);
    assert_eq!(
        versions[2].body,
        "\nSee [[2026-09/2026-09-04-lamp-driver]] or [[2026-09/2026-09-04-Lamp_X]].\n"
    );
    assert_eq!(
        versions[2].links,
        table(&[
            ("2026-09/2026-09-04-lamp-driver", Some(entry_id())),
            ("2026-09/2026-09-04-Lamp_X", Some(entry_id())),
        ])
    );
}

#[test]
fn a_link_to_a_followup_becomes_its_short_id() {
    let Linking { old, entry, .. } = linking();
    let body = "\nLeft [[2026-09-01-port]] open, twice: [[2026-09-01-port]].\n";
    old.saved(&entry, "Drove the lamp", body, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let id = document_id(Kind::Followup, PORT);
    let short = id.short().to_owned();
    assert_eq!(short.len(), 8);
    let versions = &of(&planned, Kind::Entry, LAMP_DRIVER).versions;
    assert_eq!(
        versions[1].body,
        format!("\nLeft [[{short}]] open, twice: [[{short}]].\n")
    );
    assert_eq!(versions[1].links, table(&[(&short, Some(id))]));
}

#[test]
fn a_link_to_a_fact_or_a_topic_stays_as_typed_and_names_the_document_it_named() {
    let Linking { old, port, pin, .. } = linking();
    let lamp = old.topic("lamp", "A small lamp", &[], desk(DAY_1));
    let (_, torch) = old.renamed(&lamp, "torch", desk(DAY_2));
    old.renamed(&pin, "atlas/relay", desk(DAY_2));
    let body = "\nSee [[lantern/relay-pin]], [[atlas/relay]], [[lamp]], [[torch]] and [[atlas]].\n";
    old.saved(&port, "Port the driver", body, desk(DAY_3));
    old.saved(
        &torch,
        "A torch, beside the [[lantern]]",
        "\n[[lamp]]\n",
        desk(DAY_3),
    );
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let fact = Some(document_id(Kind::Fact, PIN));
    let torch = Some(topic_id("lamp"));
    let versions = &of(&planned, Kind::Followup, PORT).versions;
    assert_eq!(versions[1].body, body);
    assert_eq!(
        versions[1].links,
        table(&[
            ("lantern/relay-pin", fact.clone()),
            ("atlas/relay", fact),
            ("lamp", torch.clone()),
            ("torch", torch.clone()),
            ("atlas", Some(topic_id("atlas"))),
        ])
    );
    let versions = &of(&planned, Kind::Topic, "lamp").versions;
    assert_eq!(
        topic(&versions[2]).summary,
        "A torch, beside the [[lantern]]"
    );
    assert_eq!(
        versions[2].links,
        table(&[("lantern", Some(topic_id("lantern"))), ("lamp", torch)])
    );
}

#[test]
fn a_link_in_a_code_span_is_no_link() {
    let Linking { old, pin, .. } = linking();
    let body =
        "\nWritten `[[2026-09/2026-09-04-lamp-driver]]`, as in `[[atlas]]`, by [[lantern]].\n";
    old.saved(&pin, "Pin 5", body, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = pin_versions(&planned);
    assert_eq!(versions[1].body, body);
    assert_eq!(
        versions[1].links,
        table(&[("lantern", Some(topic_id("lantern")))])
    );
}

#[test]
fn a_link_to_a_removed_document_names_that_document() {
    let Linking {
        old,
        entry,
        port,
        pin,
    } = linking();
    old.removed(&entry, "Never happened", desk(DAY_2));
    old.removed(&port, "Opened twice", desk(DAY_2));
    let body = "\nWas [[2026-09/2026-09-04-lamp-driver]], left [[2026-09-01-port]].\n";
    old.saved(&pin, "Pin 5", body, desk(DAY_3));
    let store = read(&old);
    let file = format!(
        "{TAGS}[documents.\"entry/{LAMP_DRIVER}\"]\nended = \"removed\"\n\
         [documents.\"followup/{PORT}\"]\nended = \"dropped\"\n"
    );
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    let port = document_id(Kind::Followup, PORT);
    let short = port.short().to_owned();
    let versions = pin_versions(&planned);
    assert_eq!(
        versions[1].body,
        format!("\nWas [[2026-09-04-lamp-driver]], left [[{short}]].\n")
    );
    assert_eq!(
        versions[1].links,
        table(&[
            ("2026-09-04-lamp-driver", Some(entry_id())),
            (&short, Some(port))
        ])
    );
}

#[test]
fn a_link_to_nothing_stays_as_typed_and_is_recorded_unresolved() {
    let Linking { old, pin, .. } = linking();
    let body = "\n[[2026-09/2026-09-09-ghost]] [[2026-09-09-ghost]] [[ghost/pin]] [[ghost]] \
                [[2026]] [[Atlas]] [[not a link]]\n";
    old.saved(&pin, "Pin 5", body, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = pin_versions(&planned);
    assert_eq!(versions[1].body, body);
    assert_eq!(
        versions[1].links,
        table(&[
            ("2026-09/2026-09-09-ghost", None),
            ("2026-09-09-ghost", None),
            ("ghost/pin", None),
            ("ghost", None),
            ("2026", None),
            ("Atlas", None),
        ])
    );
}

#[test]
fn a_link_in_a_summary_is_rewritten_on_its_one_line() {
    let Linking { old, pin, port, .. } = linking();
    let summary = "Pin 5, as [[2026-09/2026-09-04-lamp-driver]] found";
    old.saved(&pin, summary, "\n[[atlas]]\n", desk(DAY_2));
    old.done(&port, "See [[2026-09/2026-09-04-lamp-driver]]", desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let versions = pin_versions(&planned);
    assert_eq!(
        fact(&versions[1]).summary,
        "Pin 5, as [[2026-09-04-lamp-driver]] found"
    );
    assert_eq!(
        versions[1].links,
        table(&[
            ("2026-09-04-lamp-driver", Some(entry_id())),
            ("atlas", Some(topic_id("atlas"))),
        ])
    );
    let closed = &of(&planned, Kind::Followup, PORT).versions[1];
    assert_eq!(
        ending(closed).unwrap().note,
        "See [[2026-09-04-lamp-driver]]"
    );
    assert_eq!(closed.links, Links::default());
}

#[test]
fn a_link_in_a_note_or_in_a_recheck_is_rewritten_and_has_no_row() {
    let Linking {
        old,
        entry,
        port,
        pin,
    } = linking();
    let why = [(
        "recheck",
        scalar("2026-10-01 After [[2026-09/2026-09-04-lamp-driver]]"),
    )];
    let dated = old.changed(&port, "recheck", &why, desk(DAY_2));
    old.dropped(
        &dated,
        "Left to [[lantern/relay-pin]] and [[2026-09-01-port]]",
        desk(DAY_3),
    );
    old.removed(&entry, "Folded into [[2026-09-01-port]]", desk(DAY_3));
    old.removed(
        &pin,
        "See `[[2026-09-01-port]]` and [[2026-09-09-ghost]]",
        desk(DAY_3),
    );
    let store = read(&old);
    let file = format!(
        "{TAGS}[documents.\"entry/{LAMP_DRIVER}\"]\nended = \"removed\"\n\
         [documents.\"fact/{PIN}\"]\nended = \"false\"\n"
    );
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    let short = document_id(Kind::Followup, PORT).short().to_owned();
    let versions = &of(&planned, Kind::Followup, PORT).versions;
    let Some(Content::Followup(followup)) = versions[1].record.as_ref().map(|r| &r.content) else {
        panic!("not a followup");
    };
    assert_eq!(
        followup.trigger,
        Some(worklog::domain::schema::Trigger::LookAgain {
            on: date("2026-10-01"),
            why: "After [[2026-09-04-lamp-driver]]".to_owned(),
        })
    );
    assert_eq!(
        ending(&versions[2]).unwrap().note,
        format!("Left to [[lantern/relay-pin]] and [[{short}]]")
    );
    let gone = &of(&planned, Kind::Entry, LAMP_DRIVER).versions[1];
    assert_eq!(
        ending(gone).unwrap().note,
        format!("Folded into [[{short}]]")
    );
    let refuted = &pin_versions(&planned)[1];
    assert_eq!(
        ending(refuted).unwrap().note,
        "See `[[2026-09-01-port]]` and [[2026-09-09-ghost]]"
    );
    for version in [&versions[1], &versions[2], gone, refuted] {
        assert_eq!(version.links, Links::default(), "{}", version.key);
    }
}

#[test]
fn a_link_on_a_line_inside_a_fence_is_a_link_as_the_store_reads_it() {
    let Linking { old, pin, .. } = linking();
    let body = "\n```\nsee [[2026-09-01-port]]\n```\n";
    old.saved(&pin, "Pin 5", body, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(planned.questions, []);
    let port = document_id(Kind::Followup, PORT);
    let short = port.short().to_owned();
    let versions = pin_versions(&planned);
    assert_eq!(versions[1].body, format!("\n```\nsee [[{short}]]\n```\n"));
    assert_eq!(versions[1].links, table(&[(&short, Some(port))]));
}

#[test]
fn a_short_id_is_as_long_as_it_takes_to_name_one_planned_document() {
    let id = |text: &str| DocumentId::parse(text).unwrap();
    let port = id("b64f5a59aaaa00000000000000000001");
    let near = id("b64f5a59aab000000000000000000002");
    let far = id("c64f5a59aaaa00000000000000000003");
    let all = |ids: &[&DocumentId]| ids.iter().map(|id| (*id).clone()).collect();

    assert_eq!(
        super::super::links::short_among(&port, &all(&[&port, &far])),
        port.short()
    );
    assert_eq!(
        super::super::links::short_among(&port, &all(&[&port])),
        "b64f5a59"
    );
    assert_eq!(
        super::super::links::short_among(&port, &all(&[&port, &near, &far])),
        "b64f5a59aaa"
    );
    assert_eq!(
        super::super::links::short_among(&near, &all(&[&port, &near])),
        "b64f5a59aab"
    );
}

#[test]
fn two_links_that_become_one_text_for_two_documents_block() {
    let Linking { old, pin, .. } = linking();
    let body = "\n[[2026-09/2026-09-04-lamp-driver]] and [[2026-09-04-lamp-driver]]\n";
    let linked = old.saved(&pin, "Pin 5", body, desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(TAGS));

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::Value {
            document: key(Kind::Fact, PIN),
            version: linked.id.to_string(),
            field: "links".to_owned(),
            value: "[[2026-09-04-lamp-driver]]".to_owned(),
            why: "is what [[2026-09/2026-09-04-lamp-driver]] and [[2026-09-04-lamp-driver]] both \
                  become, and they name different documents"
                .to_owned(),
        })]
    );
}

const CLASH: &str = "[[2026-09/2026-09-04-lamp-driver]] and [[2026-09-04-lamp-driver]]";
const CLASHES: &str = "is what [[2026-09/2026-09-04-lamp-driver]] and \
    [[2026-09-04-lamp-driver]] both become, and they name different documents";

#[test]
fn a_clash_in_a_version_the_store_has_no_version_for_is_asked_about_where_its_text_is() {
    let Linking { old, pin, .. } = linking();
    let again = format!("2026-11-01 {CLASH}");
    let set = old.changed(&pin, "recheck", &[("recheck", scalar(&again))], desk(DAY_2));
    let store = read(&old);
    let file = format!("{CONVERSION}{TAGS}[topics.firmware]\nsummary = \"{CLASH}\"\n");
    let planned = plan(&store, &answers(&file));

    assert_eq!(
        planned.questions,
        [
            Question::Blocked(Blocked::Value {
                document: key(Kind::Fact, PIN),
                version: set.id.to_string(),
                field: "links".to_owned(),
                value: "[[2026-09-04-lamp-driver]]".to_owned(),
                why: CLASHES.to_owned(),
            }),
            Question::Open(Open::Answer {
                key: Key::topic("firmware", TopicKey::Summary),
                why: format!("[[2026-09-04-lamp-driver]] {CLASHES}"),
            }),
        ]
    );
}

#[test]
fn a_version_that_carries_the_text_of_the_one_it_follows_asks_of_no_clash_again() {
    let Linking { old, pin, .. } = linking();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    let battery = old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    let body = format!("\n{CLASH}\n");
    let linked = old.saved(&battery, "Lasts two days", &body, desk(DAY_2));
    old.removed(&phone, "Sold", desk(DAY_3));
    drop(pin);
    let store = read(&old);
    let file = format!(
        "{CONVERSION}{TAGS}[documents.\"topic/phone\"]\nended = \"retired\"\n\
         [documents.\"fact/phone/battery\"]\ntopics = [\"atlas\"]\n"
    );
    let planned = plan(&store, &answers(&file));

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::Value {
            document: key(Kind::Fact, "phone/battery"),
            version: linked.id.to_string(),
            field: "links".to_owned(),
            value: "[[2026-09-04-lamp-driver]]".to_owned(),
            why: CLASHES.to_owned(),
        })]
    );
    let versions = &of(&planned, Kind::Fact, "phone/battery").versions;
    assert_eq!(versions.len(), 3);
    assert_eq!(versions[2].links, versions[1].links);
    assert_eq!(versions[2].body, versions[1].body);
}

#[test]
fn every_kind_has_its_links_in_every_version() {
    let Linking {
        old,
        entry,
        port,
        pin,
    } = linking();
    let lantern = old.topic("torch", "A torch", &[], desk(DAY_1));
    let linked = "\n[[lantern/relay-pin]]\n";
    old.saved(&entry, "Drove the lamp", linked, desk(DAY_2));
    old.saved(&port, "Port the driver", linked, desk(DAY_2));
    let first = old.saved(&lantern, "A torch", linked, desk(DAY_2));
    old.removed(&first, "Sold", desk(DAY_3));
    old.saved(
        &pin,
        "Pin 5",
        "\n[[lantern/relay-pin]] itself\n",
        desk(DAY_2),
    );
    let store = read(&old);
    let file = format!("{TAGS}[documents.\"topic/torch\"]\nended = \"retired\"\n");
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    let row = table(&[(PIN, Some(document_id(Kind::Fact, PIN)))]);
    for (kind, origin) in [
        (Kind::Entry, LAMP_DRIVER),
        (Kind::Followup, PORT),
        (Kind::Topic, "torch"),
        (Kind::Fact, PIN),
    ] {
        let versions = &of(&planned, kind, origin).versions;
        assert_eq!(versions[0].links, Links::default(), "{origin}");
        for version in &versions[1..] {
            assert_eq!(version.links, row, "{origin}");
        }
    }
    assert_eq!(of(&planned, Kind::Topic, "torch").versions.len(), 3);
}
