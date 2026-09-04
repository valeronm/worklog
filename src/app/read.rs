//! The read use cases. None writes anything.

use std::collections::BTreeMap;

use crate::domain::entry::Entry;
use crate::domain::fact::Fact;
use crate::domain::followup::Followup;
use crate::domain::graph::{self, Via};
use crate::domain::links;
use crate::domain::recheck::Recheck;
use crate::domain::slug::{Kind, Slug};
use crate::domain::topic::Topic;
use crate::domain::version::{State, Version};

use super::load::{self, Doc, Loaded};
use super::output::{
    Check, Context, Diff, FactListing, FollowupItem, Followups, Fork, Forks, Group, Head, History,
    HistoryRow, Hit, Listing, Problem, Row, Search, Shown, Tags, TopicRow, Topics, Where,
};
use super::{Deps, Failure, machine};

fn row(slug: &Slug, date: &str, summary: &str, tags: &[String]) -> Row {
    Row {
        slug: slug.path().to_owned(),
        kind: slug.kind().dir().to_owned(),
        date: date.to_owned(),
        summary: summary.to_owned(),
        tags: tags.to_vec(),
    }
}

fn entry_row(doc: &Doc<Entry>) -> Row {
    row(&doc.slug, &doc.data.date, &doc.data.summary, &doc.data.tags)
}

fn fact_row(doc: &Doc<Fact>) -> Row {
    row(
        &doc.slug,
        doc.written_day(),
        &doc.data.summary,
        &doc.data.tags,
    )
}

fn topic_row(doc: &Doc<Topic>) -> Row {
    row(&doc.slug, doc.written_day(), &doc.data.summary, &[])
}

fn followup_row(doc: &Doc<Followup>) -> Row {
    row(
        &doc.slug,
        doc.written_day(),
        &doc.data.summary,
        &doc.data.tags,
    )
}

fn head(version: &Version) -> Head {
    Head {
        id: version.id.to_string(),
        written: version.block.written.clone(),
        machine: version.block.machine.to_string(),
        operation: version.block.operation.to_string(),
        text: version.content_text(),
    }
}

/// Tags are compared without case, so `Lantern` and `lantern` are one.
fn tag_key(tag: &str) -> String {
    tag.to_ascii_lowercase()
}

fn has_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
}

/// The document's current text, or every head of a fork.
pub fn show(deps: &Deps, slug: &Slug) -> Result<Shown, Failure> {
    let document = deps.store.document(slug)?;
    let heads = match document.state() {
        State::Live(v) => vec![head(v)],
        State::Forked(heads) => heads.into_iter().map(head).collect(),
        State::Absent | State::Tombstoned(_) => return Err(load::not_live(slug, &document)),
    };
    let followups = if slug.kind() == Kind::Entry {
        let today = deps.clock.today();
        load::followups(deps.store)?
            .iter()
            .filter(|f| &f.data.entry == slug)
            .map(|f| followup_item(f, &today, &[]))
            .collect()
    } else {
        Vec::new()
    };
    Ok(Shown {
        slug: slug.path().to_owned(),
        kind: slug.kind().dir().to_owned(),
        forked: heads.len() > 1,
        heads,
        followups,
    })
}

pub fn history(deps: &Deps, slug: &Slug) -> Result<History, Failure> {
    let document = deps.store.document(slug)?;
    if document.versions.is_empty() {
        return Err(Failure::Refused(format!("no {}: {slug}", slug.kind())));
    }
    Ok(History {
        slug: slug.path().to_owned(),
        versions: document
            .history()
            .into_iter()
            .map(|v| HistoryRow {
                id: v.id.to_string(),
                written: v.block.written.clone(),
                machine: v.block.machine.to_string(),
                operation: v.block.operation.to_string(),
                parents: v.block.parents.iter().map(ToString::to_string).collect(),
            })
            .collect(),
    })
}

/// Every live document of a kind; entries newest first, the rest by slug.
pub fn list(deps: &Deps, kind: Kind) -> Result<Listing, Failure> {
    let loaded = load::load(deps.store)?;
    let rows = match kind {
        Kind::Entry => loaded.entries.iter().map(entry_row).collect(),
        Kind::Fact => loaded.facts.iter().map(fact_row).collect(),
        Kind::Topic => loaded.topics.values().map(topic_row).collect(),
        Kind::Followup => loaded.followups.iter().map(followup_row).collect(),
    };
    Ok(Listing { rows })
}

pub fn recent(deps: &Deps, n: usize) -> Result<Listing, Failure> {
    let loaded = load::load(deps.store)?;
    Ok(Listing {
        rows: loaded.entries.iter().take(n).map(entry_row).collect(),
    })
}

/// Facts first, then entries, topics and followups, each with up to three
/// matching lines.
pub fn search(deps: &Deps, term: &str, regex: bool) -> Result<Search, Failure> {
    if term.trim().is_empty() {
        return Err(Failure::Usage("search needs a term".into()));
    }
    let pattern = if regex {
        term.to_owned()
    } else {
        regex::escape(term)
    };
    let matcher = regex::RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| Failure::Usage(format!("bad regex: {e}")))?;
    let loaded = load::load(deps.store)?;
    let mut hits = Vec::new();
    let mut consider = |row: Row, version: &Version| {
        let text = version.content_text();
        let lines: Vec<(usize, String)> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.starts_with("summary:") && matcher.is_match(line))
            .take(3)
            .map(|(i, line)| (i + 1, line.to_owned()))
            .collect();
        if !lines.is_empty() || matcher.is_match(&row.summary) {
            hits.push(Hit { row, lines });
        }
    };
    for f in &loaded.facts {
        consider(fact_row(f), &f.version);
    }
    for e in &loaded.entries {
        consider(entry_row(e), &e.version);
    }
    for t in loaded.topics.values() {
        consider(topic_row(t), &t.version);
    }
    for f in &loaded.followups {
        consider(followup_row(f), &f.version);
    }
    Ok(Search {
        term: term.to_owned(),
        hits,
    })
}

/// Facts first, then entries, carrying the tag.
pub fn tag(deps: &Deps, tag: &str) -> Result<Listing, Failure> {
    let loaded = load::load(deps.store)?;
    let mut rows: Vec<Row> = loaded
        .facts
        .iter()
        .filter(|f| has_tag(&f.data.tags, tag))
        .map(fact_row)
        .collect();
    rows.extend(
        loaded
            .entries
            .iter()
            .filter(|e| has_tag(&e.data.tags, tag))
            .map(entry_row),
    );
    Ok(Listing { rows })
}

/// Every tag with how often entries and facts carry it, most used first.
pub fn tags(deps: &Deps) -> Result<Tags, Failure> {
    let loaded = load::load(deps.store)?;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let all = loaded
        .entries
        .iter()
        .flat_map(|e| e.data.tags.iter())
        .chain(loaded.facts.iter().flat_map(|f| f.data.tags.iter()));
    for tag in all {
        *counts.entry(tag_key(tag)).or_default() += 1;
    }
    let mut tags: Vec<(String, usize)> = counts.into_iter().collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(Tags { tags })
}

/// The topics a facts listing covers: one, one and its includes, or all.
fn topics_covered(loaded: &Loaded, topic: Option<&str>, deep: bool) -> Vec<String> {
    match topic {
        None => loaded.topics.keys().cloned().collect(),
        Some(topic) if deep => graph::included(topic, &loaded.lookup())
            .into_iter()
            .map(|r| r.topic)
            .collect(),
        Some(topic) => vec![topic.to_owned()],
    }
}

/// Facts under a topic, ideas apart; `deep` follows the topic's includes.
pub fn facts(deps: &Deps, topic: Option<&str>, deep: bool) -> Result<FactListing, Failure> {
    let loaded = load::load(deps.store)?;
    let mut listing = FactListing::default();
    for topic in topics_covered(&loaded, topic, deep) {
        for f in loaded.facts_of(&topic) {
            if f.data.idea {
                listing.ideas.push(fact_row(f));
            } else {
                listing.facts.push(fact_row(f));
            }
        }
    }
    Ok(listing)
}

/// Ideas alone, under a topic or everywhere.
pub fn ideas(deps: &Deps, topic: Option<&str>, deep: bool) -> Result<Listing, Failure> {
    Ok(Listing {
        rows: facts(deps, topic, deep)?.ideas,
    })
}

pub fn topics(deps: &Deps) -> Result<Topics, Failure> {
    let loaded = load::load(deps.store)?;
    Ok(Topics {
        topics: loaded
            .topics
            .values()
            .map(|t| TopicRow {
                slug: t.slug.path().to_owned(),
                summary: t.data.summary.clone(),
                machine: t.data.machine.as_ref().map(ToString::to_string),
                includes: t.data.includes.clone(),
            })
            .collect(),
    })
}

/// Where a topic lives on this machine, or on the machine named.
pub fn where_(deps: &Deps, topic: &str, machine_name: Option<&str>) -> Result<Where, Failure> {
    let loaded = load::load(deps.store)?;
    if !loaded.has_topic(topic) {
        return Err(Failure::Refused(format!("no topic: {topic}")));
    }
    let machine_name = match machine_name {
        Some(m) => m.to_owned(),
        None => machine(deps)?.to_string(),
    };
    let (_, machine_topic) = loaded
        .machine_topic(&machine_name)
        .ok_or_else(|| Failure::Refused(format!("no topic carries `machine: {machine_name}`")))?;
    let paths = |map: &[(String, Vec<String>)]| {
        map.iter()
            .filter(|(t, _)| t == topic)
            .flat_map(|(_, paths)| paths.clone())
            .collect::<Vec<_>>()
    };
    Ok(Where {
        topic: topic.to_owned(),
        machine: machine_name,
        dirs: paths(&machine_topic.claims),
        families: paths(&machine_topic.families),
    })
}

/// The label and whether the item is due for a session about `topics`.
fn recheck_status(recheck: Option<&Recheck>, today: &str, topics: &[&str]) -> (String, bool) {
    match recheck {
        Some(r) => (r.label(today, None), r.is_due(today, topics)),
        None => ("no recheck".to_owned(), false),
    }
}

fn followup_item(doc: &Doc<Followup>, today: &str, topics: &[&str]) -> FollowupItem {
    let f = &doc.data;
    let (label, due) = recheck_status(f.recheck.as_ref(), today, topics);
    FollowupItem {
        slug: doc.slug.path().to_owned(),
        source: "followup".to_owned(),
        entry: Some(f.entry.path().to_owned()),
        state: Some(f.state.to_string()),
        summary: f.summary.clone(),
        recheck: f.recheck.as_ref().map(ToString::to_string),
        label,
        due,
    }
}

fn fact_item(doc: &Doc<Fact>, recheck: &Recheck, today: &str, topics: &[&str]) -> FollowupItem {
    let (label, due) = recheck_status(Some(recheck), today, topics);
    FollowupItem {
        slug: doc.slug.path().to_owned(),
        source: if doc.data.idea { "idea" } else { "fact" }.to_owned(),
        entry: None,
        state: None,
        summary: doc.data.summary.clone(),
        recheck: Some(recheck.to_string()),
        label,
        due,
    }
}

/// Open followups oldest first, then facts and ideas with a recheck of
/// their own, for a session about `topics`: an item counts when it is
/// tagged with one, sits under one, or touches one. No topics means all.
fn open_work(loaded: &Loaded, topics: &[&str], today: &str, closed_too: bool) -> Followups {
    let about = |tags: &[String], home: Option<&str>, recheck: Option<&Recheck>| {
        topics.is_empty()
            || topics.iter().any(|t| {
                has_tag(tags, t)
                    || home.is_some_and(|h| h.eq_ignore_ascii_case(t))
                    || recheck.is_some_and(|r| r.touches(t))
            })
    };
    let mut out = Followups::default();
    let mut entries: Vec<&str> = Vec::new();
    for f in &loaded.followups {
        if (!closed_too && !f.data.is_open()) || !about(&f.data.tags, None, f.data.recheck.as_ref())
        {
            continue;
        }
        let item = followup_item(f, today, topics);
        if f.data.is_open() {
            out.open += 1;
            if !entries.contains(&f.data.entry.path()) {
                entries.push(f.data.entry.path());
            }
            out.due += usize::from(item.due);
            out.without_recheck += usize::from(f.data.recheck.is_none());
        }
        out.items.push(item);
    }
    out.entries = entries.len();
    for f in &loaded.facts {
        let Some(recheck) = &f.data.recheck else {
            continue;
        };
        if about(&f.data.tags, f.slug.topic(), Some(recheck)) {
            out.items.push(fact_item(f, recheck, today, topics));
        }
    }
    out
}

/// Open work about a topic, or arising in one entry when `about` is an
/// entry slug; everything when nothing is named.
pub fn followups(deps: &Deps, about: Option<&str>, all: bool) -> Result<Followups, Failure> {
    let loaded = load::load(deps.store)?;
    let today = deps.clock.today();
    let entry = about
        .and_then(|a| Slug::parse(a).ok())
        .filter(|s| s.kind() == Kind::Entry);
    let Some(entry) = entry else {
        let topics: Vec<&str> = about.into_iter().collect();
        return Ok(open_work(&loaded, &topics, &today, all));
    };
    let mut out = open_work(&loaded, &[], &today, all);
    out.items
        .retain(|item| item.entry.as_deref() == Some(entry.path()));
    let open: Vec<&FollowupItem> = out
        .items
        .iter()
        .filter(|i| i.state.as_deref() == Some("open"))
        .collect();
    out.open = open.len();
    out.entries = usize::from(!open.is_empty());
    out.due = open.iter().filter(|i| i.due).count();
    out.without_recheck = open.iter().filter(|i| i.recheck.is_none()).count();
    Ok(out)
}

pub fn forks(deps: &Deps) -> Result<Forks, Failure> {
    let loaded = load::load(deps.store)?;
    Ok(Forks {
        forks: loaded
            .forks
            .iter()
            .map(|(slug, heads)| Fork {
                slug: slug.path().to_owned(),
                heads: heads.iter().map(ToString::to_string).collect(),
            })
            .collect(),
    })
}

/// The index a session opens with: the topics its directory and machine
/// reach, their facts by name, what is due, and what needs a hand.
pub fn context(deps: &Deps, directory: &str) -> Result<Context, Failure> {
    let loaded = load::load(deps.store)?;
    let today = deps.clock.today();
    let machine_name = deps.identity.machine()?.map(|m| m.to_string());
    let machine_topic = machine_name
        .as_deref()
        .and_then(|m| loaded.machine_topic(m));
    let reached = graph::resolve(machine_topic, directory, &deps.home, &loaded.lookup());
    let mut out = Context {
        machine: machine_name,
        directory: directory.to_owned(),
        ..Context::default()
    };
    for r in &reached {
        let Some(topic) = loaded.topics.get(&r.topic) else {
            continue;
        };
        let (ideas, facts): (Vec<_>, Vec<_>) = loaded.facts_of(&r.topic).partition(|f| f.data.idea);
        out.groups.push(Group {
            topic: r.topic.clone(),
            summary: topic.data.summary.clone(),
            distance: r.distance,
            via: match &r.via {
                Via::Claim(_) => "this directory".to_owned(),
                Via::Machine => "this machine".to_owned(),
                Via::Unclaimed => "unclaimed directory".to_owned(),
                Via::Included { from } => format!("via {from}"),
            },
            facts: facts.iter().map(|f| f.slug.name().to_owned()).collect(),
            ideas: ideas.iter().map(|f| f.slug.name().to_owned()).collect(),
        });
    }
    let roots: Vec<&str> = reached
        .iter()
        .filter(|r| matches!(r.via, Via::Claim(_)))
        .map(|r| r.topic.as_str())
        .collect();
    if !roots.is_empty() {
        let work = open_work(&loaded, &roots, &today, false);
        out.open = work.open;
        out.open_entries = work.entries;
        out.without_recheck = work.without_recheck;
        out.due = work.items.into_iter().filter(|item| item.due).collect();
    }
    out.forks = loaded
        .forks
        .iter()
        .map(|(s, _)| s.path().to_owned())
        .collect();
    out.drafts = deps
        .drafts
        .list()?
        .iter()
        .map(|d| d.slug.path().to_owned())
        .collect();
    for slug in loaded.topics.keys() {
        if !reached.iter().any(|r| &r.topic == slug) {
            out.unreached
                .push((slug.clone(), loaded.facts_of(slug).count()));
        }
    }
    Ok(out)
}

/// Every rule the store as a whole has to keep.
pub fn check(deps: &Deps) -> Result<Check, Failure> {
    let loaded = load::load(deps.store)?;
    let mut out = Check::default();
    let mut problem = |slug: &Slug, message: String| {
        out.problems.push(Problem {
            slug: slug.path().to_owned(),
            message,
        });
    };
    for (slug, reason) in &loaded.broken {
        problem(slug, reason.clone());
    }
    out.forks = loaded
        .forks
        .iter()
        .map(|(s, _)| s.path().to_owned())
        .collect();
    let mut machines: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, t) in &loaded.topics {
        for reference in t.data.references() {
            if !loaded.has_topic(reference) {
                problem(&t.slug, format!("names no topic: {reference}"));
            }
        }
        if let Some(machine) = &t.data.machine {
            machines
                .entry(machine.to_string())
                .or_default()
                .push(name.clone());
        }
    }
    for (machine, topics) in machines {
        if topics.len() > 1 {
            for name in &topics {
                problem(
                    &loaded.topics[name].slug,
                    format!(
                        "machine {machine} is bound to {} topics: {}",
                        topics.len(),
                        topics.join(", ")
                    ),
                );
            }
        }
    }
    let touching_unknown = |recheck: &Option<Recheck>| match recheck {
        Some(Recheck::Touching(t)) if !loaded.has_topic(t) => Some(t.clone()),
        _ => None,
    };
    for f in &loaded.facts {
        let topic = f.slug.topic().unwrap_or_default();
        if !loaded.has_topic(topic) {
            problem(&f.slug, format!("sits under no topic: {topic}"));
        }
        if let Some(t) = touching_unknown(&f.data.recheck) {
            problem(&f.slug, format!("touching no topic: {t}"));
        }
    }
    for f in &loaded.followups {
        if !loaded.is_present(&f.data.entry) {
            problem(&f.slug, format!("arose in no live entry: {}", f.data.entry));
        }
        if let Some(t) = touching_unknown(&f.data.recheck) {
            problem(&f.slug, format!("touching no topic: {t}"));
        }
    }
    let linked: Vec<(&Slug, &Version)> = loaded
        .entries
        .iter()
        .map(|d| (&d.slug, &d.version))
        .chain(loaded.facts.iter().map(|d| (&d.slug, &d.version)))
        .chain(loaded.topics.values().map(|d| (&d.slug, &d.version)))
        .chain(loaded.followups.iter().map(|d| (&d.slug, &d.version)))
        .collect();
    out.documents = linked.len();
    for (slug, version) in linked {
        for target in links::targets(&version.content_text()) {
            out.links += 1;
            match Slug::parse(&target) {
                Ok(target) if loaded.is_present(&target) => {}
                Ok(target) => problem(slug, format!("broken link: [[{target}]]")),
                Err(_) => problem(slug, format!("link names no document shape: [[{target}]]")),
            }
        }
    }
    Ok(out)
}

/// The draft against the version it was checked out from.
pub fn diff(deps: &Deps, slug: &Slug) -> Result<Diff, Failure> {
    let draft = load::draft(deps, slug)?;
    let document = deps.store.document(slug)?;
    let missing = draft.parents.iter().find(|p| document.get(p).is_none());
    if let Some(p) = missing {
        return Err(Failure::Refused(format!(
            "draft parent {} is not in the store",
            p.short()
        )));
    }
    let before = match draft.parents.as_slice() {
        [parent] => document
            .get(parent)
            .map(Version::content_text)
            .unwrap_or_default(),
        _ => String::new(),
    };
    let after = crate::domain::frontmatter::emit(&draft.fields, &draft.body);
    let text = similar::TextDiff::from_lines(&before, &after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("{slug} (store)"), &format!("{slug} (draft)"))
        .to_string();
    Ok(Diff {
        slug: slug.path().to_owned(),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::World;
    use crate::app::write;

    fn seed(deps: &Deps) {
        write::put_topic(deps, "lantern", "A Rust app", &[], None).unwrap();
        write::put_topic(deps, "android", "The toolchain", &["phone"], None).unwrap();
        write::put_topic(deps, "phone", "A phone", &[], None).unwrap();
        write::put_topic(deps, "atlas", "An app", &["android"], None).unwrap();
        write::put_machine_topic(
            deps,
            "host",
            "This machine",
            "m1",
            &[
                ("lantern", &["~/projects/lantern"]),
                ("atlas", &["~/projects/Android/atlas"]),
            ],
            &[],
            &[],
        )
        .unwrap();
        write::put_fact(
            deps,
            "lantern/relay-pin-is-fixed",
            "The relay pin is fixed",
            &["lantern"],
            false,
        )
        .unwrap();
        write::put_fact(
            deps,
            "phone/needs-beta",
            "Runs beta builds",
            &["android"],
            true,
        )
        .unwrap();
        write::put_entry(
            deps,
            "2026-09/2026-09-01-first",
            "2026-09-01",
            "Did the first thing",
            &["lantern"],
        )
        .unwrap();
        write::put_followup(
            deps,
            "2026-09-01-port",
            "2026-09/2026-09-01-first",
            "Port it",
            &["lantern"],
            Some("2026-09-03 a month"),
        )
        .unwrap();
        write::put_followup(
            deps,
            "2026-09-01-later",
            "2026-09/2026-09-01-first",
            "Later",
            &["Lantern"],
            None,
        )
        .unwrap();
    }

    #[test]
    fn context_for_a_claimed_directory() {
        let w = World::new("m1");
        let d = w.deps();
        seed(&d);
        let ctx = context(&d, "/home/u/projects/Android/atlas/app").unwrap();
        let topics: Vec<&str> = ctx.groups.iter().map(|g| g.topic.as_str()).collect();
        assert_eq!(topics, ["atlas", "android", "phone", "host"]);
        assert_eq!(ctx.groups[2].ideas, ["needs-beta"]);
        assert_eq!(ctx.unreached, [("lantern".to_owned(), 1)]);
        assert_eq!(ctx.open, 0);
        let ctx = context(&d, "/home/u/projects/lantern").unwrap();
        assert_eq!(ctx.open, 2);
        assert_eq!(ctx.open_entries, 1);
        assert_eq!(ctx.without_recheck, 1);
        assert_eq!(ctx.due.len(), 1);
        assert_eq!(ctx.due[0].slug, "2026-09-01-port");
        assert_eq!(ctx.due[0].label, "due 2026-09-03");
    }

    #[test]
    fn listings_and_search() {
        let w = World::new("m1");
        let d = w.deps();
        seed(&d);
        assert_eq!(recent(&d, 5).unwrap().rows.len(), 1);
        assert_eq!(tag(&d, "lantern").unwrap().rows.len(), 2);
        assert_eq!(
            tags(&d).unwrap().tags,
            [("lantern".to_owned(), 2), ("android".to_owned(), 1)]
        );
        let listing = facts(&d, Some("atlas"), true).unwrap();
        assert!(listing.facts.is_empty());
        assert_eq!(listing.ideas.len(), 1);
        assert_eq!(ideas(&d, None, false).unwrap().rows.len(), 1);
        let hits = search(&d, "FIXED", false).unwrap();
        assert_eq!(hits.hits.len(), 1);
        assert_eq!(hits.hits[0].row.slug, "lantern/relay-pin-is-fixed");
        assert!(search(&d, "fix.d", false).unwrap().hits.is_empty());
        assert_eq!(search(&d, "fix.d", true).unwrap().hits.len(), 1);
        let by_entry = followups(&d, Some("2026-09/2026-09-01-first"), false).unwrap();
        assert_eq!(by_entry.items.len(), 2);
        assert_eq!((by_entry.open, by_entry.entries, by_entry.due), (2, 1, 1));
        let open = followups(&d, Some("lantern"), false).unwrap();
        assert_eq!(open.open, 2);
        assert_eq!(open.due, 1);
        assert_eq!(open.without_recheck, 1);
        let shown = show(&d, &Slug::parse("2026-09/2026-09-01-first").unwrap()).unwrap();
        assert_eq!(shown.followups.len(), 2);
        assert!(!shown.forked);
        assert_eq!(
            where_(&d, "lantern", None).unwrap().dirs,
            ["~/projects/lantern"]
        );
        assert_eq!(check(&d).unwrap().problems, []);
    }

    #[test]
    fn check_reports_dangling_references() {
        let w = World::new("m1");
        let d = w.deps();
        write::put_topic(&d, "a", "A", &["missing"], None).unwrap();
        write::put_fact(&d, "nowhere/x", "Under no topic, links [[a/y]]", &[], false).unwrap();
        let report = check(&d).unwrap();
        let messages: Vec<&str> = report.problems.iter().map(|p| p.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "names no topic: missing",
                "sits under no topic: nowhere",
                "broken link: [[a/y]]"
            ]
        );
        assert_eq!(report.links, 1);
    }
}
