//! The grid of days holding entries and the numbers of what the store holds and was written.

use askama::Template;

use super::links::{Choice, Layout, Link, Place, with_query};
use worklog_app::activity::Stats;
use worklog_app::counted;
use worklog_app::holdings::Holdings;
use worklog_render::calendar::{Cell, MONTH_GAP, Month, calendar};

pub struct Square {
    pub level: usize,
    pub title: String,
    // None for a day holding no entry.
    pub href: Option<String>,
}

#[derive(Template)]
#[template(path = "stats.html")]
pub struct StatsPage {
    pub layout: Layout,
    pub topics: Vec<Choice>,
    pub week_count: usize,
    pub months: Vec<Month>,
    pub month_gap: usize,
    /// Each week's days Monday first, with `None` for a day outside the span.
    pub days: Vec<Option<Square>>,
    pub total: String,
    pub under: Option<Link>,
    pub spans: Vec<Choice>,
    pub kinds: Vec<KindRow>,
    pub held: String,
    pub unfiled: Option<String>,
    pub written: Vec<MonthRow>,
    pub machines: Vec<MachineRow>,
}

const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

fn size(bytes: usize) -> String {
    let (mut scale, mut unit) = (1_usize, 0);
    while unit + 1 < UNITS.len() && bytes / scale >= 1000 {
        scale *= 1000;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} B");
    }
    let tenths = bytes / (scale / 10);
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

pub struct KindRow {
    pub name: &'static str,
    pub live: usize,
    pub ended: usize,
}

pub struct MonthRow {
    pub month: String,
    pub created: usize,
    pub versions: usize,
    pub size: String,
}

pub struct MachineRow {
    pub machine: String,
    pub versions: usize,
    pub size: String,
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
    let href = with_query(Place::Stats.route(), &parameters);
    Choice::new(Link::plain(text, href), current)
}

fn held_line(holdings: &Holdings) -> String {
    let mut held = vec![
        counted(holdings.document_count(), "document", "documents"),
        counted(holdings.version_count, "version", "versions"),
        size(holdings.size),
    ];
    if holdings.forked_count > 0 {
        held.push(format!("{} forked", holdings.forked_count));
    }
    held.join(" · ")
}

fn unfiled_line(holdings: &Holdings) -> Option<String> {
    let unfiled = holdings.unfiled.as_ref()?;
    let mut parts = Vec::new();
    if unfiled.usage_size > 0 {
        parts.push(format!("usage log {}", size(unfiled.usage_size)));
    }
    if unfiled.unreadable_count > 0 {
        parts.push(counted(
            unfiled.unreadable_count,
            "unreadable file",
            "unreadable files",
        ));
    }
    if unfiled.unknown_count > 0 {
        parts.push(counted(
            unfiled.unknown_count,
            "document of an unknown kind",
            "documents of unknown kinds",
        ));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
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
        let holdings = &stats.holdings;
        StatsPage {
            layout: Layout::at(Some(Place::Stats)),
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
            kinds: (holdings.kinds.iter())
                .map(|count| KindRow {
                    name: count.kind.plural(),
                    live: count.live,
                    ended: count.ended,
                })
                .collect(),
            held: held_line(holdings),
            unfiled: unfiled_line(holdings),
            written: (stats.writes.months.iter())
                .map(|written| MonthRow {
                    month: written.month.clone(),
                    created: written.created_count,
                    versions: written.version_count,
                    size: size(written.size),
                })
                .collect(),
            machines: (stats.writes.machines.iter())
                .map(|written| MachineRow {
                    machine: written.machine.label.clone(),
                    versions: written.version_count,
                    size: size(written.size),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn a_size_is_given_in_the_largest_unit_it_reaches_cut_to_a_tenth() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1000), "1.0 KB");
        assert_eq!(size(999_999), "999.9 KB");
        assert_eq!(size(26_180_000), "26.1 MB");
        assert_eq!(size(3_000_000_000), "3.0 GB");
        assert_eq!(size(1_000_000_000_000_000), "1000.0 TB");
    }
}
