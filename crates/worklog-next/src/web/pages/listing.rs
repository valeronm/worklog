//! The pages that are a list of documents, a row each.

use std::fmt::Write as _;

use askama::Template;

use super::links::{Line, Link, Nav, Place, Text, doc_href, doc_links, encoded};
use crate::app::check::{Check, Finding};
use crate::app::list::{EntryRow, FactRow};
use crate::app::search::{Hit, Query};
use crate::app::show::Shown;
use crate::app::{FollowupRow, Reference, Row, TriggerShown, counted};
use crate::render::{check_summary, hit_lines, trigger_state};

pub struct Item {
    pub line: Line,
    pub lead: Vec<Text>,
    pub topics: Vec<Link>,
    pub details: Vec<Vec<Text>>,
    pub lines: Vec<String>,
    pub due: bool,
}

impl Item {
    fn new(row: &Row, lead: &str) -> Item {
        Item {
            line: Line::from(row),
            lead: vec![Text::plain(lead)],
            topics: doc_links(&row.topics),
            details: Vec::new(),
            lines: Vec::new(),
            due: false,
        }
    }
}

impl From<&FollowupRow> for Item {
    fn from(followup: &FollowupRow) -> Item {
        let state = trigger_state(followup);
        let why = match &followup.trigger {
            Some(TriggerShown::LookAgain { why, .. }) => Some(vec![Text::plain(why)]),
            Some(TriggerShown::Touching { .. }) | None => None,
        };
        let naming =
            |word: &str, document: &Reference| vec![Text::plain(word), Text::naming(document)];
        let entry = (followup.entry.as_ref()).map(|entry| naming("in", entry));
        let about = (followup.about.as_ref()).map(|about| naming("about", about));
        let mut item = Item::new(&followup.row, &state.words);
        item.lead.extend(state.touching.map(Text::naming));
        Item {
            details: [why, entry, about].into_iter().flatten().collect(),
            due: followup.due,
            ..item
        }
    }
}

pub(super) fn fact_items(rows: &[FactRow]) -> Vec<Item> {
    (rows.iter())
        .map(|fact| Item::new(&fact.row, &format!("confirmed {}", fact.confirmed)))
        .collect()
}

pub(super) fn entry_items(rows: &[EntryRow]) -> Vec<Item> {
    (rows.iter())
        .map(|entry| Item::new(&entry.row, &entry.date.to_string()))
        .collect()
}

pub(super) fn followup_items(rows: &[FollowupRow]) -> Vec<Item> {
    rows.iter().map(Item::from).collect()
}

pub struct Scope<'q> {
    pub topic: Option<&'q str>,
    // The document `topic` names.
    pub under: Option<Link>,
    pub ended: bool,
}

pub fn shown_link(shown: &Shown) -> Link {
    let (label, document) = match shown {
        Shown::Document(document) => (&document.label, &document.document),
        Shown::Version(compared) => (&compared.label, &compared.document),
    };
    Link {
        text: label.clone(),
        href: doc_href(document),
    }
}

#[derive(Template)]
#[template(path = "listing.html")]
pub struct Listing {
    pub nav: Nav,
    pub title: &'static str,
    pub count: String,
    pub under: Option<Link>,
    pub ended_switch: Link,
    pub rows: Vec<Item>,
}

impl Listing {
    fn new(place: Place, (one, several): (&str, &str), scope: Scope, rows: Vec<Item>) -> Listing {
        let mut href = place.route().to_owned();
        let mut sign = '?';
        if let Some(topic) = scope.topic {
            let _ = write!(href, "{sign}topic={}", encoded(topic));
            sign = '&';
        }
        let text = if scope.ended {
            "unended only"
        } else {
            let _ = write!(href, "{sign}ended=1");
            "ended too"
        };
        Listing {
            nav: Nav::at(Some(place)),
            title: place.title(),
            count: counted(rows.len(), one, several),
            under: scope.under,
            ended_switch: Link {
                text: text.to_owned(),
                href,
            },
            rows,
        }
    }

    pub fn facts(rows: &[FactRow], scope: Scope) -> Listing {
        let nouns = ("fact", "facts");
        Listing::new(Place::Facts, nouns, scope, fact_items(rows))
    }

    pub fn ideas(rows: &[FactRow], scope: Scope) -> Listing {
        let nouns = ("idea", "ideas");
        Listing::new(Place::Ideas, nouns, scope, fact_items(rows))
    }

    pub fn entries(rows: &[EntryRow], scope: Scope) -> Listing {
        let nouns = ("entry", "entries");
        Listing::new(Place::Entries, nouns, scope, entry_items(rows))
    }

    pub fn followups(rows: &[FollowupRow], scope: Scope) -> Listing {
        let nouns = ("follow-up", "follow-ups");
        Listing::new(Place::Followups, nouns, scope, followup_items(rows))
    }
}

#[derive(Template)]
#[template(path = "search.html")]
pub struct SearchPage<'q> {
    pub nav: Nav,
    pub asked: &'q Query<'q>,
    pub problem: Option<String>,
    pub count: Option<String>,
    pub rows: Vec<Item>,
}

impl<'q> SearchPage<'q> {
    pub fn form(asked: &'q Query<'q>) -> SearchPage<'q> {
        SearchPage {
            nav: Nav::at(None),
            asked,
            problem: None,
            count: None,
            rows: Vec::new(),
        }
    }

    pub fn found(asked: &'q Query<'q>, hits: &[Hit]) -> SearchPage<'q> {
        let rows = hits.iter().map(|hit| Item {
            lines: hit_lines(&hit.lines),
            ..Item::new(&hit.row, hit.row.kind.word())
        });
        SearchPage {
            count: Some(counted(hits.len(), "document matches", "documents match")),
            rows: rows.collect(),
            ..SearchPage::form(asked)
        }
    }

    pub fn failed(asked: &'q Query<'q>, why: String) -> SearchPage<'q> {
        SearchPage {
            problem: Some(why),
            ..SearchPage::form(asked)
        }
    }
}

pub struct Found {
    pub title: Text,
    pub what: String,
}

fn findings(found: &[Finding]) -> Vec<Found> {
    (found.iter())
        .map(|finding| Found {
            title: Text {
                text: finding.label.clone(),
                href: finding.document.as_ref().map(doc_href),
            },
            what: finding.what.clone(),
        })
        .collect()
}

#[derive(Template)]
#[template(path = "check.html")]
pub struct CheckPage {
    pub nav: Nav,
    pub problem_count: String,
    pub notice_count: String,
    pub forks: Link,
    pub problems: Vec<Found>,
    pub notices: Vec<Found>,
}

impl CheckPage {
    pub fn new(check: &Check) -> CheckPage {
        let summary = check_summary(check);
        CheckPage {
            nav: Nav::at(Some(Place::Check)),
            problem_count: summary.problems,
            notice_count: summary.notices,
            forks: Link {
                text: summary.forks,
                href: Place::Forks.route().to_owned(),
            },
            problems: findings(&check.problems),
            notices: findings(&check.notices),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::id::DocumentId;

    #[test]
    fn a_finding_links_its_label_when_it_has_a_document_and_escapes_it() {
        let finding = |document: Option<DocumentId>| Finding {
            document,
            label: "<b>lamp</b>".to_owned(),
            what: "holds <i>this</i>".to_owned(),
        };
        let lamp = DocumentId::from_bytes([7; 16]);
        let html = CheckPage::new(&Check {
            problems: vec![finding(Some(lamp.clone()))],
            notices: vec![finding(None)],
            fork_count: 0,
        })
        .render()
        .expect("a page");
        assert!(
            html.contains(&format!(
                "<li><a class=\"title\" href=\"/doc/{lamp}\">&#60;b&#62;lamp&#60;/b&#62;</a> \
                 <span class=\"problem\">holds &#60;i&#62;this&#60;/i&#62;</span></li>"
            )),
            "{html}"
        );
        assert!(
            html.contains(
                "<li><span class=\"title\">&#60;b&#62;lamp&#60;/b&#62;</span> \
                 <span class=\"notice\">holds &#60;i&#62;this&#60;/i&#62;</span></li>"
            ),
            "{html}"
        );
        assert!(!html.contains("<b>"), "{html}");
    }
}
