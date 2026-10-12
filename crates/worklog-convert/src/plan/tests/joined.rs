use super::*;

struct Raced {
    old: OldStore,
    tombstone: Stored,
    moved: Stored,
    both: Stored,
}

fn raced() -> Raced {
    raced_in(with_desk())
}

fn raced_in(old: OldStore) -> Raced {
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
    Raced {
        old,
        tombstone,
        moved,
        both,
    }
}

fn joins(raced: &Raced, why: &str) -> Question {
    Question::Blocked(Blocked::Value {
        document: key(Kind::Topic, "lantern"),
        version: raced.both.id.to_string(),
        field: "parents".to_owned(),
        value: raced.tombstone.id.to_string(),
        why: why.to_owned(),
    })
}

#[test]
fn a_document_removed_under_the_slug_it_was_renamed_from_after_the_rename_was_joined_blocks() {
    let raced = raced();
    raced.old.removed(&raced.both, "Renamed", desk(DAY_5));
    let store = read(&raced.old);
    let planned = plan(&store, &Decisions::default());

    let why = "is the tombstone of a rename, and the document was removed under lantern after \
               it and goes on under lamp";
    assert_eq!(planned.questions, [joins(&raced, why)]);
}

#[test]
fn a_document_removed_under_the_slug_it_was_renamed_to_after_the_rename_was_joined_blocks() {
    let raced = raced();
    raced.old.removed(&raced.moved, "A stray copy", desk(DAY_5));
    let store = read(&raced.old);
    let planned = plan(&store, &Decisions::default());

    let why = "is the tombstone of a rename, and the document was removed under lamp after it \
               and goes on under lantern";
    assert_eq!(planned.questions, [joins(&raced, why)]);
}

#[test]
fn a_document_removed_under_both_slugs_after_the_rename_was_joined_blocks() {
    let raced = raced();
    raced.old.removed(&raced.both, "Renamed", desk(DAY_5));
    raced.old.removed(&raced.moved, "A stray copy", desk(DAY_5));
    let store = read(&raced.old);
    let planned = plan(&store, &Decisions::default());

    let why = "is the tombstone of a rename, and the document was removed under both slugs \
               after it";
    assert_eq!(planned.questions, [joins(&raced, why)]);
}

#[test]
fn a_document_saved_again_under_the_slug_it_was_renamed_from_after_the_rename_was_joined_blocks() {
    let raced = raced();
    (raced.old).saved(&raced.both, "A lamp on a desk", "\n", desk(DAY_5));
    let store = read(&raced.old);
    let planned = plan(&store, &Decisions::default());

    let why = "is the tombstone of a rename, and the document goes on under both slugs";
    assert_eq!(planned.questions, [joins(&raced, why)]);
}

#[test]
fn a_joined_rename_with_nothing_after_it_is_asked_about_under_the_slug_of_its_one_head() {
    let old = with_desk();
    let first = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let (tombstone, _) = old.renamed(&first, "lamp", desk(DAY_2));
    let edited = old.saved(&first, "A brighter lamp", "\n", desk(DAY_3));
    let mut fields = edited.fields.clone();
    fields.set("summary", scalar(""));
    let heads = [&tombstone, &edited];
    old.write(&composed(
        "lantern",
        desk(DAY_4),
        "resolve",
        &heads,
        fields,
        "\n",
    ));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        opens(&planned),
        [Open::Summary {
            document: key(Kind::Topic, "lantern"),
        }]
    );
    assert_eq!(blocks(&planned), []);
}

const WIRE: &str = "2026-09-04-wire";

fn closed_and_renamed(old: &OldStore) -> Stored {
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_2));
    let entry = old.entry(
        "2026-09/2026-09-04-lamp",
        "Wired the lamp",
        &[],
        desk(DAY_1),
    );
    let first = old.followup(WIRE, &entry, "Wire it", &["handset"], desk(DAY_1));
    let (tombstone, _) = old.renamed(&first, "2026-09-04-cable", desk(DAY_2));
    let done = old.done(&first, "Wired", at(DAY_3, "desk"));
    let both = composed(
        WIRE,
        desk(DAY_4),
        "resolve",
        &[&tombstone, &done],
        done.fields.clone(),
        &done.body,
    );
    old.write(&both);
    both
}

#[test]
fn a_followup_renamed_on_one_machine_and_closed_on_the_other_is_planned_closed_from_its_head() {
    let old = with_desk();
    let both = closed_and_renamed(&old);
    let store = read(&old);
    let decisions = answers(&format!(
        "[documents.\"topic/phone\"]\nended = \"retired\"\n\
         [documents.\"followup/{WIRE}\"]\ntags.handset = \"phone\"\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let versions = &of(&planned, Kind::Followup, WIRE).versions;
    let last = versions.last().unwrap();
    assert_eq!(last.key, both.id.to_string());
    let ended = ending(last).unwrap();
    assert_eq!((ended.reason, ended.note.as_str()), (Reason::Done, "Wired"));
}

const TAGGED: &str = "[tags]\nwick = \"lantern\"\nshade = \"lamp\"\n";

fn raced_and_named_by_each_slug() -> Raced {
    let old = OldStore::new();
    let claims: [(&str, &[&str]); 2] = [
        ("lantern", &["~/projects/lantern"]),
        ("lamp", &["~/projects/lamp"]),
    ];
    old.machine_topic("desk", &claims, &["lantern", "lamp"], desk(DAY_1));
    let raced = raced_in(old);
    let old = &raced.old;
    old.topic("shelf", "A shelf", &["lantern", "lamp"], desk(DAY_1));
    old.fact("lantern/wick", "Cotton", &[], desk(DAY_1));
    old.fact("lamp/shade", "Glass", &[], desk(DAY_1));
    let tags = ["wick", "shade"];
    let entry = old.entry("2026-09/2026-09-04-trim", "Trimmed", &tags, desk(DAY_1));
    for (slug, tag, topic) in [
        ("2026-09-04-trim", "wick", "lantern"),
        ("2026-09-04-polish", "shade", "lamp"),
    ] {
        let followup = old.followup(slug, &entry, "Do it", &[tag], desk(DAY_1));
        let touching = [("recheck", scalar(&format!("touching {topic}")))];
        old.changed(&followup, "recheck", &touching, desk(DAY_2));
    }
    raced
}

fn only_the_block_stands(raced: &Raced, why: &str) {
    let store = read(&raced.old);
    let decisions = answers(TAGGED);
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, [joins(raced, why)]);
    assert_eq!(planned.unused, []);
    let lantern = topic_id("lantern");
    assert!(
        planned
            .documents
            .iter()
            .all(|planned| planned.id != lantern)
    );
}

#[test]
fn what_names_a_document_removed_under_the_slug_it_was_renamed_from_is_asked_nothing() {
    let raced = raced_and_named_by_each_slug();
    raced.old.removed(&raced.both, "Renamed", desk(DAY_5));

    let why = "is the tombstone of a rename, and the document was removed under lantern after \
               it and goes on under lamp";
    only_the_block_stands(&raced, why);
}

#[test]
fn what_names_a_document_removed_under_the_slug_it_was_renamed_to_is_asked_nothing() {
    let raced = raced_and_named_by_each_slug();
    raced.old.removed(&raced.moved, "A stray copy", desk(DAY_5));

    let why = "is the tombstone of a rename, and the document was removed under lamp after it \
               and goes on under lantern";
    only_the_block_stands(&raced, why);
}

#[test]
fn no_name_is_held_against_a_document_that_goes_on_under_both_slugs() {
    let raced = raced();
    raced.old.removed(&raced.moved, "A stray copy", desk(DAY_5));
    raced.old.topic("Lamp", "Another lamp", &[], desk(DAY_1));
    let store = read(&raced.old);
    let decisions = answers("[documents.\"topic/Lamp\"]\nname = \"lamp\"\n");
    let planned = plan(&store, &decisions);

    let why = "is the tombstone of a rename, and the document was removed under lamp after it \
               and goes on under lantern";
    assert_eq!(planned.questions, [joins(&raced, why)]);
    assert_eq!(planned.unused, []);
}

#[test]
fn a_block_names_the_slug_a_document_renamed_again_was_removed_under() {
    let raced = raced();
    let (_, beacon) = raced.old.renamed(&raced.moved, "beacon", desk(DAY_5));
    raced.old.removed(&beacon, "Dimmed", desk(DAY_6));
    let store = read(&raced.old);
    let planned = plan(&store, &Decisions::default());

    let why = "is the tombstone of a rename, and the document was removed under beacon after \
               it and goes on under lantern";
    assert_eq!(planned.questions, [joins(&raced, why)]);
}
