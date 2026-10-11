//! A document as it stands, and a topic as its document over what is filed under it.

use askama::Template;

use super::home::TopicLine;
use super::links::{Nav, Place, Stamped, Text, doc_href, history_href, marks};
use super::listing::{Item, entry_items, fact_items, followup_items};
use crate::app::FollowupRow;
use crate::app::list::{EntryRow, FactRow, TopicRow};
use crate::app::show::{FieldItem, ShownDocument, ShownField, ShownVersion};
use crate::render::{ended, unread_notes};
use crate::web::markdown::{Markup, to_html};

pub struct Field {
    pub name: String,
    pub values: Vec<Text>,
}

impl From<&ShownField> for Field {
    fn from(field: &ShownField) -> Field {
        let values = (field.values.iter()).map(|item| match item {
            FieldItem::Text { text } => Text::plain(text),
            FieldItem::Reference { reference } => Text::naming(reference),
        });
        Field {
            name: field.name.clone(),
            values: values.collect(),
        }
    }
}

pub struct Head {
    pub stamp: Stamped,
    pub fields: Vec<Field>,
    pub body: Markup,
}

impl From<&ShownVersion> for Head {
    fn from(version: &ShownVersion) -> Head {
        Head {
            stamp: Stamped {
                change: Some(version.change.clone()),
                ended: version.ended.as_deref().map(ended),
                ..Stamped::new(&version.version, &version.written, &version.machine)
            },
            fields: version.fields.iter().map(Field::from).collect(),
            body: to_html(&version.body, &version.links, doc_href),
        }
    }
}

pub struct Sheet {
    pub label: String,
    pub kind: Option<String>,
    pub history: String,
    pub forked: bool,
    pub marks: Vec<String>,
    pub former: Vec<String>,
    pub notes: Vec<String>,
    pub heads: Vec<Head>,
}

impl From<&ShownDocument> for Sheet {
    fn from(document: &ShownDocument) -> Sheet {
        Sheet {
            label: document.label.clone(),
            kind: document.kind.clone(),
            history: history_href(&document.document),
            forked: document.forked,
            marks: marks(document.forked, document.ended.as_deref()),
            former: document.former.clone(),
            notes: unread_notes(&document.unreadable),
            heads: document.heads.iter().map(Head::from).collect(),
        }
    }
}

#[derive(Template)]
#[template(path = "document.html")]
pub struct DocumentPage {
    pub nav: Nav,
    pub document: Sheet,
}

impl DocumentPage {
    pub fn new(document: &ShownDocument) -> DocumentPage {
        DocumentPage {
            nav: Nav::at(None),
            document: Sheet::from(document),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Parts,
    Facts,
    Ideas,
    Followups,
    Entries,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Parts,
        Section::Facts,
        Section::Ideas,
        Section::Followups,
        Section::Entries,
    ];
    pub const FIRST: Section = Section::Facts;

    pub fn named(show: &str) -> Option<Section> {
        Section::ALL
            .into_iter()
            .find(|section| section.key() == show)
    }

    fn key(self) -> &'static str {
        match self {
            Section::Parts => "parts",
            Section::Facts => "facts",
            Section::Ideas => "ideas",
            Section::Followups => "followups",
            Section::Entries => "entries",
        }
    }

    fn listing(self) -> Option<Place> {
        match self {
            Section::Parts => None,
            Section::Facts => Some(Place::Facts),
            Section::Ideas => Some(Place::Ideas),
            Section::Followups => Some(Place::Followups),
            Section::Entries => Some(Place::Entries),
        }
    }

    fn name(self) -> &'static str {
        self.listing().map_or("Parts", Place::title)
    }
}

pub struct Held<'r> {
    pub parts: &'r [TopicRow],
    pub facts: &'r [FactRow],
    pub ideas: &'r [FactRow],
    pub followups: &'r [FollowupRow],
    pub entries: &'r [EntryRow],
}

pub struct Tab {
    pub name: &'static str,
    pub href: String,
    pub count: usize,
    pub current: bool,
}

#[derive(Template)]
#[template(path = "topic.html")]
pub struct TopicPage {
    pub nav: Nav,
    pub document: Sheet,
    pub tabs: Vec<Tab>,
    pub section: &'static str,
    // None for the parts, which no listing page shows.
    pub listing: Option<String>,
    pub parts: Vec<TopicLine>,
    pub rows: Vec<Item>,
}

impl TopicPage {
    pub fn new(document: &ShownDocument, held: &Held, section: Section) -> TopicPage {
        let (parts, rows) = match section {
            Section::Parts => (held.parts.iter().map(TopicLine::from).collect(), Vec::new()),
            Section::Facts => (Vec::new(), fact_items(held.facts)),
            Section::Ideas => (Vec::new(), fact_items(held.ideas)),
            Section::Followups => (Vec::new(), followup_items(held.followups)),
            Section::Entries => (Vec::new(), entry_items(held.entries)),
        };
        let here = doc_href(&document.document);
        let tabs = Section::ALL.into_iter().map(|tab| Tab {
            name: tab.name(),
            href: if tab == Section::FIRST {
                here.clone()
            } else {
                format!("{here}?show={}", tab.key())
            },
            count: match tab {
                Section::Parts => held.parts.len(),
                Section::Facts => held.facts.len(),
                Section::Ideas => held.ideas.len(),
                Section::Followups => held.followups.len(),
                Section::Entries => held.entries.len(),
            },
            current: tab == section,
        });
        TopicPage {
            nav: Nav::at(Some(Place::Topics)),
            document: Sheet::from(document),
            tabs: tabs.collect(),
            section: section.name(),
            listing: (section.listing())
                .map(|listing| format!("{}?topic={}", listing.route(), document.document)),
            parts,
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::NOW;
    use crate::web::pages::testing::version;

    #[test]
    fn a_head_links_its_version_its_machine_and_each_document_a_field_refers_to() {
        let shown = version(b"left");
        let head = Head::from(&shown);
        let stamp = &head.stamp;
        assert_eq!(stamp.version.text, shown.version.short());
        assert_eq!(stamp.version.href, format!("/doc/{}", shown.version));
        assert_eq!(stamp.machine.href, "/doc/09090909090909090909090909090909");
        assert_eq!(stamp.written, "2026-10-09T18:22:41.118+01:00");
        assert_eq!(stamp.written_full, NOW);
        let values: Vec<Vec<(&str, Option<&str>)>> = (head.fields.iter())
            .map(|field| {
                (field.values.iter())
                    .map(|value| (value.text.as_str(), value.href.as_deref()))
                    .collect()
            })
            .collect();
        assert_eq!(
            values,
            [
                vec![("lantern", None)],
                vec![
                    ("atlas", Some("/doc/04040404040404040404040404040404")),
                    ("phone", Some("/doc/05050505050505050505050505050505"))
                ],
            ]
        );
        assert_eq!(head.body.to_string(), "<p>Lit.</p>\n");
    }
}
