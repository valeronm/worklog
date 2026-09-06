//! A coding agent's own files on this host: the skill under its skills
//! directory and the file its hooks are read from.

use std::path::PathBuf;

use crate::domain::ports::StoreError;

use super::{optional, read_optional, write_file};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub name: &'static str,
    home: PathBuf,
    /// The skill's own directory, so removing it takes nothing else.
    skill_dir: PathBuf,
    pub hooks: PathBuf,
}

impl Agent {
    pub(super) fn new(name: &'static str, home: PathBuf, hooks: &str) -> Agent {
        Agent {
            name,
            skill_dir: home.join("skills").join("worklog"),
            hooks: home.join(hooks),
            home,
        }
    }

    /// Whether the agent is on this host, by its home directory, so that
    /// an install never creates one for an agent that is not.
    pub(super) fn is_present(&self) -> bool {
        self.home.is_dir()
    }

    fn skill_file(&self) -> PathBuf {
        self.skill_dir.join("SKILL.md")
    }

    /// The skill as the agent has it, or `None` for an agent without one.
    pub fn read_skill(&self) -> Result<Option<String>, StoreError> {
        read_optional(&self.skill_file())
    }

    /// Writes the skill and returns its file.
    pub fn write_skill(&self, text: &str) -> Result<PathBuf, StoreError> {
        let file = self.skill_file();
        write_file(&file, text)?;
        Ok(file)
    }

    /// Removes the skill's directory and returns it, or `None` when there
    /// was none.
    pub fn remove_skill(&self) -> Result<Option<PathBuf>, StoreError> {
        let dir = &self.skill_dir;
        Ok(optional(dir, std::fs::remove_dir_all(dir))?.map(|()| dir.clone()))
    }

    /// The hooks file's text, or `None` for an agent without one.
    pub fn read_hooks(&self) -> Result<Option<String>, StoreError> {
        read_optional(&self.hooks)
    }

    pub fn write_hooks(&self, text: &str) -> Result<(), StoreError> {
        write_file(&self.hooks, text)
    }
}
