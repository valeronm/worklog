use std::collections::{BTreeMap, BTreeSet};

use worklog::domain::id::DocumentId;
use worklog::domain::schema::address::displayed;
use worklog::domain::schema::{Content, Followup, Trigger};
use worklog::domain::version::Links;

use super::resolve::Unsettled;
use super::{Document, Frame, Origin, Source, Version};
use crate::old::{Kind, linked, shaped};
use crate::question::Open;

struct Rewrite<'f, 'a, 'd> {
    frame: &'f Frame<'a, 'd>,
    ids: BTreeSet<DocumentId>,
    // The address of each entry whose last version is planned whole.
    addresses: BTreeMap<DocumentId, String>,
}

// A short id names one planned document, at the length the new store prints or longer.
pub(crate) fn short_among<'i>(id: &'i DocumentId, all: &BTreeSet<DocumentId>) -> &'i str {
    let whole = id.as_str();
    let shared = |prefix: &str| {
        all.iter()
            .any(|other| other != id && other.as_str().starts_with(prefix))
    };
    let lengths = id.short().len()..whole.len();
    let enough = lengths
        .into_iter()
        .find(|length| !shared(&whole[..*length]));
    enough.map_or(whole, |length| &whole[..length])
}

fn address(document: &Document<'_>) -> Option<(DocumentId, String)> {
    let record = document.head()?.record.as_ref()?;
    let entry = matches!(record.content, Content::Entry(_));
    let address = displayed(&record.content, None).filter(|_| entry)?;
    Some((document.id.clone(), address))
}

fn summary_of(content: &mut Content) -> Option<&mut String> {
    match content {
        Content::Topic(topic) => Some(&mut topic.summary),
        Content::Fact(fact) => Some(&mut fact.summary),
        Content::Entry(entry) => Some(&mut entry.summary),
        Content::Followup(followup) => Some(&mut followup.summary),
        Content::Claim(_) => None,
    }
}

fn why_of(content: &mut Content) -> Option<&mut String> {
    match content {
        Content::Followup(Followup {
            trigger: Some(Trigger::LookAgain { why, .. }),
            ..
        }) => Some(why),
        _ => None,
    }
}

struct Row {
    id: Option<DocumentId>,
    typed: String,
}

impl Rewrite<'_, '_, '_> {
    // An entry whose last name is not settled keeps the text typed.
    fn target(&self, typed: &str) -> (String, Option<DocumentId>) {
        let found =
            shaped(typed).and_then(|kind| match self.frame.resolver.document(kind, typed) {
                Ok(found) => Some((kind, found?)),
                Err(Unsettled) => None,
            });
        let Some((kind, found)) = found else {
            return (typed.to_owned(), None);
        };
        let text = match kind {
            Kind::Entry => self.addresses.get(&found.id).cloned(),
            Kind::Followup => Some(short_among(&found.id, &self.ids).to_owned()),
            Kind::Fact | Kind::Topic => None,
        };
        let text = text.unwrap_or_else(|| typed.to_owned());
        (text, Some(found.id.clone()))
    }

    // A clash is asked about where its text is held: a version of the store, or an answer
    // of the file. A version that carries the text of the one it follows adds none.
    fn clash(&self, origin: &Origin<'_>, link: &str, why: &str) {
        let stored = match origin {
            Origin::Converted(source) => *source,
            Origin::Generated {
                cause,
                answered: None,
            } => Source {
                document: cause.document,
                version: cause.version,
                stone: None,
            },
            Origin::Generated {
                answered: Some(key),
                ..
            }
            | Origin::Authored {
                answered: Some(key),
            } => {
                let why = format!("{link} {why}");
                let key = key.clone();
                return self.frame.questions.ask(Open::Answer { key, why });
            }
            Origin::Authored { answered: None } => return,
        };
        self.frame.refused(&stored, "links", link, why);
    }

    fn link(&self, typed: &str) -> String {
        format!("[[{}]]", self.target(typed).0)
    }

    // The new store keeps a row for a link of the summary and of the body, and for no
    // other text.
    fn version(&self, version: &mut Version<'_>) {
        let mut rows: BTreeMap<String, Row> = BTreeMap::new();
        let mut clashing = BTreeSet::new();
        let mut make = |typed: &str| {
            let (text, id) = self.target(typed);
            let link = format!("[[{text}]]");
            match rows.get(&text) {
                Some(held) if held.id == id => {}
                Some(held) => {
                    let why = format!(
                        "is what [[{}]] and [[{typed}]] both become, and they name different \
                         documents",
                        held.typed
                    );
                    clashing.insert((link.clone(), why));
                }
                None => {
                    let typed = typed.to_owned();
                    rows.insert(text, Row { id, typed });
                }
            }
            link
        };
        if let Some(record) = version.record.as_mut() {
            if let Some(summary) = summary_of(&mut record.content) {
                *summary = linked(summary, &mut make);
            }
            if let Some(why) = why_of(&mut record.content) {
                *why = linked(why, |typed| self.link(typed));
            }
            if let Some(ending) = record.ending.as_mut() {
                ending.note = linked(&ending.note, |typed| self.link(typed));
            }
        }
        version.body = linked(&version.body, &mut make);
        version.links = Links(rows.into_iter().map(|(text, row)| (text, row.id)).collect());
        for (link, why) in clashing {
            self.clash(&version.origin, &link, &why);
        }
    }
}

pub(crate) fn rewrite(frame: &Frame<'_, '_>, documents: &mut [Document<'_>]) {
    let rewrite = Rewrite {
        frame,
        ids: documents
            .iter()
            .map(|document| document.id.clone())
            .collect(),
        addresses: documents.iter().filter_map(address).collect(),
    };
    for version in documents
        .iter_mut()
        .flat_map(|document| &mut document.versions)
    {
        rewrite.version(version);
    }
}
