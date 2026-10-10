use std::cmp::Reverse;
use std::collections::BTreeMap;

use regex::{Regex, RegexBuilder};
use serde::Serialize;

use crate::app::heads::{ended_of, label_or_short, machine_label};
use crate::app::rows::{Row, by_label, documents, reading, row, shown};
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::schema::KindOf;
use crate::domain::version::{Stamp, Version};

const SEARCHED: [KindOf; 4] = [KindOf::Fact, KindOf::Topic, KindOf::Entry, KindOf::Followup];

pub struct Query<'a> {
    pub term: &'a str,
    pub regex: bool,
    pub topic: Option<&'a str>,
    pub ended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub row: Row,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Logged {
    pub document: DocumentId,
    /// The label of `row`, or the start of the document's id when there is none.
    pub label: String,
    pub version: VersionId,
    pub written: Stamp,
    pub machine: String,
    pub change: String,
    /// The reason; `None` too for a version that does not read as a record.
    pub ended: Option<String>,
    pub row: Option<Row>,
}

/// Facts and ideas first, then topics, entries, followups; within a kind by label.
///
/// A document matches by its label, its summary or a body line; the term ignores case and,
/// unless `regex`, is matched as plain text. `lines` are the matching summary and body
/// lines as written, trimmed. An empty term and a pattern that does not compile are usage
/// failures; an address that is not one topic is refused. A claim is never searched, and a
/// forked document is searched in its row head only.
pub fn search(deps: &Deps, query: &Query) -> Result<Vec<Hit>, Failure> {
    let matcher = matcher(query)?;
    let (lookup, topics, topic) = reading(deps, query.topic)?;
    let mut hits = Vec::new();
    for kind in SEARCHED {
        let mut found = Vec::new();
        let documents = match kind {
            KindOf::Fact | KindOf::Topic | KindOf::Entry | KindOf::Followup => {
                documents(&lookup, &topics, kind, topic.as_ref(), query.ended)?
            }
            KindOf::Claim => continue,
        };
        for shown in shown(&topics, &documents, query.ended) {
            let row = shown.row;
            let lines: Vec<String> = std::iter::once(row.summary.as_str())
                .chain(shown.head.body.lines())
                .filter(|line| matcher.is_match(line))
                .map(|line| line.trim().to_owned())
                .collect();
            if !lines.is_empty() || matcher.is_match(&row.label) {
                found.push(Hit { row, lines });
            }
        }
        by_label(&mut found, |hit| &hit.row);
        hits.extend(found);
    }
    Ok(hits)
}

fn matcher(query: &Query) -> Result<Regex, Failure> {
    if query.term.trim().is_empty() {
        return Err(Failure::Usage("search needs a term".to_owned()));
    }
    let pattern = if query.regex {
        query.term.to_owned()
    } else {
        regex::escape(query.term)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map_err(|error| Failure::Usage(format!("bad pattern: {error}")))
}

/// Newest `written` first, ties by version id; at most `limit`.
///
/// Lists every held version of every kind, a fork's heads and the kinds this worklog does
/// not know included; `row` is the row of the version's document as it stands now, `None`
/// when no head of that document reads as a record. `machine` names a topic and keeps the
/// versions written there; an address that is not one topic is refused.
pub fn log(deps: &Deps, limit: usize, machine: Option<&str>) -> Result<Vec<Logged>, Failure> {
    let (lookup, topics, machine) = reading(deps, machine)?;
    let mut documents = lookup.everything()?;
    for unknown in lookup.unknown()? {
        documents.extend(unknown.documents);
    }
    let mut versions: Vec<(&Version, &Document)> = Vec::new();
    for document in &documents {
        let written = document.history().into_iter().filter(|version| {
            (machine.as_ref()).is_none_or(|wanted| *wanted == version.envelope.machine)
        });
        versions.extend(written.map(|version| (version, &**document)));
    }
    versions.sort_by(|(a, _), (b, _)| {
        (Reverse(a.envelope.written.instant()), &a.id)
            .cmp(&(Reverse(b.envelope.written.instant()), &b.id))
    });
    versions.truncate(limit);
    let mut rows: BTreeMap<&DocumentId, Option<Row>> = BTreeMap::new();
    let logged = versions.into_iter().map(|(version, document)| {
        let of_document = rows
            .entry(document.id())
            .or_insert_with(|| row(&topics, document).map(|found| found.row));
        let label = of_document.as_ref().map(|row| row.label.clone());
        Logged {
            document: document.id().clone(),
            label: label_or_short(label, document.id()),
            version: version.id.clone(),
            written: version.envelope.written.clone(),
            machine: machine_label(version, topics.name(&version.envelope.machine)),
            change: version.envelope.change.clone(),
            ended: ended_of(version),
            row: of_document.clone(),
        }
    });
    Ok(logged.collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::heads::row_head;
    use crate::app::lookup::Stored;
    use crate::app::testing::{
        ENDED, World, fact, fact_fields, fork, head, labels, topic, world, written_at,
    };
    use crate::domain::id::DocumentId;
    use crate::domain::ports::Store;

    fn find(world: &World, term: &str) -> Vec<Hit> {
        query(world, term, false, None, false)
    }

    fn query(world: &World, term: &str, regex: bool, topic: Option<&str>, ended: bool) -> Vec<Hit> {
        search(
            &world.deps(),
            &Query {
                term,
                regex,
                topic,
                ended,
            },
        )
        .unwrap()
    }

    fn fact_with(
        world: &World,
        topic: &DocumentId,
        name: &str,
        summary: &str,
        body: &str,
    ) -> DocumentId {
        world.put("fact", &fact_fields(topic, name, summary, ""), body)
    }

    #[test]
    fn a_document_matches_by_its_label_alone() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let hits = find(&world, "relay-pin");
        assert_eq!(labels(&hits, |hit| &hit.row), ["lantern/relay-pin"]);
        assert!(hits[0].lines.is_empty());
    }

    #[test]
    fn a_document_matches_by_summary_and_body_lines_trimmed_in_order() {
        let (world, lantern) = world();
        fact_with(
            &world,
            &lantern,
            "driver",
            "The Pin is fixed",
            "first\nthe pin moved\n   indented PIN  \nnothing\n",
        );
        let hits = find(&world, "pin");
        assert_eq!(labels(&hits, |hit| &hit.row), ["lantern/driver"]);
        assert_eq!(
            hits[0].lines,
            ["The Pin is fixed", "the pin moved", "indented PIN"]
        );
    }

    #[test]
    fn a_term_ignores_case_and_a_literal_one_is_no_pattern() {
        let (world, lantern) = world();
        fact_with(&world, &lantern, "driver", "fixed", "a fix.d line\n");
        assert_eq!(find(&world, "FIXED").len(), 1);
        assert_eq!(find(&world, "FIX.D").len(), 1);
        assert!(find(&world, "fi.ed").is_empty());
        assert_eq!(query(&world, "FI.ED", true, None, false).len(), 1);
        assert_eq!(query(&world, "^fixed$", true, None, false).len(), 1);
    }

    #[test]
    fn a_pattern_that_does_not_compile_is_a_usage_failure() {
        let (world, _) = world();
        let result = search(
            &world.deps(),
            &Query {
                term: "(",
                regex: true,
                topic: None,
                ended: false,
            },
        );
        let Err(Failure::Usage(text)) = result else {
            panic!("expected a usage failure, got {result:?}");
        };
        assert!(text.contains("pattern"), "{text}");
        let literal = search(
            &world.deps(),
            &Query {
                term: "(",
                regex: false,
                topic: None,
                ended: false,
            },
        );
        assert_eq!(literal, Ok(Vec::new()));
    }

    #[test]
    fn an_empty_term_is_a_usage_failure() {
        let (world, _) = world();
        let result = search(
            &world.deps(),
            &Query {
                term: "  ",
                regex: false,
                topic: None,
                ended: false,
            },
        );
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    #[test]
    fn hits_come_facts_first_then_topics_entries_followups_each_by_label() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = world.put(
            "topic",
            "name = \"lantern\"\ncreated = 2026-09-04\nsummary = \"zebra\"\n",
            "\n",
        );
        let atlas = world.put(
            "topic",
            "name = \"atlas\"\ncreated = 2026-09-04\nsummary = \"zebra\"\n",
            "\n",
        );
        fact_with(&world, &lantern, "b-fact", "zebra", "\n");
        world.put(
            "fact",
            &format!(
                "name = \"a-idea\"\ntopic = \"{atlas}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nidea = true\nsummary = \"zebra\"\n"
            ),
            "\n",
        );
        world.put(
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\n\
                 topics = [\"{lantern}\"]\nsummary = \"zebra\"\n",
                world.machine()
            ),
            "\n",
        );
        let followup = world.put(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"zebra\"\n"),
            "\n",
        );
        let hits = find(&world, "zebra");
        assert_eq!(
            labels(&hits, |hit| &hit.row),
            [
                "atlas/a-idea",
                "lantern/b-fact",
                "atlas",
                "lantern",
                "2026-10-08-lamp-driver",
                followup.short()
            ]
        );
    }

    #[test]
    fn a_claim_is_never_returned() {
        let (world, lantern) = world();
        let desk = world.machine();
        let claim = world.put(
            "claim",
            &format!("machine = \"{desk}\"\ntopic = \"{lantern}\"\n"),
            "\n",
        );
        assert!(find(&world, claim.short()).is_empty());
        assert!(
            query(&world, ".", true, None, true)
                .iter()
                .all(|hit| hit.row.kind != KindOf::Claim)
        );
    }

    #[test]
    fn a_topic_narrows_to_what_belongs_to_it_and_its_parts() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &format!("part_of = [\"{lantern}\"]\n"));
        let phone = topic(&world, "phone", "");
        fact(&world, &lantern, "own", "");
        fact(&world, &atlas, "part", "");
        fact(&world, &phone, "other", "");
        let hits = query(&world, ".", true, Some("lantern"), false);
        assert_eq!(
            labels(&hits, |hit| &hit.row),
            ["atlas/part", "lantern/own", "atlas", "lantern"]
        );
        let hits = query(&world, ".", true, Some("atlas"), false);
        assert_eq!(labels(&hits, |hit| &hit.row), ["atlas/part", "atlas"]);
    }

    #[test]
    fn an_unknown_topic_is_refused() {
        let (world, _) = world();
        let result = search(
            &world.deps(),
            &Query {
                term: "x",
                regex: false,
                topic: Some("nowhere"),
                ended: false,
            },
        );
        assert!(matches!(result, Err(Failure::Refused(_))), "{result:?}");
    }

    #[test]
    fn an_ended_document_is_left_out_until_asked_for() {
        let (world, lantern) = world();
        fact(&world, &lantern, "live", "");
        fact(&world, &lantern, "gone", ENDED);
        assert_eq!(
            labels(&find(&world, "relay"), |hit| &hit.row),
            ["lantern/live"]
        );
        assert_eq!(
            labels(&find(&world, "gone"), |hit| &hit.row),
            Vec::<&str>::new()
        );
        let all = query(&world, "relay", false, None, true);
        assert_eq!(
            labels(&all, |hit| &hit.row),
            ["lantern/gone", "lantern/live"]
        );
        assert_eq!(all[0].row.ended.as_deref(), Some("retired"));
        let under = query(&world, "relay", false, Some("lantern"), true);
        assert_eq!(
            labels(&under, |hit| &hit.row),
            ["lantern/gone", "lantern/live"]
        );
    }

    #[test]
    fn a_forked_document_is_searched_in_its_row_head_only() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let fields = format!(
            "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
             confirmed = 2026-09-04\nsummary = \"s\"\n"
        );
        fork(&world, &relay, &fields);
        let hits = query(&world, "^(left|right)$", true, None, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].row.forked);
        assert_eq!(hits[0].lines.len(), 1);
        let shown = head_body(&world, &relay);
        assert_eq!(hits[0].lines[0], shown);
    }

    fn head_body(world: &World, id: &DocumentId) -> String {
        let held = world.store.document(id).unwrap();
        let (version, _) = row_head(&held).unwrap();
        version.body.trim().to_owned()
    }

    #[test]
    fn a_whole_store_search_asks_one_question_per_kind() {
        use crate::app::testing::Counting;
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let counting = Counting::over(world.store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &world.drafts,
            ids: &world.ids,
            clock: &world.clock,
            host: &world.host,
        };
        search(
            &deps,
            &Query {
                term: "relay",
                regex: false,
                topic: None,
                ended: true,
            },
        )
        .unwrap();
        assert_eq!(counting.kinds.get(), KindOf::ALL.len() - 1);
        assert_eq!(counting.documents.get(), 0);
        assert_eq!(counting.holdings.get(), 0);
    }

    fn rewritten(world: &World, id: &DocumentId, stamp: &str, machine: &DocumentId) -> VersionId {
        let held = head(world, id);
        written_at(
            world,
            &held,
            &held.fields.to_string(),
            stamp,
            &held.body,
            Some(machine),
        )
        .id
    }

    fn logged(world: &World, limit: usize, machine: Option<&str>) -> Vec<Logged> {
        log(&world.deps(), limit, machine).unwrap()
    }

    #[test]
    fn the_log_lists_every_held_version_newest_first_then_by_version_id() {
        let (world, lantern) = world();
        let desk = world.machine();
        let phone = topic(&world, "phone", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let older = rewritten(&world, &relay, "2026-10-10T09:00:00+00:00", &phone);
        let newer = rewritten(&world, &relay, "2026-10-11T09:00:00+00:00", &desk);
        let all = logged(&world, usize::MAX, None);
        assert_eq!(all.len(), 6);
        assert_eq!((&all[0].version, &all[1].version), (&newer, &older));
        assert_eq!(all[0].machine, "desk");
        assert_eq!(all[1].machine, "phone");
        assert_eq!(all[0].change, "save");
        assert_eq!(all[0].row.as_ref().unwrap().label, "lantern/relay-pin");
        assert_eq!(all[0].label, "lantern/relay-pin");
        assert_eq!(all[1].row, all[0].row);
        let tied: Vec<&VersionId> = all[2..].iter().map(|entry| &entry.version).collect();
        let mut sorted = tied.clone();
        sorted.sort();
        assert_eq!(tied, sorted);
        assert_eq!(logged(&world, 2, None).len(), 2);
        assert_eq!(logged(&world, 2, None)[1].version, older);
        assert!(logged(&world, 0, None).is_empty());
    }

    #[test]
    fn the_log_shows_a_machine_that_is_no_topic_as_its_short_id() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let absent = DocumentId::from_bytes([0xb8; 16]);
        let older = rewritten(&world, &lantern, "2026-10-10T09:00:00+00:00", &relay);
        let newer = rewritten(&world, &lantern, "2026-10-11T09:00:00+00:00", &absent);
        let all = logged(&world, 2, None);
        assert_eq!((&all[0].version, &all[1].version), (&newer, &older));
        assert_eq!(all[0].machine, absent.short());
        assert_eq!(all[1].machine, relay.short());
    }

    #[test]
    fn the_log_keeps_the_versions_whose_stamp_carries_the_machine() {
        let (world, lantern) = world();
        let phone = topic(&world, "phone", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let older = rewritten(&world, &relay, "2026-10-10T09:00:00+00:00", &phone);
        let only = logged(&world, usize::MAX, Some("phone"));
        let ids: Vec<&VersionId> = only.iter().map(|entry| &entry.version).collect();
        assert!(ids.contains(&&older));
        assert!(only.iter().all(|entry| entry.machine == "phone"));
        let refused = log(&world.deps(), 5, Some("nowhere"));
        assert!(matches!(refused, Err(Failure::Refused(_))), "{refused:?}");
    }

    #[test]
    fn the_log_lists_both_heads_of_a_fork() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let fields = format!(
            "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
             confirmed = 2026-09-04\nsummary = \"s\"\n"
        );
        let heads = fork(&world, &relay, &fields);
        let mine: Vec<Logged> = logged(&world, usize::MAX, None)
            .into_iter()
            .filter(|entry| entry.row.as_ref().is_some_and(|row| row.document == relay))
            .collect();
        assert_eq!(mine.len(), 3);
        for head in &heads {
            assert!(mine.iter().any(|entry| &entry.version == head));
        }
        assert!(mine.iter().all(|entry| entry.row.as_ref().unwrap().forked));
    }

    #[test]
    fn a_whole_store_log_asks_one_question_per_kind() {
        use crate::app::testing::Counting;
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let counting = Counting::over(world.store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &world.drafts,
            ids: &world.ids,
            clock: &world.clock,
            host: &world.host,
        };
        log(&deps, 10, None).unwrap();
        assert_eq!(counting.kinds.get(), KindOf::ALL.len());
        assert_eq!(counting.holdings.get(), 0);
        assert_eq!(counting.documents.get(), 0);
    }

    #[test]
    fn a_line_is_matched_as_written_and_trimmed_in_the_result() {
        let (world, lantern) = world();
        fact_with(&world, &lantern, "driver", "s", "  pin moved\n");
        let hits = find(&world, " pin");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].lines, ["pin moved"]);
    }

    #[test]
    fn a_fork_with_an_ended_head_is_searched_in_its_unended_one() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let fields = |rest: &str| {
            format!(
                "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n{rest}"
            )
        };
        world
            .store
            .put(&crate::domain::testing::after(
                &[&root],
                &fields(""),
                "open side\n",
            ))
            .unwrap();
        world
            .store
            .put(&crate::domain::testing::after(
                &[&root],
                &fields(ENDED),
                "ended side\n",
            ))
            .unwrap();
        let hits = find(&world, "side");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].lines, ["open side"]);
        assert!(hits[0].row.forked);
        assert!(find(&world, "ended side").is_empty());
    }

    #[test]
    fn a_topic_narrows_entries_and_followups_filed_on_a_part() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &format!("part_of = [\"{lantern}\"]\n"));
        let phone = topic(&world, "phone", "");
        let desk = world.machine();
        let entry = |topic: &DocumentId, name: &str| {
            world.put(
                "entry",
                &format!(
                    "name = \"{name}\"\ndate = 2026-10-08\nmachine = \"{desk}\"\n\
                     topics = [\"{topic}\"]\nsummary = \"zebra\"\n"
                ),
                "\n",
            )
        };
        let followup = |topic: &DocumentId| {
            world.put(
                "followup",
                &format!("created = 2026-09-04\ntopics = [\"{topic}\"]\nsummary = \"zebra\"\n"),
                "\n",
            )
        };
        entry(&atlas, "on-part");
        entry(&phone, "elsewhere");
        let mine = followup(&atlas);
        let other = followup(&phone);
        assert_ne!(mine.short(), other.short());
        let hits = query(&world, "zebra", false, Some("lantern"), false);
        assert_eq!(
            labels(&hits, |hit| &hit.row),
            ["2026-10-08-on-part", mine.short()]
        );
        assert_eq!(hits[1].row.document, mine);
    }

    #[test]
    fn the_log_lists_the_versions_of_a_document_whose_head_does_not_read() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let first = head(&world, &relay).id;
        let second = world.amend(&relay, "name = \"relay-pin\"\n", "\n");
        let mine: Vec<Logged> = logged(&world, usize::MAX, None)
            .into_iter()
            .filter(|entry| entry.version == first || entry.version == second)
            .collect();
        assert_eq!(mine.len(), 2);
        assert!(mine.iter().all(|entry| entry.row.is_none()));
        assert!(mine.iter().all(|entry| entry.label == relay.short()));
        assert!(mine.iter().all(|entry| entry.ended.is_none()));
    }

    #[test]
    fn the_log_lists_the_versions_of_a_kind_this_worklog_does_not_know() {
        use crate::app::testing::Counting;
        let (world, _) = world();
        let sketch = world.put("sketch", "name = \"one\"\n", "\n");
        let first = head(&world, &sketch).id;
        let second = world.amend(&sketch, "name = \"two\"\n", "\n");
        let counting = Counting::over(world.store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &world.drafts,
            ids: &world.ids,
            clock: &world.clock,
            host: &world.host,
        };
        let all = log(&deps, usize::MAX, None).unwrap();
        assert_eq!(all.len(), 4);
        let mine: Vec<&Logged> = all
            .iter()
            .filter(|entry| entry.version == first || entry.version == second)
            .collect();
        assert_eq!(mine.len(), 2);
        assert!(mine.iter().all(|entry| entry.row.is_none()));
        assert!(mine.iter().all(|entry| entry.machine == "desk"));
        assert_eq!(counting.kind_lists.get(), 1);
        assert_eq!(counting.kinds.get(), KindOf::ALL.len() + 1);
    }

    #[test]
    fn the_log_lists_the_versions_of_an_ended_document() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let first = head(&world, &relay).id;
        let fields = format!(
            "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
             confirmed = 2026-09-04\nsummary = \"s\"\n{ENDED}"
        );
        let second = world.amend(&relay, &fields, "\n");
        let mine: Vec<Logged> = logged(&world, usize::MAX, None)
            .into_iter()
            .filter(|entry| entry.version == first || entry.version == second)
            .collect();
        assert_eq!(mine.len(), 2);
        assert!(
            mine.iter()
                .all(|entry| { entry.row.as_ref().unwrap().ended.as_deref() == Some("retired") })
        );
        for entry in &mine {
            let ended = (entry.version == second).then_some("retired");
            assert_eq!(entry.ended.as_deref(), ended);
        }
    }
}
