use std::fmt::Write as _;

use super::{Render, line};
use crate::app::activity::Activity;
use crate::app::counted;
use crate::render::calendar::{Cell, TOP_LEVEL, calendar};

const WEEKDAYS: [&str; 7] = ["Mon", "", "Wed", "", "Fri", "", ""];
const LEFT: usize = 4;
// By level, from a day holding no entry to the busiest day.
const SHADES: [char; TOP_LEVEL + 1] = ['·', '░', '▒', '▓', '█'];

fn shade(cell: Option<&Cell>) -> char {
    cell.map_or(' ', |cell| SHADES[cell.level])
}

impl Render for Activity {
    fn text(&self) -> String {
        let grid = calendar(self);
        let mut months = " ".repeat(LEFT);
        for month in &grid.months {
            let at = LEFT + month.week;
            months.extend(std::iter::repeat_n(' ', at.saturating_sub(months.len())));
            months.push_str(month.name);
        }
        let mut lines = vec![months];
        for (day, name) in WEEKDAYS.iter().enumerate() {
            let mut line = format!("{name:<LEFT$}");
            line.extend(grid.weeks.iter().map(|week| shade(week[day].as_ref())));
            lines.push(line);
        }
        let mut text = String::new();
        for line in lines {
            text.push_str(line.trim_end());
            text.push('\n');
        }
        let entries = counted(self.entry_count, "entry", "entries");
        let _ = writeln!(text, "{entries} from {} to {}", self.from, self.to);
        if !self.years.is_empty() {
            let mut fields = vec!["years".to_owned()];
            fields.extend(self.years.iter().map(ToString::to_string));
            line(&mut text, &fields);
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::activity::Day;
    use crate::domain::schema::Date;

    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }

    #[test]
    fn a_grid_prints_a_row_a_weekday_a_shade_a_day_and_the_count_under_it() {
        let activity = Activity {
            from: date("2026-09-28"),
            to: date("2026-10-28"),
            year: None,
            days: vec![
                Day {
                    date: date("2026-09-28"),
                    entry_count: 1,
                },
                Day {
                    date: date("2026-10-07"),
                    entry_count: 5,
                },
            ],
            entry_count: 6,
            years: vec![2026, 2025],
        };
        assert_eq!(
            activity.text(),
            "     Oct\n\
             Mon ░····\n    ·····\n\
             Wed ·█···\n    ····\n\
             Fri ····\n    ····\n    ····\n\
             6 entries from 2026-09-28 to 2026-10-28\n\
             years  2026  2025\n"
        );
        let none = Activity {
            days: Vec::new(),
            entry_count: 0,
            years: Vec::new(),
            ..activity
        };
        assert!(
            none.text()
                .ends_with("····\n0 entries from 2026-09-28 to 2026-10-28\n")
        );
    }
}
