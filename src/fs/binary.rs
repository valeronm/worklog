//! The executable this process runs from, replaced in place.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::domain::ports::{Binary, StoreError};

use super::io_error;

pub struct FsBinary {
    exe: PathBuf,
}

impl FsBinary {
    /// This process's own executable.
    pub fn running() -> Result<FsBinary, StoreError> {
        let exe = std::env::current_exe().map_err(|e| io_error(Path::new("this binary"), &e))?;
        Ok(FsBinary { exe })
    }
}

impl Binary for FsBinary {
    /// Written beside the binary with its permissions and renamed over it,
    /// so the swap is one step on one filesystem and the running process
    /// keeps its own inode.
    fn replace(&self, bytes: &[u8]) -> Result<String, StoreError> {
        let staged = self.exe.with_extension("new");
        std::fs::write(&staged, bytes).map_err(|e| io_error(&staged, &e))?;
        let permissions = std::fs::metadata(&self.exe)
            .map_err(|e| io_error(&self.exe, &e))?
            .permissions();
        std::fs::set_permissions(&staged, permissions).map_err(|e| io_error(&staged, &e))?;
        std::fs::rename(&staged, &self.exe).map_err(|e| io_error(&self.exe, &e))?;
        Ok(self.exe.display().to_string())
    }

    fn refresh(&self) -> Result<String, StoreError> {
        let output = Command::new(&self.exe)
            .args(["agents", "refresh"])
            .output()
            .map_err(|e| io_error(&self.exe, &e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(StoreError::io(
                format!("{} agents refresh", self.exe.display()),
                stderr.trim().trim_start_matches("worklog: "),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
