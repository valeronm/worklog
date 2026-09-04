use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::draft::Draft;
use crate::domain::ports::{Drafts, StoreError};
use crate::domain::slug::Slug;

use super::{corrupt, io_error};

/// `<root>/<kind>/<slug>.md`, one editable file per document being written.
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

    fn read_path(path: &Path) -> Result<Draft, StoreError> {
        let text = fs::read_to_string(path).map_err(|e| io_error(path, &e))?;
        Draft::from_text(&text).map_err(|e| corrupt(path, e))
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
}

impl Drafts for FsDrafts {
    fn read(&self, slug: &Slug) -> Result<Option<Draft>, StoreError> {
        let path = self.path(slug);
        match fs::read_to_string(&path) {
            Ok(text) => Draft::from_text(&text)
                .map(Some)
                .map_err(|e| corrupt(&path, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(&path, &e)),
        }
    }

    fn write(&self, draft: &Draft) -> Result<String, StoreError> {
        let path = self.path(&draft.slug);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, &e))?;
        }
        fs::write(&path, draft.to_text()).map_err(|e| io_error(&path, &e))?;
        Ok(path.display().to_string())
    }

    fn delete(&self, slug: &Slug) -> Result<(), StoreError> {
        let path = self.path(slug);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error(&path, &e)),
        }
    }

    fn list(&self) -> Result<Vec<Draft>, StoreError> {
        let mut paths = Vec::new();
        FsDrafts::walk(&self.root, &mut paths)?;
        paths.sort();
        paths.iter().map(|p| FsDrafts::read_path(p)).collect()
    }

    fn location(&self, slug: &Slug) -> String {
        self.path(slug).display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::frontmatter::Fields;

    #[test]
    fn write_read_list_delete() {
        let dir = tempfile::tempdir().unwrap();
        let drafts = FsDrafts::new(dir.path().to_path_buf());
        let mut fields = Fields::default();
        fields.push_scalar("summary", "s");
        let draft = Draft {
            slug: Slug::parse("lantern/x").unwrap(),
            parents: vec![],
            fields,
            body: "\nb\n".into(),
        };
        let location = drafts.write(&draft).unwrap();
        assert!(location.ends_with("/fact/lantern/x.md"));
        assert_eq!(drafts.location(&draft.slug), location);
        assert_eq!(drafts.read(&draft.slug).unwrap(), Some(draft.clone()));
        assert_eq!(drafts.list().unwrap(), vec![draft.clone()]);
        drafts.delete(&draft.slug).unwrap();
        drafts.delete(&draft.slug).unwrap();
        assert_eq!(drafts.read(&draft.slug).unwrap(), None);
        assert_eq!(drafts.list().unwrap(), vec![]);
    }
}
