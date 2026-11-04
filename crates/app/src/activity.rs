use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::vec::Vec;

use chrono::{Datelike as _, Days, NaiveDate, Weekday};
use serde::Serialize;

use crate::heads::{ended_for, row_head};
use crate::holdings::{self, Holdings, Writes};
use crate::index::Topics;
use crate::lookup::{Lookup, Names};
use crate::rows::{Addressed, memberships, reading};
use crate::{Failure, ReadDeps};
use worklog_domain::id::DocumentId;
use worklog_schema::{Content, Date, KindOf};

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
    pub topic: Addressed,
    pub entry_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub activity: Activity,
    /// The topic counted, or none for the whole store.
    pub topic: Option<Addressed>,
    /// Each unended topic with its entries in the span, its parts' among them; the most first,
    /// then by label.
    pub topics: Vec<TopicCount>,
    pub holdings: Holdings,
    /// What was written over the span of `activity`.
    pub writes: Writes,
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
                under: topics.wholes(memberships(&document, KindOf::Entry, false)),
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

fn kept_under<'a>(dated: &'a [Dated], topic: Option<&DocumentId>) -> Vec<&'a Dated> {
    (dated.iter())
        .filter(|entry| topic.is_none_or(|topic| entry.under.contains(topic)))
        .collect()
}

/// The unended entries under `topic` and its parts, or in the whole store, counted by day over
/// the span of `year`, or of the 53 weeks up to today when no year is given. Refuses an
/// address that is not one topic, and a year outside the four digits a date is written with.
pub fn activity(
    deps: &ReadDeps,
    topic: Option<&str>,
    year: Option<i32>,
) -> Result<Activity, Failure> {
    let (from, to) = span(deps.today()?, year)?;
    let (lookup, topics, topic) = reading(deps, topic)?;
    let dated = dated_entries(&lookup, &topics)?;
    Ok(tally(&kept_under(&dated, topic.as_ref()), from, to, year))
}

/// What `activity` returns, with the topic it names, each unended topic's count over the same
/// span whatever the topic, and what the store holds and was written in that span.
pub fn stats(deps: &ReadDeps, topic: Option<&str>, year: Option<i32>) -> Result<Stats, Failure> {
    let (from, to) = span(deps.today()?, year)?;
    let (lookup, topics, topic) = reading(deps, topic)?;
    let dated = dated_entries(&lookup, &topics)?;
    let scope = holdings::under(&lookup, &topics, topic.as_ref())?;
    let unfiled = match topic {
        None => Some(holdings::unfiled(&lookup, deps.usage.size()?)?),
        Some(_) => None,
    };
    let mut counts: BTreeMap<&DocumentId, usize> = BTreeMap::new();
    for entry in dated
        .iter()
        .filter(|entry| (from..=to).contains(&entry.date))
    {
        for under in &entry.under {
            *counts.entry(under).or_default() += 1;
        }
    }
    let names = Names::of(&lookup);
    let mut counted: Vec<TopicCount> = (topics.ids())
        .filter(|id| !topics.is_ended(id))
        .map(|id| {
            Ok(TopicCount {
                topic: Addressed::of(topics.reference(id), &names)?,
                entry_count: counts.get(id).copied().unwrap_or(0),
            })
        })
        .collect::<Result<_, Failure>>()?;
    counted.sort_by(|a, b| (b.entry_count, &a.topic.label).cmp(&(a.entry_count, &b.topic.label)));
    let chosen = (topic.as_ref())
        .map(|id| Addressed::of(topics.reference(id), &names))
        .transpose()?;
    Ok(Stats {
        activity: tally(&kept_under(&dated, topic.as_ref()), from, to, year),
        topic: chosen,
        topics: counted,
        holdings: holdings::holdings(&scope, unfiled),
        writes: holdings::writes(&scope, &topics, from, to),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{entry, part_of, topic, world};
    use alloc::borrow::ToOwned;
    use alloc::string::{String, ToString};
    use worklog_domain::ports::{StoreError, UsageRead};
    use worklog_domain::usage::{Invocation, RunFilter};

    struct Unreadable;

    impl UsageRead for Unreadable {
        fn read(&self, _: &RunFilter) -> Result<Vec<Invocation>, StoreError> {
            Ok(Vec::new())
        }

        fn size(&self) -> Result<usize, StoreError> {
            Err(StoreError::io("usage", "unreadable"))
        }
    }

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

    #[test]
    fn the_activity_of_a_store_whose_usage_log_cannot_be_read_is_still_counted() {
        let (world, lantern) = world();
        entry(&world, "2026-10-08", "wiring", &[&lantern]);
        let unreadable = Unreadable;
        let deps = ReadDeps {
            usage: &unreadable,
            ..world.deps().read()
        };
        assert_eq!(activity(&deps, None, None).unwrap().entry_count, 1);
        assert!(stats(&deps, None, None).is_err());
    }
}
