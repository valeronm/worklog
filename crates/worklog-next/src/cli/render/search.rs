use std::fmt::Write as _;

use super::{INDENT, Render, change_or_ending, line, row_line, written};
use crate::app::search::{Hit, Logged};

const LINES_SHOWN: usize = 3;

impl Render for Vec<Hit> {
    fn text(&self) -> String {
        let mut text = String::new();
        for hit in self {
            row_line(&mut text, &hit.row);
            for matching in hit.lines.iter().take(LINES_SHOWN) {
                let _ = writeln!(text, "{INDENT}{matching}");
            }
            let more = hit.lines.len().saturating_sub(LINES_SHOWN);
            if more > 0 {
                let _ = writeln!(text, "{INDENT}… {more} more");
            }
        }
        text
    }
}

impl Render for Vec<Logged> {
    fn text(&self) -> String {
        let mut text = String::new();
        for version in self {
            let fields = [
                &written(&version.written),
                &version.machine,
                version.version.short(),
                &version.label,
                &change_or_ending(&version.change, version.ended.as_deref()),
            ];
            line(&mut text, &fields);
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::{WRITTEN, row, stamp};
    use super::*;
    use crate::domain::id::VersionId;
    use crate::domain::testing::{atlas, lantern};

    #[test]
    fn a_hit_is_its_row_over_at_most_three_of_its_lines() {
        let hit = |id, label: &str, lines: &[&str]| Hit {
            row: row(id, label, "Pin four"),
            lines: lines.iter().map(ToString::to_string).collect(),
        };
        let hits = vec![
            hit(&lantern(), "lantern/relay", &["Pin four", "The relay."]),
            hit(&atlas(), "atlas/relay-map", &[]),
            hit(&lantern(), "lantern", &["one", "two", "three"]),
            hit(&atlas(), "atlas", &["one", "two", "three", "four", "five"]),
        ];
        assert_eq!(
            hits.text(),
            "7f3a91c0  lantern/relay  Pin four\n\
             \x20   Pin four\n\
             \x20   The relay.\n\
             a71a5000  atlas/relay-map  Pin four\n\
             7f3a91c0  lantern  Pin four\n\
             \x20   one\n\
             \x20   two\n\
             \x20   three\n\
             a71a5000  atlas  Pin four\n\
             \x20   one\n\
             \x20   two\n\
             \x20   three\n\
             \x20   … 2 more\n"
        );
        assert_eq!(Vec::<Hit>::new().text(), "");
    }

    #[test]
    fn a_logged_version_is_a_line_naming_its_document_by_label_or_by_short_id() {
        let logged = |seed: &[u8], machine: &str, change: &str| Logged {
            document: atlas(),
            label: atlas().short().to_owned(),
            version: VersionId::of(seed),
            written: stamp(WRITTEN),
            machine: machine.to_owned(),
            change: change.to_owned(),
            ended: None,
            row: None,
        };
        let mut renamed = logged(b"left", "desk", "rename");
        renamed.document = lantern();
        renamed.label = "lantern/relay".to_owned();
        renamed.row = Some(row(&lantern(), "lantern/relay", "Pin four"));
        let short = |seed: &[u8]| VersionId::of(seed).short().to_owned();
        assert_eq!(
            vec![renamed, logged(b"right", "phone", "new")].text(),
            format!(
                "2026-10-09T18:22:41.118+01:00  desk  {}  lantern/relay  rename\n\
                 2026-10-09T18:22:41.118+01:00  phone  {}  a71a5000  new\n",
                short(b"left"),
                short(b"right")
            )
        );
        let mut ended = logged(b"left", "desk", "end");
        ended.ended = Some("retired".to_owned());
        assert_eq!(
            vec![ended].text(),
            format!(
                "2026-10-09T18:22:41.118+01:00  desk  {}  a71a5000  ended retired\n",
                short(b"left")
            )
        );
        assert_eq!(Vec::<Logged>::new().text(), "");
    }
}
