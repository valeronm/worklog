//! The grid of days holding entries, over the whole store or one topic.

use askama::Template;

use super::links::{Link, Nav, Place, with_query};
use crate::app::activity::Stats;
use crate::app::counted;
use crate::render::calendar::{Cell, MONTH_GAP, Month, calendar};

pub struct Square {
    pub level: usize,
    pub title: String,
    // None for a day holding no entry.
    pub href: Option<String>,
}

/// A topic to count, a span to count over, or the whole store; `current` for the one shown.
pub struct Choice {
    pub link: Link,
    pub current: bool,
    // The entries a topic holds over the span.
    pub count: Option<usize>,
}

#[derive(Template)]
#[template(path = "stats.html")]
pub struct StatsPage {
    pub nav: Nav,
    pub topics: Vec<Choice>,
    pub week_count: usize,
    pub months: Vec<Month>,
    pub month_gap: usize,
    /// Each week's days Monday first, with `None` for a day outside the span.
    pub days: Vec<Option<Square>>,
    pub total: String,
    pub under: Option<Link>,
    pub spans: Vec<Choice>,
}

fn square(cell: &Cell, topic: Option<&str>) -> Square {
    let on = cell.date.to_string();
    let parameters = [("topic", topic), ("on", Some(on.as_str()))];
    Square {
        level: cell.level,
        title: format!("{on}: {}", counted(cell.entry_count, "entry", "entries")),
        href: (cell.entry_count > 0).then(|| with_query(Place::Entries.route(), &parameters)),
    }
}

fn choice(text: String, topic: Option<&str>, year: Option<i32>, current: bool) -> Choice {
    let year = year.map(|year| year.to_string());
    let parameters = [("topic", topic), ("year", year.as_deref())];
    Choice {
        link: Link::plain(text, with_query(Place::Stats.route(), &parameters)),
        current,
        count: None,
    }
}

impl StatsPage {
    pub fn new(stats: &Stats) -> StatsPage {
        let activity = &stats.activity;
        let shown = calendar(activity);
        let topic = stats.topic.as_ref().map(|topic| topic.label.as_str());
        let days = (shown.weeks.iter())
            .flat_map(|week| week.iter())
            .map(|cell| cell.as_ref().map(|cell| square(cell, topic)))
            .collect();
        let mut topics = vec![choice(
            "All".to_owned(),
            None,
            activity.year,
            topic.is_none(),
        )];
        topics.extend(stats.topics.iter().map(|count| {
            let chosen = stats.topic.as_ref() == Some(&count.topic);
            let label = Some(count.topic.label.as_str());
            Choice {
                count: Some(count.entry_count),
                ..choice(count.topic.label.clone(), label, activity.year, chosen)
            }
        }));
        let mut spans = vec![choice(
            "last year".to_owned(),
            topic,
            None,
            activity.year.is_none(),
        )];
        spans.extend((activity.years.iter()).map(|year| {
            choice(
                year.to_string(),
                topic,
                Some(*year),
                activity.year == Some(*year),
            )
        }));
        let entries = counted(activity.entry_count, "entry", "entries");
        StatsPage {
            nav: Nav::at(Some(Place::Stats)),
            topics,
            week_count: shown.weeks.len(),
            months: shown.months,
            month_gap: MONTH_GAP,
            days,
            total: match activity.year {
                Some(year) => format!("{entries} in {year}"),
                None => format!("{entries} in the last year"),
            },
            under: (stats.topic.as_ref())
                .map(|topic| Link::to(&topic.document, &topic.label, topic.ended())),
            spans,
        }
    }
}
