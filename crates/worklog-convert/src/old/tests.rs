use worklog_old::domain::frontmatter::Value as StoredValue;
use worklog_old::domain::version::Version as Stored;

use std::collections::BTreeSet;

use worklog_old::domain::version::Document as StoredDocument;

use super::read::{USAGE, document_of, version_of};
use super::{
    Document, Failure, Found, Kind, Problem, State, Store, Value, Version, foreign_block_keys,
    parent_first, read, unmatched,
};
use crate::fixture::{At, OldStore, at, composed, following, scalar};
use worklog_old::domain::ports::Store as _;
use worklog_old::domain::slug::{Kind as StoredKind, Slug};
use worklog_old::fs::FsStore;

const MORNING: &str = "2026-09-04T10:15:07.412093+01:00";
const NOON: &str = "2026-09-04T12:00:00+00:00";
const EVENING: &str = "2026-09-04T21:40:11.000300-07:00";
const NEXT_DAY: &str = "2026-09-05T08:00:00.5+01:00";

fn desk(written: &str) -> At<'_> {
    at(written, "desk")
}

fn phone(written: &str) -> At<'_> {
    at(written, "phone")
}

fn held<'a>(store: &'a Store, kind: Kind, slug: &str) -> &'a Document {
    (store.documents.iter())
        .find(|document| document.kind == kind && document.slug == slug)
        .unwrap_or_else(|| panic!("no {kind} {slug} was read"))
}

fn ids(document: &Document) -> Vec<&str> {
    (document.versions.iter())
        .map(|version| version.id.as_str())
        .collect()
}

fn listed(store: &Store) -> Vec<(String, String)> {
    (store.documents.iter())
        .map(|document| (document.kind.word().to_owned(), document.slug.clone()))
        .collect()
}

fn as_read(value: &StoredValue) -> Value {
    match value {
        StoredValue::Scalar(text) => Value::Scalar(text.clone()),
        StoredValue::List(items) => Value::List(items.clone()),
        StoredValue::Map(entries) => Value::Map(
            (entries.iter())
                .map(|(key, value)| (key.clone(), as_read(value)))
                .collect(),
        ),
    }
}

fn assert_as_stored(read: &Version, stored: &Stored) {
    let parents: Vec<String> = (stored.block.parents.iter())
        .map(ToString::to_string)
        .collect();
    let fields: Vec<(String, Value)> = (stored.fields.iter())
        .map(|(key, value)| (key.to_owned(), as_read(value)))
        .collect();
    assert_eq!(read.id, stored.id.as_str());
    assert_eq!(read.parents, parents);
    assert_eq!(read.written, stored.block.written);
    assert_eq!(read.machine, stored.block.machine.as_str());
    assert_eq!(read.operation, stored.block.operation);
    assert_eq!(read.fields, fields);
    assert_eq!(read.body, stored.body);
}

fn text(value: &str) -> Value {
    Value::Scalar(value.to_owned())
}

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|item| (*item).to_owned()).collect())
}

fn field(key: &str, value: Value) -> (String, Value) {
    (key.to_owned(), value)
}

#[test]
fn versions_come_parent_first_whatever_order_their_files_were_written_in() {
    let old = OldStore::new();
    let mut chain = vec![old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &[],
        desk(MORNING),
    )];
    for step in 0..7 {
        let body = format!("\nStep {step}.\n");
        let last = chain.last().unwrap();
        chain.push(following(last, phone(NOON), "save", &[], Some(&body)));
    }
    for version in chain.iter().skip(1).rev() {
        old.write(version);
    }

    let store = read(old.root()).unwrap();
    let causal: Vec<&str> = chain.iter().map(|version| version.id.as_str()).collect();
    let mut by_id = causal.clone();
    by_id.sort_unstable();
    assert_ne!(causal, by_id);
    assert_eq!(ids(held(&store, Kind::Fact, "lantern/relay-pin")), causal);
    assert_eq!(store.problems, []);
}

#[test]
fn versions_with_no_order_between_them_come_by_id() {
    let old = OldStore::new();
    let first = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let (one, other) = old.forked(&first, desk(NOON), phone(NOON));
    let fields = one.fields.clone();
    let merged = composed(
        "lantern",
        desk(EVENING),
        "resolve",
        &[&one, &other],
        fields,
        "\nBoth.\n",
    );
    old.write(&merged);

    let store = read(old.root()).unwrap();
    let lantern = held(&store, Kind::Topic, "lantern");
    let mut sides = [one.id.as_str(), other.id.as_str()];
    sides.sort_unstable();
    assert_eq!(
        ids(lantern),
        [first.id.as_str(), sides[0], sides[1], merged.id.as_str()]
    );
    assert_eq!(
        lantern.versions[3].parents,
        [one.id.as_str(), other.id.as_str()]
    );
    assert_eq!(lantern.state, State::Live);
    assert_eq!(store.problems, []);
}

#[test]
fn fields_and_body_are_what_the_file_holds() {
    let old = OldStore::new();
    let claims: [(&str, &[&str]); 2] = [
        ("lantern", &["~/projects/lantern", "~/projects/firmware"]),
        ("atlas", &["/srv/atlas"]),
    ];
    let machine = old.machine_topic("desk-machine", &claims, &["phone"], desk(MORNING));
    let entry = old.entry(
        "2026-09/2026-09-04-lamp-driver",
        "Wrote the lamp driver",
        &["lantern", "Relay"],
        phone(EVENING),
    );
    let ragged = "\nTrailing spaces  \n\n\tA tab, [[lantern/relay-pin]] and no last line break";
    let saved = old.saved(&entry, "Rewrote the lamp driver", ragged, desk(NEXT_DAY));

    let store = read(old.root()).unwrap();
    let read_machine = &held(&store, Kind::Topic, "desk-machine").versions[0];
    assert_as_stored(read_machine, &machine);
    assert_eq!(
        read_machine.fields,
        [
            field("summary", text("The machine desk")),
            field("machine", text("desk")),
            field(
                "claims",
                Value::Map(vec![
                    field(
                        "lantern",
                        list(&["~/projects/lantern", "~/projects/firmware"])
                    ),
                    field("atlas", list(&["/srv/atlas"])),
                ])
            ),
            field("unclaimed", list(&["phone"])),
        ]
    );
    assert_eq!(read_machine.written, MORNING);
    assert_eq!(read_machine.machine, "desk");
    assert_eq!(read_machine.operation, "new");
    assert_eq!(read_machine.foreign_block, None);

    let versions = &held(&store, Kind::Entry, "2026-09/2026-09-04-lamp-driver").versions;
    assert_as_stored(&versions[0], &entry);
    assert_as_stored(&versions[1], &saved);
    assert_eq!(
        versions[0].fields,
        [
            field("date", text("2026-09-04")),
            field("machine", text("phone")),
            field("tags", list(&["lantern", "Relay"])),
            field("files_touched", list(&[])),
            field("summary", text("Wrote the lamp driver")),
        ]
    );
    assert_eq!(versions[0].written, EVENING);
    assert_eq!(versions[1].body, ragged);
    assert_eq!(versions[1].written, NEXT_DAY);
    assert_eq!(versions[1].parents, [entry.id.as_str()]);
    assert_eq!(store.problems, []);
}

#[test]
fn a_block_key_the_grammar_does_not_know_is_kept_with_its_block() {
    let old = OldStore::new();
    let topic = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let newer = topic.to_text().replace(
        "  operation: new\n",
        "  operation: new\n  signed: desk-key\n",
    );
    old.planted(
        "topic/atlas",
        &newer.replace("slug: lantern", "slug: atlas"),
    );

    let store = read(old.root()).unwrap();
    let atlas = &held(&store, Kind::Topic, "atlas").versions[0];
    let block = atlas.foreign_block.as_ref().unwrap();
    assert_eq!(block.last(), Some(&field("signed", text("desk-key"))));
    assert_eq!(block.len(), 5);
    assert_eq!(atlas.operation, "new");
    assert_eq!(store.problems, []);
}

#[test]
fn a_renamed_document_is_read_as_both_its_slugs_with_what_ties_them() {
    let old = OldStore::new();
    let first = old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &["lantern"],
        desk(MORNING),
    );
    let (tombstone, moved) = old.renamed(&first, "lantern/relay-pin-is-fixed", phone(NOON));
    let later = old.saved(
        &moved,
        "The relay pin is soldered",
        "\nSoldered.\n",
        desk(EVENING),
    );

    let store = read(old.root()).unwrap();
    let before = held(&store, Kind::Fact, "lantern/relay-pin");
    assert_eq!(before.state, State::Renamed);
    assert_eq!(ids(before), [first.id.as_str(), tombstone.id.as_str()]);
    let read_tombstone = &before.versions[1];
    assert_eq!(
        read_tombstone.superseded_by.as_deref(),
        Some("lantern/relay-pin-is-fixed")
    );
    assert_eq!(read_tombstone.renamed_from, None);
    assert!(read_tombstone.tombstone);
    assert_eq!(read_tombstone.operation, "rename");
    assert_eq!(read_tombstone.body, "");
    assert!(!before.versions[0].tombstone);

    let after = held(&store, Kind::Fact, "lantern/relay-pin-is-fixed");
    assert_eq!(after.state, State::Live);
    assert_eq!(ids(after), [moved.id.as_str(), later.id.as_str()]);
    let read_moved = &after.versions[0];
    assert_eq!(
        read_moved.renamed_from.as_deref(),
        Some("lantern/relay-pin")
    );
    assert_eq!(read_moved.superseded_by, None);
    assert!(!read_moved.tombstone);
    assert_eq!(read_moved.parents, [tombstone.id.as_str()]);
    assert_eq!(read_moved.written, NOON);
    assert_eq!(read_moved.machine, "phone");
    assert_eq!(read_moved.body, first.body);
    assert_eq!(store.problems, []);
}

#[test]
fn a_rename_cut_short_is_a_renamed_document_and_nothing_at_the_new_slug() {
    let old = OldStore::new();
    let first = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let tombstone = old.rename_cut_short(&first, "lamp", desk(NOON));

    let store = read(old.root()).unwrap();
    let lantern = held(&store, Kind::Topic, "lantern");
    assert_eq!(lantern.state, State::Renamed);
    assert_eq!(lantern.versions[1].id, tombstone.id.as_str());
    assert_eq!(lantern.versions[1].superseded_by.as_deref(), Some("lamp"));
    assert_eq!(listed(&store), [("topic".to_owned(), "lantern".to_owned())]);
    assert_eq!(store.problems, []);
}

#[test]
fn a_removed_document_ends_in_a_tombstone_holding_the_note() {
    let old = OldStore::new();
    let first = old.idea(
        "phone/needs-beta",
        "The phone needs the beta",
        &[],
        desk(MORNING),
    );
    let tombstone = old.removed(&first, "  Built in the spring.\n", phone(NOON));

    let store = read(old.root()).unwrap();
    let idea = held(&store, Kind::Fact, "phone/needs-beta");
    assert_eq!(idea.state, State::Removed);
    let last = &idea.versions[1];
    assert_as_stored(last, &tombstone);
    assert!(last.tombstone);
    assert_eq!(last.operation, "tombstone");
    assert_eq!(last.superseded_by, None);
    assert_eq!(last.body, "Built in the spring.\n");
    assert_eq!(last.fields, idea.versions[0].fields);
    assert!(last.fields.contains(&field("idea", text("true"))));
    assert_eq!(store.problems, []);
}

#[test]
fn a_followup_later_done_keeps_each_state_and_body() {
    let old = OldStore::new();
    let entry = old.entry(
        "2026-09/2026-09-04-lamp-driver",
        "Wrote the lamp driver",
        &[],
        desk(MORNING),
    );
    let open = old.followup(
        "2026-09-04-port-the-driver",
        &entry,
        "Port the driver",
        &["atlas"],
        desk(MORNING),
    );
    let recheck = [("recheck", scalar("touching atlas"))];
    let waiting = following(&open, phone(NOON), "recheck", &recheck, None);
    old.write(&waiting);
    let done = old.done(&waiting, "Ported.", phone(EVENING));

    let store = read(old.root()).unwrap();
    let followup = held(&store, Kind::Followup, "2026-09-04-port-the-driver");
    assert_eq!(followup.state, State::Live);
    assert_eq!(
        ids(followup),
        [open.id.as_str(), waiting.id.as_str(), done.id.as_str()]
    );
    let states: Vec<Option<&Value>> = (followup.versions.iter())
        .map(|version| {
            (version.fields.iter())
                .find(|(key, _)| key == "state")
                .map(|(_, value)| value)
        })
        .collect();
    assert_eq!(
        states,
        [
            Some(&text("open")),
            Some(&text("open")),
            Some(&text("done"))
        ]
    );
    assert!(
        followup.versions[0]
            .fields
            .contains(&field("entry", text("2026-09/2026-09-04-lamp-driver")))
    );
    assert!(
        followup.versions[1]
            .fields
            .contains(&field("recheck", text("touching atlas")))
    );
    assert_eq!(followup.versions[1].body, "\n");
    assert_eq!(followup.versions[2].body, "\nPorted.\n");
    assert_eq!(followup.versions[2].operation, "done");
    assert_eq!(store.problems, []);
}

#[test]
fn a_forked_document_is_collected_with_its_heads_and_still_read() {
    let old = OldStore::new();
    let first = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let (one, other) = old.forked(&first, desk(NOON), phone(NOON));
    old.topic("atlas", "A book of maps", &[], desk(MORNING));

    let store = read(old.root()).unwrap();
    let mut heads = vec![one.id.to_string(), other.id.to_string()];
    heads.sort_unstable();
    let lantern = held(&store, Kind::Topic, "lantern");
    assert_eq!(lantern.state, State::Forked);
    assert_eq!(lantern.versions.len(), 3);
    let read_heads: Vec<&str> = lantern
        .heads()
        .iter()
        .map(|head| head.id.as_str())
        .collect();
    assert_eq!(read_heads, heads);
    assert_eq!(
        store.problems,
        [Problem::Forked {
            kind: Kind::Topic,
            slug: "lantern".to_owned(),
            heads,
        }]
    );
    assert_eq!(held(&store, Kind::Topic, "atlas").state, State::Live);
}

#[test]
fn a_document_naming_a_parent_the_store_lacks_is_collected_whatever_its_heads() {
    let old = OldStore::new();
    let first = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let second = old.saved(&first, "A lamp on a relay", "\nTwo.\n", desk(NOON));
    old.saved(&second, "A lamp on two relays", "\nThree.\n", desk(EVENING));
    old.saved(
        &first,
        "A lamp, seen from the phone",
        "\nAside.\n",
        phone(NOON),
    );
    old.withhold(&second);

    let root = old.topic("atlas", "A book of maps", &[], desk(MORNING));
    let kept = old.saved(&root, "A book of charts", "\nTwo.\n", desk(NOON));
    old.saved(&kept, "A book of sea charts", "\nThree.\n", desk(EVENING));
    old.withhold(&root);

    let base = old.topic("phone", "A phone", &[], desk(MORNING));
    let trunk = old.saved(&base, "A handset", "\nTwo.\n", desk(NOON));
    let (one, other) = old.forked(&trunk, desk(EVENING), phone(EVENING));
    old.withhold(&base);

    let store = read(old.root()).unwrap();
    let mut heads = vec![one.id.to_string(), other.id.to_string()];
    heads.sort_unstable();
    assert_eq!(held(&store, Kind::Topic, "lantern").state, State::Waiting);
    assert_eq!(held(&store, Kind::Topic, "atlas").state, State::Live);
    assert_eq!(held(&store, Kind::Topic, "atlas").versions.len(), 2);
    assert_eq!(held(&store, Kind::Topic, "phone").state, State::Forked);
    let topic = |slug: &str| (Kind::Topic, slug.to_owned());
    let (kind, slug) = topic("phone");
    assert_eq!(
        store.problems,
        [
            Problem::Forked { kind, slug, heads },
            Problem::Waiting {
                kind: Kind::Topic,
                slug: "atlas".to_owned(),
                missing: vec![root.id.to_string()],
            },
            Problem::Waiting {
                kind: Kind::Topic,
                slug: "lantern".to_owned(),
                missing: vec![second.id.to_string()],
            },
            Problem::Waiting {
                kind: Kind::Topic,
                slug: "phone".to_owned(),
                missing: vec![base.id.to_string()],
            },
        ]
    );
}

#[test]
fn a_moved_version_whose_tombstone_the_store_lacks_is_collected() {
    let old = OldStore::new();
    let first = old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &[],
        desk(MORNING),
    );
    let (tombstone, _) = old.renamed(&first, "lantern/relay-pin-is-fixed", desk(NOON));
    let other = old.fact("atlas/scale", "The scale is metric", &[], desk(MORNING));
    let (gone, _) = old.renamed(&other, "atlas/metric-scale", desk(NOON));
    old.withhold(&tombstone);
    old.withhold(&other);
    old.withhold(&gone);

    let store = read(old.root()).unwrap();
    assert_eq!(
        held(&store, Kind::Fact, "lantern/relay-pin").state,
        State::Live
    );
    assert_eq!(
        store.problems,
        [
            Problem::Waiting {
                kind: Kind::Fact,
                slug: "atlas/metric-scale".to_owned(),
                missing: vec![gone.id.to_string()],
            },
            Problem::Waiting {
                kind: Kind::Fact,
                slug: "lantern/relay-pin-is-fixed".to_owned(),
                missing: vec![tombstone.id.to_string()],
            },
        ]
    );
}

#[test]
fn a_document_with_a_file_that_does_not_read_is_collected_and_the_rest_is_read() {
    let old = OldStore::new();
    let lantern = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let later = old.saved(&lantern, "A lamp on a relay", "\nTwo.\n", desk(NOON));
    old.damage(&later, b"---\nslug: lantern\n---\n");
    let atlas = old.topic("atlas", "A book of maps", &[], desk(MORNING));
    old.damage(&atlas, &[0xff, 0xfe, 0x00]);
    let phone_topic = old.topic("phone", "A phone", &[], desk(MORNING));
    let loose = phone_topic
        .to_text()
        .replace("summary: A phone", "summary:  A phone");
    let not_canonical = old.planted("topic/phone", &loose);
    let desk_topic = old.topic("desk", "A desk", &[], desk(MORNING));
    let misfiled = old.planted("topic/desk-machine", &desk_topic.to_text());
    let too_deep = old.planted("topic/desk/drawer", &desk_topic.to_text());
    old.planted("topic/desk/drawer", &loose);
    let fact = old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &[],
        desk(MORNING),
    );

    let store = read(old.root()).unwrap();
    assert_eq!(
        listed(&store),
        [
            ("fact".to_owned(), "lantern/relay-pin".to_owned()),
            ("topic".to_owned(), "desk".to_owned()),
        ]
    );
    assert_eq!(
        held(&store, Kind::Fact, "lantern/relay-pin").versions[0].id,
        fact.id.as_str()
    );
    let paths: Vec<&str> = (store.problems.iter())
        .map(|problem| match problem {
            Problem::Unreadable { path, reason } if !reason.is_empty() => path.as_str(),
            other => panic!("{other:?} is not a file that does not read"),
        })
        .collect();
    let mut expected = vec![
        format!("topic/lantern/{}.md", later.id),
        format!("topic/atlas/{}.md", atlas.id),
        not_canonical,
        misfiled,
        too_deep.rsplit_once('/').unwrap().0.to_owned(),
    ];
    expected.sort_unstable();
    assert_eq!(paths, expected);
}

#[test]
fn the_usage_log_is_under_the_name_the_converted_store_keeps_its_own_under() {
    assert_eq!(USAGE, worklog::wiring::USAGE_DIRECTORY);
}

#[test]
fn what_no_reader_of_the_store_looks_at_is_collected_but_for_dot_names_and_the_usage_log() {
    let old = OldStore::new();
    let lantern = old.topic("lantern", "A lamp", &[], desk(MORNING));
    old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &[],
        desk(MORNING),
    );
    old.stray_file("notes.txt", "a note\n");
    old.stray_file("topic/lantern/notes.md", "a note\n");
    old.stray_file("idea/lantern/notes.md", "a note\n");
    old.stray_directory("fact/lantern/empty");
    old.stray_directory("followup");
    old.stray_file("fact/atlas/scale/draft.md", "a draft\n");

    old.stray_file(".stignore", ".DS_Store\n");
    old.stray_directory(".stfolder");
    old.stray_file("usage/desk-2026-09.jsonl", "{}\n");
    old.stray_file("topic/lantern/.DS_Store", "");
    old.stray_file(
        &format!("topic/lantern/.tmp-{}", lantern.id),
        "half a version",
    );
    old.stray_file("topic/.trash/atlas/notes.md", "a note\n");

    let store = read(old.root()).unwrap();
    let skipped = |path: &str| Problem::Skipped {
        path: path.to_owned(),
    };
    assert_eq!(
        store.problems,
        [
            skipped("fact/atlas/scale"),
            skipped("fact/atlas/scale/draft.md"),
            skipped("fact/lantern/empty"),
            skipped("idea"),
            skipped("notes.txt"),
            skipped("topic/lantern/notes.md"),
        ]
    );
    assert_eq!(listed(&store), old.slugs());
    assert_eq!(store.documents.len(), 2);
}

fn everything() -> OldStore {
    let old = OldStore::new();
    let lantern = old.topic("lantern", "A lamp", &["atlas"], desk(MORNING));
    old.topic("atlas", "A book of maps", &[], desk(MORNING));
    let claims: [(&str, &[&str]); 1] = [("lantern", &["~/projects/lantern"])];
    old.machine_topic("desk-machine", &claims, &["atlas"], desk(MORNING));
    old.machine_topic("phone-machine", &[], &[], phone(MORNING));

    let pin = old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &["lantern"],
        desk(MORNING),
    );
    let pin = old.saved(
        &pin,
        "The relay pin is soldered",
        "\nSoldered.\n",
        phone(NOON),
    );
    old.saved(&pin, "The relay pin is glued", "\nGlued.\n", desk(EVENING));
    let idea = old.idea(
        "atlas/needs-an-index",
        "The atlas needs an index",
        &[],
        desk(MORNING),
    );
    old.removed(&idea, "Built.", desk(NOON));

    let entry = old.entry(
        "2026-09/2026-09-04-lamp-driver",
        "Wrote the lamp driver",
        &["lantern"],
        desk(MORNING),
    );
    let open = old.followup(
        "2026-09-04-port-the-driver",
        &entry,
        "Port the driver",
        &["lantern"],
        desk(MORNING),
    );
    old.done(&open, "Ported.", phone(EVENING));

    let scale = old.fact("atlas/scale", "The scale is metric", &[], desk(MORNING));
    old.renamed(&scale, "atlas/metric-scale", desk(NOON));
    let legend = old.fact(
        "atlas/legend",
        "The legend is on the last page",
        &[],
        desk(MORNING),
    );
    old.rename_cut_short(&legend, "atlas/key", desk(NOON));

    old.forked(&lantern, desk(NOON), phone(NOON));
    let wick = old.fact("lantern/wick", "The wick is cotton", &[], desk(MORNING));
    let trimmed = old.saved(
        &wick,
        "The wick is trimmed cotton",
        "\nTrimmed.\n",
        desk(NOON),
    );
    old.saved(
        &trimmed,
        "The wick is waxed cotton",
        "\nWaxed.\n",
        desk(EVENING),
    );
    old.withhold(&trimmed);
    let glass = old.fact("lantern/glass", "The glass is thick", &[], desk(MORNING));
    old.damage(&glass, b"not a version\n");
    old.stray_file("notes.txt", "a note\n");
    old
}

#[test]
fn documents_come_by_kind_then_slug_as_the_store_s_own_reader_lists_them() {
    let old = everything();
    let store = read(old.root()).unwrap();
    let mut expected = old.slugs();
    expected.retain(|(_, slug)| slug != "lantern/glass");
    assert_eq!(listed(&store), expected);
    let kinds: Vec<Kind> = store
        .documents
        .iter()
        .map(|document| document.kind)
        .collect();
    assert!(kinds.is_sorted());
    assert_eq!(
        (kinds.first(), kinds.last()),
        (Some(&Kind::Entry), Some(&Kind::Followup))
    );
    assert_eq!(store.problems.len(), 4);
}

#[test]
fn reading_a_store_twice_gives_the_same_result() {
    let old = everything();
    let first = read(old.root()).unwrap();
    assert_eq!(read(old.root()).unwrap(), first);
    assert!(!first.documents.is_empty());
    assert!(!first.problems.is_empty());
}

#[test]
fn a_directory_that_is_no_store_is_a_failure() {
    let old = OldStore::new();
    let absent = old.beside("nowhere");
    assert!(matches!(read(&absent), Err(Failure::Io { .. })));

    let empty = old.beside("empty");
    std::fs::create_dir_all(empty.join(".stfolder")).unwrap();
    std::fs::write(empty.join("notes.txt"), "a note\n").unwrap();
    assert_eq!(
        read(&empty),
        Err(Failure::NotAStore {
            path: empty.display().to_string()
        })
    );

    let file = old.beside("file");
    std::fs::write(&file, "").unwrap();
    assert!(matches!(read(&file), Err(Failure::Io { .. })));
}

#[test]
fn every_file_of_a_document_that_does_not_read_is_collected_by_name() {
    let old = OldStore::new();
    let first = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let second = old.saved(&first, "A lamp on a relay", "\nTwo.\n", desk(NOON));
    let third = old.saved(&second, "A lamp on two relays", "\nThree.\n", desk(EVENING));
    let fourth = old.saved(
        &third,
        "A lamp on three relays",
        "\nFour.\n",
        desk(NEXT_DAY),
    );
    for version in [&second, &third, &fourth] {
        old.damage(version, b"not a version\n");
    }

    let store = read(old.root()).unwrap();
    let paths: Vec<&str> = (store.problems.iter())
        .map(|problem| match problem {
            Problem::Unreadable { path, .. } => path.as_str(),
            other => panic!("{other:?} is not a file that does not read"),
        })
        .collect();
    let mut expected: Vec<String> = [&second, &third, &fourth]
        .iter()
        .map(|version| format!("topic/lantern/{}.md", version.id))
        .collect();
    expected.sort_unstable();
    assert_eq!(paths, expected);
    assert_eq!(store.documents, []);
}

#[test]
fn a_path_collected_is_under_the_store_however_the_store_s_directory_is_spelled() {
    let old = OldStore::new();
    let lantern = old.topic("lantern", "A lamp", &[], desk(MORNING));
    old.damage(&lantern, b"not a version\n");
    let atlas = old.topic("atlas", "A book of maps", &[], desk(MORNING));
    old.planted("topic/atlas/index", &atlas.to_text());
    old.stray_file("notes.txt", "a note\n");

    let plain = read(old.root()).unwrap();
    assert_eq!(plain.problems.len(), 3);
    let root = old.root().display().to_string();
    let name = old.root().file_name().unwrap().display().to_string();
    for spelled in [
        format!("{root}/"),
        format!("{root}/."),
        format!("{root}//"),
        format!("{root}/../{name}/"),
    ] {
        let store = read(std::path::Path::new(&spelled)).unwrap();
        assert_eq!(store, plain, "{spelled}");
    }
    for problem in &plain.problems {
        let (Problem::Unreadable { path, .. } | Problem::Skipped { path }) = problem else {
            panic!("{problem:?} carries no path");
        };
        assert!(!path.starts_with('/'), "{path}");
    }
}

#[test]
fn a_kind_s_directory_that_is_a_link_is_followed() {
    let old = OldStore::new();
    old.topic("lantern", "A lamp", &[], desk(MORNING));
    old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &[],
        desk(MORNING),
    );
    old.stray_file("usage/desk-2026-09.jsonl", "{}\n");
    old.link_out("fact");
    old.link_out("usage");

    let store = read(old.root()).unwrap();
    assert_eq!(listed(&store), old.slugs());
    assert_eq!(store.documents.len(), 2);
    assert_eq!(store.problems, []);
}

fn keys(version: &Version) -> Vec<&str> {
    version.fields.iter().map(|(key, _)| key.as_str()).collect()
}

#[test]
fn a_version_the_fixture_makes_holds_its_keys_in_the_writer_s_order() {
    let old = OldStore::new();
    let entry = old.entry(
        "2026-09/2026-09-04-lamp-driver",
        "Wrote the lamp driver",
        &[],
        desk(MORNING),
    );
    let open = old.followup(
        "2026-09-04-port-the-driver",
        &entry,
        "Port the driver",
        &[],
        desk(MORNING),
    );
    let recheck = [("recheck", scalar("2026-10-01 the beta is out"))];
    old.write(&following(&open, desk(NOON), "recheck", &recheck, None));
    let fact = old.fact(
        "lantern/relay-pin",
        "The relay pin is fixed",
        &[],
        desk(MORNING),
    );
    let verified = [("verified", scalar("2026-09-05"))];
    let checked = following(&fact, desk(NEXT_DAY), "verify", &verified, None);
    old.write(&checked);
    let recheck = [("recheck", scalar("touching lantern"))];
    old.write(&following(
        &checked,
        desk(NEXT_DAY),
        "recheck",
        &recheck,
        None,
    ));

    let store = read(old.root()).unwrap();
    let followup = held(&store, Kind::Followup, "2026-09-04-port-the-driver");
    assert_eq!(
        keys(&followup.versions[1]),
        ["entry", "tags", "recheck", "state", "summary"]
    );
    let fact = held(&store, Kind::Fact, "lantern/relay-pin");
    assert_eq!(keys(&fact.versions[1]), ["tags", "verified", "summary"]);
    assert_eq!(
        keys(&fact.versions[2]),
        ["tags", "recheck", "verified", "summary"]
    );
    assert_eq!(store.problems, []);
}

#[test]
fn the_order_files_are_written_in_changes_nothing_read() {
    let chain = |old: &OldStore| {
        let first = composed_topic(old);
        let mut chain = vec![first];
        for step in 0..7 {
            let body = format!("\nStep {step}.\n");
            let last = chain.last().unwrap();
            chain.push(following(last, phone(NOON), "save", &[], Some(&body)));
        }
        chain
    };
    let (forward, backward) = (OldStore::new(), OldStore::new());
    for version in &chain(&forward) {
        forward.write(version);
    }
    for version in chain(&backward).iter().rev() {
        backward.write(version);
    }
    let read_forward = read(forward.root()).unwrap();
    assert_eq!(read(backward.root()).unwrap(), read_forward);
    assert_eq!(read_forward.documents[0].versions.len(), 8);
}

fn composed_topic(old: &OldStore) -> worklog_old::domain::version::Version {
    old.topic("lantern", "A lamp", &[], desk(MORNING))
}

fn bare(id: &str, parents: &[&str]) -> Version {
    Version {
        id: id.to_owned(),
        parents: parents.iter().map(|parent| (*parent).to_owned()).collect(),
        written: NOON.to_owned(),
        machine: "desk".to_owned(),
        operation: "save".to_owned(),
        fields: Vec::new(),
        body: String::new(),
        superseded_by: None,
        renamed_from: None,
        tombstone: false,
        foreign_block: None,
    }
}

fn document(kind: Kind, slug: &str, state: State, versions: &[Version]) -> Document {
    Document {
        kind,
        slug: slug.to_owned(),
        state,
        versions: versions.to_vec(),
    }
}

fn found(documents: &[Document], unread: &[(Kind, &str)], problems: &[Problem]) -> Found {
    Found {
        documents: documents.to_vec(),
        unread: (unread.iter())
            .map(|(kind, slug)| (*kind, (*slug).to_owned()))
            .collect(),
        problems: problems.to_vec(),
    }
}

fn unreadable_paths(problems: &[Problem]) -> Vec<&str> {
    (problems.iter())
        .map(|problem| match problem {
            Problem::Unreadable { path, reason } if !reason.is_empty() => path.as_str(),
            other => panic!("{other:?} is not something that does not read"),
        })
        .collect()
}

#[test]
fn versions_come_in_one_order_whatever_order_they_are_given_in() {
    let versions = [
        bare("a", &["c"]),
        bare("b", &["c"]),
        bare("c", &[]),
        bare("d", &["b", "a"]),
        bare("e", &["elsewhere"]),
    ];
    for turn in 0..versions.len() {
        for backward in [false, true] {
            let mut given = versions.to_vec();
            given.rotate_left(turn);
            if backward {
                given.reverse();
            }
            let ordered = parent_first(given).unwrap();
            let ids: Vec<&str> = ordered.iter().map(|version| version.id.as_str()).collect();
            assert_eq!(ids, ["c", "a", "b", "d", "e"]);
        }
    }
}

#[test]
fn versions_that_name_each_other_as_parents_are_given_no_order() {
    let given = vec![
        bare("b", &["a"]),
        bare("c", &[]),
        bare("a", &["b"]),
        bare("d", &["a"]),
    ];
    assert_eq!(
        parent_first(given),
        Err(vec!["a".to_owned(), "b".to_owned(), "d".to_owned()])
    );
}

#[test]
fn a_document_whose_versions_name_each_other_is_collected_and_left_out() {
    let knot = document(
        Kind::Topic,
        "lantern",
        State::Live,
        &[bare("a", &["b"]), bare("b", &["a"])],
    );
    let atlas = document(Kind::Topic, "atlas", State::Live, &[bare("c", &[])]);
    let moved = Version {
        renamed_from: Some("lantern".to_owned()),
        ..bare("e", &["a"])
    };
    let lamp = document(Kind::Topic, "lamp", State::Live, &[moved]);
    let store = found(&[knot, atlas, lamp], &[], &[]).store();
    assert_eq!(
        listed(&store),
        [
            ("topic".to_owned(), "atlas".to_owned()),
            ("topic".to_owned(), "lamp".to_owned())
        ]
    );
    assert_eq!(unreadable_paths(&store.problems), ["topic/lantern"]);
}

#[test]
fn a_document_with_no_head_is_refused_with_the_reason() {
    assert!(document_of(Kind::Topic, "lantern", &StoredDocument::new(Vec::new())).is_err());
}

#[test]
fn a_version_moved_from_under_a_directory_that_does_not_read_is_not_also_waiting() {
    let moved = Version {
        renamed_from: Some("lantern/relay-pin".to_owned()),
        ..bare("b", &["a"])
    };
    let fact = document(Kind::Fact, "atlas/relay-pin", State::Live, &[moved]);
    let waiting = Problem::Waiting {
        kind: Kind::Fact,
        slug: "atlas/relay-pin".to_owned(),
        missing: vec!["a".to_owned()],
    };
    let waiting = [waiting];
    let read_with = |unread: &[(Kind, &str)]| {
        let documents = std::slice::from_ref(&fact);
        found(documents, unread, &[]).store().problems
    };
    assert_eq!(read_with(&[]), waiting);
    assert_eq!(read_with(&[(Kind::Fact, "lantern")]), []);
    assert_eq!(read_with(&[(Kind::Fact, "")]), []);
    assert_eq!(read_with(&[(Kind::Fact, "lantern/relay-pin")]), []);
    assert_eq!(read_with(&[(Kind::Fact, "lantern/relay")]), waiting);
    assert_eq!(read_with(&[(Kind::Topic, "lantern")]), waiting);
}

#[test]
fn what_was_found_gives_one_store_whatever_order_it_was_found_in() {
    let lantern = [bare("a", &[]), bare("b", &["a"]), bare("c", &["a"])];
    let documents = [
        document(Kind::Topic, "lantern", State::Forked, &lantern),
        document(
            Kind::Fact,
            "lantern/wick",
            State::Live,
            &[bare("e", &["d"])],
        ),
        document(
            Kind::Entry,
            "2026-09/2026-09-04-lamp-driver",
            State::Live,
            &[bare("f", &[])],
        ),
        document(Kind::Topic, "atlas", State::Live, &[bare("g", &[])]),
    ];
    let unreadable = |path: &str| Problem::Unreadable {
        path: path.to_owned(),
        reason: "does not read".to_owned(),
    };
    let problems = [
        unreadable("topic/phone/one.md"),
        Problem::Skipped {
            path: "notes.txt".to_owned(),
        },
        unreadable("topic/desk/two.md"),
        unreadable("topic/phone/one.md"),
    ];
    let forward = found(&documents, &[], &problems).store();

    let mut turned = documents.to_vec();
    turned.reverse();
    for document in &mut turned {
        document.versions.reverse();
    }
    let mut problems = problems.to_vec();
    problems.reverse();
    assert_eq!(found(&turned, &[], &problems).store(), forward);
    assert_eq!(forward.documents.len(), 4);
    assert_eq!(forward.problems.len(), 5);
}

#[test]
fn a_document_the_walk_and_the_store_s_reader_do_not_both_list_is_collected() {
    let set = |slugs: &[&str]| -> BTreeSet<String> {
        slugs.iter().map(|slug| (*slug).to_owned()).collect()
    };
    let walked = set(&["atlas", "lantern"]);
    let problems = unmatched(Kind::Topic, &walked, &set(&["lantern", "phone"]));
    assert_eq!(unreadable_paths(&problems), ["topic/atlas", "topic/phone"]);
    let reasons: BTreeSet<&Problem> = problems.iter().collect();
    assert_eq!(reasons.len(), 2);
    assert_eq!(unmatched(Kind::Topic, &walked, &walked), []);
}

#[test]
fn a_failure_says_what_it_concerns() {
    let failure: Box<dyn std::error::Error> = Box::new(Failure::NotAStore {
        path: "desk".to_owned(),
    });
    assert_eq!(
        failure.to_string(),
        "desk: holds no store: none of entry, fact, topic and followup is a directory in it"
    );
}

#[test]
fn a_document_the_store_s_reader_refuses_is_collected_though_each_file_reads() {
    let old = OldStore::new();
    let lantern = old.topic("lantern", "A lamp", &[], desk(MORNING));
    old.topic("atlas", "A book of maps", &[], desk(MORNING));
    let named_as_a_version = format!("topic/lantern/{}.md", "0".repeat(64));
    old.stray_directory(&named_as_a_version);
    assert!(old.file_of(&lantern).is_file());

    let store = read(old.root()).unwrap();
    assert_eq!(listed(&store), [("topic".to_owned(), "atlas".to_owned())]);
    let refused: Vec<&Problem> = (store.problems.iter())
        .filter(|problem| matches!(problem, Problem::Unreadable { .. }))
        .collect();
    assert_eq!(
        unreadable_paths(&store.problems[..refused.len()]),
        ["topic/lantern"]
    );
    assert_eq!(refused.len(), 1);
}

#[test]
fn the_keys_a_version_block_is_foreign_for_are_those_its_writer_never_writes() {
    let old = OldStore::new();
    let topic = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let (left, moved) = old.renamed(&topic, "lamp", desk(NOON));
    let signed = |version: &Stored, from: &str, to: &str| {
        (version.to_text())
            .replace(&format!("slug: {from}\n"), &format!("slug: {to}\n"))
            .replace(
                "  operation: rename\n",
                "  operation: rename\n  signed: desk-key\n",
            )
    };
    old.planted("topic/beacon", &signed(&left, "lantern", "beacon"));
    old.planted("topic/atlas", &signed(&moved, "lamp", "atlas"));

    let store = read(old.root()).unwrap();
    for slug in ["beacon", "atlas"] {
        let document = held(&store, Kind::Topic, slug);
        assert_eq!(document.foreign_keys(&document.versions[0]), ["signed"]);
    }
    let lantern = held(&store, Kind::Topic, "lantern");
    assert_eq!(lantern.foreign_keys(&lantern.versions[1]), [""; 0]);
}

#[test]
fn the_keys_of_a_version_block_are_held_to_what_the_stores_reader_calls_foreign() {
    let old = OldStore::new();
    let topic = old.topic("lantern", "A lamp", &[], desk(MORNING));
    let (left, moved) = old.renamed(&topic, "lamp", desk(NOON));
    let sealed = |version: &Stored, from: &str, to: &str, last: &str| {
        let text = version.to_text();
        assert!(text.contains(last), "{text}");
        text.replace(&format!("slug: {from}\n"), &format!("slug: {to}\n"))
            .replace(last, &format!("{last}  sealed: wax\n"))
    };
    let stone = sealed(&left, "lantern", "beacon", "  superseded_by: lamp\n");
    old.planted("topic/beacon", &stone);
    let moved = sealed(&moved, "lamp", "atlas", "  renamed_from: lantern\n");
    old.planted("topic/atlas", &moved);

    let reader = FsStore::new(old.root().to_path_buf());
    let mut foreign = 0;
    for slug in ["lantern", "lamp", "beacon", "atlas"] {
        let slug = Slug::of_kind(StoredKind::Topic, slug).unwrap();
        let stored = reader.document(&slug).unwrap();
        assert!(!stored.history().is_empty(), "{slug}");
        for version in stored.history() {
            let ours = version_of(version);
            let first = foreign_block_keys(&ours).first().copied();
            assert_eq!(version.foreign_key(), first, "{slug}");
            foreign += usize::from(first.is_some());
        }
    }
    assert_eq!(foreign, 2);
}

#[test]
fn a_slug_has_the_kind_the_stores_reader_takes_its_shape_for() {
    assert_eq!(super::shaped("2026-09/2026-09-04-lamp"), Some(Kind::Entry));
    assert_eq!(super::shaped("2026-09-04-lamp"), Some(Kind::Followup));
    assert_eq!(super::shaped("lantern/relay-pin"), Some(Kind::Fact));
    assert_eq!(super::shaped("lantern"), Some(Kind::Topic));
    for shapeless in ["2026", "2026-09-04", "a/b/c", "has space", ""] {
        assert_eq!(super::shaped(shapeless), None, "{shapeless}");
    }
}

#[test]
fn a_dated_slug_gives_its_date_and_the_name_after_it() {
    let parts = |date: &str, name: &str| Some((date.to_owned(), name.to_owned()));
    assert_eq!(
        super::dated(Kind::Entry, "2026-09/2026-09-04-lamp-driver"),
        parts("2026-09-04", "lamp-driver")
    );
    assert_eq!(
        super::dated(Kind::Followup, "2026-02-31-2026-09-04-port"),
        parts("2026-02-31", "2026-09-04-port")
    );
    assert_eq!(
        super::dated(Kind::Followup, "2026-09/2026-09-04-lamp"),
        None
    );
    assert_eq!(super::dated(Kind::Topic, "lantern"), None);
    assert_eq!(super::dated(Kind::Fact, "lantern/relay-pin"), None);
}

#[test]
fn a_kind_is_spelled_as_the_directory_the_store_keeps_it_in() {
    let stored = StoredKind::ALL.map(StoredKind::dir);
    assert_eq!(Kind::ALL.map(Kind::word), stored);
}
