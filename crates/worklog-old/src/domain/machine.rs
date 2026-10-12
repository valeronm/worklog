use std::fmt;

/// The configured name of the machine writing a version, never a hostname.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MachineName(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineNameError(pub String);

impl fmt::Display for MachineNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "machine name `{}` may hold only letters, digits, `.`, `_` and `-`",
            self.0
        )
    }
}

impl MachineName {
    pub fn parse(text: &str) -> Result<MachineName, MachineNameError> {
        let ok = !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if ok {
            Ok(MachineName(text.to_owned()))
        } else {
            Err(MachineNameError(text.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MachineName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
