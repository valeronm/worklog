//! One immutable file: the envelope a write stamps, the fields and links of
//! the document as they stood, and the body.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use toml::value::Datetime;
use toml::{Table, Value};

use super::fence;
use super::id::{DocumentId, VersionId};

/// The newest file layout this reader knows.
pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionError {
    NotAKind(String),
    NotAStamp(String),
    /// The parts render to a file that parses to other parts.
    DoesNotReadBack,
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::NotAKind(text) => write!(f, "`{text}` is not a kind"),
            VersionError::NotAStamp(text) => {
                write!(f, "`{text}` is not a date and time with an offset")
            }
            VersionError::DoesNotReadBack => f.write_str("does not read back as written"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// The bytes do not hash to the name they were found under.
    Corrupt,
    /// Written by a release that knows a later layout.
    Newer {
        format: u32,
    },
    Malformed(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Corrupt => f.write_str("the bytes do not hash to the file's name"),
            ReadError::Newer { format } => write!(f, "written in format {format}, a newer one"),
            ReadError::Malformed(why) => f.write_str(why),
        }
    }
}

fn malformed(why: impl fmt::Display) -> ReadError {
    ReadError::Malformed(why.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Kind(String);

impl Kind {
    pub fn parse(text: &str) -> Result<Kind, VersionError> {
        if !text.is_empty() && text.bytes().all(|b| b.is_ascii_lowercase()) {
            Ok(Kind(text.to_owned()))
        } else {
            Err(VersionError::NotAKind(text.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// When a version was written, with the offset of the host that wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp(Datetime);

impl Stamp {
    /// Takes RFC 3339 with an offset.
    pub fn parse(text: &str) -> Result<Stamp, VersionError> {
        text.parse()
            .ok()
            .and_then(Stamp::of)
            .ok_or_else(|| VersionError::NotAStamp(text.to_owned()))
    }

    /// The date part, `YYYY-MM-DD`.
    #[must_use]
    pub fn day(&self) -> String {
        self.to_string().chars().take(10).collect()
    }

    fn of(datetime: Datetime) -> Option<Stamp> {
        (datetime.date.is_some() && datetime.time.is_some() && datetime.offset.is_some())
            .then_some(Stamp(datetime))
    }
}

impl Stamp {
    /// Nanoseconds since the epoch, whatever offset the stamp is written in.
    #[must_use]
    pub fn instant(&self) -> i128 {
        let datetime = &self.0;
        let (Some(date), Some(time), Some(offset)) =
            (datetime.date, datetime.time, datetime.offset)
        else {
            return 0;
        };
        let offset = match offset {
            toml::value::Offset::Z => 0,
            toml::value::Offset::Custom { minutes } => i64::from(minutes),
        };
        let (year, month, day) = (
            i64::from(date.year) - i64::from(date.month <= 2),
            i64::from(date.month),
            i64::from(date.day),
        );
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        let days = era * 146_097 + day_of_era - 719_468;
        let seconds = days * 86_400
            + i64::from(time.hour) * 3_600
            + i64::from(time.minute) * 60
            + i64::from(time.second.unwrap_or(0))
            - offset * 60;
        i128::from(seconds) * 1_000_000_000 + i128::from(time.nanosecond.unwrap_or(0))
    }
}

impl Ord for Stamp {
    fn cmp(&self, other: &Stamp) -> std::cmp::Ordering {
        self.instant()
            .cmp(&other.instant())
            .then_with(|| self.to_string().cmp(&other.to_string()))
    }
}

impl PartialOrd for Stamp {
    fn partial_cmp(&self, other: &Stamp) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

pub type Fields = Table;

/// What each `[[name]]` in the body resolved to when the version was
/// saved; `None` for a name no document held.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Links(pub BTreeMap<String, Option<DocumentId>>);

/// What a write stamps on a version, apart from the document's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub document: DocumentId,
    pub kind: Kind,
    pub parents: Vec<VersionId>,
    /// Every version behind this one, the parents included.
    pub ancestors: Vec<VersionId>,
    pub written: Stamp,
    /// The topic of the machine that wrote it.
    pub machine: DocumentId,
    /// The command that wrote it, as free text.
    pub change: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub id: VersionId,
    pub envelope: Envelope,
    pub fields: Fields,
    pub links: Links,
    pub body: String,
    text: String,
}

fn ids(ids: &[VersionId]) -> Value {
    Value::Array(ids.iter().map(|id| id.as_str().into()).collect())
}

fn render(envelope: &Envelope, fields: &Fields, links: &Links, body: &str) -> Option<String> {
    let mut header = Table::new();
    header.insert("format".into(), i64::from(FORMAT).into());
    header.insert("document".into(), envelope.document.as_str().into());
    header.insert("kind".into(), envelope.kind.as_str().into());
    header.insert("parents".into(), ids(&envelope.parents));
    header.insert("ancestors".into(), ids(&envelope.ancestors));
    header.insert("written".into(), Value::Datetime(envelope.written.0));
    header.insert("machine".into(), envelope.machine.as_str().into());
    header.insert("change".into(), envelope.change.as_str().into());
    header.insert("fields".into(), Value::Table(fields.clone()));
    if !links.0.is_empty() {
        let links = links
            .0
            .iter()
            .map(|(name, id)| {
                (
                    name.clone(),
                    id.as_ref().map_or("", DocumentId::as_str).into(),
                )
            })
            .collect();
        header.insert("links".into(), Value::Table(links));
    }
    fence::join(&toml::to_string(&header).ok()?, body).ok()
}

fn take(header: &mut Table, key: &str) -> Result<Value, ReadError> {
    header
        .remove(key)
        .ok_or_else(|| malformed(format!("no `{key}`")))
}

pub(super) fn text_of(header: &mut Table, key: &str) -> Result<String, ReadError> {
    match take(header, key)? {
        Value::String(text) => Ok(text),
        _ => Err(malformed(format!("`{key}` is not text"))),
    }
}

fn document_of(header: &mut Table, key: &str) -> Result<DocumentId, ReadError> {
    DocumentId::parse(&text_of(header, key)?).map_err(malformed)
}

pub(super) fn versions_of(header: &mut Table, key: &str) -> Result<Vec<VersionId>, ReadError> {
    let Value::Array(items) = take(header, key)? else {
        return Err(malformed(format!("`{key}` is not a list")));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or_else(|| malformed(format!("`{key}` holds what is not text")))
                .and_then(|text| VersionId::parse(text).map_err(malformed))
        })
        .collect()
}

fn table_of(header: &mut Table, key: &str) -> Result<Table, ReadError> {
    match header.remove(key) {
        None => Ok(Table::new()),
        Some(Value::Table(table)) => Ok(table),
        Some(_) => Err(malformed(format!("`{key}` is not a table"))),
    }
}

fn links_of(header: &mut Table) -> Result<Links, ReadError> {
    table_of(header, "links")?
        .into_iter()
        .map(|(name, id)| match id.as_str() {
            Some("") => Ok((name, None)),
            Some(id) => Ok((name, Some(DocumentId::parse(id).map_err(malformed)?))),
            None => Err(malformed(format!("the link `{name}` is not text"))),
        })
        .collect::<Result<_, _>>()
        .map(Links)
}

impl Envelope {
    #[must_use]
    pub fn first(
        document: DocumentId,
        kind: Kind,
        written: Stamp,
        machine: DocumentId,
        change: &str,
    ) -> Envelope {
        Envelope {
            document,
            kind,
            parents: Vec::new(),
            ancestors: Vec::new(),
            written,
            machine,
            change: change.to_owned(),
        }
    }

    /// The envelope of a version written on top of `heads`, or `None`
    /// when there are none to follow.
    #[must_use]
    pub fn following(
        heads: &[&Version],
        written: Stamp,
        machine: DocumentId,
        change: &str,
    ) -> Option<Envelope> {
        let first = heads.first()?;
        let parents: BTreeSet<&VersionId> = heads.iter().map(|head| &head.id).collect();
        let ancestors: BTreeSet<&VersionId> = heads
            .iter()
            .flat_map(|head| &head.envelope.ancestors)
            .chain(parents.iter().copied())
            .collect();
        Some(Envelope {
            document: first.envelope.document.clone(),
            kind: first.envelope.kind.clone(),
            parents: parents.into_iter().cloned().collect(),
            ancestors: ancestors.into_iter().cloned().collect(),
            written,
            machine,
            change: change.to_owned(),
        })
    }
}

impl Version {
    /// Builds the version from its parts, refusing parts whose file would
    /// parse to anything else.
    pub fn compose(
        envelope: Envelope,
        fields: Fields,
        links: Links,
        body: String,
    ) -> Result<Version, VersionError> {
        let text =
            render(&envelope, &fields, &links, &body).ok_or(VersionError::DoesNotReadBack)?;
        let version = Version {
            id: VersionId::of(text.as_bytes()),
            envelope,
            fields,
            links,
            body,
            text,
        };
        if Version::read(&version.id, &version.text).as_ref() == Ok(&version) {
            Ok(version)
        } else {
            Err(VersionError::DoesNotReadBack)
        }
    }

    /// The bytes the version was composed as or read from.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Refuses bytes that do not hash to `name`.
    pub fn read(name: &VersionId, text: &str) -> Result<Version, ReadError> {
        if VersionId::of(text.as_bytes()) != *name {
            return Err(ReadError::Corrupt);
        }
        let (header, body) = fence::split(text).map_err(malformed)?;
        let mut header: Table = header
            .parse()
            .map_err(|e| malformed(format!("the header is not TOML: {e}")))?;
        let format = match take(&mut header, "format")? {
            Value::Integer(format) => u32::try_from(format).ok().filter(|f| *f > 0),
            _ => None,
        }
        .ok_or_else(|| malformed("`format` is not a format number"))?;
        if format > FORMAT {
            return Err(ReadError::Newer { format });
        }
        let envelope = Envelope {
            document: document_of(&mut header, "document")?,
            kind: Kind::parse(&text_of(&mut header, "kind")?).map_err(malformed)?,
            parents: versions_of(&mut header, "parents")?,
            ancestors: versions_of(&mut header, "ancestors")?,
            written: match take(&mut header, "written")? {
                Value::Datetime(written) => Stamp::of(written),
                _ => None,
            }
            .ok_or_else(|| malformed("`written` is not a date and time with an offset"))?,
            machine: document_of(&mut header, "machine")?,
            change: text_of(&mut header, "change")?,
        };
        let fields = table_of(&mut header, "fields")?;
        let links = links_of(&mut header)?;
        if let Some(key) = header.keys().next() {
            return Err(malformed(format!("`{key}` is no key of format {format}")));
        }
        Ok(Version {
            id: name.clone(),
            envelope,
            fields,
            links,
            body: body.to_owned(),
            text: text.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_order_by_the_moment_whatever_the_offset() {
        let stamp = |text| Stamp::parse(text).unwrap();
        let later = stamp("2026-10-09T11:30:00+02:00");
        let earlier = stamp("2026-10-09T09:00:00+00:00");
        assert!(earlier < later);
        assert!(stamp("2026-10-09T08:30:00+00:00") < stamp("2026-10-09T08:30:00.5+00:00"));
        assert!(stamp("2026-02-28T23:00:00Z") < stamp("2026-03-01T00:00:00Z"));
        assert!(stamp("2025-12-31T23:59:59Z") < stamp("2026-01-01T00:00:00Z"));
        let (zulu, shifted) = (
            stamp("2026-10-09T10:00:00Z"),
            stamp("2026-10-09T11:00:00+01:00"),
        );
        assert_eq!(zulu.instant(), shifted.instant());
        assert_ne!(zulu, shifted);
        assert_eq!(zulu.cmp(&zulu), std::cmp::Ordering::Equal);
    }

    const LANTERN: &str = "7f3a91c05be2446d8a10c3f29b7e6d54";
    const DESK: &str = "03be552100000000000000000000000a";
    const BOOT_STRAPS: &str = "2be0417f00000000000000000000000b";

    fn envelope() -> Envelope {
        Envelope {
            document: DocumentId::parse(LANTERN).unwrap(),
            kind: Kind::parse("fact").unwrap(),
            parents: vec![VersionId::of(b"parent")],
            ancestors: vec![VersionId::of(b"parent"), VersionId::of(b"root")],
            written: Stamp::parse("2026-10-09T18:22:41.118204+01:00").unwrap(),
            machine: DocumentId::parse(DESK).unwrap(),
            change: "save".to_owned(),
        }
    }

    fn fields() -> Fields {
        let mut fields = Fields::new();
        fields.insert("name".into(), "relay-pin-is-fixed".into());
        fields.insert(
            "summary".into(),
            "The relay's pin is fixed, since the \"boot strap\" pins can't be driven".into(),
        );
        fields.insert("idea".into(), false.into());
        fields
    }

    fn links() -> Links {
        Links(BTreeMap::from([
            (
                "lantern/boot-straps".to_owned(),
                Some(DocumentId::parse(BOOT_STRAPS).unwrap()),
            ),
            ("lantern/unwritten".to_owned(), None),
        ]))
    }

    fn version(body: &str) -> Version {
        Version::compose(envelope(), fields(), links(), body.to_owned()).unwrap()
    }

    #[test]
    fn a_composed_version_reads_back_as_it_was_composed() {
        let version = version("The relay is wired to GPIO 5.\n");
        assert_eq!(version.id, VersionId::of(version.text().as_bytes()));
        assert_eq!(Version::read(&version.id, version.text()), Ok(version));
    }

    #[test]
    fn the_header_keeps_the_envelope_first_and_in_one_order() {
        let version = version("body\n");
        let text = version.text();
        assert!(text.starts_with("+++\nformat = 1\n"), "{text}");
        let at = |needle: &str| {
            text.find(needle)
                .unwrap_or_else(|| panic!("{needle}: {text}"))
        };
        let order = [
            "format = ",
            "document = ",
            "kind = ",
            "parents = ",
            "ancestors = ",
            "written = 2026-10-09T18:22:41.118204+01:00",
            "machine = ",
            "change = ",
            "[fields]",
            "[links]",
            "\n+++\nbody\n",
        ];
        assert!(order.map(at).is_sorted(), "{text}");
        assert!(!text.contains(version.id.as_str()), "{text}");
    }

    #[test]
    fn a_file_written_by_hand_reads_to_its_parts() {
        let text = format!(
            "+++\n\
             format = 1\n\
             document = \"{LANTERN}\"\n\
             kind = \"fact\"\n\
             parents = []\n\
             ancestors = []\n\
             written = 2026-09-04T10:00:00+01:00\n\
             machine = \"{DESK}\"\n\
             change = \"new\"\n\
             \n\
             [fields]\n\
             name = \"relay-pin-is-fixed\"\n\
             idea = false\n\
             \n\
             [links]\n\
             \"lantern/boot-straps\" = \"{BOOT_STRAPS}\"\n\
             \"lantern/unwritten\" = \"\"\n\
             +++\n\
             \n\
             The relay is wired to GPIO 5.\n"
        );
        let version = Version::read(&VersionId::of(text.as_bytes()), &text).unwrap();
        assert_eq!(version.envelope.document.as_str(), LANTERN);
        assert_eq!(version.envelope.kind.as_str(), "fact");
        assert!(version.envelope.parents.is_empty());
        assert_eq!(version.envelope.change, "new");
        assert_eq!(
            version.fields.get("name").and_then(|v| v.as_str()),
            Some("relay-pin-is-fixed")
        );
        assert_eq!(version.links, links());
        assert_eq!(version.body, "\nThe relay is wired to GPIO 5.\n");
        assert_eq!(version.text(), text);
    }

    #[test]
    fn a_version_with_no_links_carries_no_links_table() {
        let version =
            Version::compose(envelope(), fields(), Links::default(), String::new()).unwrap();
        assert!(!version.text().contains("[links]"), "{}", version.text());
        assert_eq!(Version::read(&version.id, version.text()), Ok(version));
    }

    #[test]
    fn bytes_that_do_not_hash_to_the_name_are_corrupt() {
        let version = version("The relay is wired to GPIO 5.\n");
        let damaged = version.text().replace("GPIO 5", "GPIO 6");
        assert_eq!(
            Version::read(&version.id, &damaged),
            Err(ReadError::Corrupt)
        );
    }

    #[test]
    fn a_higher_format_is_newer_and_not_damage() {
        let text = version("body\n")
            .text()
            .replace("format = 1\n", "format = 2\n");
        assert_eq!(
            Version::read(&VersionId::of(text.as_bytes()), &text),
            Err(ReadError::Newer { format: 2 })
        );
    }

    #[test]
    fn what_is_no_version_is_malformed_with_the_reason() {
        for (text, reason) in [
            ("body only\n".to_owned(), "no opening fence"),
            ("+++\nformat = 1\n".to_owned(), "no closing fence"),
            ("+++\nformat = \n+++\n".to_owned(), "TOML"),
            (
                version("body\n").text().replace("kind = \"fact\"\n", ""),
                "kind",
            ),
            (
                version("body\n")
                    .text()
                    .replace("change = \"save\"\n", "change = \"save\"\nsurprise = 1\n"),
                "surprise",
            ),
        ] {
            match Version::read(&VersionId::of(text.as_bytes()), &text) {
                Err(ReadError::Malformed(why)) => assert!(why.contains(reason), "{why}"),
                other => panic!("{reason}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_fence_line_in_the_body_stays_in_the_body() {
        let version = version("before\n+++\nafter\n");
        assert_eq!(
            Version::read(&version.id, version.text()).unwrap().body,
            "before\n+++\nafter\n"
        );
    }

    #[test]
    fn a_field_holding_a_fence_line_is_refused() {
        let mut fields = fields();
        fields.insert("note".into(), "before\n+++\nafter".into());
        assert_eq!(
            Version::compose(envelope(), fields, links(), String::new()),
            Err(VersionError::DoesNotReadBack)
        );
    }

    #[test]
    fn a_field_of_several_lines_reads_back() {
        let mut fields = fields();
        fields.insert("note".into(), "before\nafter".into());
        let version = Version::compose(envelope(), fields, links(), String::new()).unwrap();
        assert_eq!(Version::read(&version.id, version.text()), Ok(version));
    }

    #[test]
    fn a_kind_is_a_lowercase_word_and_a_stamp_carries_its_offset() {
        assert!(Kind::parse("fact").is_ok());
        for bad in ["", "Fact", "fact 2", "fo/o"] {
            assert!(Kind::parse(bad).is_err(), "{bad}");
        }
        for bad in ["2026-10-09", "2026-10-09T18:22:41", "yesterday"] {
            assert!(Stamp::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            Stamp::parse("2026-10-09T18:22:41.118204+01:00")
                .unwrap()
                .to_string(),
            "2026-10-09T18:22:41.118204+01:00"
        );
    }

    #[test]
    fn a_stamp_reports_its_day() {
        let stamp = Stamp::parse("2026-10-09T18:22:41.118204+01:00").unwrap();
        assert_eq!(stamp.day(), "2026-10-09");
    }
}
