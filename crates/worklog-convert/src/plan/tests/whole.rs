use std::collections::BTreeMap;

use super::*;

#[test]
fn every_planned_version_of_every_kind_reads_as_the_new_schema_reads_it() {
    let old = rich();
    let store = read(&old);
    let decisions = answers(FILE);
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(decisions.unused(), []);
    let mut read_back: BTreeMap<&str, usize> = BTreeMap::new();
    for document in &planned.documents {
        let kind = NewKind::parse(document.kind.word()).unwrap();
        for version in &document.versions {
            let record = version.record.as_ref().unwrap();
            assert_eq!(Record::read(&kind, &record.fields()).as_ref(), Ok(record));
            assert!(version.stamp.is_some());
            assert_eq!(version.machine, Some(topic_id("desk")));
            *read_back.entry(document.kind.word()).or_default() += 1;
        }
    }
    let expected = [
        ("claim", 3),
        ("entry", 8),
        ("fact", 12),
        ("followup", 9),
        ("topic", 12),
    ];
    assert_eq!(read_back, BTreeMap::from(expected));
    let versions: usize = (store.documents.iter())
        .map(|document| document.versions.len())
        .sum();
    assert_eq!(versions, 40);
    assert_eq!(planned.authored.len(), 3);
    let kinds: Vec<KindOf> = (planned.documents.iter())
        .map(|document| document.kind)
        .collect();
    let mut ordered = kinds.clone();
    ordered.sort_by_key(|kind| KindOf::ALL.iter().position(|known| known == kind));
    assert_eq!(kinds, ordered);
}

#[test]
fn a_whole_store_is_planned_the_same_in_any_run_and_any_order_of_files() {
    let old = rich();
    let (one, other) = (read(&old), read(&in_another_order(&old)));
    let (answered, unanswered) = (answers(FILE), Decisions::default());

    let first = plan(&one, &answered);
    assert_eq!(first, plan(&one, &answered));
    assert_eq!(first, plan(&other, &answers(FILE)));
    let open = plan(&one, &unanswered);
    assert_eq!(open, plan(&one, &unanswered));
    assert_eq!(open, plan(&other, &Decisions::default()));
    let mut sorted = open.questions.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(open.questions, sorted);
    assert_eq!(opens(&open).len(), 10);
    let mut shuffled = one.clone();
    shuffled.documents.reverse();
    shuffled.documents.rotate_left(3);
    assert_ne!(shuffled.documents, one.documents);
    assert_eq!(plan(&shuffled, &answers(FILE)), first);
    assert_eq!(plan(&shuffled, &Decisions::default()), open);
    let linked = &of(&first, Kind::Fact, PIN).versions[1];
    assert_eq!(linked.body, "\nFound in [[2026-09-04-lamp]].\n");
    assert_eq!(linked.links.0.len(), 2);
    let again = &by_id(&first, &generated_id("recheck\0lantern/relay-pin")).versions[0];
    let summary = again.record.as_ref().unwrap().fields()["summary"].clone();
    assert_eq!(summary.as_str(), Some("See [[2026-09-04-lamp]]"));
    assert_eq!(again.links.0.len(), 1);
    let created = &by_id(&first, &created_topic_id("firmware")).versions[0];
    assert_eq!(
        created.links.0.get("lantern"),
        Some(&Some(topic_id("lantern")))
    );
}
