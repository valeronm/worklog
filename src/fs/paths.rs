use std::path::PathBuf;

use crate::domain::ports::StoreError;

use super::config::Config;

/// Where the config, the drafts and the store live on this host.
///
/// The config file is at a fixed place and names the store; until `init`
/// has written it there is no store to read. `WORKLOG_HOME=<dir>` puts all
/// three under one directory, which is how a test or a migration dry run
/// keeps clear of the real ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub drafts: PathBuf,
    /// `None` until `init` has run.
    pub store: Option<PathBuf>,
    /// The store `init` records when not told otherwise.
    pub default_store: PathBuf,
    /// The user's home, for `~/` in claims.
    pub home: PathBuf,
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
        let store = Config::read(&config)?.map(|c| c.store);
        Ok(Paths {
            config,
            drafts,
            store,
            default_store,
            home,
        })
    }
}
