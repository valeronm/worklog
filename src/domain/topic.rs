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
    /// Topic to the directories claimed for it on this host.
    pub claims: Vec<(String, Vec<String>)>,
    /// Topic to the path prefixes claimed for it, matching every directory
    /// sharing the prefix.
    pub families: Vec<(String, Vec<String>)>,
    /// Topics loaded only when no claim matches the directory.
    pub unclaimed: Vec<String>,
}

pub const KEYS: [&str; 6] = [
    "summary",
    "includes",
    "machine",
    "claims",
    "families",
    "unclaimed",
];

fn map_of_lists(fields: &Fields, key: &str) -> Result<Vec<(String, Vec<String>)>, FieldError> {
    match fields.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Scalar(s)) if s.is_empty() => Ok(Vec::new()),
        Some(Value::Map(entries)) => entries
            .iter()
            .map(|(topic, value)| match value {
                Value::List(paths) => Ok((topic.clone(), paths.clone())),
                _ => Err(FieldError::Invalid {
                    key: key.to_owned(),
                    reason: format!("`{topic}` must hold a list"),
                }),
            })
            .collect(),
        Some(_) => Err(FieldError::Invalid {
            key: key.to_owned(),
            reason: "must be a map of lists".into(),
        }),
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
            families: map_of_lists(fields, "families")?,
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
        for (key, map) in [("claims", &self.claims), ("families", &self.families)] {
            if !map.is_empty() {
                fields.push(
                    key,
                    Value::Map(
                        map.iter()
                            .map(|(topic, paths)| (topic.clone(), Value::List(paths.clone())))
                            .collect(),
                    ),
                );
            }
        }
        if !self.unclaimed.is_empty() {
            fields.push_list("unclaimed", &self.unclaimed);
        }
        fields
    }

    /// Every topic name this document refers to.
    pub fn references(&self) -> impl Iterator<Item = &str> {
        self.includes
            .iter()
            .chain(self.unclaimed.iter())
            .chain(self.claims.iter().map(|(t, _)| t))
            .chain(self.families.iter().map(|(t, _)| t))
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
            families: vec![("laptop".into(), vec!["~/projects/lab-".into()])],
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
}
