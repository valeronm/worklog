use std::fmt;

use super::frontmatter::{FieldError, Fields, Value};
use super::machine::MachineName;

/// What a project, device, machine or subject is, and which other topics a
/// session about it also needs. A machine topic additionally says where the
/// topics live on that host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Topic {
    pub summary: String,
    pub includes: Vec<String>,
    /// Set on a machine topic; the host whose sessions always load it.
    pub machine: Option<MachineName>,
    /// Topic to the directories claimed for it on this host; a claim
    /// covers the directory and everything under it.
    pub claims: Vec<(String, Vec<String>)>,
    /// Topics loaded only when no claim matches the directory.
    pub unclaimed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaimError {
    Already { topic: String, path: String },
    Missing { topic: String, path: String },
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaimError::Already { topic, path } => write!(f, "already claims {path} for {topic}"),
            ClaimError::Missing { topic, path } => write!(f, "does not claim {path} for {topic}"),
        }
    }
}

pub const KEYS: [&str; 5] = ["summary", "includes", "machine", "claims", "unclaimed"];

fn map_of_lists(fields: &Fields, key: &str) -> Result<Vec<(String, Vec<String>)>, FieldError> {
    match fields.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Scalar(s)) if s.is_empty() => Ok(Vec::new()),
        Some(Value::Map(entries)) => entries
            .iter()
            .map(|(topic, value)| match value {
                Value::List(paths) => Ok((topic.clone(), paths.clone())),
                _ => Err(FieldError::invalid(
                    key,
                    format!("`{topic}` must hold a list"),
                )),
            })
            .collect(),
        Some(_) => Err(FieldError::invalid(key, "must be a map of lists")),
    }
}

impl Topic {
    pub fn from_fields(fields: &Fields) -> Result<Topic, FieldError> {
        fields.reject_unknown(&KEYS)?;
        let machine = fields
            .optional("machine")
            .map(MachineName::parse)
            .transpose()
            .map_err(|e| FieldError::invalid("machine", e))?;
        Ok(Topic {
            summary: fields.required("summary")?.to_owned(),
            includes: fields.list_or_empty("includes"),
            machine,
            claims: map_of_lists(fields, "claims")?,
            unclaimed: fields.list_or_empty("unclaimed"),
        })
    }

    #[must_use]
    pub fn to_fields(&self) -> Fields {
        let mut fields = Fields::default();
        fields.push_scalar("summary", &self.summary);
        if !self.includes.is_empty() {
            fields.push_list("includes", &self.includes);
        }
        if let Some(machine) = &self.machine {
            fields.push_scalar("machine", machine.as_str());
        }
        if !self.claims.is_empty() {
            fields.push(
                "claims",
                Value::Map(
                    self.claims
                        .iter()
                        .map(|(topic, paths)| (topic.clone(), Value::List(paths.clone())))
                        .collect(),
                ),
            );
        }
        if !self.unclaimed.is_empty() {
            fields.push_list("unclaimed", &self.unclaimed);
        }
        fields
    }

    /// Claims `path` for `topic` on this machine.
    pub fn claim(&mut self, topic: &str, path: &str) -> Result<(), ClaimError> {
        match self.claims.iter_mut().find(|(t, _)| t == topic) {
            Some((_, paths)) if paths.iter().any(|p| p == path) => Err(ClaimError::Already {
                topic: topic.to_owned(),
                path: path.to_owned(),
            }),
            Some((_, paths)) => {
                paths.push(path.to_owned());
                Ok(())
            }
            None => {
                self.claims.push((topic.to_owned(), vec![path.to_owned()]));
                Ok(())
            }
        }
    }

    /// Takes that claim back.
    pub fn unclaim(&mut self, topic: &str, path: &str) -> Result<(), ClaimError> {
        let missing = || ClaimError::Missing {
            topic: topic.to_owned(),
            path: path.to_owned(),
        };
        let (i, (_, paths)) = self
            .claims
            .iter_mut()
            .enumerate()
            .find(|(_, (t, _))| t == topic)
            .ok_or_else(missing)?;
        let at = paths.iter().position(|p| p == path).ok_or_else(missing)?;
        paths.remove(at);
        if paths.is_empty() {
            self.claims.remove(i);
        }
        Ok(())
    }

    /// Every topic name this document refers to.
    pub fn references(&self) -> impl Iterator<Item = &str> {
        self.includes
            .iter()
            .chain(self.unclaimed.iter())
            .chain(self.claims.iter().map(|(t, _)| t))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let topic = Topic {
            summary: "the primary machine".into(),
            includes: vec!["personal".into()],
            machine: Some(MachineName::parse("desk").unwrap()),
            claims: vec![(
                "lantern".into(),
                vec!["~/projects/lantern".into(), "~/projects/firmware".into()],
            )],
            unclaimed: vec!["personal".into()],
        };
        assert_eq!(Topic::from_fields(&topic.to_fields()).unwrap(), topic);
        let plain = Topic {
            summary: "an app".into(),
            ..Topic::default()
        };
        assert_eq!(plain.to_fields().iter().count(), 1);
        assert_eq!(Topic::from_fields(&plain.to_fields()).unwrap(), plain);
    }

    #[test]
    fn claims_come_and_go() {
        let mut machine = Topic::default();
        machine.claim("lantern", "~/projects/lantern").unwrap();
        assert_eq!(
            machine.claims,
            [("lantern".to_owned(), vec!["~/projects/lantern".to_owned()])]
        );
        let again = machine.claim("lantern", "~/projects/lantern");
        assert_eq!(
            again.unwrap_err().to_string(),
            "already claims ~/projects/lantern for lantern"
        );
        machine.claim("lantern", "~/src/lantern").unwrap();
        assert_eq!(machine.claims[0].1.len(), 2);
        let gone = machine.unclaim("lantern", "~/elsewhere");
        assert_eq!(
            gone.unwrap_err().to_string(),
            "does not claim ~/elsewhere for lantern"
        );
        machine.unclaim("lantern", "~/src/lantern").unwrap();
        machine.unclaim("lantern", "~/projects/lantern").unwrap();
        assert!(machine.claims.is_empty());
    }
}
