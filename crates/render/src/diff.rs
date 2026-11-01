//! Two texts as the lines that differ between them, in the hunks of a unified diff.

use similar::{ChangeTag, TextDiff};

const CONTEXT_LINES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Context,
    Added,
    Removed,
}

impl Mark {
    /// The character a unified diff opens the line with.
    #[must_use]
    pub fn sign(self) -> char {
        match self {
            Mark::Context => ' ',
            Mark::Added => '+',
            Mark::Removed => '-',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub mark: Mark,
    pub text: String,
    /// The newline, carriage return or both that end the line in its text; empty for a last
    /// line nothing ends.
    pub terminator: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@` line that opens it.
    pub header: String,
    pub lines: Vec<Line>,
}

/// The line a unified diff puts after a line that ends its text with no newline.
pub const UNTERMINATED: &str = "\\ No newline at end of file";

/// The hunks that turn `before` into `after`; none when the texts are equal.
#[must_use]
pub fn hunks(before: &str, after: &str) -> Vec<Hunk> {
    let diff = TextDiff::from_lines(before, after);
    let mut unified = diff.unified_diff();
    unified.context_radius(CONTEXT_LINES);
    let mut hunks = Vec::new();
    for hunk in unified.iter_hunks() {
        let lines = (hunk.iter_changes())
            .map(|change| {
                let value = change.value();
                let terminator = ["\r\n", "\n", "\r", ""]
                    .into_iter()
                    .find(|terminator| value.ends_with(terminator))
                    .unwrap_or_default();
                Line {
                    mark: match change.tag() {
                        ChangeTag::Equal => Mark::Context,
                        ChangeTag::Insert => Mark::Added,
                        ChangeTag::Delete => Mark::Removed,
                    },
                    text: value[..value.len() - terminator.len()].to_owned(),
                    terminator,
                }
            })
            .collect();
        hunks.push(Hunk {
            header: hunk.header().to_string(),
            lines,
        });
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(mark: Mark, text: &str) -> Line {
        Line {
            mark,
            text: text.to_owned(),
            terminator: "\n",
        }
    }

    #[test]
    fn equal_texts_have_no_hunks() {
        assert_eq!(hunks("a\nb\n", "a\nb\n"), []);
        assert_eq!(hunks("", ""), []);
    }

    #[test]
    fn a_change_is_a_hunk_of_marked_lines_under_its_header() {
        assert_eq!(
            hunks("a\nb\nc\n", "a\nB\nc\nd\n"),
            [Hunk {
                header: "@@ -1,3 +1,4 @@".to_owned(),
                lines: vec![
                    line(Mark::Context, "a"),
                    line(Mark::Removed, "b"),
                    line(Mark::Added, "B"),
                    line(Mark::Context, "c"),
                    line(Mark::Added, "d"),
                ],
            }]
        );
        assert_eq!(
            hunks("", "a\n"),
            [Hunk {
                header: "@@ -0,0 +1 @@".to_owned(),
                lines: vec![line(Mark::Added, "a")],
            }]
        );
    }

    #[test]
    fn changes_more_than_their_context_apart_are_hunks_of_their_own() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let after = "A\nb\nc\nd\ne\nf\ng\nh\ni\nj\nK\n";
        let found = hunks(before, after);
        let headers: Vec<&str> = found.iter().map(|hunk| hunk.header.as_str()).collect();
        assert_eq!(headers, ["@@ -1,4 +1,4 @@", "@@ -8,4 +8,4 @@"]);
        assert_eq!(found[0].lines.len(), 5);
        assert_eq!(found[1].lines[4], line(Mark::Added, "K"));
    }

    #[test]
    fn a_last_line_with_no_newline_is_marked_so() {
        let found = hunks("a\nb", "a\nc\n");
        assert_eq!(
            found[0].lines,
            [
                line(Mark::Context, "a"),
                Line {
                    mark: Mark::Removed,
                    text: "b".to_owned(),
                    terminator: "",
                },
                line(Mark::Added, "c"),
            ]
        );
        assert_eq!(hunks("a", "a\n").len(), 1);
    }

    #[test]
    fn a_line_keeps_the_carriage_return_that_ends_it_apart_from_its_text() {
        let found = hunks("a\r\nb\rc\n", "a\r\nB\rc\n");
        let ended: Vec<(Mark, &str, &str)> = (found[0].lines.iter())
            .map(|line| (line.mark, line.text.as_str(), line.terminator))
            .collect();
        assert_eq!(
            ended,
            [
                (Mark::Context, "a", "\r\n"),
                (Mark::Removed, "b", "\r"),
                (Mark::Added, "B", "\r"),
                (Mark::Context, "c", "\n"),
            ]
        );
    }

    #[test]
    fn each_mark_has_the_sign_a_unified_diff_gives_it() {
        let signs = [Mark::Context, Mark::Added, Mark::Removed].map(Mark::sign);
        assert_eq!(signs, [' ', '+', '-']);
    }
}
