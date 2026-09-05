use super::frontmatter::{FieldError, Fields, checked_date};
use super::recheck::Recheck;

/// What is true now, or for an idea, a settled design not yet built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub tags: Vec<String>,
    pub idea: bool,
    pub recheck: Option<Recheck>,
    /// The day the fact was last confirmed, as opposed to last written.
    pub verified: Option<String>,
    pub summary: String,
}

pub const KEYS: [&str; 5] = ["tags", "idea", "recheck", "verified", "summary"];

impl Fact {
    pub fn from_fields(fields: &Fields) -> Result<Fact, FieldError> {
        let idea = match fields.optional("idea") {
            None => false,
            Some("true") => true,
            Some(other) => {
                return Err(FieldError::invalid(
                    "idea",
                    format!("`{other}` is not `true`"),
                ));
            }
        };
        let recheck = fields
            .optional("recheck")
            .map(Recheck::parse)
            .transpose()
            .map_err(|e| FieldError::invalid("recheck", e))?;
        let verified = fields
            .optional("verified")
            .map(|day| checked_date("verified", day))
            .transpose()?;
        Ok(Fact {
            tags: fields.list_or_empty("tags"),
            idea,
            recheck,
            verified,
            summary: fields.required("summary")?.to_owned(),
        })
    }

    #[must_use]
    pub fn to_fields(&self) -> Fields {
        let mut fields = Fields::default();
        fields.push_list("tags", &self.tags);
        if self.idea {
            fields.push_scalar("idea", "true");
        }
        if let Some(recheck) = &self.recheck {
            fields.push_scalar("recheck", &recheck.to_string());
        }
        if let Some(day) = &self.verified {
            fields.push_scalar("verified", day);
        }
        fields.push_scalar("summary", &self.summary);
        fields
    }

    #[must_use]
    pub fn verified_on(&self, today: &str) -> Fact {
        Fact {
            verified: Some(today.to_owned()),
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let fact = Fact {
            tags: vec!["lantern".into()],
            idea: true,
            recheck: Some(Recheck::parse("touching lantern").unwrap()),
            verified: Some("2026-09-04".into()),
            summary: "the thing".into(),
        };
        assert_eq!(Fact::from_fields(&fact.to_fields()).unwrap(), fact);
        let plain = Fact {
            idea: false,
            recheck: None,
            verified: None,
            ..fact
        };
        assert_eq!(plain.to_fields().iter().count(), 2);
        assert_eq!(Fact::from_fields(&plain.to_fields()).unwrap(), plain);
    }

    #[test]
    fn verify_changes_only_the_date() {
        let fact = Fact {
            tags: vec![],
            idea: false,
            recheck: None,
            verified: None,
            summary: "s".into(),
        };
        let checked = fact.verified_on("2026-09-05");
        assert_eq!(checked.verified.as_deref(), Some("2026-09-05"));
        assert_eq!(
            Fact {
                verified: None,
                ..checked
            },
            fact
        );
    }
}
