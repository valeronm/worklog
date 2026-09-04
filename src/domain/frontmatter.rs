//! The frontmatter grammar. It is deliberately not YAML: a file's name is
//! the hash of its bytes, so the writer has to be the only shape the reader
//! accepts, and a grammar this small has nothing a second writer could vary.
//!
//! ```text
//! ---
//! key: scalar
//! key: [item, item]
//! key:
//!   sub: scalar
//!   sub: [item]
//! ---
//! body
//! ```
//!
//! Keys are `[A-Za-z_][A-Za-z0-9_-]*`. A scalar is the rest of the line,
//! trimmed, so it holds no newline and no leading or trailing space. A list
//! item holds no comma. A `#` line at the top level is skipped when read and
//! never written. One level of nesting, and the nested values are scalars
//! or lists.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Scalar(String),
    List(Vec<String>),
    Map(Vec<(String, Value)>),
}

/// Ordered fields; order is part of the bytes and so of the hash.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields(Vec<(String, Value)>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    NoFence,
    UnterminatedFence,
    BadLine { line: usize, text: String },
    Nested { line: usize },
    Duplicate(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoFence => f.write_str("no `---` fence opens the file"),
            ParseError::UnterminatedFence => f.write_str("the `---` fence is never closed"),
            ParseError::BadLine { line, text } => write!(f, "line {line}: cannot read `{text}`"),
            ParseError::Nested { line } => write!(f, "line {line}: nested deeper than one level"),
            ParseError::Duplicate(key) => write!(f, "field `{key}` appears twice"),
        }
    }
}

/// A field the kind-specific readers could not accept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldError {
    Missing(&'static str),
    Invalid { key: String, reason: String },
    Unknown(String),
}

impl FieldError {
    #[must_use]
    pub fn invalid(key: &str, reason: impl fmt::Display) -> FieldError {
        FieldError::Invalid {
            key: key.to_owned(),
            reason: reason.to_string(),
        }
    }
}

/// A `YYYY-MM-DD` field value, or the refusal naming the key.
pub fn checked_date(key: &str, value: &str) -> Result<String, FieldError> {
    if crate::domain::slug::is_date(value) {
        Ok(value.to_owned())
    } else {
        Err(FieldError::invalid(
            key,
            format!("`{value}` is not YYYY-MM-DD"),
        ))
    }
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldError::Missing(key) => write!(f, "field `{key}` is missing"),
            FieldError::Invalid { key, reason } => write!(f, "field `{key}`: {reason}"),
            FieldError::Unknown(key) => write!(f, "field `{key}` is not one this kind carries"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Split {
    pub fields: Fields,
    pub body: String,
}

fn key_ok(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn split_key(text: &str) -> Option<(&str, &str)> {
    let (key, rest) = text.split_once(':')?;
    if !key_ok(key) {
        return None;
    }
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some((key, rest.trim()))
}

fn leaf(rest: &str) -> Option<Value> {
    if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        if inner.trim().is_empty() {
            return Some(Value::List(Vec::new()));
        }
        let items: Vec<String> = inner
            .split(',')
            .map(|item| item.trim().to_owned())
            .collect();
        if items.iter().any(String::is_empty) {
            return None;
        }
        return Some(Value::List(items));
    }
    Some(Value::Scalar(rest.to_owned()))
}

/// A fenced document: the fields between the `---` lines and the body after.
pub fn parse(text: &str) -> Result<Split, ParseError> {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(|l| l.trim_end_matches('\n')) != Some("---") {
        return Err(ParseError::NoFence);
    }
    let (fields, consumed) = parse_lines(lines, 2, true)?;
    Ok(Split {
        fields,
        body: text[(consumed + 4).min(text.len())..].to_owned(),
    })
}

/// Fields alone, with no fences and no body: the shape of a config file.
pub fn parse_fields(text: &str) -> Result<Fields, ParseError> {
    parse_lines(text.split_inclusive('\n'), 1, false).map(|(fields, _)| fields)
}

/// Fields in the grammar's shape, without fences.
#[must_use]
pub fn emit_fields(fields: &Fields) -> String {
    let fenced = emit(fields, "");
    fenced[4..fenced.len() - 4].to_owned()
}

/// The fields on the lines, and the bytes read; with `fenced`, reading
/// stops at a closing `---` and its absence is an error.
fn parse_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
    first_number: usize,
    fenced: bool,
) -> Result<(Fields, usize), ParseError> {
    let mut fields = Fields::default();
    let mut consumed = 0;
    let mut closed = false;
    let mut open_map: Option<String> = None;
    for (number, raw) in (first_number..).zip(lines) {
        consumed += raw.len();
        let line = raw.trim_end_matches('\n');
        if fenced && line == "---" {
            closed = true;
            break;
        }
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("  ") {
            if rest.starts_with(' ') {
                return Err(ParseError::Nested { line: number });
            }
            let Some(parent) = &open_map else {
                return Err(ParseError::BadLine {
                    line: number,
                    text: line.to_owned(),
                });
            };
            let bad = || ParseError::BadLine {
                line: number,
                text: line.to_owned(),
            };
            let (key, value) = split_key(rest).ok_or_else(bad)?;
            let value = leaf(value).ok_or_else(bad)?;
            let Some(Value::Map(entries)) = fields.get_mut(parent) else {
                unreachable!("open_map names the map pushed last")
            };
            if entries.iter().any(|(k, _)| k == key) {
                return Err(ParseError::Duplicate(format!("{parent}.{key}")));
            }
            entries.push((key.to_owned(), value));
            continue;
        }
        if line.starts_with(' ') {
            return Err(ParseError::BadLine {
                line: number,
                text: line.to_owned(),
            });
        }
        let bad = || ParseError::BadLine {
            line: number,
            text: line.to_owned(),
        };
        let (key, value) = split_key(line).ok_or_else(bad)?;
        if fields.get(key).is_some() {
            return Err(ParseError::Duplicate(key.to_owned()));
        }
        if value.is_empty() {
            fields.push(key, Value::Map(Vec::new()));
            open_map = Some(key.to_owned());
        } else {
            fields.push(key, leaf(value).ok_or_else(bad)?);
            open_map = None;
        }
    }
    if fenced && !closed {
        return Err(ParseError::UnterminatedFence);
    }
    // A key with nothing under it is an empty scalar, not a map.
    for (_, value) in &mut fields.0 {
        if matches!(value, Value::Map(entries) if entries.is_empty()) {
            *value = Value::Scalar(String::new());
        }
    }
    Ok((fields, consumed))
}

fn emit_leaf(out: &mut String, key: &str, value: &Value) {
    match value {
        Value::Scalar(s) if s.is_empty() => {
            out.push_str(key);
            out.push_str(":\n");
        }
        Value::Scalar(s) => {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(s);
            out.push('\n');
        }
        Value::List(items) => {
            out.push_str(key);
            out.push_str(": [");
            out.push_str(&items.join(", "));
            out.push_str("]\n");
        }
        Value::Map(_) => unreachable!("a map is emitted by the caller"),
    }
}

#[must_use]
pub fn emit(fields: &Fields, body: &str) -> String {
    let mut out = String::from("---\n");
    for (key, value) in &fields.0 {
        match value {
            Value::Map(entries) => {
                out.push_str(key);
                out.push_str(":\n");
                for (sub, value) in entries {
                    out.push_str("  ");
                    emit_leaf(&mut out, sub, value);
                }
            }
            leaf => emit_leaf(&mut out, key, leaf),
        }
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

impl Fields {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    #[must_use]
    pub fn scalar(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Scalar(s)) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn list(&self, key: &str) -> Option<&[String]> {
        match self.get(key) {
            Some(Value::List(items)) => Some(items),
            _ => None,
        }
    }

    pub fn push(&mut self, key: &str, value: Value) {
        self.0.push((key.to_owned(), value));
    }

    pub fn push_scalar(&mut self, key: &str, value: &str) {
        self.push(key, Value::Scalar(value.trim().to_owned()));
    }

    pub fn push_list(&mut self, key: &str, items: &[String]) {
        self.push(
            key,
            Value::List(items.iter().map(|s| s.trim().to_owned()).collect()),
        );
    }

    /// Replaces the value under `key`, or appends the field.
    pub fn set(&mut self, key: &str, value: Value) {
        match self.get_mut(key) {
            Some(slot) => *slot = value,
            None => self.push(key, value),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let index = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(index).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn required(&self, key: &'static str) -> Result<&str, FieldError> {
        match self.scalar(key) {
            Some(value) if !value.is_empty() => Ok(value),
            _ => Err(FieldError::Missing(key)),
        }
    }

    #[must_use]
    pub fn optional(&self, key: &str) -> Option<&str> {
        self.scalar(key).filter(|value| !value.is_empty())
    }

    #[must_use]
    pub fn list_or_empty(&self, key: &str) -> Vec<String> {
        self.list(key).map(<[String]>::to_vec).unwrap_or_default()
    }

    pub fn reject_unknown(&self, known: &[&str]) -> Result<(), FieldError> {
        match self.0.iter().find(|(k, _)| !known.contains(&k.as_str())) {
            Some((key, _)) => Err(FieldError::Unknown(key.clone())),
            None => Ok(()),
        }
    }
}

impl FromIterator<(String, Value)> for Fields {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        Fields(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Fields {
        let mut f = Fields::default();
        f.push_scalar("slug", "lantern/x");
        f.push_list("tags", &["a".into(), "b".into()]);
        f.push_scalar("summary", "");
        f.push(
            "version",
            Value::Map(vec![
                ("parents".into(), Value::List(vec!["abc".into()])),
                ("machine".into(), Value::Scalar("m".into())),
            ]),
        );
        f
    }

    #[test]
    fn round_trips() {
        let text = emit(&fields(), "\n## What\nbody\n");
        let split = parse(&text).unwrap();
        assert_eq!(split.fields, fields());
        assert_eq!(split.body, "\n## What\nbody\n");
        assert_eq!(emit(&split.fields, &split.body), text);
    }

    #[test]
    fn emitted_shape() {
        assert_eq!(
            emit(&fields(), ""),
            "---\nslug: lantern/x\ntags: [a, b]\nsummary:\nversion:\n  parents: [abc]\n  machine: m\n---\n"
        );
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let split = parse("---\n# note\n\nkey: v\n---\n").unwrap();
        assert_eq!(split.fields.scalar("key"), Some("v"));
    }

    #[test]
    fn empty_list_and_empty_scalar() {
        let split = parse("---\ntags: []\nsummary:\n---\n").unwrap();
        assert_eq!(split.fields.list("tags"), Some(&[][..]));
        assert_eq!(split.fields.scalar("summary"), Some(""));
    }

    #[test]
    fn errors() {
        assert_eq!(parse("key: v\n"), Err(ParseError::NoFence));
        assert_eq!(parse("---\nkey: v\n"), Err(ParseError::UnterminatedFence));
        assert_eq!(
            parse("---\nkey: v\nkey: w\n---\n"),
            Err(ParseError::Duplicate("key".into()))
        );
        assert_eq!(
            parse("---\na:\n  b:\n    c: 1\n---\n"),
            Err(ParseError::Nested { line: 4 })
        );
        assert!(matches!(
            parse("---\n  b: 1\n---\n"),
            Err(ParseError::BadLine { line: 2, .. })
        ));
        assert!(matches!(
            parse("---\nbad key: 1\n---\n"),
            Err(ParseError::BadLine { .. })
        ));
        assert!(matches!(
            parse("---\nk: [a,,b]\n---\n"),
            Err(ParseError::BadLine { .. })
        ));
    }

    #[test]
    fn unfenced_fields() {
        let text = emit_fields(&fields());
        assert!(!text.starts_with("---"));
        assert_eq!(parse_fields(&text).unwrap(), fields());
        assert_eq!(parse_fields("").unwrap(), Fields::default());
        assert!(matches!(
            parse_fields("machine: m\n  odd: 1\n"),
            Err(ParseError::BadLine { line: 2, .. })
        ));
    }

    #[test]
    fn a_scalar_keeps_its_colons() {
        let split = parse("---\nsummary: a: b — c\n---\n").unwrap();
        assert_eq!(split.fields.scalar("summary"), Some("a: b — c"));
    }
}
