use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemaError {
    Field {
        key: String,
        why: String,
    },
    UnknownKey(String),
    UnknownKind(String),
    NotAName(String),
    NotADate(String),
    NotAnAddress(String),
    /// A draft carries a key only the tool writes.
    ToolOwned(String),
    /// A field that never changes after the first version was changed.
    SetOnce(String),
    Ended,
    NotEnded,
    ReasonOfAnotherKind {
        reason: String,
        kind: &'static str,
    },
    NoNote,
    NoDocument {
        key: String,
        name: String,
    },
    Collision {
        key: String,
        name: String,
    },
}

impl SchemaError {
    pub(super) fn field(key: &str, why: impl fmt::Display) -> SchemaError {
        SchemaError::Field {
            key: key.to_owned(),
            why: why.to_string(),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::Field { key, why } => write!(f, "`{key}` {why}"),
            SchemaError::UnknownKey(key) => write!(f, "`{key}` is no field of this kind"),
            SchemaError::UnknownKind(kind) => write!(f, "`{kind}` is no kind"),
            SchemaError::NotAName(text) => write!(
                f,
                "`{text}` is not a name: lowercase letters, digits and hyphens, with a letter, not opening with a date and not shaped as a document id"
            ),
            SchemaError::NotADate(text) => write!(f, "`{text}` is not a date"),
            SchemaError::NotAnAddress(text) => write!(f, "`{text}` names no document"),
            SchemaError::ToolOwned(key) => write!(f, "`{key}` is not a draft's to set"),
            SchemaError::SetOnce(key) => write!(f, "`{key}` never changes after it is first set"),
            SchemaError::Ended => f.write_str("the document is ended; reopen it first"),
            SchemaError::NotEnded => f.write_str("the document is not ended"),
            SchemaError::ReasonOfAnotherKind { reason, kind } => {
                write!(f, "a {kind} does not end as {reason}")
            }
            SchemaError::NoNote => f.write_str("an ending says why in its note"),
            SchemaError::NoDocument { key, name } => {
                write!(f, "`{key}` names `{name}`, which no document holds")
            }
            SchemaError::Collision { key, name } => {
                write!(f, "`{key}` names `{name}`, which several documents hold")
            }
        }
    }
}
