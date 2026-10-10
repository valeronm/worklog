use std::fmt::Write as _;

use similar::TextDiff;

use super::{Render, change_or_ending, line, written};
use crate::app::show::{Diff, History, Shown, ShownVersion, Unread};

const CONTEXT_LINES: usize = 3;

fn version_fields(version: &ShownVersion) -> Vec<String> {
    vec![
        version.version.short().to_owned(),
        written(&version.written),
        version.machine.clone(),
        change_or_ending(&version.change, version.ended.as_deref()),
    ]
}

fn named(text: &mut String, version: &ShownVersion) {
    let _ = writeln!(text, "==== {}", version_fields(version).join("  "));
    text.push_str(&version.text);
    if !version.text.is_empty() && !version.text.ends_with('\n') {
        text.push('\n');
    }
}

fn unread(label: &str, files: &[Unread]) -> Vec<String> {
    files
        .iter()
        .map(|file| {
            let version = file.version.short();
            let why = file.why.text();
            format!("{label}: version {version} does not read: {why}")
        })
        .collect()
}

impl Render for Shown {
    fn text(&self) -> String {
        let mut text = String::new();
        match self {
            Shown::Document(document) => {
                let ended = document.heads.iter().any(|head| head.ended.is_some());
                for head in &document.heads {
                    if document.forked || ended {
                        named(&mut text, head);
                    } else {
                        text.push_str(&head.text);
                    }
                }
            }
            Shown::Version(version) => named(&mut text, version),
        }
        text
    }

    fn notes(&self) -> Vec<String> {
        let Shown::Document(document) = self else {
            return Vec::new();
        };
        let mut notes = unread(&document.label, &document.unreadable);
        if document.forked {
            notes.push(format!("{}: forked: resolve it", document.label));
        }
        notes
    }
}

impl Render for History {
    fn text(&self) -> String {
        let mut text = String::new();
        for version in &self.versions {
            let mut fields = version_fields(version);
            if version.head {
                fields.push("head".to_owned());
            }
            line(&mut text, &fields);
        }
        text
    }

    fn notes(&self) -> Vec<String> {
        unread(&self.label, &self.unreadable)
    }
}

fn changes(before: &str, after: &str) -> String {
    if before == after {
        return "no changes\n".to_owned();
    }
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(CONTEXT_LINES)
        .to_string()
}

impl Render for Diff {
    fn text(&self) -> String {
        let mut text = String::new();
        for side in &self.sides {
            let against = match &side.against {
                None => "nothing".to_owned(),
                Some(version) if side.held => version.short().to_owned(),
                Some(version) => format!("{} (not held)", version.short()),
            };
            let _ = writeln!(text, "--- {against}\n+++ {}", self.label);
            text.push_str(&changes(&side.before, &self.after));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{WRITTEN, stamp};
    use super::*;
    use crate::app::show::{ShownDocument, ShownVersion, Side};
    use crate::domain::document::Unreadable;
    use crate::domain::id::VersionId;
    use crate::domain::testing::lantern;
    use crate::domain::version::ReadError;

    const SHOWN: &str = "2026-10-09T18:22:41.118+01:00";

    fn version(seed: &[u8], text: &str) -> ShownVersion {
        ShownVersion {
            document: lantern(),
            label: "lantern".to_owned(),
            version: VersionId::of(seed),
            written: stamp(WRITTEN),
            machine: "desk".to_owned(),
            change: "save".to_owned(),
            parents: Vec::new(),
            head: true,
            ended: None,
            text: text.to_owned(),
        }
    }

    fn document(heads: Vec<ShownVersion>) -> ShownDocument {
        ShownDocument {
            document: lantern(),
            kind: Some("topic".to_owned()),
            label: "lantern".to_owned(),
            former: Vec::new(),
            forked: heads.len() > 1,
            heads,
            unreadable: Vec::new(),
        }
    }

    fn short(seed: &[u8]) -> String {
        VersionId::of(seed).short().to_owned()
    }

    #[test]
    fn a_live_document_is_its_text_alone() {
        let shown = Shown::Document(document(vec![version(
            b"left",
            "+++\nname = \"lantern\"\n",
        )]));
        assert_eq!(shown.text(), "+++\nname = \"lantern\"\n");
        assert_eq!(shown.notes(), Vec::<String>::new());
    }

    #[test]
    fn a_forked_document_names_each_head_and_notes_the_fork() {
        let mut right = version(b"right", "right\n");
        right.machine = "phone".to_owned();
        right.change = "rename".to_owned();
        let shown = Shown::Document(document(vec![version(b"left", "left\n"), right]));
        assert_eq!(
            shown.text(),
            format!(
                "==== {}  {SHOWN}  desk  save\nleft\n==== {}  {SHOWN}  phone  rename\nright\n",
                short(b"left"),
                short(b"right")
            )
        );
        assert_eq!(shown.notes(), ["lantern: forked: resolve it"]);
    }

    #[test]
    fn a_head_whose_text_ends_without_a_newline_is_closed_before_the_next_heading() {
        let heads = vec![version(b"left", "left"), version(b"right", "right")];
        assert_eq!(
            Shown::Document(document(heads)).text(),
            format!(
                "==== {}  {SHOWN}  desk  save\nleft\n==== {}  {SHOWN}  desk  save\nright\n",
                short(b"left"),
                short(b"right")
            )
        );
    }

    #[test]
    fn an_ended_document_names_its_one_head() {
        let mut ended = version(b"left", "left\n");
        ended.ended = Some("retired".to_owned());
        ended.change = "end".to_owned();
        let shown = Shown::Document(document(vec![ended]));
        assert_eq!(
            shown.text(),
            format!(
                "==== {}  {SHOWN}  desk  ended retired\nleft\n",
                short(b"left")
            )
        );
        assert_eq!(shown.notes(), Vec::<String>::new());
    }

    #[test]
    fn a_file_that_does_not_read_is_a_note_beside_what_does() {
        let mut held = document(vec![version(b"left", "left\n")]);
        held.unreadable = [
            Unreadable {
                version: VersionId::of(b"lost"),
                why: ReadError::Corrupt,
            },
            Unreadable {
                version: VersionId::of(b"later"),
                why: ReadError::Newer { format: 9 },
            },
        ]
        .iter()
        .map(Unread::from)
        .collect();
        let notes = [
            format!(
                "lantern: version {} does not read: the bytes do not hash to the file's name",
                short(b"lost")
            ),
            format!(
                "lantern: version {} does not read: written in format 9, a newer one",
                short(b"later")
            ),
        ];
        let shown = Shown::Document(held.clone());
        assert_eq!(shown.text(), "left\n");
        assert_eq!(shown.notes(), notes);

        let history = History {
            document: lantern(),
            label: "lantern".to_owned(),
            versions: held.heads,
            unreadable: held.unreadable,
        };
        assert_eq!(history.notes(), notes);
    }

    #[test]
    fn a_version_is_its_text_under_a_line_naming_it() {
        let mut old = version(b"left", "left\n");
        old.head = false;
        let shown = Shown::Version(old);
        assert_eq!(
            shown.text(),
            format!("==== {}  {SHOWN}  desk  save\nleft\n", short(b"left"))
        );
        assert_eq!(shown.notes(), Vec::<String>::new());
    }

    #[test]
    fn a_history_is_a_line_a_version_with_the_heads_marked() {
        let mut first = version(b"first", "");
        first.head = false;
        first.change = "new".to_owned();
        first.machine = "phone".to_owned();
        let history = History {
            document: lantern(),
            label: "lantern".to_owned(),
            versions: vec![version(b"left", ""), first],
            unreadable: Vec::new(),
        };
        assert_eq!(
            history.text(),
            format!(
                "{}  {SHOWN}  desk  save  head\n{}  {SHOWN}  phone  new\n",
                short(b"left"),
                short(b"first")
            )
        );
        assert_eq!(history.notes(), Vec::<String>::new());
        let mut ended = version(b"left", "");
        ended.ended = Some("false".to_owned());
        ended.change = "end".to_owned();
        let closed = History {
            versions: vec![ended],
            ..history.clone()
        };
        assert_eq!(
            closed.text(),
            format!("{}  {SHOWN}  desk  ended false  head\n", short(b"left"))
        );
        let none = History {
            versions: Vec::new(),
            ..history
        };
        assert_eq!(none.text(), "");
    }

    fn hashed(short: &str) -> VersionId {
        VersionId::parse(&format!("b3-{short:0<64}")).expect("the start of a hash")
    }

    fn diff(sides: &[(Option<&str>, &str)], after: &str) -> Diff {
        Diff {
            label: "lantern".to_owned(),
            sides: sides
                .iter()
                .map(|(against, before)| Side {
                    against: against.map(hashed),
                    held: true,
                    before: (*before).to_owned(),
                })
                .collect(),
            after: after.to_owned(),
        }
    }

    #[test]
    fn a_diff_is_unified_over_lines_with_three_of_context() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let after = "a\nb\nc\nd\nE\nf\ng\nh\ni\nj\nk\n";
        assert_eq!(
            diff(&[(Some("1a2b3c4d5e6f"), before)], after).text(),
            "--- 1a2b3c4d5e6f\n+++ lantern\n@@ -2,9 +2,10 @@\n b\n c\n d\n-e\n+E\n f\n g\n h\n i\n j\n+k\n"
        );
    }

    #[test]
    fn a_diff_has_one_block_a_side_and_says_when_a_side_is_equal() {
        let after = "name = \"lantern\"\n";
        assert_eq!(
            diff(
                &[
                    (Some("1a2b3c4d5e6f"), after),
                    (Some("6f5e4d3c2b1a"), "name = \"lamp\"\n")
                ],
                after
            )
            .text(),
            "--- 1a2b3c4d5e6f\n+++ lantern\nno changes\n\
             --- 6f5e4d3c2b1a\n+++ lantern\n@@ -1 +1 @@\n-name = \"lamp\"\n+name = \"lantern\"\n"
        );
        assert_eq!(
            diff(&[(None, "")], "a\nb\n").text(),
            "--- nothing\n+++ lantern\n@@ -0,0 +1,2 @@\n+a\n+b\n"
        );
        assert_eq!(diff(&[], after).text(), "");
    }

    #[test]
    fn a_side_whose_version_is_not_held_says_so_over_everything_added() {
        let mut unheld = diff(&[(Some("1a2b3c4d5e6f"), "")], "a\n");
        unheld.sides[0].held = false;
        assert_eq!(
            unheld.text(),
            "--- 1a2b3c4d5e6f (not held)\n+++ lantern\n@@ -0,0 +1 @@\n+a\n"
        );
    }

    #[test]
    fn a_text_that_ends_without_a_newline_is_marked_under_its_last_line() {
        assert_eq!(
            diff(&[(Some("1a2b3c4d5e6f"), "a\nb")], "a\nc\n").text(),
            "--- 1a2b3c4d5e6f\n+++ lantern\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n"
        );
    }
}
