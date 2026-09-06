use std::path::PathBuf;

use crate::domain::ports::StoreError;

use super::agent::Agent;
use super::config::Config;
use super::shell::Shell;

/// Where everything lives on this host: the config, the drafts, the
/// store the config names, and the agents and shells that take something
/// from the binary.
///
/// `WORKLOG_HOME=<dir>` puts the config, the drafts and the default store
/// under one directory, which is how a test or a migration dry run keeps
/// clear of the real ones; the agents' files follow `HOME` as always.
/// `WORKLOG_RELEASES=<dir>` is where `upgrade` reads releases instead of
/// GitHub, for the same reason.
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
    /// The shells that take completions.
    pub shells: Vec<Shell>,
    /// A directory of releases to upgrade from, when set.
    pub releases: Option<PathBuf>,
}

impl Paths {
    pub fn from_env() -> Result<Paths, StoreError> {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| StoreError::io("$HOME", "no home directory"))?;
        let home = base.home_dir().to_path_buf();
        // Not `base.config_dir()`, which on macOS is not where fish reads.
        let config_home =
            std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
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
            shells: vec![Shell::new(
                clap_complete::Shell::Fish,
                config_home.join("fish").join("completions"),
                "worklog.fish",
            )],
            releases: std::env::var_os("WORKLOG_RELEASES").map(PathBuf::from),
            home,
        })
    }

    /// The agents on this host.
    #[must_use]
    pub fn present_agents(&self) -> Vec<&Agent> {
        self.agents.iter().filter(|a| a.is_present()).collect()
    }

    /// The shells on this host.
    #[must_use]
    pub fn present_shells(&self) -> Vec<&Shell> {
        self.shells.iter().filter(|s| s.is_present()).collect()
    }

    /// The store the config names, or `None` until `init` has run.
    pub fn store(&self) -> Result<Option<PathBuf>, StoreError> {
        Ok(Config::read(&self.config)?.map(|c| c.store))
    }
}
