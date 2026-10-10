//! Typed values out of a table of fields, each refusal naming its key.

use std::fmt;

use toml::Value;
use toml::value::Datetime;

use super::error::SchemaError;
use crate::domain::id::DocumentId;
use crate::domain::version::Fields;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date(toml::value::Date);

impl Date {
    /// Takes `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Result<Date, SchemaError> {
        text.parse()
            .ok()
            .and_then(|datetime| Date::of(&datetime))
            .ok_or_else(|| SchemaError::NotADate(text.to_owned()))
    }

    fn of(datetime: &Datetime) -> Option<Date> {
        let alone = datetime.time.is_none() && datetime.offset.is_none();
        datetime.date.filter(|_| alone).map(Date)
    }

    pub(super) fn value(self) -> Value {
        Value::Datetime(self.0.into())
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

pub(super) fn dated(text: &str) -> Option<(Date, &str)> {
    let date = Date::parse(text.get(..10)?).ok()?;
    Some((date, &text[10..]))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(String);

impl Name {
    pub fn parse(text: &str) -> Result<Name, SchemaError> {
        let shaped = text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && text.bytes().any(|b| b.is_ascii_lowercase())
            && !text.starts_with('-')
            && !text.ends_with('-');
        if shaped && dated(text).is_none() && DocumentId::parse(text).is_err() {
            Ok(Name(text.to_owned()))
        } else {
            Err(SchemaError::NotAName(text.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub(super) struct Reader {
    left: Fields,
}

impl Reader {
    pub(super) fn new(fields: &Fields) -> Reader {
        Reader {
            left: fields.clone(),
        }
    }

    fn required<T>(key: &str, value: Option<T>) -> Result<T, SchemaError> {
        value.ok_or_else(|| SchemaError::field(key, "is missing"))
    }

    pub(super) fn optional_text(&mut self, key: &str) -> Result<Option<String>, SchemaError> {
        match self.left.remove(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(SchemaError::field(key, "is not text")),
        }
    }

    pub(super) fn text(&mut self, key: &str) -> Result<String, SchemaError> {
        let text = self.optional_text(key)?;
        Reader::required(key, text)
    }

    pub(super) fn optional_line(&mut self, key: &str) -> Result<Option<String>, SchemaError> {
        match self.optional_text(key)? {
            Some(text) if text.contains('\n') => Err(SchemaError::field(key, "holds a line break")),
            text => Ok(text),
        }
    }

    pub(super) fn line(&mut self, key: &str) -> Result<String, SchemaError> {
        let line = self.optional_line(key)?;
        match Reader::required(key, line)? {
            line if line.trim().is_empty() => Err(SchemaError::field(key, "is empty")),
            line => Ok(line),
        }
    }

    pub(super) fn optional_date(&mut self, key: &str) -> Result<Option<Date>, SchemaError> {
        match self.left.remove(key) {
            None => Ok(None),
            Some(Value::Datetime(datetime)) => Date::of(&datetime)
                .map(Some)
                .ok_or_else(|| SchemaError::field(key, "is not a date alone")),
            Some(Value::String(_)) => Err(SchemaError::field(
                key,
                "is text; write the date without quotes",
            )),
            Some(_) => Err(SchemaError::field(key, "is not a date")),
        }
    }

    pub(super) fn date(&mut self, key: &str) -> Result<Date, SchemaError> {
        let date = self.optional_date(key)?;
        Reader::required(key, date)
    }

    /// False when the key is absent.
    pub(super) fn flag(&mut self, key: &str) -> Result<bool, SchemaError> {
        match self.left.remove(key) {
            None => Ok(false),
            Some(Value::Boolean(flag)) => Ok(flag),
            Some(_) => Err(SchemaError::field(key, "is not true or false")),
        }
    }

    pub(super) fn optional_id(&mut self, key: &str) -> Result<Option<DocumentId>, SchemaError> {
        self.optional_text(key)?
            .map(|text| DocumentId::parse(&text).map_err(|e| SchemaError::field(key, e)))
            .transpose()
    }

    pub(super) fn id(&mut self, key: &str) -> Result<DocumentId, SchemaError> {
        let id = self.optional_id(key)?;
        Reader::required(key, id)
    }

    /// Empty when the key is absent.
    fn list<T>(
        &mut self,
        key: &str,
        item: impl Fn(&str) -> Result<T, SchemaError>,
    ) -> Result<Vec<T>, SchemaError> {
        match self.left.remove(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| SchemaError::field(key, "holds what is not text"))
                        .and_then(&item)
                })
                .collect(),
            Some(_) => Err(SchemaError::field(key, "is not a list")),
        }
    }

    pub(super) fn ids(&mut self, key: &str) -> Result<Vec<DocumentId>, SchemaError> {
        self.list(key, |text| {
            DocumentId::parse(text).map_err(|e| SchemaError::field(key, e))
        })
    }

    pub(super) fn name(&mut self, key: &str) -> Result<Name, SchemaError> {
        Name::parse(&self.text(key)?)
    }

    pub(super) fn names(&mut self, key: &str) -> Result<Vec<Name>, SchemaError> {
        self.list(key, Name::parse)
    }

    pub(super) fn finish(self) -> Result<(), SchemaError> {
        match self.left.keys().next() {
            Some(key) => Err(SchemaError::UnknownKey(key.clone())),
            None => Ok(()),
        }
    }
}

/// An empty list, an absent value and a false flag write no key.
#[derive(Default)]
pub(super) struct Writer(Fields);

impl Writer {
    pub(super) fn text(mut self, key: &str, text: &str) -> Writer {
        self.0.insert(key.to_owned(), text.into());
        self
    }

    pub(super) fn optional_text(self, key: &str, text: Option<&str>) -> Writer {
        match text {
            Some(text) => self.text(key, text),
            None => self,
        }
    }

    pub(super) fn date(mut self, key: &str, date: Date) -> Writer {
        self.0.insert(key.to_owned(), date.value());
        self
    }

    pub(super) fn flag(mut self, key: &str, flag: bool) -> Writer {
        if flag {
            self.0.insert(key.to_owned(), flag.into());
        }
        self
    }

    pub(super) fn id(self, key: &str, id: &DocumentId) -> Writer {
        self.text(key, id.as_str())
    }

    pub(super) fn optional_id(self, key: &str, id: Option<&DocumentId>) -> Writer {
        self.optional_text(key, id.map(DocumentId::as_str))
    }

    fn list<'a>(mut self, key: &str, items: impl Iterator<Item = &'a str>) -> Writer {
        let items: Vec<Value> = items.map(Value::from).collect();
        if !items.is_empty() {
            self.0.insert(key.to_owned(), Value::Array(items));
        }
        self
    }

    pub(super) fn ids(self, key: &str, ids: &[DocumentId]) -> Writer {
        self.list(key, ids.iter().map(DocumentId::as_str))
    }

    pub(super) fn names(self, key: &str, names: &[Name]) -> Writer {
        self.list(key, names.iter().map(Name::as_str))
    }

    pub(super) fn done(self) -> Fields {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::LANTERN;

    fn reader(toml: &str) -> Reader {
        Reader::new(&toml.parse().unwrap())
    }

    fn refused<T: fmt::Debug>(result: Result<T, SchemaError>, key: &str, why: &str) {
        match result {
            Err(SchemaError::Field {
                key: named,
                why: said,
            }) => {
                assert_eq!(named, key);
                assert!(said.contains(why), "{said}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn each_value_is_read_as_its_type() {
        let mut reader = reader(&format!(
            "summary = \"The relay\"\nconfirmed = 2026-10-09\nidea = true\ntopic = \"{LANTERN}\"\n\
             topics = [\"{LANTERN}\"]\nformer_names = [\"relay\"]\nname = \"relay-pin\"\n"
        ));
        assert_eq!(reader.line("summary").unwrap(), "The relay");
        assert_eq!(reader.date("confirmed").unwrap().to_string(), "2026-10-09");
        assert!(reader.flag("idea").unwrap());
        assert_eq!(reader.id("topic").unwrap().as_str(), LANTERN);
        assert_eq!(reader.ids("topics").unwrap().len(), 1);
        assert_eq!(reader.names("former_names").unwrap()[0].as_str(), "relay");
        assert_eq!(reader.name("name").unwrap().as_str(), "relay-pin");
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn what_is_absent_is_missing_or_empty_by_what_was_asked() {
        let mut reader = reader("");
        refused(reader.text("summary"), "summary", "missing");
        refused(reader.date("confirmed"), "confirmed", "missing");
        refused(reader.id("topic"), "topic", "missing");
        assert_eq!(reader.optional_text("note"), Ok(None));
        assert_eq!(reader.optional_date("look_again"), Ok(None));
        assert_eq!(reader.optional_id("entry"), Ok(None));
        assert_eq!(reader.flag("idea"), Ok(false));
        assert_eq!(reader.ids("topics"), Ok(vec![]));
        assert_eq!(reader.names("former_names"), Ok(vec![]));
    }

    #[test]
    fn a_value_of_another_type_is_refused_by_its_key() {
        refused(reader("summary = 1").text("summary"), "summary", "not text");
        refused(
            reader("summary = \"a\\nb\"").line("summary"),
            "summary",
            "line break",
        );
        for blank in ["summary = \"\"", "summary = \"  \""] {
            refused(reader(blank).line("summary"), "summary", "empty");
        }
        refused(
            reader("confirmed = \"2026-10-09\"").date("confirmed"),
            "confirmed",
            "without quotes",
        );
        refused(
            reader("confirmed = 2026-10-09T10:00:00").date("confirmed"),
            "confirmed",
            "date alone",
        );
        refused(
            reader("confirmed = 1").date("confirmed"),
            "confirmed",
            "not a date",
        );
        refused(
            reader("idea = \"yes\"").flag("idea"),
            "idea",
            "true or false",
        );
        refused(
            reader("topic = \"lantern\"").id("topic"),
            "topic",
            "document id",
        );
        refused(
            reader("topics = \"x\"").ids("topics"),
            "topics",
            "not a list",
        );
        refused(reader("topics = [1]").ids("topics"), "topics", "not text");
        refused(
            reader("topics = [\"x\"]").ids("topics"),
            "topics",
            "document id",
        );
    }

    #[test]
    fn a_key_nobody_took_is_refused_by_name() {
        let mut reader = reader("summary = \"s\"\nsurprise = 1\n");
        reader.text("summary").unwrap();
        assert_eq!(
            reader.finish(),
            Err(SchemaError::UnknownKey("surprise".to_owned()))
        );
    }

    #[test]
    fn a_name_is_lowercase_with_a_letter_and_opens_with_no_date() {
        for good in ["lantern", "relay-pin-is-fixed", "1password", "decade", "a"] {
            assert_eq!(Name::parse(good).unwrap().as_str(), good);
        }
        for bad in [
            "",
            "Lantern",
            "relay pin",
            "lantern/relay",
            "-relay",
            "relay-",
            "2026",
            "2026-10-09-lamp",
            "2026-10-09",
            LANTERN,
        ] {
            assert_eq!(
                Name::parse(bad),
                Err(SchemaError::NotAName(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_date_orders_and_prints_as_written() {
        let (early, late) = (
            Date::parse("2026-09-04").unwrap(),
            Date::parse("2026-10-09").unwrap(),
        );
        assert!(early < late);
        assert_eq!(late.to_string(), "2026-10-09");
        for bad in ["2026-10", "2026-13-01", "yesterday", "2026-10-09T10:00:00"] {
            assert_eq!(Date::parse(bad), Err(SchemaError::NotADate(bad.to_owned())));
        }
        let written = Writer::default().date("confirmed", late).done();
        assert_eq!(
            toml::to_string(&written).unwrap(),
            "confirmed = 2026-10-09\n"
        );
    }
}
