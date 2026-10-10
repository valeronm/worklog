use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::Serialize;

use crate::app::heads::is_forked;
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::rows::{
    FollowupRow, Row, by_label, followup_rows, members, memberships, row, shown,
};
use crate::app::rules::claims_of;
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::id::DocumentId;
use crate::domain::schema::{Claim, Content, Date, Directory, KindOf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Context {
    pub machine: String,
    /// The claimed topics, closest directory first, then what they reach, then what the
    /// machine topic reaches.
    pub topics: Vec<Loaded>,
    /// By date, then those waiting on a topic.
    pub due: Vec<FollowupRow>,
    /// The open followups that are not due.
    pub open_count: usize,
    /// The forked ones among the loaded topics, their facts and ideas, the open followups and
    /// the claims that loaded a topic, by label.
    pub forks: Vec<Row>,
    pub draft_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Loaded {
    pub document: DocumentId,
    pub label: String,
    pub summary: String,
    pub via: Via,
    /// Filed on the topic itself, by label.
    pub facts: Vec<Row>,
    pub ideas: Vec<Row>,
    /// By label.
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Via {
    Claim { directory: Option<String> },
    Edge,
    Machine,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Part {
    pub label: String,
    /// The facts and ideas of the part and of everything it covers.
    pub fact_count: usize,
    pub open_count: usize,
}

/// What a session starting in `directory` is opened with. A `directory` from the root is
/// read as the host resolves it, `.` and `..` folded as typed first, and at or under the
/// host's home is compared with a claim's in the `~/` form; one no claim could carry is a
/// usage failure. A host with no machine topic is refused as not set up, and one whose
/// machine topic is ended until that topic is reopened.
pub fn context(deps: &Deps, directory: &str) -> Result<Context, Failure> {
    let directory = &deps.directory(directory)?;
    let machine = deps.machine()?;
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let today = deps.today()?;
    if topics.is_ended(&machine) {
        return Err(Failure::at(
            topics.label(&machine),
            "is this host's machine topic and is ended; reopen it first",
        ));
    }

    let claimed = Claimed::read(&lookup, &topics, &machine, directory)?;
    let session = topics.reach(&claimed.order);
    let of_machine = topics.reach(std::slice::from_ref(&machine));
    let mut loaded = session.clone();
    for id in &of_machine {
        if !loaded.contains(id) {
            loaded.push(id.clone());
        }
    }
    let mut counted: BTreeSet<DocumentId> = session.iter().cloned().collect();
    if !claimed.in_directory {
        counted.extend(of_machine.iter().cloned());
    }

    let (asked, mut parts) = parts_of(&topics, &loaded);
    let facts = by_topic(
        &members(&lookup, KindOf::Fact, &asked, false)?,
        KindOf::Fact,
    );
    let followups = by_topic(
        &members(&lookup, KindOf::Followup, &asked, false)?,
        KindOf::Followup,
    );
    let mut forks: Vec<Row> = (claimed.loading.iter())
        .filter(|claim| is_forked(claim))
        .filter_map(|claim| row(&topics, claim))
        .map(|found| found.row)
        .collect();
    let mut shown_topics = Vec::new();
    for id in &loaded {
        let (plain, ideas) = filed_on(&topics, &facts, id);
        let document = lookup.document(id)?;
        if is_forked(&document) {
            forks.extend(row(&topics, &document).map(|found| found.row));
        }
        let listed = plain.iter().chain(&ideas);
        forks.extend(listed.filter(|fact| fact.forked).cloned());
        let via = match claimed.directories.get(id) {
            Some(directory) => Via::Claim {
                directory: directory.as_ref().map(Directory::to_string),
            },
            None if session.contains(id) => Via::Edge,
            None => Via::Machine,
        };
        shown_topics.push(Loaded {
            document: id.clone(),
            label: topics.label(id),
            summary: topics.summary(id).unwrap_or_default().to_owned(),
            via,
            facts: plain,
            ideas,
            parts: (parts.remove(id).unwrap_or_default().into_iter())
                .map(|(label, covered)| Part {
                    label,
                    fact_count: filed(&facts, &covered).len(),
                    open_count: filed(&followups, &covered).len(),
                })
                .collect(),
        });
    }

    let work = open_work(&lookup, &topics, &followups, &counted, today)?;
    let forked = work.iter().filter(|followup| followup.row.forked);
    forks.extend(forked.map(|followup| followup.row.clone()));
    let (due, waiting): (Vec<FollowupRow>, Vec<FollowupRow>) =
        work.into_iter().partition(|followup| followup.due);

    by_label(&mut forks, |fork| fork);
    forks.dedup_by(|a, b| a.document == b.document);
    Ok(Context {
        machine: topics.label(&machine),
        topics: shown_topics,
        open_count: waiting.len(),
        due,
        forks,
        draft_count: deps.drafts.list()?.len(),
    })
}

type Parts<'l> = BTreeMap<&'l DocumentId, Vec<(String, BTreeSet<DocumentId>)>>;

fn parts_of<'l>(topics: &Topics, loaded: &'l [DocumentId]) -> (BTreeSet<DocumentId>, Parts<'l>) {
    let is_loaded: BTreeSet<DocumentId> = loaded.iter().cloned().collect();
    let mut asked = is_loaded.clone();
    let mut parts = Parts::new();
    for id in loaded {
        let mut of_topic = Vec::new();
        for part in topics.direct_parts(id) {
            if !is_loaded.contains(&part) {
                let covered = topics.covered(&part);
                asked.extend(covered.iter().cloned());
                of_topic.push((topics.label(&part), covered));
            }
        }
        of_topic.sort_by(|a, b| a.0.cmp(&b.0));
        parts.insert(id, of_topic);
    }
    (asked, parts)
}

type Filed = BTreeMap<DocumentId, Vec<Rc<Document>>>;

fn by_topic(documents: &[Rc<Document>], kind: KindOf) -> Filed {
    let mut grouped = Filed::new();
    for document in documents {
        for topic in memberships(document, kind) {
            grouped.entry(topic).or_default().push(Rc::clone(document));
        }
    }
    grouped
}

fn filed(grouped: &Filed, topics: &BTreeSet<DocumentId>) -> Vec<Rc<Document>> {
    let mut found: BTreeMap<&DocumentId, &Rc<Document>> = BTreeMap::new();
    for topic in topics {
        for document in grouped.get(topic).into_iter().flatten() {
            found.insert(document.id(), document);
        }
    }
    found.into_values().cloned().collect()
}

fn filed_on(topics: &Topics, facts: &Filed, topic: &DocumentId) -> (Vec<Row>, Vec<Row>) {
    let on_topic = facts.get(topic).map(Vec::as_slice).unwrap_or_default();
    let (mut plain, mut ideas) = (Vec::new(), Vec::new());
    for found in shown(topics, on_topic, false) {
        match found.record.content {
            Content::Fact(fact) if fact.idea => ideas.push(found.row),
            _ => plain.push(found.row),
        }
    }
    by_label(&mut plain, |fact| fact);
    by_label(&mut ideas, |fact| fact);
    (plain, ideas)
}

fn open_work(
    lookup: &Lookup,
    topics: &Topics,
    followups: &Filed,
    counted: &BTreeSet<DocumentId>,
    today: Date,
) -> Result<Vec<FollowupRow>, Failure> {
    let counted_work = filed(followups, counted);
    let rows = shown(topics, &counted_work, false);
    followup_rows(lookup, topics, rows, today, counted)
}

struct Claimed {
    order: Vec<DocumentId>,
    directories: BTreeMap<DocumentId, Option<Directory>>,
    in_directory: bool,
    loading: Vec<Rc<Document>>,
}

struct Loading {
    // Zero for a claim with no directory.
    depth: usize,
    label: String,
    claim: Claim,
    document: Rc<Document>,
}

impl Loading {
    fn closest_first(&self) -> (Reverse<usize>, &str, &DocumentId) {
        (Reverse(self.depth), &self.label, self.document.id())
    }
}

impl Claimed {
    fn read(
        lookup: &Lookup,
        topics: &Topics,
        machine: &DocumentId,
        directory: &Directory,
    ) -> Result<Claimed, Failure> {
        let mut covering = Vec::new();
        let mut anywhere = Vec::new();
        for (document, claim) in claims_of(lookup, machine)? {
            if topics.name(&claim.topic).is_none() || topics.is_ended(&claim.topic) {
                continue;
            }
            let label = topics.label(&claim.topic);
            let depth = (claim.directory.as_ref()).map(|claimed| claimed.covers(directory));
            let (into, depth) = match depth {
                None => (&mut anywhere, 0),
                Some(Some(depth)) => (&mut covering, depth),
                Some(None) => continue,
            };
            into.push(Loading {
                depth,
                label,
                claim,
                document,
            });
        }
        let in_directory = !covering.is_empty();
        let mut claims = if in_directory { covering } else { anywhere };
        claims.sort_by(|a, b| a.closest_first().cmp(&b.closest_first()));
        let mut claimed = Claimed {
            order: Vec::new(),
            directories: BTreeMap::new(),
            in_directory,
            loading: Vec::new(),
        };
        for Loading {
            claim, document, ..
        } in claims
        {
            if !claimed.directories.contains_key(&claim.topic) {
                claimed.order.push(claim.topic.clone());
                claimed.directories.insert(claim.topic, claim.directory);
            }
            claimed.loading.push(document);
        }
        Ok(claimed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::lookup::Stored;
    use crate::app::rows::TriggerShown;
    use crate::app::testing::{
        Counting, ENDED, ENDED_FACT, World, claim_fields, fact, followup, fork, head, labels,
        part_of, refused, topic, topic_fields, uses, world_of,
    };
    use crate::domain::draft::Draft;
    use crate::domain::ports::{Drafts, Store};
    use crate::domain::testing::{FixedHost, after};
    use crate::domain::version::{Fields, Kind};

    fn claim(world: &World, topic: &DocumentId, directory: &str) -> DocumentId {
        world.put(
            "claim",
            &claim_fields(&world.machine(), topic, directory),
            "\n",
        )
    }

    fn dated(on: &str) -> String {
        format!("look_again = {on}\nwhy = \"w\"\n")
    }

    fn at(world: &World, directory: &str) -> Context {
        context(&world.deps(), directory).unwrap()
    }

    fn claimed(directory: &str) -> Via {
        Via::Claim {
            directory: (!directory.is_empty()).then(|| directory.to_owned()),
        }
    }

    fn loaded(context: &Context) -> Vec<(&str, Via)> {
        context
            .topics
            .iter()
            .map(|topic| (topic.label.as_str(), topic.via.clone()))
            .collect()
    }

    fn summaries(rows: &[FollowupRow]) -> Vec<&str> {
        rows.iter().map(|row| row.row.summary.as_str()).collect()
    }

    fn of<'a>(context: &'a Context, label: &str) -> &'a Loaded {
        context
            .topics
            .iter()
            .find(|topic| topic.label == label)
            .expect("a loaded topic")
    }

    #[test]
    fn the_unended_claims_of_this_machine_load_their_topics() {
        let (world, [lantern, atlas, phone]) = world_of(["lantern", "atlas", "phone"]);
        claim(&world, &lantern, "/work/lantern");
        world.put(
            "claim",
            &claim_fields(&phone, &atlas, "/work/lantern"),
            "\n",
        );
        let ended = claim(&world, &atlas, "/work/lantern");
        world.amend(
            &ended,
            &format!(
                "{}ended = \"removed\"\nended_on = 2026-10-09\nnote = \"n\"\n",
                claim_fields(&world.machine(), &atlas, "/work/lantern")
            ),
            "\n",
        );

        let context = at(&world, "/work/lantern");
        assert_eq!(context.machine, "desk");
        assert_eq!(
            loaded(&context),
            [
                ("lantern", claimed("/work/lantern")),
                ("desk", Via::Machine)
            ]
        );
        assert_eq!(context.topics[0].document, lantern);
        assert_eq!(context.topics[0].summary, "s");
    }

    #[test]
    fn a_claim_covers_its_directory_and_what_is_under_it_closest_first() {
        let (world, [lantern, atlas, phone, relay]) =
            world_of(["lantern", "atlas", "phone", "relay"]);
        claim(&world, &phone, "/work/lantern/");
        claim(&world, &lantern, "/work/lantern");
        claim(&world, &atlas, "/work");
        claim(&world, &relay, "/work/lan");

        let inside = [
            ("lantern", claimed("/work/lantern")),
            ("phone", claimed("/work/lantern")),
            ("atlas", claimed("/work")),
            ("desk", Via::Machine),
        ];
        assert_eq!(loaded(&at(&world, "/work/lantern/src")), inside);
        assert_eq!(loaded(&at(&world, "/work/lantern")), inside);
        assert_eq!(loaded(&at(&world, "/work/lantern/")), inside);
        assert_eq!(
            loaded(&at(&world, "/work/lantern-two")),
            [("atlas", claimed("/work")), ("desk", Via::Machine)]
        );
        assert_eq!(
            loaded(&at(&world, "/work/lan")),
            [
                ("relay", claimed("/work/lan")),
                ("atlas", claimed("/work")),
                ("desk", Via::Machine)
            ]
        );
        assert_eq!(loaded(&at(&world, "/other")), [("desk", Via::Machine)]);
        assert_eq!(loaded(&at(&world, "/")), [("desk", Via::Machine)]);
    }

    #[test]
    fn a_claim_stored_with_a_trailing_slash_covers_as_the_one_without_it() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "~/lantern/");
        let inside = [("lantern", claimed("~/lantern")), ("desk", Via::Machine)];
        assert_eq!(loaded(&at(&world, "~/lantern")), inside);
        assert_eq!(loaded(&at(&world, "~/lantern/")), inside);
        assert_eq!(loaded(&at(&world, "~/lantern/src/")), inside);
        assert_eq!(
            loaded(&at(&world, "~/lantern-two")),
            [("desk", Via::Machine)]
        );
        assert_eq!(loaded(&at(&world, "~")), [("desk", Via::Machine)]);
    }

    #[test]
    fn a_claim_on_the_root_covers_every_absolute_directory() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "/");
        assert_eq!(loaded(&at(&world, "/work"))[0], ("lantern", claimed("/")));
        assert_eq!(loaded(&at(&world, "/"))[0], ("lantern", claimed("/")));
        assert_eq!(loaded(&at(&world, "~/work")), [("desk", Via::Machine)]);
    }

    #[test]
    fn the_claims_with_no_directory_load_where_no_directory_covers() {
        let (world, [lantern, atlas, phone]) = world_of(["lantern", "atlas", "phone"]);
        claim(&world, &lantern, "");
        claim(&world, &atlas, "");
        claim(&world, &phone, "~/work/phone");

        assert_eq!(
            loaded(&at(&world, "~/elsewhere")),
            [
                ("atlas", claimed("")),
                ("lantern", claimed("")),
                ("desk", Via::Machine)
            ]
        );
        assert_eq!(
            loaded(&at(&world, "~/work/phone/src")),
            [("phone", claimed("~/work/phone")), ("desk", Via::Machine)]
        );
    }

    #[test]
    fn a_topic_claimed_for_two_covering_directories_loads_once_by_the_closest() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "/work");
        claim(&world, &lantern, "/work/lantern");
        assert_eq!(
            loaded(&at(&world, "/work/lantern")),
            [
                ("lantern", claimed("/work/lantern")),
                ("desk", Via::Machine)
            ]
        );
    }

    #[test]
    fn what_a_claimed_topic_reaches_loads_after_the_claimed_ones() {
        let (world, [lantern, relay]) = world_of(["lantern", "relay"]);
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", &uses(&[&atlas]));
        claim(&world, &phone, "/work");
        claim(&world, &relay, "/work");

        assert_eq!(
            loaded(&at(&world, "/work")),
            [
                ("phone", claimed("/work")),
                ("relay", claimed("/work")),
                ("atlas", Via::Edge),
                ("lantern", Via::Edge),
                ("desk", Via::Machine)
            ]
        );
    }

    #[test]
    fn what_the_machine_topic_reaches_loads_last_and_a_claimed_one_stays_claimed() {
        let (world, [lantern]) = world_of(["lantern"]);
        let relay = topic(&world, "relay", &part_of(&[&lantern]));
        world.amend(
            &world.machine(),
            &topic_fields("desk", &uses(&[&relay])),
            "\n",
        );
        claim(&world, &lantern, "/work");

        assert_eq!(
            loaded(&at(&world, "/work")),
            [
                ("lantern", claimed("/work")),
                ("desk", Via::Machine),
                ("relay", Via::Machine)
            ]
        );
        assert_eq!(
            loaded(&at(&world, "/other")),
            [
                ("desk", Via::Machine),
                ("relay", Via::Machine),
                ("lantern", Via::Machine)
            ]
        );
    }

    #[test]
    fn a_claimed_machine_topic_is_claimed() {
        let (world, []) = world_of([]);
        claim(&world, &world.machine(), "/work");
        assert_eq!(loaded(&at(&world, "/work")), [("desk", claimed("/work"))]);
    }

    #[test]
    fn a_loaded_topic_brings_its_own_unended_facts_and_ideas() {
        let (world, [lantern]) = world_of(["lantern"]);
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        claim(&world, &lantern, "/work");
        fact(&world, &lantern, "pin", "");
        fact(&world, &lantern, "bulb", "");
        fact(&world, &lantern, "dimmer", "idea = true\n");
        fact(&world, &lantern, "cord", "idea = true\n");
        fact(&world, &lantern, "wick", ENDED_FACT);
        fact(&world, &atlas, "map", "");
        fact(&world, &world.machine(), "lamp", "");

        let context = at(&world, "/work");
        let lantern = of(&context, "lantern");
        assert_eq!(
            labels(&lantern.facts, |row| row),
            ["lantern/bulb", "lantern/pin"]
        );
        assert_eq!(
            labels(&lantern.ideas, |row| row),
            ["lantern/cord", "lantern/dimmer"]
        );
        assert_eq!(
            labels(&of(&context, "desk").facts, |row| row),
            ["desk/lamp"]
        );
        assert!(context.forks.is_empty());
    }

    #[test]
    fn the_machine_s_open_work_counts_only_outside_a_claimed_directory() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        claim(&world, &lantern, "/work");
        claim(&world, &atlas, "");
        followup(
            &world,
            &[&world.machine()],
            "desk due",
            &dated("2026-10-01"),
        );
        followup(&world, &[&world.machine()], "desk open", "");
        followup(&world, &[&lantern], "lantern open", "");
        followup(&world, &[&atlas], "atlas open", "");

        let inside = at(&world, "/work");
        assert_eq!(summaries(&inside.due), Vec::<&str>::new());
        assert_eq!(inside.open_count, 1);

        let outside = at(&world, "/other");
        assert_eq!(summaries(&outside.due), ["desk due"]);
        assert_eq!(outside.open_count, 2);
    }

    #[test]
    fn a_followup_is_due_by_its_date_or_by_touching_a_topic_that_counts() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "/work");
        followup(&world, &[&lantern], "today", &dated("2026-10-09"));
        followup(&world, &[&lantern], "tomorrow", &dated("2026-10-10"));
        followup(
            &world,
            &[&lantern],
            "touching",
            &format!("touching = \"{lantern}\"\n"),
        );
        followup(&world, &[&lantern], "past", &dated("2026-09-01"));
        followup(&world, &[&lantern], "untriggered", "");
        followup(
            &world,
            &[&lantern, &world.machine()],
            "touching desk",
            &format!("touching = \"{}\"\n", world.machine()),
        );

        let inside = at(&world, "/work");
        assert_eq!(summaries(&inside.due), ["past", "today", "touching"]);
        assert!(inside.due.iter().all(|row| row.due));
        assert_eq!(
            inside.due[2].trigger,
            Some(TriggerShown::Touching {
                topic: "lantern".to_owned()
            })
        );
        assert_eq!(inside.open_count, 3);

        let outside = at(&world, "/other");
        assert_eq!(summaries(&outside.due), ["touching desk"]);
        assert_eq!(outside.open_count, 0);
    }

    #[test]
    fn a_part_that_is_not_loaded_is_counted_with_everything_it_covers() {
        let (world, [lantern]) = world_of(["lantern"]);
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", &part_of(&[&atlas]));
        let relay = topic(&world, "relay", &part_of(&[&lantern]));
        topic(&world, "globe", &format!("{}{ENDED}", part_of(&[&lantern])));
        topic(&world, "compass", &part_of(&[&lantern]));
        claim(&world, &lantern, "/work");
        claim(&world, &relay, "/work");
        fact(&world, &atlas, "map", "");
        fact(&world, &phone, "dial", "");
        fact(&world, &phone, "ring", "idea = true\n");
        fact(&world, &phone, "cord", ENDED_FACT);
        fact(&world, &relay, "coil", "");
        followup(&world, &[&atlas], "a", "");
        followup(&world, &[&phone, &atlas], "b", &dated("2026-10-01"));
        followup(&world, &[&relay], "c", "");

        let context = at(&world, "/work");
        assert_eq!(
            of(&context, "lantern").parts,
            [
                Part {
                    label: "atlas".to_owned(),
                    fact_count: 3,
                    open_count: 2
                },
                Part {
                    label: "compass".to_owned(),
                    fact_count: 0,
                    open_count: 0
                }
            ]
        );
        assert_eq!(of(&context, "relay").parts, []);
        assert!(of(&context, "lantern").facts.is_empty());
        assert_eq!(summaries(&context.due), Vec::<&str>::new());
        assert_eq!(context.open_count, 1);
    }

    #[test]
    fn a_forked_topic_loads_with_the_edges_of_its_unended_heads() {
        let (world, [lantern, atlas, phone]) = world_of(["lantern", "atlas", "phone"]);
        let root = head(&world, &lantern);
        for edge in [part_of(&[&atlas]), uses(&[&phone])] {
            let fields = topic_fields("lantern", &edge);
            world.store.put(&after(&[&root], &fields, "\n")).unwrap();
        }
        claim(&world, &lantern, "/work");

        let context = at(&world, "/work");
        let mut shown = loaded(&context);
        assert_eq!(shown.remove(0), ("lantern", claimed("/work")));
        assert_eq!(shown.pop(), Some(("desk", Via::Machine)));
        shown.sort_by_key(|(label, _)| *label);
        assert_eq!(shown, [("atlas", Via::Edge), ("phone", Via::Edge)]);
        assert_eq!(labels(&context.forks, |row| row), ["lantern"]);
    }

    #[test]
    fn an_ended_topic_does_not_load_and_neither_does_what_only_it_reaches() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        let relay = topic(&world, "relay", &format!("{}{ENDED}", part_of(&[&atlas])));
        claim(&world, &relay, "/work");
        claim(&world, &lantern, "/work");
        fact(&world, &relay, "coil", "");
        followup(&world, &[&relay], "r", &dated("2026-10-01"));

        let context = at(&world, "/work");
        assert_eq!(
            loaded(&context),
            [("lantern", claimed("/work")), ("desk", Via::Machine)]
        );
        assert!(context.due.is_empty());
        assert_eq!(context.open_count, 0);
    }

    #[test]
    fn the_forked_documents_among_what_loaded_are_listed_by_label() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        claim(&world, &lantern, "/work");
        let pin = fact(&world, &lantern, "pin", "");
        let fields = head(&world, &pin).fields.to_string();
        fork(&world, &pin, &fields);
        let map = fact(&world, &atlas, "map", "");
        let fields = head(&world, &map).fields.to_string();
        fork(&world, &map, &fields);
        let due = followup(&world, &[&lantern], "due", &dated("2026-10-01"));
        let fields = head(&world, &due).fields.to_string();
        fork(&world, &due, &fields);
        let waiting = followup(&world, &[&lantern], "waiting", "");
        let fields = head(&world, &waiting).fields.to_string();
        fork(&world, &waiting, &fields);

        let context = at(&world, "/work");
        assert_eq!(
            labels(&context.forks, |row| row),
            [due.short(), waiting.short(), "lantern/pin"]
        );
        let ids: Vec<&DocumentId> = context.forks.iter().map(|row| &row.document).collect();
        assert_eq!(ids, [&due, &waiting, &pin]);
        assert!(context.forks.iter().all(|row| row.forked));
        assert_eq!(context.open_count, 1);
    }

    #[test]
    fn the_drafts_on_this_machine_are_counted() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        assert_eq!(at(&world, "/work").draft_count, 0);
        for id in [lantern, atlas] {
            let draft = Draft::first(
                id,
                Kind::parse("topic").unwrap(),
                Fields::default(),
                String::new(),
            );
            world.drafts.write(&draft).unwrap();
        }
        assert_eq!(at(&world, "/work").draft_count, 2);
    }

    #[test]
    fn a_host_with_no_machine_topic_is_refused() {
        let (mut world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "/work");
        world.host = FixedHost::new(None, None);
        let text = refused(context(&world.deps(), "/work"));
        assert!(text.contains("this host is not set up"), "{text}");
    }

    #[test]
    fn a_machine_topic_the_store_lacks_is_shown_by_its_short_id() {
        let world = World::new();
        let context = at(&world, "/work");
        assert_eq!(context.machine, world.machine().short());
        assert!(context.topics.is_empty());
    }

    #[test]
    fn a_claim_whose_topic_does_not_load_is_as_if_it_were_not_there() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        let relay = topic(&world, "relay", ENDED);
        let pin = fact(&world, &lantern, "pin", "");
        claim(&world, &relay, "/work");
        claim(&world, &DocumentId::from_bytes([0x42; 16]), "/work");
        claim(&world, &pin, "/work");
        claim(&world, &atlas, "");
        followup(
            &world,
            &[&world.machine()],
            "desk due",
            &dated("2026-10-01"),
        );

        let context = at(&world, "/work/src");
        assert_eq!(
            loaded(&context),
            [("atlas", claimed("")), ("desk", Via::Machine)]
        );
        assert_eq!(summaries(&context.due), ["desk due"]);
        assert!(context.forks.is_empty());
    }

    #[test]
    fn a_directory_no_claim_could_hold_is_a_usage_failure() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "/");
        for bad in ["", "work/x", "~work", "./work"] {
            let Err(Failure::Usage(text)) = context(&world.deps(), bad) else {
                panic!("{bad:?} must be a usage failure");
            };
            assert!(text.contains("absolute path"), "{text}");
        }
        assert_eq!(loaded(&at(&world, "//work"))[0], ("lantern", claimed("/")));
        assert_eq!(loaded(&at(&world, "~")), [("desk", Via::Machine)]);
    }

    #[test]
    fn a_claim_on_the_home_directory_covers_what_is_under_it() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "~");
        assert_eq!(loaded(&at(&world, "~/x"))[0], ("lantern", claimed("~")));
        assert_eq!(loaded(&at(&world, "~"))[0], ("lantern", claimed("~")));
        assert_eq!(loaded(&at(&world, "/x")), [("desk", Via::Machine)]);
    }

    #[test]
    fn a_directory_under_the_host_s_home_is_covered_by_a_claim_in_the_home_form() {
        let (mut world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        claim(&world, &lantern, "~/lantern");
        claim(&world, &atlas, "/home/desk/lantern");
        let stored_as_given = [
            ("atlas", claimed("/home/desk/lantern")),
            ("desk", Via::Machine),
        ];
        assert_eq!(
            loaded(&at(&world, "/home/desk/lantern/src")),
            stored_as_given
        );

        world.host.set_home(Some("/home/desk".to_owned()));
        let in_the_home_form = [("lantern", claimed("~/lantern")), ("desk", Via::Machine)];
        assert_eq!(
            loaded(&at(&world, "/home/desk/lantern/src")),
            in_the_home_form
        );
        assert_eq!(loaded(&at(&world, "~/lantern/src")), in_the_home_form);
        assert_eq!(
            loaded(&at(&world, "/home/desktop/lantern")),
            [("desk", Via::Machine)]
        );

        let (mut world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "~");
        world.host.set_home(Some("/home/desk".to_owned()));
        assert_eq!(
            loaded(&at(&world, "/home/desk"))[0],
            ("lantern", claimed("~"))
        );
        assert_eq!(
            loaded(&at(&world, "/home/desk/x"))[0],
            ("lantern", claimed("~"))
        );
        assert_eq!(
            loaded(&at(&world, "/home/desktop/x")),
            [("desk", Via::Machine)]
        );
    }

    #[test]
    fn a_directory_reached_through_a_link_is_covered_by_a_claim_on_what_it_links_to() {
        let (mut world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "~/projects/lantern");
        world.host.set_home(Some("/home/link".to_owned()));
        world.host.link("/home/link", "/home/desk");
        world
            .host
            .link("/srv/lantern", "/home/desk/projects/lantern");
        let through = [
            ("lantern", claimed("~/projects/lantern")),
            ("desk", Via::Machine),
        ];
        for directory in [
            "/srv/lantern",
            "/srv/lantern/case",
            "/home/link/projects/lantern",
            "/home/desk/projects/lantern/case",
        ] {
            assert_eq!(loaded(&at(&world, directory)), through, "{directory}");
        }
        assert_eq!(loaded(&at(&world, "/srv")), [("desk", Via::Machine)]);
    }

    #[test]
    fn a_forked_claim_loads_its_topic_once_and_is_listed_as_forked() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        let forked = claim(&world, &lantern, "/work");
        fork(
            &world,
            &forked,
            &claim_fields(&world.machine(), &lantern, "/work"),
        );
        let unused = claim(&world, &atlas, "");
        fork(&world, &unused, &claim_fields(&world.machine(), &atlas, ""));

        let context = at(&world, "/work");
        assert_eq!(
            loaded(&context),
            [("lantern", claimed("/work")), ("desk", Via::Machine)]
        );
        assert_eq!(labels(&context.forks, |row| row), ["lantern at /work"]);
        assert_eq!(context.forks[0].document, forked);
        assert_eq!(context.forks[0].kind, KindOf::Claim);
    }

    #[test]
    fn a_followup_on_two_counted_topics_is_counted_once() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        claim(&world, &lantern, "/work");
        claim(&world, &atlas, "/work");
        followup(&world, &[&lantern, &atlas], "both", "");
        followup(&world, &[&atlas, &lantern], "due", &dated("2026-10-01"));

        let context = at(&world, "/work");
        assert_eq!(context.open_count, 1);
        assert_eq!(summaries(&context.due), ["due"]);
    }

    #[test]
    fn a_topic_whose_claim_does_not_load_is_shown_by_the_edge_that_reached_it() {
        let (world, [lantern]) = world_of(["lantern"]);
        let phone = topic(&world, "phone", &uses(&[&lantern]));
        claim(&world, &lantern, "");
        claim(&world, &phone, "/work");
        assert_eq!(
            loaded(&at(&world, "/work")),
            [
                ("phone", claimed("/work")),
                ("lantern", Via::Edge),
                ("desk", Via::Machine)
            ]
        );
    }

    #[test]
    fn a_part_of_two_loaded_topics_is_listed_under_each() {
        let (world, [lantern, atlas]) = world_of(["lantern", "atlas"]);
        let compass = topic(
            &world,
            "compass",
            &format!("part_of = [\"{lantern}\", \"{atlas}\"]\n"),
        );
        fact(&world, &compass, "needle", "");
        claim(&world, &lantern, "/work");
        claim(&world, &atlas, "/work");

        let context = at(&world, "/work");
        let part = Part {
            label: "compass".to_owned(),
            fact_count: 1,
            open_count: 0,
        };
        assert_eq!(of(&context, "lantern").parts, std::slice::from_ref(&part));
        assert_eq!(of(&context, "atlas").parts, [part]);
    }

    #[test]
    fn an_ended_machine_topic_is_refused() {
        let (world, [lantern]) = world_of(["lantern"]);
        claim(&world, &lantern, "/work");
        world.amend(&world.machine(), &topic_fields("desk", ENDED), "\n");
        let text = refused(context(&world.deps(), "/work"));
        assert!(
            text.contains("desk: is this host's machine topic and is ended"),
            "{text}"
        );
    }

    fn scans_for(claimed: usize, parts: usize) -> usize {
        let world = World::new();
        topic(&world, "desk", "");
        for topic_number in 0..claimed {
            let name = format!("lantern-{topic_number}");
            let claimed = topic(&world, &name, "");
            claim(&world, &claimed, "/work");
            fact(&world, &claimed, "pin", "");
            followup(&world, &[&claimed], "due", &dated("2026-10-01"));
            for part_number in 0..parts {
                let part = topic(
                    &world,
                    &format!("{name}-part-{part_number}"),
                    &part_of(&[&claimed]),
                );
                fact(&world, &part, "pin", "");
                followup(&world, &[&part], "open", "");
            }
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
        let context = context(&deps, "/work/src").unwrap();
        assert_eq!(context.topics.len(), claimed + 1);
        assert_eq!(context.due.len(), claimed);
        assert!(context.topics[..claimed].iter().all(|topic| {
            topic.parts.len() == parts
                && topic
                    .parts
                    .iter()
                    .all(|part| (part.fact_count, part.open_count) == (1, 1))
        }));
        counting.scans()
    }

    #[test]
    fn the_store_is_scanned_the_same_whatever_the_number_of_topics_and_parts() {
        assert_eq!(scans_for(1, 1), 4);
        assert_eq!(scans_for(3, 5), 4);
    }
}
