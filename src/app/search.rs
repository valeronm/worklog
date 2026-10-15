use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::ops::Range;

use regex::{Regex, RegexBuilder};
use serde::Serialize;

use crate::app::heads::{ended_of, label_or_short};
use crate::app::rows::{Reference, Row, by_label, documents, reading, row, shown};
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::schema::KindOf;
use crate::domain::version::{Stamp, Version};

const SEARCHED: [KindOf; 4] = [KindOf::Topic, KindOf::Fact, KindOf::Entry, KindOf::Followup];

pub struct Query<'a> {
    pub term: &'a str,
    pub regex: bool,
    pub topic: Option<&'a str>,
    pub ended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub row: Row,
    pub lines: Vec<Matched>,
    /// Where the term matched `row.label`.
    pub label_spans: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Matched {
    pub text: String,
    /// The byte ranges of `text` the term matched, in order, apart and none of them empty.
    pub spans: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Logged {
    pub document: DocumentId,
    /// The label of `row`, or the start of the document's id when there is none.
    pub label: String,
    pub version: VersionId,
    pub written: Stamp,
    pub machine: Reference,
    pub change: String,
    /// The reason; `None` too for a version that does not read as a record.
    pub ended: Option<String>,
    pub row: Option<Row>,
}

/// Topics first, then facts and ideas, entries, followups; within a kind newest row head
/// first, ties by label.
///
/// A document matches by its label, its summary or a body line; the term ignores case and,
/// unless `regex`, is matched as plain text. `lines` are the matching summary and body
/// lines as written, trimmed, each with where the term matched it. An empty term and a
/// pattern that does not compile are usage failures; an address that is not one topic is
/// refused. A claim is never searched, and a forked document is searched in its row head
/// only.
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
            let lines: Vec<Matched> = std::iter::once(row.summary.as_str())
                .chain(shown.head.body.lines())
                .filter_map(|line| {
                    let found: Vec<Range<usize>> =
                        matcher.find_iter(line).map(|found| found.range()).collect();
                    (!found.is_empty()).then(|| trimmed(line, &found))
                })
                .collect();
            if !lines.is_empty() || matcher.is_match(&row.label) {
                let label_spans = spans(&matcher, &row.label);
                let written = shown.head.envelope.written.instant();
                found.push((
                    written,
                    Hit {
                        row,
                        lines,
                        label_spans,
                    },
                ));
            }
        }
        by_label(&mut found, |(_, hit)| &hit.row);
        found.sort_by_key(|(written, _)| Reverse(*written));
        hits.extend(found.into_iter().map(|(_, hit)| hit));
    }
    Ok(hits)
}

// A pattern can match an empty stretch, which marks nothing.
fn spans(matcher: &Regex, text: &str) -> Vec<Range<usize>> {
    (matcher.find_iter(text))
        .map(|found| found.range())
        .filter(|span| !span.is_empty())
        .collect()
}

// A line is matched as written, so an anchored pattern holds at the line's own edges.
fn trimmed(line: &str, spans: &[Range<usize>]) -> Matched {
    let text = line.trim();
    let start = line.len() - line.trim_start().len();
    let end = start + text.len();
    Matched {
        text: text.to_owned(),
        spans: (spans.iter())
            .map(|span| span.start.clamp(start, end) - start..span.end.clamp(start, end) - start)
            .filter(|span| span.start < span.end)
            .collect(),
    }
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
            machine: topics.reference(&version.envelope.machine),
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
    use crate::app::testing::{
        ENDED, World, fact, fact_fields, fork, head, labels, to, topic, world, written_at,
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

    fn texts(hit: &Hit) -> Vec<&str> {
        hit.lines.iter().map(|line| line.text.as_str()).collect()
    }

    fn bounds(spans: &[Range<usize>]) -> Vec<(usize, usize)> {
        spans.iter().map(|span| (span.start, span.end)).collect()
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
            texts(&hits[0]),
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
    fn a_pattern_too_large_to_compile_is_a_usage_failure_and_a_wide_class_repeated_is_not() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let deps = world.deps();
        let asked = |term: &str| {
            search(
                &deps,
                &Query {
                    term,
                    regex: true,
                    topic: None,
                    ended: false,
                },
            )
        };
        let Err(Failure::Usage(text)) = asked("[a-z]{60000}") else {
            panic!("expected a usage failure");
        };
        assert!(text.starts_with("bad pattern: "), "{text}");
        assert!(text.contains("size limit"), "{text}");
        assert_eq!(asked(r"\w{40}"), Ok(Vec::new()));
        assert_eq!(asked(r"\pL{50}"), Ok(Vec::new()));
        assert_eq!(asked(r"^the \pL{5}\.$").unwrap().len(), 1);
        assert_eq!(asked(r"(?:rel|del)ay-p[a-z]{2}\b").unwrap().len(), 1);
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
    fn hits_come_topics_first_then_facts_entries_followups_ties_by_label() {
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
                "atlas",
                "lantern",
                "atlas/a-idea",
                "lantern/b-fact",
                "2026-10-08-lamp-driver",
                followup.short()
            ]
        );
    }

    #[test]
    fn hits_of_a_kind_come_newest_row_head_first() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        fact(&world, &lantern, "lamp-pin", "");
        rewritten(
            &world,
            &relay,
            "2026-10-11T09:00:00+00:00",
            &world.machine(),
        );
        let hits = find(&world, "pin");
        assert_eq!(
            labels(&hits, |hit| &hit.row),
            ["lantern/relay-pin", "lantern/lamp-pin"]
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
            ["atlas", "lantern", "atlas/part", "lantern/own"]
        );
        let hits = query(&world, ".", true, Some("atlas"), false);
        assert_eq!(labels(&hits, |hit| &hit.row), ["atlas", "atlas/part"]);
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
        assert_eq!(hits[0].lines[0].text, shown);
    }

    fn head_body(world: &World, id: &DocumentId) -> String {
        let held = world.store.document(id).unwrap();
        let (version, _) = row_head(&held).unwrap();
        version.body.trim().to_owned()
    }

    #[test]
    fn a_whole_store_search_asks_one_question_per_kind() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let world = world.counted();
        let deps = world.deps();
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
        assert_eq!(world.store.kinds.get(), KindOf::ALL.len() - 1);
        assert_eq!(world.store.documents.get(), 0);
        assert_eq!(world.store.holdings.get(), 0);
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
        assert_eq!(all[0].machine, to(&world.machine(), "desk"));
        assert_eq!(all[1].machine.label, "phone");
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
        assert_eq!(all[0].machine, to(&absent, absent.short()));
        assert_eq!(all[1].machine, to(&relay, relay.short()));
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
        assert!(only.iter().all(|entry| entry.machine.label == "phone"));
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
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let world = world.counted();
        let deps = world.deps();
        log(&deps, 10, None).unwrap();
        assert_eq!(world.store.kinds.get(), KindOf::ALL.len());
        assert_eq!(world.store.holdings.get(), 0);
        assert_eq!(world.store.documents.get(), 0);
    }

    #[test]
    fn a_line_is_matched_as_written_and_trimmed_in_the_result() {
        let (world, lantern) = world();
        fact_with(&world, &lantern, "driver", "s", "  pin moved\n");
        let hits = find(&world, " pin");
        assert_eq!(hits.len(), 1);
        assert_eq!(texts(&hits[0]), ["pin moved"]);
        assert_eq!(bounds(&hits[0].lines[0].spans), [(0, 3)]);
    }

    #[test]
    fn a_hit_holds_where_the_term_matched_its_label_and_each_line() {
        let (world, lantern) = world();
        fact_with(&world, &lantern, "pin", "Pin and PIN", "  a pin\nnothing\n");
        let hits = find(&world, "pin");
        assert_eq!(bounds(&hits[0].label_spans), [(8, 11)]);
        let spans: Vec<_> = (hits[0].lines.iter())
            .map(|line| bounds(&line.spans))
            .collect();
        assert_eq!(spans, [vec![(0, 3), (8, 11)], vec![(2, 5)]]);
    }

    #[test]
    fn a_pattern_marks_none_of_its_empty_matches() {
        let (world, lantern) = world();
        fact_with(&world, &lantern, "driver", "s", "a pin\n");
        let hits = query(&world, "pin|x*", true, None, false);
        let driver = hits
            .iter()
            .find(|hit| hit.row.kind == KindOf::Fact)
            .unwrap();
        assert_eq!(bounds(&driver.lines.last().unwrap().spans), [(2, 5)]);
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
        assert_eq!(texts(&hits[0]), ["open side"]);
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
        let (world, _) = world();
        let sketch = world.put("sketch", "name = \"one\"\n", "\n");
        let first = head(&world, &sketch).id;
        let second = world.amend(&sketch, "name = \"two\"\n", "\n");
        let world = world.counted();
        let deps = world.deps();
        let all = log(&deps, usize::MAX, None).unwrap();
        assert_eq!(all.len(), 4);
        let mine: Vec<&Logged> = all
            .iter()
            .filter(|entry| entry.version == first || entry.version == second)
            .collect();
        assert_eq!(mine.len(), 2);
        assert!(mine.iter().all(|entry| entry.row.is_none()));
        assert!(mine.iter().all(|entry| entry.machine.label == "desk"));
        assert_eq!(world.store.kind_lists.get(), 1);
        assert_eq!(world.store.kinds.get(), KindOf::ALL.len() + 1);
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
