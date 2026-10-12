use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use toml::{Table, Value};

use super::{Decisions, Held};
use crate::key::{
    ConversionKey, DocumentField, DocumentKey, Folded, Key, PLACEHOLDER, Placement, TopicKey,
    quoted, segment,
};
use crate::old::Kind;
use crate::text::first_line;

/// `key` is the path of the key in the file, empty when the file is not TOML at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) key: String,
    pub(crate) reason: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            f.write_str(&self.reason)
        } else {
            write!(f, "{}: {}", self.key, self.reason)
        }
    }
}

impl Decisions {
    pub(crate) fn parse(text: &str) -> Result<Decisions, Vec<Refused>> {
        let file: Table = text.parse().map_err(|error| vec![not_toml(text, &error)])?;
        let mut reading = Reading::default();
        for (name, value) in &file {
            let path = segment(name);
            match name.as_str() {
                "conversion" => reading.conversion(&path, value),
                "tags" => reading.tags(&path, value, None),
                "topics" => reading.topics(&path, value),
                "machines" => reading.machines(&path, value),
                "documents" => reading.documents(&path, value),
                _ => reading.refuse(&path, "is not a table of a decision file"),
            }
        }
        if reading.refused.is_empty() {
            let held = reading.held;
            let used = std::cell::RefCell::default();
            let named = ["conversion", "tags", "machines"];
            let mut tables: BTreeSet<String> = (held.keys().filter(|key| key.is_table()))
                .map(Key::table)
                .collect();
            tables.extend((file.keys().filter(|name| named.contains(&name.as_str()))).cloned());
            Ok(Decisions {
                held,
                tables,
                unanswered: reading.unanswered,
                placeholders: reading.placeholders,
                used,
            })
        } else {
            Err(reading.refused)
        }
    }
}

// The parser's message for a table or a key written twice names neither.
fn not_toml(text: &str, error: &toml::de::Error) -> Refused {
    let span = error.span().unwrap_or(0..0);
    let line = text.get(..span.start).unwrap_or(text).matches('\n').count() + 1;
    let at = text.get(span).map_or("", first_line);
    let reason = if at.trim().is_empty() {
        format!("line {line}: {}", error.message())
    } else {
        format!("line {line}: {} at {at}", error.message())
    };
    Refused {
        key: String::new(),
        reason,
    }
}

fn under(path: &str, key: &str) -> String {
    format!("{path}.{}", segment(key))
}

fn no_kind() -> String {
    let [first @ .., last] = Kind::ALL.map(|kind| format!("{kind}/"));
    format!(
        "does not open with {} or {last} before a slug",
        first.join(", ")
    )
}

#[derive(Default)]
struct Reading {
    held: BTreeMap<Key, Held>,
    unanswered: BTreeSet<Key>,
    placeholders: BTreeSet<Key>,
    // Whether the value read last holds a placeholder.
    placeholder: bool,
    refused: Vec<Refused>,
}

impl Reading {
    fn refuse(&mut self, key: &str, reason: impl Into<String>) {
        self.refused.push(Refused {
            key: key.to_owned(),
            reason: reason.into(),
        });
    }

    fn table<'a>(&mut self, path: &str, value: &'a Value) -> Option<&'a Table> {
        let table = value.as_table();
        if table.is_none() {
            self.refuse(path, "is not a table");
        }
        table
    }

    // A placeholder answers nothing, whatever else was read under the key.
    fn hold(&mut self, key: Key, read: Option<Held>) {
        if std::mem::take(&mut self.placeholder) {
            self.placeholders.insert(key.clone());
            self.unanswered.insert(key);
        } else if let Some(held) = read {
            self.held.insert(key, held);
        }
    }

    // A list of a topic's table with no item answers nothing, and no answer is owed for
    // it as one is for a placeholder.
    fn hold_list(&mut self, line: Key, items: &[String]) {
        let empty = items.is_empty() && !self.placeholder;
        self.hold(line.clone(), None);
        if empty {
            self.unanswered.insert(line);
        }
    }

    fn text<'a>(&mut self, path: &str, value: &'a Value) -> Option<&'a str> {
        let Some(text) = value.as_str() else {
            self.placeholder = false;
            self.refuse(path, "is not text");
            return None;
        };
        self.placeholder = text == PLACEHOLDER;
        (!self.placeholder).then_some(text)
    }

    fn line(&mut self, path: &str, value: &Value) -> Option<String> {
        let text = self.text(path, value)?;
        if text.trim().is_empty() {
            self.refuse(path, "is empty");
        } else if text.contains(['\n', '\r']) {
            self.refuse(path, "is not one line");
        } else {
            return Some(text.to_owned());
        }
        None
    }

    fn list(&mut self, path: &str, value: &Value) -> Option<Vec<String>> {
        let items = value.as_array().and_then(|items| {
            let texts = items.iter().map(|item| item.as_str().map(str::to_owned));
            texts.collect::<Option<Vec<String>>>()
        });
        self.placeholder = false;
        let Some(mut items) = items else {
            self.refuse(path, "is not a list of text");
            return None;
        };
        self.placeholder = items.iter().any(|item| item == PLACEHOLDER);
        items.retain(|item| item != PLACEHOLDER);
        let before = self.refused.len();
        for (at, item) in items.iter().enumerate() {
            let folded = Folded::new(item);
            if item.trim().is_empty() {
                self.refuse(path, "holds an empty item");
            } else if items[..at].iter().any(|other| Folded::new(other) == folded) {
                self.refuse(path, format!("holds {} twice", quoted(item)));
            }
        }
        (self.refused.len() == before).then_some(items)
    }

    fn conversion(&mut self, path: &str, value: &Value) {
        let Some(table) = self.table(path, value) else {
            return;
        };
        for (key, value) in table {
            let path = under(path, key);
            let (key, read) = match key.as_str() {
                "written" => (ConversionKey::Written, self.when(&path, value)),
                "machine" => (ConversionKey::Machine, self.line(&path, value)),
                _ => {
                    self.refuse(&path, "is not a key of the conversion's table");
                    continue;
                }
            };
            self.hold(Key::Conversion(key), read.map(Held::Text));
        }
        for required in ["written", "machine"] {
            if !table.contains_key(required) {
                self.refuse(&under(path, required), "is missing");
            }
        }
    }

    fn when(&mut self, path: &str, value: &Value) -> Option<String> {
        self.placeholder = matches!(value, Value::String(text) if text == PLACEHOLDER);
        match value {
            Value::String(_) if self.placeholder => {}
            Value::String(_) => {
                self.refuse(path, "is text: write the date and time without quotes");
            }
            Value::Datetime(when)
                if when.date.is_some()
                    && when.time.is_some_and(|time| time.second.is_some())
                    && when.offset.is_some() =>
            {
                return Some(when.to_string());
            }
            _ => self.refuse(path, "is not a date and time with seconds and an offset"),
        }
        None
    }

    fn tags(&mut self, path: &str, value: &Value, document: Option<&DocumentKey>) {
        for (tag, value) in self.table(path, value).into_iter().flatten() {
            let path = under(path, tag);
            let folded = Folded::new(tag);
            if folded.as_str() != tag {
                let reason = "holds an uppercase letter, and a tag is keyed in lowercase";
                self.refuse(&path, reason);
            }
            let answer = self.blank_or_line(&path, value);
            let key = match document {
                Some(document) => Key::document(document, DocumentField::Tag(folded)),
                None => Key::Tag(folded),
            };
            self.hold(key, answer);
        }
    }

    fn topics(&mut self, path: &str, value: &Value) {
        let mut tables: BTreeMap<Folded, String> = BTreeMap::new();
        for (name, value) in self.table(path, value).into_iter().flatten() {
            let path = under(path, name);
            let folded = Folded::new(name);
            if let Some(other) = tables.get(&folded) {
                let reason = format!("is the topic of {other} in another case");
                self.refuse(&path, reason);
                continue;
            }
            tables.insert(folded.clone(), path.clone());
            self.topic(&path, &folded, value);
        }
    }

    fn topic(&mut self, path: &str, name: &Folded, value: &Value) {
        let Some(table) = self.table(path, value) else {
            return;
        };
        let key = |key| Key::Topic(name.clone(), key);
        self.held.insert(key(TopicKey::Table), Held::Present);
        let mut placed: BTreeMap<Folded, Placement> = BTreeMap::new();
        for (word, value) in table {
            let path = under(path, word);
            let list = Placement::ALL.into_iter().find(|list| list.word() == word);
            match (word.as_str(), list) {
                ("summary", _) => {
                    let summary = self.line(&path, value);
                    self.hold(key(TopicKey::Summary), summary.map(Held::Text));
                }
                (_, Some(list)) => {
                    let items = self.list(&path, value).unwrap_or_default();
                    self.hold_list(key(TopicKey::Placed(list, Folded::new(""))), &items);
                    for item in items {
                        let item = Folded::new(&item);
                        let other = placed.get(&item).filter(|_| list != Placement::Drop);
                        if let Some(other) = other {
                            let held = quoted(item.as_str());
                            let reason = format!("holds {held}, which {} holds too", other.word());
                            self.refuse(&path, reason);
                            continue;
                        }
                        if list != Placement::Drop {
                            placed.insert(item.clone(), list);
                        }
                        self.held
                            .insert(key(TopicKey::Placed(list, item)), Held::Present);
                    }
                }
                (TopicKey::UNCLAIM, _) => {
                    let items = self.list(&path, value).unwrap_or_default();
                    self.hold_list(key(TopicKey::Unclaim(Folded::new(""))), &items);
                    for item in items {
                        let item = TopicKey::Unclaim(Folded::new(&item));
                        self.held.insert(key(item), Held::Present);
                    }
                }
                _ => self.refuse(&path, "is not a key of a topic's table"),
            }
        }
    }

    fn machines(&mut self, path: &str, value: &Value) {
        for (name, value) in self.table(path, value).into_iter().flatten() {
            let topic = self.line(&under(path, name), value);
            self.hold(Key::Machine(name.clone()), topic.map(Held::Text));
        }
    }

    fn document_key(&mut self, path: &str, text: &str) -> Option<DocumentKey> {
        let key = DocumentKey::parse(text);
        if key.is_none() {
            self.refuse(path, no_kind());
        }
        key
    }

    fn documents(&mut self, path: &str, value: &Value) {
        for (key, value) in self.table(path, value).into_iter().flatten() {
            let path = under(path, key);
            let Some(document) = self.document_key(&path, key) else {
                continue;
            };
            let Some(table) = self.table(&path, value) else {
                continue;
            };
            let table_key = Key::document(&document, DocumentField::Table);
            self.held.insert(table_key, Held::Present);
            for (word, value) in table {
                self.field(&under(&path, word), &document, word, value);
            }
        }
    }

    fn field(&mut self, path: &str, document: &DocumentKey, word: &str, value: &Value) {
        let (field, held) = match word {
            "tags" => return self.tags(path, value, Some(document)),
            "topics" => (
                DocumentField::Topics,
                self.list(path, value).map(Held::Topics),
            ),
            "name" => (DocumentField::Name, self.line(path, value).map(Held::Text)),
            "former_names" => return self.former_names(path, value, document),
            "summary" => (
                DocumentField::Summary,
                self.line(path, value).map(Held::Text),
            ),
            "recheck_summary" => {
                let summary = self.line(path, value).map(Held::Text);
                (DocumentField::RecheckSummary, summary)
            }
            "entry" => (DocumentField::Entry, self.blank_or_line(path, value)),
            "touching" => (DocumentField::Touching, self.blank_or_line(path, value)),
            "ended" => (DocumentField::Ended, self.line(path, value).map(Held::Text)),
            "ended_by" => {
                let text = self.line(path, value);
                let by = text.and_then(|text| self.document_key(path, &text));
                (DocumentField::EndedBy, by.map(Held::Document))
            }
            "machine" => (
                DocumentField::Machine,
                self.line(path, value).map(Held::Text),
            ),
            _ => return self.refuse(path, "is not a key of a document's table"),
        };
        self.hold(Key::document(document, field), held);
    }

    fn former_names(&mut self, path: &str, value: &Value, document: &DocumentKey) {
        for (name, value) in self.table(path, value).into_iter().flatten() {
            let answer = self.blank_or_line(&under(path, name), value);
            let field = DocumentField::FormerName(name.clone());
            self.hold(Key::document(document, field), answer);
        }
    }

    fn blank_or_line(&mut self, path: &str, value: &Value) -> Option<Held> {
        let text = self.text(path, value)?;
        if text.is_empty() {
            Some(Held::Text(String::new()))
        } else {
            self.line(path, value).map(Held::Text)
        }
    }
}
