//! A document being edited on this machine: its fields and body as a
//! session edits them, and beside them what the edit started from.

use std::fmt;

use toml::Table;

use super::fence::{self, FenceError};
use super::id::{DocumentId, VersionId};
use super::version::{self, Fields, Kind, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DraftError {
    Fence(FenceError),
    NotToml(String),
    /// The file kept beside the draft does not say what it started from.
    Origin(String),
}

impl fmt::Display for DraftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DraftError::Fence(e) => e.fmt(f),
            DraftError::NotToml(why) => write!(f, "the fields are not TOML: {why}"),
            DraftError::Origin(why) => write!(f, "the draft's origin: {why}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub document: DocumentId,
    pub kind: Kind,
    /// The heads the edit started from; none for a document being created.
    pub parents: Vec<VersionId>,
    pub fields: Fields,
    pub body: String,
}

fn content(text: &str) -> Result<(Fields, String), DraftError> {
    let (fields, body) = fence::split(text).map_err(DraftError::Fence)?;
    let fields = fields
        .parse()
        .map_err(|e: toml::de::Error| DraftError::NotToml(e.to_string()))?;
    Ok((fields, body.to_owned()))
}

impl Draft {
    #[must_use]
    pub fn first(document: DocumentId, kind: Kind, fields: Fields, body: String) -> Draft {
        Draft {
            document,
            kind,
            parents: Vec::new(),
            fields,
            body,
        }
    }

    #[must_use]
    pub fn of(version: &Version) -> Draft {
        Draft {
            document: version.envelope.document.clone(),
            kind: version.envelope.kind.clone(),
            parents: vec![version.id.clone()],
            fields: version.fields.clone(),
            body: version.body.clone(),
        }
    }

    pub fn text(&self) -> Result<String, DraftError> {
        let fields =
            toml::to_string(&self.fields).map_err(|e| DraftError::NotToml(e.to_string()))?;
        fence::join(&fields, &self.body).map_err(DraftError::Fence)
    }

    /// The draft with the fields and body of an edited `text`.
    pub fn with_text(&self, text: &str) -> Result<Draft, DraftError> {
        let (fields, body) = content(text)?;
        Ok(Draft {
            fields,
            body,
            ..self.clone()
        })
    }

    /// What is kept beside the text.
    #[must_use]
    pub fn origin(&self) -> String {
        let parents: Vec<String> = self.parents.iter().map(|id| format!("\"{id}\"")).collect();
        format!(
            "kind = \"{}\"\nparents = [{}]\n",
            self.kind,
            parents.join(", ")
        )
    }

    pub fn read(document: DocumentId, origin: &str, text: &str) -> Result<Draft, DraftError> {
        let wrong = |why: &dyn fmt::Display| DraftError::Origin(why.to_string());
        let mut origin: Table = origin.parse().map_err(|e: toml::de::Error| wrong(&e))?;
        let kind = version::text_of(&mut origin, "kind").map_err(|e| wrong(&e))?;
        let parents = version::versions_of(&mut origin, "parents").map_err(|e| wrong(&e))?;
        let (fields, body) = content(text)?;
        Ok(Draft {
            document,
            kind: Kind::parse(&kind).map_err(|e| wrong(&e))?,
            parents,
            fields,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{first, lantern};

    fn draft() -> Draft {
        Draft::of(&first(
            &lantern(),
            "fact",
            "name = \"relay\"\ntopics = [\"lantern\", \"phone\"]",
            "The relay is wired to GPIO 5.\n",
        ))
    }

    #[test]
    fn a_draft_of_a_version_starts_from_it() {
        let version = first(&lantern(), "fact", "name = \"relay\"", "body\n");
        let draft = Draft::of(&version);
        assert_eq!(draft.document, lantern());
        assert_eq!(draft.kind.as_str(), "fact");
        assert_eq!(draft.parents, std::slice::from_ref(&version.id));
        assert_eq!(draft.fields, version.fields);
        assert_eq!(draft.body, "body\n");
        let fresh = Draft::first(lantern(), draft.kind.clone(), Fields::new(), String::new());
        assert!(fresh.parents.is_empty());
    }

    #[test]
    fn the_text_is_the_fields_and_the_body_and_nothing_of_the_tools() {
        let draft = draft();
        let text = draft.text().unwrap();
        assert_eq!(
            text,
            "+++\nname = \"relay\"\ntopics = [\"lantern\", \"phone\"]\n+++\nThe relay is wired to GPIO 5.\n"
        );
        assert_eq!(draft.with_text(&text), Ok(draft));
    }

    #[test]
    fn an_edited_text_keeps_where_the_draft_started() {
        let draft = draft();
        let edited = draft
            .with_text("+++\nname = \"relay-pin\"\n+++\nRewired.\n")
            .unwrap();
        assert_eq!(
            edited.fields.get("name").and_then(|name| name.as_str()),
            Some("relay-pin")
        );
        assert_eq!(edited.fields.len(), 1);
        assert_eq!(edited.body, "Rewired.\n");
        assert_eq!(edited.parents, draft.parents);
        assert_eq!(edited.document, draft.document);
    }

    #[test]
    fn a_text_that_is_no_draft_is_refused_with_the_reason() {
        let draft = draft();
        assert_eq!(
            draft.with_text("name = \"relay\"\n"),
            Err(DraftError::Fence(FenceError::NoOpening))
        );
        match draft.with_text("+++\nname: relay\n+++\n") {
            Err(DraftError::NotToml(why)) => assert!(!why.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_field_holding_a_fence_line_has_no_text() {
        let mut draft = draft();
        draft
            .fields
            .insert("note".into(), "before\n+++\nafter".into());
        assert_eq!(draft.text(), Err(DraftError::Fence(FenceError::InHeader)));
    }

    #[test]
    fn a_draft_reads_back_from_its_origin_and_its_text() {
        let draft = draft();
        let text = draft.text().unwrap();
        assert_eq!(
            Draft::read(lantern(), &draft.origin(), &text),
            Ok(draft.clone())
        );
        let fresh = Draft::first(lantern(), draft.kind.clone(), Fields::new(), String::new());
        assert_eq!(fresh.origin(), "kind = \"fact\"\nparents = []\n");
        for origin in [
            "",
            "kind = \"fact\"\n",
            "kind = \"Fact\"\nparents = []\n",
            "kind = \"fact\"\nparents = [\"abc\"]\n",
            "kind",
        ] {
            assert!(
                matches!(
                    Draft::read(lantern(), origin, &text),
                    Err(DraftError::Origin(_))
                ),
                "{origin}"
            );
        }
    }
}
