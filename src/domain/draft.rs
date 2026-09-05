use super::frontmatter::{self, Fields, ParseError};
use super::slug::Slug;
use super::version::{Version, VersionError, VersionId};

/// A document being written: the kind's own fields and the body, plus the
/// versions it was checked out from. Everything else a version carries is
/// stamped by `save`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub slug: Slug,
    pub parents: Vec<VersionId>,
    pub fields: Fields,
    pub body: String,
}

const CONFLICT_OPEN: &str = "<<<<<<<";
const CONFLICT_SEPARATOR: &str = "=======";
const CONFLICT_CLOSE: &str = ">>>>>>>";

impl Draft {
    /// The file a person edits: the kind's own fields and the body, the
    /// same text `show` prints, so nothing in it belongs to the tool.
    #[must_use]
    pub fn to_text(&self) -> String {
        frontmatter::emit(&self.fields, &self.body)
    }

    /// Reads the edited file back; the slug and the parents are kept beside
    /// it by whoever stores drafts, never in the text.
    pub fn from_text(slug: Slug, parents: Vec<VersionId>, text: &str) -> Result<Draft, ParseError> {
        let split = frontmatter::parse(text)?;
        Ok(Draft {
            slug,
            parents,
            fields: split.fields,
            body: split.body,
        })
    }

    /// The parents as stored beside the draft: one id per line.
    #[must_use]
    pub fn parents_text(&self) -> String {
        self.parents.iter().fold(String::new(), |mut out, p| {
            out.push_str(&p.to_string());
            out.push('\n');
            out
        })
    }

    pub fn parse_parents(text: &str) -> Result<Vec<VersionId>, VersionError> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| VersionId::parse(l.trim()))
            .collect()
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

#[cfg(test)]
mod tests {
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
        assert_eq!(text, "---\nsummary: s\n---\n\nbody\n");
        let parents = Draft::parse_parents(&draft.parents_text()).unwrap();
        assert_eq!(
            Draft::from_text(draft.slug.clone(), parents, &text).unwrap(),
            draft
        );
    }

    #[test]
    fn a_parents_file_holds_only_ids() {
        assert_eq!(Draft::parse_parents("\n").unwrap(), vec![]);
        assert!(Draft::parse_parents("not-an-id\n").is_err());
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
                    renamed_from: None,
                    raw: None,
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
