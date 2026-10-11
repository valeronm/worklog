//! The adapters a front end runs its use cases over, built from this host's environment.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

mod fs;

use crate::app::{Deps, Failure, Stored};
use fs::{Config, FsAgent, FsDrafts, FsHost, FsStore, Paths, RandomIds, SystemClock};

/// The ports of one command, since the store among them is read once.
pub struct Adapters {
    store: FsStore,
    drafts: FsDrafts,
    ids: RandomIds,
    clock: SystemClock,
    host: FsHost,
}

impl Adapters {
    #[must_use]
    pub fn deps(&self) -> Deps<'_> {
        Deps {
            store: Stored::new(&self.store),
            drafts: &self.drafts,
            ids: &self.ids,
            clock: &self.clock,
            host: &self.host,
        }
    }

    /// Every later answer of these adapters' store comes from one reading of its tree, made
    /// here unless one was made.
    pub fn settle(&self) -> Result<(), Failure> {
        Ok(self.store.settle()?)
    }
}

/// The adapters over the store this host's config holds, or over `chosen`, the store a host
/// with no config is about to be bound to; `None` for a host with neither, one that is not
/// set up. Refuses a configured store that is not a directory unless a store is being chosen,
/// and a `chosen` one that is a file, or a directory holding something not dot-named and no
/// store.
pub fn wire(
    variable: &dyn Fn(&str) -> Option<OsString>,
    chosen: Option<PathBuf>,
) -> Result<Option<Adapters>, Failure> {
    let paths = Paths::resolved(variable)?;
    let (store, host) = match (Config::read(&paths.config)?, chosen) {
        (Some(held), chosen) => {
            let store = match chosen {
                None => FsStore::open(held.store.clone())?,
                Some(_) => FsStore::new(held.store.clone()),
            };
            (store, FsHost::bound(paths.config, paths.home, held))
        }
        (None, Some(chosen)) => (
            FsStore::chosen(chosen.clone())?,
            FsHost::unbound(paths.config, paths.home, chosen),
        ),
        (None, None) => return Ok(None),
    };
    Ok(Some(Adapters {
        store,
        drafts: FsDrafts::new(paths.drafts),
        ids: RandomIds,
        clock: SystemClock,
        host,
    }))
}

pub const SKILL_DIRECTORY: &str = fs::SKILL_DIRECTORY;

/// A coding agent on this host: its skill and the file its hooks are read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    files: FsAgent,
    homes: Vec<PathBuf>,
}

impl Agent {
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.files.name()
    }

    #[must_use]
    pub fn hooks(&self) -> &Path {
        self.files.hooks()
    }

    /// The user's home directory as given and, where a link leads elsewhere, as it resolves.
    #[must_use]
    pub(crate) fn homes(&self) -> Vec<&Path> {
        self.homes.iter().map(PathBuf::as_path).collect()
    }

    /// Whether the agent's home directory is there; nothing here ever creates one.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.files.is_present()
    }

    pub fn read_skill(&self) -> Result<Option<String>, Failure> {
        Ok(self.files.read_skill()?)
    }

    /// Returns the file written.
    pub fn write_skill(&self, text: &str) -> Result<PathBuf, Failure> {
        Ok(self.files.write_skill(text)?)
    }

    /// Removes the skill's own directory and returns it, or `None` when there was none.
    pub fn remove_skill(&self) -> Result<Option<PathBuf>, Failure> {
        Ok(self.files.remove_skill()?)
    }

    pub fn read_hooks(&self) -> Result<Option<String>, Failure> {
        Ok(self.files.read_hooks()?)
    }

    pub fn write_hooks(&self, text: &str) -> Result<(), Failure> {
        Ok(self.files.write_hooks(text)?)
    }
}

/// The agents this tool knows, present on this host or not, under `HOME` and not under the
/// directory `WORKLOG_NEXT_HOME` moves the rest to.
pub fn agents(variable: &dyn Fn(&str) -> Option<OsString>) -> Result<Vec<Agent>, Failure> {
    let paths = Paths::resolved(variable)?;
    let Some(home) = paths.home else {
        return Err(Failure::at("$HOME", "not set"));
    };
    let home = PathBuf::from(home);
    let mut homes = vec![home.clone()];
    if let Ok(resolved) = std::fs::canonicalize(&home)
        && resolved != home
    {
        homes.push(resolved);
    }
    Ok([
        ("Claude Code", ".claude", "settings.json"),
        ("Codex", ".codex", "hooks.json"),
    ]
    .map(|(name, directory, hooks)| Agent {
        files: FsAgent::new(name, home.join(directory), hooks),
        homes: homes.clone(),
    })
    .into())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::testing::lantern;

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        (kept, root)
    }

    fn wired(root: &Path, chosen: Option<PathBuf>) -> Result<Option<Adapters>, Failure> {
        let variable = |name: &str| match name {
            "WORKLOG_NEXT_HOME" => Some(root.join("next").into_os_string()),
            _ => None,
        };
        wire(&variable, chosen)
    }

    #[test]
    fn a_host_with_no_config_has_adapters_only_over_a_store_it_is_given() {
        let (_kept, root) = scratch();
        assert!(wired(&root, None).unwrap().is_none());
        assert!(wired(&root, Some(root.join("store"))).unwrap().is_some());
        assert!(!root.join("store").exists());
        assert!(!root.join("next").exists());
    }

    #[test]
    fn a_configured_store_that_is_gone_is_refused_except_to_one_choosing_a_store() {
        let (_kept, root) = scratch();
        let store = root.join("store");
        let config = Config {
            store: store.clone(),
            machine: lantern(),
        };
        config.write(&root.join("next/config.toml")).unwrap();

        let Err(Failure::Store(refused)) = wired(&root, None) else {
            panic!("a store that is not there must be refused");
        };
        assert_eq!(
            refused.to_string(),
            format!("{}: this host's store is not a directory", store.display())
        );
        assert!(
            wired(&root, Some(root.join("elsewhere")))
                .unwrap()
                .is_some()
        );

        std::fs::create_dir_all(&store).unwrap();
        assert!(wired(&root, None).unwrap().is_some());
    }

    #[test]
    fn a_damaged_config_is_a_failure_and_not_a_host_that_is_not_set_up() {
        let (_kept, root) = scratch();
        std::fs::create_dir_all(root.join("next")).unwrap();
        std::fs::write(root.join("next/config.toml"), "store = 4\n").unwrap();
        assert!(matches!(wired(&root, None), Err(Failure::Store(_))));
        let chosen = Some(root.join("store"));
        assert!(matches!(wired(&root, chosen), Err(Failure::Store(_))));
    }

    fn with_home(home: Option<&Path>, next: Option<&Path>) -> impl Fn(&str) -> Option<OsString> {
        let home = home.map(|home| home.as_os_str().to_owned());
        let next = next.map(|next| next.as_os_str().to_owned());
        move |name: &str| match name {
            "HOME" => home.clone(),
            "WORKLOG_NEXT_HOME" => next.clone(),
            _ => None,
        }
    }

    #[test]
    fn the_agents_are_under_home_whatever_the_rest_is_moved_to() {
        let (_kept, root) = scratch();
        for next in [None, Some(root.join("next"))] {
            let listed = agents(&with_home(Some(&root), next.as_deref())).unwrap();
            let seen: Vec<_> = listed
                .iter()
                .map(|agent| (agent.name(), agent.hooks().to_path_buf()))
                .collect();
            assert_eq!(
                seen,
                [
                    ("Claude Code", root.join(".claude/settings.json")),
                    ("Codex", root.join(".codex/hooks.json")),
                ]
            );
        }
    }

    #[test]
    fn an_agent_is_present_by_its_home_and_its_files_are_reached_through_it() {
        let (_kept, root) = scratch();
        std::fs::create_dir(root.join(".codex")).unwrap();
        let listed = agents(&with_home(Some(&root), None)).unwrap();
        let present: Vec<_> = listed.iter().filter(|a| a.is_present()).collect();
        assert_eq!(present.len(), 1);
        let codex = present[0];
        assert_eq!(codex.name(), "Codex");
        assert_eq!(codex.read_hooks().unwrap(), None);
        codex.write_hooks("{}").unwrap();
        assert_eq!(codex.read_hooks().unwrap(), Some("{}".to_owned()));
        let skill = codex.write_skill("lantern").unwrap();
        assert_eq!(skill, root.join(".codex/skills/worklog-next/SKILL.md"));
        assert_eq!(codex.read_skill().unwrap(), Some("lantern".to_owned()));
        assert_eq!(
            codex.remove_skill().unwrap(),
            Some(root.join(".codex/skills/worklog-next"))
        );
        assert!(!root.join(".claude").exists());
        assert_eq!(codex.homes(), [root.as_path()]);
    }

    #[test]
    fn no_home_directory_is_a_refusal_naming_it() {
        let (_kept, root) = scratch();
        for next in [None, Some(root.as_path())] {
            let refused = agents(&with_home(None, next)).unwrap_err().to_string();
            assert!(refused.contains("$HOME"), "{refused}");
        }
    }

    #[test]
    fn a_home_reached_through_a_link_is_given_as_it_is_and_as_it_resolves() {
        let (_kept, root) = scratch();
        std::fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        let listed = agents(&with_home(Some(&root.join("link")), None)).unwrap();
        assert_eq!(
            listed[0].homes(),
            [root.join("link").as_path(), root.join("real").as_path()]
        );
    }
}
