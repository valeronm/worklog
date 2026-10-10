//! A line a read prints separates its fields with two spaces; the one line a write command
//! prints, and each line of `drafts`, with one.

mod check;
mod documents;
mod listings;
mod search;
#[cfg(test)]
mod testing;

use std::fmt::Write as _;

use serde::Serialize;

use super::json::{Pinned, nothing, printed};
use crate::app::context::Context;
use crate::app::draft::DraftRow;
use crate::app::followup::Made;
use crate::app::setup::Bound;
use crate::app::{DraftRef, Row, Written};
use crate::domain::version::Stamp;

/// What one command gives back: `text` for stdout, `notes` for stderr, and its exit code.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Output {
    pub(super) text: String,
    pub(super) notes: Vec<String>,
    pub(super) exit: i32,
}

impl Output {
    /// A command that has nothing to give and succeeds, `note` saying why.
    pub(super) fn note(note: String, json: bool) -> Output {
        Output {
            text: if json { nothing() } else { String::new() },
            notes: vec![note],
            exit: 0,
        }
    }
}

/// A use case's result as a person reads it.
pub(super) trait Render {
    fn text(&self) -> String;

    /// What a person may act on that is no part of the data asked for.
    fn notes(&self) -> Vec<String> {
        Vec::new()
    }

    fn exit(&self) -> i32 {
        0
    }
}

/// The text of `result` with its notes, or under `json` the result itself with no notes,
/// since it carries what they say.
pub(super) fn output<T: Render + Serialize + Pinned>(result: &T, json: bool) -> Output {
    let (text, notes) = if json {
        (printed(result), Vec::new())
    } else {
        (result.text(), result.notes())
    };
    Output {
        text,
        notes,
        exit: result.exit(),
    }
}

const INDENT: &str = "    ";

fn line<S: AsRef<str>>(text: &mut String, fields: &[S]) {
    let shown: Vec<&str> = fields
        .iter()
        .map(AsRef::as_ref)
        .filter(|field| !field.is_empty())
        .collect();
    let _ = writeln!(text, "{}", shown.join("  "));
}

fn row_fields(row: &Row, between: Vec<String>) -> Vec<String> {
    let short = row.document.short();
    let mut fields = vec![short.to_owned()];
    // A followup has no name, and its label is the short id.
    if row.label != short {
        fields.push(row.label.clone());
    }
    fields.push(row.summary.clone());
    fields.extend(between);
    if row.forked {
        fields.push("forked".to_owned());
    }
    if let Some(reason) = &row.ended {
        fields.push(format!("ended {reason}"));
    }
    fields
}

fn row_line(text: &mut String, row: &Row) {
    line(text, &row_fields(row, Vec::new()));
}

fn change_or_ending(change: &str, ended: Option<&str>) -> String {
    match ended {
        Some(reason) => format!("ended {reason}"),
        None => change.to_owned(),
    }
}

fn written(stamp: &Stamp) -> String {
    let stamp = stamp.to_string();
    let Some(dot) = stamp.find('.') else {
        return stamp;
    };
    let fraction = dot + 1;
    let digits = stamp[fraction..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    let shown = fraction + digits.min(3);
    format!("{}{}", &stamp[..shown], &stamp[fraction + digits..])
}

impl Render for () {
    fn text(&self) -> String {
        String::new()
    }
}

impl Render for Written {
    fn text(&self) -> String {
        format!("{} {}\n", self.label, self.version.short())
    }
}

impl Render for Vec<Written> {
    fn text(&self) -> String {
        self.iter().map(Render::text).collect()
    }
}

impl Render for DraftRef {
    fn text(&self) -> String {
        format!("{}\n", self.location)
    }
}

impl Render for Made {
    fn text(&self) -> String {
        match self {
            Made::Written(written) => written.text(),
            Made::Draft(draft) => draft.text(),
        }
    }
}

impl Render for Bound {
    fn text(&self) -> String {
        let how = if self.created { "created" } else { "bound" };
        format!("{} {how}\n", self.label)
    }
}

impl Render for Vec<DraftRow> {
    fn text(&self) -> String {
        let mut text = String::new();
        for row in self {
            let _ = writeln!(text, "{} {}", row.label, row.location);
        }
        text
    }
}

impl Render for Context {
    fn text(&self) -> String {
        let mut text = String::new();
        for topic in &self.topics {
            line(&mut text, &[&topic.label, &topic.summary]);
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{row, stamp};
    use super::*;
    use crate::app::show::{Shown, ShownDocument, Unread};
    use crate::domain::id::VersionId;
    use crate::domain::testing::{atlas, lantern};

    fn stored(label: &str) -> Written {
        Written {
            document: lantern(),
            label: label.to_owned(),
            version: VersionId::of(b"lantern"),
        }
    }

    fn draft() -> DraftRef {
        DraftRef {
            document: lantern(),
            location: "/home/desk/drafts/lantern.md".to_owned(),
        }
    }

    #[test]
    fn a_result_becomes_its_text_with_no_notes_and_its_own_exit_code() {
        let made = output(&draft(), false);
        assert_eq!(made.text, "/home/desk/drafts/lantern.md\n");
        assert_eq!((made.notes.len(), made.exit), (0, 0));
        let json = output(&draft(), true);
        assert_eq!(
            json.text,
            format!(
                "{{\n  \"document\": \"{}\",\n  \"path\": \"/home/desk/drafts/lantern.md\"\n}}\n",
                lantern()
            )
        );
        assert_eq!((json.notes.len(), json.exit), (0, 0));
        assert_eq!(output(&(), false).text, "");

        let noted = Output::note("this host is not set up".to_owned(), false);
        assert_eq!(noted.text, "");
        assert_eq!(noted.notes, ["this host is not set up"]);
        assert_eq!(noted.exit, 0);
        let noted = Output::note("this host is not set up".to_owned(), true);
        assert_eq!(noted.text, "null\n");
        assert_eq!(noted.notes, ["this host is not set up"]);
        assert_eq!(noted.exit, 0);
    }

    #[test]
    fn a_result_s_notes_go_with_its_text_and_stay_out_of_its_json() {
        let unheld = Shown::Document(ShownDocument {
            document: lantern(),
            kind: Some("topic".to_owned()),
            label: "lantern".to_owned(),
            former: Vec::new(),
            forked: false,
            heads: Vec::new(),
            unreadable: vec![Unread::from(&crate::domain::document::Unreadable {
                version: VersionId::of(b"lost"),
                why: crate::domain::version::ReadError::Corrupt,
            })],
        });
        assert_eq!(output(&unheld, false).notes.len(), 1);
        let json = output(&unheld, true);
        assert_eq!(json.notes, Vec::<String>::new());
        assert!(json.text.contains("\"unreadable\""), "{}", json.text);
    }

    #[test]
    fn a_stored_version_is_its_label_and_the_start_of_its_hash() {
        let one = stored("lantern/relay");
        let short = one.version.short().to_owned();
        assert_eq!(short.len(), 12);
        assert_eq!(one.text(), format!("lantern/relay {short}\n"));
        assert_eq!(
            vec![one.clone(), stored("lantern/fuse")].text(),
            format!("lantern/relay {short}\nlantern/fuse {short}\n")
        );
        assert_eq!(Vec::<Written>::new().text(), "");
        assert_eq!(Made::Written(one.clone()).text(), one.text());
        assert_eq!(Made::Draft(draft()).text(), draft().text());
    }

    #[test]
    fn a_bound_host_says_whether_its_topic_was_made() {
        let mut made = Bound {
            document: atlas(),
            label: "atlas".to_owned(),
            created: true,
        };
        assert_eq!(made.text(), "atlas created\n");
        made.created = false;
        assert_eq!(made.text(), "atlas bound\n");
    }

    #[test]
    fn a_draft_is_listed_by_its_label_and_its_place() {
        let row = |label: &str, location: &str| DraftRow {
            document: lantern(),
            kind: "topic".to_owned(),
            label: label.to_owned(),
            location: location.to_owned(),
        };
        assert_eq!(
            vec![row("lantern", "/desk/a.md"), row("atlas", "/desk/b.md")].text(),
            "lantern /desk/a.md\natlas /desk/b.md\n"
        );
        assert_eq!(Vec::<DraftRow>::new().text(), "");
    }

    #[test]
    fn a_row_is_its_short_id_its_label_and_its_summary_then_its_marks() {
        let fields = |row: &Row| row_fields(row, Vec::new()).join("  ");
        let mut lamp = row(&lantern(), "lantern", "A lamp");
        assert_eq!(fields(&lamp), "7f3a91c0  lantern  A lamp");
        lamp.forked = true;
        assert_eq!(fields(&lamp), "7f3a91c0  lantern  A lamp  forked");
        lamp.ended = Some("retired".to_owned());
        assert_eq!(
            fields(&lamp),
            "7f3a91c0  lantern  A lamp  forked  ended retired"
        );

        let mut text = String::new();
        row_line(&mut text, &row(&atlas(), "atlas", ""));
        row_line(&mut text, &row(&atlas(), "a71a5000", "Check"));
        assert_eq!(text, "a71a5000  atlas\na71a5000  Check\n");
        lamp.forked = false;
        assert_eq!(
            row_fields(&lamp, vec!["uses desk".to_owned()]).join("  "),
            "7f3a91c0  lantern  A lamp  uses desk  ended retired"
        );
    }

    #[test]
    fn a_written_time_is_shown_to_the_millisecond() {
        assert_eq!(
            written(&stamp("2026-10-09T18:22:41.118204+01:00")),
            "2026-10-09T18:22:41.118+01:00"
        );
        assert_eq!(
            written(&stamp("2026-10-09T18:22:41.5Z")),
            "2026-10-09T18:22:41.5Z"
        );
        assert_eq!(
            written(&stamp("2026-10-09T18:22:41-03:00")),
            "2026-10-09T18:22:41-03:00"
        );
    }
}
