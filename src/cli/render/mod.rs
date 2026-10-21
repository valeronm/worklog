//! A line a read prints separates its fields with two spaces; the one line a write command
//! prints with one.

mod activity;
mod check;
mod context;
mod documents;
mod listings;
mod search;
#[cfg(test)]
mod testing;

use std::fmt::Write as _;

use serde::Serialize;

use super::json::{Pinned, nothing, printed};
use crate::app::draft::{DraftRow, Opener};
use crate::app::followup::Made;
use crate::app::{DraftRef, Failure, Row, Written, counted};
use crate::render::{change_or_ending, ended, written};

#[cfg(test)]
pub(super) use context::CONTEXT_BYTES;
pub(super) use listings::Refusals;
#[cfg(test)]
pub(super) use testing::assert_fits;

/// `text` goes to stdout and `notes` to stderr.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Output {
    pub(super) text: String,
    pub(super) notes: Vec<String>,
    pub(super) exit: i32,
    pub(super) hits: Option<usize>,
}

impl Output {
    /// A command that has nothing to give and succeeds, `note` saying why.
    pub(super) fn note(note: String, json: bool) -> Output {
        Output {
            text: if json { nothing() } else { String::new() },
            notes: vec![note],
            exit: 0,
            hits: None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Stopped {
    pub(super) done: Option<Output>,
    pub(super) failure: Failure,
}

impl From<Failure> for Stopped {
    fn from(failure: Failure) -> Stopped {
        Stopped {
            done: None,
            failure,
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

    fn hits(&self) -> Option<usize> {
        None
    }
}

/// The text of `result` with its notes, or under `json` the result itself with no notes,
/// since it carries what they say.
pub(super) fn output<T: Render + Serialize + Pinned>(result: &T, json: bool) -> Output {
    output_as(result, result, json)
}

/// The text of `view` with its notes, or under `json` `result` itself with no notes.
pub(super) fn output_as<T, V>(result: &T, view: &V, json: bool) -> Output
where
    T: Render + Serialize + Pinned,
    V: Render,
{
    let (text, notes) = if json {
        (printed(result), Vec::new())
    } else {
        (view.text(), view.notes())
    };
    Output {
        text,
        notes,
        exit: result.exit(),
        hits: result.hits(),
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
        fields.push(ended(reason));
    }
    fields
}

fn filed_counts(fact_count: usize, idea_count: usize, open_count: usize) -> Option<String> {
    let counts = crate::render::filed_counts(fact_count, idea_count, open_count);
    (!counts.is_empty()).then(|| format!("({})", counts.join(", ")))
}

fn row_line(text: &mut String, row: &Row) {
    line(text, &row_fields(row, Vec::new()));
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

impl Render for Vec<DraftRow> {
    fn text(&self) -> String {
        let fields: Vec<[String; 4]> = (self.iter())
            .map(|row| {
                let (opener, age) = (opener(&row.opener), age(row.age_seconds));
                [row.label.clone(), opener, age, row.location.clone()]
            })
            .collect();
        let widest = |at: usize| {
            let widths = fields.iter().map(|row| row[at].chars().count());
            widths.max().unwrap_or(0)
        };
        let (label, opener, age) = (widest(0), widest(1), widest(2));
        let mut text = String::new();
        for [label_of, opener_of, age_of, location] in &fields {
            let _ = writeln!(
                text,
                "{label_of:<label$}  {opener_of:<opener$}  {age_of:<age$}  {location}"
            );
        }
        text
    }
}

fn opener(opener: &Opener) -> String {
    match opener {
        Opener::ThisSession => "this session".to_owned(),
        Opener::NoSession => "no session".to_owned(),
        Opener::OtherSession { agent, session } => {
            format!("{agent} {}", session.chars().take(8).collect::<String>())
        }
    }
}

fn age(seconds: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const TWO_DAYS: u64 = 2 * DAY;
    const WEEK: u64 = 7 * DAY;
    const TWO_WEEKS: u64 = 2 * WEEK;
    let (count, one, several) = match seconds {
        0..MINUTE => return "just now".to_owned(),
        MINUTE..HOUR => (seconds / MINUTE, "minute", "minutes"),
        HOUR..TWO_DAYS => (seconds / HOUR, "hour", "hours"),
        TWO_DAYS..TWO_WEEKS => (seconds / DAY, "day", "days"),
        _ => (seconds / WEEK, "week", "weeks"),
    };
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    format!("{} ago", counted(count, one, several))
}

#[cfg(test)]
mod tests {
    use super::testing::{row, stamp};
    use super::*;
    use crate::app::show::{Shown, ShownDocument, Unread};
    use crate::domain::id::VersionId;
    use crate::domain::testing::{atlas, lantern};
    use crate::domain::version::Stamp;

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
        assert_eq!(made.hits, None);
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
            reads_as: None,
            label: "lantern".to_owned(),
            former: Vec::new(),
            forked: false,
            ended: None,
            heads: Vec::new(),
            unreadable: vec![Unread::from(&crate::domain::document::Unreadable {
                version: VersionId::of(b"lost"),
                why: crate::domain::version::ReadError::Corrupt,
            })],
            linked_from: Vec::new(),
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
    fn a_draft_is_listed_by_its_label_its_opener_its_age_and_its_place() {
        let row = |label: &str, opener: Opener, age_seconds: u64, location: &str| DraftRow {
            document: lantern(),
            kind: "topic".to_owned(),
            label: label.to_owned(),
            location: location.to_owned(),
            opener,
            touched: Stamp::parse("2026-09-04T10:00:00+00:00").unwrap(),
            age_seconds,
        };
        let other = Opener::OtherSession {
            agent: "Lamp".to_owned(),
            session: "7f3a91c0-55e2".to_owned(),
        };
        assert_eq!(
            vec![
                row("lantern", Opener::ThisSession, 240, "/desk/a.md"),
                row("atlas", other, 2 * 86_400, "/desk/b.md"),
                row("2026-09-04-lamp", Opener::NoSession, 30, "/desk/c.md"),
            ]
            .text(),
            "lantern          this session   4 minutes ago  /desk/a.md\n\
             atlas            Lamp 7f3a91c0  2 days ago     /desk/b.md\n\
             2026-09-04-lamp  no session     just now       /desk/c.md\n"
        );
        assert_eq!(Vec::<DraftRow>::new().text(), "");
    }

    #[test]
    fn a_draft_s_age_is_counted_in_the_largest_unit_that_reads_plainly() {
        let ages = [
            (59, "just now"),
            (60, "1 minute ago"),
            (3_599, "59 minutes ago"),
            (3_600, "1 hour ago"),
            (2 * 86_400 - 1, "47 hours ago"),
            (2 * 86_400, "2 days ago"),
            (14 * 86_400 - 1, "13 days ago"),
            (14 * 86_400, "2 weeks ago"),
        ];
        for (seconds, shown) in ages {
            assert_eq!(age(seconds), shown, "{seconds}");
        }
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
