use super::frontmatter::{FieldError, Fields, checked_date};
use super::machine::MachineName;

/// What happened on a day. Written once; its follow-ups are documents of
/// their own that name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub date: String,
    /// Where the work happened, which is not where the version was written.
    pub machine: MachineName,
    pub tags: Vec<String>,
    pub files_touched: Vec<String>,
    pub summary: String,
}

pub const KEYS: [&str; 5] = ["date", "machine", "tags", "files_touched", "summary"];

impl Entry {
    pub fn from_fields(fields: &Fields) -> Result<Entry, FieldError> {
        fields.reject_unknown(&KEYS)?;
        let date = checked_date("date", fields.required("date")?)?;
        let machine = MachineName::parse(fields.required("machine")?)
            .map_err(|e| FieldError::invalid("machine", e))?;
        Ok(Entry {
            date,
            machine,
            tags: fields.list_or_empty("tags"),
            files_touched: fields.list_or_empty("files_touched"),
            summary: fields.required("summary")?.to_owned(),
        })
    }

    #[must_use]
    pub fn to_fields(&self) -> Fields {
        let mut fields = Fields::default();
        fields.push_scalar("date", &self.date);
        fields.push_scalar("machine", self.machine.as_str());
        fields.push_list("tags", &self.tags);
        fields.push_list("files_touched", &self.files_touched);
        fields.push_scalar("summary", &self.summary);
        fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let entry = Entry {
            date: "2026-09-04".into(),
            machine: MachineName::parse("m").unwrap(),
            tags: vec!["a".into()],
            files_touched: vec![],
            summary: "did a thing".into(),
        };
        assert_eq!(Entry::from_fields(&entry.to_fields()).unwrap(), entry);
    }

    #[test]
    fn unknown_and_missing() {
        let mut fields = Fields::default();
        fields.push_scalar("date", "2026-09-04");
        assert!(matches!(
            Entry::from_fields(&fields),
            Err(FieldError::Missing("machine"))
        ));
        fields.push_scalar("scope", "machine");
        assert_eq!(
            Entry::from_fields(&fields),
            Err(FieldError::Unknown("scope".into()))
        );
    }
}
