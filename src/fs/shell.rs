//! A shell on this host that takes completions from the binary.

use std::path::PathBuf;

use crate::domain::ports::StoreError;

use super::write_file;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shell {
    pub kind: clap_complete::Shell,
    /// Where the shell reads completions, whether or not it is on this
    /// host.
    dir: PathBuf,
    file: PathBuf,
}

impl Shell {
    pub(super) fn new(kind: clap_complete::Shell, dir: PathBuf, file: &str) -> Shell {
        Shell {
            kind,
            file: dir.join(file),
            dir,
        }
    }

    /// Whether the shell is set up on this host, by its completions
    /// directory, so that a refresh never creates one.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.dir.is_dir()
    }

    /// Writes the completions and returns their file.
    pub fn write_completions(&self, text: &str) -> Result<PathBuf, StoreError> {
        write_file(&self.file, text)?;
        Ok(self.file.clone())
    }
}
