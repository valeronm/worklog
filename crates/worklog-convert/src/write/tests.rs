use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use worklog::app::check::{Finding, check};
use worklog::app::context::context;
use worklog::app::list::{facts, followups};
use worklog::app::setup::init;
use worklog::app::show::{Shown, history, show};
use worklog::domain::id::{DocumentId, VersionId};
use worklog::domain::schema::{Content, Date, KindOf, Reason, Record};
use worklog::domain::version::{Envelope, Kind as NewKind, Links, Stamp, Version as NewVersion};
use worklog::wiring::{Adapters, wire};

use super::compose::Defect;
use super::compose::{Composed, compose, file_of};
use super::counts::counted;
use super::counts::{Counts, Kinded};
use super::place::Place;
use super::verify::Fault;
use super::verify::verify;
use super::{Failure, ready};
use crate::decisions::Decisions;
use crate::fixture::{
    BENCH, CONVERSION, DAY_1, DAY_2, DAY_3, DAY_4, DAY_5, ENDED_PHONE, FILE, OldStore, PIN, SCALE,
    at, composed, desk, in_another_order, no_host, planned_of, read, rich, tree, with_desk,
};
use crate::old::{Kind, Store};
use crate::plan::{Complete, Document, Origin, Outcome, Planned, document_id, generated_id, plan};
use crate::question::Question;

mod places;
mod reads;
mod shapes;

fn answers() -> Decisions {
    Decisions::parse(FILE).unwrap()
}

fn finished(store: &Store) -> Complete<'_> {
    match plan(store, &answers()) {
        Outcome::Complete(complete) => complete,
        Outcome::Open(open) => panic!("{:?} {:?}", open.questions, open.unused),
    }
}

fn editable(store: &Store) -> Planned<'_> {
    plan(store, &answers()).into_planned()
}

struct Scratch {
    kept: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        let kept = tempfile::tempdir().unwrap();
        std::fs::create_dir(kept.path().join("at")).unwrap();
        std::fs::create_dir(kept.path().join("store")).unwrap();
        Scratch { kept }
    }

    fn place(&self) -> Result<Place, Failure> {
        Place::checked(&self.target(), &self.path("store"))
    }

    fn at(&self) -> PathBuf {
        self.kept.path().join("at")
    }

    fn target(&self) -> PathBuf {
        self.at().join("new")
    }

    fn staging(&self) -> PathBuf {
        self.at().join("new.partial")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.kept.path().join(name)
    }
}

#[derive(Debug)]
struct Written {
    counts: Counts,
    notices: Vec<Finding>,
}

fn write(plan: &Complete, store: &Store, scratch: &Scratch) -> Result<Written, Failure> {
    let place = scratch.place()?;
    let ready = ready(plan, store)?;
    let notices = ready.write(&place)?;
    Ok(Written {
        counts: ready.counts().clone(),
        notices,
    })
}

fn written(store: &Store, scratch: &Scratch) -> (Vec<Composed>, Written) {
    let complete = finished(store);
    let outcome = write(&complete, store, scratch).unwrap();
    (compose(&complete).unwrap(), outcome)
}

fn wired(scratch: &Scratch) -> Adapters {
    wire(&no_host(scratch.kept.path()), Some(scratch.target()))
        .unwrap()
        .unwrap()
}

fn of<'c>(composed: &'c [Composed], id: &DocumentId) -> &'c Composed {
    (composed.iter().find(|document| document.id == *id))
        .unwrap_or_else(|| panic!("no document {id} was composed"))
}

fn ids(versions: &[&NewVersion]) -> Vec<VersionId> {
    let mut ids: Vec<VersionId> = versions.iter().map(|version| version.id.clone()).collect();
    ids.sort();
    ids
}

fn problems(failure: &Failure) -> Vec<String> {
    let Failure::Unsound(faults) = failure else {
        panic!("not unsound: {failure:?}");
    };
    faults.iter().map(ToString::to_string).collect()
}

#[test]
fn only_planning_a_store_with_nothing_open_and_nothing_unused_gives_a_complete_plan() {
    let old = rich();
    let store = read(&old);

    let Outcome::Open(unanswered) = plan(&store, &Decisions::default()) else {
        panic!("a plan with open questions is complete");
    };
    assert!(!unanswered.questions.is_empty());

    let more = format!("{FILE}[documents.\"fact/atlas/scale\"]\nsummary = \"One line\"\n");
    let Outcome::Open(surplus) = plan(&store, &Decisions::parse(&more).unwrap()) else {
        panic!("a plan with an unused decision is complete");
    };
    assert_eq!(surplus.questions, []);
    assert_eq!(surplus.unused.len(), 1);

    let Outcome::Complete(complete) = plan(&store, &answers()) else {
        panic!("a plan with nothing open is not complete");
    };
    assert_eq!(complete.planned().questions, []);
    assert_eq!(complete.planned().unused, []);
    assert_eq!(complete.planned().documents, surplus.documents);

    let legend = old.fact("atlas/legend", "Blue is water", &[], desk(DAY_1));
    old.forked(&legend, desk(DAY_2), desk(DAY_3));
    let forked = read(&old);
    let Outcome::Open(blocked) = plan(&forked, &answers()) else {
        panic!("a plan with a blocked question is complete");
    };
    let stands = |question: &&Question| matches!(question, Question::Blocked(_));
    assert_eq!(blocked.questions.iter().filter(stands).count(), 1);
}

#[test]
fn two_runs_write_the_same_bytes_at_the_same_paths() {
    let old = rich();
    let store = read(&old);
    let (one, other) = (Scratch::new(), Scratch::new());

    written(&store, &one);
    written(&store, &other);

    let first = tree(&one.target());
    assert_eq!(first, tree(&other.target()));
    let files = first.values().filter(|bytes| bytes.is_some()).count();
    assert_eq!(files, 44);
    assert!(!one.staging().exists());
}

#[test]
fn the_order_the_old_files_were_written_in_changes_no_byte() {
    let old = rich();
    let copied = in_another_order(&old);
    let (store, reordered) = (read(&old), read(&copied));
    let (one, other) = (Scratch::new(), Scratch::new());

    written(&store, &one);
    written(&reordered, &other);

    assert_eq!(tree(old.root()), tree(copied.root()));
    assert_eq!(tree(&one.target()), tree(&other.target()));
}

#[test]
fn every_file_reads_back_and_each_document_has_the_planned_last_version_as_its_one_head() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let complete = finished(&store);
    let planned = complete.planned();
    write(&complete, &store, &scratch).unwrap();

    let adapters = wired(&scratch);
    let deps = adapters.deps();
    let mut files = 0;
    for document in &planned.documents {
        let read = history(&deps, document.id.as_str()).unwrap();
        assert_eq!(read.unreadable, []);
        assert_eq!(read.versions.len(), document.versions.len());
        let heads: Vec<&VersionId> = (read.versions.iter())
            .filter(|row| row.head)
            .map(|row| &row.version)
            .collect();
        let [head] = heads[..] else {
            panic!("{} has the heads {heads:?}", document.id);
        };
        let last = document.head().unwrap();
        let path = (scratch.target().join(document.id.bucket()))
            .join(document.id.as_str())
            .join(format!("{head}.md"));
        let text = std::fs::read_to_string(path).unwrap();
        let stored = NewVersion::read(head, &text).unwrap();
        assert_eq!(stored.fields, last.record.as_ref().unwrap().fields());
        assert_eq!(stored.body, last.body);
        assert_eq!(stored.links, last.links);
        assert_eq!(Some(&stored.envelope.written), last.stamp.as_ref());
        assert_eq!(Some(&stored.envelope.machine), last.machine.as_ref());
        assert_eq!(stored.envelope.change, last.change);
        assert_eq!(stored.envelope.kind.to_string(), document.kind.word());
        let Shown::Document(shown) = show(&deps, document.id.as_str()).unwrap() else {
            panic!("a document id shows a document");
        };
        assert!(!shown.forked);
        assert_eq!(shown.reads_as, Some(document.kind));
        files += read.versions.len();
    }
    let on_disk = tree(&scratch.target());
    assert_eq!(
        on_disk.values().filter(|bytes| bytes.is_some()).count(),
        files
    );
    let directories = on_disk.values().filter(|bytes| bytes.is_none()).count();
    let buckets: std::collections::BTreeSet<&str> = (planned.documents.iter())
        .map(|document| document.id.bucket())
        .collect();
    assert_eq!(directories, planned.documents.len() + buckets.len());
}

#[test]
fn a_chain_of_four_names_its_parent_and_everything_behind_it_as_the_tool_writes_them() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let (composed, _) = written(&store, &scratch);

    let lantern = of(&composed, &document_id(Kind::Topic, "lantern"));
    let [first, second, third, fourth] = &lantern.versions[..] else {
        panic!("lantern has {} versions", lantern.versions.len());
    };
    assert_eq!(first.envelope.parents, []);
    assert_eq!(first.envelope.ancestors, []);
    assert_eq!(second.envelope.parents, std::slice::from_ref(&first.id));
    assert_eq!(second.envelope.ancestors, std::slice::from_ref(&first.id));
    assert_eq!(third.envelope.parents, std::slice::from_ref(&second.id));
    assert_eq!(third.envelope.ancestors, ids(&[first, second]));
    assert_eq!(fourth.envelope.parents, std::slice::from_ref(&third.id));
    assert_eq!(fourth.envelope.ancestors, ids(&[first, second, third]));

    for (parent, child) in [(first, second), (second, third), (third, fourth)] {
        let envelope = &child.envelope;
        let (written, machine) = (envelope.written.clone(), envelope.machine.clone());
        let saved = Envelope::following(&[parent], written, machine, &envelope.change);
        assert_eq!(saved.as_ref(), Some(envelope));
    }
    for version in &lantern.versions {
        let text = std::fs::read_to_string(scratch.target().join(file_of(version))).unwrap();
        assert_eq!(NewVersion::read(&version.id, &text).as_ref(), Ok(version));
    }
}

#[test]
fn a_version_that_joins_two_names_both_and_everything_behind_them_as_the_tool_writes_them() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let (composed, _) = written(&store, &scratch);

    let scale = of(&composed, &document_id(Kind::Fact, SCALE));
    let [first, one, other, joined] = &scale.versions[..] else {
        panic!("scale has {} versions", scale.versions.len());
    };
    assert_eq!(one.envelope.parents, std::slice::from_ref(&first.id));
    assert_eq!(other.envelope.parents, std::slice::from_ref(&first.id));
    assert_eq!(joined.envelope.parents, ids(&[one, other]));
    assert_eq!(joined.envelope.ancestors, ids(&[first, one, other]));
    assert_eq!(joined.envelope.change, "resolve");
    let envelope = &joined.envelope;
    for heads in [[one, other], [other, one]] {
        let (written, machine) = (envelope.written.clone(), envelope.machine.clone());
        let saved = Envelope::following(&heads, written, machine, "resolve");
        assert_eq!(saved.as_ref(), Some(envelope));
    }
    let text = std::fs::read_to_string(scratch.target().join(file_of(joined))).unwrap();
    assert_eq!(NewVersion::read(&joined.id, &text).as_ref(), Ok(joined));
}

#[test]
fn the_new_check_finds_no_problem_and_no_fork_in_what_was_written() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let (_, outcome) = written(&store, &scratch);

    let adapters = wired(&scratch);
    let found = check(&adapters.deps()).unwrap();
    assert_eq!(found.problems, []);
    assert_eq!(found.fork_count, 0);
    assert_eq!(found.notices, outcome.notices);
}

#[test]
fn two_entries_of_one_unknown_machine_are_written_under_the_machine_each_names() {
    let old = with_desk();
    old.machine_topic("phone", &[], &[], at(DAY_1, "phone"));
    let (lamp, wiring) = ("2026-09/2026-09-04-lamp", "2026-09/2026-09-05-wiring");
    for slug in [lamp, wiring] {
        let draft = OldStore::new().entry(slug, "Drove it", &[], at(DAY_1, "bench"));
        let first = composed(slug, desk(DAY_1), "new", &[], draft.fields, &draft.body);
        old.write(&first);
        old.saved(&first, "Drove it again", "\nAgain.\n", desk(DAY_2));
    }
    let store = read(&old);
    let file = format!(
        "[documents.\"entry/{lamp}\"]\nmachine = \"phone\"\n\
         [documents.\"entry/{wiring}\"]\nmachine = \"desk\"\n"
    );
    let outcome = plan(&store, &Decisions::parse(&file).unwrap());
    assert_eq!(outcome.planned().questions, []);
    assert_eq!(outcome.planned().unused, []);
    let Outcome::Complete(complete) = outcome else {
        panic!("a plan with nothing open is complete");
    };
    let scratch = Scratch::new();
    let outcome = write(&complete, &store, &scratch).unwrap();

    let adapters = wired(&scratch);
    let found = check(&adapters.deps()).unwrap();
    assert_eq!(found.problems, []);
    assert_eq!(found.fork_count, 0);
    assert_eq!(found.notices, outcome.notices);
    let composed = compose(&complete).unwrap();
    let worked_on = |slug: &str| {
        let entry = of(&composed, &document_id(Kind::Entry, slug));
        let machine = |version: &NewVersion| {
            let machine = version.fields.get("machine")?.as_str()?;
            Some(machine.to_owned())
        };
        entry.versions.iter().map(machine).collect::<Vec<_>>()
    };
    let topic = |name: &str| Some(document_id(Kind::Topic, name).as_str().to_owned());
    assert_eq!(worked_on(lamp), [topic("phone"), topic("phone")]);
    assert_eq!(worked_on(wiring), [topic("desk"), topic("desk")]);
}

#[test]
fn what_was_read_and_what_was_written_are_counted_and_add_up() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let (_, outcome) = written(&store, &scratch);

    let kinded = |kind, documents, versions| Kinded {
        kind,
        documents,
        versions,
    };
    let expected = Counts {
        old_documents: 18,
        old_versions: 40,
        kinds: vec![
            kinded(KindOf::Topic, 6, 12),
            kinded(KindOf::Fact, 4, 12),
            kinded(KindOf::Entry, 3, 8),
            kinded(KindOf::Followup, 4, 9),
            kinded(KindOf::Claim, 2, 3),
        ],
        converted: 37,
        renames: 3,
        generated: 4,
        generated_documents: 3,
        authored: 3,
    };
    assert_eq!(outcome.counts, expected);
    let counts = &outcome.counts;
    assert_eq!(counts.old_versions, counts.converted + counts.renames);
    let versions: usize = counts.kinds.iter().map(|kind| kind.versions).sum();
    let documents: usize = counts.kinds.iter().map(|kind| kind.documents).sum();
    assert_eq!(
        versions,
        counts.converted + counts.generated + counts.authored
    );
    assert_eq!(documents, 19);
    let files = tree(&scratch.target());
    let files = files.values().filter(|bytes| bytes.is_some()).count();
    assert_eq!(versions, files);
}

#[test]
fn a_plan_that_leaves_an_old_version_out_is_not_written() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let mut planned = editable(&store);
    let lantern = planned_of(&mut planned, &document_id(Kind::Topic, "lantern"));
    let lost = lantern.versions.pop().unwrap();
    let battery = planned_of(&mut planned, &document_id(Kind::Fact, "phone/battery"));
    let twice = battery.versions[1].origin.clone();
    battery.versions[2].origin = twice;

    let complete = Complete::unchecked(planned.clone());
    let Err(Failure::Miscount(miscount)) = write(&complete, &store, &scratch) else {
        panic!("a plan that does not add up was written");
    };

    assert_eq!(miscount.lost, [format!("topic/lantern/{}", lost.key)]);
    let again = &planned_of(&mut planned, &document_id(Kind::Fact, "phone/battery")).versions[1];
    assert_eq!(
        miscount.surplus,
        [format!("fact/phone/battery/{}", again.key)]
    );
    assert_eq!(miscount.counts.old_versions, 40);
    assert_eq!(miscount.counts.converted, 37);
    assert_eq!(miscount.counts.renames, 3);
    let text = Failure::Miscount(miscount).to_string();
    assert!(text.contains("40 versions were read"), "{text}");
    assert!(text.contains("defect of the converter"), "{text}");
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

#[test]
fn a_plan_whose_counts_alone_are_off_is_not_written() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let mut planned = editable(&store);
    let coil = planned_of(&mut planned, &document_id(Kind::Fact, "relay/coil"));
    let Origin::Converted(source) = &mut coil.versions[0].origin else {
        panic!("the coil is converted");
    };
    source.stone = Some(crate::plan::Stored {
        document: source.document,
        version: source.version,
    });

    let complete = Complete::unchecked(planned.clone());
    let Err(Failure::Miscount(miscount)) = counted(&store, &complete).map_err(Failure::Miscount)
    else {
        panic!("a plan that does not add up was counted");
    };
    assert_eq!(miscount.lost, [""; 0]);
    assert_eq!(miscount.surplus.len(), 1);
    assert_eq!(miscount.counts.renames, 4);
    assert!(write(&complete, &store, &scratch).is_err());
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

#[test]
fn a_document_planned_twice_or_a_key_held_twice_is_a_defect() {
    let old = rich();
    let store = read(&old);
    let id = document_id(Kind::Fact, "relay/coil");

    let mut planned = editable(&store);
    let coil = planned_of(&mut planned, &id).clone();
    planned.documents.push(coil);
    let complete = Complete::unchecked(planned.clone());
    let twice = Defect::Twice {
        document: id.clone(),
    };
    assert_eq!(compose(&complete), Err(twice));

    let mut planned = editable(&store);
    let lantern = document_id(Kind::Topic, "lantern");
    let chain = planned_of(&mut planned, &lantern);
    let key = chain.versions[0].key.clone();
    chain.versions.truncate(2);
    chain.versions[1].key.clone_from(&key);
    let complete = Complete::unchecked(planned.clone());
    let defect = Defect::KeyTwice {
        document: lantern,
        version: key,
    };
    assert_eq!(compose(&complete), Err(defect));
}

fn unsound(at: &dyn Fn() -> Scratch, change: impl FnOnce(&mut Planned)) -> (Scratch, Failure) {
    let old = rich();
    let store = read(&old);
    let scratch = at();
    let mut planned = editable(&store);
    change(&mut planned);
    let complete = Complete::unchecked(planned.clone());
    let refused = write(&complete, &store, &scratch).unwrap_err();
    (scratch, refused)
}

#[test]
fn a_store_the_new_check_faults_is_removed_and_never_stands_at_the_target() {
    let coil = document_id(Kind::Fact, "relay/coil");
    let nowhere = generated_id("nowhere");
    let (scratch, refused) = unsound(&Scratch::new, |planned| {
        let last = planned_of(planned, &coil).versions.last_mut().unwrap();
        let record = last.record.as_mut().unwrap();
        let Content::Fact(fact) = &mut record.content else {
            panic!("the coil is a fact");
        };
        fact.topic = nowhere.clone();
    });

    let Failure::Unsound(faults) = &refused else {
        panic!("not unsound: {refused:?}");
    };
    let [
        Fault::Version { document, what, .. },
        Fault::Problem(finding),
    ] = &faults[..]
    else {
        panic!("{faults:?}");
    };
    assert_eq!(*document, coil);
    assert_eq!(*what, format!("topic: {nowhere} is no written document"));
    assert_eq!(finding.document, Some(coil));
    assert_eq!(
        finding.what,
        format!("topic: {} is missing", nowhere.short())
    );
    let text = refused.to_string();
    assert!(text.contains("defect of the converter"), "{text}");
    assert!(
        text.contains(&finding.what) && text.contains(&finding.label),
        "{text}"
    );
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

fn unjoined(planned: &mut Planned) {
    let scale = planned_of(planned, &document_id(Kind::Fact, SCALE));
    scale.versions[3].parents.clear();
}

#[test]
fn a_document_written_with_several_heads_is_faulted_as_a_fork() {
    let scale = document_id(Kind::Fact, SCALE);
    let (scratch, refused) = unsound(&Scratch::new, unjoined);
    let faults = problems(&refused);
    assert_eq!(faults.len(), 2, "{faults:?}");
    let heads = format!("{scale}: has 3 heads, ");
    assert!(faults[0].starts_with(&heads), "{faults:?}");
    assert_eq!(faults[1], "1 document is forked");
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

#[test]
fn a_failure_leaves_a_target_that_was_an_empty_directory_as_it_was() {
    let empty = || {
        let scratch = Scratch::new();
        std::fs::create_dir(scratch.target()).unwrap();
        scratch
    };
    let before = tree(&empty().at());
    assert_eq!(before.len(), 1);

    let (scratch, refused) = unsound(&empty, unjoined);
    assert!(matches!(refused, Failure::Unsound(_)), "{refused:?}");
    assert_eq!(tree(&scratch.at()), before);
}

fn staged() -> (Scratch, Vec<Composed>) {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let complete = finished(&store);
    let composed = compose(&complete).unwrap();
    for version in composed.iter().flat_map(|document| &document.versions) {
        worklog::wiring::write_version(&scratch.staging(), version).unwrap();
    }
    (scratch, composed)
}

#[test]
fn what_is_staged_as_planned_verifies_and_changes_nothing_staged() {
    let (scratch, composed) = staged();
    let before = tree(&scratch.at());
    assert_eq!(verify(&composed, &scratch.staging()), Ok(Vec::new()));
    assert_eq!(tree(&scratch.at()), before);
}

#[test]
fn a_staged_file_that_is_not_its_hash_is_reported_unreadable() {
    let (scratch, composed) = staged();
    let coil = of(&composed, &document_id(Kind::Fact, "relay/coil"));
    let version = &coil.versions[0];
    let path = scratch.staging().join(file_of(version));
    let damaged = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Twelve", "Eleven");
    std::fs::write(&path, damaged).unwrap();

    let faults = verify(&composed, &scratch.staging()).unwrap_err();
    let unreadable = Fault::Unreadable {
        document: coil.id.clone(),
        version: version.id.clone(),
        why: "the bytes do not hash to the file's name".to_owned(),
    };
    assert!(faults.contains(&unreadable), "{faults:?}");
    let reported = |fault: &Fault| {
        matches!(fault, Fault::Problem(finding)
        if finding.document.as_ref() == Some(&coil.id) && finding.what.contains(version.id.short()))
    };
    assert!(faults.iter().any(reported), "{faults:?}");
}

#[test]
fn a_staged_tree_with_a_file_too_many_or_too_few_is_faulted() {
    let (scratch, composed) = staged();
    let coil = of(&composed, &document_id(Kind::Fact, "relay/coil"));
    let version = &coil.versions[0];
    let path = scratch.staging().join(file_of(version));
    let relative = path.strip_prefix(scratch.staging()).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(scratch.staging().join("stray"), "").unwrap();
    let extra = generated_id("extra");
    let other = scratch.staging().join(extra.bucket()).join(extra.as_str());
    std::fs::create_dir_all(&other).unwrap();

    let faults = verify(&composed, &scratch.staging()).unwrap_err();
    let absent = Fault::Absent {
        path: relative.display().to_string(),
    };
    assert!(faults.contains(&absent), "{faults:?}");
    let stray = |path: String| Fault::Stray { path };
    assert!(faults.contains(&stray("stray".to_owned())), "{faults:?}");
    let other = format!("{}/{extra}", extra.bucket());
    assert!(faults.contains(&stray(other)), "{faults:?}");
    let missing =
        |fault: &Fault| matches!(fault, Fault::Missing { document, .. } if *document == coil.id);
    assert!(faults.iter().any(missing), "{faults:?}");
}

#[test]
fn a_document_holding_a_version_no_plan_made_is_faulted() {
    let (scratch, composed) = staged();
    let lantern = of(&composed, &document_id(Kind::Topic, "lantern"));
    let last = lantern.versions.last().unwrap();
    let envelope = &last.envelope;
    let (written, machine) = (envelope.written.clone(), envelope.machine.clone());
    let envelope = Envelope::following(&[last], written, machine, "save").unwrap();
    let more = NewVersion::compose(
        envelope,
        last.fields.clone(),
        last.links.clone(),
        "\nMore.\n".to_owned(),
    )
    .unwrap();
    worklog::wiring::write_version(&scratch.staging(), &more).unwrap();

    let faults = verify(&composed, &scratch.staging()).unwrap_err();
    let strays: Vec<&Fault> = (faults.iter())
        .filter(|fault| matches!(fault, Fault::Stray { .. }))
        .collect();
    assert_eq!(strays.len(), 1, "{faults:?}");
    let held = |fault: &Fault| {
        matches!(fault, Fault::Versions { document, held, planned }
        if *document == lantern.id && held.len() == 5 && planned.len() == 4)
    };
    assert!(faults.iter().any(held), "{faults:?}");
    let heads = Fault::Heads {
        document: lantern.id.clone(),
        heads: vec![more.id.clone()],
        planned: last.id.clone(),
    };
    assert!(faults.contains(&heads), "{faults:?}");
}

fn fixed(
    key: &str,
    parents: &[&str],
    written: &str,
    summary: &str,
) -> crate::plan::Version<'static> {
    let fields = format!("name = \"lantern\"\ncreated = 2026-09-04\nsummary = \"{summary}\"\n");
    let kind = NewKind::parse("topic").unwrap();
    let record = Record::read(&kind, &fields.parse().unwrap()).unwrap();
    let mut links = Links::default();
    (links.0).insert("atlas".to_owned(), Some(DocumentId::from_bytes([0xa7; 16])));
    links.0.insert("nowhere".to_owned(), None);
    crate::plan::Version {
        key: key.to_owned(),
        parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
        outside: Vec::new(),
        stamp: Some(Stamp::parse(written).unwrap()),
        machine: Some(DocumentId::from_bytes([0xd5; 16])),
        change: if parents.is_empty() { "new" } else { "save" }.to_owned(),
        record: Some(record),
        body: format!("\n{summary}, see [[atlas]] and [[nowhere]].\n"),
        links,
        origin: Origin::Authored { answered: None },
    }
}

#[test]
fn a_fixed_plan_composes_to_the_same_version_ids_on_every_machine() {
    let document = Document {
        id: DocumentId::from_bytes([0x4c; 16]),
        kind: KindOf::Topic,
        versions: vec![
            fixed("one", &[], "2026-09-04T10:15:07.412093+01:00", "A lamp"),
            fixed("two", &["one"], "2026-09-05T08:00:00Z", "A lamp on a relay"),
        ],
    };
    let planned = Planned {
        documents: vec![document],
        ..Planned::default()
    };

    let composed = compose(&Complete::unchecked(planned)).unwrap();
    let ids: Vec<&str> = (composed[0].versions.iter())
        .map(|version| version.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "b3-2d867b760304050d5ab7f45d2902a3166364183ef9c91d4d93eabe8e0026859a",
            "b3-c8523a2a8a9579b284b859c9d98277ae30f62fe9d0c6c6a6692f64ba1b4eead4",
        ]
    );
}

fn recorded(
    kind: &str,
    fields: &str,
    key: &str,
    parents: &[&str],
    body: &str,
) -> crate::plan::Version<'static> {
    let record = Record::read(&NewKind::parse(kind).unwrap(), &fields.parse().unwrap());
    crate::plan::Version {
        key: key.to_owned(),
        parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
        outside: Vec::new(),
        stamp: Some(Stamp::parse("2026-09-06T23:30:00.25-07:00").unwrap()),
        machine: Some(DocumentId::from_bytes([0xd5; 16])),
        change: key.to_owned(),
        record: Some(record.unwrap()),
        body: body.to_owned(),
        links: Links::default(),
        origin: Origin::Authored { answered: None },
    }
}

fn composed_ids(id: u8, kind: KindOf, versions: Vec<crate::plan::Version<'static>>) -> Vec<String> {
    let document = Document {
        id: DocumentId::from_bytes([id; 16]),
        kind,
        versions,
    };
    let planned = Planned {
        documents: vec![document],
        ..Planned::default()
    };
    let composed = compose(&Complete::unchecked(planned)).unwrap();
    let ids = composed[0].versions.iter().map(|version| &version.id);
    ids.map(ToString::to_string).collect()
}

#[test]
fn a_record_of_every_shape_composes_to_the_same_version_ids_on_every_machine() {
    let topic = "a7".repeat(16);
    let other = "b8".repeat(16);
    let fact = format!(
        "name = \"relay-pin\"\nformer_names = [\"pin\", \"relay\"]\ntopic = \"{topic}\"\n\
         created = 2026-09-04\nconfirmed = 2026-09-05\nidea = true\n\
         summary = 'Pin \"5\" at 3,3 V \\ naïve, über, 灯'\n"
    );
    let ended = format!(
        "{fact}ended = \"abandoned\"\nended_on = 2026-09-06\n\
         note = 'Said \"no\": c:\\lamp, 灯'\nended_by = \"{other}\"\n"
    );
    assert_eq!(
        composed_ids(
            0x1f,
            KindOf::Fact,
            vec![
                recorded("fact", &fact, "new", &[], "\nA \"quoted\" body, 灯.\n"),
                recorded("fact", &ended, "abandoned", &["new"], "\n"),
            ]
        ),
        [
            "b3-428f7b45cabeb34be9d0320823ed30dce33cebb185ea266c2826183921de46f8",
            "b3-f809e92d0cf7f051def8f5616055fe5e535793ef432a20b923a10a3af67608c8",
        ]
    );

    let followup = format!(
        "created = 2026-09-04\ntopics = [\"{topic}\", \"{other}\"]\nentry = \"{other}\"\n\
         about = \"{topic}\"\nlook_again = 2026-11-01\nwhy = 'The \"board\" ships'\n\
         summary = \"Port it\"\n"
    );
    let touching = format!(
        "created = 2026-09-04\ntopics = [\"{topic}\"]\ntouching = \"{topic}\"\n\
         summary = \"Port it\"\nended = \"done\"\nended_on = 2026-09-07\nnote = \"\"\n"
    );
    assert_eq!(
        composed_ids(
            0x2e,
            KindOf::Followup,
            vec![
                recorded("followup", &followup, "new", &[], "\n"),
                recorded("followup", &touching, "done", &["new"], "\n"),
            ]
        ),
        [
            "b3-724c943c643a67fdf5529c9598c6a448c5cac4b4c75c979a2fde59e9424a2957",
            "b3-a6057626f330a49fc4b1ea5101c0e0d3807bbb4e2ea0319ef3fdabe3b7e07aaf",
        ]
    );

    let entry = format!(
        "name = \"lamp\"\ndate = 2026-09-04\nmachine = \"{other}\"\ntopics = []\n\
         summary = \"Drove it\"\n"
    );
    assert_eq!(
        composed_ids(
            0x3d,
            KindOf::Entry,
            vec![recorded("entry", &entry, "new", &[], "\n## What\n")]
        ),
        ["b3-31cef248e3067123b55f03b971049d2ce2a528d1bbc2c38b92269b4abb648b91",]
    );
}

fn broken(change: impl FnOnce(&mut Planned)) -> (Vec<Composed>, Vec<Fault>) {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let mut planned = editable(&store);
    change(&mut planned);
    let complete = Complete::unchecked(planned);
    let composed = compose(&complete).unwrap();
    let refused = write(&complete, &store, &scratch).unwrap_err();
    let Failure::Unsound(faults) = refused else {
        panic!("not unsound: {refused:?}");
    };
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
    (composed, faults)
}

fn content<'p>(planned: &'p mut Planned, id: &DocumentId, at: usize) -> &'p mut Content {
    let version = &mut planned_of(planned, id).versions[at];
    &mut version.record.as_mut().unwrap().content
}

fn rule(composed: &[Composed], id: &DocumentId, at: usize, what: &str) -> Fault {
    Fault::Version {
        document: id.clone(),
        version: of(composed, id).versions[at].id.clone(),
        what: what.to_owned(),
    }
}

#[test]
fn a_version_behind_the_head_that_reads_as_no_record_is_faulted() {
    let lantern = document_id(Kind::Topic, "lantern");
    let (composed, faults) = broken(|planned| {
        let Content::Topic(topic) = content(planned, &lantern, 1) else {
            panic!("lantern is a topic");
        };
        topic.summary = String::new();
    });
    let version = &of(&composed, &lantern).versions[1].id;
    let [
        Fault::Version {
            document,
            version: faulted,
            what,
        },
    ] = &faults[..]
    else {
        panic!("{faults:?}");
    };
    assert_eq!((document, faulted), (&lantern, version));
    assert!(what.starts_with("does not read as a topic: "), "{what}");
    assert!(what.contains("summary"), "{what}");
    let text = faults[0].to_string();
    assert!(
        text.starts_with(&format!("{lantern}: {version}: does not")),
        "{text}"
    );
}

#[test]
fn a_reference_of_an_ended_head_to_no_document_or_one_of_another_kind_is_faulted() {
    let solder = document_id(Kind::Followup, "2026-09-03-solder");
    let (nowhere, lantern) = (generated_id("nowhere"), document_id(Kind::Topic, "lantern"));
    let (composed, faults) = broken(|planned| {
        let Content::Followup(followup) = content(planned, &solder, 2) else {
            panic!("solder is a followup");
        };
        followup.topics = vec![nowhere.clone()];
        followup.entry = Some(lantern.clone());
    });
    let expected = [
        rule(
            &composed,
            &solder,
            2,
            &format!("topics: {nowhere} is no written document"),
        ),
        rule(
            &composed,
            &solder,
            2,
            &format!("entry: {lantern} is written as topic, not as entry"),
        ),
        rule(
            &composed,
            &solder,
            2,
            &format!(
                "entry: is not what the first version {} has",
                of(&composed, &solder).versions[0].id
            ),
        ),
    ];
    assert_eq!(faults, expected);

    let wiring = document_id(Kind::Entry, "2026-09/2026-09-05-wiring");
    let (composed, faults) = broken(|planned| {
        let last = planned_of(planned, &wiring).versions.last_mut().unwrap();
        let ending = last.record.as_mut().unwrap().ending.as_mut().unwrap();
        ending.by = Some(nowhere.clone());
    });
    let what = format!("ended_by: {nowhere} is no written document");
    assert_eq!(faults, [rule(&composed, &wiring, 1, &what)]);
}

#[test]
fn a_version_written_by_what_is_no_written_topic_is_faulted() {
    let lantern = document_id(Kind::Topic, "lantern");
    let (nowhere, coil) = (
        generated_id("nowhere"),
        document_id(Kind::Fact, "relay/coil"),
    );
    let (composed, faults) = broken(|planned| {
        let versions = &mut planned_of(planned, &lantern).versions;
        versions[1].machine = Some(nowhere.clone());
        versions[2].machine = Some(coil.clone());
    });
    let by = "the machine that wrote it";
    let expected = [
        rule(
            &composed,
            &lantern,
            1,
            &format!("{by}: {nowhere} is no written document"),
        ),
        rule(
            &composed,
            &lantern,
            2,
            &format!("{by}: {coil} is written as fact, not as topic"),
        ),
    ];
    assert_eq!(faults, expected);
}

#[test]
fn a_set_once_field_that_changes_between_versions_is_faulted() {
    let battery = document_id(Kind::Fact, "phone/battery");
    let (composed, faults) = broken(|planned| {
        let Content::Fact(fact) = content(planned, &battery, 1) else {
            panic!("the battery is a fact");
        };
        fact.created = Date::parse("2026-01-01").unwrap();
    });
    let first = &of(&composed, &battery).versions[0].id;
    let what = format!("created: is not what the first version {first} has");
    assert_eq!(faults, [rule(&composed, &battery, 1, &what)]);
}

#[test]
fn a_name_a_version_had_and_a_later_one_lacks_is_faulted() {
    let pin = document_id(Kind::Fact, PIN);
    let (composed, faults) = broken(|planned| {
        let Content::Fact(fact) = content(planned, &pin, 3) else {
            panic!("the pin is a fact");
        };
        assert_eq!(fact.former_names.len(), 1);
        fact.former_names.clear();
    });
    let what = "former_names: lacks relay-pin, a name of a version behind it";
    assert_eq!(faults, [rule(&composed, &pin, 3, what)]);
}

#[test]
fn an_ending_for_a_reason_its_kind_has_none_of_is_faulted() {
    let wiring = document_id(Kind::Entry, "2026-09/2026-09-05-wiring");
    let (composed, faults) = broken(|planned| {
        let last = planned_of(planned, &wiring).versions.last_mut().unwrap();
        last.record
            .as_mut()
            .unwrap()
            .ending
            .as_mut()
            .unwrap()
            .reason = Reason::Done;
    });
    let what = "ended: done is no reason of the kind entry";
    assert_eq!(faults, [rule(&composed, &wiring, 1, what)]);
}

#[test]
fn the_counts_print_what_was_read_what_was_written_by_kind_and_where_each_version_came_from() {
    let old = rich();
    let store = read(&old);
    let counts = counted(&store, &finished(&store)).unwrap();
    assert_eq!(
        counts.shown(true),
        "read: 18 documents, 40 versions\n\
         written: 19 documents, 44 versions\n\
         \x20 topic: 6 documents, 12 versions\n\
         \x20 fact: 4 documents, 12 versions\n\
         \x20 entry: 3 documents, 8 versions\n\
         \x20 followup: 4 documents, 9 versions\n\
         \x20 claim: 2 documents, 3 versions\n\
         versions written, by what each was made from:\n\
         \x20 a version of the store: 37, of which 3 from the two halves of a rename\n\
         \x20 a version of another document: 4, of which 3 begin a document\n\
         \x20 the decision file alone: 3"
    );

    let one = Counts {
        old_documents: 1,
        old_versions: 1,
        kinds: vec![Kinded {
            kind: KindOf::Topic,
            documents: 1,
            versions: 1,
        }],
        converted: 1,
        renames: 0,
        generated: 0,
        generated_documents: 0,
        authored: 0,
    };
    assert!(
        one.shown(true).starts_with(
            "read: 1 document, 1 version\n\
             written: 1 document, 1 version\n\
             \x20 topic: 1 document, 1 version\n"
        ),
        "{}",
        one.shown(true)
    );
}
