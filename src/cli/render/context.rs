use std::fmt::Write as _;

use super::listings::followup_line;
use super::{Render, filed_counts};
use crate::app::context::{Context, Loaded, Part, Via};
use crate::app::{Row, counted};

// An agent keeps only the start of a session hook's output longer than 10,000 characters,
// and a character never takes fewer than a byte.
pub(in crate::cli) const CONTEXT_BYTES: usize = 10_000;

// The topics and the due lines give way for the line of fork labels, which no `Step` shortens.
const FORK_LABEL_BYTES: usize = 800;
const COLUMNS: usize = 78;
const SUMMARY_CHARS: usize = 96;
const DUE: &str = "due now — `worklog show <id>` for one, `worklog due`, run here, for all:\n";
const CLOSING: &str = "Durable facts and ideas, by name where they fit and counted where not —\n\
     `worklog facts <topic>` for a topic's facts,\n\
     `worklog ideas <topic>` for a topic's ideas,\n\
     `worklog show <topic>/<name>` for one,\n\
     `worklog followups <topic>` for open work.\n";

fn cut(text: &str) -> String {
    if text.chars().count() <= SUMMARY_CHARS {
        return text.to_owned();
    }
    let kept: String = text.chars().take(SUMMARY_CHARS - 1).collect();
    format!("{}…", kept.trim_end())
}

fn wrapped(items: &[&str]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (at, item) in items.iter().enumerate() {
        let comma = if at + 1 < items.len() { "," } else { "" };
        let width = item.chars().count() + comma.len();
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + width <= COLUMNS => {
                let _ = write!(line, " {item}{comma}");
            }
            _ => lines.push(format!("  {item}{comma}")),
        }
    }
    let mut text = String::new();
    for line in lines {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

fn count_line(count: usize, one: &str, more: &str) -> String {
    format!("  ({})\n", counted(count, one, more))
}

fn names(rows: &[Row]) -> Vec<&str> {
    rows.iter()
        .map(|row| row.name.as_deref().unwrap_or(&row.label))
        .collect()
}

fn part(part: &Part) -> String {
    match filed_counts(part.fact_count, part.idea_count, part.open_count) {
        Some(counts) => format!("{} {counts}", part.label),
        None => part.label.clone(),
    }
}

fn via(via: &Via) -> String {
    match via {
        Via::Claim {
            directory: Some(directory),
            ..
        } => format!("claimed at {directory}"),
        Via::Claim {
            directory: None, ..
        } => "claimed anywhere".to_owned(),
        Via::Edge { from } | Via::Machine { from: Some(from) } => format!("via {}", from.label),
        Via::Machine { from: None } => "this machine".to_owned(),
    }
}

#[derive(Default)]
struct Listed {
    named: String,
    counted: String,
}

fn listed(heading: &str, items: &[&str], one: &str, more: &str) -> Listed {
    if items.is_empty() {
        return Listed::default();
    }
    Listed {
        named: format!("{heading}{}", wrapped(items)),
        counted: count_line(items.len(), one, more),
    }
}

const FORMS: usize = 4;

fn forms(topic: &Loaded) -> [String; FORMS] {
    let mut head = format!("{} — {}:", topic.label, via(&topic.via));
    if !topic.summary.is_empty() {
        let _ = write!(head, " {}", cut(&topic.summary));
    }
    head.push('\n');
    let facts = if topic.facts.is_empty() && topic.ideas.is_empty() {
        Listed {
            named: "  (no facts)\n".to_owned(),
            counted: "  (no facts)\n".to_owned(),
        }
    } else {
        listed("", &names(&topic.facts), "fact", "facts")
    };
    let ideas = listed(
        "Ideas — unbuilt, kept with their settled design; opened like a fact:\n",
        &names(&topic.ideas),
        "idea",
        "ideas",
    );
    let parts: Vec<String> = topic.parts.iter().map(part).collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let parts = listed(
        "Parts, with their facts, ideas and open followups:\n",
        &parts,
        "part",
        "parts",
    );
    let lists = [facts, ideas, parts];
    std::array::from_fn(|counted| {
        let mut form = head.clone();
        for (at, list) in lists.iter().enumerate() {
            form.push_str(if at < counted {
                &list.counted
            } else {
                &list.named
            });
        }
        form
    })
}

fn more_due(count: usize) -> String {
    let lists = if count == 1 {
        "lists it"
    } else {
        "lists them all"
    };
    format!("{count} more due — `worklog due`, run here, {lists}\n")
}

fn forks(rows: &[Row]) -> String {
    let labels: Vec<&str> = rows.iter().map(|row| &*row.label).collect();
    let labels = labels.join(", ");
    if labels.len() <= FORK_LABEL_BYTES {
        return format!(
            "Forked — `worklog forks` lists each with its id, \
             `worklog resolve <id>` opens one: {labels}\n"
        );
    }
    let lists = if rows.len() == 1 {
        "lists it with its id"
    } else {
        "lists each with its id"
    };
    format!(
        "{} — `worklog forks` {lists}\n",
        counted(rows.len(), "forked document", "forked documents")
    )
}

struct Page {
    due: Vec<String>,
    forks: Option<String>,
    counts: Vec<String>,
    topics: Vec<[String; FORMS]>,
}

struct Given {
    // `FORMS` for a topic left out.
    forms: Vec<usize>,
    due_counted: usize,
}

fn open(text: &mut String, section: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(section);
}

impl Page {
    fn of(context: &Context) -> Page {
        let mut due = Vec::new();
        for followup in &context.due {
            let mut shown = followup.clone();
            shown.row.summary = cut(&shown.row.summary);
            let mut line = String::new();
            followup_line(&mut line, &shown);
            due.push(line);
        }
        let mut counts = Vec::new();
        if context.draft_count > 0 {
            counts.push(format!(
                "{} left open on this machine — `worklog drafts`\n",
                counted(context.draft_count, "draft", "drafts")
            ));
        }
        if context.open_count > 0 {
            counts.push(format!(
                "{} not due — `worklog followups <topic>`\n",
                counted(context.open_count, "open followup", "open followups")
            ));
        }
        Page {
            due,
            forks: (!context.forks.is_empty()).then(|| forks(&context.forks)),
            counts,
            topics: context.topics.iter().map(forms).collect(),
        }
    }

    fn whole(&self) -> Given {
        Given {
            forms: vec![0; self.topics.len()],
            due_counted: 0,
        }
    }

    fn fill(&self, given: &Given, text: &mut String) {
        text.clear();
        if !self.due.is_empty() {
            open(text, DUE);
            for line in &self.due[..self.due.len() - given.due_counted] {
                text.push_str(line);
            }
            if given.due_counted > 0 {
                text.push_str(&more_due(given.due_counted));
            }
        }
        for section in self.forks.iter().chain(&self.counts) {
            open(text, section);
        }
        let mut left_out = 0;
        for (forms, form) in self.topics.iter().zip(&given.forms) {
            match forms.get(*form) {
                Some(form) => open(text, form),
                None => left_out += 1,
            }
        }
        if left_out > 0 {
            open(
                text,
                &format!(
                    "{} not shown — `worklog context --json` has every one\n",
                    counted(left_out, "loaded topic", "loaded topics")
                ),
            );
        }
        open(text, CLOSING);
    }
}

enum Step {
    Topic(usize),
    Due,
}

impl Render for Context {
    fn text(&self) -> String {
        let page = Page::of(self);
        let mut given = page.whole();
        let topics = (0..FORMS).flat_map(|_| (0..page.topics.len()).rev().map(Step::Topic));
        let steps = topics.chain(page.due.iter().map(|_| Step::Due));
        let mut text = String::new();
        page.fill(&given, &mut text);
        for step in steps {
            if text.len() <= CONTEXT_BYTES {
                break;
            }
            match step {
                Step::Topic(at) => given.forms[at] += 1,
                Step::Due => given.due_counted += 1,
            }
            page.fill(&given, &mut text);
        }
        // The last step leaves fixed lines, counts and a bounded line of fork labels.
        text
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{assert_case_fits, assert_fits, followup, look_again, row, to};
    use super::*;
    use crate::app::{FollowupRow, TriggerShown};
    use crate::domain::testing::{atlas, lantern};

    const IDEAS: &str = "Ideas — unbuilt, kept with their settled design; opened like a fact:\n";
    const PARTS: &str = "Parts, with their facts, ideas and open followups:\n";
    const FORKED: &str = "Forked — `worklog forks` lists each with its id, \
                          `worklog resolve <id>` opens one: ";

    fn here() -> Via {
        Via::Claim {
            directory: Some("~/projects/lantern".to_owned()),
            closest: true,
        }
    }

    fn edge(from: &str) -> Via {
        Via::Edge { from: to(from) }
    }

    fn machine() -> Via {
        Via::Machine { from: None }
    }

    fn fact(topic: &str, name: &str) -> Row {
        let mut fact = row(&lantern(), &format!("{topic}/{name}"), "s");
        fact.name = Some(name.to_owned());
        fact
    }

    fn named(topic: &str, names: &[&str]) -> Vec<Row> {
        names.iter().map(|name| fact(topic, name)).collect()
    }

    fn many(topic: &str, word: &str, count: usize) -> Vec<Row> {
        (0..count)
            .map(|n| fact(topic, &format!("{word}-{n}-is-fixed")))
            .collect()
    }

    fn part(label: &str, fact_count: usize, open_count: usize) -> Part {
        Part {
            document: atlas(),
            label: label.to_owned(),
            fact_count,
            idea_count: 0,
            open_count,
        }
    }

    fn loaded(label: &str, via: Via, facts: Vec<Row>, ideas: Vec<Row>) -> Loaded {
        Loaded {
            document: lantern(),
            label: label.to_owned(),
            summary: "A thing".to_owned(),
            via,
            facts,
            ideas,
            parts: Vec::new(),
        }
    }

    fn filled(label: &str, via: Via, facts: usize, ideas: usize) -> Loaded {
        let (facts, ideas) = (many(label, "relay", facts), many(label, "dimmer", ideas));
        loaded(label, via, facts, ideas)
    }

    fn due_on(day: &str, summary: &str) -> FollowupRow {
        followup(summary, Some(look_again(day)), true)
    }

    fn due(n: usize) -> FollowupRow {
        due_on(
            "2026-01-01",
            &format!("Add relay {n} to the board and wire its driver"),
        )
    }

    fn context(topics: Vec<Loaded>, due: Vec<FollowupRow>) -> Context {
        Context {
            machine: to("desk"),
            directory: "~/projects/lantern".to_owned(),
            topics,
            due,
            open_count: 0,
            forks: Vec::new(),
            draft_count: 0,
        }
    }

    fn forked(count: usize) -> Vec<Row> {
        (0..count)
            .map(|n| row(&lantern(), &format!("lantern/übergröße-{n}"), "s"))
            .collect()
    }

    fn number_before(text: &str, words: &str) -> Option<usize> {
        let line = text.lines().find(|line| line.contains(words))?;
        line.split(' ').next()?.parse().ok()
    }

    #[test]
    fn a_session_opens_on_what_to_act_on_then_each_loaded_topic_then_how_to_read_more() {
        let mut lamp = loaded(
            "lantern",
            here(),
            named("lantern", &["fuse", "relay"]),
            named("lantern", &["dimmer"]),
        );
        lamp.summary = "A lamp".to_owned();
        let mut wick = part("wick", 1, 2);
        wick.idea_count = 3;
        let mut shade = part("shade", 0, 0);
        shade.idea_count = 1;
        lamp.parts = vec![part("case", 2, 1), part("lens", 0, 0), shade, wick];
        let mut map = loaded("atlas", edge("lantern"), Vec::new(), Vec::new());
        map.summary = String::new();
        let desk = loaded("desk", machine(), named("desk", &["keys"]), Vec::new());
        let through = Via::Machine {
            from: Some(to("desk")),
        };
        let phone = loaded("phone", through, Vec::new(), Vec::new());
        let mut waiting = due_on("2026-10-02", "Fit the lens");
        waiting.row = row(&atlas(), "a71a5000", "Fit the lens");
        waiting.trigger = Some(TriggerShown::Touching {
            topic: to("lantern"),
        });
        let mut relay = row(&lantern(), "lantern/relay", "Pin four");
        relay.forked = true;
        let mut claim = row(&atlas(), "lantern at ~/projects/lantern", "");
        claim.forked = true;
        let opened = Context {
            open_count: 3,
            forks: vec![relay, claim],
            draft_count: 1,
            ..context(
                vec![lamp, map, desk, phone],
                vec![due_on("2026-10-01", "Order a fuse"), waiting],
            )
        };
        assert_eq!(
            opened.text(),
            format!(
                "{DUE}\
                 due 2026-10-01  7f3a91c0  Order a fuse\n\
                 touching lantern  a71a5000  Fit the lens\n\
                 \n\
                 {FORKED}lantern/relay, lantern at ~/projects/lantern\n\
                 \n\
                 1 draft left open on this machine — `worklog drafts`\n\
                 \n\
                 3 open followups not due — `worklog followups <topic>`\n\
                 \n\
                 lantern — claimed at ~/projects/lantern: A lamp\n\
                 \x20 fuse, relay\n\
                 {IDEAS}\
                 \x20 dimmer\n\
                 {PARTS}\
                 \x20 case (2 facts, 1 open), lens, shade (1 idea), wick (1 fact, 3 ideas, 2 open)\n\
                 \n\
                 atlas — via lantern:\n\
                 \x20 (no facts)\n\
                 \n\
                 desk — this machine: A thing\n\
                 \x20 keys\n\
                 \n\
                 phone — via desk: A thing\n\
                 \x20 (no facts)\n\
                 \n\
                 {CLOSING}"
            )
        );
        assert_eq!(
            DUE,
            "due now — `worklog show <id>` for one, `worklog due`, run here, for all:\n"
        );
        assert_eq!(
            CLOSING,
            "Durable facts and ideas, by name where they fit and counted where not —\n\
             `worklog facts <topic>` for a topic's facts,\n\
             `worklog ideas <topic>` for a topic's ideas,\n\
             `worklog show <topic>/<name>` for one,\n\
             `worklog followups <topic>` for open work.\n"
        );
    }

    #[test]
    fn a_host_with_nothing_to_show_opens_on_its_machine_topic_alone() {
        let desk = loaded("desk", machine(), Vec::new(), Vec::new());
        assert_eq!(
            context(vec![desk], Vec::new()).text(),
            format!("desk — this machine: A thing\n  (no facts)\n\n{CLOSING}")
        );
        assert_eq!(context(Vec::new(), Vec::new()).text(), CLOSING);
    }

    #[test]
    fn a_topic_claimed_for_no_directory_says_so_and_ideas_alone_need_no_fact_line() {
        let anywhere = Via::Claim {
            directory: None,
            closest: true,
        };
        let lamp = loaded(
            "lantern",
            anywhere,
            Vec::new(),
            named("lantern", &["dimmer"]),
        );
        let mut opened = context(vec![lamp], Vec::new());
        opened.draft_count = 2;
        opened.open_count = 1;
        assert_eq!(
            opened.text(),
            format!(
                "2 drafts left open on this machine — `worklog drafts`\n\
                 \n\
                 1 open followup not due — `worklog followups <topic>`\n\
                 \n\
                 lantern — claimed anywhere: A thing\n\
                 {IDEAS}\
                 \x20 dimmer\n\
                 \n\
                 {CLOSING}"
            )
        );
    }

    #[test]
    fn names_fold_at_78_columns_and_a_name_is_never_split() {
        let lamp = filled("lantern", here(), 9, 0);
        let mut text = context(vec![lamp], Vec::new()).text();
        text.truncate(text.find("\n\n").expect("a blank line") + 1);
        assert_eq!(
            text,
            "lantern — claimed at ~/projects/lantern: A thing\n\
             \x20 relay-0-is-fixed, relay-1-is-fixed, relay-2-is-fixed, relay-3-is-fixed,\n\
             \x20 relay-4-is-fixed, relay-5-is-fixed, relay-6-is-fixed, relay-7-is-fixed,\n\
             \x20 relay-8-is-fixed\n"
        );

        let mut cased = loaded("lantern", here(), Vec::new(), Vec::new());
        cased.parts = (0..4).map(|n| part(&format!("case-{n}"), 12, 3)).collect();
        let text = context(vec![cased], Vec::new()).text();
        assert!(
            text.contains(
                "\n  case-0 (12 facts, 3 open), case-1 (12 facts, 3 open),\n\
                 \x20 case-2 (12 facts, 3 open), case-3 (12 facts, 3 open)\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_fact_is_shown_by_its_name_and_a_row_with_none_by_its_label() {
        let mut facts = named("lantern", &["relay"]);
        facts.push(row(&atlas(), "a71a5000/fuse", "s"));
        let lamp = loaded("lantern", here(), facts, Vec::new());
        let text = context(vec![lamp], Vec::new()).text();
        assert!(text.contains("\n  relay, a71a5000/fuse\n"), "{text}");
    }

    #[test]
    fn a_long_summary_is_cut_at_96_characters_on_a_due_line_and_in_a_topic_s_header() {
        let (long, fits) = ("é".repeat(97), "é".repeat(96));
        let kept = format!("{}…", "é".repeat(95));
        let mut lamp = loaded("lantern", here(), Vec::new(), Vec::new());
        lamp.summary.clone_from(&long);
        let text = context(vec![lamp.clone()], vec![due_on("2026-10-01", &long)]).text();
        let line = format!("due 2026-10-01  7f3a91c0  {kept}\n");
        assert!(text.contains(&line), "{text}");
        let header = format!("lantern — claimed at ~/projects/lantern: {kept}\n");
        assert!(text.contains(&header), "{text}");

        lamp.summary.clone_from(&fits);
        let text = context(vec![lamp], vec![due_on("2026-10-01", &fits)]).text();
        assert!(text.contains(&format!("  {fits}\n")), "{text}");
        assert!(text.contains(&format!(": {fits}\n")), "{text}");

        let mut spaced = loaded("lantern", here(), Vec::new(), Vec::new());
        spaced.summary = format!("{} {}", "a".repeat(94), "b".repeat(9));
        let text = context(vec![spaced], Vec::new()).text();
        assert!(text.contains(&format!(": {}…\n", "a".repeat(94))), "{text}");
    }

    #[test]
    fn the_facts_of_the_topics_loaded_last_go_to_counts_first() {
        let opened = context(
            vec![
                filled("lantern", here(), 3, 1),
                filled("atlas", edge("lantern"), 600, 1),
                filled("phone", machine(), 2, 1),
            ],
            vec![due(1)],
        );
        let text = opened.text();
        assert_fits(&text);
        let first = format!("{DUE}due 2026-01-01  7f3a91c0  Add relay 1 ");
        assert!(text.starts_with(&first), "{text}");
        assert!(
            text.contains("lantern — claimed at ~/projects/lantern: A thing\n  relay-0-is-fixed, "),
            "{text}"
        );
        assert!(
            text.contains("atlas — via lantern: A thing\n  (600 facts)\nIdeas"),
            "{text}"
        );
        assert!(
            text.contains("phone — this machine: A thing\n  (2 facts)\nIdeas"),
            "{text}"
        );
        assert_eq!(text.matches("  dimmer-0-is-fixed\n").count(), 3, "{text}");
        assert!(text.ends_with(CLOSING), "{text}");

        let mut one = context(vec![filled("lantern", here(), 1, 600)], Vec::new());
        one.topics[0].ideas.truncate(1);
        one.topics.push(filled("atlas", edge("lantern"), 1, 600));
        let text = one.text();
        assert!(text.contains("  (1 fact)\n  (600 ideas)\n"), "{text}");
        let named = format!("  (1 fact)\n{IDEAS}  dimmer-0-is-fixed\n");
        assert!(text.contains(&named), "{text}");
    }

    #[test]
    fn ideas_go_to_counts_once_every_topic_s_facts_have() {
        let opened = context(
            vec![
                filled("lantern", here(), 4, 20),
                filled("atlas", edge("lantern"), 2, 900),
            ],
            Vec::new(),
        );
        let text = opened.text();
        assert_fits(&text);
        assert!(
            text.contains(
                "lantern — claimed at ~/projects/lantern: A thing\n  (4 facts)\nIdeas — "
            ),
            "{text}"
        );
        assert!(text.contains(" dimmer-19-is-fixed\n\natlas"), "{text}");
        assert!(
            text.contains("atlas — via lantern: A thing\n  (2 facts)\n  (900 ideas)\n"),
            "{text}"
        );
    }

    #[test]
    fn parts_go_to_a_count_after_ideas_and_then_the_topics_loaded_last_are_left_out() {
        let parts = |count: usize| -> Vec<Part> {
            (0..count)
                .map(|n| part(&format!("case-{n}"), 2, 1))
                .collect()
        };
        let mut lamp = filled("lantern", here(), 2, 2);
        lamp.parts = parts(3);
        let mut map = filled("atlas", edge("lantern"), 2, 2);
        map.parts = parts(700);
        let text = context(vec![lamp, map], Vec::new()).text();
        assert_fits(&text);
        let named = format!(
            "lantern — claimed at ~/projects/lantern: A thing\n  (2 facts)\n  (2 ideas)\n\
             {PARTS}  case-0 (2 facts, 1 open), "
        );
        assert!(text.contains(&named), "{text}");
        assert!(
            text.contains(
                "atlas — via lantern: A thing\n  (2 facts)\n  (2 ideas)\n  (700 parts)\n"
            ),
            "{text}"
        );

        let crowd: Vec<Loaded> = (0..300)
            .map(|n| {
                let mut topic = filled(&format!("lantern-{n}"), edge("atlas"), 1, 0);
                topic.parts = parts(1);
                topic
            })
            .collect();
        let text = context(crowd, vec![due(1), due(2)]).text();
        assert_fits(&text);
        assert_eq!(text.matches("Add relay ").count(), 2, "{text}");
        let block =
            |n: usize| format!("lantern-{n} — via atlas: A thing\n  (1 fact)\n  (1 part)\n");
        let shown = text.matches(" — via atlas: ").count();
        assert!(shown > 100 && shown < 300, "{shown}");
        for n in 0..shown {
            assert!(text.contains(&block(n)), "{n} in {text}");
        }
        assert!(!text.contains(&format!("lantern-{shown} —")), "{text}");
        let left_out = format!(
            "\n\n{} loaded topics not shown — `worklog context --json` has every one\n\n{CLOSING}",
            300 - shown
        );
        assert!(text.ends_with(&left_out), "{text}");
        let (blank, digit) = (1, 1);
        let grown = text.len() + blank + block(shown).len() - digit;
        assert!(grown > CONTEXT_BYTES, "{shown}");
    }

    #[test]
    fn fork_labels_are_named_while_their_line_is_short_whatever_else_gives_way() {
        let mut opened = context(vec![filled("lantern", here(), 3, 1)], vec![due(1)]);
        opened.forks = forked(400);
        let text = opened.text();
        assert_fits(&text);
        assert_eq!(
            text,
            format!(
                "{DUE}\
                 due 2026-01-01  7f3a91c0  Add relay 1 to the board and wire its driver\n\
                 \n\
                 400 forked documents — `worklog forks` lists each with its id\n\
                 \n\
                 lantern — claimed at ~/projects/lantern: A thing\n\
                 \x20 relay-0-is-fixed, relay-1-is-fixed, relay-2-is-fixed\n\
                 {IDEAS}\
                 \x20 dimmer-0-is-fixed\n\
                 \n\
                 {CLOSING}"
            )
        );

        let labeled = |bytes: usize| {
            let mut one = row(&lantern(), &"é".repeat((bytes - 16) / 2), "s");
            one.label.push_str("/x");
            vec![one, row(&atlas(), "lantern/fuse", "s")]
        };
        let line = |forks: Vec<Row>| {
            let crowd = (0..300).map(|n| filled(&format!("atlas-{n}"), machine(), 9, 9));
            let mut opened = context(crowd.collect(), Vec::new());
            opened.forks = forks;
            let text = opened.text();
            assert_fits(&text);
            assert!(text.contains(" loaded topics not shown — "), "{text}");
            text.lines().next().expect("a line").to_owned()
        };
        let at_the_bound = labeled(FORK_LABEL_BYTES);
        let joined = format!("{}, lantern/fuse", at_the_bound[0].label);
        assert_eq!(joined.len(), FORK_LABEL_BYTES);
        assert_eq!(line(at_the_bound), format!("{FORKED}{joined}"));
        assert_eq!(
            line(labeled(FORK_LABEL_BYTES + 2)),
            "2 forked documents — `worklog forks` lists each with its id"
        );

        let mut alone = context(Vec::new(), Vec::new());
        alone.forks = vec![row(&lantern(), &"é".repeat(401), "s")];
        assert_eq!(
            alone.text(),
            format!("1 forked document — `worklog forks` lists it with its id\n\n{CLOSING}")
        );
    }

    #[test]
    fn the_due_lines_give_way_from_the_last_to_a_count_of_them() {
        let mut opened = context(
            vec![
                filled("lantern", here(), 3, 1),
                filled("desk", machine(), 1, 0),
            ],
            (0..200).map(due).collect(),
        );
        opened.open_count = 4;
        opened.draft_count = 1;
        opened.forks = forked(2);
        let text = opened.text();
        assert_fits(&text);
        assert!(text.starts_with(DUE), "{text}");
        let named = text.matches("due 2026-01-01  7f3a91c0  Add relay ").count();
        assert!(named > 100 && named < 200, "{named}");
        for n in 0..named {
            let line = format!("  Add relay {n} to the board and wire its driver\n");
            assert!(text.contains(&line), "{n} in {text}");
        }
        let rest = format!(
            "driver\n{} more due — `worklog due`, run here, lists them all\n\n\
             {FORKED}lantern/übergröße-0, lantern/übergröße-1\n\n\
             1 draft left open on this machine — `worklog drafts`\n\n\
             4 open followups not due — `worklog followups <topic>`\n\n\
             2 loaded topics not shown — `worklog context --json` has every one\n\n\
             {CLOSING}",
            200 - named
        );
        assert!(text.ends_with(&rest), "{text}");
        let next = format!(
            "due 2026-01-01  7f3a91c0  Add relay {named} to the board and wire its driver\n"
        );
        let digit = 1;
        assert!(text.len() + next.len() - digit > CONTEXT_BYTES, "{named}");

        opened.due.truncate(120);
        let text = opened.text();
        assert_fits(&text);
        assert_eq!(text.matches("due 2026-01-01  ").count(), 120, "{text}");
        assert!(!text.contains("more due"), "{text}");
    }

    #[test]
    fn one_due_line_that_does_not_fit_is_counted_as_one() {
        let page = Page {
            due: vec!["due 2026-01-01  7f3a91c0  Order a fuse\n".to_owned(); 2],
            forks: None,
            counts: Vec::new(),
            topics: Vec::new(),
        };
        let counting = |due_counted: usize| {
            let mut text = String::new();
            let given = Given {
                due_counted,
                ..page.whole()
            };
            page.fill(&given, &mut text);
            text
        };
        assert_eq!(
            counting(1),
            format!(
                "{DUE}due 2026-01-01  7f3a91c0  Order a fuse\n\
                 1 more due — `worklog due`, run here, lists it\n\n{CLOSING}"
            )
        );
        assert_eq!(
            counting(2),
            format!("{DUE}2 more due — `worklog due`, run here, lists them all\n\n{CLOSING}")
        );
    }

    #[test]
    fn no_session_is_opened_with_more_than_the_budget_and_each_says_what_it_left_out() {
        let overdue = |count: usize| -> Vec<FollowupRow> {
            (0..count)
                .map(|n| due_on("2026-01-01", &format!("{n} {}", "ü".repeat(96))))
                .collect()
        };
        let crowd = |count: usize| -> Vec<Loaded> {
            (0..count)
                .map(|n| {
                    let mut topic = filled(&format!("läntern-{n}"), edge("ätlas"), 30, 30);
                    topic.summary = "ö".repeat(200);
                    topic.parts = (0..30).map(|p| part(&format!("cäse-{p}"), 2, 1)).collect();
                    topic
                })
                .collect()
        };
        let cases = [
            (200, 0, 0),
            (2000, 0, 0),
            (0, 400, 0),
            (0, 4000, 0),
            (0, 0, 300),
            (2000, 4000, 300),
            (200, 400, 300),
            (30, 4000, 3),
            (2000, 3, 3),
            (0, 40, 300),
            (2000, 30, 300),
        ];
        for (due_count, fork_count, topic_count) in cases {
            let mut opened = context(crowd(topic_count), overdue(due_count));
            opened.forks = forked(fork_count);
            opened.open_count = usize::MAX;
            opened.draft_count = usize::MAX;
            let text = opened.text();
            let case = format!("{due_count} due, {fork_count} forks, {topic_count} topics");
            assert_case_fits(&case, &text);
            assert!(text.ends_with(CLOSING), "{case}");

            let named = text.matches("due 2026-01-01  7f3a91c0  ").count();
            let more = number_before(&text, " more due — ").unwrap_or(0);
            assert_eq!(named + more, due_count, "{case}");
            assert_eq!(text.starts_with(DUE), due_count > 0, "{case}");

            let labels = text.matches("lantern/übergröße-").count();
            let counted = number_before(&text, " forked document").unwrap_or(0);
            let joined: usize = opened.forks.iter().map(|fork| fork.label.len() + 2).sum();
            let named = joined.saturating_sub(2) <= FORK_LABEL_BYTES;
            let expected = if named {
                (fork_count, 0)
            } else {
                (0, fork_count)
            };
            assert_eq!((labels, counted), expected, "{case}");

            let shown = text.matches(" — via ätlas: ").count();
            let left_out = number_before(&text, " loaded topic").unwrap_or(0);
            assert_eq!(shown + left_out, topic_count, "{case}");
            if more > 0 {
                assert_eq!(shown, 0, "{case}");
            }
        }
    }
}
