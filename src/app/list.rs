use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::rc::Rc;

use serde::Serialize;

use crate::app::heads::{ended_of, is_forked, label_or_short};
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::rows::{
    Filings, FollowupRow, Reference, Row, by_label, documents, followup_rows, reading, row, shown,
};
use crate::app::rules::{claims_of, claims_placing};
use crate::app::{Failure, ReadDeps, day_given};
use crate::domain::document::Document;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::schema::{Claim, Content, Date, KindOf};
use crate::domain::version::Stamp;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TopicRow {
    pub row: Row,
    pub part_of: Vec<Reference>,
    pub uses: Vec<Reference>,
    /// The topics directly part of this one, none of them ended, in the order `topics` lists
    /// its rows.
    pub parts: Vec<Reference>,
    /// Whether a walk through `parts` starts here: true for a topic that is part of no
    /// unended topic, and for the first topic, in the order `topics` lists its rows, of a
    /// ring of topics part of each other no member of which is part of a topic outside it;
    /// every unended topic is reached from a `top` one.
    pub top: bool,
    /// The documents filed on the topic itself, not on its parts, with a head that is not
    /// ended, as `idea_count` and `open_count` are; an idea is not counted as a fact.
    pub fact_count: usize,
    pub idea_count: usize,
    pub open_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FactRow {
    pub row: Row,
    pub confirmed: Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryRow {
    pub row: Row,
    pub date: Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClaimRow {
    pub document: DocumentId,
    pub machine: Reference,
    pub topic: Reference,
    pub directory: Option<String>,
    /// Whether the directory is on this host; `None` for a claim with no directory, one of
    /// another machine, and one under `~` on a host with no home.
    pub exists: Option<bool>,
    pub forked: bool,
}

impl ClaimRow {
    /// The directory, when it is not on this host.
    #[must_use]
    pub fn missing(&self) -> Option<&str> {
        match (self.exists, &self.directory) {
            (Some(false), Some(directory)) => Some(directory),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForkHead {
    pub version: VersionId,
    pub written: Stamp,
    pub machine: Reference,
    pub ended: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForkRow {
    pub document: DocumentId,
    /// The label of `row`, or the start of the document's id when there is none.
    pub label: String,
    /// None when no head reads as a record.
    pub row: Option<Row>,
    /// In head order; one that does not read as a record is not ended.
    pub heads: Vec<ForkHead>,
}

/// By name; the ended ones only when `ended`. With `of`, only the topics directly part of
/// that topic, and an address that is not one topic is refused. `ended` has no effect beside
/// `of`, since an ended topic is part of nothing.
pub fn topics(deps: &ReadDeps, of: Option<&str>, ended: bool) -> Result<Vec<TopicRow>, Failure> {
    let (lookup, topics, of) = reading(deps, of)?;
    let documents = lookup.of_kind(KindOf::Topic)?;
    let mut shown = shown(&topics, &documents, true);
    let mut order: Vec<&Row> = shown.iter().map(|shown| &shown.row).collect();
    by_label(&mut order, |row| *row);
    let order: Vec<DocumentId> = order.into_iter().map(|row| row.document.clone()).collect();
    let tops = topics.tops(&order);
    let place = |id: &DocumentId| order.iter().position(|listed| listed == id);
    let of_parts = of.as_ref().map(|of| topics.direct_parts(of));
    shown.retain(|shown| {
        (ended || shown.row.ended.is_none())
            && (of_parts.as_ref()).is_none_or(|parts| parts.contains(&shown.row.document))
    });
    let listed: BTreeSet<DocumentId> = (shown.iter())
        .map(|shown| shown.row.document.clone())
        .collect();
    let filings = Filings::of(&lookup, &listed)?;
    let mut rows: Vec<TopicRow> = shown
        .into_iter()
        .map(|shown| {
            let own = BTreeSet::from([shown.row.document.clone()]);
            let counts = filings.counts(&own);
            let mut parts = topics.direct_parts(&shown.row.document);
            parts.sort_by_key(place);
            TopicRow {
                part_of: topics.references(topics.part_of(&shown.row.document)),
                uses: topics.references(topics.uses(&shown.row.document)),
                parts: topics.references(&parts),
                top: tops.contains(&shown.row.document),
                row: shown.row,
                fact_count: counts.facts,
                idea_count: counts.ideas,
                open_count: counts.open,
            }
        })
        .collect();
    by_label(&mut rows, |shown| &shown.row);
    Ok(rows)
}

/// Refuses an address that is not one topic. With a topic the facts of its parts are
/// included; by label.
pub fn facts(deps: &ReadDeps, topic: Option<&str>, ended: bool) -> Result<Vec<FactRow>, Failure> {
    fact_rows(deps, topic, ended, false)
}

/// Like `facts`, for the facts flagged `idea`.
pub fn ideas(deps: &ReadDeps, topic: Option<&str>, ended: bool) -> Result<Vec<FactRow>, Failure> {
    fact_rows(deps, topic, ended, true)
}

fn fact_rows(
    deps: &ReadDeps,
    topic: Option<&str>,
    ended: bool,
    ideas: bool,
) -> Result<Vec<FactRow>, Failure> {
    let (lookup, topics, topic) = reading(deps, topic)?;
    let documents = documents(&lookup, &topics, KindOf::Fact, topic.as_ref(), ended)?;
    let mut rows: Vec<FactRow> = shown(&topics, &documents, ended)
        .into_iter()
        .filter_map(|shown| match shown.record.content {
            Content::Fact(fact) if fact.idea == ideas => Some(FactRow {
                row: shown.row,
                confirmed: fact.confirmed,
            }),
            _ => None,
        })
        .collect();
    by_label(&mut rows, |shown| &shown.row);
    Ok(rows)
}

/// Newest date first, then by name; `limit` cuts after the ordering. Refuses an address that
/// is not one topic, and includes the entries of its parts. `on` is `YYYY-MM-DD` and keeps
/// the entries dated that day; anything else is a usage failure.
pub fn entries(
    deps: &ReadDeps,
    topic: Option<&str>,
    on: Option<&str>,
    limit: Option<usize>,
    ended: bool,
) -> Result<Vec<EntryRow>, Failure> {
    let on = on.map(day_given).transpose()?;
    let (lookup, topics, topic) = reading(deps, topic)?;
    let documents = documents(&lookup, &topics, KindOf::Entry, topic.as_ref(), ended)?;
    let mut rows: Vec<EntryRow> = shown(&topics, &documents, ended)
        .into_iter()
        .filter_map(|shown| match shown.record.content {
            Content::Entry(entry) if on.is_none_or(|on| on == entry.date) => Some(EntryRow {
                row: shown.row,
                date: entry.date,
            }),
            _ => None,
        })
        .collect();
    rows.sort_by(|a, b| (Reverse(a.date), &a.row.label).cmp(&(Reverse(b.date), &b.row.label)));
    rows.truncate(limit.unwrap_or(usize::MAX));
    Ok(rows)
}

/// `about` names a topic, for the rollup over its parts, or an entry, for the followups that
/// name it; anything else is refused. Due ones come first by date, then the other dated ones,
/// then those waiting on a topic, then those with no trigger.
pub fn followups(
    deps: &ReadDeps,
    about: Option<&str>,
    ended: bool,
) -> Result<Vec<FollowupRow>, Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let today = deps.today()?;
    let documents = match about {
        None => documents(&lookup, &topics, KindOf::Followup, None, ended)?,
        Some(address) => {
            let id = lookup.one(address)?;
            if lookup.is_kind(&id, KindOf::Topic)? {
                documents(&lookup, &topics, KindOf::Followup, Some(&id), ended)?
            } else if lookup.is_kind(&id, KindOf::Entry)? {
                naming_entry(&lookup, &id, ended)?
            } else {
                return Err(Failure::at(address, "is neither a topic nor an entry"));
            }
        }
    };
    let shown = shown(&topics, &documents, ended);
    followup_rows(&lookup, &topics, shown, today, &BTreeSet::new())
}

fn naming_entry(
    lookup: &Lookup,
    entry: &DocumentId,
    ended: bool,
) -> Result<Vec<Rc<Document>>, Failure> {
    let Some(key) = KindOf::Followup.key_where(|reference| reference.target == Some(KindOf::Entry))
    else {
        return Ok(Vec::new());
    };
    lookup.holders(KindOf::Followup, key, &[entry], ended)
}

/// The unended claims of this host's machine topic, or of the machine topic named; `topic`
/// narrows to one topic with no rollup. By topic label and id, then directory, a claim with
/// no directory first.
pub fn where_(
    deps: &ReadDeps,
    topic: Option<&str>,
    machine: Option<&str>,
) -> Result<Vec<ClaimRow>, Failure> {
    let lookup = Lookup::new(deps.store);
    let machine = match machine {
        Some(address) => lookup.one_topic(address)?,
        None => deps.machine()?,
    };
    let topic = topic.map(|address| lookup.one_topic(address)).transpose()?;
    let mut claims = claims_of(&lookup, &machine)?;
    claims.retain(|(_, claim)| topic.as_ref().is_none_or(|topic| *topic == claim.topic));
    let mut rows = claim_rows(deps, &lookup, claims)?;
    by_label_of(&mut rows, |row| &row.topic);
    Ok(rows)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopicClaims {
    /// The claims of the topic as a machine, ordered as `where_` orders them.
    pub held: Vec<ClaimRow>,
    /// The claims placing the topic on another machine, by the machine's label and id, then
    /// directory, a claim with no directory first.
    pub placing: Vec<ClaimRow>,
}

/// The unended claims naming `topic`, as the machine they belong to or as the topic they place.
pub fn claims_on(deps: &ReadDeps, topic: &DocumentId) -> Result<TopicClaims, Failure> {
    let lookup = Lookup::new(deps.store);
    let mut held = claim_rows(deps, &lookup, claims_of(&lookup, topic)?)?;
    by_label_of(&mut held, |row| &row.topic);
    let mut placing = claims_placing(&lookup, topic)?;
    placing.retain(|(_, claim)| claim.machine != *topic);
    let mut placing = claim_rows(deps, &lookup, placing)?;
    by_label_of(&mut placing, |row| &row.machine);
    Ok(TopicClaims { held, placing })
}

fn by_label_of(rows: &mut [ClaimRow], side: fn(&ClaimRow) -> &Reference) {
    let key = |row: &ClaimRow| {
        let side = side(row);
        (
            side.label.clone(),
            side.document.clone(),
            row.directory.clone(),
            row.document.clone(),
        )
    };
    rows.sort_by_key(key);
}

fn claim_rows(
    deps: &ReadDeps,
    lookup: &Lookup,
    claims: Vec<(Rc<Document>, Claim)>,
) -> Result<Vec<ClaimRow>, Failure> {
    let here = (deps.host.machine()?)
        .filter(|here| claims.iter().any(|(_, claim)| claim.machine == *here));
    // The home of this host's machine, whose claims alone have directories this host can check.
    let here = match here {
        Some(machine) => Some((machine, deps.home()?)),
        None => None,
    };
    let mut rows = Vec::new();
    for (document, claim) in claims {
        let home = (here.as_ref())
            .filter(|(machine, _)| *machine == claim.machine)
            .map(|(_, home)| home);
        let exists = match (home, &claim.directory) {
            (Some(home), Some(directory)) => (directory.from_root(home.as_deref()))
                .map(|path| deps.host.is_directory(&path))
                .transpose()?,
            _ => None,
        };
        rows.push(ClaimRow {
            exists,
            document: document.id().clone(),
            machine: lookup.topic_reference(&claim.machine)?,
            topic: lookup.topic_reference(&claim.topic)?,
            directory: claim.directory.map(|directory| directory.to_string()),
            forked: is_forked(&document),
        });
    }
    Ok(rows)
}

/// Every document with more than one head, of any kind this worklog knows, ended ones
/// included; by label, one with no row by its short id.
pub fn forks(deps: &ReadDeps) -> Result<Vec<ForkRow>, Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let mut rows = Vec::new();
    for document in lookup.forks()? {
        let heads = document
            .heads()
            .into_iter()
            .map(|head| ForkHead {
                version: head.id.clone(),
                written: head.envelope.written.clone(),
                machine: topics.reference(&head.envelope.machine),
                ended: ended_of(head),
            })
            .collect();
        let row = row(&topics, &document).map(|shown| shown.row);
        let label = row.as_ref().map(|row| row.label.clone());
        rows.push(ForkRow {
            document: document.id().clone(),
            label: label_or_short(label, document.id()),
            row,
            heads,
        });
    }
    rows.sort_by(|a, b| (&a.label, &a.document).cmp(&(&b.label, &b.document)));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::rows::TriggerShown;
    use crate::app::testing::{
        ENDED, ENDED_FACT, World, claim_fields, entry, fact, fact_fields, followup, head, labels,
        part_of, referred, refused, to, topic, topic_fields, world, written_at,
    };
    use crate::domain::id::VersionId;
    use crate::domain::ports::{Store, StoreRead};
    use crate::domain::testing::first;
    use crate::domain::version::Version;

    const DATED: &str = "look_again = 2026-10-20\nwhy = \"w\"\n";

    fn sibling(world: &World, root: &Version, fields: &str, stamp: &str) -> Version {
        written_at(world, root, fields, stamp, &(stamp.to_owned() + "\n"), None)
    }

    fn claim(
        world: &World,
        machine: &DocumentId,
        topic: &DocumentId,
        directory: &str,
    ) -> DocumentId {
        world.put("claim", &claim_fields(machine, topic, directory), "\n")
    }

    #[test]
    fn topics_are_listed_by_name_with_their_parts_and_uses() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", &format!("uses = [\"{atlas}\"]\n"));
        world.amend(
            &phone,
            &format!(
                "name = \"phone\"\ncreated = 2026-09-04\nsummary = \"s\"\nuses = [\"{atlas}\"]\n\
                 ended = \"retired\"\nended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "\n",
        );
        let deps = world.deps();

        let shown = topics(&deps.read(), None, false).unwrap();
        assert_eq!(labels(&shown, |t| &t.row), ["atlas", "desk", "lantern"]);
        assert_eq!(shown[0].part_of, [to(&lantern, "lantern")]);
        assert_eq!(shown[0].uses, []);

        let all = topics(&deps.read(), None, true).unwrap();
        assert_eq!(
            labels(&all, |t| &t.row),
            ["atlas", "desk", "lantern", "phone"]
        );
        assert_eq!(all[3].uses, []);
        assert_eq!(all[3].row.ended.as_deref(), Some("retired"));
    }

    fn counts(rows: &[TopicRow]) -> Vec<(&str, usize, usize, usize)> {
        rows.iter()
            .map(|topic| {
                (
                    topic.row.label.as_str(),
                    topic.fact_count,
                    topic.idea_count,
                    topic.open_count,
                )
            })
            .collect()
    }

    #[test]
    fn a_topic_row_counts_what_is_filed_on_the_topic_itself_and_not_ended() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        fact(&world, &lantern, "pin", "");
        fact(&world, &lantern, "bulb", "");
        fact(&world, &lantern, "dimmer", "idea = true\n");
        fact(&world, &lantern, "wick", ENDED_FACT);
        fact(&world, &atlas, "map", "");
        followup(&world, &[&lantern], "open", "");
        followup(&world, &[&lantern, &atlas], "both", DATED);
        followup(
            &world,
            &[&atlas],
            "due",
            "look_again = 2026-10-01\nwhy = \"w\"\n",
        );
        let done = followup(&world, &[&lantern], "done", "");
        world.amend(
            &done,
            &format!(
                "created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"done\"\n\
                 ended = \"done\"\nended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "\n",
        );

        let shown = topics(&world.deps().read(), None, false).unwrap();
        assert_eq!(
            counts(&shown),
            [("atlas", 1, 0, 2), ("desk", 0, 0, 0), ("lantern", 2, 1, 2)]
        );
    }

    #[test]
    fn a_forked_fact_is_counted_once_for_each_topic_an_unended_head_files_it_on() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", "");
        let phone = topic(&world, "phone", "");
        let id = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &id);
        sibling(
            &world,
            &root,
            &fact_fields(&atlas, "relay-pin", "s", ""),
            "2026-10-09T10:00:00+01:00",
        );
        sibling(
            &world,
            &root,
            &fact_fields(&atlas, "relay-pin", "t", ""),
            "2026-10-09T10:30:00+01:00",
        );
        sibling(
            &world,
            &root,
            &fact_fields(&phone, "relay-pin", "s", "idea = true\n"),
            "2026-10-09T09:00:00+01:00",
        );
        sibling(
            &world,
            &root,
            &fact_fields(&lantern, "relay-pin", "s", ENDED_FACT),
            "2026-10-09T11:00:00+01:00",
        );

        let shown = topics(&world.deps().read(), None, false).unwrap();
        assert_eq!(
            counts(&shown),
            [
                ("atlas", 1, 0, 0),
                ("desk", 0, 0, 0),
                ("lantern", 0, 0, 0),
                ("phone", 1, 0, 0)
            ]
        );
    }

    #[test]
    fn an_ended_topic_is_counted_as_any_other_when_it_is_listed() {
        let (world, lantern) = world();
        fact(&world, &lantern, "pin", "");
        followup(&world, &[&lantern], "open", "");
        world.amend(&lantern, &topic_fields("lantern", ENDED), "\n");
        let deps = world.deps();
        assert_eq!(
            counts(&topics(&deps.read(), None, false).unwrap()),
            [("desk", 0, 0, 0)]
        );
        assert_eq!(
            counts(&topics(&deps.read(), None, true).unwrap()),
            [("desk", 0, 0, 0), ("lantern", 1, 0, 1)]
        );
    }

    fn tree(rows: &[TopicRow]) -> Vec<(&str, bool, Vec<&str>)> {
        rows.iter()
            .map(|topic| {
                let parts = topic.parts.iter().map(|part| part.label.as_str());
                (topic.row.label.as_str(), topic.top, parts.collect())
            })
            .collect()
    }

    #[test]
    fn a_chain_of_parts_has_its_first_topic_at_the_top() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", &part_of(&[&atlas]));

        let shown = topics(&world.deps().read(), None, false).unwrap();
        assert_eq!(
            tree(&shown),
            [
                ("atlas", false, vec!["phone"]),
                ("desk", true, vec![]),
                ("lantern", true, vec!["atlas"]),
                ("phone", false, vec![]),
            ]
        );
        assert_eq!(shown[0].parts[0].document, phone);
        assert_eq!(
            tree(&topics(&world.deps().read(), Some("lantern"), false).unwrap()),
            [("atlas", false, vec!["phone"])]
        );
    }

    #[test]
    fn a_topic_under_two_topics_is_a_part_of_each_in_the_order_of_the_listing() {
        let (world, lantern) = world();
        let phone = topic(&world, "phone", &part_of(&[&lantern]));
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        topic(&world, "relay", &part_of(&[&phone, &atlas]));

        assert_eq!(
            tree(&topics(&world.deps().read(), None, false).unwrap()),
            [
                ("atlas", false, vec!["relay"]),
                ("desk", true, vec![]),
                ("lantern", true, vec!["atlas", "phone"]),
                ("phone", false, vec!["relay"]),
                ("relay", false, vec![]),
            ]
        );
    }

    #[test]
    fn a_ring_with_no_topic_above_it_has_its_first_listed_topic_at_the_top() {
        let (world, lantern) = world();
        let phone = topic(&world, "phone", "");
        let atlas = topic(&world, "atlas", &part_of(&[&phone]));
        let relay = topic(&world, "relay", &part_of(&[&atlas]));
        world.amend(&phone, &topic_fields("phone", &part_of(&[&relay])), "\n");
        topic(&world, "compass", &part_of(&[&relay]));
        let deps = world.deps();

        assert_eq!(
            tree(&topics(&deps.read(), None, false).unwrap()),
            [
                ("atlas", true, vec!["relay"]),
                ("compass", false, vec![]),
                ("desk", true, vec![]),
                ("lantern", true, vec![]),
                ("phone", false, vec!["atlas"]),
                ("relay", false, vec!["compass", "phone"]),
            ]
        );

        world.amend(
            &phone,
            &topic_fields("phone", &part_of(&[&relay, &lantern])),
            "\n",
        );
        assert_eq!(
            tree(&topics(&deps.read(), None, false).unwrap()),
            [
                ("atlas", false, vec!["relay"]),
                ("compass", false, vec![]),
                ("desk", true, vec![]),
                ("lantern", true, vec!["phone"]),
                ("phone", false, vec!["atlas"]),
                ("relay", false, vec!["compass", "phone"]),
            ]
        );
    }

    #[test]
    fn a_ring_under_another_ring_is_reached_from_the_ring_above_it() {
        let (world, _) = world();
        let relay = topic(&world, "relay", "");
        let phone = topic(&world, "phone", &part_of(&[&relay]));
        world.amend(&relay, &topic_fields("relay", &part_of(&[&phone])), "\n");
        let atlas = topic(&world, "atlas", &part_of(&[&phone]));
        let compass = topic(&world, "compass", &part_of(&[&atlas]));
        world.amend(
            &atlas,
            &topic_fields("atlas", &part_of(&[&phone, &compass])),
            "\n",
        );
        let own = topic(&world, "globe", "");
        world.amend(&own, &topic_fields("globe", &part_of(&[&own])), "\n");

        assert_eq!(
            tree(&topics(&world.deps().read(), None, false).unwrap()),
            [
                ("atlas", false, vec!["compass"]),
                ("compass", false, vec!["atlas"]),
                ("desk", true, vec![]),
                ("globe", true, vec!["globe"]),
                ("lantern", true, vec![]),
                ("phone", true, vec!["atlas", "relay"]),
                ("relay", false, vec!["phone"]),
            ]
        );
    }

    #[test]
    fn the_part_of_an_ended_topic_is_at_the_top() {
        let (world, lantern) = world();
        topic(&world, "atlas", &part_of(&[&lantern]));
        world.amend(&lantern, &topic_fields("lantern", ENDED), "\n");
        let deps = world.deps();

        let shown = topics(&deps.read(), None, false).unwrap();
        assert_eq!(
            tree(&shown),
            [("atlas", true, vec![]), ("desk", true, vec![])]
        );
        assert_eq!(shown[0].part_of[0].document, lantern);
        assert_eq!(
            tree(&topics(&deps.read(), None, true).unwrap()),
            [
                ("atlas", true, vec![]),
                ("desk", true, vec![]),
                ("lantern", true, vec!["atlas"]),
            ]
        );
    }

    #[test]
    fn two_topics_sharing_a_name_each_keep_their_own_parts() {
        let (world, _) = world();
        let first = topic(&world, "atlas", "");
        let second = topic(&world, "atlas", "");
        let phone = topic(&world, "phone", &part_of(&[&second]));
        let relay = topic(&world, "relay", &part_of(&[&first]));

        let shown = topics(&world.deps().read(), None, false).unwrap();
        assert_eq!(
            tree(&shown)[..2],
            [
                ("atlas", true, vec!["relay"]),
                ("atlas", true, vec!["phone"])
            ]
        );
        assert_eq!(
            (&shown[0].row.document, &shown[0].parts[0].document),
            (&first, &relay)
        );
        assert_eq!(
            (&shown[1].row.document, &shown[1].parts[0].document),
            (&second, &phone)
        );
        assert_eq!(shown[4].part_of[0].document, second);
        assert_eq!(shown[5].part_of[0].document, first);
    }

    #[test]
    fn the_topics_of_a_topic_are_its_unended_direct_parts() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        topic(&world, "phone", &part_of(&[&atlas]));
        let relay = topic(&world, "relay", &part_of(&[&lantern, &atlas]));
        topic(
            &world,
            "globe",
            &format!("{}{}", part_of(&[&lantern]), ENDED),
        );
        topic(&world, "compass", "");
        fact(&world, &atlas, "map", "");
        fact(&world, &relay, "coil", "idea = true\n");
        followup(&world, &[&relay], "open", "");
        let deps = world.deps();

        let parts = topics(&deps.read(), Some("lantern"), false).unwrap();
        assert_eq!(counts(&parts), [("atlas", 1, 0, 0), ("relay", 0, 1, 1)]);
        assert_eq!(referred(&parts[0].part_of), ["lantern"]);
        assert_eq!(referred(&parts[1].part_of), ["lantern", "atlas"]);
        assert_eq!(topics(&deps.read(), Some("lantern"), true).unwrap(), parts);
        assert_eq!(
            labels(&topics(&deps.read(), Some("atlas"), false).unwrap(), |t| &t
                .row),
            ["phone", "relay"]
        );
        assert_eq!(topics(&deps.read(), Some("compass"), false).unwrap(), []);
        assert_eq!(
            topics(&deps.read(), Some(lantern.short()), false).unwrap(),
            parts
        );

        assert!(
            refused(topics(&deps.read(), Some("nowhere"), false)).contains("names no document")
        );
        let text = refused(topics(&deps.read(), Some("atlas/map"), false));
        assert!(text.contains("atlas/map: is not a topic"), "{text}");
        assert!(matches!(
            topics(&deps.read(), Some("not an address!"), false),
            Err(Failure::Usage(_))
        ));
    }

    fn topic_scans_for(count: usize) -> usize {
        let (world, lantern) = world();
        for index in 0..count {
            let part = topic(
                &world,
                &format!("part-{}", ["a", "b", "c", "d", "e"][index]),
                &part_of(&[&lantern]),
            );
            fact(&world, &part, "pin", "");
            fact(&world, &part, "cord", "idea = true\n");
            followup(&world, &[&part, &lantern], "open", "");
        }
        let world = world.counted();
        let deps = world.deps();
        let shown = topics(&deps.read(), None, false).unwrap();
        assert_eq!(shown.len(), count + 2);
        let parts: Vec<&TopicRow> = (shown.iter())
            .filter(|topic| topic.row.label.starts_with("part-"))
            .collect();
        assert_eq!(parts.len(), count);
        assert!(
            parts
                .iter()
                .all(|part| { (part.fact_count, part.idea_count, part.open_count) == (1, 1, 1) })
        );
        world.store.scans()
    }

    #[test]
    fn topics_scans_the_store_the_same_whatever_the_number_of_topics() {
        assert_eq!(topic_scans_for(1), 3);
        assert_eq!(topic_scans_for(5), 3);
    }

    #[test]
    fn facts_roll_up_a_topic_over_two_levels_of_parts() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", &part_of(&[&atlas]));
        let desk = world.machine();
        fact(&world, &lantern, "one", "");
        fact(&world, &atlas, "two", "");
        fact(&world, &phone, "three", "");
        fact(&world, &desk, "four", "");
        let deps = world.deps();

        let all = facts(&deps.read(), None, false).unwrap();
        assert_eq!(
            labels(&all, |f| &f.row),
            ["atlas/two", "desk/four", "lantern/one", "phone/three"]
        );
        assert_eq!(all[0].confirmed, Date::parse("2026-09-04").unwrap());
        let under = |address| {
            labels(&facts(&deps.read(), Some(address), false).unwrap(), |f| {
                &f.row
            })
        };
        assert_eq!(
            under("lantern"),
            ["atlas/two", "lantern/one", "phone/three"]
        );
        assert_eq!(under("atlas"), ["atlas/two", "phone/three"]);
        assert_eq!(under("phone"), ["phone/three"]);
    }

    #[test]
    fn a_topic_argument_must_be_one_topic() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        let deps = world.deps();
        assert!(refused(facts(&deps.read(), Some("nowhere"), false)).contains("names no document"));
        let text = refused(facts(&deps.read(), Some("lantern/relay-pin"), false));
        assert!(
            text.contains("lantern/relay-pin") && text.contains("is not a topic"),
            "{text}"
        );
        assert!(matches!(
            facts(&deps.read(), Some("not an address!"), false),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn a_forked_fact_belongs_by_its_unended_heads_and_is_marked() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", "");
        let id = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &id);
        sibling(
            &world,
            &root,
            &fact_fields(&atlas, "relay-pin", "s", "idea = false\n"),
            "2026-10-09T10:00:00+01:00",
        );
        sibling(
            &world,
            &root,
            &fact_fields(&lantern, "relay-pin", "s", ENDED_FACT),
            "2026-10-09T11:00:00+01:00",
        );
        let deps = world.deps();

        let under_atlas = facts(&deps.read(), Some("atlas"), false).unwrap();
        assert_eq!(under_atlas.len(), 1);
        assert!(under_atlas[0].row.forked);
        assert_eq!(under_atlas[0].row.label, "atlas/relay-pin");
        assert!(
            facts(&deps.read(), Some("lantern"), false)
                .unwrap()
                .is_empty()
        );
        let whole = facts(&deps.read(), None, false).unwrap();
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].row.ended, None);
    }

    #[test]
    fn ended_documents_are_listed_only_when_asked_for() {
        let (world, lantern) = world();
        fact(&world, &lantern, "kept", "");
        let gone = fact(&world, &lantern, "gone", "");
        world.amend(&gone, &fact_fields(&lantern, "gone", "s", ENDED_FACT), "\n");
        let deps = world.deps();

        for topic in [None, Some("lantern")] {
            let kept = facts(&deps.read(), topic, false).unwrap();
            assert_eq!(labels(&kept, |f| &f.row), ["lantern/kept"]);
            let every = facts(&deps.read(), topic, true).unwrap();
            assert_eq!(labels(&every, |f| &f.row), ["lantern/gone", "lantern/kept"]);
            assert_eq!(every[0].row.ended.as_deref(), Some("false"));
        }
    }

    #[test]
    fn an_idea_is_in_ideas_and_not_in_facts() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        fact(&world, &lantern, "second-relay", "idea = true\n");
        let deps = world.deps();
        assert_eq!(
            labels(&facts(&deps.read(), None, false).unwrap(), |f| &f.row),
            ["lantern/relay-pin"]
        );
        assert_eq!(
            labels(&ideas(&deps.read(), None, false).unwrap(), |f| &f.row),
            ["lantern/second-relay"]
        );
        assert_eq!(
            labels(&ideas(&deps.read(), Some("lantern"), false).unwrap(), |f| {
                &f.row
            }),
            ["lantern/second-relay"]
        );
    }

    #[test]
    fn entries_come_newest_date_first_then_by_name_and_are_limited() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        entry(&world, "2026-10-08", "b-driver", &[&lantern]);
        entry(&world, "2026-10-08", "a-driver", &[&atlas]);
        entry(&world, "2026-10-07", "z-driver", &[&lantern]);
        entry(&world, "2026-10-09", "m-driver", &[&lantern]);
        let deps = world.deps();

        let all = entries(&deps.read(), None, None, None, false).unwrap();
        assert_eq!(
            labels(&all, |e| &e.row),
            [
                "2026-10-09-m-driver",
                "2026-10-08-a-driver",
                "2026-10-08-b-driver",
                "2026-10-07-z-driver"
            ]
        );
        assert_eq!(all[0].date, Date::parse("2026-10-09").unwrap());
        let two = entries(&deps.read(), Some("lantern"), None, Some(2), false).unwrap();
        assert_eq!(
            labels(&two, |e| &e.row),
            ["2026-10-09-m-driver", "2026-10-08-a-driver"]
        );
        let under_atlas = entries(&deps.read(), Some("atlas"), None, None, false).unwrap();
        assert_eq!(labels(&under_atlas, |e| &e.row), ["2026-10-08-a-driver"]);
        let on = entries(
            &deps.read(),
            Some("lantern"),
            Some("2026-10-08"),
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            labels(&on, |e| &e.row),
            ["2026-10-08-a-driver", "2026-10-08-b-driver"]
        );
        assert!(matches!(
            entries(&deps.read(), None, Some("yesterday"), None, false),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn followups_are_ordered_by_urgency_and_due_includes_today() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let later = followup(
            &world,
            &[&lantern],
            "s",
            "look_again = 2026-10-10\nwhy = \"w\"\n",
        );
        let today = followup(
            &world,
            &[&lantern],
            "s",
            "look_again = 2026-10-09\nwhy = \"w\"\n",
        );
        let touching = followup(&world, &[&atlas], "s", &format!("touching = \"{atlas}\"\n"));
        let bare = followup(&world, &[&lantern], "s", "");
        let early = followup(
            &world,
            &[&atlas],
            "s",
            "look_again = 2026-10-01\nwhy = \"w\"\n",
        );
        let deps = world.deps();

        let rows = followups(&deps.read(), None, false).unwrap();
        let ids: Vec<&DocumentId> = rows.iter().map(|r| &r.row.document).collect();
        assert_eq!(ids, [&early, &today, &later, &touching, &bare]);
        let due: Vec<bool> = rows.iter().map(|r| r.due).collect();
        assert_eq!(due, [true, true, false, false, false]);
        assert_eq!(
            rows[1].trigger,
            Some(TriggerShown::LookAgain {
                on: Date::parse("2026-10-09").unwrap(),
                why: "w".to_owned()
            })
        );
        assert_eq!(
            rows[3].trigger,
            Some(TriggerShown::Touching {
                topic: to(&atlas, "atlas")
            })
        );
        assert_eq!(rows[4].trigger, None);

        let rollup = followups(&deps.read(), Some("atlas"), false).unwrap();
        assert_eq!(rollup.len(), 2);
        assert_eq!(
            followups(&deps.read(), Some("lantern"), false)
                .unwrap()
                .len(),
            5
        );
    }

    #[test]
    fn followups_of_an_entry_are_those_that_name_it() {
        let (world, lantern) = world();
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&lantern]);
        let relay = fact(&world, &lantern, "relay-pin", "");
        let named = followup(
            &world,
            &[&lantern],
            "s",
            &format!("entry = \"{driver}\"\nabout = \"{relay}\"\n"),
        );
        followup(&world, &[&lantern], "s", "");
        let deps = world.deps();

        let rows = followups(&deps.read(), Some("2026-10-08-lamp-driver"), false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row.document, named);
        assert_eq!(rows[0].entry, Some(to(&driver, "2026-10-08-lamp-driver")));
        assert_eq!(rows[0].about, Some(to(&relay, "lantern/relay-pin")));
        let text = refused(followups(&deps.read(), Some("lantern/relay-pin"), false));
        assert!(text.contains("neither a topic nor an entry"), "{text}");
    }

    #[test]
    fn ended_followups_are_listed_only_when_asked_for() {
        let (world, lantern) = world();
        followup(&world, &[&lantern], "s", "");
        let done = followup(&world, &[&lantern], "s", "");
        world.amend(
            &done,
            &format!(
                "created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n\
                 ended = \"done\"\nended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "\n",
        );
        let deps = world.deps();
        assert_eq!(followups(&deps.read(), None, false).unwrap().len(), 1);
        assert_eq!(followups(&deps.read(), None, true).unwrap().len(), 2);
        assert_eq!(
            followups(&deps.read(), Some("lantern"), false)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            followups(&deps.read(), Some("lantern"), true)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn a_claim_of_this_machine_says_whether_its_directory_is_on_this_host() {
        let (mut world, lantern) = world();
        let phone = topic(&world, "phone", "");
        let desk = world.machine();
        claim(&world, &desk, &lantern, "~/here");
        claim(&world, &desk, &lantern, "~/gone");
        claim(&world, &desk, &lantern, "/srv/gone");
        claim(&world, &desk, &lantern, "");
        claim(&world, &phone, &lantern, "~/gone");
        world.host.set_home(Some("/home/desk".to_owned()));
        world.host.remove("/home/desk/gone");
        world.host.remove("/srv/gone");
        let placed = |directory: &str, exists| (Some(directory.to_owned()), exists);
        assert_eq!(
            claim_exists(&world, None),
            [
                (None, None),
                placed("/srv/gone", Some(false)),
                placed("~/gone", Some(false)),
                placed("~/here", Some(true)),
            ]
        );
        assert_eq!(
            claim_exists(&world, Some("phone")),
            [placed("~/gone", None)]
        );
        world.host.set_home(None);
        assert_eq!(
            claim_exists(&world, None),
            [
                (None, None),
                placed("/srv/gone", Some(false)),
                placed("~/gone", None),
                placed("~/here", None),
            ]
        );
    }

    fn claim_exists(world: &World, machine: Option<&str>) -> Vec<(Option<String>, Option<bool>)> {
        let rows = where_(&world.deps().read(), None, machine).unwrap();
        rows.into_iter()
            .map(|row| (row.directory, row.exists))
            .collect()
    }

    #[test]
    fn claims_are_this_machine_s_by_topic_then_directory() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", "");
        let phone = topic(&world, "phone", "");
        let desk = world.machine();
        claim(&world, &desk, &lantern, "~/b");
        let bare = claim(&world, &desk, &lantern, "");
        claim(&world, &desk, &atlas, "~/a");
        claim(&world, &phone, &lantern, "~/p");
        let gone = claim(&world, &desk, &atlas, "~/z");
        world.amend(
            &gone,
            &format!(
                "machine = \"{desk}\"\ntopic = \"{atlas}\"\ndirectory = \"~/z\"\n\
                 ended = \"removed\"\nended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "\n",
        );
        let deps = world.deps();

        let mine = where_(&deps.read(), None, None).unwrap();
        let shown: Vec<(&str, Option<&str>)> = mine
            .iter()
            .map(|c| (c.topic.label.as_str(), c.directory.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("atlas", Some("~/a")),
                ("lantern", None),
                ("lantern", Some("~/b"))
            ]
        );
        assert_eq!(mine[1].document, bare);
        assert_eq!(mine[0].machine, to(&world.machine(), "desk"));
        assert!(!mine[0].forked);

        let narrowed = where_(&deps.read(), Some("lantern"), None).unwrap();
        assert_eq!(narrowed.len(), 2);
        let other = where_(&deps.read(), None, Some("phone")).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].machine.label, "phone");
        assert_eq!(other[0].directory.as_deref(), Some("~/p"));
        assert!(
            where_(&deps.read(), Some("atlas"), Some("phone"))
                .unwrap()
                .is_empty()
        );
        assert!(refused(where_(&deps.read(), None, Some("nowhere"))).contains("names no document"));
    }

    #[test]
    fn forks_lists_forked_documents_of_every_kind_by_label() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        fact(&world, &lantern, "steady", "");
        let fields = fact_fields(&lantern, "relay-pin", "s", "");
        let root = head(&world, &relay);
        let left = sibling(&world, &root, &fields, "2026-10-09T10:00:00+01:00");
        let right = sibling(
            &world,
            &root,
            &format!("{fields}{ENDED_FACT}"),
            "2026-10-09T11:00:00+01:00",
        );
        let atlas = topic(&world, "atlas", "");
        let atlas_root = head(&world, &atlas);
        let atlas_fields = "name = \"atlas\"\ncreated = 2026-09-04\nsummary = \"s\"\n";
        sibling(
            &world,
            &atlas_root,
            atlas_fields,
            "2026-10-09T10:00:00+01:00",
        );
        sibling(
            &world,
            &atlas_root,
            atlas_fields,
            "2026-10-09T11:00:00+01:00",
        );
        let deps = world.deps();

        let rows = forks(&deps.read()).unwrap();
        assert_eq!(
            labels(&rows, |f| f.row.as_ref().unwrap()),
            ["atlas", "lantern/relay-pin"]
        );
        assert!(rows.iter().all(|f| f.row.as_ref().unwrap().forked));
        assert_eq!(
            rows.iter().map(|f| f.label.as_str()).collect::<Vec<_>>(),
            ["atlas", "lantern/relay-pin"]
        );
        assert_eq!(rows[1].document, relay);
        let stored = world.store.headers(&relay).unwrap();
        let in_store: Vec<VersionId> = stored.heads().iter().map(|h| h.id.clone()).collect();
        let listed: Vec<VersionId> = rows[1].heads.iter().map(|h| h.version.clone()).collect();
        assert_eq!(listed, in_store);
        for shown in &rows[1].heads {
            assert_eq!(shown.machine.label, "desk");
            let (written, ended) = if shown.version == left.id {
                (&left.envelope.written, None)
            } else {
                assert_eq!(shown.version, right.id);
                (&right.envelope.written, Some("false"))
            };
            assert_eq!((&shown.written, shown.ended.as_deref()), (written, ended));
        }
    }

    #[test]
    fn forks_shows_a_machine_that_is_no_topic_as_its_short_id() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let absent = DocumentId::from_bytes([0xb8; 16]);
        let root = head(&world, &lantern);
        let fields = topic_fields("lantern", "");
        let mut written = Vec::new();
        for (machine, stamp) in [
            (&relay, "2026-10-09T10:00:00+01:00"),
            (&absent, "2026-10-09T11:00:00+01:00"),
        ] {
            let head = written_at(&world, &root, &fields, stamp, "\n", Some(machine));
            written.push((head.id.clone(), machine.short()));
        }
        written.sort();

        let rows = forks(&world.deps().read()).unwrap();
        assert_eq!(rows.len(), 1);
        let shown: Vec<(VersionId, &str)> = rows[0]
            .heads
            .iter()
            .map(|head| (head.version.clone(), head.machine.label.as_str()))
            .collect();
        assert_eq!(shown, written);
    }

    #[test]
    fn a_fork_with_no_readable_head_is_listed_with_no_row_and_counted_by_check() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let fields = fact_fields(&lantern, "relay-pin", "s", "");
        sibling(&world, &root, &fields, "2026-10-09T10:00:00+01:00");
        sibling(&world, &root, &fields, "2026-10-09T11:00:00+01:00");
        let atlas = topic(&world, "atlas", "");
        let root = head(&world, &atlas);
        let unread = [
            sibling(
                &world,
                &root,
                "name = \"atlas\"\n",
                "2026-10-09T10:00:00+01:00",
            ),
            sibling(
                &world,
                &root,
                "name = \"atlas\"\n",
                "2026-10-09T11:00:00+01:00",
            ),
        ];
        let sketch = world.put("sketch", "name = \"one\"\n", "\n");
        let root = head(&world, &sketch);
        sibling(
            &world,
            &root,
            "name = \"one\"\n",
            "2026-10-09T10:00:00+01:00",
        );
        sibling(
            &world,
            &root,
            "name = \"one\"\n",
            "2026-10-09T11:00:00+01:00",
        );
        world.store.plant_unreadable(
            &sketch,
            VersionId::of(b"damaged"),
            crate::domain::version::ReadError::Corrupt,
        );
        let deps = world.deps();

        let rows = forks(&deps.read()).unwrap();
        let shown: Vec<(&DocumentId, Option<&str>)> = rows
            .iter()
            .map(|fork| {
                (
                    &fork.document,
                    fork.row.as_ref().map(|row| row.label.as_str()),
                )
            })
            .collect();
        assert_eq!(shown, [(&atlas, None), (&relay, Some("lantern/relay-pin"))]);
        let mut listed: Vec<&VersionId> = rows[0].heads.iter().map(|head| &head.version).collect();
        listed.sort();
        let mut stored: Vec<&VersionId> = unread.iter().map(|version| &version.id).collect();
        stored.sort();
        assert_eq!(listed, stored);
        for shown in &rows[0].heads {
            assert_eq!(
                (shown.machine.label.as_str(), &shown.ended),
                ("desk", &None)
            );
        }
        assert_eq!(
            crate::app::check::check(&deps.read()).unwrap().fork_count,
            rows.len()
        );
    }

    #[test]
    fn forks_asks_for_the_forked_and_for_the_topics_that_label_them() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let fields = fact_fields(&lantern, "relay-pin", "s", "");
        sibling(&world, &root, &fields, "2026-10-09T10:00:00+01:00");
        sibling(&world, &root, &fields, "2026-10-09T11:00:00+01:00");
        let world = world.counted();
        let deps = world.deps();
        assert_eq!(forks(&deps.read()).unwrap().len(), 1);
        assert_eq!(world.store.forks.get(), 1);
        assert_eq!(world.store.kinds.get(), 1);
        assert_eq!(world.store.scans(), 2);
    }

    #[test]
    fn a_topic_argument_may_be_ended_or_forked_but_must_read_as_a_topic() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        fact(&world, &atlas, "pin", "");
        let ended_topic = topic(&world, "phone", "");
        fact(&world, &ended_topic, "gone", "");
        world.amend(
            &ended_topic,
            "name = \"phone\"\ncreated = 2026-09-04\nsummary = \"s\"\n\
             ended = \"retired\"\nended_on = 2026-10-09\nnote = \"n\"\n",
            "\n",
        );
        let relay = topic(&world, "relay", "");
        fact(&world, &relay, "pin", "");
        let root = head(&world, &relay);
        let fields = "name = \"relay\"\ncreated = 2026-09-04\nsummary = \"s\"\n";
        sibling(&world, &root, fields, "2026-10-09T10:00:00+01:00");
        sibling(
            &world,
            &root,
            "name = \"relay\"\n",
            "2026-10-09T11:00:00+01:00",
        );
        let broken = world.put("topic", "name = \"broken\"\n", "\n");
        let deps = world.deps();

        let under = |address| {
            labels(&facts(&deps.read(), Some(address), false).unwrap(), |f| {
                &f.row
            })
        };
        assert_eq!(under("lantern"), ["atlas/pin"]);
        assert_eq!(under("relay"), ["relay/pin"]);
        assert_eq!(
            labels(&facts(&deps.read(), Some("phone"), true).unwrap(), |f| &f
                .row),
            ["phone/gone"]
        );
        let text = refused(facts(&deps.read(), Some(broken.as_str()), false));
        assert!(
            text.contains(broken.as_str()) && text.contains("is not a topic"),
            "{text}"
        );
    }

    #[test]
    fn where_refuses_a_machine_that_is_no_topic_and_marks_a_forked_claim() {
        let (world, lantern) = world();
        let desk = world.machine();
        fact(&world, &lantern, "relay-pin", "");
        let id = claim(&world, &desk, &lantern, "~/a");
        let root = head(&world, &id);
        let fields = format!("machine = \"{desk}\"\ntopic = \"{lantern}\"\ndirectory = \"~/a\"\n");
        sibling(&world, &root, &fields, "2026-10-09T10:00:00+01:00");
        sibling(&world, &root, &fields, "2026-10-09T11:00:00+01:00");
        let deps = world.deps();

        let text = refused(where_(&deps.read(), None, Some("lantern/relay-pin")));
        assert!(text.contains("is not a topic"), "{text}");
        let rows = where_(&deps.read(), None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].forked);
    }

    #[test]
    fn followups_with_equal_dates_come_in_short_id_order() {
        let (world, lantern) = world();
        let stored = |byte: u8, rest: &str| {
            let id = DocumentId::from_bytes([byte; 16]);
            let fields =
                format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n{rest}");
            world
                .store
                .put(&first(&id, "followup", &fields, "\n"))
                .unwrap();
            id
        };
        let dated = [0xc1, 0xb1, 0xa1].map(|byte| stored(byte, DATED));
        let bare = [0xc2, 0xb2, 0xa2].map(|byte| stored(byte, ""));
        let deps = world.deps();

        let rows = followups(&deps.read(), None, false).unwrap();
        let shorts: Vec<&str> = rows.iter().map(|r| r.row.document.short()).collect();
        assert_eq!(
            shorts,
            [
                "a1a1a1a1", "b1b1b1b1", "c1c1c1c1", "a2a2a2a2", "b2b2b2b2", "c2c2c2c2"
            ]
        );
        assert_eq!(rows.len(), dated.len() + bare.len());
    }

    fn questions_for(parts: usize) -> usize {
        let (world, lantern) = world();
        fact(&world, &lantern, "base", "");
        for index in 0..parts {
            let part = topic(
                &world,
                &format!("part-{}", ["a", "b", "c", "d", "e"][index]),
                &part_of(&[&lantern]),
            );
            fact(&world, &part, "pin", "");
        }
        let world = world.counted();
        let deps = world.deps();
        let shown = facts(&deps.read(), Some("lantern"), false).unwrap();
        assert_eq!(shown.len(), parts + 1);
        world.store.questions()
    }

    #[test]
    fn a_rollup_asks_the_store_the_same_whatever_the_number_of_parts() {
        assert_eq!(questions_for(1), questions_for(5));
    }
}
