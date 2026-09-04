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

pub fn log(l: &Log) -> String {
    let mut out = String::new();
    for v in &l.versions {
        let _ = writeln!(
            out,
            "{}  {}  {}  {}  {}",
            short(&v.stamp.id),
            v.stamp.written,
            v.stamp.machine,
            v.stamp.operation,
            v.slug
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

/// How one side of a change is painted: the line's tint, the stronger
/// tint of a word that differs, and the colour of its number and sign.
struct Tint {
    line: &'static str,
    word: &'static str,
    mark: &'static str,
}

/// Tuned for a dark terminal, from an editor's diff view.
const REMOVED: Tint = Tint {
    line: "\x1b[48;2;61;1;0m",
    word: "\x1b[48;2;92;2;0m",
    mark: "\x1b[38;2;220;90;90m",
};
const ADDED: Tint = Tint {
    line: "\x1b[48;2;2;40;0m",
    word: "\x1b[48;2;4;71;0m",
    mark: "\x1b[38;2;80;200;80m",
};
const DIM: &str = "\x1b[2m";
const UNCHANGED: Tint = Tint {
    line: "",
    word: "",
    mark: DIM,
};

/// A unified diff of the two sides, three lines of context. Painted, which
/// the caller decides from where stdout goes, it is what an editor shows:
/// a removed or added line on a faint tint running to the edge, the words
/// that differ on a stronger tint where a removed line pairs with an added
/// one, and the line's number and sign in the hue.
pub fn diff(d: &Diff, paint: bool) -> String {
    use similar::ChangeTag;
    const RESET: &str = "\x1b[0m";
    const TO_EDGE: &str = "\x1b[K";
    let diff = similar::TextDiff::from_lines(&d.before.text, &d.after.text);
    let mut unified = diff.unified_diff();
    unified.context_radius(3);
    if !paint {
        return unified.header(&d.before.name, &d.after.name).to_string();
    }
    let mut out = String::new();
    let _ = writeln!(out, "{DIM}--- {}{RESET}", d.before.name);
    let _ = writeln!(out, "{DIM}+++ {}{RESET}", d.after.name);
    for hunk in unified.iter_hunks() {
        let _ = writeln!(out, "{DIM}{}{RESET}", hunk.header());
        for op in hunk.ops() {
            for change in diff.iter_inline_changes(op) {
                let (sign, tint, index) = match change.tag() {
                    ChangeTag::Delete => ('-', &REMOVED, change.old_index()),
                    ChangeTag::Insert => ('+', &ADDED, change.new_index()),
                    ChangeTag::Equal => (' ', &UNCHANGED, change.new_index()),
                };
                let number = index.map_or(String::new(), |i| (i + 1).to_string());
                let _ = write!(
                    out,
                    "{}{}{number:>4} {sign}{RESET}{}",
                    tint.line, tint.mark, tint.line
                );
                for (emphasised, piece) in change.iter_strings_lossy() {
                    let piece = piece.strip_suffix('\n').unwrap_or(&piece);
                    if emphasised {
                        let _ = write!(out, "{}{piece}{}", tint.word, tint.line);
                    } else {
                        out.push_str(piece);
                    }
                }
                let _ = writeln!(out, "{TO_EDGE}{RESET}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::output::Side;

    fn two_sides() -> Diff {
        Diff {
            slug: "lantern".into(),
            before: Side {
                name: "lantern@aaaaaaaaaaaa".into(),
                text: "---\nsummary: s\n---\n\nthe relay pin is fixed\n".into(),
            },
            after: Side {
                name: "lantern@bbbbbbbbbbbb".into(),
                text: "---\nsummary: s\n---\n\nthe relay pin is free\n".into(),
            },
        }
    }

    #[test]
    fn plain_is_a_unified_diff_and_painted_marks_the_word() {
        let plain = diff(&two_sides(), false);
        assert!(plain.starts_with("--- lantern@aaaaaaaaaaaa\n+++ lantern@bbbbbbbbbbbb\n@@ "));
        assert!(plain.contains("-the relay pin is fixed\n+the relay pin is free\n"));
        assert!(!plain.contains('\x1b'));
        let painted = diff(&two_sides(), true);
        assert!(painted.contains(&format!("{}fixed{}", REMOVED.word, REMOVED.line)));
        assert!(painted.contains(&format!("{}free{}", ADDED.word, ADDED.line)));
        assert!(painted.contains("   5 -"), "{painted}");
    }
}
