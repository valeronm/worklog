//! Text renderings of the outputs. Each returns the exact stdout, so a
//! golden can pin it.

#![allow(
    clippy::must_use_candidate,
    reason = "every function here is a renderer whose String is the whole point of calling it"
)]

use std::fmt::Write as _;

use crate::app::output::{
    Check, Context, Diff, DraftList, DraftRef, FactListing, FollowupItem, Followups, Forks, Group,
    History, Listing, Log, Row, Search, Shown, Tags, Topics, Usage, Where, Written, short,
};

pub const IDEAS_HEADING: &str = "Ideas — unbuilt, kept with their settled design:";

/// Parent ids on one line, or `none` for a first version.
fn parents(ids: &[String]) -> String {
    if ids.is_empty() {
        "none".to_owned()
    } else {
        ids.iter().map(|p| short(p)).collect::<Vec<_>>().join(", ")
    }
}

fn count_line(out: &mut String, count: usize, name: &str) {
    let _ = writeln!(out, "{count:>7} {name}");
}

fn row(out: &mut String, r: &Row) {
    let _ = writeln!(out, "● {}  {}\n  {}", r.date, r.summary, r.slug);
}

pub fn listing(l: &Listing) -> String {
    rows(&l.rows)
}

pub fn rows(rows: &[Row]) -> String {
    let mut out = String::new();
    for r in rows {
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

fn followup_line(out: &mut String, item: &FollowupItem, summary: &str) {
    let _ = writeln!(out, "- ({}) {}", item.label, summary);
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
    match &s.removed {
        Some(note) if note.is_empty() => {
            let _ = writeln!(out, "{} was removed", s.slug);
        }
        Some(note) => {
            let _ = writeln!(out, "{} was removed: {note}", s.slug);
        }
        None => {
            for head in &s.heads {
                if s.forked {
                    let _ = writeln!(
                        out,
                        "==== head {} — {} on {} by {}",
                        head.stamp.short(),
                        head.stamp.operation,
                        head.stamp.machine,
                        head.stamp.written_to_millis()
                    );
                }
                out.push_str(&head.text);
            }
        }
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
        let (sep, moved) = if v.slug == h.slug {
            ("", "")
        } else {
            ("  as ", v.slug.as_str())
        };
        let _ = writeln!(
            out,
            "{}  {}  {}  {}  parents: {}{sep}{moved}",
            v.stamp.short(),
            v.stamp.written_to_millis(),
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
            v.stamp.short(),
            v.stamp.written_to_millis(),
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
    for tag in &t.tags {
        count_line(&mut out, tag.count, &tag.name);
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
        followup_line(&mut out, item, &item.summary);
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
    if names.is_empty() {
        return;
    }
    out.push_str("  ");
    out.push_str(&wrap(&names.join(", "), 2, 78));
    out.push('\n');
}

/// The words of `text` folded so no line passes `columns`, every line
/// after the first indented to sit under the first, which the caller
/// places at `indent`.
pub fn wrap(text: &str, indent: usize, columns: usize) -> String {
    let room = columns.saturating_sub(indent);
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= room => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    lines.join(&format!("\n{}", " ".repeat(indent)))
}

/// Claude Code replaces a hook's output longer than 10,000 characters with
/// its first 2,000, and a character never takes fewer than a byte.
pub const CONTEXT_BYTES: usize = 10_000;

/// A topic's block in `context`, its facts and its ideas each by name or
/// as a count.
fn group(g: &Group, facts: bool, ideas: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} — {}:", g.topic, g.via);
    if !g.facts.is_empty() {
        if facts {
            wrapped(&mut out, &g.facts);
        } else {
            let _ = writeln!(out, "  ({})", many(g.facts.len(), "fact", "facts"));
        }
    } else if g.ideas.is_empty() {
        out.push_str("  (no facts)\n");
    }
    if !g.ideas.is_empty() {
        if ideas {
            out.push_str("Ideas — unbuilt, kept with their settled design; opened like a fact:\n");
            wrapped(&mut out, &g.ideas);
        } else {
            let _ = writeln!(out, "  ({})", many(g.ideas.len(), "idea", "ideas"));
        }
    }
    out
}

/// A count and the word it counts, in the number the count calls for.
fn many(n: usize, one: &str, more: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { more })
}

/// One rung for each thing `context` can give up.
fn rungs(c: &Context) -> usize {
    c.groups.len() * 2 + c.due.len()
}

/// `context` with what `rung` gives up left out: fact names go first,
/// topic by topic, then idea names, then the oldest due items.
fn page(c: &Context, rung: usize) -> String {
    let topics = c.groups.len();
    let naming_facts = topics.saturating_sub(rung);
    let naming_ideas = topics.saturating_sub(rung.saturating_sub(topics));
    let dropped = rung.saturating_sub(topics * 2).min(c.due.len());
    let mut sections: Vec<String> = Vec::new();
    let mut work = String::new();
    if c.open > 0 {
        let _ = writeln!(
            work,
            "{} in {} here, {} without recheck — `worklog followups <topic>`",
            many(c.open, "open follow-up", "open follow-ups"),
            many(c.open_entries, "entry", "entries"),
            c.without_recheck
        );
    }
    if dropped < c.due.len() {
        work.push_str("due now:\n");
        for item in &c.due[dropped..] {
            // An index names the item; `worklog show <slug>` has the rest.
            followup_line(&mut work, item, &cut(&item.summary, 96));
        }
    }
    if dropped > 0 {
        // What a cut drops is the oldest, an item already passed over.
        let _ = writeln!(
            work,
            "{} due — `worklog followups <topic>`",
            many(dropped, "older item", "older items")
        );
    }
    if !work.is_empty() {
        sections.push(work);
    }
    if !c.forks.is_empty() {
        sections.push(format!(
            "Forked, needing `worklog resolve`: {}\n",
            c.forks.join(", ")
        ));
    }
    if !c.drafts.is_empty() {
        sections.push(format!(
            "Drafts left open on this machine — `worklog drafts`: {}\n",
            c.drafts.join(", ")
        ));
    }
    sections.push(
        "Durable facts and ideas, by name where they fit and counted where not —\n`worklog facts <topic>` for their claims, `worklog show <topic>/<name>` for one.\n"
            .to_owned(),
    );
    for (n, g) in c.groups.iter().enumerate() {
        sections.push(group(g, n < naming_facts, n < naming_ideas));
    }
    if !c.unreached.is_empty() {
        sections.push(format!(
            "{} — `worklog topics` says what each is\n",
            many(c.unreached.len(), "other topic", "other topics")
        ));
    }
    sections.join("\n")
}

pub fn context(c: &Context) -> String {
    let Some(machine) = &c.machine else {
        return "No machine name: `worklog init <name>` before the store can place this directory.\n"
            .to_owned();
    };
    if c.groups.is_empty() {
        return format!(
            "No topic carries `machine: {machine}`, so nothing reaches this directory.\n"
        );
    }
    let last = rungs(c);
    let (mut low, mut high) = (0, last);
    let mut fitting = None;
    while low < high {
        let rung = usize::midpoint(low, high);
        let out = page(c, rung);
        if out.len() <= CONTEXT_BYTES {
            high = rung;
            fitting = Some(out);
        } else {
            low = rung + 1;
        }
    }
    // The last rung gives up everything there is, so it is the answer
    // whether or not it fits.
    fitting.unwrap_or_else(|| page(c, last))
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
    for n in &c.notices {
        let _ = writeln!(out, "notice: {}: {}", n.slug, n.message);
    }
    let _ = writeln!(
        out,
        "check: {} documents, {} links, {} problems, {} forks, {} notices",
        c.documents,
        c.links,
        c.problems.len(),
        c.forks.len(),
        c.notices.len()
    );
    out
}

pub fn usage(u: &Usage) -> String {
    let mut out = String::new();
    for machine in &u.machines {
        if !out.is_empty() {
            out.push('\n');
        }
        let runs: usize = machine.commands.iter().map(|c| c.count).sum();
        let _ = writeln!(out, "{} — {runs} runs", machine.machine);
        for c in &machine.commands {
            count_line(&mut out, c.count, &c.name);
        }
    }
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
    let (dim, reset) = if paint { (DIM, RESET) } else { ("", "") };
    let mut out = String::new();
    let _ = writeln!(out, "{dim}--- {}{reset}", d.before.name);
    let _ = writeln!(out, "{dim}+++ {}{reset}", d.after.name);
    // A rename copies the content, so its versions report the move rather
    // than a body leaving and returning.
    if let Some(renamed) = &d.renamed {
        let _ = writeln!(out, "renamed from {} to {}", renamed.from, renamed.to);
        return out;
    }
    let diff = similar::TextDiff::from_lines(&d.before.text, &d.after.text);
    let mut unified = diff.unified_diff();
    unified.context_radius(3);
    if !paint {
        out.push_str(&unified.to_string());
        return out;
    }
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
    use crate::app::output::{Count, Side};

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
            renamed: None,
        }
    }

    fn topic(name: &str, facts: usize, ideas: usize) -> Group {
        Group {
            topic: name.into(),
            summary: String::new(),
            distance: 0,
            via: "this directory".into(),
            facts: (0..facts)
                .map(|n| format!("relay-pin-{n}-is-fixed"))
                .collect(),
            ideas: (0..ideas).map(|n| format!("idea-{n}-is-unbuilt")).collect(),
        }
    }

    fn due_item(n: usize) -> FollowupItem {
        FollowupItem {
            slug: format!("2026-09-01-port-{n}"),
            source: "followup".into(),
            entry: Some("2026-09/2026-09-01-lamp-driver".into()),
            state: Some("open".into()),
            summary: format!("Add relay {n} to the board and wire its driver"),
            recheck: None,
            label: "due 2026-01-01".into(),
            due: true,
        }
    }

    #[test]
    fn context_counts_the_facts_whose_names_would_not_fit() {
        let c = Context {
            machine: Some("m1".into()),
            groups: vec![
                topic("lantern", 3, 1),
                topic("atlas", 600, 1),
                topic("phone", 2, 1),
            ],
            open: 1,
            open_entries: 1,
            due: vec![due_item(1)],
            ..Context::default()
        };
        let out = context(&c);
        assert!(out.len() <= CONTEXT_BYTES, "{out}");
        assert!(out.starts_with("1 open follow-up in 1 entry here"), "{out}");
        assert!(
            out.contains("lantern — this directory:\n  relay-pin-0-is-fixed, "),
            "{out}"
        );
        assert!(
            out.contains("atlas — this directory:\n  (600 facts)\nIdeas"),
            "{out}"
        );
        assert!(
            out.contains("phone — this directory:\n  (2 facts)\nIdeas"),
            "{out}"
        );
        assert!(out.ends_with("  idea-0-is-unbuilt\n"), "{out}");
    }

    #[test]
    fn ideas_go_to_counts_before_the_due_list_is_cut() {
        let c = Context {
            machine: Some("m1".into()),
            groups: vec![topic("lantern", 4, 300)],
            open: 90,
            open_entries: 40,
            due: (0..90).map(due_item).collect(),
            ..Context::default()
        };
        let out = context(&c);
        assert!(out.len() <= CONTEXT_BYTES, "{}", out.len());
        assert!(
            out.contains("lantern — this directory:\n  (4 facts)\n  (300 ideas)"),
            "{out}"
        );
        // The newest survive a cut, so the item opened last is always there.
        assert!(out.contains("Add relay 89 to the board"), "{out}");
        assert!(!out.contains("Add relay 0 to the board"), "{out}");
        let cut = out
            .lines()
            .find(|l| l.contains("older items due"))
            .expect("the count of what was cut");
        let dropped: usize = cut.split(' ').next().unwrap().parse().unwrap();
        assert_eq!(dropped + out.matches("— in 2026-09/").count(), 90, "{out}");
    }

    #[test]
    fn the_search_lands_where_a_rung_by_rung_walk_would() {
        let c = Context {
            machine: Some("m1".into()),
            groups: vec![
                topic("lantern", 120, 200),
                topic("atlas", 40, 1),
                topic("phone", 1, 1),
            ],
            open: 60,
            open_entries: 30,
            due: (0..60).map(due_item).collect(),
            unreached: vec![Count {
                name: "personal".into(),
                count: 2,
            }],
            ..Context::default()
        };
        let walked = (0..=rungs(&c))
            .map(|rung| page(&c, rung))
            .find(|out| out.len() <= CONTEXT_BYTES)
            .expect("a rung that fits");
        assert_eq!(context(&c), walked);
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
