use std::fmt::Write as _;

use super::Render;
use crate::app::check::Check;
use crate::render::check_summary;

impl Render for Check {
    fn text(&self) -> String {
        let mut text = String::new();
        for problem in &self.problems {
            let _ = writeln!(text, "{}: {}", problem.label, problem.what);
        }
        for notice in &self.notices {
            let _ = writeln!(text, "notice: {}: {}", notice.label, notice.what);
        }
        let summary = check_summary(self);
        let _ = writeln!(
            text,
            "{}, {}, {}",
            summary.problems, summary.notices, summary.forks
        );
        text
    }

    fn exit(&self) -> i32 {
        i32::from(!self.problems.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::super::output;
    use super::*;
    use crate::app::check::Finding;

    #[test]
    fn a_check_counts_what_it_found_marks_a_notice_and_exits_one_on_a_problem() {
        let finding = |label: &str, what: &str| Finding {
            document: None,
            label: label.to_owned(),
            ended: None,
            what: what.to_owned(),
        };
        let clean = Check::default();
        assert_eq!(clean.text(), "0 problems, 0 notices, 0 forks\n");
        assert_eq!(clean.exit(), 0);
        let mut found = Check {
            problems: Vec::new(),
            notices: vec![
                finding("atlas", "is placed nowhere"),
                finding("phone", "is old"),
            ],
            fork_count: 1,
        };
        assert_eq!(output(&found, false).exit, 0);
        found.problems.push(finding("lantern", "has no summary"));
        assert_eq!(
            found.text(),
            "lantern: has no summary\nnotice: atlas: is placed nowhere\nnotice: phone: is old\n\
             1 problem, 2 notices, 1 fork\n"
        );
        assert_eq!(output(&found, false).exit, 1);
    }
}
