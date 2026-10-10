//! The form `--json` gives a result: the use case's own type, one value and a newline.
//!
//! One convention holds for every value. A key is in snake case. An enum that carries data
//! is an object whose `type` key holds the variant in snake case, and one that carries none
//! is that word alone. A value that is absent is `null` under its key. An id is written in
//! full, a version's with its algorithm, a time as RFC 3339 and a date as `YYYY-MM-DD`.
//!
//! `Made`, what `new followup` gives, is the one enum with no `type` key: it is the stored
//! version or the opened draft itself, as the command that only stores or only opens prints
//! it.
//!
//! A document's id is under `document`, as a value's own id and as a reference alike, and a
//! version's id under `version` where the value is that version. `ended` is an ending's
//! reason, or `null`. A count is under a key ending in `_count`, and a plural noun is a list.

use serde::Serialize;

/// Implemented in a test build only beside a type's snapshot, so a result with no snapshot
/// does not compile.
pub(super) trait Pinned {}

#[cfg(not(test))]
impl<T> Pinned for T {}

pub(super) fn printed<T: Serialize>(result: &T) -> String {
    let mut text = serde_json::to_string_pretty(result).expect("a result with text for keys");
    text.push('\n');
    text
}

pub(super) fn nothing() -> String {
    printed(&serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::render::{Render, output};
    use super::*;
    use crate::app::check::Check;
    use crate::app::context::Context;
    use crate::app::draft::DraftRow;
    use crate::app::followup::{Made, NewFollowup};
    use crate::app::list::{ClaimRow, EntryRow, FactRow, ForkRow, TopicRow};
    use crate::app::search::{Hit, Logged, Query};
    use crate::app::setup::Bound;
    use crate::app::show::{Diff, History, Shown};
    use crate::app::testing::{
        ENDED as RETIRED, ENDED_FACT, World, claim_fields, entry, fact, followup, fork, head,
        part_of, topic, topic_fields, uses, world, world_with_atlas, written_at,
    };
    use crate::app::{
        DraftRef, FollowupRow, Written, amend, bulk, check, context, draft, followup as followups,
        list, search, setup, show,
    };
    use crate::domain::id::{DocumentId, VersionId};
    use crate::domain::schema::{Date, Directory, KindOf, Name};
    use crate::domain::testing::{LANTERN, atlas, first};
    use crate::domain::version::{Kind, ReadError, Stamp};

    const NOW: &str = "2026-10-09T18:22:41.118204+01:00";

    fn snake(word: &str) -> bool {
        word.starts_with(|first: char| first.is_ascii_lowercase())
            && word
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    fn conventional(value: &Value) -> Result<(), String> {
        match value {
            Value::Object(keyed) => keyed.iter().try_for_each(|(key, inner)| {
                if !snake(key) {
                    return Err(format!("the key `{key}`"));
                }
                if key == "type" && !inner.as_str().is_some_and(snake) {
                    return Err(format!("the type {inner}"));
                }
                if key == "id" {
                    return Err("the key `id`, which says no kind of id".to_owned());
                }
                if key == "ended" && !(inner.is_string() || inner.is_null()) {
                    return Err(format!("the ending {inner}"));
                }
                conventional(inner)
            }),
            Value::Array(items) => items.iter().try_for_each(conventional),
            _ => Ok(()),
        }
    }

    fn parsed<T: Render + Serialize + Pinned>(result: &T) -> Value {
        let made = output(result, true);
        assert_eq!((made.notes.len(), made.exit), (0, result.exit()));
        let value = made.text.strip_suffix('\n').expect("a newline ends it");
        assert_eq!(value.trim(), value);
        let value: Value = serde_json::from_str(value).expect("one value");
        assert_eq!(conventional(&value), Ok(()), "{value:#}");
        value
    }

    macro_rules! pinned {
        ($($test:ident: $result:ty = $fixture:ident,)*) => {$(
            impl Pinned for $result {}

            #[test]
            fn $test() {
                let pinned: Vec<($result, Value)> = $fixture();
                assert!(!pinned.is_empty());
                for (result, expected) in &pinned {
                    assert_eq!(format!("{:#}", parsed(result)), format!("{expected:#}"));
                }
            }
        )*};
    }

    pinned! {
        a_stored_version_is_its_document_its_label_and_its_whole_hash: Written = stored,
        the_versions_a_move_stores_are_listed: Vec<Written> = moved,
        a_draft_opened_is_its_document_and_its_path: DraftRef = opened,
        a_listed_draft_holds_its_kind_its_label_and_its_path: Vec<DraftRow> = drafts,
        a_draft_discarded_is_null: () = discarded,
        a_followup_made_is_the_version_stored_or_the_draft_opened: Made = made,
        a_bound_host_is_its_machine_topic_and_whether_it_was_made: Bound = bound,
        a_topic_row_holds_its_row_and_the_labels_of_its_edges: Vec<TopicRow> = topics,
        a_fact_row_holds_its_date_and_an_ended_one_its_reason: Vec<FactRow> = facts,
        an_entry_row_holds_its_date: Vec<EntryRow> = entries,
        a_followup_row_holds_its_trigger_by_type_and_null_for_what_it_lacks: Vec<FollowupRow> =
            followup_rows,
        a_claim_row_holds_null_for_a_claim_on_no_directory: Vec<ClaimRow> = claims,
        a_fork_row_holds_its_heads_and_null_for_a_row_no_head_reads_as: Vec<ForkRow> = forks,
        a_hit_holds_its_row_and_the_lines_that_matched: Vec<Hit> = hits,
        a_logged_version_holds_null_for_a_row_its_document_does_not_read_as: Vec<Logged> = logged,
        a_shown_document_and_a_shown_version_are_told_apart_by_type: Shown = shown,
        a_history_holds_its_versions_and_why_each_unreadable_one_is: History = history,
        a_diff_holds_the_version_each_side_is_against_and_the_text_after: Diff = diffs,
        a_context_says_by_type_what_loaded_each_topic: Context = contexts,
        a_check_holds_its_findings_and_its_count_of_forks: Check = checks,
    }

    fn row(id: &DocumentId, kind: &str, label: &str, topics: &[&str]) -> Value {
        json!({
            "document": id,
            "kind": kind,
            "label": label,
            "summary": "s",
            "topics": topics,
            "forked": false,
            "ended": null,
        })
    }

    fn with(mut object: Value, key: &str, value: Value) -> Value {
        *object.get_mut(key).expect("a key the object has") = value;
        object
    }

    fn by_version(mut versions: Vec<Value>) -> Value {
        versions.sort_by_key(|version| version["version"].as_str().map(str::to_owned));
        Value::Array(versions)
    }

    fn version(world: &World, id: &DocumentId, label: &str) -> Value {
        let head = head(world, id);
        json!({
            "document": id,
            "label": label,
            "version": head.id,
            "written": NOW,
            "machine": "desk",
            "change": "new",
            "parents": [],
            "head": true,
            "ended": null,
            "text": "+++\nsize = 4\n+++\n\n",
        })
    }

    struct Damaged {
        world: World,
        lantern: DocumentId,
        gadget: DocumentId,
        unreadable: Value,
    }

    fn damaged() -> Damaged {
        let (world, lantern) = world();
        let gadget = world.put("gadget", "size = 4\n", "\n");
        let whys = [
            (
                VersionId::of(b"corrupt"),
                ReadError::Corrupt,
                json!({"type": "corrupt", "text": "the bytes do not hash to the file's name"}),
            ),
            (
                VersionId::of(b"newer"),
                ReadError::Newer { format: 2 },
                json!({"type": "newer", "format": 2, "text": "written in format 2, a newer one"}),
            ),
            (
                VersionId::of(b"malformed"),
                ReadError::Malformed("no envelope".to_owned()),
                json!({"type": "malformed", "text": "no envelope"}),
            ),
        ];
        let mut unreadable = Vec::new();
        for (id, why, shown) in whys {
            unreadable.push(json!({"version": id, "why": shown}));
            world.store.plant_unreadable(&gadget, id, why);
        }
        Damaged {
            world,
            lantern,
            gadget,
            unreadable: by_version(unreadable),
        }
    }

    #[test]
    fn a_key_or_a_type_in_another_case_breaks_the_convention() {
        for word in ["look_again", "part_of", "b3", "id"] {
            assert!(snake(word), "{word}");
        }
        for word in ["lookAgain", "LookAgain", "look-again", "_id", "3b", ""] {
            assert!(!snake(word), "{word}");
        }
        assert_eq!(
            conventional(&json!([{"row": {"partOf": []}}])),
            Err("the key `partOf`".to_owned())
        );
        assert_eq!(
            conventional(&json!({"via": {"type": "LookAgain"}})),
            Err("the type \"LookAgain\"".to_owned())
        );
        assert_eq!(
            conventional(&json!({"via": {"type": {"claim": null}}})),
            Err("the type {\"claim\":null}".to_owned())
        );
        assert_eq!(
            conventional(&json!({"row": {"id": "7f3a91c0"}})),
            Err("the key `id`, which says no kind of id".to_owned())
        );
        assert_eq!(
            conventional(&json!([{"ended": true}])),
            Err("the ending true".to_owned())
        );
        assert_eq!(
            conventional(&json!({"type": "look_again", "on": null, "ended": null})),
            Ok(())
        );
        assert_eq!(conventional(&json!({"ended": "retired"})), Ok(()));
    }

    #[test]
    fn an_id_a_time_a_date_a_name_a_kind_and_a_directory_are_the_text_a_person_reads() {
        let value = |text: String| serde_json::from_str::<Value>(&text).expect("one value");
        let lantern = DocumentId::parse(LANTERN).unwrap();
        assert_eq!(value(printed(&lantern)), json!(LANTERN));
        let hashed = VersionId::of(b"lantern");
        assert_eq!(value(printed(&hashed)), json!(hashed.as_str()));
        assert_eq!(hashed.as_str().len(), "b3-".len() + 64);
        assert_eq!(value(printed(&Stamp::parse(NOW).unwrap())), json!(NOW));
        let day = Date::parse("2026-10-08").unwrap();
        assert_eq!(value(printed(&day)), json!("2026-10-08"));
        let name = Name::parse("lantern").unwrap();
        assert_eq!(value(printed(&name)), json!("lantern"));
        let kind = Kind::parse("gadget").unwrap();
        assert_eq!(value(printed(&kind)), json!("gadget"));
        assert_eq!(
            value(printed(&KindOf::ALL)),
            json!(["topic", "fact", "entry", "followup", "claim"])
        );
        let directory = Directory::on_host("/home/desk/projects/lantern", Some("/home/desk"));
        assert_eq!(
            value(printed(&directory.unwrap())),
            json!("~/projects/lantern")
        );
    }

    fn stored() -> Vec<(Written, Value)> {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let verified = amend::verify(&world.deps(), "lantern/relay").unwrap();
        let version = head(&world, &relay).id;
        assert!(version.as_str().starts_with("b3-"));
        let expected = json!({"document": relay, "label": "lantern/relay", "version": version});
        vec![(verified, expected)]
    }

    fn moved() -> Vec<(Vec<Written>, Value)> {
        let (world, lantern, _) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay", "");
        let moved = bulk::move_to(&world.deps(), &["lantern/relay"], "lantern", "atlas").unwrap();
        let version = head(&world, &relay).id;
        let expected = json!([{"document": relay, "label": "atlas/relay", "version": version}]);
        vec![(moved, expected)]
    }

    fn opened() -> Vec<(DraftRef, Value)> {
        let (world, lantern) = world();
        let opened = draft::checkout(&world.deps(), "lantern").unwrap();
        let expected = json!({"document": lantern, "path": format!("memory:{lantern}")});
        vec![(opened, expected)]
    }

    fn drafts() -> Vec<(Vec<DraftRow>, Value)> {
        let (world, lantern) = world();
        let deps = world.deps();
        let none = draft::drafts(&deps).unwrap();
        draft::checkout(&deps, "lantern").unwrap();
        let expected = json!([{
            "document": lantern,
            "kind": "topic",
            "label": "lantern",
            "path": format!("memory:{lantern}"),
        }]);
        vec![(draft::drafts(&deps).unwrap(), expected), (none, json!([]))]
    }

    fn discarded() -> Vec<((), Value)> {
        let (world, _) = world();
        let deps = world.deps();
        draft::checkout(&deps, "lantern").unwrap();
        vec![(draft::discard(&deps, "lantern").unwrap(), Value::Null)]
    }

    fn made() -> Vec<(Made, Value)> {
        let (world, _) = world();
        let deps = world.deps();
        let mut what = NewFollowup {
            topics: &["lantern"],
            summary: Some("Order a fuse"),
            entry: None,
            about: None,
            trigger: None,
        };
        let stored = followups::new_followup(&deps, &what).unwrap();
        let Made::Written(written) = &stored else {
            panic!("a followup with a summary is stored");
        };
        let stored_as = json!({
            "document": written.document,
            "label": written.document.short(),
            "version": head(&world, &written.document).id,
        });

        what.summary = None;
        let opened = followups::new_followup(&deps, &what).unwrap();
        let Made::Draft(drafted) = &opened else {
            panic!("a followup with no summary is drafted");
        };
        let opened_as = json!({
            "document": drafted.document,
            "path": format!("memory:{}", drafted.document),
        });
        vec![(stored, stored_as), (opened, opened_as)]
    }

    fn bound() -> Vec<(Bound, Value)> {
        let mut world = World::new();
        world.host.set_machine(None);
        let bound = setup::init(&world.deps(), "desk", Some("A machine")).unwrap();
        let document = world.host.bound().expect("a host that init bound");
        let expected = json!({"document": document, "label": "desk", "created": true});
        vec![(bound, expected)]
    }

    fn topics() -> Vec<(Vec<TopicRow>, Value)> {
        let (world, lantern, atlas) = world_with_atlas();
        let edges = format!("{}{}", part_of(&[&lantern]), uses(&[&atlas]));
        let phone = topic(&world, "phone", &edges);
        let bare = |id: &DocumentId, label: &str| json!({"row": row(id, "topic", label, &[]), "part_of": [], "uses": []});
        let expected = json!([
            bare(&atlas, "atlas"),
            bare(&world.machine(), "desk"),
            bare(&lantern, "lantern"),
            {
                "row": row(&phone, "topic", "phone", &[]),
                "part_of": ["lantern"],
                "uses": ["atlas"],
            },
        ]);
        vec![(list::topics(&world.deps(), false).unwrap(), expected)]
    }

    fn facts() -> Vec<(Vec<FactRow>, Value)> {
        let (world, lantern) = world();
        let deps = world.deps();
        let fuse = fact(&world, &lantern, "fuse", ENDED_FACT);
        let relay = fact(&world, &lantern, "relay", "");
        let dimmer = fact(&world, &lantern, "dimmer", "idea = true\n");
        let filed = |id: &DocumentId, label: &str| json!({"row": row(id, "fact", label, &["lantern"]), "confirmed": "2026-09-04"});
        let ended = with(
            row(&fuse, "fact", "lantern/fuse", &["lantern"]),
            "ended",
            json!("false"),
        );
        vec![
            (
                list::facts(&deps, None, true).unwrap(),
                json!([
                    {"row": ended, "confirmed": "2026-09-04"},
                    filed(&relay, "lantern/relay"),
                ]),
            ),
            (
                list::ideas(&deps, None, false).unwrap(),
                json!([filed(&dimmer, "lantern/dimmer")]),
            ),
        ]
    }

    fn entries() -> Vec<(Vec<EntryRow>, Value)> {
        let (world, lantern) = world();
        let wiring = entry(&world, "2026-10-08", "wiring", &[&lantern]);
        let expected = json!([{
            "row": row(&wiring, "entry", "2026-10-08-wiring", &["lantern"]),
            "date": "2026-10-08",
        }]);
        let listed = list::entries(&world.deps(), None, None, false).unwrap();
        vec![(listed, expected)]
    }

    fn followup_rows() -> Vec<(Vec<FollowupRow>, Value)> {
        let (world, lantern, atlas) = world_with_atlas();
        let wiring = entry(&world, "2026-10-08", "wiring", &[&lantern]);
        let dated = followup(
            &world,
            &[&lantern],
            "s",
            &format!(
                "look_again = 2026-10-01\nwhy = \"The part arrives\"\nentry = \"{wiring}\"\n\
                 about = \"{atlas}\"\n"
            ),
        );
        let touching = format!("touching = \"{lantern}\"\n");
        let waiting = followup(&world, &[&lantern], "s", &touching);
        let bare = followup(&world, &[&atlas], "s", "");
        let labeled = |id: &DocumentId, topic: &str| row(id, "followup", id.short(), &[topic]);
        let expected = json!([
            {
                "row": labeled(&dated, "lantern"),
                "trigger": {
                    "type": "look_again",
                    "on": "2026-10-01",
                    "why": "The part arrives",
                },
                "due": true,
                "entry": "2026-10-08-wiring",
                "about": "atlas",
            },
            {
                "row": labeled(&waiting, "lantern"),
                "trigger": {"type": "touching", "topic": "lantern"},
                "due": false,
                "entry": null,
                "about": null,
            },
            {
                "row": labeled(&bare, "atlas"),
                "trigger": null,
                "due": false,
                "entry": null,
                "about": null,
            },
        ]);
        let listed = list::followups(&world.deps(), None, false).unwrap();
        vec![(listed, expected)]
    }

    fn claims() -> Vec<(Vec<ClaimRow>, Value)> {
        let (world, lantern, atlas) = world_with_atlas();
        let machine = world.machine();
        let placed = claim_fields(&machine, &lantern, "~/projects/lantern");
        let placed = world.put("claim", &placed, "\n");
        let anywhere = world.put("claim", &claim_fields(&machine, &atlas, ""), "\n");
        let expected = json!([
            {
                "document": anywhere,
                "machine": "desk",
                "topic": "atlas",
                "directory": null,
                "forked": false,
            },
            {
                "document": placed,
                "machine": "desk",
                "topic": "lantern",
                "directory": "~/projects/lantern",
                "forked": false,
            },
        ]);
        let listed = list::where_(&world.deps(), None, None).unwrap();
        vec![(listed, expected)]
    }

    fn forks() -> Vec<(Vec<ForkRow>, Value)> {
        let (world, lantern, atlas) = world_with_atlas();
        let root = head(&world, &lantern);
        let fields = topic_fields("lantern", "");
        let (early, late) = ("2026-10-09T10:00:00+01:00", "2026-10-09T11:00:00+01:00");
        let live = written_at(&world, &root, &fields, early, "left\n", None);
        let retired = format!("{fields}{RETIRED}");
        let retired = written_at(&world, &root, &retired, late, "right\n", None);
        let read = by_version(vec![
            json!({"version": live.id, "written": early, "machine": "desk", "ended": null}),
            json!({"version": retired.id, "written": late, "machine": "desk", "ended": "retired"}),
        ]);
        let unread = fork(&world, &atlas, "name = 4\n");
        let unread = unread.iter().map(|id| {
            json!({
                "version": id,
                "written": "2026-09-04T10:00:00+01:00",
                "machine": "dededede",
                "ended": null,
            })
        });
        let forked = with(
            row(&lantern, "topic", "lantern", &[]),
            "forked",
            json!(true),
        );
        let expected = json!([
            {
                "document": atlas,
                "label": atlas.short(),
                "row": null,
                "heads": unread.collect::<Vec<Value>>(),
            },
            {"document": lantern, "label": "lantern", "row": forked, "heads": read},
        ]);
        vec![(list::forks(&world.deps()).unwrap(), expected)]
    }

    fn hits() -> Vec<(Vec<Hit>, Value)> {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        let query = Query {
            term: "the RELAY",
            regex: false,
            topic: None,
            ended: false,
        };
        let expected = json!([{
            "row": row(&relay, "fact", "lantern/relay", &["lantern"]),
            "lines": ["The relay."],
        }]);
        vec![(search::search(&world.deps(), &query).unwrap(), expected)]
    }

    fn logged() -> Vec<(Vec<Logged>, Value)> {
        let (world, lantern) = world();
        let gadget = world.put("gadget", "size = 4\n", "\n");
        let logged = |id: &DocumentId, version: &VersionId, row: Value| {
            let label = row["label"].as_str().unwrap_or(id.short());
            json!({
                "document": id,
                "label": label,
                "version": version,
                "written": NOW,
                "machine": "desk",
                "change": "new",
                "ended": null,
                "row": row,
            })
        };
        let first = |id: &DocumentId, row: Value| logged(id, &head(&world, id).id, row);
        let root = head(&world, &lantern).id;
        let retired = world.amend(&lantern, &topic_fields("lantern", RETIRED), "\n");
        let ended = with(
            row(&lantern, "topic", "lantern", &[]),
            "ended",
            json!("retired"),
        );
        let ending = with(
            with(
                logged(&lantern, &retired, ended.clone()),
                "change",
                json!("save"),
            ),
            "ended",
            json!("retired"),
        );
        let expected = by_version(vec![
            first(
                &world.machine(),
                row(&world.machine(), "topic", "desk", &[]),
            ),
            logged(&lantern, &root, ended),
            ending,
            first(&gadget, Value::Null),
        ]);
        vec![(search::log(&world.deps(), 4, None).unwrap(), expected)]
    }

    fn shown() -> Vec<(Shown, Value)> {
        let Damaged {
            world,
            lantern,
            gadget,
            unreadable,
        } = damaged();
        let deps = world.deps();
        let label = gadget.short();
        let unknown = json!({
            "type": "document",
            "document": gadget,
            "kind": "gadget",
            "label": label,
            "former": [],
            "forked": false,
            "heads": [version(&world, &gadget, label)],
            "unreadable": unreadable,
        });
        let of_unknown = show::show(&deps, label).unwrap();

        let hash = head(&world, &gadget).id;
        let mut one = json!({"type": "version"});
        let rest = version(&world, &gadget, label);
        let fields = rest.as_object().expect("an object").clone();
        one.as_object_mut().expect("an object").extend(fields);
        let of_version = show::show(&deps, hash.short()).unwrap();

        let root = head(&world, &lantern).id;
        let retired = world.amend(&lantern, &topic_fields("lantern", RETIRED), "\n");
        let ended = json!({
            "type": "document",
            "document": lantern,
            "kind": "topic",
            "label": "lantern",
            "former": [],
            "forked": false,
            "heads": [{
                "document": lantern,
                "label": "lantern",
                "version": retired,
                "written": NOW,
                "machine": "desk",
                "change": "save",
                "parents": [root],
                "head": true,
                "ended": "retired",
                "text": "+++\nname = \"lantern\"\ncreated = 2026-09-04\nsummary = \"s\"\n\
                             ended = \"retired\"\nended_on = 2026-10-09\nnote = \"n\"\n+++\n\n",
            }],
            "unreadable": [],
        });
        let of_ended = show::show(&deps, "lantern").unwrap();

        let lost = VersionId::of(b"lost");
        world
            .store
            .plant_unreadable(&atlas(), lost.clone(), ReadError::Corrupt);
        let unread = json!({
            "type": "document",
            "document": atlas(),
            "kind": null,
            "label": atlas().short(),
            "former": [],
            "forked": false,
            "heads": [],
            "unreadable": [{
                "version": lost,
                "why": {"type": "corrupt", "text": "the bytes do not hash to the file's name"},
            }],
        });
        let of_unread = show::show(&deps, atlas().short()).unwrap();
        vec![
            (of_unknown, unknown),
            (of_version, one),
            (of_ended, ended),
            (of_unread, unread),
        ]
    }

    fn history() -> Vec<(History, Value)> {
        let Damaged {
            world,
            gadget,
            unreadable,
            ..
        } = damaged();
        let label = gadget.short();
        let expected = json!({
            "document": gadget,
            "label": label,
            "versions": [version(&world, &gadget, label)],
            "unreadable": unreadable,
        });
        vec![(show::history(&world.deps(), label).unwrap(), expected)]
    }

    fn diffs() -> Vec<(Diff, Value)> {
        let (world, lantern) = world();
        let deps = world.deps();
        let root = head(&world, &lantern).id;
        draft::checkout(&deps, "lantern").unwrap();
        let text = "+++\nname = \"lantern\"\nsummary = \"s\"\n+++\n\n";
        let drafted = json!({
            "label": "lantern",
            "sides": [{"against": root, "held": true, "before": text}],
            "after": text,
        });
        let of_draft = show::diff(&deps, "lantern", None).unwrap();

        let made = json!({
            "label": "lantern",
            "sides": [{"against": null, "held": true, "before": ""}],
            "after": "+++\nname = \"lantern\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n\n",
        });
        let of_first = show::diff(&deps, root.short(), None).unwrap();

        let fields = topic_fields("atlas", "");
        let lost = first(&atlas(), "topic", &fields, "\n");
        let kept = written_at(&world, &lost, &fields, NOW, "\n", None);
        let unheld = json!({
            "label": "atlas",
            "sides": [{"against": lost.id, "held": false, "before": ""}],
            "after": "+++\nname = \"atlas\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n\n",
        });
        let of_unheld = show::diff(&deps, kept.id.short(), None).unwrap();
        vec![(of_draft, drafted), (of_first, made), (of_unheld, unheld)]
    }

    fn contexts() -> Vec<(Context, Value)> {
        let (mut world, lantern, atlas) = world_with_atlas();
        world.host.set_home(Some("/home/desk".to_owned()));
        let machine = world.machine();
        world.amend(&lantern, &topic_fields("lantern", &uses(&[&atlas])), "\n");
        let phone = topic(&world, "phone", &part_of(&[&lantern]));
        let relay = fact(&world, &lantern, "relay", "");
        let dimmer = fact(&world, &lantern, "dimmer", "idea = true\n");
        fact(&world, &phone, "case", "");
        let due = followup(
            &world,
            &[&lantern],
            "s",
            "look_again = 2026-10-01\nwhy = \"w\"\n",
        );
        followup(&world, &[&phone], "s", "");
        followup(&world, &[&lantern], "s", "");
        let placed = claim_fields(&machine, &lantern, "~/projects/lantern");
        world.put("claim", &placed, "\n");
        let loaded = |id: &DocumentId, label: &str, via: Value| {
            json!({
                "document": id,
                "label": label,
                "summary": "s",
                "via": via,
                "facts": [],
                "ideas": [],
                "parts": [],
            })
        };
        let filed = |id: &DocumentId, label: &str| row(id, "fact", label, &["lantern"]);
        let expected = json!({
            "machine": "desk",
            "topics": [
                {
                    "document": lantern,
                    "label": "lantern",
                    "summary": "s",
                    "via": {"type": "claim", "directory": "~/projects/lantern"},
                    "facts": [filed(&relay, "lantern/relay")],
                    "ideas": [filed(&dimmer, "lantern/dimmer")],
                    "parts": [{"label": "phone", "fact_count": 1, "open_count": 1}],
                },
                loaded(&atlas, "atlas", json!({"type": "edge"})),
                loaded(&machine, "desk", json!({"type": "machine"})),
            ],
            "due": [{
                "row": row(&due, "followup", due.short(), &["lantern"]),
                "trigger": {"type": "look_again", "on": "2026-10-01", "why": "w"},
                "due": true,
                "entry": null,
                "about": null,
            }],
            "open_count": 1,
            "forks": [],
            "draft_count": 0,
        });
        let here = context::context(&world.deps(), "/home/desk/projects/lantern/case").unwrap();
        vec![(here, expected)]
    }

    #[test]
    fn a_claim_on_no_directory_loads_its_topic_with_null_for_the_directory() {
        let (world, _, atlas) = world_with_atlas();
        world.put("claim", &claim_fields(&world.machine(), &atlas, ""), "\n");
        let nowhere = parsed(&context::context(&world.deps(), "/srv").unwrap());
        assert_eq!(
            nowhere["topics"][0]["via"],
            json!({"type": "claim", "directory": null})
        );
    }

    fn checks() -> Vec<(Check, Value)> {
        let Damaged { world, gadget, .. } = damaged();
        let label = gadget.short();
        let finding = |hashed: &[u8], what: &str| {
            let what = format!("{}: {what}", VersionId::of(hashed).short());
            json!({"label": label, "what": what})
        };
        let mut problems = [
            finding(b"corrupt", "the bytes do not hash to the file's name"),
            finding(b"malformed", "no envelope"),
        ];
        problems.sort_by_key(|problem| problem["what"].as_str().map(str::to_owned));
        let expected = json!({
            "problems": problems,
            "notices": [
                finding(b"newer", "written in format 2, a newer one"),
                {
                    "label": "gadget",
                    "what": "1 document of a kind this worklog does not know",
                },
            ],
            "fork_count": 0,
        });
        vec![(check::check(&world.deps()).unwrap(), expected)]
    }

    #[test]
    fn a_check_keeps_its_exit_code_under_json() {
        let Damaged { world, .. } = damaged();
        let found = check::check(&world.deps()).unwrap();
        assert_eq!(output(&found, true).exit, 1);
        assert_eq!(output(&Check::default(), true).exit, 0);
    }
}
