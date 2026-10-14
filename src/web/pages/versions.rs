//! The versions of a document: its history, the log, the forks, and one version against its parents.

use askama::Template;

use super::document::Head;
use super::links::{Line, Link, Nav, Place, Stamped, Text, history_href, marks, version_link};
use crate::app::list::{ForkHead, ForkRow};
use crate::app::search::Logged;
use crate::app::show::{Compared, History, Side};
use crate::app::{Row, counted};
use crate::render::diff::{Hunk, Mark, UNTERMINATED, hunks};
use crate::render::{against, change_or_ending, ended, unread_notes};
use crate::web::markdown::{marked, summary_html};

#[derive(Template)]
#[template(path = "history.html")]
pub struct HistoryPage {
    pub nav: Nav,
    pub document: Link,
    pub notes: Vec<String>,
    pub rows: Vec<Stamped>,
}

impl HistoryPage {
    pub fn new(history: &History) -> HistoryPage {
        let rows = (history.versions.iter())
            .map(|version| Stamped {
                head: version.head,
                change: Some(change_or_ending(&version.change, version.ended.as_deref())),
                ..Stamped::new(&version.version, &version.written, &version.machine)
            })
            .collect();
        HistoryPage {
            nav: Nav::at(None),
            document: Link::to(&history.document, &history.label, history.ended.as_deref()),
            notes: unread_notes(&history.unreadable),
            rows,
        }
    }
}

fn ended_of(row: Option<&Row>) -> Option<&str> {
    row.and_then(|row| row.ended.as_deref())
}

pub(super) fn document_link(compared: &Compared) -> Link {
    Link::to(
        &compared.document,
        &compared.label,
        compared.document_ended.as_deref(),
    )
}

pub struct LogRow {
    pub stamp: Stamped,
    pub document: Link,
}

#[derive(Template)]
#[template(path = "log.html")]
pub struct LogPage {
    pub nav: Nav,
    pub count: String,
    pub rows: Vec<LogRow>,
}

impl LogPage {
    pub fn new(versions: &[Logged]) -> LogPage {
        let rows = versions.iter().map(|logged| LogRow {
            stamp: Stamped {
                change: Some(change_or_ending(&logged.change, logged.ended.as_deref())),
                ..Stamped::new(&logged.version, &logged.written, &logged.machine)
            },
            document: Link::to(
                &logged.document,
                &logged.label,
                ended_of(logged.row.as_ref()),
            ),
        });
        LogPage {
            nav: Nav::at(Some(Place::Log)),
            count: counted(versions.len(), "newest version", "newest versions"),
            rows: rows.collect(),
        }
    }
}

pub struct Fork {
    pub line: Line,
    pub heads: Vec<Stamped>,
}

impl From<&ForkRow> for Fork {
    fn from(fork: &ForkRow) -> Fork {
        let row = fork.row.as_ref();
        let head = |head: &ForkHead| Stamped {
            ended: head.ended.as_deref().map(ended),
            ..Stamped::new(&head.version, &head.written, &head.machine)
        };
        Fork {
            line: Line {
                title: Link::to(&fork.document, &fork.label, ended_of(row)),
                label: marked(&fork.label, &[]),
                summary: summary_html(row.map_or("", |row| &row.summary), None),
                counts: Vec::new(),
                marks: marks(false, ended_of(row)),
            },
            heads: fork.heads.iter().map(head).collect(),
        }
    }
}

#[derive(Template)]
#[template(path = "forks.html")]
pub struct ForksPage {
    pub nav: Nav,
    pub count: String,
    pub rows: Vec<Fork>,
}

impl ForksPage {
    pub fn new(forks: &[ForkRow]) -> ForksPage {
        ForksPage {
            nav: Nav::at(Some(Place::Forks)),
            count: counted(forks.len(), "forked document", "forked documents"),
            rows: forks.iter().map(Fork::from).collect(),
        }
    }
}

pub struct DiffLine {
    pub class: &'static str,
    pub text: String,
}

fn diff_lines(found: Vec<Hunk>) -> Vec<DiffLine> {
    let mut lines = Vec::new();
    for hunk in found {
        lines.push(DiffLine {
            class: "hunk",
            text: hunk.header,
        });
        for line in hunk.lines {
            let class = match line.mark {
                Mark::Context => "context",
                Mark::Added => "added",
                Mark::Removed => "removed",
            };
            lines.push(DiffLine {
                class,
                text: format!("{}{}", line.mark.sign(), line.text),
            });
            if line.terminator.is_empty() {
                lines.push(DiffLine {
                    class: "note",
                    text: UNTERMINATED.to_owned(),
                });
            }
        }
    }
    lines
}

pub struct Changes {
    pub against: Text,
    pub lines: Vec<DiffLine>,
}

impl Changes {
    fn new(side: &Side, after: &str) -> Changes {
        let against = against(side);
        Changes {
            against: match against.held {
                Some(parent) => Text::Link(Link::plain(against.words, version_link(parent).href)),
                None => Text::Plain(against.words),
            },
            lines: diff_lines(hunks(&side.before, after)),
        }
    }
}

#[derive(Template)]
#[template(path = "version.html")]
pub struct VersionPage {
    pub nav: Nav,
    pub document: Link,
    pub history: String,
    pub is_head: bool,
    pub sides: Vec<Changes>,
    pub head: Head,
}

impl VersionPage {
    pub fn new(compared: &Compared) -> VersionPage {
        let version = &compared.version;
        VersionPage {
            nav: Nav::at(None),
            document: document_link(compared),
            history: history_href(&compared.document),
            is_head: version.head,
            sides: (compared.sides.iter())
                .map(|side| Changes::new(side, &version.text))
                .collect(),
            head: Head::from(version),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::show::ShownVersion;
    use crate::app::testing::history_row;
    use crate::domain::id::{DocumentId, VersionId};
    use crate::web::pages::testing::version;

    #[test]
    fn a_row_shows_a_change_up_to_its_colon_and_keeps_the_whole_as_a_title() {
        let row = |seed: &[u8], change: &str| {
            history_row(ShownVersion {
                change: change.to_owned(),
                ..version(seed)
            })
        };
        let history = History {
            document: DocumentId::from_bytes([3; 16]),
            label: "lantern".to_owned(),
            ended: None,
            versions: vec![
                row(b"back", "reopen: lit again: by the desk"),
                row(b"first", "new"),
            ],
            unreadable: Vec::new(),
        };
        let html = HistoryPage::new(&history).render().expect("a page");
        assert!(
            html.contains(
                "<td class=\"mono\"><span title=\"reopen: lit again: by the desk\">reopen</span></td>"
            ),
            "{html}"
        );
        assert!(html.contains("<td class=\"mono\">new</td>"), "{html}");
    }

    #[test]
    fn a_version_s_stamp_shows_a_change_up_to_its_colon() {
        let compared = Compared {
            document: DocumentId::from_bytes([3; 16]),
            label: "lantern".to_owned(),
            document_ended: None,
            version: ShownVersion {
                change: "reopen: lit again".to_owned(),
                ..version(b"back")
            },
            sides: Vec::new(),
        };
        let html = VersionPage::new(&compared).render().expect("a page");
        assert!(
            html.contains("</a> · <span title=\"reopen: lit again\">reopen</span> on <a"),
            "{html}"
        );
    }

    fn side(against: Option<&[u8]>, held: bool, before: &str) -> Side {
        Side {
            against: against.map(VersionId::of),
            held,
            before: before.to_owned(),
        }
    }

    #[test]
    fn a_side_is_named_by_what_it_is_against_and_linked_when_that_is_held() {
        let compared = Compared {
            document: DocumentId::from_bytes([3; 16]),
            label: "lantern".to_owned(),
            document_ended: None,
            version: version(b"joined"),
            sides: vec![
                side(Some(b"left"), true, "a\nc\n"),
                side(Some(b"right"), false, ""),
                side(None, true, ""),
                side(Some(b"same"), true, "a\nb"),
            ],
        };
        let page = VersionPage::new(&compared);
        let named: Vec<(&str, Option<&str>)> = (page.sides.iter())
            .map(|side| {
                let against = &side.against;
                (
                    against.words(),
                    against.link().map(|link| link.href.as_str()),
                )
            })
            .collect();
        let (left, same) = (VersionId::of(b"left"), VersionId::of(b"same"));
        let not_held = format!("{} (not held)", VersionId::of(b"right").short());
        assert_eq!(
            named,
            [
                (left.short(), Some(format!("/doc/{left}").as_str())),
                (not_held.as_str(), None),
                ("nothing", None),
                (same.short(), Some(format!("/doc/{same}").as_str())),
            ]
        );
        let lines = |at: usize| -> Vec<(&str, &str)> {
            (page.sides[at].lines.iter())
                .map(|line| (line.class, line.text.as_str()))
                .collect()
        };
        assert_eq!(
            lines(0),
            [
                ("hunk", "@@ -1,2 +1,2 @@"),
                ("context", " a"),
                ("removed", "-c"),
                ("added", "+b"),
                ("note", "\\ No newline at end of file"),
            ]
        );
        assert_eq!(lines(1)[0], ("hunk", "@@ -0,0 +1,2 @@"));
        assert_eq!(lines(1), lines(2));
        assert!(lines(3).is_empty());

        let html = page.render().expect("a page");
        assert!(
            html.contains(&format!("<p class=\"parents\">against {not_held}</p>")),
            "{html}"
        );
        assert!(
            html.contains(
                "<span class=\"added\">+b</span>\
                 <span class=\"note\">\\ No newline at end of file</span></pre>"
            ),
            "{html}"
        );
    }
}
