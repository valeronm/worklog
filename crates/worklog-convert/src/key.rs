//! Where an answer lives in a decision file: the path of each key as a stub and a report
//! print it. The reader of the file spells the same words in its own match.

use std::fmt::{self, Write};

use crate::old::Kind;

// A stub holds it where an answer goes, and it answers nothing wherever a file holds it.
pub(crate) const PLACEHOLDER: &str = "?";

/// A tag or a topic name as the store being converted compares it, without regard to ASCII
/// case.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Folded(String);

impl Folded {
    pub(crate) fn new(text: &str) -> Folded {
        Folded(text.to_ascii_lowercase())
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Folded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A document of the store being converted, by kind and slug since two kinds can share a
/// slug.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DocumentKey {
    pub(crate) kind: Kind,
    pub(crate) slug: String,
}

impl DocumentKey {
    pub(crate) fn new(kind: Kind, slug: &str) -> DocumentKey {
        let slug = slug.to_owned();
        DocumentKey { kind, slug }
    }

    pub(crate) fn parse(text: &str) -> Option<DocumentKey> {
        let (word, slug) = text.split_once('/')?;
        let kind = Kind::ALL.into_iter().find(|kind| kind.word() == word)?;
        (!slug.is_empty()).then(|| DocumentKey::new(kind, slug))
    }
}

impl fmt::Display for DocumentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.kind, self.slug)
    }
}

/// Prints as the path of its key in the file and orders as the file's tables do.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Key {
    Conversion(ConversionKey),
    Tag(Folded),
    Topic(Folded, TopicKey),
    Machine(String),
    Document(DocumentKey, DocumentField),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ConversionKey {
    Written,
    Machine,
}

/// A list's key stands for one item of the list, the topic it names.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TopicKey {
    Table,
    Summary,
    Placed(Placement, Folded),
    /// A topic whose claims a machine topic's table ends or leaves behind.
    Unclaim(Folded),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DocumentField {
    Table,
    Tag(Folded),
    Topics,
    Name,
    /// A name the document was stored under before its last rename, as stored.
    FormerName(String),
    Summary,
    RecheckSummary,
    Entry,
    Touching,
    Ended,
    EndedBy,
    Machine,
}

/// The list of a topic's table an item of the topic's `includes` goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Placement {
    PartOf,
    Uses,
    Drop,
}

impl Placement {
    pub(crate) const ALL: [Placement; 3] = [Placement::PartOf, Placement::Uses, Placement::Drop];

    pub(crate) fn word(self) -> &'static str {
        match self {
            Placement::PartOf => "part_of",
            Placement::Uses => "uses",
            Placement::Drop => "drop",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edges<T> {
    pub(crate) part_of: Vec<T>,
    pub(crate) uses: Vec<T>,
}

impl<T> Default for Edges<T> {
    fn default() -> Edges<T> {
        Edges {
            part_of: Vec::new(),
            uses: Vec::new(),
        }
    }
}

impl<T> Edges<T> {
    pub(crate) fn of(&mut self, placement: Placement) -> Option<&mut Vec<T>> {
        match placement {
            Placement::PartOf => Some(&mut self.part_of),
            Placement::Uses => Some(&mut self.uses),
            Placement::Drop => None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.part_of.is_empty() && self.uses.is_empty()
    }

    pub(crate) fn map<U>(&self, to: impl Fn(&T) -> U) -> Edges<U> {
        Edges {
            part_of: self.part_of.iter().map(&to).collect(),
            uses: self.uses.iter().map(&to).collect(),
        }
    }
}

impl TopicKey {
    pub(crate) fn placed(&self) -> Option<(Placement, &Folded)> {
        match self {
            TopicKey::Table | TopicKey::Summary | TopicKey::Unclaim(_) => None,
            TopicKey::Placed(placement, item) => Some((*placement, item)),
        }
    }
}

impl TopicKey {
    pub(crate) const UNCLAIM: &'static str = "unclaim";

    pub(crate) fn item(&self) -> Option<&Folded> {
        match self {
            TopicKey::Table | TopicKey::Summary => None,
            TopicKey::Placed(_, item) | TopicKey::Unclaim(item) => Some(item),
        }
    }

    pub(crate) fn line(&self) -> TopicKey {
        let none = Folded::new("");
        match self {
            TopicKey::Table => TopicKey::Table,
            TopicKey::Summary => TopicKey::Summary,
            TopicKey::Placed(placement, _) => TopicKey::Placed(*placement, none),
            TopicKey::Unclaim(_) => TopicKey::Unclaim(none),
        }
    }
}

impl Key {
    pub(crate) fn unclaim(machine_topic: &str, item: &str) -> Key {
        Key::topic(machine_topic, TopicKey::Unclaim(Folded::new(item)))
    }
    #[cfg(test)]
    pub(crate) fn tag(tag: &str) -> Key {
        Key::Tag(Folded::new(tag))
    }
    pub(crate) fn topic(name: &str, key: TopicKey) -> Key {
        Key::Topic(Folded::new(name), key)
    }
    pub(crate) fn placed(topic: &str, placement: Placement, item: &str) -> Key {
        Key::topic(topic, TopicKey::Placed(placement, Folded::new(item)))
    }
    pub(crate) fn document(document: &DocumentKey, field: DocumentField) -> Key {
        Key::Document(document.clone(), field)
    }

    pub(crate) fn table(&self) -> String {
        match self {
            Key::Conversion(_) => "conversion".to_owned(),
            Key::Tag(_) => "tags".to_owned(),
            Key::Topic(name, _) => format!("topics.{}", segment(name.as_str())),
            Key::Machine(_) => "machines".to_owned(),
            Key::Document(document, _) => {
                format!("documents.{}", segment(&document.to_string()))
            }
        }
    }

    /// The key as written under its table, a list's without its item; `None` for a table
    /// itself.
    pub(crate) fn line(&self) -> Option<String> {
        let line = match self {
            Key::Conversion(ConversionKey::Written) => "written".to_owned(),
            Key::Conversion(ConversionKey::Machine) => "machine".to_owned(),
            Key::Tag(tag) => segment(tag.as_str()),
            Key::Machine(name) => segment(name),
            Key::Topic(_, TopicKey::Table) | Key::Document(_, DocumentField::Table) => {
                return None;
            }
            Key::Topic(_, TopicKey::Summary) => "summary".to_owned(),
            Key::Topic(_, TopicKey::Placed(placement, _)) => placement.word().to_owned(),
            Key::Topic(_, TopicKey::Unclaim(_)) => TopicKey::UNCLAIM.to_owned(),
            Key::Document(_, field) => match field {
                DocumentField::Table => return None,
                DocumentField::Tag(tag) => format!("tags.{}", segment(tag.as_str())),
                DocumentField::Topics => "topics".to_owned(),
                DocumentField::Name => "name".to_owned(),
                DocumentField::FormerName(name) => format!("former_names.{}", segment(name)),
                DocumentField::Summary => "summary".to_owned(),
                DocumentField::RecheckSummary => "recheck_summary".to_owned(),
                DocumentField::Entry => "entry".to_owned(),
                DocumentField::Touching => "touching".to_owned(),
                DocumentField::Ended => "ended".to_owned(),
                DocumentField::EndedBy => "ended_by".to_owned(),
                DocumentField::Machine => "machine".to_owned(),
            },
        };
        Some(line)
    }

    pub(crate) fn item(&self) -> Option<&Folded> {
        match self {
            Key::Topic(_, key) => key.item(),
            _ => None,
        }
    }

    /// The key of the line the answer is written on: a list's key without its item.
    pub(crate) fn of_line(self) -> Key {
        match self {
            Key::Topic(name, list) => Key::Topic(name, list.line()),
            key => key,
        }
    }

    pub(crate) fn of_table(&self) -> Option<Key> {
        match self {
            Key::Topic(name, _) => Some(Key::Topic(name.clone(), TopicKey::Table)),
            Key::Document(document, _) => Some(Key::document(document, DocumentField::Table)),
            _ => None,
        }
    }

    pub(crate) fn is_table(&self) -> bool {
        self.line().is_none()
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.table())?;
        if let Some(line) = self.line() {
            write!(f, ".{line}")?;
        }
        match self.item() {
            Some(item) => write!(f, "[{}]", quoted(item.as_str())),
            None => Ok(()),
        }
    }
}

pub(crate) fn segment(key: &str) -> String {
    let bare = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_';
    if !key.is_empty() && key.bytes().all(bare) {
        key.to_owned()
    } else {
        quoted(key)
    }
}

pub(crate) fn quoted(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(quoted, "\\u{:04X}", u32::from(c));
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}
