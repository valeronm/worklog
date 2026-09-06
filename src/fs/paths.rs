use std::path::PathBuf;

use crate::domain::ports::StoreError;

use super::config::Config;

/// Where everything lives on this host: the config, the drafts, the
/// store the config names, and the agent's own files.
///
/// `WORKLOG_HOME=<dir>` puts the config, the drafts and the default store
/// under one directory, which is how a test or a migration dry run keeps
/// clear of the real ones; the agent's files follow `HOME` as always.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub drafts: PathBuf,
    /// The store `init` records when not told otherwise.
    pub default_store: PathBuf,
    /// The user's home, for `~/` in claims.
    pub home: PathBuf,
    /// The agents that take the skill and the session hook.
    pub agents: Vec<Agent>,
}

/// An agent with a user-wide home directory that holds its skills and the
/// file its hooks are read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub name: &'static str,
    pub home: PathBuf,
    pub skills: PathBuf,
    pub hooks: PathBuf,
}

impl Agent {
    fn new(name: &'static str, home: PathBuf, hooks: &str) -> Agent {
        Agent {
            name,
            skills: home.join("skills"),
            hooks: home.join(hooks),
            home,
        }
    }

    /// Whether the agent is on this host, by its home directory, so that
    /// an install never creates one for an agent that is not.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.home.is_dir()
    }
}

impl Paths {
    pub fn from_env() -> Result<Paths, StoreError> {
        let base = directories::BaseDirs::new().ok_or_else(|| StoreError::Io {
            location: "$HOME".into(),
            reason: "no home directory".into(),
        })?;
        let home = base.home_dir().to_path_buf();
        let (config, drafts, default_store) = match std::env::var_os("WORKLOG_HOME") {
            Some(root) => {
                let root = PathBuf::from(root);
                (root.join("config"), root.join("drafts"), root.join("store"))
            }
            None => (
                base.config_dir().join("worklog").join("config"),
                base.state_dir()
                    .unwrap_or(base.data_local_dir())
                    .join("worklog")
                    .join("drafts"),
                home.join("worklog"),
            ),
        };
        Ok(Paths {
            config,
            drafts,
            default_store,
            agents: vec![
                Agent::new("Claude Code", home.join(".claude"), "settings.json"),
                Agent::new("Codex", home.join(".codex"), "hooks.json"),
            ],
            home,
        })
    }

    /// The agents on this host.
    #[must_use]
    pub fn present_agents(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.is_present()).collect()
    }

    /// The store the config names, or `None` until `init` has run.
    pub fn store(&self) -> Result<Option<PathBuf>, StoreError> {
        Ok(Config::read(&self.config)?.map(|c| c.store))
    }
}
