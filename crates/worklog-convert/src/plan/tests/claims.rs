use worklog::domain::schema::{Claim, Directory};

use super::*;

const LANTERN_DIR: &str = "~/projects/lantern";

fn claim(version: &Version) -> Claim {
    match version.record.as_ref().map(|record| &record.content) {
        Some(Content::Claim(claim)) => claim.clone(),
        other => panic!("not a claim: {other:?}"),
    }
}

fn claim_id(machine: &str, topic: &str, directory: &str, again: usize) -> DocumentId {
    generated_id(&format!("claim\0{machine}\0{topic}\0{directory}\0{again}"))
}

fn on(topic: DocumentId, directory: Option<&str>) -> Claim {
    Claim {
        machine: topic_id("desk"),
        topic,
        directory: directory.map(|directory| Directory::parse(directory).unwrap()),
    }
}

fn with_lantern() -> OldStore {
    let old = OldStore::new();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old
}

fn unclaimed(on: &str) -> Ending {
    Ending {
        reason: Reason::Removed,
        on: date(on),
        by: None,
        note: "unclaimed".to_owned(),
    }
}

#[test]
fn a_claim_is_a_document_made_where_it_appears_and_kept_while_it_stays() {
    let old = with_lantern();
    let claimed = [("lantern", &["~/projects/lantern/"][..])];
    let first = old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    old.saved(
        &first,
        "The desk by the window",
        "\nBy the window.\n",
        desk(DAY_2),
    );
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let [document] = of_kind(&planned, KindOf::Claim)[..] else {
        panic!("one claim");
    };
    assert_eq!(document.id, claim_id("desk", "lantern", LANTERN_DIR, 0));
    let [version] = document.versions.as_slice() else {
        panic!("one version: {:?}", document.versions);
    };
    assert_eq!(version.key, first.id.to_string());
    assert_eq!(version.parents, [] as [&str; 0]);
    assert_eq!(stamped(version), DAY_1);
    assert_eq!(version.machine, Some(topic_id("desk")));
    assert_eq!(version.change, "claim");
    assert_eq!(version.body, "\n");
    assert_eq!(
        version.origin,
        Origin::Generated {
            cause: held(&store, Kind::Topic, "desk", &first),
            answered: None,
        }
    );
    assert_eq!(claim(version), on(topic_id("lantern"), Some(LANTERN_DIR)));
    assert_eq!(ending(version), None);
}

#[test]
fn a_claim_gone_ends_removed_and_made_again_is_another_document() {
    let old = with_lantern();
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    let first = old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    let gone = old.changed(&first, "unclaim", &[("claims", claims(&[]))], desk(DAY_3));
    let again = old.changed(&gone, "claim", &[("claims", claims(&claimed))], desk(DAY_4));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    assert_eq!(of_kind(&planned, KindOf::Claim).len(), 2);
    let once = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 0));
    assert_eq!(once.versions.len(), 2);
    assert_eq!(ending(&once.versions[0]), None);
    let ended = &once.versions[1];
    assert_eq!(ended.key, gone.id.to_string());
    assert_eq!(ended.parents, [first.id.to_string()]);
    assert_eq!(stamped(ended), DAY_3);
    assert_eq!(ended.change, "unclaim");
    assert_eq!(ended.body, "\n");
    assert_eq!(
        ended.origin,
        Origin::Generated {
            cause: held(&store, Kind::Topic, "desk", &gone),
            answered: None,
        }
    );
    assert_eq!(claim(ended), claim(&once.versions[0]));
    assert_eq!(ending(ended), Some(unclaimed("2026-09-06")));
    let twice = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 1));
    assert_eq!(twice.versions.len(), 1);
    assert_eq!(twice.versions[0].key, again.id.to_string());
    assert_eq!(stamped(&twice.versions[0]), DAY_4);
    assert_eq!(claim(&twice.versions[0]), claim(ended));
}

#[test]
fn each_directory_and_each_unclaimed_topic_is_a_claim_of_its_own() {
    let old = with_lantern();
    let claimed = [("lantern", &[LANTERN_DIR, "/srv/lantern"][..])];
    let first = old.machine_topic("desk", &claimed, &["atlas"], desk(DAY_1));
    let fewer = [("lantern", &["/srv/lantern"][..])];
    old.changed(
        &first,
        "unclaim",
        &[("claims", claims(&fewer))],
        desk(DAY_2),
    );
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    assert_eq!(of_kind(&planned, KindOf::Claim).len(), 3);
    let home = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 0));
    assert_eq!(home.versions.len(), 2);
    let served = by_id(&planned, &claim_id("desk", "lantern", "/srv/lantern", 0));
    assert_eq!(served.versions.len(), 1);
    assert_eq!(
        claim(&served.versions[0]),
        on(topic_id("lantern"), Some("/srv/lantern"))
    );
    let anywhere = by_id(&planned, &claim_id("desk", "atlas", "", 0));
    assert_eq!(anywhere.versions.len(), 1);
    assert_eq!(claim(&anywhere.versions[0]), on(topic_id("atlas"), None));
    assert_eq!(anywhere.versions[0].change, "claim");
}

#[test]
fn a_claim_is_one_document_through_the_renames_of_its_topic_and_its_machine() {
    let old = with_lantern();
    let beacon = old.topic("beacon", "A light on a hill", &[], desk(DAY_1));
    old.renamed(&beacon, "lamp", desk(DAY_2));
    let claimed = [("beacon", &[LANTERN_DIR][..])];
    let first = old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    let renamed = [("lamp", &[LANTERN_DIR][..])];
    let spelled = old.changed(
        &first,
        "claim",
        &[("claims", claims(&renamed))],
        desk(DAY_3),
    );
    let (_, moved) = old.renamed(&spelled, "bench", desk(DAY_4));
    old.saved(&moved, "The bench", "\n", desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let [document] = of_kind(&planned, KindOf::Claim)[..] else {
        panic!("one claim");
    };
    assert_eq!(document.id, claim_id("desk", "beacon", LANTERN_DIR, 0));
    assert_eq!(document.versions.len(), 1);
    assert_eq!(
        claim(&document.versions[0]),
        on(topic_id("beacon"), Some(LANTERN_DIR))
    );
}

#[test]
fn a_claim_on_no_topic_is_asked_about_and_dropped_or_made_on_a_created_topic() {
    let old = with_lantern();
    let claimed = [("ghost", &["~/ghost"][..])];
    old.machine_topic("desk", &claimed, &["Spirit"], desk(DAY_1));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    let on_nothing = |item: &str| Open::Claim {
        machine_topic: Folded::new("desk"),
        item: Folded::new(item),
    };
    assert_eq!(opens(&asked), [on_nothing("ghost"), on_nothing("spirit")]);
    assert_eq!(of_kind(&asked, KindOf::Claim).len(), 0);

    let includes = answers("[topics.desk]\ndrop = [\"ghost\", \"spirit\"]\n");
    let still = plan(&store, &includes);
    assert_eq!(opens(&still), [on_nothing("ghost"), on_nothing("spirit")]);
    assert_eq!(still.unused.len(), 2);

    let dropped = answers("[topics.desk]\nunclaim = [\"ghost\", \"spirit\"]\n");
    let planned = plan(&store, &dropped);
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    assert_eq!(of_kind(&planned, KindOf::Claim).len(), 0);

    let created = answers(&format!(
        "{CONVERSION}[topics.ghost]\nsummary = \"A ghost\"\n[topics.desk]\nunclaim = [\"spirit\"]\n"
    ));
    let planned = plan(&store, &created);
    assert_eq!(planned.questions, []);
    let document = by_id(&planned, &claim_id("desk", "ghost", "~/ghost", 0));
    assert_eq!(
        claim(&document.versions[0]),
        on(created_topic_id("ghost"), Some("~/ghost"))
    );
}

#[test]
fn what_no_claim_of_the_new_store_can_carry_blocks_once_where_it_first_stands() {
    let old = with_lantern();
    old.topic("Phone", "A phone", &[], desk(DAY_1));
    old.topic("PHONE", "Another phone", &[], desk(DAY_1));
    let claimed = [
        ("lantern", &["projects/lantern", LANTERN_DIR][..]),
        ("phone", &["~/phone"][..]),
    ];
    let first = old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    old.saved(&first, "The desk by the window", "\n", desk(DAY_2));
    let beacon = old.topic("beacon", "A light on a hill", &[], desk(DAY_1));
    let stray = [("atlas", &["~/atlas"][..]), ("lantern", &["~/lamp"][..])];
    let strayed = old.changed(&beacon, "save", &[("claims", claims(&stray))], desk(DAY_2));
    old.saved(&strayed, "A light on a far hill", "\n", desk(DAY_3));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let value = |slug: &str, version: &Stored, value: &str, why: &str| Blocked::Value {
        document: key(Kind::Topic, slug),
        version: version.id.to_string(),
        field: "claims".to_owned(),
        value: value.to_owned(),
        why: why.to_owned(),
    };
    assert_eq!(
        blocks(&planned),
        [
            value(
                "beacon",
                &strayed,
                "atlas, lantern",
                "is claimed by a topic that is no machine's"
            ),
            value(
                "desk",
                &first,
                "projects/lantern",
                "is neither under `~/` nor an absolute path"
            ),
        ]
    );
    let refused = |slug: &str| Open::Name {
        document: key(Kind::Topic, slug),
        name: slug.to_owned(),
        why: "holds an uppercase letter".to_owned(),
    };
    assert_eq!(
        opens(&planned),
        [
            Open::Claim {
                machine_topic: Folded::new("desk"),
                item: Folded::new("phone"),
            },
            refused("PHONE"),
            refused("Phone"),
        ]
    );
    let made = of_kind(&planned, KindOf::Claim);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].id, claim_id("desk", "lantern", LANTERN_DIR, 0));
}

#[test]
fn claims_that_are_no_map_of_lists_block() {
    let old = with_lantern();
    let first = old.machine_topic("desk", &[], &[], desk(DAY_1));
    let mut fields = first.fields.clone();
    fields.set("claims", scalar("lantern"));
    let flat = composed("desk", desk(DAY_2), "save", &[&first], fields, "\n");
    old.write(&flat);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    let value = |version: &Stored, value: &str, why: &str| Blocked::Value {
        document: key(Kind::Topic, "desk"),
        version: version.id.to_string(),
        field: "claims".to_owned(),
        value: value.to_owned(),
        why: why.to_owned(),
    };
    let expected = [value(&flat, "lantern", "is not a map of lists")];
    assert_eq!(blocks(&planned), expected);
    assert_eq!(of_kind(&planned, KindOf::Claim).len(), 0);
}

#[test]
fn a_claim_ended_where_the_write_time_is_not_read_has_no_record_there() {
    let old = with_lantern();
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    let first = old.machine_topic("desk", &claimed, &[], desk(DAY_1));
    let unclaim = [("claims", claims(&[]))];
    let gone = following(&first, desk("yesterday"), "unclaim", &unclaim, None);
    old.write(&gone);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::WriteTime {
            document: key(Kind::Topic, "desk"),
            version: gone.id.to_string(),
            text: "yesterday".to_owned(),
        })]
    );
    let [document] = of_kind(&planned, KindOf::Claim)[..] else {
        panic!("one claim");
    };
    assert_eq!(document.versions.len(), 2);
    assert!(document.versions[0].record.is_some());
    assert_eq!(document.versions[1].key, gone.id.to_string());
    assert_eq!(document.versions[1].stamp, None);
    assert_eq!(document.versions[1].record, None);
}

struct Fork {
    old: OldStore,
    one: Stored,
    other: Stored,
}

fn fork(
    before: &[(&str, &[&str])],
    one: &[(&str, &[&str])],
    other: &[(&str, &[&str])],
    turn: usize,
) -> Fork {
    let old = with_lantern();
    let first = old.machine_topic("desk", before, &[], desk(DAY_1));
    let one = old.changed(&first, "claim", &[("claims", claims(one))], desk(DAY_2));
    let summary = format!("The desk, as told the {turn}. time");
    let changes = [("claims", claims(other)), ("summary", scalar(&summary))];
    let other = old.changed(&first, "save", &changes, desk(DAY_3));
    Fork { old, one, other }
}

fn joined(fork: &Fork, held: &[(&str, &[&str])]) -> Stored {
    let mut fields = fork.one.fields.clone();
    let held = if held.is_empty() {
        scalar("")
    } else {
        claims(held)
    };
    fields.set("claims", held);
    let heads = [&fork.one, &fork.other];
    let joined = composed("desk", desk(DAY_4), "resolve", &heads, fields, "\n");
    fork.old.write(&joined);
    joined
}

const TURNS: usize = 6;

#[test]
fn a_merge_makes_no_claim_and_ends_none_that_a_side_it_joins_holds() {
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    let mut orders = BTreeSet::new();
    for turn in 0..TURNS {
        let one_side = fork(&[], &claimed, &[], turn);
        orders.insert(one_side.one.id < one_side.other.id);
        joined(&one_side, &claimed);
        let store = read(&one_side.old);
        let planned = plan(&store, &Decisions::default());
        assert_eq!(planned.questions, []);
        let [document] = of_kind(&planned, KindOf::Claim)[..] else {
            panic!("one claim in turn {turn}");
        };
        assert_eq!(document.id, claim_id("desk", "lantern", LANTERN_DIR, 0));
        assert_eq!(document.versions.len(), 1);
        assert_eq!(document.versions[0].key, one_side.one.id.to_string());

        let both = fork(&claimed, &claimed, &claimed, turn);
        joined(&both, &claimed);
        let store = read(&both.old);
        let planned = plan(&store, &Decisions::default());
        assert_eq!(planned.questions, []);
        let [document] = of_kind(&planned, KindOf::Claim)[..] else {
            panic!("one claim in turn {turn}");
        };
        assert_eq!(document.versions.len(), 1);
    }
    assert_eq!(orders.len(), 2);
}

#[test]
fn a_claim_one_side_ended_stays_ended_where_the_merge_agrees() {
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    for turn in 0..TURNS {
        let fork = fork(&claimed, &[], &claimed, turn);
        let merge = joined(&fork, &[]);
        let again = [("claims", claims(&claimed))];
        let back = fork.old.changed(&merge, "claim", &again, desk(DAY_5));
        let store = read(&fork.old);
        let planned = plan(&store, &Decisions::default());
        assert_eq!(planned.questions, []);
        let once = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 0));
        assert_eq!(once.versions.len(), 2);
        assert_eq!(once.versions[1].key, fork.one.id.to_string());
        assert_eq!(ending(&once.versions[1]), Some(unclaimed("2026-09-05")));
        let twice = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 1));
        assert_eq!(twice.versions.len(), 1);
        assert_eq!(twice.versions[0].key, back.id.to_string());
    }
}

#[test]
fn a_claim_whose_history_no_one_line_of_versions_tells_blocks() {
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    let claim_text = format!("lantern at {LANTERN_DIR}");
    let value = |version: &Stored, why: String| Blocked::Value {
        document: key(Kind::Topic, "desk"),
        version: version.id.to_string(),
        field: "claims".to_owned(),
        value: claim_text.clone(),
        why,
    };
    for turn in 0..TURNS {
        let apart = fork(&[], &claimed, &claimed, turn);
        let merge = joined(&apart, &claimed);
        let store = read(&apart.old);
        let planned = plan(&store, &Decisions::default());
        let mut made = [apart.one.id.to_string(), apart.other.id.to_string()];
        made.sort();
        let why = format!(
            "is claimed at {} and at {}, neither after the other, and this version follows both",
            made[0], made[1]
        );
        assert_eq!(blocks(&planned), [value(&merge, why)]);

        let twice = fork(&claimed, &[], &[], turn);
        let merge = joined(&twice, &[]);
        let store = read(&twice.old);
        let planned = plan(&store, &Decisions::default());
        let sides = [&twice.one, &twice.other];
        let ended_twice = |at: &Stored, before: &Stored| {
            let why = format!(
                "is unclaimed at {} and at this version, neither after the other",
                before.id
            );
            value(at, why)
        };
        let either = [
            ended_twice(sides[0], sides[1]),
            ended_twice(sides[1], sides[0]),
        ];
        let blocked = blocks(&planned);
        assert_eq!(blocked.len(), 1, "{blocked:?}");
        assert!(either.contains(&blocked[0]), "{blocked:?}");
        assert_eq!(merge.block.parents.len(), 2);
    }
}

#[test]
fn a_claim_one_side_ended_and_the_merge_holds_is_made_again_at_the_merge() {
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    for turn in 0..TURNS {
        let kept = fork(&claimed, &[], &claimed, turn);
        let merge = joined(&kept, &claimed);
        let store = read(&kept.old);
        let planned = plan(&store, &Decisions::default());

        assert_eq!(planned.questions, []);
        let once = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 0));
        assert_eq!(once.versions.len(), 2);
        assert_eq!(once.versions[1].key, kept.one.id.to_string());
        assert_eq!(once.versions[1].change, "unclaim");
        assert_eq!(ending(&once.versions[1]), Some(unclaimed("2026-09-05")));
        let again = by_id(&planned, &claim_id("desk", "lantern", LANTERN_DIR, 1));
        assert_eq!(again.versions.len(), 1);
        assert_eq!(again.versions[0].key, merge.id.to_string());
        assert_eq!(again.versions[0].change, "claim");
        assert_eq!(again.versions[0].parents, [""; 0]);
        assert_eq!(ending(&again.versions[0]), None);
        assert_eq!(of_kind(&planned, KindOf::Claim).len(), 2);
    }
}

#[test]
fn a_claim_on_an_ended_topic_is_asked_about_and_ended_in_one_more_version() {
    let old = with_lantern();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    let claimed = [("phone", &["~/phone", "~/handset"][..])];
    let first = old.machine_topic("desk", &claimed, &["phone"], desk(DAY_1));
    let fewer = [("phone", &["~/phone"][..])];
    let changes = [("claims", claims(&fewer)), ("includes", list(&["phone"]))];
    old.changed(&first, "unclaim", &changes, desk(DAY_2));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);
    let edge = format!("{ENDED_PHONE}[topics.desk]\ndrop = [\"phone\"]\n");

    let asked = plan(&store, &answers(&edge));
    assert_eq!(
        asked.questions,
        [Question::Open(Open::EndedTopic {
            document: key(Kind::Topic, "desk"),
            through: Through::Claim,
            topic: "phone".to_owned(),
        })]
    );
    assert_eq!(asked.unused, []);
    let id = claim_id("desk", "phone", "~/phone", 0);
    assert_eq!(by_id(&asked, &id).versions.len(), 1);

    let decisions = answers(&format!("{CONVERSION}{edge}unclaim = [\"phone\"]\n"));
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let document = by_id(&planned, &id);
    assert_eq!(document.versions.len(), 2);
    let ended = &document.versions[1];
    authored_by_desk(ended, &[&first], "unclaim");
    assert_eq!(ended.origin, Origin::Authored { answered: None });
    assert_eq!(claim(ended), on(topic_id("phone"), Some("~/phone")));
    assert_eq!(ending(ended), Some(unclaimed("2026-10-20")));
    let anywhere = by_id(&planned, &claim_id("desk", "phone", "", 0));
    assert_eq!(anywhere.versions.len(), 2);
    assert_eq!(ending(&anywhere.versions[1]), Some(unclaimed("2026-10-20")));
    let before = by_id(&planned, &claim_id("desk", "phone", "~/handset", 0));
    assert_eq!(before.versions.len(), 2);
    assert_eq!(ending(&before.versions[1]), Some(unclaimed("2026-09-05")));
    let unclaimed = |directory: &str| Authored::Unclaimed {
        machine_topic: "desk".to_owned(),
        topic: "phone".to_owned(),
        directory: directory.to_owned(),
    };
    assert_eq!(planned.authored, [unclaimed(""), unclaimed("~/phone")]);
}

#[test]
fn a_machine_topic_removed_ends_its_claims_where_it_ends() {
    let old = with_lantern();
    old.machine_topic("desk", &[], &[], desk(DAY_1));
    let claimed = [("lantern", &[LANTERN_DIR][..])];
    let bench = old.machine_topic("bench", &claimed, &["atlas"], at(DAY_1, "bench"));
    let removed = old.removed(&bench, "Sold the bench", desk(DAY_3));
    let store = read(&old);
    let decisions = answers("[documents.\"topic/bench\"]\nended = \"retired\"\n");
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    let made = of_kind(&planned, KindOf::Claim);
    assert_eq!(made.len(), 2);
    for document in made {
        assert_eq!(document.versions.len(), 2);
        let ended = &document.versions[1];
        assert_eq!(ended.key, removed.id.to_string());
        assert_eq!(ended.parents, [bench.id.to_string()]);
        assert_eq!(stamped(ended), DAY_3);
        assert_eq!(ended.machine, Some(topic_id("desk")));
        assert_eq!(document.versions[0].machine, Some(topic_id("bench")));
        assert_eq!(claim(ended).machine, topic_id("bench"));
        assert_eq!(ending(ended), Some(unclaimed("2026-09-06")));
    }
}
