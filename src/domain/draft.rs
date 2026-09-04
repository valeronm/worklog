use super::frontmatter::{self, FieldError, Fields, ParseError, Value};
use super::version::{self, Version, VersionError, VersionId};

/// A document being written: the kind's own fields and the body, plus the
/// versions it was checked out from. Everything else a version carries is
/// stamped by `save`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub slug: super::slug::Slug,
    pub parents: Vec<VersionId>,
    pub fields: Fields,
    pub body: String,
}

const CONFLICT_OPEN: &str = "<<<<<<<";
const CONFLICT_SEPARATOR: &str = "=======";
const CONFLICT_CLOSE: &str = ">>>>>>>";

impl Draft {
    #[must_use]
    pub fn to_text(&self) -> String {
        let block = Value::Map(vec![(
            "parents".to_owned(),
            Value::List(self.parents.iter().map(ToString::to_string).collect()),
        )]);
        frontmatter::emit(
            &version::envelope(&self.slug, &self.fields, block),
            &self.body,
        )
    }

    /// Reads a draft back after editing; only `parents` is accepted under
    /// `version`, since a person can change nothing else there.
    pub fn from_text(text: &str) -> Result<Draft, VersionError> {
        let split = frontmatter::parse(text)?;
        let (slug, block, fields) = version::open_envelope(split.fields)?;
        let parents = match block {
            Value::Map(entries) => {
                let block: Fields = entries.into_iter().collect();
                block.reject_unknown(&["parents"])?;
                version::parse_parents(&block)?
            }
            Value::Scalar(s) if s.is_empty() => Vec::new(),
            _ => return Err(FieldError::Missing("version").into()),
        };
        Ok(Draft {
            slug,
            parents,
            fields,
            body: split.body,
        })
    }

    /// A draft holding every head of a fork, for a person to reconcile:
    /// the first head's fields, and the bodies between conflict markers.
    #[must_use]
    pub fn merging(heads: &[Version]) -> Draft {
        let mut body = String::new();
        for (i, head) in heads.iter().enumerate() {
            let marker = if i == 0 {
                CONFLICT_OPEN
            } else {
                CONFLICT_SEPARATOR
            };
            let header = format!(
                "{marker} {} written {} on {}\n",
                head.id.short(),
                head.block.written,
                head.block.machine
            );
            body.push_str(&header);
            if head.fields != heads[0].fields {
                body.push_str("(fields differ from the first head:)\n");
                body.push_str(&frontmatter::emit(&head.fields, ""));
            }
            body.push_str(&head.body);
        }
        body.push_str(CONFLICT_CLOSE);
        body.push('\n');
        Draft {
            slug: heads[0].slug.clone(),
            parents: heads.iter().map(|h| h.id.clone()).collect(),
            fields: heads[0].fields.clone(),
            body,
        }
    }

    /// Whether a merging draft still carries its markers.
    #[must_use]
    pub fn has_conflict_markers(&self) -> bool {
        self.body
            .lines()
            .any(|l| l.starts_with(CONFLICT_OPEN) || l.starts_with(CONFLICT_CLOSE))
    }
}

impl From<ParseError> for FieldError {
    fn from(e: ParseError) -> Self {
        FieldError::invalid("frontmatter", e)
    }
}

#[cfg(test)]
mod tests {
    use super::super::slug::Slug;
    use super::*;

    #[test]
    fn round_trip() {
        let mut fields = Fields::default();
        fields.push_scalar("summary", "s");
        let draft = Draft {
            slug: Slug::parse("lantern/x").unwrap(),
            parents: vec![VersionId::of("a")],
            fields,
            body: "\nbody\n".into(),
        };
        let text = draft.to_text();
        assert!(text.contains("version:\n  parents: ["));
        assert_eq!(Draft::from_text(&text).unwrap(), draft);
    }

    #[test]
    fn a_hand_edited_version_block_is_refused() {
        let text = "---\nslug: lantern\nkind: topic\nsummary: s\nversion:\n  parents: []\n  machine: m\n---\n";
        assert!(matches!(
            Draft::from_text(text),
            Err(VersionError::Field(FieldError::Unknown(_)))
        ));
    }

    #[test]
    fn merging_carries_markers_until_edited() {
        use super::super::machine::MachineName;
        use super::super::version::{Operation, VersionBlock};
        let head = |body: &str| {
            let mut fields = Fields::default();
            fields.push_scalar("summary", "s");
            Version::compose(
                Slug::parse("t").unwrap(),
                VersionBlock {
                    parents: vec![],
                    written: "2026-09-04T10:00:00+01:00".into(),
                    machine: MachineName::parse("m").unwrap(),
                    operation: Operation::Save,
                    superseded_by: None,
                },
                fields,
                body.to_owned(),
            )
        };
        let draft = Draft::merging(&[head("\none\n"), head("\ntwo\n")]);
        assert_eq!(draft.parents.len(), 2);
        assert!(draft.has_conflict_markers());
        assert!(draft.body.contains("\none\n=======") && draft.body.contains("\ntwo\n>>>>>>>"));
        let settled = Draft {
            body: "\nboth\n".into(),
            ..draft
        };
        assert!(!settled.has_conflict_markers());
    }
}
