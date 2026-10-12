use worklog_old::domain::version::{TOMBSTONE, note_body};

use super::*;

const LAMP: &str = "2026-09/2026-09-04-lamp";
const WIRING: &str = "2026-09/2026-09-05-wiring";

fn unused(planned: &Planned) -> Vec<String> {
    planned.unused.iter().map(Key::to_string).collect()
}

fn worked_on(planned: &Planned, origin: &str) -> Vec<Option<DocumentId>> {
    let versions = &of(planned, Kind::Entry, origin).versions;
    let machine = |version: &Version| match version.record.as_ref().map(|record| &record.content) {
        Some(Content::Entry(entry)) => Some(entry.machine.clone()),
        _ => None,
    };
    versions.iter().map(machine).collect()
}

fn own(slug: &str, topic: &str) -> String {
    format!("[documents.\"entry/{slug}\"]\nmachine = \"{topic}\"\n")
}

fn own_key(slug: &str) -> Key {
    Key::document(&key(Kind::Entry, slug), DocumentField::Machine)
}

fn bench(versions: usize, documents: &[&str]) -> Open {
    Open::Machine {
        name: "bench".to_owned(),
        versions,
        documents: documents
            .iter()
            .map(|slug| key(Kind::Entry, slug))
            .collect(),
    }
}

fn two_entries_on_the_bench() -> OldStore {
    let old = with_topics();
    old.machine_topic("phone", &[], &[], at(DAY_1, "phone"));
    for slug in [LAMP, WIRING] {
        let first = old.entry(slug, "Drove it", &[], desk(DAY_1));
        let elsewhere = [("machine", scalar("bench"))];
        let moved = old.changed(&first, "save", &elsewhere, desk(DAY_2));
        old.saved(&moved, "Drove it again", "\nAgain.\n", desk(DAY_3));
    }
    old
}

#[test]
fn the_machine_of_a_document_stands_in_each_version_whose_machine_no_topic_holds() {
    let old = two_entries_on_the_bench();
    let store = read(&old);
    let file = format!("{}{}", own(LAMP, "Phone"), own(WIRING, "desk"));
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(unused(&planned), [""; 0]);
    let (desk, phone) = (Some(topic_id("desk")), Some(topic_id("phone")));
    assert_eq!(
        worked_on(&planned, LAMP),
        [desk.clone(), phone.clone(), phone]
    );
    assert_eq!(
        worked_on(&planned, WIRING),
        [desk.clone(), desk.clone(), desk.clone()]
    );
    for version in &of(&planned, Kind::Entry, LAMP).versions {
        assert_eq!(version.machine, desk);
    }
}

#[test]
fn the_machine_of_a_document_stands_before_the_one_of_the_machines_table() {
    let old = two_entries_on_the_bench();
    let store = read(&old);
    let file = format!("[machines]\nbench = \"desk\"\n{}", own(LAMP, "phone"));
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(unused(&planned), [""; 0]);
    let (desk, phone) = (Some(topic_id("desk")), Some(topic_id("phone")));
    assert_eq!(
        worked_on(&planned, LAMP),
        [desk.clone(), phone.clone(), phone]
    );
    assert_eq!(
        worked_on(&planned, WIRING),
        [desk.clone(), desk.clone(), desk]
    );
}

#[test]
fn a_machine_stays_open_for_the_versions_no_answer_reaches() {
    let old = two_entries_on_the_bench();
    old.topic("relay", "A relay", &[], at(DAY_2, "bench"));
    let store = read(&old);

    let neither = plan(&store, &Decisions::default());
    assert_eq!(opens(&neither), [bench(5, &[LAMP, WIRING])]);

    let one = plan(&store, &answers(&own(LAMP, "phone")));
    assert_eq!(opens(&one), [bench(3, &[WIRING])]);
    assert_eq!(unused(&one), [""; 0]);
    let (desk, phone) = (Some(topic_id("desk")), Some(topic_id("phone")));
    assert_eq!(worked_on(&one, LAMP), [desk.clone(), phone.clone(), phone]);
    assert_eq!(worked_on(&one, WIRING), [desk, None, None]);

    let held = format!("{}{}", own(LAMP, "phone"), own(WIRING, "?"));
    let held = plan(&store, &answers(&held));
    assert_eq!(opens(&held), [bench(3, &[WIRING])]);
    assert_eq!(unused(&held), [""; 0]);

    let both = format!("{}{}", own(LAMP, "phone"), own(WIRING, "desk"));
    let both = plan(&store, &answers(&both));
    assert_eq!(opens(&both), [bench(1, &[])]);
}

#[test]
fn a_machine_of_a_document_that_answers_nothing_is_unused() {
    let old = two_entries_on_the_bench();
    let resolved = "2026-09/2026-09-06-bench";
    old.entry(resolved, "Benched", &[], desk(DAY_3));
    old.fact("lantern/pin", "Pin 5", &[], desk(DAY_1));
    let idle = "2026-09/2026-09-07-idle";
    old.entry(idle, "Idled", &[], desk(DAY_4));
    let store = read(&old);
    let file = format!(
        "[machines]\nbench = \"desk\"\n{}{}{}{}\
         [documents.\"fact/lantern/pin\"]\nmachine = \"desk\"\n",
        own(LAMP, "phone"),
        own(WIRING, "phone"),
        own(resolved, "phone"),
        own(idle, "?"),
    );
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(
        unused(&planned),
        [
            "machines.bench".to_owned(),
            own_key(resolved).to_string(),
            own_key(idle).to_string(),
            "documents.\"fact/lantern/pin\".machine".to_owned(),
        ]
    );
    assert_eq!(worked_on(&planned, resolved), [Some(topic_id("desk"))]);
}

#[test]
fn a_machine_of_a_document_that_names_no_topic_is_asked_about_where_the_answer_is() {
    let old = two_entries_on_the_bench();
    let store = read(&old);
    let file = format!("{}{}", own(LAMP, "slate"), own(WIRING, "desk"));
    let planned = plan(&store, &answers(&file));

    assert_eq!(
        planned.questions,
        [Question::Open(Open::Answer {
            key: own_key(LAMP),
            why: "slate names no topic".to_owned(),
        })]
    );
    assert_eq!(unused(&planned), [""; 0]);
    assert_eq!(
        worked_on(&planned, LAMP),
        [Some(topic_id("desk")), None, None]
    );
}

fn on_the_bench(old: &OldStore, slug: &str) -> Stored {
    let draft = OldStore::new().entry(slug, "Drove it", &[], at(DAY_1, "bench"));
    let first = composed(slug, desk(DAY_1), "new", &[], draft.fields, &draft.body);
    old.write(&first);
    first
}

#[test]
fn a_machine_of_a_document_no_planned_version_takes_is_unused_whatever_else_is_open() {
    let old = two_entries_on_the_bench();
    let gone = "2026-09/2026-09-06-bench";
    let first = old.entry(gone, "Benched", &[], desk(DAY_3));
    let body = note_body("Never happened").unwrap();
    let elsewhere = [("machine", scalar("bench"))];
    old.write(&following(
        &first,
        desk(DAY_4),
        TOMBSTONE,
        &elsewhere,
        Some(&body),
    ));
    let store = read(&old);
    let ended = format!("[documents.\"entry/{gone}\"]\nended = \"removed\"\nmachine = \"phone\"\n");

    let closed = plan(
        &store,
        &answers(&format!("[machines]\nbench = \"desk\"\n{ended}")),
    );
    assert_eq!(closed.questions, []);
    assert_eq!(unused(&closed), [own_key(gone).to_string()]);

    let open = plan(&store, &answers(&ended));
    assert_eq!(opens(&open), [bench(4, &[LAMP, WIRING])]);
    assert_eq!(unused(&open), [own_key(gone).to_string()]);
}

#[test]
fn the_machine_of_a_document_is_not_the_machine_that_stamped_its_versions() {
    let old = with_topics();
    old.machine_topic("phone", &[], &[], at(DAY_1, "phone"));
    let first = old.entry(LAMP, "Drove it", &[], at(DAY_1, "bench"));
    old.saved(&first, "Drove it again", "\nAgain.\n", desk(DAY_2));
    let store = read(&old);
    let planned = plan(&store, &answers(&own(LAMP, "phone")));

    assert_eq!(opens(&planned), [bench(1, &[])]);
    assert_eq!(unused(&planned), [""; 0]);
    let phone = Some(topic_id("phone"));
    assert_eq!(worked_on(&planned, LAMP), [phone.clone(), phone]);
    let versions = &of(&planned, Kind::Entry, LAMP).versions;
    assert_eq!(versions[0].machine, None);
    assert_eq!(versions[1].machine, Some(topic_id("desk")));
}

#[test]
fn a_renamed_entry_is_answered_under_its_newest_slug() {
    let old = with_topics();
    old.machine_topic("phone", &[], &[], at(DAY_1, "phone"));
    let first = on_the_bench(&old, LAMP_DRIVER);
    let (_, moved) = old.renamed(&first, LAMP, desk(DAY_2));
    old.saved(&moved, "Drove it again", "\nAgain.\n", desk(DAY_3));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    assert_eq!(opens(&asked), [bench(3, &[LAMP])]);

    let answered = plan(&store, &answers(&own(LAMP, "phone")));
    assert_eq!(answered.questions, []);
    assert_eq!(unused(&answered), [""; 0]);
    let phone = Some(topic_id("phone"));
    assert_eq!(
        worked_on(&answered, LAMP_DRIVER),
        [phone.clone(), phone.clone(), phone]
    );

    let under_the_old = plan(&store, &answers(&own(LAMP_DRIVER, "phone")));
    assert_eq!(opens(&under_the_old), [bench(3, &[LAMP])]);
    assert_eq!(unused(&under_the_old), [own_key(LAMP_DRIVER).to_string()]);
}

#[test]
fn a_placeholder_for_a_document_leaves_it_to_the_machines_table() {
    let old = two_entries_on_the_bench();
    let store = read(&old);
    let file = format!("[machines]\nbench = \"desk\"\n{}", own(WIRING, "?"));
    let planned = plan(&store, &answers(&file));

    assert_eq!(planned.questions, []);
    assert_eq!(unused(&planned), [own_key(WIRING).to_string()]);
    let desk = Some(topic_id("desk"));
    assert_eq!(
        worked_on(&planned, WIRING),
        [desk.clone(), desk.clone(), desk]
    );
}

#[test]
fn a_document_answered_wrongly_leaves_the_count_of_a_machine_still_open() {
    let old = two_entries_on_the_bench();
    let store = read(&old);
    let planned = plan(&store, &answers(&own(LAMP, "slate")));

    assert_eq!(
        opens(&planned),
        [
            bench(2, &[WIRING]),
            Open::Answer {
                key: own_key(LAMP),
                why: "slate names no topic".to_owned(),
            },
        ]
    );
    assert_eq!(unused(&planned), [""; 0]);
}

#[test]
fn the_machine_line_of_the_readme_answers_for_its_entry() {
    let readme = include_str!("../../../README.md");
    let slug = "2026-09/2026-09-04-lantern";
    let table = format!("[documents.\"entry/{slug}\"]");
    let (_, under) = readme.split_once(&table).unwrap();
    let line = under.lines().find(|line| line.starts_with("machine = "));
    let old = with_topics();
    on_the_bench(&old, slug);
    let store = read(&old);
    let planned = plan(&store, &answers(&format!("{table}\n{}\n", line.unwrap())));

    assert_eq!(planned.questions, []);
    assert_eq!(unused(&planned), [""; 0]);
    assert_eq!(worked_on(&planned, slug), [Some(topic_id("desk"))]);
}
