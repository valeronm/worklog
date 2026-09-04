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
    /// The agent's settings file, where the session hook goes.
    pub agent_settings: PathBuf,
    /// The agent's skills directory, where the skill goes.
    pub agent_skills: PathBuf,
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
        let agent = home.join(".claude");
        Ok(Paths {
            config,
            drafts,
            default_store,
            agent_settings: agent.join("settings.json"),
            agent_skills: agent.join("skills"),
            home,
        })
    }

    /// The store the config names, or `None` until `init` has run.
    pub fn store(&self) -> Result<Option<PathBuf>, StoreError> {
        Ok(Config::read(&self.config)?.map(|c| c.store))
    }
}
