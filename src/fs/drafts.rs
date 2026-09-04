use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::draft::Draft;
use crate::domain::ports::{Drafts, StoreError};
use crate::domain::slug::{Kind, Slug};

use super::{corrupt, io_error, write_file};

/// `<root>/<kind>/<slug>.md`, one editable file per document being
/// written, with the versions it came from in `<slug>.parents` beside it.
pub struct FsDrafts {
    root: PathBuf,
}

impl FsDrafts {
    #[must_use]
    pub fn new(root: PathBuf) -> FsDrafts {
        FsDrafts { root }
    }

    fn path(&self, slug: &Slug) -> PathBuf {
        self.root
            .join(slug.kind().dir())
            .join(format!("{}.md", slug.path()))
    }

    fn parents_path(&self, slug: &Slug) -> PathBuf {
        self.path(slug).with_extension("parents")
    }

    /// The slug a draft file stands for, from where it sits under the root.
    fn slug_of(&self, path: &Path) -> Result<Slug, StoreError> {
        let relative = path
            .strip_prefix(&self.root)
            .ok()
            .and_then(|p| p.to_str())
            .and_then(|p| p.strip_suffix(".md"))
            .ok_or_else(|| corrupt(path, "not a draft path"))?;
        let (kind, rest) = relative
            .split_once('/')
            .ok_or_else(|| corrupt(path, "not a draft path"))?;
        let kind = Kind::from_dir(kind).ok_or_else(|| corrupt(path, "not a kind"))?;
        Slug::of_kind(kind, rest).map_err(|e| corrupt(path, e))
    }

    fn read_path(&self, path: &Path, slug: Slug) -> Result<Draft, StoreError> {
        let parents_path = self.parents_path(&slug);
        let parents = fs::read_to_string(&parents_path)
            .map_err(|e| io_error(&parents_path, &e))
            .and_then(|t| Draft::parse_parents(&t).map_err(|e| corrupt(&parents_path, e)))?;
        let text = fs::read_to_string(path).map_err(|e| io_error(path, &e))?;
        Draft::from_text(slug, parents, &text).map_err(|e| corrupt(path, e))
    }

    fn walk(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), StoreError> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_error(dir, &e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| io_error(dir, &e))?;
            let path = entry.path();
            if path.is_dir() {
                FsDrafts::walk(&path, found)?;
            } else if path.extension().is_some_and(|ext| ext == "md") {
                found.push(path);
            }
        }
        Ok(())
    }

    fn remove(path: &Path) -> Result<(), StoreError> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error(path, &e)),
        }
    }
}

impl Drafts for FsDrafts {
    fn read(&self, slug: &Slug) -> Result<Option<Draft>, StoreError> {
        let path = self.path(slug);
        if !path.is_file() {
            return Ok(None);
        }
        self.read_path(&path, slug.clone()).map(Some)
    }

    /// The parents land first, so a draft on disk always has them beside it.
    fn write(&self, draft: &Draft) -> Result<String, StoreError> {
        write_file(&self.parents_path(&draft.slug), &draft.parents_text())?;
        let path = self.path(&draft.slug);
        write_file(&path, &draft.to_text())?;
        Ok(path.display().to_string())
    }

    fn delete(&self, slug: &Slug) -> Result<(), StoreError> {
        FsDrafts::remove(&self.path(slug))?;
        FsDrafts::remove(&self.parents_path(slug))
    }

    fn list(&self) -> Result<Vec<Draft>, StoreError> {
        let mut paths = Vec::new();
        FsDrafts::walk(&self.root, &mut paths)?;
        paths.sort();
        paths
            .iter()
            .map(|p| self.read_path(p, self.slug_of(p)?))
            .collect()
    }

    fn location(&self, slug: &Slug) -> String {
        self.path(slug).display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::frontmatter::Fields;
    use crate::domain::version::VersionId;

    #[test]
    fn write_read_list_delete() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = FsDrafts::new(dir.path().to_path_buf());
        let mut fields = Fields::default();
        fields.push_scalar("summary", "s");
        let draft = Draft {
            slug: Slug::parse("2026-09/2026-09-04-x").unwrap(),
            parents: vec![VersionId::of("a")],
            fields,
            body: "\nb\n".into(),
        };
        let location = drafts.write(&draft).unwrap();
        assert!(location.ends_with("/entry/2026-09/2026-09-04-x.md"));
        assert_eq!(drafts.location(&draft.slug), location);
        let text = fs::read_to_string(&location).unwrap();
        assert_eq!(text, "---\nsummary: s\n---\n\nb\n");
        assert_eq!(drafts.read(&draft.slug).unwrap(), Some(draft.clone()));
        assert_eq!(drafts.list().unwrap(), vec![draft.clone()]);
        drafts.delete(&draft.slug).unwrap();
        drafts.delete(&draft.slug).unwrap();
        assert_eq!(drafts.read(&draft.slug).unwrap(), None);
        assert_eq!(drafts.list().unwrap(), vec![]);
        assert!(!drafts.parents_path(&draft.slug).exists());
    }

    #[test]
    fn a_draft_without_its_parents_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = FsDrafts::new(dir.path().to_path_buf());
        let slug = Slug::parse("lantern").unwrap();
        write_file(&drafts.path(&slug), "---\nsummary: s\n---\n").unwrap();
        assert!(drafts.read(&slug).is_err());
        assert!(drafts.list().is_err());
    }
}
