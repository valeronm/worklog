use std::fmt;

/// The four document kinds, each with its own directory and slug shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Entry,
    Fact,
    Topic,
    Followup,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Entry, Kind::Fact, Kind::Topic, Kind::Followup];

    /// An entry is what was true on its date, so its links cite; every
    /// other kind is live and rests on what it links.
    #[must_use]
    pub fn cites(self) -> bool {
        matches!(self, Kind::Entry)
    }

    #[must_use]
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Entry => "entry",
            Kind::Fact => "fact",
            Kind::Topic => "topic",
            Kind::Followup => "followup",
        }
    }

    #[must_use]
    pub fn from_dir(dir: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.dir() == dir)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.dir())
    }
}

/// A document's identity: its kind and its path under the kind directory.
/// The shape of the path says the kind, so a bare slug names one document
/// across the store; `docs/documents.md` has the shapes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug {
    kind: Kind,
    path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlugError {
    BadSegment(String),
    Shape { text: String, kind: Option<Kind> },
}

impl fmt::Display for SlugError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SlugError::BadSegment(segment) => {
                write!(
                    f,
                    "slug segment `{segment}` may hold only letters, digits, `.`, `_` and `-` and may not open with `.`"
                )
            }
            SlugError::Shape {
                text,
                kind: Some(kind),
            } => write!(f, "`{text}` is not shaped like a {kind} slug"),
            SlugError::Shape { text, kind: None } => {
                write!(f, "`{text}` is shaped like no kind of slug")
            }
        }
    }
}

fn segment_ok(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// `YYYY-MM-DD` by shape alone; whether the day exists is not this type's business.
#[must_use]
pub fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

fn is_dated_name(text: &str) -> bool {
    text.len() > 11
        && is_date(&text[..10])
        && text.as_bytes()[10] == b'-'
        && segment_ok(&text[11..])
}

fn has_letter(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_alphabetic())
}

fn shape(segments: &[&str]) -> Option<Kind> {
    match segments {
        // The month directory is the date's own prefix, so a valid name
        // makes it `YYYY-MM` by construction.
        [month, name] if month.len() == 7 && is_dated_name(name) && name.starts_with(month) => {
            Some(Kind::Entry)
        }
        [name] if is_dated_name(name) => Some(Kind::Followup),
        [topic, _] if has_letter(topic) => Some(Kind::Fact),
        [topic] if has_letter(topic) => Some(Kind::Topic),
        _ => None,
    }
}

impl Slug {
    pub fn parse(text: &str) -> Result<Slug, SlugError> {
        let segments = Slug::segments(text)?;
        match shape(&segments) {
            Some(kind) => Ok(Slug {
                kind,
                path: text.to_owned(),
            }),
            None => Err(SlugError::Shape {
                text: text.to_owned(),
                kind: None,
            }),
        }
    }

    pub fn of_kind(kind: Kind, text: &str) -> Result<Slug, SlugError> {
        let segments = Slug::segments(text)?;
        if shape(&segments) == Some(kind) {
            Ok(Slug {
                kind,
                path: text.to_owned(),
            })
        } else {
            Err(SlugError::Shape {
                text: text.to_owned(),
                kind: Some(kind),
            })
        }
    }

    /// `<month>/<date>-<name>`; the date is checked by shape here and the
    /// entry's own `date` field has to equal it.
    pub fn entry(date: &str, name: &str) -> Result<Slug, SlugError> {
        let month = date.get(..7).unwrap_or(date);
        Slug::of_kind(Kind::Entry, &format!("{month}/{date}-{name}"))
    }

    pub fn followup(date: &str, name: &str) -> Result<Slug, SlugError> {
        Slug::of_kind(Kind::Followup, &format!("{date}-{name}"))
    }

    pub fn fact(topic: &str, name: &str) -> Result<Slug, SlugError> {
        Slug::of_kind(Kind::Fact, &format!("{topic}/{name}"))
    }

    fn segments(text: &str) -> Result<Vec<&str>, SlugError> {
        let segments: Vec<&str> = text.split('/').collect();
        match segments.iter().find(|segment| !segment_ok(segment)) {
            Some(bad) => Err(SlugError::BadSegment((*bad).to_owned())),
            None => Ok(segments),
        }
    }

    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The topic a fact sits under.
    #[must_use]
    pub fn topic(&self) -> Option<&str> {
        match self.kind {
            Kind::Fact => self.path.split('/').next(),
            _ => None,
        }
    }

    /// The last segment, which is what a listing shows when the kind and
    /// the topic are already on the page.
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The date an entry or followup slug opens with.
    #[must_use]
    pub fn date(&self) -> Option<&str> {
        match self.kind {
            Kind::Entry | Kind::Followup => Some(&self.name()[..10]),
            _ => None,
        }
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_decide_the_kind() {
        assert_eq!(Slug::parse("lantern").unwrap().kind(), Kind::Topic);
        assert_eq!(
            Slug::parse("lantern/ec-firmware").unwrap().kind(),
            Kind::Fact
        );
        assert_eq!(
            Slug::parse("2026-09/2026-09-04-lamp-driver")
                .unwrap()
                .kind(),
            Kind::Entry
        );
        assert_eq!(
            Slug::parse("2026-09-04-port").unwrap().kind(),
            Kind::Followup
        );
    }

    #[test]
    fn an_entry_month_must_match_its_date() {
        assert!(Slug::parse("2026-08/2026-09-04-lamp-driver").is_err());
        assert!(Slug::parse("2026/2026-09-04-lamp-driver").is_err());
    }

    #[test]
    fn a_topic_needs_a_letter() {
        assert!(Slug::parse("2026").is_err());
        assert!(Slug::parse("2026/name").is_err());
    }

    #[test]
    fn segments_are_checked_before_shape() {
        assert_eq!(
            Slug::parse("lantern/.hidden"),
            Err(SlugError::BadSegment(".hidden".into()))
        );
        assert!(Slug::parse("frame guin").is_err());
        assert!(Slug::parse("").is_err());
    }

    #[test]
    fn of_kind_refuses_another_shape() {
        assert!(Slug::of_kind(Kind::Fact, "lantern").is_err());
        assert!(Slug::of_kind(Kind::Topic, "lantern").is_ok());
    }

    #[test]
    fn parts() {
        let fact = Slug::parse("lantern/ec-firmware").unwrap();
        assert_eq!(fact.topic(), Some("lantern"));
        assert_eq!(fact.name(), "ec-firmware");
        let entry = Slug::parse("2026-09/2026-09-04-lamp-driver").unwrap();
        assert_eq!(entry.date(), Some("2026-09-04"));
        assert_eq!(entry.name(), "2026-09-04-lamp-driver");
    }
}
