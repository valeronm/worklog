//! The form `--json` gives a result: the use case's own type, or for `agents` and `init` the
//! command line's own, one value and a newline.
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
//! version's id under `version` where the value is that version. A reference to another
//! document is an object with `document`, `label` and `ended`. `ended` is an ending's reason,
//! or `null`. A count is under a key ending in `_count`, and a plural noun is a list.

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

    use super::super::agents::{Initialized, Placed};
    use super::super::render::{Render, output};
    use super::*;
    use crate::app::activity::{self, Activity};
    use crate::app::check::Check;
    use crate::app::context::Context;
    use crate::app::draft::DraftRow;
    use crate::app::followup::{Made, NewFollowup, TriggerArg};
    use crate::app::list::{ClaimRow, EntryRow, FactRow, ForkRow, TopicRow};
    use crate::app::machines::PLACEHOLDER;
    use crate::app::search::{Hit, Log, Query};
    use crate::app::show::{Diff, History, Shown};
    use crate::app::testing::{
        ENDED as RETIRED, ENDED_FACT, NOW, World, claim_fields, entry, fact, followup, fork, head,
        head_id, part_of, topic, topic_fields, uses, world, world_with_atlas, written_at,
    };
    use crate::app::upgrade::{Checked, Upgrade};
    use crate::app::usage::MachineUsage;
    use crate::app::{
        DraftRef, FollowupRow, Written, amend, bulk, check, context, draft, followup as followups,
        list, search, setup, show, upgrade, usage,
    };
    use crate::domain::id::{DocumentId, VersionId};
    use crate::domain::ports::Usage as _;
    use crate::domain::schema::{Date, Directory, KindOf, Name};
    use crate::domain::session::Session;
    use crate::domain::testing::{
        ASSET, BINARY, DRAFTED, LANTERN, REFRESHED, atlas, build, first, lamp_session, published,
        ran, version as release,
    };
    use crate::domain::usage::Invocation;
    use crate::domain::version::{Kind, ReadError, Stamp};

    fn snake(word: &str) -> bool {
        word.starts_with(|first: char| first.is_ascii_lowercase())
            && word
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }

    fn conventional(value: &Value) -> Result<(), String> {
        match value {
            Value::Object(keyed) => keyed.iter().try_for_each(|(key, inner)| {
                let reference = keyed.len() == 3
                    && ["document", "label", "ended"]
                        .iter()
                        .all(|key| keyed.contains_key(*key));
                if reference && key != "ended" && !inner.is_string() {
                    return Err(format!("the {key} {inner} of a reference"));
                }
                if !snake(key) {
                    return Err(format!("the key `{key}`"));
                }
                if key == "type" && !inner.as_str().is_some_and(snake) {
                    return Err(format!("the type {inner}"));
                }
                if key == "id" {
                    return Err("the key `id`, which says no kind of id".to_owned());
                }
                let ending = key == "ended" || key == "document_ended";
                if ending && !(inner.is_string() || inner.is_null()) {
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
        a_listed_draft_holds_its_kind_its_label_its_path_its_opener_and_when_it_was_touched:
            Vec<DraftRow> = drafts,
        a_draft_discarded_is_null: () = discarded,
        a_followup_made_is_the_version_stored_or_the_draft_opened: Made = made,
        a_bound_host_is_its_machine_topic_whether_it_was_made_and_what_agents_got: Initialized = bound,
        the_paths_an_agents_command_wrote_are_listed: Placed = placed,
        a_topic_row_holds_its_row_and_the_labels_of_its_edges: Vec<TopicRow> = topics,
        a_fact_row_holds_its_date_and_an_ended_one_its_reason: Vec<FactRow> = facts,
        an_entry_row_holds_its_date: Vec<EntryRow> = entries,
        an_activity_holds_its_span_and_the_days_holding_an_entry: Activity = activities,
        a_followup_row_holds_its_trigger_by_type_and_null_for_what_it_lacks: Vec<FollowupRow> =
            followup_rows,
        a_claim_row_holds_null_for_a_claim_on_no_directory: Vec<ClaimRow> = claims,
        a_fork_row_holds_its_heads_and_null_for_a_row_no_head_reads_as: Vec<ForkRow> = forks,
        a_hit_holds_its_row_and_the_lines_that_matched: Vec<Hit> = hits,
        a_log_holds_its_machine_the_machines_and_null_for_a_row_its_document_does_not_read_as: Log =
            logged,
        a_shown_document_and_a_shown_version_are_told_apart_by_type: Shown = shown,
        a_history_holds_its_versions_and_why_each_unreadable_one_is: History = history,
        a_diff_holds_the_version_each_side_is_against_and_the_text_after: Diff = diffs,
        a_context_says_by_type_what_loaded_each_topic: Context = contexts,
        a_check_holds_its_findings_and_its_count_of_forks: Check = checks,
        a_machine_s_usage_holds_its_topic_and_a_count_for_each_command: Vec<MachineUsage> =
            usages,
        a_check_for_a_release_holds_the_version_running_and_the_latest: Checked = checked,
        an_upgrade_holds_both_versions_what_became_of_the_binary_and_the_paths_written: Upgrade =
            upgrades,
    }

    fn to(id: &DocumentId, label: &str) -> Value {
        json!({"document": id, "label": label, "ended": null})
    }

    fn to_ended(id: &DocumentId, label: &str, reason: &str) -> Value {
        json!({"document": id, "label": label, "ended": reason})
    }

    fn row(id: &DocumentId, kind: &str, label: &str, topics: &[Value]) -> Value {
        let name = match kind {
            "topic" => Some(label),
            "fact" => label.split_once('/').map(|(_, name)| name),
            "entry" => label.get("2026-10-08-".len()..),
            _ => None,
        };
        json!({
            "document": id,
            "kind": kind,
            "label": label,
            "name": name,
            "summary": "s",
            "topics": topics,
            "forked": false,
            "ended": null,
        })
    }

    fn unheld_machine() -> Value {
        to(&DocumentId::from_bytes([0xde; 16]), "dededede")
    }

    fn with(mut object: Value, key: &str, value: Value) -> Value {
        *object.get_mut(key).expect("a key the object has") = value;
        object
    }

    fn by_version(mut versions: Vec<Value>) -> Value {
        versions.sort_by_key(|version| version["version"].as_str().map(str::to_owned));
        Value::Array(versions)
    }

    fn text_field(name: &str, text: &str) -> Value {
        json!({"name": name, "values": [{"type": "text", "text": text}]})
    }

    fn reference_field(name: &str, reference: &Value) -> Value {
        json!({"name": name, "values": [{"type": "reference", "reference": reference}]})
    }

    fn topic_shown(name: &str) -> Vec<Value> {
        vec![
            text_field("name", name),
            text_field("created", "2026-09-04"),
            text_field("summary", "s"),
        ]
    }

    fn version(world: &World, id: &DocumentId) -> Value {
        let head = head(world, id);
        json!({
            "version": head.id,
            "written": NOW,
            "machine": to(&world.machine(), "desk"),
            "change": "new",
            "parents": [],
            "head": true,
            "ended": null,
            "text": "+++\nsize = 4\n+++\n\n",
            "fields": [text_field("size", "4")],
            "body": "\n",
            "links": [],
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
            conventional(&json!({"document_ended": 4})),
            Err("the ending 4".to_owned())
        );
        assert_eq!(conventional(&json!({"document_ended": null})), Ok(()));
        assert_eq!(
            conventional(&json!({"type": "look_again", "on": null, "ended": null})),
            Ok(())
        );
        assert_eq!(conventional(&json!({"ended": "retired"})), Ok(()));
        assert_eq!(
            conventional(&json!({"from": {"document": LANTERN, "label": 4, "ended": null}})),
            Err("the label 4 of a reference".to_owned())
        );
        assert_eq!(
            conventional(&json!({"from": {"document": null, "label": "lantern", "ended": null}})),
            Err("the document null of a reference".to_owned())
        );
        assert_eq!(
            conventional(
                &json!({"from": {"document": LANTERN, "label": "lantern", "ended": "retired"}})
            ),
            Ok(())
        );
        assert_eq!(
            conventional(&json!({"text": "phone", "document": null, "label": null})),
            Ok(())
        );
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
        let version = head_id(&world, &relay);
        assert!(version.as_str().starts_with("b3-"));
        let expected = json!({"document": relay, "label": "lantern/relay", "version": version});
        vec![(verified, expected)]
    }

    fn moved() -> Vec<(Vec<Written>, Value)> {
        let (world, lantern, _) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay", "");
        let moved = bulk::move_to(&world.deps(), &["lantern/relay"], "lantern", "atlas").unwrap();
        let version = head_id(&world, &relay);
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
        let (mut world, lantern, atlas) = world_with_atlas();
        let none = draft::drafts(&world.deps().read()).unwrap();
        world.host.set_session(Some(lamp_session()));
        draft::checkout(&world.deps(), "lantern").unwrap();
        world.host.set_session(None);
        draft::checkout(&world.deps(), "atlas").unwrap();
        let row = |document: &DocumentId, label: &str, opener: Value| {
            json!({
                "type": "open",
                "document": document,
                "kind": "topic",
                "label": label,
                "path": format!("memory:{document}"),
                "opener": opener,
                "touched": DRAFTED,
                "age_seconds": 761,
            })
        };
        let at_a_terminal = draft::drafts(&world.deps().read()).unwrap();
        let expected = json!([
            row(
                &lantern,
                "lantern",
                json!({"type": "other_session", "agent": "Lamp", "session": "s-1"})
            ),
            row(&atlas, "atlas", json!({"type": "no_session"})),
        ]);
        world.host.set_session(Some(lamp_session()));
        let in_the_session = draft::drafts(&world.deps().read()).unwrap();
        let this = json!([
            row(&lantern, "lantern", json!({"type": "this_session"})),
            row(&atlas, "atlas", json!({"type": "no_session"})),
        ]);
        world.drafts.break_draft(&atlas);
        let broken = draft::drafts(&world.deps().read()).unwrap();
        let with_broken = json!([
            row(&lantern, "lantern", json!({"type": "this_session"})),
            {
                "type": "broken",
                "document": atlas,
                "path": format!("memory:{atlas}"),
                "error": format!("memory:{atlas}: no opening fence"),
            },
        ]);
        vec![
            (at_a_terminal, expected),
            (in_the_session, this),
            (broken, with_broken),
            (none, json!([])),
        ]
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
            body: None,
            entry: None,
            about: None,
            trigger: Some(TriggerArg::Touching("lantern")),
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

    fn bound() -> Vec<(Initialized, Value)> {
        let mut world = World::new();
        world.host.set_machine(None);
        let bound = setup::init(&world.deps(), "desk", Some("A machine")).unwrap();
        let document = world.host.bound().expect("a host that init bound");
        let paths = vec!["/home/desk/.claude/settings.json".to_owned()];
        let expected = json!({
            "document": document,
            "label": "desk",
            "created": true,
            "written": paths,
        });
        let bare = json!({
            "document": document,
            "label": "desk",
            "created": true,
            "written": [],
        });
        let again = Initialized::new(bound.clone(), Vec::new());
        vec![(Initialized::new(bound, paths), expected), (again, bare)]
    }

    fn placed() -> Vec<(Placed, Value)> {
        let written = vec![
            "/home/desk/.claude/skills/worklog/SKILL.md".to_owned(),
            "/home/desk/.claude/settings.json".to_owned(),
        ];
        let nothing = Placed {
            written: Vec::new(),
        };
        let some = Placed {
            written: written.clone(),
        };
        vec![
            (some, json!({"written": written})),
            (nothing, json!({"written": []})),
        ]
    }

    fn topics() -> Vec<(Vec<TopicRow>, Value)> {
        let (world, lantern, atlas) = world_with_atlas();
        let edges = format!("{}{}", part_of(&[&lantern]), uses(&[&atlas]));
        let phone = topic(&world, "phone", &edges);
        fact(&world, &phone, "case", "");
        fact(&world, &phone, "strap", "");
        fact(&world, &phone, "dial", "idea = true\n");
        followup(&world, &[&phone, &atlas], "s", "");
        let counted = |id: &DocumentId, label: &str, open_count: usize, parts: Value| {
            json!({
                "row": row(id, "topic", label, &[]),
                "part_of": [],
                "uses": [],
                "parts": parts,
                "top": true,
                "fact_count": 0,
                "idea_count": 0,
                "open_count": open_count,
            })
        };
        let part = json!({
            "row": row(&phone, "topic", "phone", &[]),
            "part_of": [to(&lantern, "lantern")],
            "uses": [to(&atlas, "atlas")],
            "parts": [],
            "top": false,
            "fact_count": 2,
            "idea_count": 1,
            "open_count": 1,
        });
        let expected = json!([
            counted(&atlas, "atlas", 1, json!([])),
            counted(&world.machine(), "desk", 0, json!([])),
            counted(&lantern, "lantern", 0, json!([to(&phone, "phone")])),
            part,
        ]);
        let deps = world.deps();
        vec![
            (list::topics(&deps.read(), None, false).unwrap(), expected),
            (
                list::topics(&deps.read(), Some("lantern"), false).unwrap(),
                json!([part]),
            ),
            (
                list::topics(&deps.read(), Some("phone"), false).unwrap(),
                json!([]),
            ),
        ]
    }

    fn facts() -> Vec<(Vec<FactRow>, Value)> {
        let (world, lantern) = world();
        let deps = world.deps();
        let fuse = fact(&world, &lantern, "fuse", ENDED_FACT);
        let relay = fact(&world, &lantern, "relay", "");
        let dimmer = fact(&world, &lantern, "dimmer", "idea = true\n");
        let on = [to(&lantern, "lantern")];
        let filed = |id: &DocumentId, label: &str| json!({"row": row(id, "fact", label, &on), "confirmed": "2026-09-04"});
        let ended = with(
            row(&fuse, "fact", "lantern/fuse", &on),
            "ended",
            json!("false"),
        );
        vec![
            (
                list::facts(&deps.read(), None, true).unwrap(),
                json!([
                    {"row": ended, "confirmed": "2026-09-04"},
                    filed(&relay, "lantern/relay"),
                ]),
            ),
            (
                list::ideas(&deps.read(), None, false).unwrap(),
                json!([filed(&dimmer, "lantern/dimmer")]),
            ),
        ]
    }

    fn entries() -> Vec<(Vec<EntryRow>, Value)> {
        let (world, lantern) = world();
        let wiring = entry(&world, "2026-10-08", "wiring", &[&lantern]);
        let expected = json!([{
            "row": row(&wiring, "entry", "2026-10-08-wiring", &[to(&lantern, "lantern")]),
            "date": "2026-10-08",
        }]);
        let listed = list::entries(&world.deps().read(), None, None, None, false).unwrap();
        vec![(listed, expected)]
    }

    fn activities() -> Vec<(Activity, Value)> {
        let (world, lantern) = world();
        entry(&world, "2026-10-08", "wiring", &[&lantern]);
        let deps = world.deps();
        let last = json!({
            "from": "2025-10-06",
            "to": "2026-10-09",
            "year": null,
            "days": [{"date": "2026-10-08", "entry_count": 1}],
            "entry_count": 1,
            "years": [2026],
        });
        let year = json!({
            "from": "2025-01-01",
            "to": "2025-12-31",
            "year": 2025,
            "days": [],
            "entry_count": 0,
            "years": [2026],
        });
        vec![
            (activity::activity(&deps.read(), None, None).unwrap(), last),
            (
                activity::activity(&deps.read(), None, Some(2025)).unwrap(),
                year,
            ),
        ]
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
        let labeled = |id: &DocumentId, topic: Value| row(id, "followup", id.short(), &[topic]);
        let (on_lantern, on_atlas) = (to(&lantern, "lantern"), to(&atlas, "atlas"));
        let expected = json!([
            {
                "row": labeled(&dated, on_lantern.clone()),
                "trigger": {
                    "type": "look_again",
                    "on": "2026-10-01",
                    "why": "The part arrives",
                },
                "due": true,
                "entry": to(&wiring, "2026-10-08-wiring"),
                "about": on_atlas,
            },
            {
                "row": labeled(&waiting, on_lantern.clone()),
                "trigger": {"type": "touching", "topic": on_lantern},
                "due": false,
                "entry": null,
                "about": null,
            },
            {
                "row": labeled(&bare, on_atlas.clone()),
                "trigger": null,
                "due": false,
                "entry": null,
                "about": null,
            },
        ]);
        let listed = list::followups(&world.deps().read(), None, false).unwrap();

        world.put("claim", &claim_fields(&world.machine(), &lantern, ""), "\n");
        let mut due_here = expected.clone();
        let rows = due_here.as_array_mut().expect("the rows");
        rows.truncate(2);
        rows[1]["due"] = json!(true);
        let here = context::due(&world.deps().read(), "/srv/lantern").unwrap();
        vec![(listed, expected), (here, due_here)]
    }

    fn claims() -> Vec<(Vec<ClaimRow>, Value)> {
        let (mut world, lantern, atlas) = world_with_atlas();
        world.host.set_home(Some("/home/desk".to_owned()));
        let machine = world.machine();
        let placed = claim_fields(&machine, &lantern, "~/projects/lantern");
        let placed = world.put("claim", &placed, "\n");
        let anywhere = world.put("claim", &claim_fields(&machine, &atlas, ""), "\n");
        let expected = json!([
            {
                "document": anywhere,
                "machine": to(&machine, "desk"),
                "topic": to(&atlas, "atlas"),
                "directory": null,
                "exists": null,
                "forked": false,
            },
            {
                "document": placed,
                "machine": to(&machine, "desk"),
                "topic": to(&lantern, "lantern"),
                "directory": "~/projects/lantern",
                "exists": true,
                "forked": false,
            },
        ]);
        let listed = list::where_(&world.deps().read(), None, None).unwrap();
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
        let desk = to(&world.machine(), "desk");
        let read = by_version(vec![
            json!({"version": live.id, "written": early, "machine": desk, "ended": null}),
            json!({"version": retired.id, "written": late, "machine": desk, "ended": "retired"}),
        ]);
        let unread = fork(&world, &atlas, "name = 4\n");
        let unread = unread.iter().map(|id| {
            json!({
                "version": id,
                "written": "2026-09-04T10:00:00+01:00",
                "machine": unheld_machine(),
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
        vec![(list::forks(&world.deps().read()).unwrap(), expected)]
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
            "row": row(&relay, "fact", "lantern/relay", &[to(&lantern, "lantern")]),
            "lines": [{"text": "The relay.", "spans": [{"start": 0, "end": 9}]}],
            "label_spans": [],
            "unread": null,
        }]);
        let whole = search::search(&world.deps().read(), &query).unwrap();
        let torn = head_id(&world, &relay);
        world
            .store
            .plant_unreadable(&relay, torn.clone(), ReadError::Corrupt);
        let unread = json!([{
            "row": row(&relay, "fact", "lantern/relay", &[to(&lantern, "lantern")]),
            "lines": [],
            "label_spans": [{"start": 8, "end": 13}],
            "unread": {
                "version": torn,
                "why": {"type": "corrupt", "text": "the bytes do not hash to the file's name"},
            },
        }]);
        let by_label = Query {
            term: "relay",
            ..query
        };
        vec![
            (whole, expected),
            (
                search::search(&world.deps().read(), &by_label).unwrap(),
                unread,
            ),
        ]
    }

    fn logged() -> Vec<(Log, Value)> {
        let (world, lantern) = world();
        let gadget = world.put("gadget", "size = 4\n", "\n");
        let logged = |id: &DocumentId, version: &VersionId, row: Value| {
            let label = row["label"].as_str().unwrap_or(id.short());
            json!({
                "document": id,
                "label": label,
                "version": version,
                "written": NOW,
                "machine": to(&world.machine(), "desk"),
                "change": "new",
                "ended": null,
                "row": row,
            })
        };
        let first = |id: &DocumentId, row: Value| logged(id, &head(&world, id).id, row);
        let root = head_id(&world, &lantern);
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
        let desk = to(&world.machine(), "desk");
        let log = |machine: Option<&str>| search::log(&world.deps().read(), 4, machine).unwrap();
        vec![
            (
                log(None),
                json!({ "machine": null, "machines": [desk], "versions": expected }),
            ),
            (
                log(Some("desk")),
                json!({ "machine": desk, "machines": [desk], "versions": expected }),
            ),
        ]
    }

    fn forked(world: &World) -> (Shown, Value) {
        let phone = topic(world, "phone", "");
        let root = head_id(world, &phone);
        let heads = fork(world, &phone, &topic_fields("phone", ""));
        let of_forked = show::show(&world.deps().read(), "phone").unwrap();
        let bodies: Vec<&str> = match &of_forked {
            Shown::Document(document) => (document.heads.iter())
                .map(|head| head.text.rsplit("+++\n").next().unwrap_or_default())
                .collect(),
            Shown::Version(_) => Vec::new(),
        };
        let mut sorted = bodies.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, ["left\n", "right\n"]);
        let sides: Vec<Value> = (heads.iter().zip(&bodies))
            .map(|(version, body)| {
                json!({
                    "version": version,
                    "written": "2026-09-04T10:00:00+01:00",
                    "machine": unheld_machine(),
                    "change": "save",
                    "parents": [root],
                    "head": true,
                    "ended": null,
                    "text": format!(
                        "+++\nname = \"phone\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n{body}"
                    ),
                    "fields": topic_shown("phone"),
                    "body": body,
                    "links": [],
                })
            })
            .collect();
        let expected = json!({
            "type": "document",
            "document": phone,
            "kind": "topic",
            "reads_as": "topic",
            "label": "phone",
            "former": [],
            "forked": true,
            "ended": null,
            "heads": sides,
            "unreadable": [],
            "linked_from": [],
        });
        (of_forked, expected)
    }

    fn linking(world: &World, lantern: &DocumentId) -> (Shown, Value) {
        let compass = topic(world, "compass", "");
        let made = head_id(world, &compass);
        let unheld = DocumentId::from_bytes([0x42; 16]);
        let body = "See [[lantern]], [[phone]] and [[unknown]].\n";
        let linking = world.amend_linking(
            &compass,
            &topic_fields("compass", ""),
            body,
            &[
                ("lantern", Some(lantern)),
                ("phone", Some(&unheld)),
                ("unknown", None),
            ],
        );
        let linked = json!({
            "type": "version",
            "document": compass,
            "label": "compass",
            "document_ended": null,
            "version": linking,
            "written": NOW,
            "machine": to(&world.machine(), "desk"),
            "change": "save",
            "parents": [made],
            "head": true,
            "ended": null,
            "text": format!(
                "+++\nname = \"compass\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n{body}"
            ),
            "fields": topic_shown("compass"),
            "body": body,
            "links": [
                {"text": "lantern", "target": to_ended(lantern, "lantern", "retired")},
                {"text": "phone", "target": to(&unheld, unheld.short())},
                {"text": "unknown", "target": null},
            ],
            "sides": [{
                "against": made,
                "held": true,
                "before": "+++\nname = \"compass\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n\n",
            }],
        });
        let of_linking = show::show(&world.deps().read(), linking.as_str()).unwrap();
        (of_linking, linked)
    }

    fn linked(world: &World, lantern: &DocumentId, compass: &DocumentId) -> (Shown, Value) {
        let deps = world.deps();
        let mut of_linked = show::show(&deps.read(), lantern.as_str()).unwrap();
        let mut expected = serde_json::to_value(&of_linked).unwrap();
        expected["linked_from"] = json!([{
            "row": {
                "document": compass,
                "kind": "topic",
                "label": "compass",
                "name": "compass",
                "summary": "s",
                "topics": [],
                "forked": false,
                "ended": null,
            },
            "by": ["link"],
        }]);
        if let Shown::Document(document) = &mut of_linked {
            document.linked_from = show::linked_from(&deps.read(), lantern, false).unwrap();
        }
        (of_linked, expected)
    }

    fn referring(world: &World, lantern: &DocumentId) -> (Shown, Value) {
        let machine = world.machine();
        let driver = world.put(
            "entry",
            &format!(
                "name = \"lamp-driver\"\nformer_names = [\"old-driver\"]\ndate = 2026-10-08\n\
                 machine = \"{machine}\"\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n"
            ),
            "Wired.\n",
        );
        let expected = json!({
            "type": "document",
            "document": driver,
            "kind": "entry",
            "reads_as": "entry",
            "label": "2026-10-08-lamp-driver",
            "former": ["2026-10-08-old-driver"],
            "forked": false,
            "ended": null,
            "heads": [{
                "version": head(world, &driver).id,
                "written": NOW,
                "machine": to(&machine, "desk"),
                "change": "new",
                "parents": [],
                "head": true,
                "ended": null,
                "text": "+++\nname = \"lamp-driver\"\nformer_names = [\"old-driver\"]\n\
                         date = 2026-10-08\nmachine = \"desk\"\ntopics = [\"lantern\"]\n\
                         summary = \"s\"\n+++\nWired.\n",
                "fields": [
                    text_field("name", "lamp-driver"),
                    text_field("former_names", "old-driver"),
                    text_field("date", "2026-10-08"),
                    reference_field("machine", &to(&machine, "desk")),
                    reference_field("topics", &to_ended(lantern, "lantern", "retired")),
                    text_field("summary", "s"),
                ],
                "body": "Wired.\n",
                "links": [],
            }],
            "unreadable": [],
            "linked_from": [],
        });
        let of_referring = show::show(&world.deps().read(), driver.as_str()).unwrap();
        (of_referring, expected)
    }

    fn old_version(world: &World, lantern: &DocumentId, root: &VersionId) -> (Shown, Value) {
        let of_old = show::show(&world.deps().read(), root.short()).unwrap();
        let old = json!({
            "type": "version",
            "document": lantern,
            "label": "lantern",
            "document_ended": "retired",
            "version": root,
            "written": NOW,
            "machine": to(&world.machine(), "desk"),
            "change": "new",
            "parents": [],
            "head": false,
            "ended": null,
            "text": "+++\nname = \"lantern\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n\n",
            "fields": topic_shown("lantern"),
            "body": "\n",
            "links": [],
            "sides": [{"against": null, "held": true, "before": ""}],
        });
        (of_old, old)
    }

    fn retired(world: &World, lantern: &DocumentId, root: &VersionId) -> (Shown, Value) {
        let retired = world.amend(lantern, &topic_fields("lantern", RETIRED), "\n");
        let ended = json!({
            "type": "document",
            "document": lantern,
            "kind": "topic",
            "reads_as": "topic",
            "label": "lantern",
            "former": [],
            "forked": false,
            "ended": "retired",
            "heads": [{
                "version": retired,
                "written": NOW,
                "machine": to(&world.machine(), "desk"),
                "change": "save",
                "parents": [root],
                "head": true,
                "ended": "retired",
                "text": "+++\nname = \"lantern\"\ncreated = 2026-09-04\nsummary = \"s\"\n\
                             ended = \"retired\"\nended_on = 2026-10-09\nnote = \"n\"\n+++\n\n",
                "fields": [
                    text_field("name", "lantern"),
                    text_field("created", "2026-09-04"),
                    text_field("summary", "s"),
                    text_field("ended", "retired"),
                    text_field("ended_on", "2026-10-09"),
                    text_field("note", "n"),
                ],
                "body": "\n",
                "links": [],
            }],
            "unreadable": [],
            "linked_from": [],
        });
        let of_ended = show::show(&world.deps().read(), "lantern").unwrap();
        (of_ended, ended)
    }

    fn unread(world: &World) -> (Shown, Value) {
        let lost = VersionId::of(b"lost");
        world
            .store
            .plant_unreadable(&atlas(), lost.clone(), ReadError::Corrupt);
        let unread = json!({
            "type": "document",
            "document": atlas(),
            "kind": null,
            "reads_as": null,
            "label": atlas().short(),
            "former": [],
            "forked": false,
            "ended": null,
            "heads": [],
            "unreadable": [{
                "version": lost,
                "why": {"type": "corrupt", "text": "the bytes do not hash to the file's name"},
            }],
            "linked_from": [],
        });
        let of_unread = show::show(&world.deps().read(), atlas().short()).unwrap();
        (of_unread, unread)
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
            "reads_as": null,
            "label": label,
            "former": [],
            "forked": false,
            "ended": null,
            "heads": [version(&world, &gadget)],
            "unreadable": unreadable,
            "linked_from": [],
        });
        let of_unknown = show::show(&deps.read(), label).unwrap();

        let hash = head_id(&world, &gadget);
        let mut one = json!({
            "type": "version",
            "document": gadget,
            "label": label,
            "document_ended": null,
        });
        let rest = version(&world, &gadget);
        let fields = rest.as_object().expect("an object").clone();
        one.as_object_mut().expect("an object").extend(fields);
        one["sides"] = json!([{"against": null, "held": true, "before": ""}]);
        let of_version = show::show(&deps.read(), hash.short()).unwrap();

        let root = head_id(&world, &lantern);
        let ended = retired(&world, &lantern, &root);
        let unread = unread(&world);
        let (old, forked) = (old_version(&world, &lantern, &root), forked(&world));
        let linking = linking(&world, &lantern);
        let Shown::Version(compared) = &linking.0 else {
            panic!("expected a version");
        };
        let compass = compared.document.clone();

        vec![
            old,
            forked,
            linking,
            linked(&world, &lantern, &compass),
            referring(&world, &lantern),
            (of_unknown, unknown),
            (of_version, one),
            ended,
            unread,
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
            "ended": null,
            "versions": [{
                "version": head(&world, &gadget).id,
                "written": NOW,
                "machine": to(&world.machine(), "desk"),
                "change": "new",
                "parents": [],
                "head": true,
                "ended": null,
            }],
            "unreadable": unreadable,
        });
        let of_gadget = show::history(&world.deps().read(), label).unwrap();
        let desk = world.machine();
        let made = head_id(&world, &desk);
        let retired = world.amend(&desk, &topic_fields("desk", RETIRED), "\n");
        let row = |version: &VersionId, change: &str, parents: Value, ended: Value| {
            json!({
                "version": version,
                "written": NOW,
                "machine": to_ended(&desk, "desk", "retired"),
                "change": change,
                "parents": parents,
                "head": ended.is_string(),
                "ended": ended,
            })
        };
        let of_ended = json!({
            "document": desk,
            "label": "desk",
            "ended": "retired",
            "versions": [
                row(&retired, "save", json!([made]), json!("retired")),
                row(&made, "new", json!([]), Value::Null),
            ],
            "unreadable": [],
        });
        vec![
            (of_gadget, expected),
            (
                show::history(&world.deps().read(), "desk").unwrap(),
                of_ended,
            ),
        ]
    }

    fn diffs() -> Vec<(Diff, Value)> {
        let (world, lantern) = world();
        let deps = world.deps();
        let root = head_id(&world, &lantern);
        draft::checkout(&deps, "lantern").unwrap();
        let text = "+++\nname = \"lantern\"\nsummary = \"s\"\n+++\n\n";
        let drafted = json!({
            "label": "lantern",
            "sides": [{"against": root, "held": true, "before": text}],
            "after": text,
        });
        let of_draft = show::diff(&deps.read(), "lantern", None).unwrap();

        let made = json!({
            "label": "lantern",
            "sides": [{"against": null, "held": true, "before": ""}],
            "after": "+++\nname = \"lantern\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n\n",
        });
        let of_first = show::diff(&deps.read(), root.short(), None).unwrap();

        let fields = topic_fields("atlas", "");
        let lost = first(&atlas(), "topic", &fields, "\n");
        let kept = written_at(&world, &lost, &fields, NOW, "\n", None);
        let unheld = json!({
            "label": "atlas",
            "sides": [{"against": lost.id, "held": false, "before": ""}],
            "after": "+++\nname = \"atlas\"\ncreated = 2026-09-04\nsummary = \"s\"\n+++\n\n",
        });
        let of_unheld = show::diff(&deps.read(), kept.id.short(), None).unwrap();
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
        fact(&world, &phone, "strap", "idea = true\n");
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
        let compass = topic(&world, "compass", "");
        world.amend(&machine, &topic_fields("desk", &uses(&[&compass])), "\n");
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
        let on = [to(&lantern, "lantern")];
        let filed = |id: &DocumentId, label: &str| row(id, "fact", label, &on);
        let expected = json!({
            "machine": to(&machine, "desk"),
            "directory": "~/projects/lantern/case",
            "topics": [
                {
                    "document": lantern,
                    "label": "lantern",
                    "summary": "s",
                    "via": {"type": "claim", "directory": "~/projects/lantern", "closest": true},
                    "facts": [filed(&relay, "lantern/relay")],
                    "ideas": [filed(&dimmer, "lantern/dimmer")],
                    "parts": [{
                        "document": phone,
                        "label": "phone",
                        "fact_count": 1,
                        "idea_count": 1,
                        "open_count": 1,
                    }],
                },
                loaded(&atlas, "atlas", json!({"type": "edge", "from": on[0]})),
                loaded(&machine, "desk", json!({"type": "machine", "from": null})),
                loaded(&compass, "compass", json!({"type": "machine", "from": to(&machine, "desk")})),
            ],
            "due": [{
                "row": row(&due, "followup", due.short(), &on),
                "trigger": {"type": "look_again", "on": "2026-10-01", "why": "w"},
                "due": true,
                "entry": null,
                "about": null,
            }],
            "open_count": 1,
            "forks": [],
            "draft_count": 0,
            "machine_needs_summary": false,
            "unclaimed": false,
        });
        let here =
            context::context(&world.deps().read(), "/home/desk/projects/lantern/case").unwrap();
        vec![(here, expected)]
    }

    #[test]
    fn a_context_says_when_the_machine_topic_still_has_the_placeholder_summary() {
        let (world, _) = world();
        let held = topic_fields("desk", "").replace("\"s\"", &format!("{PLACEHOLDER:?}"));
        world.amend(&world.machine(), &held, "\n");
        let text = parsed(&context::context(&world.deps().read(), "/srv").unwrap());
        assert_eq!(text["machine_needs_summary"], json!(true));
    }

    #[test]
    fn a_claim_on_no_directory_loads_its_topic_with_null_for_the_directory() {
        let (world, _, atlas) = world_with_atlas();
        world.put("claim", &claim_fields(&world.machine(), &atlas, ""), "\n");
        let nowhere = parsed(&context::context(&world.deps().read(), "/srv").unwrap());
        assert_eq!(
            nowhere["topics"][0]["via"],
            json!({"type": "claim", "directory": null, "closest": true})
        );
    }

    fn checks() -> Vec<(Check, Value)> {
        let Damaged {
            world,
            gadget,
            lantern,
            ..
        } = damaged();
        world.amend(&lantern, &topic_fields("lantern", RETIRED), "\n");
        let lost = VersionId::of(b"lost");
        world
            .store
            .plant_unreadable(&lantern, lost.clone(), ReadError::Corrupt);
        let of_ended = json!({
            "document": lantern,
            "label": "lantern",
            "ended": "retired",
            "what": format!("{}: the bytes do not hash to the file's name", lost.short()),
        });
        let label = gadget.short();
        let finding = |hashed: &[u8], what: &str| {
            let what = format!("{}: {what}", VersionId::of(hashed).short());
            json!({"document": gadget, "label": label, "ended": null, "what": what})
        };
        let mut problems = vec![
            finding(b"corrupt", "the bytes do not hash to the file's name"),
            finding(b"malformed", "no envelope"),
        ];
        problems.sort_by_key(|problem| problem["what"].as_str().map(str::to_owned));
        problems.push(of_ended);
        let expected = json!({
            "problems": problems,
            "notices": [
                finding(b"newer", "written in format 2, a newer one"),
                {
                    "document": null,
                    "label": "gadget",
                    "ended": null,
                    "what": "1 document of a kind this worklog does not know",
                },
            ],
            "fork_count": 0,
        });
        vec![(check::check(&world.deps().read()).unwrap(), expected)]
    }

    fn usages() -> Vec<(Vec<MachineUsage>, Value)> {
        let (world, _) = world();
        let unheld = DocumentId::from_bytes([0xde; 16]);
        for (machine, command) in [
            (&world.machine(), "show"),
            (&world.machine(), "new entry"),
            (&world.machine(), "show"),
            (&unheld, "context"),
        ] {
            let run = ran(machine, command, "2026-09-04");
            world.usage.record(&run).unwrap();
        }
        let refused = Invocation {
            session: Some(Session {
                agent: "Lamp".to_owned(),
                id: "s1".to_owned(),
            }),
            exit: 1,
            refusal: Some("torch: names no document".to_owned()),
            ..ran(&world.machine(), "show", "2026-09-04")
        };
        world.usage.record(&refused).unwrap();
        let expected = json!([
            {
                "machine": to(&unheld, unheld.as_str()),
                "topic": null,
                "agents": [{"agent": null, "run_count": 1, "session_count": 0}],
                "commands": [{"command": "context", "run_count": 1, "refusals": []}],
            },
            {
                "machine": to(&world.machine(), "desk"),
                "topic": "desk",
                "agents": [
                    {"agent": null, "run_count": 3, "session_count": 0},
                    {"agent": "Lamp", "run_count": 1, "session_count": 1},
                ],
                "commands": [
                    {
                        "command": "show",
                        "run_count": 3,
                        "refusals": [{"text": "torch: names no document", "count": 1}],
                    },
                    {"command": "new entry", "run_count": 1, "refusals": []},
                ],
            },
        ]);
        let deps = world.deps();
        let none = usage::usage(&deps.read(), None, Some("2099-01-01"), None).unwrap();
        vec![
            (
                usage::usage(&deps.read(), None, None, None).unwrap(),
                expected,
            ),
            (none, json!([])),
        ]
    }

    fn checked() -> Vec<(Checked, Value)> {
        let releases = published("v0.10.0", ASSET, b"", b"");
        let found = |current: &str, newer: bool| {
            let (checked, _) = upgrade::check(&releases, release(current).into()).unwrap();
            let expected = json!({"current": current, "latest": "0.10.0", "newer": newer});
            (checked, expected)
        };
        let (kept, _) = upgrade::check(&releases, build("0.10.0")).unwrap();
        let named = json!({"current": "0.10.0+3f2a1c9", "latest": "0.10.0", "newer": false});
        vec![
            found("0.9.0", true),
            found("0.10.0", false),
            found("0.11.0", false),
            (kept, named),
        ]
    }

    fn upgrades() -> Vec<(Upgrade, Value)> {
        let releases = published("v0.3.0", ASSET, b"new", b"new");
        let own = "/home/desk/.claude/settings.json";
        let ran = |current: &str, written: &[&str]| {
            let (checked, _) = upgrade::check(&releases, release(current).into()).unwrap();
            checked.upgrade(written.iter().map(|path| (*path).to_owned()).collect())
        };
        let stayed = |current: &str, outcome: &str| json!({"current": current, "latest": "0.3.0", "outcome": outcome, "written": [own]});
        let upgraded = json!({
            "current": "0.2.1",
            "latest": "0.3.0",
            "outcome": "upgraded",
            "written": [BINARY, REFRESHED],
        });
        vec![
            (ran("0.2.1", &[BINARY, REFRESHED]), upgraded),
            (ran("0.3.0", &[own]), stayed("0.3.0", "current")),
            (ran("0.4.0", &[own]), stayed("0.4.0", "ahead")),
        ]
    }

    #[test]
    fn a_check_keeps_its_exit_code_under_json() {
        let Damaged { world, .. } = damaged();
        let found = check::check(&world.deps().read()).unwrap();
        assert_eq!(output(&found, true).exit, 1);
        assert_eq!(output(&Check::default(), true).exit, 0);
    }
}
