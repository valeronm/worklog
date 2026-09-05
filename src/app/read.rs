//! The read use cases. None writes anything.

use std::collections::{BTreeMap, BTreeSet};

use crate::domain::entry::Entry;
use crate::domain::fact::Fact;
use crate::domain::followup::Followup;
use crate::domain::graph::{self, Via};
use crate::domain::links;
use crate::domain::recheck::Recheck;
use crate::domain::slug::{Kind, Slug};
use crate::domain::topic::Topic;
use crate::domain::version::{State, Version, VersionId};

use super::load::{self, Doc, Loaded};
use super::output::{
    Check, Claimed, Context, Count, Diff, FactListing, FollowupItem, Followups, Fork, Forks, Group,
    Head, History, HistoryRow, Hit, Listing, Log, LogRow, MachineUsage, Problem, Renamed, Row,
    Search, Shown, Side, Stamp, Tags, TopicRow, Topics, Usage, Where,
};
use super::{Deps, Failure, machine};
use crate::domain::machine::MachineName;

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

fn stamp(version: &Version) -> Stamp {
    Stamp {
        id: version.id.to_string(),
        written: version.block.written.clone(),
        machine: version.block.machine.to_string(),
        operation: version.operation_name().to_owned(),
    }
}

fn head(version: &Version) -> Head {
    Head {
        stamp: stamp(version),
        parents: version
            .block
            .parents
            .iter()
            .map(ToString::to_string)
            .collect(),
        text: version.content_text(),
    }
}

/// When a version was written, as an instant, since machines stamp their
/// own offsets; a stamp that does not parse sorts before everything.
fn instant(version: &Version) -> i64 {
    chrono::DateTime::parse_from_rfc3339(&version.block.written)
        .map_or(i64::MIN, |t| t.timestamp_micros())
}

/// Tags are compared without case, so `Lantern` and `lantern` are one.
fn tag_key(tag: &str) -> String {
    tag.to_ascii_lowercase()
}

fn has_tag(tags: &[String], tag: &str) -> bool {
    tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
}

fn most_used_first(counts: BTreeMap<String, usize>) -> Vec<Count> {
    let mut ranked: Vec<Count> = counts
        .into_iter()
        .map(|(name, count)| Count { name, count })
        .collect();
    ranked.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    ranked
}

/// A document's current text, or every head of a fork; or one stored
/// version, named by its id.
pub fn show(deps: &Deps, name: &str, kind: Option<Kind>) -> Result<Shown, Failure> {
    // A stored version stands on its own; its entry's followups belong to
    // the document as it is now.
    let (slug, heads, foreign) = match load::named(deps, name, kind)? {
        load::Named::Version(v) => (v.slug.clone(), vec![head(&v)], load::foreign_note(&v)),
        load::Named::Slug(slug, document) => {
            let (slug, document) = load::follow(deps.store, slug, document)?;
            let heads: Vec<&Version> = match document.state() {
                State::Live(v) => vec![v],
                State::Forked(heads) => heads,
                State::Absent | State::Tombstoned(_) => {
                    return Err(load::not_live(&slug, &document));
                }
            };
            let foreign = heads.iter().find_map(|v| load::foreign_note(v));
            (slug, heads.into_iter().map(head).collect(), foreign)
        }
    };
    let slug = &slug;
    let followups = if slug.kind() == Kind::Entry && heads.len() == 1 {
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
        foreign,
    })
}

/// Newest first, from the slug a rename moved the document to, back into
/// every document it was moved from.
pub fn history(deps: &Deps, slug: &Slug) -> Result<History, Failure> {
    let document = deps.store.document(slug)?;
    if document.versions.is_empty() {
        return Err(Failure::Refused(format!("no {}: {slug}", slug.kind())));
    }
    let (_, mut document) = load::follow(deps.store, slug.clone(), document)?;
    let foreign = document.current().and_then(load::foreign_note);
    let mut versions = Vec::new();
    loop {
        let ordered = document.history();
        versions.extend(ordered.iter().map(|v| HistoryRow {
            stamp: stamp(v),
            slug: v.slug.path().to_owned(),
            parents: v.block.parents.iter().map(ToString::to_string).collect(),
        }));
        let Some(from) = ordered.last().and_then(|v| v.block.renamed_from.as_ref()) else {
            break;
        };
        document = deps.store.document(from)?;
    }
    Ok(History {
        slug: slug.path().to_owned(),
        versions,
        foreign,
    })
}

/// The newest `n` versions written anywhere in the store, from every
/// machine or from one, ordered by the instant they were written.
pub fn log(deps: &Deps, n: usize, machine_name: Option<&str>) -> Result<Log, Failure> {
    let only = machine_name.map(MachineName::parse).transpose()?;
    // Two stamps can still fall together, so a document's own order,
    // newest first, breaks a tie between its versions.
    let mut rows = Vec::new();
    for (slug, document) in load::documents(deps.store)? {
        for (place, v) in document.history().into_iter().enumerate() {
            if only.as_ref().is_none_or(|m| &v.block.machine == m) {
                let row = LogRow {
                    stamp: stamp(v),
                    slug: slug.path().to_owned(),
                };
                rows.push((std::cmp::Reverse(instant(v)), place, row));
            }
        }
    }
    rows.sort_by_key(|(newest, place, _)| (*newest, *place));
    Ok(Log {
        versions: rows.into_iter().take(n).map(|(_, _, row)| row).collect(),
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
    Ok(Tags {
        tags: most_used_first(counts),
    })
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
            .map(|t| {
                let name = t.slug.path();
                let (ideas, facts): (Vec<_>, Vec<_>) =
                    loaded.facts_of(name).partition(|f| f.data.idea);
                TopicRow {
                    slug: name.to_owned(),
                    summary: t.data.summary.clone(),
                    machine: t.data.machine.as_ref().map(ToString::to_string),
                    includes: t.data.includes.clone(),
                    facts: facts.len(),
                    ideas: ideas.len(),
                }
            })
            .collect(),
    })
}

/// Where a topic, or every topic, lives on this machine or on the machine
/// named. Only this machine's directories can be checked for existence.
pub fn where_(
    deps: &Deps,
    topic: Option<&str>,
    machine_name: Option<&str>,
) -> Result<Where, Failure> {
    let loaded = load::load(deps.store)?;
    if let Some(topic) = topic
        && !loaded.has_topic(topic)
    {
        return Err(Failure::Refused(format!("no topic: {topic}")));
    }
    let here = machine_name.is_none();
    let machine_name = match machine_name {
        Some(m) => m.to_owned(),
        None => machine(deps)?.to_string(),
    };
    let machine_topic = &load::machine_topic(loaded.topics.values(), &machine_name)?.data;
    let claims = machine_topic
        .claims
        .iter()
        .filter(|(t, _)| topic.is_none_or(|topic| t == topic))
        .flat_map(|(t, dirs)| {
            dirs.iter().map(|dir| Claimed {
                topic: t.clone(),
                dir: dir.clone(),
                exists: here.then(|| deps.host.dir_exists(&graph::expand(dir, &deps.home))),
            })
        })
        .collect();
    Ok(Where {
        machine: machine_name,
        claims,
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
            out.unreached.push(Count {
                name: slug.clone(),
                count: loaded.facts_of(slug).count(),
            });
        }
    }
    Ok(out)
}

/// How often each command was run, by the machine that ran it. A date
/// keeps to the lines written on it and after.
pub fn usage(deps: &Deps, machine: Option<&str>, since: Option<&str>) -> Result<Usage, Failure> {
    let only = machine.map(MachineName::parse).transpose()?;
    let mut counted: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for run in deps.usage.all()? {
        if only.as_ref().is_some_and(|m| &run.machine != m) {
            continue;
        }
        if since.is_some_and(|day| run.written.as_str() < day) {
            continue;
        }
        *counted
            .entry(run.machine.to_string())
            .or_default()
            .entry(run.command)
            .or_default() += 1;
    }
    let mut out = Usage::default();
    for (machine, commands) in counted {
        out.machines.push(MachineUsage {
            machine,
            commands: most_used_first(commands),
        });
    }
    Ok(out)
}

/// Every rule the store as a whole has to keep.
pub fn check(deps: &Deps) -> Result<Check, Failure> {
    let loaded = load::load(deps.store)?;
    let mut out = Check::default();
    let mut problem = |slug: &Slug, message: String| out.problems.push(Problem::at(slug, message));
    for (slug, reason) in &loaded.broken {
        problem(slug, reason.clone());
    }
    for (slug, what) in loaded.foreign() {
        out.notices
            .push(Problem::at(slug, load::foreign_reason(what)));
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
    check_links(&loaded, &mut out);
    Ok(out)
}

/// Every link in every live document and every tombstone's note, since
/// the link to where a document ended is the one a reader follows.
fn check_links(loaded: &Loaded, out: &mut Check) {
    let linked: Vec<(&Slug, &Version)> = loaded
        .entries
        .iter()
        .map(|d| (&d.slug, &d.version))
        .chain(loaded.facts.iter().map(|d| (&d.slug, &d.version)))
        .chain(loaded.topics.values().map(|d| (&d.slug, &d.version)))
        .chain(loaded.followups.iter().map(|d| (&d.slug, &d.version)))
        .collect();
    let noted: Vec<(&Slug, &str)> = loaded
        .removed()
        .filter_map(|(slug, note)| note.map(|n| (slug, n)))
        .collect();
    out.documents = linked.len() + noted.len();
    let mut inbound: BTreeMap<Slug, usize> = BTreeMap::new();
    let mut stale: BTreeSet<(Slug, Slug)> = BTreeSet::new();
    let mut scan = |slug: &Slug, text: &str| {
        for target in links::targets(text) {
            out.links += 1;
            let Ok(target) = Slug::parse(&target) else {
                out.problems.push(Problem::at(
                    slug,
                    format!("link names no document shape: [[{target}]]"),
                ));
                continue;
            };
            match loaded.landing(&target) {
                load::Landing::Present => {}
                load::Landing::Removed(removed) => {
                    *inbound.entry(removed.clone()).or_default() += 1;
                    if !slug.kind().cites() {
                        stale.insert((slug.clone(), target));
                    }
                }
                load::Landing::Missing => {
                    out.problems
                        .push(Problem::at(slug, format!("broken link: [[{target}]]")));
                }
            }
        }
    };
    for (slug, version) in &linked {
        scan(slug, &version.content_text());
    }
    for (slug, note) in noted {
        scan(slug, note);
    }
    for (slug, target) in stale {
        out.notices.push(Problem::at(
            &slug,
            format!("links a removed document: [[{target}]]"),
        ));
    }
    for (removed, note) in loaded.removed() {
        if let (None, Some(links)) = (note, inbound.get(removed)) {
            out.notices.push(Problem::at(
                removed,
                format!("removed with no note saying why, linked from {links}"),
            ));
        }
    }
}

/// The draft against the version it was checked out from.
fn at(v: &Version) -> Side {
    Side {
        name: format!("{}@{}", v.slug, v.id.short()),
        text: v.content_text(),
    }
}

/// A draft against the version it came from, when given a slug; a stored
/// version against its parent, when given an id; or two stored versions,
/// the earlier on the left whichever was named first.
pub fn diff(
    deps: &Deps,
    name: &str,
    other: Option<&str>,
    kind: Option<Kind>,
) -> Result<Diff, Failure> {
    let first = load::named(deps, name, kind)?;
    let second = other.map(|o| load::named(deps, o, kind)).transpose()?;
    let (slug, before, after, renamed) = match (first, second) {
        (load::Named::Slug(slug, document), None) => {
            let draft = load::draft(deps, &slug)?;
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
            let before = Side {
                name: format!("{slug} (store)"),
                text: before,
            };
            let after = Side {
                name: format!("{slug} (draft)"),
                text: after,
            };
            (slug, before, after, None)
        }
        (load::Named::Slug(slug, _), Some(_)) | (_, Some(load::Named::Slug(slug, _))) => {
            return Err(Failure::Usage(format!(
                "{slug} is a document; two sides of a diff are version ids"
            )));
        }
        (load::Named::Version(v), None) => {
            let parent = load::parent(deps.store, &v)?;
            let before = parent.as_ref().map_or_else(
                || Side {
                    name: "(none)".to_owned(),
                    text: String::new(),
                },
                at,
            );
            let renamed = v.rename_sides().map(|(from, to)| Renamed {
                from: from.path().to_owned(),
                to: to.path().to_owned(),
            });
            let after = at(&v);
            (v.slug, before, after, renamed)
        }
        (load::Named::Version(a), Some(load::Named::Version(b))) => {
            // Within one document the chain says which came first
            // whatever the clocks did; across documents only the instant
            // can.
            let a_is_older = if a.slug == b.slug {
                let order = deps.store.document(&a.slug)?.history_ids();
                let place = |id: &VersionId| order.iter().position(|o| o == id);
                place(&a.id) >= place(&b.id)
            } else {
                instant(&a) <= instant(&b)
            };
            let (older, newer) = if a_is_older { (a, b) } else { (b, a) };
            let (before, after) = (at(&older), at(&newer));
            (newer.slug, before, after, None)
        }
    };
    Ok(Diff {
        slug: slug.path().to_owned(),
        before,
        after,
        renamed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::World;
    use crate::app::write;

    fn counted(name: &str, count: usize) -> Count {
        Count {
            name: name.to_owned(),
            count,
        }
    }

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
        assert_eq!(ctx.unreached, [counted("lantern", 1)]);
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
            [counted("lantern", 2), counted("android", 1)]
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
        let shown = show(&d, "2026-09/2026-09-01-first", None).unwrap();
        assert_eq!(shown.followups.len(), 2);
        assert!(!shown.forked);
        let lantern = where_(&d, Some("lantern"), None).unwrap().claims;
        assert_eq!(lantern.len(), 1);
        assert_eq!(lantern[0].dir, "~/projects/lantern");
        assert_eq!(lantern[0].exists, Some(true));
        let all = where_(&d, None, None).unwrap().claims;
        assert!(all.len() > 1, "{all:?}");
        assert!(all.iter().any(|c| c.exists == Some(false)), "{all:?}");
        let elsewhere = where_(&d, None, Some("m1")).unwrap().claims;
        assert!(
            elsewhere.iter().all(|c| c.exists.is_none()),
            "{elsewhere:?}"
        );
        assert_eq!(check(&d).unwrap().problems, []);
    }

    #[test]
    fn usage_counts_each_command_under_its_machine() {
        use crate::domain::ports::Usage as _;
        use crate::domain::usage::Invocation;

        let w = World::new("m1");
        for (machine, command, day) in [
            ("desk", "context", "2026-09-01"),
            ("desk", "context", "2026-09-02"),
            ("desk", "show", "2026-09-02"),
            ("phone", "context", "2026-09-02"),
        ] {
            w.usage
                .record(&Invocation {
                    written: format!("{day}T10:00:00.000001+01:00"),
                    machine: MachineName::parse(machine).unwrap(),
                    command: command.to_owned(),
                    exit: 0,
                    directory: "~/projects/lantern".into(),
                    arguments: vec![],
                })
                .unwrap();
        }
        let d = &w.deps();
        let all = usage(d, None, None).unwrap();
        assert_eq!(all.machines.len(), 2);
        assert_eq!(all.machines[0].machine, "desk");
        assert_eq!(
            all.machines[0].commands,
            [counted("context", 2), counted("show", 1)]
        );
        let phone = usage(d, Some("phone"), None).unwrap();
        assert_eq!(phone.machines.len(), 1);
        assert_eq!(phone.machines[0].commands[0].count, 1);
        let recent = usage(d, None, Some("2026-09-02")).unwrap();
        assert_eq!(
            recent.machines[0].commands,
            [counted("context", 1), counted("show", 1)]
        );
        assert!(usage(d, Some("nobody"), None).unwrap().machines.is_empty());
    }

    #[test]
    fn check_lists_a_linked_tombstone_that_says_nothing() {
        use crate::domain::ports::Store;
        use crate::domain::version::Operation;
        let w = World::new("m1");
        let d = w.deps();
        write::put_topic(&d, "lantern", "A lamp", &[], None).unwrap();
        write::put_fact(
            &d,
            "lantern/relay",
            "The relay is fixed",
            &["lantern"],
            false,
        )
        .unwrap();
        write::put_fact(
            &d,
            "lantern/timing",
            "After [[lantern/relay]]",
            &["lantern"],
            false,
        )
        .unwrap();
        let slug = Slug::parse("lantern/relay").unwrap();
        let head = w.store.document(&slug).unwrap().current().unwrap().clone();
        // The tombstone an older binary wrote: no note in its body.
        write::store_version(
            &d,
            slug,
            vec![head.id.clone()],
            Operation::Tombstone,
            head.fields,
            String::new(),
            None,
            None,
        )
        .unwrap();
        let report = check(&d).unwrap();
        let notices: Vec<&str> = report.notices.iter().map(|n| n.message.as_str()).collect();
        assert_eq!(
            notices,
            [
                "links a removed document: [[lantern/relay]]",
                "removed with no note saying why, linked from 1"
            ]
        );
        assert!(report.problems.is_empty());
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
