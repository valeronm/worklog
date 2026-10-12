use std::path::PathBuf;

use crate::domain::ports::{Releases, StoreError};

use super::io_error;

/// One release published as a directory: a `latest` file holding the tag, and the assets
/// beside it.
pub struct DirReleases {
    dir: PathBuf,
}

impl DirReleases {
    pub fn new(dir: PathBuf) -> DirReleases {
        DirReleases { dir }
    }
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_directory_of_releases_gives_the_tag_it_holds_and_each_asset_whole() {
        let kept = tempfile::tempdir().unwrap();
        let releases = DirReleases::new(kept.path().to_path_buf());
        let missing = releases.latest().unwrap_err().to_string();
        let latest = kept.path().join("latest");
        assert!(
            missing.starts_with(&latest.display().to_string()),
            "{missing}"
        );

        fs::write(&latest, "v0.3.0\n").unwrap();
        fs::write(kept.path().join("lantern"), [0, 159, 10]).unwrap();
        assert_eq!(releases.latest(), Ok("v0.3.0".to_owned()));
        assert_eq!(releases.fetch("v0.3.0", "lantern"), Ok(vec![0, 159, 10]));
        let missing = releases.fetch("v0.3.0", "atlas").unwrap_err().to_string();
        let asset = kept.path().join("atlas");
        assert!(
            missing.starts_with(&asset.display().to_string()),
            "{missing}"
        );
    }
}
