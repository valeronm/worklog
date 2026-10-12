use worklog_next::app::TriggerShown;
use worklog_next::app::claim::unclaim;
use worklog_next::app::context::Via;

use super::*;

fn converted() -> (Scratch, Adapters) {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    written(&store, &scratch);
    let adapters = wired(&scratch);
    (scratch, adapters)
}

#[test]
fn a_renamed_fact_is_shown_by_the_address_it_had() {
    let (_scratch, adapters) = converted();
    let deps = adapters.deps();

    let Shown::Document(old_address) = show(&deps, "lantern/relay-pin").unwrap() else {
        panic!("an address shows a document");
    };
    let Shown::Document(new_address) = show(&deps, "lantern/relay-output").unwrap() else {
        panic!("an address shows a document");
    };
    assert_eq!(old_address, new_address);
    assert_eq!(old_address.document, document_id(Kind::Fact, PIN));
    assert_eq!(old_address.label, "lantern/relay-output");
    assert_eq!(old_address.former, ["lantern/relay-pin"]);
    assert_eq!(old_address.ended, None);
    assert_eq!(old_address.heads.len(), 1);
    assert_eq!(old_address.heads[0].change, "rename");
    assert_eq!(
        old_address.heads[0].body,
        "\nFound in [[2026-09-04-lamp]].\n"
    );
}

#[test]
fn the_history_of_a_document_renamed_and_ended_reads_in_the_order_it_was_written() {
    let (_scratch, adapters) = converted();
    let deps = adapters.deps();
    let bench = document_id(Kind::Entry, BENCH);

    let read = history(&deps, bench.as_str()).unwrap();
    let rows: Vec<(&str, String, bool, Option<&str>, usize)> = (read.versions.iter())
        .map(|row| {
            (
                row.change.as_str(),
                row.written.to_string(),
                row.head,
                row.ended.as_deref(),
                row.parents.len(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("removed", DAY_5.to_owned(), true, Some("removed"), 1),
            ("rename", DAY_4.to_owned(), false, None, 1),
            ("new", DAY_3.to_owned(), false, None, 0),
        ]
    );
    assert_eq!(read.label, "2026-09-06-bench-test");
    assert!(read.versions.iter().all(|row| row.machine.label == "desk"));
    let Shown::Document(shown) = show(&deps, "2026-09-06-bench").unwrap() else {
        panic!("a former name shows a document");
    };
    assert_eq!(shown.document, bench);
    assert_eq!(shown.ended.as_deref(), Some("removed"));
}

#[test]
fn the_followups_of_a_topic_are_the_open_ones_and_the_ended_ones_when_asked() {
    let (_scratch, adapters) = converted();
    let deps = adapters.deps();
    let lantern = document_id(Kind::Topic, "lantern");

    let open = followups(&deps, Some("lantern"), false).unwrap();
    let summaries: Vec<&str> = open.iter().map(|row| row.row.summary.as_str()).collect();
    assert_eq!(summaries, ["See [[2026-09-04-lamp]]", "Wire it"]);
    let wire = &open[1];
    assert_eq!(
        wire.row.document,
        document_id(Kind::Followup, "2026-09-02-wire")
    );
    let Some(TriggerShown::Touching { topic }) = &wire.trigger else {
        panic!("{:?}", wire.trigger);
    };
    assert_eq!(topic.document, lantern);
    let entry = wire.entry.as_ref().unwrap();
    assert_eq!(entry.label, "2026-09-04-lamp");
    let again = &open[0];
    assert_eq!(again.about.as_ref().unwrap().label, "lantern/relay-output");
    assert_eq!(
        again.row.document,
        generated_id("recheck\0lantern/relay-pin")
    );

    let all = followups(&deps, Some("lantern"), true).unwrap();
    let ended: Vec<(&str, Option<&str>)> = (all.iter())
        .map(|row| (row.row.summary.as_str(), row.row.ended.as_deref()))
        .filter(|(_, ended)| ended.is_some())
        .collect();
    assert_eq!(ended, [("Port it", Some("done"))]);
    assert_eq!(all.len(), 3);
}

#[test]
fn the_facts_of_a_topic_cover_the_facts_of_its_parts() {
    let (_scratch, adapters) = converted();
    let deps = adapters.deps();

    let rows = facts(&deps, Some("lantern"), false).unwrap();
    let labels: Vec<&str> = rows.iter().map(|row| row.row.label.as_str()).collect();
    assert_eq!(labels, ["lantern/relay-output", "relay/coil"]);
    let rows = facts(&deps, Some("atlas"), false).unwrap();
    let labels: Vec<&str> = rows.iter().map(|row| row.row.label.as_str()).collect();
    assert_eq!(labels, ["atlas/battery", "atlas/scale"]);
}

#[test]
fn a_session_in_a_claimed_directory_is_opened_with_the_claimed_topic() {
    let (scratch, adapters) = converted();
    let deps = adapters.deps();
    let bound = init(&deps, "desk", None).unwrap();
    assert_eq!(bound.document, document_id(Kind::Topic, "desk"));
    assert!(!bound.created);

    let directory = scratch.path("home/projects/lantern/src");
    let opened = context(&deps, directory.to_str().unwrap()).unwrap();
    assert_eq!(opened.machine.label, "desk");
    assert_eq!(opened.forks, []);
    let lantern = (opened.topics.iter())
        .find(|topic| topic.label == "lantern")
        .unwrap();
    let claimed = Via::Claim {
        directory: Some("~/projects/lantern".to_owned()),
        closest: true,
    };
    assert_eq!(lantern.via, claimed);
    assert_eq!(lantern.summary, "A lamp on a relay");
    let facts: Vec<&str> = lantern.facts.iter().map(|row| row.label.as_str()).collect();
    assert_eq!(facts, ["lantern/relay-output"]);
    let parts: Vec<&str> = lantern
        .parts
        .iter()
        .map(|part| part.label.as_str())
        .collect();
    assert_eq!(parts, ["firmware", "relay"]);
    assert!(!opened.topics.iter().any(|topic| topic.label == "atlas"));

    let elsewhere = scratch.path("home/projects/atlas");
    let opened = context(&deps, elsewhere.to_str().unwrap()).unwrap();
    assert!(!opened.topics.iter().any(|topic| topic.label == "lantern"));
}

#[test]
fn a_claim_the_conversion_ends_is_labeled_and_noted_as_the_new_store_ends_one() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let (composed, _) = written(&store, &scratch);
    let said = |version: &NewVersion| {
        let field = |key: &str| version.fields.get(key).and_then(|value| value.as_str());
        let (ended, note) = (field("ended"), field("note"));
        let said = [Some(version.envelope.change.as_str()), ended, note];
        said.map(|part| part.map(str::to_owned))
    };
    let claims = composed.iter().filter(|held| held.kind == KindOf::Claim);
    let mut converted = claims.filter_map(|claim| claim.head().map(said));
    let ended = converted.find(|said| said[1].is_some()).unwrap();

    let adapters = wired(&scratch);
    let deps = adapters.deps();
    init(&deps, "desk", None).unwrap();
    let directory = scratch.path("home/projects/lantern");
    let unclaimed = unclaim(&deps, "lantern", directory.to_str()).unwrap();
    let file = scratch.target().join(unclaimed.document.bucket());
    let file = file.join(unclaimed.document.as_str());
    let text = std::fs::read_to_string(file.join(format!("{}.md", unclaimed.version))).unwrap();
    let written = NewVersion::read(&unclaimed.version, &text).unwrap();

    assert_eq!(ended, said(&written));
    assert_eq!(ended[0].as_deref(), Some("unclaim"));
}
