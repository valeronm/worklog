use std::fs;
use std::path::PathBuf;

use crate::domain::draft::Draft;
use crate::domain::id::DocumentId;
use crate::domain::ports::{Drafts, StoreError};

use super::{files, io_error, optional};

pub struct FsDrafts {
    dir: PathBuf,
}

const TEXT: &str = ".md";
const ORIGIN: &str = ".origin.toml";

impl FsDrafts {
    #[must_use]
    pub fn new(dir: PathBuf) -> FsDrafts {
        FsDrafts { dir }
    }

    fn text(&self, document: &DocumentId) -> PathBuf {
        self.dir.join(format!("{document}{TEXT}"))
    }

    fn origin(&self, document: &DocumentId) -> PathBuf {
        self.dir.join(format!("{document}{ORIGIN}"))
    }
}

impl Drafts for FsDrafts {
    fn read(&self, document: &DocumentId) -> Result<Option<Draft>, StoreError> {
        let path = self.text(document);
        let Some(text) = optional(&path, fs::read_to_string(&path))? else {
            return Ok(None);
        };
        let origin_path = self.origin(document);
        let origin =
            optional(&origin_path, fs::read_to_string(&origin_path))?.ok_or_else(|| {
                StoreError::io(
                    origin_path.display(),
                    "missing, so what the draft started from is unknown",
                )
            })?;
        Draft::read(document.clone(), &origin, &text)
            .map(Some)
            .map_err(|e| StoreError::io(path.display(), e))
    }

    fn write(&self, draft: &Draft) -> Result<String, StoreError> {
        let path = self.text(&draft.document);
        let text = draft
            .text()
            .map_err(|e| StoreError::io(path.display(), e))?;
        fs::create_dir_all(&self.dir).map_err(|e| io_error(&self.dir, &e))?;
        // A text without an origin is refused, where an origin without a
        // text reads as no draft.
        let origin = self.origin(&draft.document);
        fs::write(&origin, draft.origin()).map_err(|e| io_error(&origin, &e))?;
        fs::write(&path, text).map_err(|e| io_error(&path, &e))?;
        Ok(self.location(&draft.document))
    }

    fn delete(&self, document: &DocumentId) -> Result<(), StoreError> {
        for path in [self.text(document), self.origin(document)] {
            optional(&path, fs::remove_file(&path))?;
        }
        Ok(())
    }

    fn list(&self) -> Result<Vec<Draft>, StoreError> {
        let mut drafts = Vec::new();
        for name in files(&self.dir)? {
            let Some(document) = name
                .strip_suffix(TEXT)
                .and_then(|stem| DocumentId::parse(stem).ok())
            else {
                continue;
            };
            drafts.extend(self.read(&document)?);
        }
        Ok(drafts)
    }

    fn location(&self, document: &DocumentId) -> String {
        self.text(document).display().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{LANTERN, drafts_contract, first, lantern};
    use crate::fs::RandomIds;

    fn scratch() -> (tempfile::TempDir, FsDrafts) {
        let dir = tempfile::tempdir().unwrap();
        let drafts = FsDrafts::new(dir.path().join("drafts"));
        (dir, drafts)
    }

    fn draft() -> Draft {
        Draft::of(&first(&lantern(), "fact", "name = \"relay\"", "first\n"))
    }

    #[test]
    fn the_directory_drafts_keep_the_contract() {
        let (_dir, drafts) = scratch();
        drafts_contract(&drafts, &RandomIds);
    }

    #[test]
    fn a_draft_is_a_text_to_edit_and_an_origin_beside_it() {
        let (dir, drafts) = scratch();
        let draft = draft();
        let location = drafts.write(&draft).unwrap();
        let text = dir.path().join("drafts").join(format!("{LANTERN}.md"));
        assert_eq!(location, text.display().to_string());
        assert_eq!(fs::read_to_string(&text).unwrap(), draft.text().unwrap());
        let origin = dir
            .path()
            .join("drafts")
            .join(format!("{LANTERN}.origin.toml"));
        assert_eq!(fs::read_to_string(origin).unwrap(), draft.origin());

        fs::write(&text, "+++\nname = \"relay-pin\"\n+++\nsecond\n").unwrap();
        let edited = drafts.read(&draft.document).unwrap().unwrap();
        assert_eq!(edited.body, "second\n");
        assert_eq!(edited.parents, draft.parents);
    }

    #[test]
    fn a_text_without_its_origin_is_refused_and_an_origin_alone_is_no_draft() {
        let (_dir, drafts) = scratch();
        let draft = draft();
        drafts.write(&draft).unwrap();
        let origin = drafts.origin(&draft.document);
        let kept = fs::read_to_string(&origin).unwrap();
        fs::remove_file(&origin).unwrap();
        let refused = drafts.read(&draft.document).unwrap_err();
        assert!(refused.to_string().contains("origin.toml"), "{refused}");
        assert!(drafts.list().is_err());

        fs::write(&origin, kept).unwrap();
        fs::remove_file(drafts.text(&draft.document)).unwrap();
        assert_eq!(drafts.read(&draft.document).unwrap(), None);
        assert!(drafts.list().unwrap().is_empty());
    }

    #[test]
    fn a_text_a_session_broke_is_refused_with_where_it_is() {
        let (_dir, drafts) = scratch();
        let draft = draft();
        drafts.write(&draft).unwrap();
        fs::write(drafts.text(&draft.document), "name: relay\n").unwrap();
        let refused = drafts.read(&draft.document).unwrap_err().to_string();
        assert!(refused.contains(&format!("{LANTERN}.md")), "{refused}");
        assert!(refused.contains("no opening fence"), "{refused}");
    }

    #[test]
    fn a_draft_with_no_text_writes_nothing() {
        let (dir, drafts) = scratch();
        let mut draft = draft();
        draft
            .fields
            .insert("note".into(), "before\n+++\nafter".into());
        let refused = drafts.write(&draft).unwrap_err().to_string();
        assert!(refused.contains("+++"), "{refused}");
        assert!(!dir.path().join("drafts").exists());
    }
}
