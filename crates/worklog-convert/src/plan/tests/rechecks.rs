use super::*;

fn recheck_id(origin: &str) -> DocumentId {
    generated_id(&format!("recheck\0{origin}"))
}

fn rechecked(old: &OldStore, parent: &Stored, recheck: &str, at: At) -> Stored {
    old.changed(parent, "recheck", &[("recheck", scalar(recheck))], at)
}

fn pin_key() -> DocumentKey {
    key(Kind::Fact, PIN)
}

#[test]
fn a_fact_to_look_at_again_on_a_day_has_a_followup_about_it() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    let set = rechecked(&old, &pin, "2026-11-01 The board ships", desk(DAY_2));
    old.saved(&set, "Pin 6", "\nMoved.\n", desk(DAY_4));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let [document] = of_kind(&planned, KindOf::Followup)[..] else {
        panic!("one followup");
    };
    assert_eq!(document.id, recheck_id(PIN));
    let [version] = document.versions.as_slice() else {
        panic!("one version: {:?}", document.versions);
    };
    assert_eq!(version.key, set.id.to_string());
    assert_eq!(version.parents, [] as [&str; 0]);
    assert_eq!(stamped(version), DAY_2);
    assert_eq!(version.machine, Some(topic_id("desk")));
    assert_eq!(version.change, "new");
    assert_eq!(version.body, "\n");
    assert_eq!(
        version.origin,
        Origin::Generated {
            cause: held(&store, Kind::Fact, PIN, &set),
            answered: None,
        }
    );
    assert_eq!(
        followup(version),
        Followup {
            created: date("2026-09-05"),
            topics: vec![topic_id("lantern")],
            entry: None,
            about: Some(document_id(Kind::Fact, PIN)),
            trigger: Some(Trigger::LookAgain {
                on: date("2026-11-01"),
                why: "The board ships".to_owned(),
            }),
            summary: "The board ships".to_owned(),
        }
    );
    assert_eq!(ending(version), None);
}

#[test]
fn a_recheck_touching_a_topic_takes_its_summary_from_the_file() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    rechecked(&old, &pin, "touching Lantern", desk(DAY_2));
    let store = read(&old);

    let asked = plan(&store, &Decisions::default());
    assert_eq!(
        asked.questions,
        [Question::Open(Open::RecheckSummary {
            fact: pin_key(),
            touching: "Lantern".to_owned(),
        })]
    );
    assert_eq!(by_id(&asked, &recheck_id(PIN)).versions[0].record, None);

    let file = "[documents.\"fact/lantern/relay-pin\"]\nrecheck_summary = \"Look again\"\n";
    let planned = plan(&store, &answers(file));
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let version = &by_id(&planned, &recheck_id(PIN)).versions[0];
    let found = followup(version);
    assert_eq!(found.topics, [topic_id("lantern")]);
    assert_eq!(found.trigger, Some(Trigger::Touching(topic_id("lantern"))));
    assert_eq!(found.summary, "Look again");
    let answered = Some(Key::document(&pin_key(), DocumentField::RecheckSummary));
    assert!(matches!(&version.origin, Origin::Generated { answered: at, .. } if *at == answered));
}

#[test]
fn a_recheck_touching_another_topic_files_the_followup_under_it_too() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    rechecked(&old, &pin, "touching atlas", desk(DAY_2));
    let store = read(&old);
    let file = "[documents.\"fact/lantern/relay-pin\"]\nrecheck_summary = \"Look again\"\n";
    let planned = plan(&store, &answers(file));

    assert_eq!(planned.questions, []);
    let found = followup(&by_id(&planned, &recheck_id(PIN)).versions[0]);
    assert_eq!(found.topics, [topic_id("lantern"), topic_id("atlas")]);
    assert_eq!(found.trigger, Some(Trigger::Touching(topic_id("atlas"))));
}

#[test]
fn a_recheck_touching_no_topic_that_is_not_ended_is_asked_about_under_the_fact() {
    let old = with_topics();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    rechecked(&old, &pin, "touching phone", desk(DAY_2));
    let dimmer = old.fact("lantern/dimmer", "Dims by half", &[], desk(DAY_1));
    rechecked(&old, &dimmer, "touching ghost", desk(DAY_2));
    let store = read(&old);
    let dimmer_key = key(Kind::Fact, "lantern/dimmer");
    let summaries = format!(
        "{ENDED_PHONE}[documents.\"fact/lantern/relay-pin\"]\nrecheck_summary = \"Look again\"\n\
         [documents.\"fact/lantern/dimmer\"]\nrecheck_summary = \"Look at it\"\n"
    );

    let asked = plan(&store, &answers(&summaries));
    assert_eq!(
        opens(&asked),
        [
            Open::Touching {
                document: dimmer_key,
                topic: "ghost".to_owned(),
            },
            Open::EndedTopic {
                document: pin_key(),
                through: Through::Touching,
                topic: "phone".to_owned(),
            },
        ]
    );
    assert_eq!(by_id(&asked, &recheck_id(PIN)).versions[0].record, None);

    let file = format!(
        "{ENDED_PHONE}[documents.\"fact/lantern/relay-pin\"]\nrecheck_summary = \"Look again\"\n\
         touching = \"atlas\"\n[documents.\"fact/lantern/dimmer\"]\n\
         recheck_summary = \"Look at it\"\ntouching = \"\"\n"
    );
    let planned = plan(&store, &answers(&file));
    assert_eq!(planned.questions, []);
    assert_eq!(planned.unused, []);
    let pin = followup(&by_id(&planned, &recheck_id(PIN)).versions[0]);
    assert_eq!(pin.trigger, Some(Trigger::Touching(topic_id("atlas"))));
    assert_eq!(pin.topics, [topic_id("lantern"), topic_id("atlas")]);
    let dimmer = followup(&by_id(&planned, &recheck_id("lantern/dimmer")).versions[0]);
    assert_eq!(dimmer.trigger, None);
    assert_eq!(dimmer.topics, [topic_id("lantern")]);
    assert_eq!(dimmer.summary, "Look at it");
}

#[test]
fn a_followup_is_stamped_where_the_recheck_the_fact_ends_with_was_set() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    let first = rechecked(&old, &pin, "2026-11-01 The board ships", desk(DAY_2));
    let changed = rechecked(&old, &first, "2026-12-01 The board ships", desk(DAY_3));
    let saved = old.saved(&changed, "Pin 6", "\nMoved.\n", desk(DAY_4));
    old.renamed(&saved, "lantern/relay", desk(DAY_5));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let version = &by_id(&planned, &recheck_id(PIN)).versions[0];
    assert_eq!(version.key, changed.id.to_string());
    assert_eq!(stamped(version), DAY_3);
    let found = followup(version);
    assert_eq!(found.created, date("2026-09-06"));
    assert_eq!(
        found.trigger,
        Some(Trigger::LookAgain {
            on: date("2026-12-01"),
            why: "The board ships".to_owned(),
        })
    );
    assert_eq!(found.about, Some(document_id(Kind::Fact, PIN)));
}

#[test]
fn a_recheck_cleared_or_on_an_ended_fact_makes_no_followup() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    let set = rechecked(&old, &pin, "2026-11-01 The board ships", desk(DAY_2));
    old.changed(&set, "save", &[("recheck", scalar(""))], desk(DAY_3));
    let dimmer = old.fact("lantern/dimmer", "Dims by half", &[], desk(DAY_1));
    let set = rechecked(&old, &dimmer, "touching lantern", desk(DAY_2));
    old.removed(&set, "It never dimmed", desk(DAY_3));
    let store = read(&old);
    let file = "[documents.\"fact/lantern/dimmer\"]\nended = \"false\"\n";
    let planned = plan(&store, &answers(file));

    assert_eq!(planned.questions, []);
    assert_eq!(of_kind(&planned, KindOf::Followup).len(), 0);
}

#[test]
fn an_idea_with_a_recheck_has_its_followup() {
    let old = with_topics();
    let idea = old.idea("lantern/solar-panel", "Charge by day", &[], desk(DAY_1));
    rechecked(&old, &idea, "2026-11-01 Spring comes", desk(DAY_1));
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(planned.questions, []);
    let version = &by_id(&planned, &recheck_id("lantern/solar-panel")).versions[0];
    let about = document_id(Kind::Fact, "lantern/solar-panel");
    assert_eq!(followup(version).about, Some(about));
    assert_eq!(followup(version).summary, "Spring comes");
}

#[test]
fn a_followup_of_a_fact_filed_again_is_filed_under_its_new_topic() {
    let old = with_topics();
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    let battery = old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    rechecked(&old, &battery, "2026-11-01 Winter comes", desk(DAY_2));
    old.removed(&phone, "Sold", desk(DAY_3));
    let store = read(&old);

    let asked = plan(&store, &answers(ENDED_PHONE));
    assert_eq!(
        asked.questions,
        [Question::Open(Open::EndedTopic {
            document: key(Kind::Fact, "phone/battery"),
            through: Through::FactTopic,
            topic: "phone".to_owned(),
        })]
    );
    let id = recheck_id("phone/battery");
    assert_eq!(by_id(&asked, &id).versions[0].record, None);

    let decisions = answers(&format!(
        "{CONVERSION}{ENDED_PHONE}[documents.\"fact/phone/battery\"]\ntopics = [\"atlas\"]\n"
    ));
    let planned = plan(&store, &decisions);
    assert_eq!(planned.questions, []);
    let version = &by_id(&planned, &id).versions[0];
    assert_eq!(followup(version).topics, [topic_id("atlas")]);
    assert_eq!(stamped(version), DAY_2);
}

#[test]
fn a_recheck_that_is_neither_form_blocks() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    let mut fields = pin.fields.clone();
    fields.set("recheck", scalar("sometime"));
    let vague = composed(PIN, desk(DAY_2), "save", &[&pin], fields, &pin.body);
    old.write(&vague);
    let store = read(&old);
    let planned = plan(&store, &Decisions::default());

    assert_eq!(
        planned.questions,
        [Question::Blocked(Blocked::Value {
            document: pin_key(),
            version: vague.id.to_string(),
            field: "recheck".to_owned(),
            value: "sometime".to_owned(),
            why: "is neither a day with a reason nor `touching` with a topic".to_owned(),
        })]
    );
    assert_eq!(of_kind(&planned, KindOf::Followup).len(), 0);
}

#[test]
fn the_run_of_a_recheck_starts_anew_at_a_merge_and_after_a_tombstone() {
    let old = with_topics();
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    let set = rechecked(&old, &pin, "2026-11-01 The board ships", desk(DAY_2));
    let (one, other) = old.forked(&set, desk(DAY_3), desk(DAY_3));
    let heads = [&one, &other];
    let merge = composed(
        PIN,
        desk(DAY_4),
        "resolve",
        &heads,
        one.fields.clone(),
        &one.body,
    );
    old.write(&merge);
    old.saved(&merge, "Pin 6", "\nMoved.\n", desk(DAY_5));
    let dimmer = old.fact("lantern/dimmer", "Dims by half", &[], desk(DAY_1));
    let set = rechecked(&old, &dimmer, "2026-11-01 Winter comes", desk(DAY_2));
    let gone = old.removed(&set, "It never dimmed", desk(DAY_3));
    let back = old.changed(&gone, "save", &[("summary", scalar("Dims"))], desk(DAY_4));
    old.saved(&back, "Dims by a third", "\nA third.\n", desk(DAY_5));
    let store = read(&old);
    let file = "[documents.\"fact/lantern/dimmer\"]\nended = \"false\"\n";
    let planned = plan(&store, &answers(file));

    assert_eq!(planned.questions, []);
    let joined = &by_id(&planned, &recheck_id(PIN)).versions[0];
    assert_eq!(joined.key, merge.id.to_string());
    assert_eq!(stamped(joined), DAY_4);
    assert_eq!(followup(joined).created, date("2026-09-08"));
    let restored = &by_id(&planned, &recheck_id("lantern/dimmer")).versions[0];
    assert_eq!(restored.key, back.id.to_string());
    assert_eq!(followup(restored).created, date("2026-09-08"));
}
