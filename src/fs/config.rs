use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::frontmatter::{self, Fields};
use crate::domain::machine::MachineName;
use crate::domain::ports::{Identity, StoreError};

use super::{corrupt, io_error};

/// What `init` records for a host: its name and where its store is.
///
/// ```text
/// machine: desk
/// store: /home/u/worklog
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub machine: MachineName,
    pub store: PathBuf,
}

impl Config {
    pub fn read(path: &Path) -> Result<Option<Config>, StoreError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_error(path, &e)),
        };
        let fields = frontmatter::parse_fields(&text).map_err(|e| corrupt(path, e))?;
        fields
            .reject_unknown(&["machine", "store"])
            .map_err(|e| corrupt(path, e))?;
        let machine = fields.required("machine").map_err(|e| corrupt(path, e))?;
        let machine = MachineName::parse(machine).map_err(|e| corrupt(path, e))?;
        let store = fields.required("store").map_err(|e| corrupt(path, e))?;
        Ok(Some(Config {
            machine,
            store: PathBuf::from(store),
        }))
    }

    pub fn write(&self, path: &Path) -> Result<(), StoreError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
        }
        let mut fields = Fields::default();
        fields.push_scalar("machine", self.machine.as_str());
        fields.push_scalar("store", &self.store.display().to_string());
        fs::write(path, frontmatter::emit_fields(&fields)).map_err(|e| io_error(path, &e))
    }
}

/// The machine name as the config file records it.
pub struct FileIdentity {
    path: PathBuf,
}

impl FileIdentity {
    #[must_use]
    pub fn new(path: PathBuf) -> FileIdentity {
        FileIdentity { path }
    }
}

impl Identity for FileIdentity {
    fn machine(&self) -> Result<Option<MachineName>, StoreError> {
        Ok(Config::read(&self.path)?.map(|c| c.machine))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_absence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/worklog/config");
        assert_eq!(Config::read(&path).unwrap(), None);
        let config = Config {
            machine: MachineName::parse("desk").unwrap(),
            store: PathBuf::from("/home/u/worklog"),
        };
        config.write(&path).unwrap();
        assert_eq!(Config::read(&path).unwrap(), Some(config.clone()));
        assert_eq!(
            FileIdentity::new(path.clone()).machine().unwrap(),
            Some(config.machine)
        );
        fs::write(&path, "machine: m\n").unwrap();
        assert!(matches!(
            Config::read(&path),
            Err(StoreError::Corrupt { .. })
        ));
    }
}
