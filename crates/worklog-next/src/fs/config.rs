use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::domain::id::DocumentId;
use crate::domain::ports::StoreError;

use super::{io_error, optional};

const STORE: &str = "store";
const MACHINE: &str = "machine";

/// What binding a host records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub store: PathBuf,
    pub machine: DocumentId,
}

fn text_of<'t>(table: &'t Table, key: &str, path: &Path) -> Result<&'t str, StoreError> {
    match table.get(key) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(StoreError::io(
            path.display(),
            format!("`{key}` is not text"),
        )),
        None => Err(StoreError::io(path.display(), format!("no `{key}`"))),
    }
}

// A file renamed before its bytes reach the disk can be empty after a crash.
fn staged(staging: &Path, text: &str) -> std::io::Result<()> {
    let mut file = fs::File::create(staging)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

impl Config {
    /// `None` for no file; a file that is not a whole config is refused with its path.
    pub fn read(path: &Path) -> Result<Option<Config>, StoreError> {
        let Some(text) = optional(path, fs::read_to_string(path))? else {
            return Ok(None);
        };
        let wrong = |why: String| StoreError::io(path.display(), why);
        let table: Table = text
            .parse()
            .map_err(|e: toml::de::Error| wrong(format!("is not a config: {}", e.message())))?;
        let store = PathBuf::from(text_of(&table, STORE, path)?);
        if !store.is_absolute() {
            return Err(wrong(format!("`{STORE}` is not an absolute path")));
        }
        let machine = DocumentId::parse(text_of(&table, MACHINE, path)?)
            .map_err(|e| wrong(format!("`{MACHINE}`: {e}")))?;
        Ok(Some(Config { store, machine }))
    }

    /// Creates the directory and replaces the file whole.
    pub fn write(&self, path: &Path) -> Result<(), StoreError> {
        let Some(store) = self.store.to_str() else {
            return Err(StoreError::io(
                self.store.display(),
                "is not a path a config can hold",
            ));
        };
        let mut table = Table::new();
        table.insert(STORE.to_owned(), store.into());
        table.insert(MACHINE.to_owned(), self.machine.as_str().into());
        let text = toml::to_string(&table).map_err(|e| StoreError::io(path.display(), e))?;
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(StoreError::io(path.display(), "is not a file's path"));
        };
        fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;
        // A rename replaces the file whole only from the same file system.
        let staging = dir.join(format!(".tmp-{}", name.to_string_lossy()));
        let replaced = staged(&staging, &text)
            .map_err(|e| io_error(&staging, &e))
            .and_then(|()| fs::rename(&staging, path).map_err(|e| io_error(path, &e)));
        if replaced.is_err() {
            let _ = fs::remove_file(&staging);
        }
        replaced
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{LANTERN, lantern};
    use crate::fs::{directories, files};

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("worklog-next").join("config.toml");
        (dir, path)
    }

    fn config() -> Config {
        Config {
            store: PathBuf::from("/home/desk/store"),
            machine: lantern(),
        }
    }

    fn damaged(text: &str) -> String {
        let (_dir, path) = scratch();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        let refused = Config::read(&path).unwrap_err().to_string();
        assert!(
            refused.starts_with(&path.display().to_string()),
            "{refused}"
        );
        refused
    }

    #[test]
    fn a_config_reads_back_as_written_and_a_missing_one_is_none() {
        let (_dir, path) = scratch();
        assert_eq!(Config::read(&path), Ok(None));
        config().write(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("store = \"/home/desk/store\"\nmachine = \"{LANTERN}\"\n")
        );
        assert_eq!(Config::read(&path), Ok(Some(config())));
    }

    #[test]
    fn a_config_written_over_another_leaves_one_file() {
        let (_dir, path) = scratch();
        config().write(&path).unwrap();
        let moved = Config {
            store: PathBuf::from("/home/desk/atlas"),
            machine: lantern(),
        };
        moved.write(&path).unwrap();
        assert_eq!(Config::read(&path), Ok(Some(moved)));
        assert_eq!(
            files(path.parent().unwrap()).unwrap(),
            ["config.toml".to_owned()]
        );
    }

    #[test]
    fn a_config_that_is_not_toml_is_refused_with_its_path() {
        damaged("store: /home/desk/store\n");
    }

    #[test]
    fn a_config_lacking_a_key_is_refused_with_the_key() {
        let refused = damaged(&format!("machine = \"{LANTERN}\"\n"));
        assert!(refused.contains("`store`"), "{refused}");
        let refused = damaged("store = \"/home/desk/store\"\n");
        assert!(refused.contains("`machine`"), "{refused}");
    }

    #[test]
    fn a_config_whose_values_are_no_path_and_no_id_is_refused() {
        let refused = damaged(&format!("store = 4\nmachine = \"{LANTERN}\"\n"));
        assert!(refused.contains("`store`"), "{refused}");
        let refused = damaged(&format!("store = \"store\"\nmachine = \"{LANTERN}\"\n"));
        assert!(refused.contains("`store`"), "{refused}");
        let refused = damaged("store = \"/home/desk/store\"\nmachine = \"lantern\"\n");
        assert!(refused.contains("`machine`"), "{refused}");
    }

    #[test]
    fn a_key_this_reader_does_not_know_is_passed_over() {
        let (_dir, path) = scratch();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = format!("store = \"/home/desk/store\"\nmachine = \"{LANTERN}\"\nphone = 1\n");
        fs::write(&path, text).unwrap();
        assert_eq!(Config::read(&path), Ok(Some(config())));
    }

    #[test]
    fn a_write_that_fails_leaves_nothing_beside_the_config() {
        let (_dir, path) = scratch();
        fs::create_dir_all(&path).unwrap();
        let refused = config().write(&path).unwrap_err().to_string();
        assert!(
            refused.starts_with(&path.display().to_string()),
            "{refused}"
        );
        let dir = path.parent().unwrap();
        assert!(files(dir).unwrap().is_empty());
        assert_eq!(directories(dir).unwrap(), ["config.toml".to_owned()]);
    }

    #[test]
    fn a_config_under_what_is_not_a_directory_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("worklog-next"), "a file").unwrap();
        let path = dir.path().join("worklog-next").join("config.toml");
        assert!(config().write(&path).is_err());
        assert!(Config::read(&path).is_err());
    }
}
