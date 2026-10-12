use std::fmt;

use super::slug::is_date;

/// When to look at an open item again: a date with the reason, or the next
/// session touching a topic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recheck {
    On { date: String, why: String },
    Touching(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecheckError(pub String);

impl fmt::Display for RecheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "recheck `{}` is neither `YYYY-MM-DD why` nor `touching <topic>`",
            self.0
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecheckState {
    Due,
    By,
    Touching,
}

impl Recheck {
    pub fn parse(text: &str) -> Result<Recheck, RecheckError> {
        let text = text.trim();
        if let Some(topic) = text.strip_prefix("touching ") {
            let topic = topic.trim();
            if !topic.is_empty() && !topic.contains(char::is_whitespace) {
                return Ok(Recheck::Touching(topic.to_owned()));
            }
        }
        if let Some((date, why)) = text.split_once(char::is_whitespace) {
            let why = why.trim();
            if is_date(date) && !why.is_empty() {
                return Ok(Recheck::On {
                    date: date.to_owned(),
                    why: why.to_owned(),
                });
            }
        }
        Err(RecheckError(text.to_owned()))
    }

    #[must_use]
    pub fn state(&self, today: &str, topic: Option<&str>) -> RecheckState {
        match self {
            Recheck::On { date, .. } if date.as_str() <= today => RecheckState::Due,
            Recheck::On { .. } => RecheckState::By,
            Recheck::Touching(t) if topic.is_some_and(|topic| topic.eq_ignore_ascii_case(t)) => {
                RecheckState::Due
            }
            Recheck::Touching(_) => RecheckState::Touching,
        }
    }

    /// Whether this is `touching` the topic, whatever the case.
    #[must_use]
    pub fn touches(&self, topic: &str) -> bool {
        matches!(self, Recheck::Touching(t) if t.eq_ignore_ascii_case(topic))
    }

    /// Due today for a session about any of `topics`.
    #[must_use]
    pub fn is_due(&self, today: &str, topics: &[&str]) -> bool {
        match self {
            Recheck::On { date, .. } => date.as_str() <= today,
            Recheck::Touching(_) => topics.iter().any(|t| self.touches(t)),
        }
    }

    /// The topic among `topics` this touches, for the label.
    #[must_use]
    pub fn touched<'a>(&self, topics: &[&'a str]) -> Option<&'a str> {
        topics.iter().copied().find(|t| self.touches(t))
    }

    /// The short form a listing puts in front of an item.
    #[must_use]
    pub fn label(&self, today: &str, topic: Option<&str>) -> String {
        match (self, self.state(today, topic)) {
            (Recheck::On { date, .. }, RecheckState::Due) => format!("due {date}"),
            (Recheck::On { date, .. }, _) => format!("by {date}"),
            (Recheck::Touching(t), _) => format!("touching {t}"),
        }
    }
}

impl fmt::Display for Recheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Recheck::On { date, why } => write!(f, "{date} {why}"),
            Recheck::Touching(topic) => write!(f, "touching {topic}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        assert_eq!(
            Recheck::parse("2026-10-01 the board is back").unwrap(),
            Recheck::On {
                date: "2026-10-01".into(),
                why: "the board is back".into()
            }
        );
        assert_eq!(
            Recheck::parse("touching lantern").unwrap(),
            Recheck::Touching("lantern".into())
        );
        assert!(Recheck::parse("2026-10-01").is_err());
        assert!(Recheck::parse("touching").is_err());
        assert!(Recheck::parse("soon").is_err());
    }

    #[test]
    fn states() {
        let on = Recheck::parse("2026-10-01 why").unwrap();
        assert_eq!(on.state("2026-10-01", None), RecheckState::Due);
        assert_eq!(on.state("2026-09-30", None), RecheckState::By);
        let touching = Recheck::parse("touching Lantern").unwrap();
        assert_eq!(
            touching.state("2026-09-30", Some("lantern")),
            RecheckState::Due
        );
        assert_eq!(
            touching.state("2026-09-30", Some("other")),
            RecheckState::Touching
        );
        assert_eq!(touching.state("2026-09-30", None), RecheckState::Touching);
        assert_eq!(on.label("2026-09-30", None), "by 2026-10-01");
        assert_eq!(touching.to_string(), "touching Lantern");
    }
}
