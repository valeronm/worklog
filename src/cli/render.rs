//! Text renderings of the outputs. Each returns the exact stdout, so a
//! golden can pin it.

#![allow(
    clippy::must_use_candidate,
    reason = "every function here is a renderer whose String is the whole point of calling it"
)]

use std::fmt::Write as _;

use crate::app::output::{
    Check, Context, Diff, DraftList, DraftRef, FactListing, FollowupItem, Followups, Forks,
    History, Listing, Log, Row, Search, Shown, Tags, Topics, Where, Written,
};

pub const IDEAS_HEADING: &str = "Ideas — unbuilt, kept with their settled design:";

/// Enough of a version id to tell versions of one document apart.
fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

/// Parent ids on one line, or `none` for a first version.
fn parents(ids: &[String]) -> String {
    if ids.is_empty() {
        "none".to_owned()
    } else {
        ids.iter().map(|p| short(p)).collect::<Vec<_>>().join(", ")
    }
}

fn row(out: &mut String, r: &Row) {
    let _ = writeln!(out, "● {}  {}\n  {}", r.date, r.summary, r.slug);
}

pub fn listing(l: &Listing) -> String {
    let mut out = String::new();
    for r in &l.rows {
        row(&mut out, r);
    }
    out
}

pub fn fact_listing(l: &FactListing) -> String {
    let mut out = String::new();
    for r in &l.facts {
        row(&mut out, r);
    }
    if !l.ideas.is_empty() {
        if !l.facts.is_empty() {
            out.push('\n');
        }
        out.push_str(IDEAS_HEADING);
        out.push('\n');
        for r in &l.ideas {
            row(&mut out, r);
        }
    }
    out
}

fn followup_line(out: &mut String, item: &FollowupItem) {
    let _ = writeln!(out, "- ({}) {}", item.label, item.summary);
    match &item.entry {
        Some(entry) => {
            let _ = writeln!(out, "    {} — in {entry}", item.slug);
        }
        None => {
            let _ = writeln!(out, "    {} — {}", item.slug, item.source);
        }
    }
}

pub fn shown(s: &Shown) -> String {
    let mut out = String::new();
    for head in &s.heads {
        if s.forked {
            let _ = writeln!(
                out,
                "==== head {} — {} on {} by {}",
                short(&head.stamp.id),
                head.stamp.operation,
                head.stamp.machine,
                head.stamp.written
            );
        }
        out.push_str(&head.text);
    }
    if !s.followups.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\n## Follow-ups\n");
        for item in &s.followups {
            let mark = if item.state.as_deref() == Some("open") {
                " "
            } else {
                "x"
            };
            let _ = writeln!(
                out,
                "- [{mark}] {} ({}) — {}",
                item.summary, item.label, item.slug
            );
        }
    }
    out
}

pub fn history(h: &History) -> String {
    let mut out = String::new();
    for v in &h.versions {
        let _ = writeln!(
            out,
            "{}  {}  {}  {}  parents: {}",
            short(&v.stamp.id),
            v.stamp.written,
            v.stamp.machine,
            v.stamp.operation,
            parents(&v.parents)
        );
    }
    out
}

/// No id: the line names what changed and who, and `history` on the slug
/// has the ids.
pub fn log(l: &Log) -> String {
    let mut out = String::new();
    for v in &l.versions {
        let _ = writeln!(
            out,
            "{}  {}  {}  {}",
            v.stamp.written, v.stamp.machine, v.stamp.operation, v.slug
        );
    }
    out
}

pub fn search(s: &Search) -> String {
    let mut out = String::new();
    for hit in &s.hits {
        row(&mut out, &hit.row);
        for (n, line) in &hit.lines {
            let _ = writeln!(out, "    {n}:{line}");
        }
    }
    out
}

pub fn tags(t: &Tags) -> String {
    let mut out = String::new();
    for (tag, count) in &t.tags {
        let _ = writeln!(out, "{count:>7} {tag}");
    }
    out
}

pub fn followups(f: &Followups) -> String {
    let mut out = String::new();
    let mut facts_started = false;
    for item in &f.items {
        if item.source != "followup" && !facts_started {
            facts_started = true;
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("Facts and ideas with a recheck of their own:\n");
        }
        followup_line(&mut out, item);
    }
    if f.open > 0 || !f.items.is_empty() {
        let _ = writeln!(
            out,
            "\n{} open in {} entries, {} due, {} without recheck",
            f.open, f.entries, f.due, f.without_recheck
        );
    }
    out
}

pub fn topics(t: &Topics) -> String {
    let mut out = String::new();
    for topic in &t.topics {
        let _ = writeln!(out, "{} — {}", topic.slug, topic.summary);
    }
    out
}

/// One topic's directories bare; every topic's with the topic in front.
pub fn where_(w: &Where, topic: Option<&str>) -> String {
    let mut out = String::new();
    let width = w.claims.iter().map(|c| c.topic.len()).max().unwrap_or(0);
    for c in &w.claims {
        if topic.is_none() {
            let _ = write!(out, "{:width$}  ", c.topic);
        }
        let _ = write!(out, "{}", c.dir);
        if c.exists == Some(false) {
            let _ = write!(out, " (missing)");
        }
        let _ = writeln!(out);
    }
    if w.claims.is_empty() {
        let _ = match topic {
            Some(_) => writeln!(
                out,
                "no directory on {} — a device, not a checkout",
                w.machine
            ),
            None => writeln!(out, "no claims on {}", w.machine),
        };
    }
    out
}

fn cut(text: &str, chars: usize) -> String {
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    let kept: String = text.chars().take(chars - 1).collect();
    format!("{}…", kept.trim_end())
}

/// Comma-joined names wrapped at 78 columns, two spaces in, as an index
/// a session reads rather than a listing it scrolls.
fn wrapped(out: &mut String, names: &[String]) {
    let mut line = String::from(" ");
    for (i, name) in names.iter().enumerate() {
        let piece = if i + 1 == names.len() {
            format!(" {name}")
        } else {
            format!(" {name},")
        };
        if line.len() + piece.len() > 78 && line.len() > 1 {
            out.push_str(&line);
            out.push('\n');
            line = String::from(" ");
        }
        line.push_str(&piece);
    }
    if line.len() > 1 {
        out.push_str(&line);
        out.push('\n');
    }
}

pub fn context(c: &Context) -> String {
    let mut out = String::new();
    let Some(machine) = &c.machine else {
        out.push_str(
            "No machine name: `worklog init <name>` before the store can place this directory.\n",
        );
        return out;
    };
    if c.groups.is_empty() {
        let _ = writeln!(
            out,
            "No topic carries `machine: {machine}`, so nothing reaches this directory."
        );
        return out;
    }
    out.push_str(
        "Durable facts and ideas, by name — `worklog facts <topic>` for what each\nclaims, `worklog show <topic>/<name>` for one whole.\n",
    );
    for g in &c.groups {
        let _ = writeln!(out, "\n{} — {}:", g.topic, g.via);
        if g.facts.is_empty() && g.ideas.is_empty() {
            out.push_str("  (no facts)\n");
        }
        wrapped(&mut out, &g.facts);
        if !g.ideas.is_empty() {
            out.push_str("Ideas — unbuilt, kept with their settled design; opened like a fact:\n");
            wrapped(&mut out, &g.ideas);
        }
    }
    if c.open > 0 {
        let _ = writeln!(
            out,
            "\n{} open follow-ups in {} entries here, {} without recheck — `worklog followups <topic>`",
            c.open, c.open_entries, c.without_recheck
        );
    }
    if !c.due.is_empty() {
        out.push_str("due now:\n");
        for item in &c.due {
            // An index names the item; `worklog show <slug>` has the rest.
            let brief = FollowupItem {
                summary: cut(&item.summary, 96),
                ..item.clone()
            };
            followup_line(&mut out, &brief);
        }
    }
    if !c.forks.is_empty() {
        let _ = writeln!(
            out,
            "\nForked, needing `worklog resolve`: {}",
            c.forks.join(", ")
        );
    }
    if !c.drafts.is_empty() {
        let _ = writeln!(
            out,
            "\nDrafts left open on this machine — `worklog drafts`: {}",
            c.drafts.join(", ")
        );
    }
    if !c.unreached.is_empty() {
        out.push_str(
            "\nNot reached here, with fact counts — `worklog topics` says what each is:\n",
        );
        let names: Vec<String> = c
            .unreached
            .iter()
            .map(|(t, n)| format!("{t} ({n})"))
            .collect();
        wrapped(&mut out, &names);
    }
    out
}

pub fn forks(f: &Forks) -> String {
    let mut out = String::new();
    for fork in &f.forks {
        let _ = writeln!(out, "{}: {}", fork.slug, parents(&fork.heads));
    }
    out
}

pub fn check(c: &Check) -> String {
    let mut out = String::new();
    for p in &c.problems {
        let _ = writeln!(out, "{}: {}", p.slug, p.message);
    }
    for f in &c.forks {
        let _ = writeln!(out, "{f}: forked");
    }
    let _ = writeln!(
        out,
        "check: {} documents, {} links, {} problems, {} forks",
        c.documents,
        c.links,
        c.problems.len(),
        c.forks.len()
    );
    out
}

pub fn written(w: &Written) -> String {
    match &w.tombstone {
        Some(stone) => format!(
            "{}\nmoved to {}; the old slug's tombstone is {}\n",
            w.id, w.slug, stone
        ),
        None => format!("{}\n", w.id),
    }
}

pub fn draft_ref(d: &DraftRef) -> String {
    format!("{}\n", d.location)
}

pub fn drafts(d: &DraftList) -> String {
    let mut out = String::new();
    for draft in &d.drafts {
        let _ = writeln!(
            out,
            "{}  {}  parents: {}",
            draft.location,
            draft.slug,
            parents(&draft.parents)
        );
    }
    out
}

pub fn diff(d: &Diff) -> String {
    d.text.clone()
}
