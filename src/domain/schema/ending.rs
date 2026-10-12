//! How a document ends: a reason from its kind's list, the day, what
//! ended it, and a note.

use super::error::SchemaError;
use super::field::{Date, Reader, Writer};
use crate::domain::id::DocumentId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    False,
    Moved,
    Superseded,
    Built,
    Abandoned,
    Done,
    Dropped,
    Retired,
    Merged,
    Removed,
    /// A word a newer release writes.
    Other(String),
}

impl Reason {
    const KNOWN: [Reason; 10] = [
        Reason::False,
        Reason::Moved,
        Reason::Superseded,
        Reason::Built,
        Reason::Abandoned,
        Reason::Done,
        Reason::Dropped,
        Reason::Retired,
        Reason::Merged,
        Reason::Removed,
    ];

    #[must_use]
    pub fn parse(word: &str) -> Reason {
        Reason::KNOWN
            .into_iter()
            .find(|known| known.word() == word)
            .unwrap_or_else(|| Reason::Other(word.to_owned()))
    }

    #[must_use]
    pub fn word(&self) -> &str {
        match self {
            Reason::False => "false",
            Reason::Moved => "moved",
            Reason::Superseded => "superseded",
            Reason::Built => "built",
            Reason::Abandoned => "abandoned",
            Reason::Done => "done",
            Reason::Dropped => "dropped",
            Reason::Retired => "retired",
            Reason::Merged => "merged",
            Reason::Removed => "removed",
            Reason::Other(word) => word,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ending {
    pub reason: Reason,
    pub on: Date,
    /// The document that ended it.
    pub by: Option<DocumentId>,
    pub note: String,
}

impl Ending {
    pub(super) const BY: &'static str = "ended_by";
    pub(super) const KEYS: [&'static str; 4] = ["ended", "ended_on", Ending::BY, "note"];

    pub(super) fn read(reader: &mut Reader) -> Result<Option<Ending>, SchemaError> {
        let reason = reader.optional_line("ended")?;
        let on = reader.optional_date("ended_on")?;
        let by = reader.optional_id(Ending::BY)?;
        let note = reader.optional_text("note")?;
        let Some(reason) = reason else {
            let stray = [
                ("ended_on", on.is_some()),
                (Ending::BY, by.is_some()),
                ("note", note.is_some()),
            ];
            return match stray.iter().find(|(_, present)| *present) {
                Some((key, _)) => Err(SchemaError::field(key, "is set without `ended`")),
                None => Ok(None),
            };
        };
        Ok(Some(Ending {
            reason: Reason::parse(&reason),
            on: on.ok_or_else(|| SchemaError::field("ended_on", "is missing"))?,
            by,
            note: note.unwrap_or_default(),
        }))
    }

    pub(super) fn write(&self, writer: Writer) -> Writer {
        let note = (!self.note.is_empty()).then_some(self.note.as_str());
        writer
            .text("ended", self.reason.word())
            .date("ended_on", self.on)
            .optional_id(Ending::BY, self.by.as_ref())
            .optional_text("note", note)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_reason_parses_from_its_own_word() {
        for reason in Reason::KNOWN {
            assert_eq!(Reason::parse(reason.word()), reason);
        }
        assert_eq!(
            Reason::parse("archived"),
            Reason::Other("archived".to_owned())
        );
        assert_eq!(Reason::Other("archived".to_owned()).word(), "archived");
    }

    #[test]
    fn an_endings_keys_are_the_ones_it_writes() {
        let ending = Ending {
            reason: Reason::Moved,
            on: Date::parse("2026-10-09").unwrap(),
            by: Some(DocumentId::from_bytes([7; 16])),
            note: "to the board's README".to_owned(),
        };
        let written = ending.write(Writer::default()).done();
        assert_eq!(
            written.keys().map(String::as_str).collect::<Vec<_>>(),
            Ending::KEYS
        );
    }
}
