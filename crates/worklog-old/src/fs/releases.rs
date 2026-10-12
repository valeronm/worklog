//! Releases published as a directory: a `latest` file naming the tag and
//! the assets beside it.

use std::path::PathBuf;

use crate::domain::ports::{Releases, StoreError};

use super::io_error;

pub struct DirReleases {
    pub dir: PathBuf,
}

impl Releases for DirReleases {
    fn latest(&self) -> Result<String, StoreError> {
        let file = self.dir.join("latest");
        let text = std::fs::read_to_string(&file).map_err(|e| io_error(&file, &e))?;
        Ok(text.trim().to_owned())
    }

    fn fetch(&self, _tag: &str, asset: &str) -> Result<Vec<u8>, StoreError> {
        let file = self.dir.join(asset);
        std::fs::read(&file).map_err(|e| io_error(&file, &e))
    }
}
