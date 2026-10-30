use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::ports::StoreError;

use super::{io_error, optional, replace};

pub const SKILL_DIRECTORY: &str = "worklog";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsAgent {
    name: &'static str,
    home: PathBuf,
    skill_dir: PathBuf,
    hooks: PathBuf,
}

impl FsAgent {
    pub fn new(name: &'static str, home: PathBuf, hooks: &str) -> FsAgent {
        FsAgent {
            name,
            skill_dir: home.join("skills").join(SKILL_DIRECTORY),
            hooks: home.join(hooks),
            home,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn hooks(&self) -> &Path {
        &self.hooks
    }

    pub fn is_present(&self) -> bool {
        self.home.is_dir()
    }

    fn skill_file(&self) -> PathBuf {
        self.skill_dir.join("SKILL.md")
    }

    pub fn read_skill(&self) -> Result<Option<String>, StoreError> {
        read_optional(&self.skill_file())
    }

    pub fn write_skill(&self, text: &str) -> Result<PathBuf, StoreError> {
        self.write(&self.skill_file(), text)
    }

    pub fn remove_skill(&self) -> Result<Option<PathBuf>, StoreError> {
        let dir = &self.skill_dir;
        Ok(optional(dir, fs::remove_dir_all(dir))?.map(|()| dir.clone()))
    }

    pub fn read_hooks(&self) -> Result<Option<String>, StoreError> {
        read_optional(&self.hooks)
    }

    pub fn write_hooks(&self, text: &str) -> Result<PathBuf, StoreError> {
        self.write(&self.hooks, text)
    }

    pub fn file(&self, under_home: &str) -> PathBuf {
        self.home.join(under_home)
    }

    pub fn write(&self, file: &Path, text: &str) -> Result<PathBuf, StoreError> {
        replace_under(&self.home, file, text)?;
        Ok(file.to_path_buf())
    }
}

// `replace` creates every missing directory above the file, a missing `dir` among them.
pub fn replace_under(dir: &Path, file: &Path, text: &str) -> Result<(), StoreError> {
    if !dir.is_dir() {
        return Err(StoreError::io(dir.display(), "is not a directory"));
    }
    replace_file(file, text)
}

pub fn read_optional(path: &Path) -> Result<Option<String>, StoreError> {
    optional(path, fs::read_to_string(path))
}

pub fn remove_optional(path: &Path) -> Result<Option<PathBuf>, StoreError> {
    Ok(optional(path, fs::remove_file(path))?.map(|()| path.to_path_buf()))
}

const LINK_HOPS: usize = 40;

// A rename onto a link replaces the link itself.
fn target_of(path: &Path) -> Result<PathBuf, StoreError> {
    let mut at = path.to_path_buf();
    for _ in 0..LINK_HOPS {
        match fs::symlink_metadata(&at) {
            Ok(found) if found.is_symlink() => {
                let to = fs::read_link(&at).map_err(|e| io_error(&at, &e))?;
                at = at.parent().map_or(to.clone(), |dir| dir.join(&to));
            }
            Ok(_) => return fs::canonicalize(&at).map_err(|e| io_error(&at, &e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(at),
            Err(e) => return Err(io_error(&at, &e)),
        }
    }
    Err(StoreError::io(
        path.display(),
        "is a chain of links too long to follow",
    ))
}

fn replace_file(path: &Path, text: &str) -> Result<(), StoreError> {
    let target = target_of(path)?;
    let kept = fs::metadata(&target).ok().map(|held| held.permissions());
    replace(&target, text.as_bytes(), kept.as_ref(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(home: &Path) -> FsAgent {
        FsAgent::new("Claude Code", home.join(".claude"), "settings.json")
    }

    #[test]
    fn a_file_under_the_home_is_written_read_and_removed_alone() {
        let kept = tempfile::tempdir().unwrap();
        let home = kept.path().join(".codex");
        fs::create_dir(&home).unwrap();
        let agent = FsAgent::new("Codex", home.clone(), "hooks.json");
        let file = agent.file("rules/lantern.rules");
        assert_eq!(file, home.join("rules/lantern.rules"));
        assert_eq!(read_optional(&file), Ok(None));
        assert_eq!(remove_optional(&file), Ok(None));

        assert_eq!(agent.write(&file, "atlas"), Ok(file.clone()));
        assert_eq!(read_optional(&file), Ok(Some("atlas".to_owned())));
        fs::write(home.join("rules/default.rules"), "desk").unwrap();
        assert_eq!(remove_optional(&file), Ok(Some(file.clone())));
        assert!(!file.exists());
        assert_eq!(
            fs::read_to_string(home.join("rules/default.rules")).unwrap(),
            "desk"
        );
    }

    #[test]
    fn an_agent_is_present_when_its_home_is_a_directory_and_an_install_does_not_make_one() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        assert!(!agent.is_present());
        fs::write(kept.path().join(".claude"), "").unwrap();
        assert!(!agent.is_present());
        fs::remove_file(kept.path().join(".claude")).unwrap();
        fs::create_dir(kept.path().join(".claude")).unwrap();
        assert!(agent.is_present());
    }

    #[test]
    fn the_skill_is_written_read_and_removed_with_its_own_directory_alone() {
        let kept = tempfile::tempdir().unwrap();
        let home = kept.path().join(".claude");
        let agent = agent(kept.path());
        fs::create_dir(&home).unwrap();
        assert_eq!(agent.read_skill(), Ok(None));
        assert_eq!(agent.remove_skill(), Ok(None));

        let file = agent.write_skill("lantern").unwrap();
        assert_eq!(file, home.join("skills/worklog/SKILL.md"));
        assert_eq!(agent.read_skill(), Ok(Some("lantern".to_owned())));
        fs::write(home.join("skills/worklog/notes.md"), "atlas").unwrap();
        fs::create_dir_all(home.join("skills/other")).unwrap();
        fs::write(home.join("skills/other/SKILL.md"), "desk").unwrap();

        assert_eq!(agent.remove_skill(), Ok(Some(home.join("skills/worklog"))));
        assert!(!home.join("skills/worklog").exists());
        assert_eq!(
            fs::read_to_string(home.join("skills/other/SKILL.md")).unwrap(),
            "desk"
        );
        assert_eq!(agent.read_skill(), Ok(None));
    }

    #[test]
    fn the_hooks_are_none_until_written_and_read_back_as_written() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        fs::create_dir(kept.path().join(".claude")).unwrap();
        assert_eq!(agent.hooks(), kept.path().join(".claude/settings.json"));
        assert_eq!(agent.read_hooks(), Ok(None));
        agent.write_hooks("{}\n").unwrap();
        assert_eq!(agent.read_hooks(), Ok(Some("{}\n".to_owned())));
        agent.write_hooks("{\"hooks\": {}}\n").unwrap();
        assert_eq!(agent.read_hooks(), Ok(Some("{\"hooks\": {}}\n".to_owned())));
    }

    #[test]
    fn a_hooks_file_that_is_a_link_stays_one_when_written() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        let target = kept.path().join("dotfiles-settings.json");
        fs::create_dir(kept.path().join(".claude")).unwrap();
        fs::write(&target, "{}").unwrap();
        std::os::unix::fs::symlink(&target, agent.hooks()).unwrap();
        agent.write_hooks("{\"a\": 1}").unwrap();
        assert!(fs::symlink_metadata(agent.hooks()).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\": 1}");
    }

    #[test]
    fn a_home_that_is_a_file_is_a_failure_naming_the_path() {
        let kept = tempfile::tempdir().unwrap();
        fs::write(kept.path().join(".claude"), "").unwrap();
        let agent = agent(kept.path());
        let refused = agent.write_hooks("{}").unwrap_err().to_string();
        assert!(refused.contains(".claude"), "{refused}");
    }

    #[test]
    fn a_write_never_makes_the_home() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        for refused in [
            agent.write_hooks("{}").unwrap_err().to_string(),
            agent.write_skill("lantern").unwrap_err().to_string(),
        ] {
            assert!(refused.contains(".claude"), "{refused}");
        }
        assert!(!kept.path().join(".claude").exists());
    }

    #[test]
    fn a_write_keeps_the_permissions_of_the_file_it_replaces_and_leaves_nothing_beside_it() {
        use std::os::unix::fs::PermissionsExt as _;
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        fs::create_dir(kept.path().join(".claude")).unwrap();
        fs::write(agent.hooks(), "{}").unwrap();
        fs::set_permissions(agent.hooks(), fs::Permissions::from_mode(0o600)).unwrap();
        agent.write_hooks("{\"a\": 1}").unwrap();
        let mode = fs::metadata(agent.hooks()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(agent.read_hooks(), Ok(Some("{\"a\": 1}".to_owned())));
        let beside: Vec<_> = fs::read_dir(kept.path().join(".claude"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(beside, ["settings.json"]);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temporary_one() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        let home = kept.path().join(".claude");
        fs::create_dir(&home).unwrap();
        fs::create_dir(agent.hooks()).unwrap();
        assert!(agent.write_hooks("{}").is_err());
        assert!(agent.hooks().is_dir());
        let beside: Vec<_> = fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(beside, ["settings.json"]);
    }

    #[test]
    fn a_file_that_was_not_there_is_created() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        fs::create_dir(kept.path().join(".claude")).unwrap();
        agent.write_hooks("{}").unwrap();
        assert_eq!(agent.read_hooks(), Ok(Some("{}".to_owned())));
        agent.write_skill("lantern").unwrap();
        assert_eq!(agent.read_skill(), Ok(Some("lantern".to_owned())));
    }

    #[test]
    fn a_hooks_path_that_is_a_dangling_link_is_written_through() {
        let kept = tempfile::tempdir().unwrap();
        let agent = agent(kept.path());
        fs::create_dir(kept.path().join(".claude")).unwrap();
        let target = kept.path().join("dotfiles/settings.json");
        std::os::unix::fs::symlink(&target, agent.hooks()).unwrap();
        agent.write_hooks("{\"a\": 1}").unwrap();
        assert!(fs::symlink_metadata(agent.hooks()).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"a\": 1}");

        let relative = kept.path().join(".claude/skills");
        fs::create_dir(&relative).unwrap();
        std::os::unix::fs::symlink("elsewhere/later.json", relative.join("a.json")).unwrap();
        replace_file(&relative.join("a.json"), "x").unwrap();
        assert_eq!(
            fs::read_to_string(relative.join("elsewhere/later.json")).unwrap(),
            "x"
        );
    }
}
