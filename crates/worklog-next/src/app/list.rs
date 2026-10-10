use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::app::heads::{is_forked, label_or_short, machine_label, version_is_ended};
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::rows::{FollowupRow, Row, by_label, documents, followup_rows, reading, row, shown};
use crate::app::rules::claims_of;
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::schema::kind::Reference;
use crate::domain::schema::{Content, Date, KindOf};
use crate::domain::version::Stamp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopicRow {
    pub row: Row,
    pub part_of: Vec<String>,
    pub uses: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactRow {
    pub row: Row,
    pub confirmed: Date,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRow {
    pub row: Row,
    pub date: Date,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRow {
    pub id: DocumentId,
    pub machine: String,
    pub topic: String,
    pub directory: Option<String>,
    pub forked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkHead {
    pub id: VersionId,
    pub written: Stamp,
    pub machine: String,
    pub ended: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkRow {
    pub id: DocumentId,
    /// None when no head reads as a record.
    pub row: Option<Row>,
    /// In head order; one that does not read as a record is not ended.
    pub heads: Vec<ForkHead>,
}

/// By name; the ended ones only when `ended`.
pub fn topics(deps: &Deps, ended: bool) -> Result<Vec<TopicRow>, Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let labels = |ids: &[DocumentId]| ids.iter().map(|id| topics.label(id)).collect();
    let documents = lookup.of_kind(KindOf::Topic)?;
    let mut rows: Vec<TopicRow> = shown(&topics, &documents, ended)
        .into_iter()
        .map(|shown| TopicRow {
            part_of: labels(topics.part_of(&shown.row.id)),
            uses: labels(topics.uses(&shown.row.id)),
            row: shown.row,
        })
        .collect();
    by_label(&mut rows, |shown| &shown.row);
    Ok(rows)
}

/// Refuses an address that is not one topic. With a topic the facts of its parts are
/// included; by label.
pub fn facts(deps: &Deps, topic: Option<&str>, ended: bool) -> Result<Vec<FactRow>, Failure> {
    fact_rows(deps, topic, ended, false)
}

/// Like `facts`, for the facts flagged `idea`.
pub fn ideas(deps: &Deps, topic: Option<&str>, ended: bool) -> Result<Vec<FactRow>, Failure> {
    fact_rows(deps, topic, ended, true)
}

fn fact_rows(
    deps: &Deps,
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
/// is not one topic, and includes the entries of its parts.
pub fn entries(
    deps: &Deps,
    topic: Option<&str>,
    limit: Option<usize>,
    ended: bool,
) -> Result<Vec<EntryRow>, Failure> {
    let (lookup, topics, topic) = reading(deps, topic)?;
    let documents = documents(&lookup, &topics, KindOf::Entry, topic.as_ref(), ended)?;
    let mut rows: Vec<EntryRow> = shown(&topics, &documents, ended)
        .into_iter()
        .filter_map(|shown| match shown.record.content {
            Content::Entry(entry) => Some(EntryRow {
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
    deps: &Deps,
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
    let to_entry = |reference: &Reference| reference.target == Some(KindOf::Entry);
    let Some(key) = KindOf::Followup.key_where(to_entry) else {
        return Ok(Vec::new());
    };
    lookup.holders(KindOf::Followup, key, &[entry], ended)
}

/// The unended claims of this host's machine topic, or of the machine topic named; `topic`
/// narrows to one topic with no rollup. By topic label, then directory, a claim with no
/// directory first.
pub fn where_(
    deps: &Deps,
    topic: Option<&str>,
    machine: Option<&str>,
) -> Result<Vec<ClaimRow>, Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let machine = match machine {
        Some(address) => lookup.one_topic(address)?,
        None => deps.machine()?,
    };
    let topic = topic.map(|address| lookup.one_topic(address)).transpose()?;
    let rows = claims_of(&lookup, &machine)?
        .into_iter()
        .filter(|(_, claim)| topic.as_ref().is_none_or(|topic| *topic == claim.topic))
        .map(|(document, claim)| ClaimRow {
            id: document.id().clone(),
            machine: topics.label(&claim.machine),
            topic: topics.label(&claim.topic),
            directory: claim.directory.map(|directory| directory.to_string()),
            forked: is_forked(&document),
        });
    let mut rows: Vec<ClaimRow> = rows.collect();
    rows.sort_by(|a, b| (&a.topic, &a.directory, &a.id).cmp(&(&b.topic, &b.directory, &b.id)));
    Ok(rows)
}

/// Every document with more than one head, of any kind this worklog knows, ended ones
/// included; by label, one with no row by its short id.
pub fn forks(deps: &Deps) -> Result<Vec<ForkRow>, Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let mut rows = Vec::new();
    for document in lookup.forks()? {
        let heads = document
            .heads()
            .into_iter()
            .map(|head| ForkHead {
                id: head.id.clone(),
                written: head.envelope.written.clone(),
                machine: machine_label(head, topics.name(&head.envelope.machine)),
                ended: version_is_ended(head),
            })
            .collect();
        rows.push(ForkRow {
            id: document.id().clone(),
            row: row(&topics, &document).map(|shown| shown.row),
            heads,
        });
    }
    rows.sort_by_cached_key(|fork| {
        let label = fork.row.as_ref().map(|row| row.label.clone());
        (label_or_short(label, &fork.id), fork.id.clone())
    });
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::lookup::Stored;
    use crate::app::rows::TriggerShown;
    use crate::app::testing::{
        Counting, ENDED_FACT, World, claim_fields, entry, fact, fact_fields, followup, head,
        labels, part_of, refused, topic, topic_fields, world, written_at,
    };
    use crate::domain::id::VersionId;
    use crate::domain::ports::Store;
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

        let shown = topics(&deps, false).unwrap();
        assert_eq!(labels(&shown, |t| &t.row), ["atlas", "desk", "lantern"]);
        assert_eq!(shown[0].part_of, ["lantern"]);
        assert_eq!(shown[0].uses, Vec::<String>::new());

        let all = topics(&deps, true).unwrap();
        assert_eq!(
            labels(&all, |t| &t.row),
            ["atlas", "desk", "lantern", "phone"]
        );
        assert_eq!(all[3].uses, Vec::<String>::new());
        assert_eq!(all[3].row.ended.as_deref(), Some("retired"));
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

        let all = facts(&deps, None, false).unwrap();
        assert_eq!(
            labels(&all, |f| &f.row),
            ["atlas/two", "desk/four", "lantern/one", "phone/three"]
        );
        assert_eq!(all[0].confirmed, Date::parse("2026-09-04").unwrap());
        let under = |address| labels(&facts(&deps, Some(address), false).unwrap(), |f| &f.row);
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
        assert!(refused(facts(&deps, Some("nowhere"), false)).contains("names no document"));
        let text = refused(facts(&deps, Some("lantern/relay-pin"), false));
        assert!(
            text.contains("lantern/relay-pin") && text.contains("is not a topic"),
            "{text}"
        );
        assert!(matches!(
            facts(&deps, Some("not an address!"), false),
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

        let under_atlas = facts(&deps, Some("atlas"), false).unwrap();
        assert_eq!(under_atlas.len(), 1);
        assert!(under_atlas[0].row.forked);
        assert_eq!(under_atlas[0].row.label, "atlas/relay-pin");
        assert!(facts(&deps, Some("lantern"), false).unwrap().is_empty());
        let whole = facts(&deps, None, false).unwrap();
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
            let kept = facts(&deps, topic, false).unwrap();
            assert_eq!(labels(&kept, |f| &f.row), ["lantern/kept"]);
            let every = facts(&deps, topic, true).unwrap();
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
            labels(&facts(&deps, None, false).unwrap(), |f| &f.row),
            ["lantern/relay-pin"]
        );
        assert_eq!(
            labels(&ideas(&deps, None, false).unwrap(), |f| &f.row),
            ["lantern/second-relay"]
        );
        assert_eq!(
            labels(&ideas(&deps, Some("lantern"), false).unwrap(), |f| &f.row),
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

        let all = entries(&deps, None, None, false).unwrap();
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
        let two = entries(&deps, Some("lantern"), Some(2), false).unwrap();
        assert_eq!(
            labels(&two, |e| &e.row),
            ["2026-10-09-m-driver", "2026-10-08-a-driver"]
        );
        let under_atlas = entries(&deps, Some("atlas"), None, false).unwrap();
        assert_eq!(labels(&under_atlas, |e| &e.row), ["2026-10-08-a-driver"]);
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

        let rows = followups(&deps, None, false).unwrap();
        let ids: Vec<&DocumentId> = rows.iter().map(|r| &r.row.id).collect();
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
            Some(TriggerShown::Touching("atlas".to_owned()))
        );
        assert_eq!(rows[4].trigger, None);

        let rollup = followups(&deps, Some("atlas"), false).unwrap();
        assert_eq!(rollup.len(), 2);
        assert_eq!(followups(&deps, Some("lantern"), false).unwrap().len(), 5);
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

        let rows = followups(&deps, Some("2026-10-08-lamp-driver"), false).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row.id, named);
        assert_eq!(rows[0].entry.as_deref(), Some("2026-10-08-lamp-driver"));
        assert_eq!(rows[0].about.as_deref(), Some("lantern/relay-pin"));
        let text = refused(followups(&deps, Some("lantern/relay-pin"), false));
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
        assert_eq!(followups(&deps, None, false).unwrap().len(), 1);
        assert_eq!(followups(&deps, None, true).unwrap().len(), 2);
        assert_eq!(followups(&deps, Some("lantern"), false).unwrap().len(), 1);
        assert_eq!(followups(&deps, Some("lantern"), true).unwrap().len(), 2);
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

        let mine = where_(&deps, None, None).unwrap();
        let shown: Vec<(&str, Option<&str>)> = mine
            .iter()
            .map(|c| (c.topic.as_str(), c.directory.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("atlas", Some("~/a")),
                ("lantern", None),
                ("lantern", Some("~/b"))
            ]
        );
        assert_eq!(mine[1].id, bare);
        assert_eq!(mine[0].machine, "desk");
        assert!(!mine[0].forked);

        let narrowed = where_(&deps, Some("lantern"), None).unwrap();
        assert_eq!(narrowed.len(), 2);
        let other = where_(&deps, None, Some("phone")).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].machine, "phone");
        assert_eq!(other[0].directory.as_deref(), Some("~/p"));
        assert!(
            where_(&deps, Some("atlas"), Some("phone"))
                .unwrap()
                .is_empty()
        );
        assert!(refused(where_(&deps, None, Some("nowhere"))).contains("names no document"));
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

        let rows = forks(&deps).unwrap();
        assert_eq!(
            labels(&rows, |f| f.row.as_ref().unwrap()),
            ["atlas", "lantern/relay-pin"]
        );
        assert!(rows.iter().all(|f| f.row.as_ref().unwrap().forked));
        assert_eq!(rows[1].id, relay);
        let stored = world.store.document(&relay).unwrap();
        let in_store: Vec<VersionId> = stored.heads().iter().map(|h| h.id.clone()).collect();
        let listed: Vec<VersionId> = rows[1].heads.iter().map(|h| h.id.clone()).collect();
        assert_eq!(listed, in_store);
        for shown in &rows[1].heads {
            assert_eq!(shown.machine, "desk");
            let (written, ended) = if shown.id == left.id {
                (&left.envelope.written, false)
            } else {
                assert_eq!(shown.id, right.id);
                (&right.envelope.written, true)
            };
            assert_eq!((&shown.written, shown.ended), (written, ended));
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
            written.push((head.id, machine.short()));
        }
        written.sort();

        let rows = forks(&world.deps()).unwrap();
        assert_eq!(rows.len(), 1);
        let shown: Vec<(VersionId, &str)> = rows[0]
            .heads
            .iter()
            .map(|head| (head.id.clone(), head.machine.as_str()))
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

        let rows = forks(&deps).unwrap();
        let shown: Vec<(&DocumentId, Option<&str>)> = rows
            .iter()
            .map(|fork| (&fork.id, fork.row.as_ref().map(|row| row.label.as_str())))
            .collect();
        assert_eq!(shown, [(&atlas, None), (&relay, Some("lantern/relay-pin"))]);
        let mut listed: Vec<&VersionId> = rows[0].heads.iter().map(|head| &head.id).collect();
        listed.sort();
        let mut stored: Vec<&VersionId> = unread.iter().map(|version| &version.id).collect();
        stored.sort();
        assert_eq!(listed, stored);
        for shown in &rows[0].heads {
            assert_eq!((shown.machine.as_str(), shown.ended), ("desk", false));
        }
        assert_eq!(crate::app::check::check(&deps).unwrap().forks, rows.len());
    }

    #[test]
    fn forks_asks_for_the_forked_and_for_the_topics_that_label_them() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let fields = fact_fields(&lantern, "relay-pin", "s", "");
        sibling(&world, &root, &fields, "2026-10-09T10:00:00+01:00");
        sibling(&world, &root, &fields, "2026-10-09T11:00:00+01:00");
        let World {
            store,
            drafts,
            ids,
            clock,
            host,
        } = world;
        let counting = Counting::over(store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &drafts,
            ids: &ids,
            clock: &clock,
            host: &host,
        };
        assert_eq!(forks(&deps).unwrap().len(), 1);
        assert_eq!(counting.forks.get(), 1);
        assert_eq!(counting.kinds.get(), 1);
        assert_eq!(counting.scans(), 2);
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

        let under = |address| labels(&facts(&deps, Some(address), false).unwrap(), |f| &f.row);
        assert_eq!(under("lantern"), ["atlas/pin"]);
        assert_eq!(under("relay"), ["relay/pin"]);
        assert_eq!(
            labels(&facts(&deps, Some("phone"), true).unwrap(), |f| &f.row),
            ["phone/gone"]
        );
        let text = refused(facts(&deps, Some(broken.as_str()), false));
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

        let text = refused(where_(&deps, None, Some("lantern/relay-pin")));
        assert!(text.contains("is not a topic"), "{text}");
        let rows = where_(&deps, None, None).unwrap();
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

        let rows = followups(&deps, None, false).unwrap();
        let shorts: Vec<&str> = rows.iter().map(|r| r.row.id.short()).collect();
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
        let World {
            store,
            drafts,
            ids,
            clock,
            host,
        } = world;
        let counting = Counting::over(store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &drafts,
            ids: &ids,
            clock: &clock,
            host: &host,
        };
        let shown = facts(&deps, Some("lantern"), false).unwrap();
        assert_eq!(shown.len(), parts + 1);
        counting.questions()
    }

    #[test]
    fn a_rollup_asks_the_store_the_same_whatever_the_number_of_parts() {
        assert_eq!(questions_for(1), questions_for(5));
    }
}
