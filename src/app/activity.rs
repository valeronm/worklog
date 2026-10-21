use std::collections::{BTreeMap, BTreeSet};

use chrono::{Datelike as _, Days, NaiveDate, Weekday};
use serde::Serialize;

use crate::app::heads::{ended_for, row_head};
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::rows::{memberships, reading};
use crate::app::{Failure, ReadDeps, Reference};
use crate::domain::id::DocumentId;
use crate::domain::schema::{Content, Date, KindOf};

const WEEKS_BEFORE: u64 = 52;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Activity {
    /// The first and the last day counted: a Monday 52 weeks before the week holding today,
    /// and today, or the first and the last day of `year`.
    pub from: Date,
    pub to: Date,
    pub year: Option<i32>,
    /// The days from `from` to `to` holding an entry, oldest first.
    pub days: Vec<Day>,
    pub entry_count: usize,
    /// The years holding an entry, whatever the span, newest first.
    pub years: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    pub date: Date,
    pub entry_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TopicCount {
    pub topic: Reference,
    pub entry_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub activity: Activity,
    /// The topic counted, or none for the whole store.
    pub topic: Option<Reference>,
    /// Each unended topic with its entries in the span, its parts' among them; the most first,
    /// then by label.
    pub topics: Vec<TopicCount>,
}

fn span(today: Date, year: Option<i32>) -> Result<(Date, Date), Failure> {
    let Some(year) = year else {
        let monday = today.on_calendar().week(Weekday::Mon).first_day();
        let from = (monday.checked_sub_days(Days::new(WEEKS_BEFORE * 7)))
            .and_then(Date::of_calendar)
            .ok_or_else(|| Failure::Usage(format!("{today}: no year before it to count")))?;
        return Ok((from, today));
    };
    let day = |month, day| {
        NaiveDate::from_ymd_opt(year, month, day)
            .and_then(Date::of_calendar)
            .ok_or_else(|| Failure::Usage(format!("{year}: not a year a date is written in")))
    };
    Ok((day(1, 1)?, day(12, 31)?))
}

struct Dated {
    date: Date,
    /// The topics the entry is filed on and every topic those are part of.
    under: BTreeSet<DocumentId>,
}

// Every unended entry, under the topics `members` lists it under.
fn dated_entries(lookup: &Lookup, topics: &Topics) -> Result<Vec<Dated>, Failure> {
    let mut dated = Vec::new();
    for document in lookup.of_kind(KindOf::Entry)? {
        let Some((_, record)) = row_head(&document) else {
            continue;
        };
        if let Content::Entry(entry) = &record.content
            && ended_for(&record).is_none()
        {
            dated.push(Dated {
                date: entry.date,
                under: topics.wholes(memberships(&document, KindOf::Entry)),
            });
        }
    }
    Ok(dated)
}

fn tally(dated: &[&Dated], from: Date, to: Date, year: Option<i32>) -> Activity {
    let mut by_day: BTreeMap<Date, usize> = BTreeMap::new();
    for entry in dated
        .iter()
        .filter(|entry| (from..=to).contains(&entry.date))
    {
        *by_day.entry(entry.date).or_default() += 1;
    }
    let mut years: Vec<i32> = (dated.iter())
        .map(|entry| entry.date.on_calendar().year())
        .collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    Activity {
        from,
        to,
        year,
        entry_count: by_day.values().sum(),
        days: (by_day.into_iter())
            .map(|(date, entry_count)| Day { date, entry_count })
            .collect(),
        years,
    }
}

/// The unended entries under `topic` and its parts, or in the whole store, counted by day over
/// the span of `year`, or of the 53 weeks up to today when no year is given. Refuses an
/// address that is not one topic, and a year outside the four digits a date is written with.
pub fn activity(
    deps: &ReadDeps,
    topic: Option<&str>,
    year: Option<i32>,
) -> Result<Activity, Failure> {
    Ok(stats(deps, topic, year)?.activity)
}

/// What `activity` returns, with the topic it names and each unended topic's count over the
/// same span whatever the topic.
pub fn stats(deps: &ReadDeps, topic: Option<&str>, year: Option<i32>) -> Result<Stats, Failure> {
    let (from, to) = span(deps.today()?, year)?;
    let (lookup, topics, topic) = reading(deps, topic)?;
    let dated = dated_entries(&lookup, &topics)?;
    let mut counts: BTreeMap<&DocumentId, usize> = BTreeMap::new();
    for entry in dated
        .iter()
        .filter(|entry| (from..=to).contains(&entry.date))
    {
        for under in &entry.under {
            *counts.entry(under).or_default() += 1;
        }
    }
    let mut counted: Vec<TopicCount> = (topics.ids())
        .filter(|id| !topics.is_ended(id))
        .map(|id| TopicCount {
            topic: topics.reference(id),
            entry_count: counts.get(id).copied().unwrap_or(0),
        })
        .collect();
    counted.sort_by(|a, b| (b.entry_count, &a.topic.label).cmp(&(a.entry_count, &b.topic.label)));
    let under: Vec<&Dated> = (dated.iter())
        .filter(|entry| {
            topic
                .as_ref()
                .is_none_or(|topic| entry.under.contains(topic))
        })
        .collect();
    Ok(Stats {
        activity: tally(&under, from, to, year),
        topic: topic.map(|id| topics.reference(&id)),
        topics: counted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{entry, part_of, topic, world};

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn days(activity: &Activity) -> Vec<(String, usize)> {
        (activity.days.iter())
            .map(|day| (day.date.to_string(), day.entry_count))
            .collect()
    }

    #[test]
    fn the_last_year_runs_from_a_monday_fifty_two_weeks_before_this_week_to_today() {
        let (world, lantern) = world();
        entry(&world, "2025-10-05", "before", &[&lantern]);
        entry(&world, "2025-10-06", "first", &[&lantern]);
        entry(&world, "2026-10-08", "wiring", &[&lantern]);
        entry(&world, "2026-10-08", "relay", &[&lantern]);
        entry(&world, "2026-10-10", "tomorrow", &[&lantern]);
        let activity = activity(&world.deps().read(), None, None).unwrap();
        assert_eq!(activity.from, date("2025-10-06"));
        assert_eq!(activity.to, date("2026-10-09"));
        assert_eq!(
            days(&activity),
            [("2025-10-06".to_owned(), 1), ("2026-10-08".to_owned(), 2)]
        );
        assert_eq!(activity.entry_count, 3);
        assert_eq!(activity.years, [2026, 2025]);
    }

    #[test]
    fn a_year_runs_from_its_first_day_to_its_last_and_a_topic_counts_its_parts() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        entry(&world, "2024-12-31", "late", &[&lantern]);
        entry(&world, "2025-01-01", "early", &[&atlas]);
        entry(&world, "2025-12-31", "last", &[&lantern]);
        let other = topic(&world, "desk", "");
        entry(&world, "2025-06-01", "elsewhere", &[&other]);
        let deps = world.deps();
        let activity = activity(&deps.read(), Some("lantern"), Some(2025)).unwrap();
        assert_eq!(
            (activity.from, activity.to),
            (date("2025-01-01"), date("2025-12-31"))
        );
        assert_eq!(
            days(&activity),
            [("2025-01-01".to_owned(), 1), ("2025-12-31".to_owned(), 1)]
        );
        assert_eq!(activity.years, [2025, 2024]);
        assert!(matches!(
            super::activity(&deps.read(), None, Some(10_000)),
            Err(Failure::Usage(_))
        ));
    }

    #[test]
    fn topics_are_counted_with_their_parts_over_the_span_the_most_first() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", "");
        entry(&world, "2026-10-08", "wiring", &[&atlas]);
        entry(&world, "2026-10-07", "relay", &[&lantern, &atlas]);
        entry(&world, "2026-10-06", "case", &[&phone]);
        entry(&world, "2026-10-05", "screen", &[&phone]);
        entry(&world, "2024-10-05", "old", &[&phone]);
        let counted = |year| -> Vec<(String, usize)> {
            (stats(&world.deps().read(), None, year)
                .unwrap()
                .topics
                .into_iter())
            .map(|count| (count.topic.label, count.entry_count))
            .collect()
        };
        let pairs = |list: &[(&str, usize)]| -> Vec<(String, usize)> {
            list.iter()
                .map(|(label, n)| ((*label).to_owned(), *n))
                .collect()
        };
        assert_eq!(
            counted(None),
            pairs(&[("atlas", 2), ("lantern", 2), ("phone", 2), ("desk", 0)])
        );
        assert_eq!(
            counted(Some(2024)),
            pairs(&[("phone", 1), ("atlas", 0), ("desk", 0), ("lantern", 0)])
        );
    }
}
