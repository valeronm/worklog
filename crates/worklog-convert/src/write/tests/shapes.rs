use super::*;
use crate::fixture::{claims, composed, scalar};
use crate::plan::created_topic_id;

fn converted(old: &OldStore, file: &str) -> (Scratch, Vec<Composed>, Adapters) {
    let store = read(old);
    let decisions = Decisions::parse(file).unwrap();
    let complete = match plan(&store, &decisions) {
        Outcome::Complete(complete) => complete,
        Outcome::Open(open) => panic!("{:?} {:?}", open.questions, open.unused),
    };
    let scratch = Scratch::new();
    let outcome = write(&complete, &store, &scratch).unwrap();
    let adapters = wired(&scratch);
    let found = check(&adapters.deps()).unwrap();
    assert_eq!(found.problems, []);
    assert_eq!(found.fork_count, 0);
    assert_eq!(found.notices, outcome.notices);
    (scratch, compose(&complete).unwrap(), adapters)
}

fn changes(document: &Composed) -> Vec<&str> {
    let changes = document.versions.iter();
    changes
        .map(|version| version.envelope.change.as_str())
        .collect()
}

fn shown(adapters: &Adapters, address: &str) -> worklog_next::app::show::ShownDocument {
    match show(&adapters.deps(), address).unwrap() {
        Shown::Document(shown) => shown,
        Shown::Version(_) => panic!("{address} shows a version"),
    }
}

#[test]
fn a_document_the_conversion_renames_is_written_and_passes_the_new_check() {
    let old = with_desk();
    let shade = old.topic("shade", "A shade", &[], desk(DAY_1));
    old.fact("shade/cord", "Two meters", &[], desk(DAY_1));
    old.renamed(&shade, "lampshade", desk(DAY_2));
    old.fact("lampshade/cord", "Three meters", &[], desk(DAY_3));
    let file = format!("{CONVERSION}[documents.\"fact/lampshade/cord\"]\nname = \"cord-two\"\n");

    let (_scratch, composed, adapters) = converted(&old, &file);

    let renamed = of(&composed, &document_id(Kind::Fact, "lampshade/cord"));
    assert_eq!(changes(renamed), ["new", "rename"]);
    let by_new = shown(&adapters, "lampshade/cord-two");
    assert_eq!(by_new.document, renamed.id);
    assert_eq!(by_new.former, ["lampshade/cord"]);
    assert_eq!(by_new.heads.len(), 1);
    assert_eq!(by_new.heads[0].change, "rename");
    let other = shown(&adapters, "lampshade/cord");
    assert_eq!(other.document, document_id(Kind::Fact, "shade/cord"));
}

#[test]
fn a_fact_the_conversion_files_again_and_renames_in_one_version_passes_the_new_check() {
    let old = with_desk();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    old.fact("atlas/battery", "A page on cells", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
    let file = format!(
        "{CONVERSION}{ENDED_PHONE}[documents.\"fact/phone/battery\"]\ntopics = [\"atlas\"]\n\
         name = \"battery-life\"\n"
    );

    let (_scratch, composed, adapters) = converted(&old, &file);

    let moved = of(&composed, &document_id(Kind::Fact, "phone/battery"));
    assert_eq!(changes(moved), ["new", "rename"]);
    let by_new = shown(&adapters, "atlas/battery-life");
    assert_eq!(by_new.document, moved.id);
    assert_eq!(by_new.heads.len(), 1);
    let kept = shown(&adapters, "atlas/battery");
    assert_eq!(kept.document, document_id(Kind::Fact, "atlas/battery"));
    let rows = facts(&adapters.deps(), Some("atlas"), false).unwrap();
    let labels: Vec<&str> = rows.iter().map(|row| row.row.label.as_str()).collect();
    assert_eq!(labels, ["atlas/battery", "atlas/battery-life"]);
}

#[test]
fn a_claim_made_again_where_a_fork_is_joined_is_written_and_passes_the_new_check() {
    let directory = "~/projects/lantern";
    let claimed = [("lantern", &[directory][..])];
    let old = OldStore::new();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let first = old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    let none = [("claims", claims(&[]))];
    let one = old.changed(&first, "claim", &none, desk(DAY_2));
    let kept = [("summary", scalar("The desk by the window"))];
    let other = old.changed(&first, "save", &kept, desk(DAY_3));
    let mut fields = one.fields.clone();
    fields.set("claims", claims(&claimed));
    let heads = [&one, &other];
    let merge = composed("desk", desk(DAY_4), "resolve", &heads, fields, "\n");
    old.write(&merge);

    let (scratch, composed, adapters) = converted(&old, "");

    let claim = |again: usize| {
        let described = format!("claim\0desk\0lantern\0{directory}\0{again}");
        of(&composed, &generated_id(&described))
    };
    assert_eq!(changes(claim(0)), ["claim", "unclaim"]);
    assert_eq!(changes(claim(1)), ["claim"]);
    let deps = adapters.deps();
    init(&deps, "desk", None).unwrap();
    let within = scratch.path("home/projects/lantern");
    let opened = context(&deps, within.to_str().unwrap()).unwrap();
    let lantern = opened.topics.iter().find(|topic| topic.label == "lantern");
    assert!(lantern.is_some(), "{:?}", opened.topics);
}

#[test]
fn a_document_first_written_twice_and_joined_is_written_and_passes_the_new_check() {
    let old = with_desk();
    let first =
        |summary: &str, written: &str| OldStore::new().topic("globe", summary, &[], desk(written));
    let (earlier, later) = (first("A globe", DAY_2), first("A round map", DAY_3));
    old.write(&earlier);
    old.write(&later);
    let heads = [&earlier, &later];
    let both = composed(
        "globe",
        desk(DAY_4),
        "resolve",
        &heads,
        later.fields.clone(),
        "\n",
    );
    old.write(&both);

    let (_scratch, composed, adapters) = converted(&old, "");

    let globe = of(&composed, &document_id(Kind::Topic, "globe"));
    let [one, other, joined] = &globe.versions[..] else {
        panic!("globe has {} versions", globe.versions.len());
    };
    assert_eq!(one.envelope.parents, []);
    assert_eq!(other.envelope.parents, []);
    assert_eq!(joined.envelope.parents, ids(&[one, other]));
    let read = history(&adapters.deps(), "globe").unwrap();
    let heads: Vec<&VersionId> = (read.versions.iter())
        .filter(|row| row.head)
        .map(|row| &row.version)
        .collect();
    assert_eq!(heads, [&joined.id]);
}

#[test]
fn a_rename_joined_with_an_edit_under_the_earlier_slug_is_written_and_passes_the_new_check() {
    let old = with_desk();
    let first = old.topic("lens", "A lens", &[], desk(DAY_1));
    let (tombstone, _) = old.renamed(&first, "optic", desk(DAY_2));
    let edited = old.saved(&first, "A ground lens", "\n", desk(DAY_3));
    let heads = [&tombstone, &edited];
    let both = composed(
        "lens",
        desk(DAY_4),
        "resolve",
        &heads,
        edited.fields.clone(),
        "\n",
    );
    old.write(&both);

    let (_scratch, composed, adapters) = converted(&old, "");

    let lens = of(&composed, &document_id(Kind::Topic, "lens"));
    assert_eq!(lens.versions.len(), 4);
    let last = lens.versions.last().unwrap();
    assert_eq!(last.envelope.parents.len(), 2);
    assert_eq!(last.envelope.change, "resolve");
    let by_old = shown(&adapters, "lens");
    assert_eq!(by_old.document, lens.id);
    assert_eq!(by_old.label, "lens");
    assert_eq!(by_old.former, ["optic"]);
    assert_eq!(by_old.heads.len(), 1);
    assert_eq!(by_old.heads[0].version, last.id);
    assert_eq!(shown(&adapters, "optic").document, lens.id);
    assert_ne!(lens.id, created_topic_id("lens"));
}

#[test]
fn a_followup_renamed_on_one_machine_and_closed_on_the_other_is_written_closed() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let entry = old.entry(
        "2026-09/2026-09-04-lamp",
        "Wired the lamp",
        &[],
        desk(DAY_1),
    );
    let wire = "2026-09-04-wire";
    let first = old.followup(wire, &entry, "Wire it", &["lantern"], desk(DAY_1));
    let (tombstone, _) = old.renamed(&first, "2026-09-04-cable", desk(DAY_2));
    let done = old.done(&first, "Wired", desk(DAY_3));
    let heads = [&tombstone, &done];
    let fields = done.fields.clone();
    old.write(&composed(
        wire,
        desk(DAY_4),
        "resolve",
        &heads,
        fields,
        &done.body,
    ));

    let (_scratch, composed, adapters) = converted(&old, "[tags]\nlantern = \"lantern\"\n");

    let followup = of(&composed, &document_id(Kind::Followup, wire));
    assert_eq!(changes(followup), ["new", "done", "rename", "resolve"]);
    let last = followup.versions.last().unwrap();
    let record = Record::read(&last.envelope.kind, &last.fields).unwrap();
    let ended = record.ending.unwrap();
    assert_eq!((ended.reason, ended.note.as_str()), (Reason::Done, "Wired"));
    let read = history(&adapters.deps(), followup.id.as_str()).unwrap();
    let heads: Vec<&VersionId> = (read.versions.iter())
        .filter(|row| row.head)
        .map(|row| &row.version)
        .collect();
    assert_eq!(heads, [&last.id]);
    let open = followups(&adapters.deps(), Some("lantern"), false).unwrap();
    assert_eq!(open.len(), 0);
}
