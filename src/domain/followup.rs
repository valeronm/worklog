use std::fmt;

use super::frontmatter::{FieldError, Fields};
use super::recheck::Recheck;
use super::slug::{Kind, Slug};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowupState {
    Open,
    Done,
    Dropped,
}

impl FollowupState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FollowupState::Open => "open",
            FollowupState::Done => "done",
            FollowupState::Dropped => "dropped",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<FollowupState> {
        [
            FollowupState::Open,
            FollowupState::Done,
            FollowupState::Dropped,
        ]
        .into_iter()
        .find(|s| s.as_str() == text)
    }
}

impl fmt::Display for FollowupState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Open work that arose in an entry and is closed on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Followup {
    pub entry: Slug,
    pub tags: Vec<String>,
    pub recheck: Option<Recheck>,
    pub state: FollowupState,
    pub summary: String,
}

pub const KEYS: [&str; 5] = ["entry", "tags", "recheck", "state", "summary"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotOpen(pub FollowupState);

impl fmt::Display for NotOpen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the follow-up is already {}", self.0)
    }
}

impl Followup {
    pub fn from_fields(fields: &Fields) -> Result<Followup, FieldError> {
        fields.reject_unknown(&KEYS)?;
        let entry = Slug::of_kind(Kind::Entry, fields.required("entry")?)
            .map_err(|e| FieldError::invalid("entry", e))?;
        let recheck = fields
            .optional("recheck")
            .map(Recheck::parse)
            .transpose()
            .map_err(|e| FieldError::invalid("recheck", e))?;
        let state = fields.required("state")?;
        let state = FollowupState::parse(state)
            .ok_or_else(|| FieldError::invalid("state", format!("`{state}` is no state")))?;
        Ok(Followup {
            entry,
            tags: fields.list_or_empty("tags"),
            recheck,
            state,
            summary: fields.required("summary")?.to_owned(),
        })
    }

    #[must_use]
    pub fn to_fields(&self) -> Fields {
        let mut fields = Fields::default();
        fields.push_scalar("entry", self.entry.path());
        fields.push_list("tags", &self.tags);
        if let Some(recheck) = &self.recheck {
            fields.push_scalar("recheck", &recheck.to_string());
        }
        fields.push_scalar("state", self.state.as_str());
        fields.push_scalar("summary", &self.summary);
        fields
    }

    fn closed(&self, state: FollowupState) -> Result<Followup, NotOpen> {
        match self.state {
            FollowupState::Open => Ok(Followup {
                state,
                ..self.clone()
            }),
            other => Err(NotOpen(other)),
        }
    }

    pub fn done(&self) -> Result<Followup, NotOpen> {
        self.closed(FollowupState::Done)
    }

    pub fn dropped(&self) -> Result<Followup, NotOpen> {
        self.closed(FollowupState::Dropped)
    }

    pub fn rescheduled(&self, recheck: Recheck) -> Result<Followup, NotOpen> {
        match self.state {
            FollowupState::Open => Ok(Followup {
                recheck: Some(recheck),
                ..self.clone()
            }),
            other => Err(NotOpen(other)),
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state == FollowupState::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Followup {
        Followup {
            entry: Slug::parse("2026-09/2026-09-02-rewrite").unwrap(),
            tags: vec!["worklog".into()],
            recheck: None,
            state: FollowupState::Open,
            summary: "port it".into(),
        }
    }

    #[test]
    fn round_trip() {
        let f = Followup {
            recheck: Some(Recheck::parse("2026-10-01 why").unwrap()),
            ..open()
        };
        assert_eq!(Followup::from_fields(&f.to_fields()).unwrap(), f);
    }

    #[test]
    fn transitions_only_from_open() {
        let done = open().done().unwrap();
        assert_eq!(done.state, FollowupState::Done);
        assert_eq!(done.dropped(), Err(NotOpen(FollowupState::Done)));
        assert_eq!(
            done.rescheduled(Recheck::Touching("x".into())),
            Err(NotOpen(FollowupState::Done))
        );
        assert!(
            open()
                .rescheduled(Recheck::Touching("x".into()))
                .unwrap()
                .is_open()
        );
    }

    #[test]
    fn the_entry_must_be_an_entry_slug() {
        let mut fields = open().to_fields();
        fields.set(
            "entry",
            super::super::frontmatter::Value::Scalar("lantern".into()),
        );
        assert!(matches!(
            Followup::from_fields(&fields),
            Err(FieldError::Invalid { .. })
        ));
    }
}
