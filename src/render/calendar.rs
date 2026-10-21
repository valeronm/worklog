//! A span of days as the columns of weeks a grid shows them in, Monday first.

use std::collections::BTreeMap;

use chrono::{Datelike as _, NaiveDate, Weekday};

use crate::app::activity::Activity;
use crate::domain::schema::Date;

/// The level of the busiest day in the span.
pub const TOP_LEVEL: usize = 4;

/// The fewest weeks between two month names, so that a name of three letters and a space fit.
pub const MONTH_GAP: usize = 4;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub date: Date,
    pub entry_count: usize,
    /// 0 for a day holding no entry, and otherwise the quarter of the busiest day's count the
    /// entry count reaches into, from 1 to `TOP_LEVEL`.
    pub level: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Month {
    pub week: usize,
    pub name: &'static str,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Calendar {
    /// Every week from the one holding the span's first day to the one holding its last,
    /// Monday first, with `None` for a day outside the span.
    pub weeks: Vec<[Option<Cell>; 7]>,
    /// The name of a month over the first week whose first day in the span is in it, left out
    /// where it would sit within `MONTH_GAP` weeks of the next one.
    pub months: Vec<Month>,
}

fn level(entry_count: usize, busiest: usize) -> usize {
    if entry_count == 0 {
        0
    } else {
        (entry_count * TOP_LEVEL).div_ceil(busiest)
    }
}

// Where two names are too close, the earlier one goes.
fn spaced(named: Vec<Month>) -> Vec<Month> {
    let mut kept: Vec<Month> = Vec::new();
    for month in named.into_iter().rev() {
        if kept
            .last()
            .is_none_or(|next| next.week - month.week >= MONTH_GAP)
        {
            kept.push(month);
        }
    }
    kept.reverse();
    kept
}

#[must_use]
pub fn calendar(activity: &Activity) -> Calendar {
    let (from, to) = (activity.from.on_calendar(), activity.to.on_calendar());
    let counts: BTreeMap<Date, usize> = (activity.days.iter())
        .map(|day| (day.date, day.entry_count))
        .collect();
    let busiest = counts.values().copied().max().unwrap_or(0);
    let cell = |day: NaiveDate| {
        let date = Date::of_calendar(day).filter(|_| (from..=to).contains(&day))?;
        let entry_count = counts.get(&date).copied().unwrap_or(0);
        Some(Cell {
            date,
            entry_count,
            level: level(entry_count, busiest),
        })
    };
    let mut weeks = Vec::new();
    let mut named: Vec<Month> = Vec::new();
    let mondays = from.week(Weekday::Mon).first_day().iter_weeks();
    for monday in mondays.take_while(|monday| *monday <= to) {
        let mut days = monday.iter_days();
        weeks.push(std::array::from_fn(|_| days.next().and_then(cell)));
        let month = usize::try_from(monday.max(from).month0()).ok();
        let name = month.and_then(|month| MONTHS.get(month));
        if let Some(name) = name.filter(|name| named.last().is_none_or(|last| last.name != **name))
        {
            named.push(Month {
                week: weeks.len() - 1,
                name,
            });
        }
    }
    Calendar {
        months: spaced(named),
        weeks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::activity::Day;

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    fn activity(from: &str, to: &str, days: &[(&str, usize)]) -> Activity {
        Activity {
            from: date(from),
            to: date(to),
            year: None,
            days: (days.iter())
                .map(|(day, entry_count)| Day {
                    date: date(day),
                    entry_count: *entry_count,
                })
                .collect(),
            entry_count: days.iter().map(|(_, count)| count).sum(),
            years: Vec::new(),
        }
    }

    #[test]
    fn a_span_fills_whole_weeks_monday_first_with_the_days_outside_it_left_empty() {
        let shown = calendar(&activity(
            "2026-10-01",
            "2026-10-13",
            &[("2026-10-01", 1), ("2026-10-13", 6)],
        ));
        assert_eq!(shown.weeks.len(), 3);
        let first = shown.weeks[0];
        assert_eq!(first[..3], [None; 3]);
        let thursday = first[3].unwrap();
        assert_eq!((thursday.date, thursday.level), (date("2026-10-01"), 1));
        assert_eq!(first[4].unwrap().level, 0);
        let last = shown.weeks[2];
        let tuesday = last[1].unwrap();
        assert_eq!((tuesday.entry_count, tuesday.level), (6, TOP_LEVEL));
        assert_eq!(last[2..], [None; 5]);
    }

    #[test]
    fn a_day_s_level_is_the_quarter_of_the_busiest_day_its_count_reaches_into() {
        let shown = calendar(&activity(
            "2026-10-05",
            "2026-10-11",
            &[
                ("2026-10-05", 1),
                ("2026-10-06", 2),
                ("2026-10-07", 3),
                ("2026-10-08", 6),
                ("2026-10-09", 8),
            ],
        ));
        let levels: Vec<usize> = shown.weeks[0]
            .iter()
            .flatten()
            .map(|cell| cell.level)
            .collect();
        assert_eq!(levels, [1, 1, 2, 3, TOP_LEVEL, 0, 0]);
    }

    #[test]
    fn a_month_is_named_over_its_first_week_unless_the_next_name_is_too_close() {
        let shown = calendar(&activity("2026-09-28", "2026-12-31", &[]));
        let names: Vec<(usize, &str)> = (shown.months.iter())
            .map(|month| (month.week, month.name))
            .collect();
        assert_eq!(names, [(1, "Oct"), (5, "Nov"), (10, "Dec")]);
        let shown = calendar(&activity("2026-01-01", "2026-12-31", &[]));
        assert_eq!(shown.months.first().map(|month| month.name), Some("Jan"));
        assert_eq!(shown.months.len(), 12);
    }

    #[test]
    fn a_span_whose_end_comes_first_has_no_weeks() {
        assert_eq!(
            calendar(&activity("2026-10-09", "2026-10-01", &[])),
            Calendar::default()
        );
    }
}
