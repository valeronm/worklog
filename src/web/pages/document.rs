//! A document as it stands, and a topic as its document over what is filed under it.

use askama::Template;

use super::home::TopicLine;
use super::links::{Link, Nav, Place, Stamped, Text, doc_href, history_href, marks};
use super::listing::{Item, entry_items, fact_items, followup_items, leave_out};
use crate::app::FollowupRow;
use crate::app::list::{ClaimRow, EntryRow, FactRow, TopicRow};
use crate::app::show::Linker;
use crate::app::show::{FieldItem, ShownDocument, ShownField, ShownVersion};
use crate::domain::schema::KindOf;
use crate::render::{ended, unread_notes};
use crate::web::markdown::{Markup, summary_html, to_html};

pub struct Field {
    pub name: String,
    pub values: Vec<Text>,
}

impl From<&ShownField> for Field {
    fn from(field: &ShownField) -> Field {
        Field {
            name: field.name.clone(),
            values: (field.values.iter())
                .map(|item| match item {
                    FieldItem::Text { text } => Text::plain(text),
                    FieldItem::Reference { reference } => Text::naming(reference),
                })
                .collect(),
        }
    }
}

pub struct Head {
    pub stamp: Stamped,
    pub summary: Option<Markup>,
    pub fields: Vec<Field>,
    pub body: Markup,
}

impl From<&ShownVersion> for Head {
    fn from(version: &ShownVersion) -> Head {
        let mut summary = None;
        let mut fields = Vec::new();
        for field in &version.fields {
            match field.values.as_slice() {
                [FieldItem::Text { text }] if field.name == "summary" => {
                    summary = Some(summary_html(text, Some((&version.links, doc_href))));
                }
                _ => fields.push(Field::from(field)),
            }
        }
        Head {
            stamp: Stamped {
                change: Some(version.change.clone()),
                ended: version.ended.as_deref().map(ended),
                ..Stamped::new(&version.version, &version.written, &version.machine)
            },
            summary,
            fields,
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
    pub linked_from: Vec<Item>,
    pub cited_by: Vec<Item>,
}

fn linker_items(linkers: &[&Linker]) -> Vec<Item> {
    (linkers.iter())
        .map(|linker| Item::new(&linker.row, &linker.by.join(", ")))
        .collect()
}

impl From<&ShownDocument> for Sheet {
    fn from(document: &ShownDocument) -> Sheet {
        let (cited_by, linked_from): (Vec<_>, Vec<_>) =
            (document.linked_from.iter()).partition(|linker| linker.row.kind == KindOf::Entry);
        Sheet {
            label: document.label.clone(),
            kind: document.kind.clone(),
            history: history_href(&document.document),
            forked: document.forked,
            marks: marks(document.forked, document.ended.as_deref()),
            former: document.former.clone(),
            notes: unread_notes(&document.unreadable),
            heads: document.heads.iter().map(Head::from).collect(),
            linked_from: linker_items(&linked_from),
            cited_by: linker_items(&cited_by),
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
    Claims,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::Parts,
        Section::Facts,
        Section::Ideas,
        Section::Followups,
        Section::Entries,
        Section::Claims,
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
            Section::Claims => "claims",
        }
    }

    fn listing(self) -> Option<Place> {
        match self {
            Section::Parts | Section::Claims => None,
            Section::Facts => Some(Place::Facts),
            Section::Ideas => Some(Place::Ideas),
            Section::Followups => Some(Place::Followups),
            Section::Entries => Some(Place::Entries),
        }
    }

    fn name(self) -> &'static str {
        (self.listing()).map_or_else(
            || match self {
                Section::Parts => "Parts",
                _ => "Claims",
            },
            Place::title,
        )
    }
}

pub struct Held<'r> {
    pub parts: &'r [TopicRow],
    pub facts: &'r [FactRow],
    pub ideas: &'r [FactRow],
    pub followups: &'r [FollowupRow],
    pub entries: &'r [EntryRow],
    pub claims: &'r [ClaimRow],
}

/// A claimed topic with the claims placing it.
pub struct ClaimLine {
    pub topic: Text,
    pub places: Vec<ClaimPlace>,
}

pub struct ClaimPlace {
    /// The claim's directory, or `anywhere`, linking the claim.
    pub directory: Link,
    pub marks: Vec<String>,
}

// The claims come sorted by topic.
fn by_topic(claims: &[ClaimRow]) -> impl Iterator<Item = &[ClaimRow]> {
    claims.chunk_by(|a, b| a.topic.document == b.topic.document)
}

fn claim_lines(claims: &[ClaimRow]) -> Vec<ClaimLine> {
    by_topic(claims)
        .map(|of_topic| ClaimLine {
            topic: Text::naming(&of_topic[0].topic),
            places: (of_topic.iter())
                .map(|row| ClaimPlace {
                    directory: Link::to(
                        &row.document,
                        row.directory.as_deref().unwrap_or("anywhere"),
                        None,
                    ),
                    marks: (marks(row.forked, None).into_iter())
                        .chain(row.missing().map(|_| "missing".to_owned()))
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

pub enum Body {
    Parts(Vec<TopicLine>),
    Claims(Vec<ClaimLine>),
    Rows(Vec<Item>),
}

impl Body {
    pub fn is_empty(&self) -> bool {
        match self {
            Body::Parts(parts) => parts.is_empty(),
            Body::Claims(claims) => claims.is_empty(),
            Body::Rows(rows) => rows.is_empty(),
        }
    }
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
    // None for the parts and the claims, which no listing page shows.
    pub listing: Option<String>,
    pub body: Body,
}

impl TopicPage {
    pub fn new(document: &ShownDocument, held: &Held, section: Section) -> TopicPage {
        let here = doc_href(&document.document);
        let rows = |mut rows: Vec<Item>| {
            leave_out(&mut rows, &here);
            Body::Rows(rows)
        };
        let body = match section {
            Section::Parts => Body::Parts(held.parts.iter().map(TopicLine::from).collect()),
            Section::Claims => Body::Claims(claim_lines(held.claims)),
            Section::Facts => rows(fact_items(held.facts)),
            Section::Ideas => rows(fact_items(held.ideas)),
            Section::Followups => rows(followup_items(held.followups)),
            Section::Entries => rows(entry_items(held.entries)),
        };
        let claimed = by_topic(held.claims).count();
        // Only a machine topic holds claims.
        let shown = |tab: &Section| *tab != Section::Claims || claimed > 0 || *tab == section;
        let tabs = Section::ALL.into_iter().filter(shown).map(|tab| Tab {
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
                Section::Claims => claimed,
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
            body,
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
                    .map(|value| (value.words(), value.link().map(|link| link.href.as_str())))
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
