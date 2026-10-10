use super::{INDENT, Render, line, row_fields, row_line, written};
use crate::app::list::{ClaimRow, EntryRow, FactRow, ForkRow, TopicRow};
use crate::app::{FollowupRow, Row, TriggerShown};

fn dated(text: &mut String, day: &impl ToString, row: &Row) {
    let mut fields = vec![day.to_string()];
    fields.extend(row_fields(row, Vec::new()));
    line(text, &fields);
}

fn edges(word: &str, topics: &[String]) -> Option<String> {
    (!topics.is_empty()).then(|| format!("{word} {}", topics.join(", ")))
}

impl Render for Vec<TopicRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for topic in self {
            let linked = [edges("part of", &topic.part_of), edges("uses", &topic.uses)];
            let fields = row_fields(&topic.row, linked.into_iter().flatten().collect());
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

fn trigger_state(followup: &FollowupRow) -> String {
    match &followup.trigger {
        Some(TriggerShown::LookAgain { on, .. }) if followup.due => format!("due {on}"),
        Some(TriggerShown::LookAgain { on, .. }) => format!("by {on}"),
        Some(TriggerShown::Touching { topic }) => format!("touching {topic}"),
        None => "no trigger".to_owned(),
    }
}

impl Render for Vec<FollowupRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for followup in self {
            let mut fields = vec![trigger_state(followup)];
            fields.extend(row_fields(&followup.row, Vec::new()));
            line(&mut text, &fields);
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
            let fields = [&row.topic, directory, row.document.short(), forked];
            line(&mut text, &fields);
        }
        text
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
                let fields = [&version, &written(&head.written), &head.machine, ended];
                line(&mut text, &fields);
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{row, stamp};
    use super::*;
    use crate::app::list::ForkHead;
    use crate::domain::id::{DocumentId, VersionId};
    use crate::domain::schema::Date;
    use crate::domain::testing::{atlas, lantern};

    fn date(text: &str) -> Date {
        Date::parse(text).expect("a date")
    }

    #[test]
    fn a_topic_is_its_row_then_what_it_is_part_of_and_what_it_uses() {
        let topic = |id: DocumentId, label: &str, part_of: &[&str], uses: &[&str]| TopicRow {
            row: row(&id, label, "A thing"),
            part_of: part_of.iter().map(ToString::to_string).collect(),
            uses: uses.iter().map(ToString::to_string).collect(),
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
        let followup = |summary: &str, trigger: Option<TriggerShown>, due: bool| FollowupRow {
            row: row(&lantern(), "7f3a91c0", summary),
            trigger,
            due,
            entry: Some("2026-10-08-wiring".to_owned()),
            about: None,
        };
        let on = |day: &str| {
            Some(TriggerShown::LookAgain {
                on: date(day),
                why: "The part arrives".to_owned(),
            })
        };
        let mut done = followup("Solder it", None, false);
        done.row.ended = Some("done".to_owned());
        let listed = vec![
            followup("Order a fuse", on("2026-10-01"), true),
            followup("Check the driver", on("2026-11-01"), false),
            followup(
                "Fit the lens",
                Some(TriggerShown::Touching {
                    topic: "lantern".to_owned(),
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
            machine: "desk".to_owned(),
            topic: topic.to_owned(),
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
            written: stamp("2026-10-09T18:22:41.118204+01:00"),
            machine: machine.to_owned(),
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
