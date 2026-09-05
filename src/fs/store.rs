use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::ports::{Store, StoreError};
use crate::domain::slug::{Kind, Slug};
use crate::domain::version::{Document, Version, VersionId};

use super::{corrupt, io_error};

/// A version lands under this name before its rename; the folder's ignore
/// file names the same prefix so a sync never ships it.
const STAGING_PREFIX: &str = ".tmp-";

/// `<root>/<kind>/<slug>/<hash>.md`, one file per version, never rewritten.
pub struct FsStore {
    root: PathBuf,
}

/// The writer names a file `<version id>.md` and nothing else counts as a
/// version.
fn version_name(name: &str) -> Option<&str> {
    name.strip_suffix(".md")
        .filter(|hash| VersionId::parse(hash).is_ok())
}

impl FsStore {
    #[must_use]
    pub fn new(root: PathBuf) -> FsStore {
        FsStore { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A folder Syncthing manages carries its `.stfolder` marker, and its
    /// ignore file is per host and never synced, so a folder without one
    /// is given the patterns that keep Finder's metadata and a half-written
    /// version from leaving this machine. A file already there is the
    /// host's own and is left alone.
    fn keep_sync_clean(&self) -> Result<(), StoreError> {
        if !self.root.join(".stfolder").exists() {
            return Ok(());
        }
        let ignore = self.root.join(".stignore");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&ignore);
        match file {
            Ok(mut file) => {
                use std::io::Write as _;
                writeln!(file, ".DS_Store\n{STAGING_PREFIX}*").map_err(|e| io_error(&ignore, &e))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(io_error(&ignore, &e)),
        }
    }

    fn slug_dir(&self, slug: &Slug) -> PathBuf {
        self.root.join(slug.kind().dir()).join(slug.path())
    }
}

/// Directories under `dir` holding version files, as paths relative to
/// the kind directory.
fn walk(dir: &Path, relative: &str, kind: Kind, found: &mut Vec<Slug>) -> Result<(), StoreError> {
    let mut holds_versions = false;
    let mut subdirs = Vec::new();
    for entry in super::entries(dir)? {
        let entry = entry.map_err(|e| io_error(dir, &e))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let file_type = entry.file_type().map_err(|e| io_error(dir, &e))?;
        if file_type.is_dir() {
            if !name.starts_with('.') {
                subdirs.push(name.into_owned());
            }
        } else if version_name(&name).is_some() {
            holds_versions = true;
        }
    }
    if holds_versions {
        found.push(Slug::of_kind(kind, relative).map_err(|e| corrupt(dir, e))?);
    }
    subdirs.sort();
    for sub in subdirs {
        let relative = if relative.is_empty() {
            sub.clone()
        } else {
            format!("{relative}/{sub}")
        };
        walk(&dir.join(&sub), &relative, kind, found)?;
    }
    Ok(())
}

impl Store for FsStore {
    fn slugs(&self, kind: Kind) -> Result<Vec<Slug>, StoreError> {
        let mut found = Vec::new();
        walk(&self.root.join(kind.dir()), "", kind, &mut found)?;
        found.sort();
        Ok(found)
    }

    fn document(&self, slug: &Slug) -> Result<Document, StoreError> {
        let dir = self.slug_dir(slug);
        let mut versions = Vec::new();
        for entry in super::entries(&dir)? {
            let entry = entry.map_err(|e| io_error(&dir, &e))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(hash) = version_name(&name) else {
                continue;
            };
            let path = entry.path();
            let text = fs::read_to_string(&path).map_err(|e| io_error(&path, &e))?;
            let version = Version::from_named_text(hash, &text).map_err(|e| corrupt(&path, e))?;
            if version.slug != *slug {
                return Err(corrupt(
                    &path,
                    format!("names slug {} but sits under {slug}", version.slug),
                ));
            }
            versions.push(version);
        }
        versions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Document::new(versions))
    }

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let dir = self.slug_dir(&version.slug);
        let target = dir.join(format!("{}.md", version.id));
        if target.exists() {
            return Ok(());
        }
        self.keep_sync_clean()?;
        fs::create_dir_all(&dir).map_err(|e| io_error(&dir, &e))?;
        // Syncthing picks up a file as soon as it appears, so the bytes land
        // under a name it ignores and become the version in one rename.
        let staging = dir.join(format!("{STAGING_PREFIX}{}", version.id));
        fs::write(&staging, version.to_text()).map_err(|e| io_error(&staging, &e))?;
        fs::rename(&staging, &target).map_err(|e| io_error(&target, &e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::frontmatter::Fields;
    use crate::domain::machine::MachineName;
    use crate::domain::version::{Operation, VersionBlock};

    fn version(slug: &str, body: &str) -> Version {
        let mut fields = Fields::default();
        fields.push_scalar("summary", "s");
        Version::compose(
            Slug::parse(slug).unwrap(),
            VersionBlock {
                parents: vec![],
                written: "2026-09-04T10:00:00+01:00".into(),
                machine: MachineName::parse("m").unwrap(),
                operation: Operation::New,
                superseded_by: None,
                renamed_from: None,
                raw: None,
            },
            fields,
            body.to_owned(),
        )
    }

    #[test]
    fn round_trip_and_listing() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::new(dir.path().to_path_buf());
        let topic = version("lantern", "\nt\n");
        let entry = version("2026-09/2026-09-04-x", "\ne\n");
        store.put(&topic).unwrap();
        store.put(&entry).unwrap();
        store.put(&entry).unwrap();
        assert_eq!(store.slugs(Kind::Topic).unwrap(), vec![topic.slug.clone()]);
        assert_eq!(store.slugs(Kind::Entry).unwrap(), vec![entry.slug.clone()]);
        assert_eq!(store.slugs(Kind::Fact).unwrap(), vec![]);
        let doc = store.document(&entry.slug).unwrap();
        assert_eq!(doc.versions, vec![entry.clone()]);
        assert!(
            dir.path()
                .join("entry/2026-09/2026-09-04-x")
                .join(format!("{}.md", entry.id))
                .exists()
        );
        assert_eq!(
            store.document(&Slug::parse("absent").unwrap()).unwrap(),
            Document::default()
        );
    }

    #[test]
    fn a_damaged_file_is_reported_not_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::new(dir.path().to_path_buf());
        let topic = version("lantern", "\nt\n");
        store.put(&topic).unwrap();
        let path = dir
            .path()
            .join("topic/lantern")
            .join(format!("{}.md", topic.id));
        fs::write(&path, "---\nslug: lantern\n---\n").unwrap();
        assert!(matches!(
            store.document(&topic.slug),
            Err(StoreError::Corrupt { .. })
        ));
    }
}
