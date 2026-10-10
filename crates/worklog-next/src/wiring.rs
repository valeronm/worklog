//! The adapters a front end runs its use cases over, built from this host's environment.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::app::{Deps, Failure, Stored};
use crate::fs::{Config, FsDrafts, FsHost, FsStore, Paths, RandomIds, SystemClock};

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
}

/// The adapters over the store this host's config holds, or over `chosen`, the store a host
/// with no config is about to be bound to; `None` for a host with neither, one that is not
/// set up. Refuses a configured store that is not a directory unless a store is being chosen,
/// and a `chosen` one that `FsStore::chosen` refuses.
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
}
