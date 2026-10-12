use std::fmt::Write as _;

use super::{INDENT, Render, filed_counts, line, row_fields, row_line, written};
use crate::app::list::{ClaimRow, EntryRow, FactRow, ForkRow, TopicRow};
use crate::app::usage::MachineUsage;
use crate::app::{FollowupRow, Reference, Row, counted};
use crate::render::trigger_state;

fn dated(text: &mut String, day: &impl ToString, row: &Row) {
    let mut fields = vec![day.to_string()];
    fields.extend(row_fields(row, Vec::new()));
    line(text, &fields);
}

fn edges(word: &str, topics: &[Reference]) -> Option<String> {
    let labels: Vec<&str> = topics.iter().map(|topic| topic.label.as_str()).collect();
    (!labels.is_empty()).then(|| format!("{word} {}", labels.join(", ")))
}

impl Render for Vec<TopicRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for topic in self {
            let between = [
                edges("part of", &topic.part_of),
                edges("uses", &topic.uses),
                filed_counts(topic.fact_count, topic.idea_count, topic.open_count),
            ];
            let fields = row_fields(&topic.row, between.into_iter().flatten().collect());
            line(&mut text, &fields);
        }
        text
    }
}

impl Render for Vec<FactRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for fact in self {
            dated(&mut text, &fact.confirmed, &fact.row);
        }
        text
    }
}

impl Render for Vec<EntryRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for entry in self {
            dated(&mut text, &entry.date, &entry.row);
        }
        text
    }
}

fn trigger_words(followup: &FollowupRow) -> String {
    let state = trigger_state(followup);
    match state.touching {
        Some(topic) => format!("{} {}", state.words, topic.label),
        None => state.words,
    }
}

pub(super) fn followup_line(text: &mut String, followup: &FollowupRow) {
    let mut fields = vec![trigger_words(followup)];
    fields.extend(row_fields(&followup.row, Vec::new()));
    line(text, &fields);
}

impl Render for Vec<FollowupRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for followup in self {
            followup_line(&mut text, followup);
        }
        text
    }
}

impl Render for Vec<ClaimRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for row in self {
            let directory = row.directory.as_deref().unwrap_or("(anywhere)");
            let forked = if row.forked { "forked" } else { "" };
            let fields = [&row.topic.label, directory, row.document.short(), forked];
            line(&mut text, &fields);
        }
        text
    }
}

impl Render for Vec<MachineUsage> {
    fn text(&self) -> String {
        let mut text = String::new();
        for (at, counts) in self.iter().enumerate() {
            if at > 0 {
                text.push('\n');
            }
            let runs = counts.commands.iter().map(|count| count.run_count).sum();
            let runs = counted(runs, "run", "runs");
            let _ = writeln!(text, "{} — {runs}", counts.machine.label);
            for count in &counts.commands {
                let _ = writeln!(text, "{:>7} {}", count.run_count, count.command);
            }
        }
        text
    }

    fn notes(&self) -> Vec<String> {
        if self.is_empty() {
            vec!["nothing logged".to_owned()]
        } else {
            Vec::new()
        }
    }
}

impl Render for Vec<ForkRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for fork in self {
            match &fork.row {
                Some(row) => row_line(&mut text, row),
                None => line(&mut text, &[&fork.label]),
            }
            for head in &fork.heads {
                let ended = if head.ended.is_some() { "ended" } else { "" };
                let version = format!("{INDENT}{}", head.version.short());
                let fields = [
                    &version,
                    &written(&head.written),
                    &head.machine.label,
                    ended,
                ];
                line(&mut text, &fields);
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{WRITTEN, followup, look_again, row, stamp, to};
    use super::*;
    use crate::app::TriggerShown;
    use crate::app::list::ForkHead;
    use crate::app::testing::counted;
    use crate::domain::id::{DocumentId, VersionId};
    use crate::domain::schema::Date;
    use crate::domain::testing::{atlas, lantern};

    fn date(text: &str) -> Date {
        Date::parse(text).expect("a date")
    }

    #[test]
    fn usage_is_each_machine_over_its_commands_with_how_often_each_ran() {
        let counts = |label: &str, commands: &[(&str, usize)]| MachineUsage {
            machine: to(label),
            topic: Some(label.to_owned()),
            commands: counted(commands),
        };
        let listed = vec![
            counts("desk", &[("show", 1200), ("new entry", 2)]),
            counts("phone", &[("context", 1)]),
        ];
        assert_eq!(
            listed.text(),
            "desk — 1202 runs\n\
             \x20  1200 show\n\
             \x20     2 new entry\n\
             \n\
             phone — 1 run\n\
             \x20     1 context\n"
        );
        assert_eq!(Vec::<MachineUsage>::new().text(), "");
    }

    #[test]
    fn a_machine_is_headed_by_its_label_and_no_run_at_all_is_a_note() {
        let labeled = |label: &str, topic: Option<&str>| MachineUsage {
            machine: Reference {
                document: atlas(),
                label: label.to_owned(),
            },
            topic: topic.map(str::to_owned),
            commands: counted(&[("show", 2)]),
        };
        let listed = vec![labeled(atlas().as_str(), None)];
        assert_eq!(
            listed.text(),
            format!("{} — 2 runs\n      2 show\n", atlas())
        );
        let short = atlas().short().to_owned();
        assert_eq!(
            vec![labeled(&short, Some(&short))].text(),
            format!("{short} — 2 runs\n      2 show\n")
        );
        assert_eq!(listed.notes(), [""; 0]);
        assert_eq!(Vec::<MachineUsage>::new().notes(), ["nothing logged"]);
    }

    #[test]
    fn a_topic_is_its_row_then_what_it_is_part_of_and_what_it_uses() {
        let topic = |id: DocumentId, label: &str, part_of: &[&str], uses: &[&str]| TopicRow {
            row: row(&id, label, "A thing"),
            part_of: part_of.iter().map(|label| to(label)).collect(),
            uses: uses.iter().map(|label| to(label)).collect(),
            parts: vec![to("relay")],
            top: false,
            fact_count: 0,
            idea_count: 0,
            open_count: 0,
        };
        let mut ended = topic(atlas(), "phone", &[], &["desk"]);
        ended.row.ended = Some("retired".to_owned());
        let listed = vec![
            topic(atlas(), "atlas", &[], &[]),
            topic(lantern(), "lantern", &["atlas", "desk"], &["phone"]),
            topic(lantern(), "relay", &["lantern"], &[]),
            ended,
        ];
        assert_eq!(
            listed.text(),
            "a71a5000  atlas  A thing\n\
             7f3a91c0  lantern  A thing  part of atlas, desk  uses phone\n\
             7f3a91c0  relay  A thing  part of lantern\n\
             a71a5000  phone  A thing  uses desk  ended retired\n"
        );
        assert_eq!(Vec::<TopicRow>::new().text(), "");
    }

    #[test]
    fn a_topic_with_something_filed_on_it_shows_the_counts_that_are_not_zero() {
        let topic = |label: &str, fact_count, idea_count, open_count| TopicRow {
            row: row(&lantern(), label, "A thing"),
            part_of: Vec::new(),
            uses: Vec::new(),
            parts: Vec::new(),
            top: true,
            fact_count,
            idea_count,
            open_count,
        };
        let mut whole = topic("relay", 1, 1, 1);
        whole.part_of = vec![to("lantern")];
        whole.uses = vec![to("phone")];
        whole.row.forked = true;
        whole.row.ended = Some("retired".to_owned());
        let listed = vec![
            topic("lantern", 2, 1, 3),
            topic("atlas", 0, 0, 1),
            topic("phone", 1, 0, 0),
            topic("desk", 0, 2, 0),
            whole,
        ];
        assert_eq!(
            listed.text(),
            "7f3a91c0  lantern  A thing  (2 facts, 1 idea, 3 open)\n\
             7f3a91c0  atlas  A thing  (1 open)\n\
             7f3a91c0  phone  A thing  (1 fact)\n\
             7f3a91c0  desk  A thing  (2 ideas)\n\
             7f3a91c0  relay  A thing  part of lantern  uses phone  (1 fact, 1 idea, 1 open)  \
             forked  ended retired\n"
        );
    }

    #[test]
    fn a_fact_leads_with_the_day_it_was_confirmed_and_an_entry_with_its_day() {
        let mut ended = row(&atlas(), "lantern/fuse", "Two amps");
        ended.ended = Some("false".to_owned());
        ended.forked = true;
        let facts = vec![
            FactRow {
                row: row(&lantern(), "lantern/relay", "Pin four"),
                confirmed: date("2026-09-04"),
            },
            FactRow {
                row: ended,
                confirmed: date("2026-01-05"),
            },
        ];
        assert_eq!(
            facts.text(),
            "2026-09-04  7f3a91c0  lantern/relay  Pin four\n\
             2026-01-05  a71a5000  lantern/fuse  Two amps  forked  ended false\n"
        );
        assert_eq!(Vec::<FactRow>::new().text(), "");

        let entries = vec![EntryRow {
            row: row(&lantern(), "2026-10-08-wiring", "Wired the relay"),
            date: date("2026-10-08"),
        }];
        assert_eq!(
            entries.text(),
            "2026-10-08  7f3a91c0  2026-10-08-wiring  Wired the relay\n"
        );
        assert_eq!(Vec::<EntryRow>::new().text(), "");
    }

    #[test]
    fn a_followup_leads_with_the_state_of_its_trigger() {
        let mut done = followup("Solder it", None, false);
        done.row.ended = Some("done".to_owned());
        let listed = vec![
            followup("Order a fuse", Some(look_again("2026-10-01")), true),
            followup("Check the driver", Some(look_again("2026-11-01")), false),
            followup(
                "Fit the lens",
                Some(TriggerShown::Touching {
                    topic: to("lantern"),
                }),
                false,
            ),
            followup("Paint the case", None, false),
            done,
        ];
        assert_eq!(
            listed.text(),
            "due 2026-10-01  7f3a91c0  Order a fuse\n\
             by 2026-11-01  7f3a91c0  Check the driver\n\
             touching lantern  7f3a91c0  Fit the lens\n\
             no trigger  7f3a91c0  Paint the case\n\
             no trigger  7f3a91c0  Solder it  ended done\n"
        );
        assert_eq!(Vec::<FollowupRow>::new().text(), "");
    }

    #[test]
    fn a_claim_is_its_topic_its_directory_and_its_short_id() {
        let claim = |id, topic: &str, directory: Option<&str>, forked| ClaimRow {
            document: id,
            machine: to("desk"),
            topic: to(topic),
            directory: directory.map(str::to_owned),
            forked,
        };
        let listed = vec![
            claim(atlas(), "atlas", None, false),
            claim(lantern(), "lantern", Some("~/projects/lantern"), true),
        ];
        assert_eq!(
            listed.text(),
            "atlas  (anywhere)  a71a5000\n\
             lantern  ~/projects/lantern  7f3a91c0  forked\n"
        );
        assert_eq!(Vec::<ClaimRow>::new().text(), "");
    }

    #[test]
    fn a_fork_is_its_row_over_a_line_for_each_head() {
        let head = |seed: &[u8], machine: &str, ended: bool| ForkHead {
            version: VersionId::of(seed),
            written: stamp(WRITTEN),
            machine: to(machine),
            ended: ended.then(|| "retired".to_owned()),
        };
        let short = |seed: &[u8]| VersionId::of(seed).short().to_owned();
        let mut forked = row(&lantern(), "lantern", "A lamp");
        forked.forked = true;
        let listed = vec![
            ForkRow {
                document: lantern(),
                label: "lantern".to_owned(),
                row: Some(forked),
                heads: vec![head(b"left", "desk", false), head(b"right", "phone", true)],
            },
            ForkRow {
                document: atlas(),
                label: atlas().short().to_owned(),
                row: None,
                heads: vec![head(b"left", "desk", false)],
            },
        ];
        assert_eq!(
            listed.text(),
            format!(
                "7f3a91c0  lantern  A lamp  forked\n\
                 \x20   {left}  2026-10-09T18:22:41.118+01:00  desk\n\
                 \x20   {right}  2026-10-09T18:22:41.118+01:00  phone  ended\n\
                 a71a5000\n\
                 \x20   {left}  2026-10-09T18:22:41.118+01:00  desk\n",
                left = short(b"left"),
                right = short(b"right")
            )
        );
        assert_eq!(Vec::<ForkRow>::new().text(), "");
    }
}
