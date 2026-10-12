//! The answers a person gives to what a conversion cannot settle from the store alone, and
//! the record of which of them a run asked for.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::key::{ConversionKey, DocumentField, DocumentKey, Folded, Key, Placement, TopicKey};
use crate::old::Kind;
use crate::question::Open;

mod parse;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TagAnswer<'a> {
    Topic(&'a str),
    Dropped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryAnswer<'a> {
    /// The slug path of the entry a followup arose from.
    Entry(&'a str),
    NoEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TouchingAnswer<'a> {
    Topic(&'a str),
    NoTrigger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormerAnswer<'a> {
    Name(&'a str),
    LeftOut,
}

impl<'a> TagAnswer<'a> {
    pub(crate) fn of(text: &'a str) -> TagAnswer<'a> {
        given(text).map_or(TagAnswer::Dropped, TagAnswer::Topic)
    }
}

impl<'a> EntryAnswer<'a> {
    pub(crate) fn of(text: &'a str) -> EntryAnswer<'a> {
        given(text).map_or(EntryAnswer::NoEntry, EntryAnswer::Entry)
    }
}

impl<'a> TouchingAnswer<'a> {
    pub(crate) fn of(text: &'a str) -> TouchingAnswer<'a> {
        given(text).map_or(TouchingAnswer::NoTrigger, TouchingAnswer::Topic)
    }
}

impl<'a> FormerAnswer<'a> {
    pub(crate) fn of(text: &'a str) -> FormerAnswer<'a> {
        given(text).map_or(FormerAnswer::LeftOut, FormerAnswer::Name)
    }
}

// The empty text is the answer that says there is none.
fn given(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

enum Held {
    Present,
    Text(String),
    Topics(Vec<String>),
    Document(DocumentKey),
}

#[derive(Clone, Copy)]
pub(crate) struct Answer<'a> {
    key: &'a Key,
    held: &'a Held,
}

impl<'a> Answer<'a> {
    pub(crate) fn key(&self) -> &'a Key {
        self.key
    }

    pub(crate) fn text(&self) -> Option<&'a str> {
        match self.held {
            Held::Text(text) => Some(text),
            Held::Present | Held::Topics(_) | Held::Document(_) => None,
        }
    }

    pub(crate) fn topics(&self) -> Option<&'a [String]> {
        match self.held {
            Held::Topics(topics) => Some(topics),
            Held::Present | Held::Text(_) | Held::Document(_) => None,
        }
    }

    pub(crate) fn document(&self) -> Option<&'a DocumentKey> {
        match self.held {
            Held::Document(document) => Some(document),
            Held::Present | Held::Text(_) | Held::Topics(_) => None,
        }
    }

    pub(crate) fn placement(&self) -> Option<Placement> {
        match self.key {
            Key::Topic(_, key) => key.placed().map(|(placement, _)| placement),
            _ => None,
        }
    }

    pub(crate) fn refused(&self, why: impl Into<String>) -> Open {
        let (key, why) = (self.key.clone(), why.into());
        Open::Answer { key, why }
    }

    pub(crate) fn one_topic(&self) -> Result<&'a str, Open> {
        match self.topics() {
            Some([one]) => Ok(one),
            _ => Err(self.refused("is not one name, and a fact is filed under one topic")),
        }
    }

    pub(crate) fn one_of(&self, allowed: &[&str]) -> Result<&'a str, Open> {
        let word = self.text().unwrap_or_default();
        if allowed.contains(&word) {
            Ok(word)
        } else {
            Err(self.refused(format!("{word} is not one of: {}", allowed.join(", "))))
        }
    }
}

/// Reading one records its key as used, since no run lists it as open, except for
/// `Unasked::DropsTag`, which records nothing.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Unasked<'k> {
    Conversion(ConversionKey),
    /// The whole list of topics the file files an entry or a followup under, in place of
    /// what its tags give; nothing is read for a document of another kind.
    Topics(&'k DocumentKey),
    Tag(&'k DocumentKey, &'k str),
    /// The document that ended another, read where the file gives a reason.
    EndedBy(&'k DocumentKey),
    /// Every item the table of a topic holds in one list, in order.
    Listed(&'k str, Placement),
    /// The answer for a tag on a document, its own or the one of `[tags]`, where a
    /// `topics` list stands in for the document's tags, which are then asked of nobody.
    DropsTag(&'k DocumentKey, &'k str),
}

/// An answer is reached by the question it answers or as one of `Unasked`.
#[derive(Default)]
pub(crate) struct Decisions {
    held: BTreeMap<Key, Held>,
    tables: BTreeSet<String>,
    unanswered: BTreeSet<Key>,
    placeholders: BTreeSet<Key>,
    used: RefCell<BTreeSet<Key>>,
}

impl Decisions {
    fn read(&self, key: &Key) -> Option<Answer<'_>> {
        let found = self.held.get_key_value(key);
        let mut used = self.used.borrow_mut();
        used.extend(key.of_table());
        if found.is_some() {
            used.insert(key.clone());
        }
        found.map(|(key, held)| Answer { key, held })
    }

    pub(crate) fn answer(&self, open: &Open) -> Option<Answer<'_>> {
        let mut offers = open.offers().into_iter();
        offers.find_map(|offer| self.read(&offer.key))
    }

    pub(crate) fn answers(&self, open: &Open) -> Vec<Answer<'_>> {
        let offers = open.offers();
        let held = offers.iter().filter_map(|offer| self.read(&offer.key));
        held.collect()
    }

    pub(crate) fn unasked(&self, read: Unasked<'_>) -> Vec<Answer<'_>> {
        let one = |key: Key| self.read(&key).into_iter().collect();
        match read {
            Unasked::Conversion(key) => one(Key::Conversion(key)),
            Unasked::Topics(document) => {
                if matches!(document.kind, Kind::Entry | Kind::Followup) {
                    one(Key::document(document, DocumentField::Topics))
                } else {
                    Vec::new()
                }
            }
            Unasked::Tag(document, tag) => {
                let tag = DocumentField::Tag(Folded::new(tag));
                one(Key::document(document, tag))
            }
            Unasked::EndedBy(document) => one(Key::document(document, DocumentField::EndedBy)),
            Unasked::Listed(topic, list) => {
                let topic = Folded::new(topic);
                let listed = self.held.keys().filter(|key| match key {
                    Key::Topic(name, TopicKey::Placed(placement, _)) => {
                        *name == topic && *placement == list
                    }
                    _ => false,
                });
                listed.filter_map(|key| self.read(key)).collect()
            }
            Unasked::DropsTag(document, tag) => {
                let tag = Folded::new(tag);
                let own = Key::document(document, DocumentField::Tag(tag.clone()));
                let found = [own, Key::Tag(tag)]
                    .iter()
                    .find_map(|key| self.held.get_key_value(key));
                let found = found.map(|(key, held)| Answer { key, held });
                found.into_iter().collect()
            }
        }
    }

    /// Listing the names of the `[topics.<name>]` tables records no key as used.
    pub(crate) fn topics(&self) -> impl Iterator<Item = &str> {
        self.held.keys().filter_map(|key| match key {
            Key::Topic(name, TopicKey::Table) => Some(name.as_str()),
            _ => None,
        })
    }

    /// The keys the file holds no answer under: a placeholder, or a list of a topic's table
    /// with no item; a list's key is without its item.
    pub(crate) fn unanswered(&self) -> &BTreeSet<Key> {
        &self.unanswered
    }

    /// A list's key is without its item.
    pub(crate) fn placeholders(&self) -> &BTreeSet<Key> {
        &self.placeholders
    }

    /// The header of every table the file holds, as a stub writes it.
    pub(crate) fn tables(&self) -> &BTreeSet<String> {
        &self.tables
    }

    /// The question for a followup that is filed under no topic after its tags, its
    /// recheck and the file's `topics` list: `Open::Answer` when the file lists none.
    pub(crate) fn unfiled(&self, document: &DocumentKey, summary: &str) -> Open {
        match self.unasked(Unasked::Topics(document)).first() {
            Some(listed) => listed.refused("names no topic, and a followup is filed under one"),
            None => Open::NoTopic {
                document: document.clone(),
                summary: summary.to_owned(),
            },
        }
    }

    /// The answers no question has reached so far, in the order of the file's tables; a
    /// table is listed itself when it holds no answer and nothing was asked under it, and
    /// `[conversion]` never, since the file may hold it before an answer needs it.
    pub(crate) fn unused(&self) -> Vec<Key> {
        let used = self.used.borrow();
        let filled: BTreeSet<Key> = (self.held.keys())
            .filter(|key| !key.is_table())
            .filter_map(Key::of_table)
            .collect();
        let unused = (self.held.keys())
            .filter(|key| !used.contains(key) && !filled.contains(key))
            .filter(|key| !matches!(key, Key::Conversion(_)))
            .cloned();
        unused.collect()
    }
}
