use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::domain::id::DocumentId;
use crate::domain::ports::{Host, HostRead, StoreError};
use crate::domain::usage::Session;

use super::Config;

// This host as its config file and its environment described it when the command started.
pub struct FsHost {
    config: PathBuf,
    home: Option<String>,
    machine: RefCell<Option<DocumentId>>,
    store: PathBuf,
    session: Option<Session>,
}

impl FsHost {
    #[must_use]
    pub fn bound(
        config: PathBuf,
        home: Option<String>,
        held: Config,
        session: Option<Session>,
    ) -> FsHost {
        FsHost {
            config,
            home,
            machine: RefCell::new(Some(held.machine)),
            store: held.store,
            session,
        }
    }

    // `store` has to be from the root: a config holding any other path is refused when read.
    #[must_use]
    pub fn unbound(
        config: PathBuf,
        home: Option<String>,
        store: PathBuf,
        session: Option<Session>,
    ) -> FsHost {
        FsHost {
            config,
            home,
            machine: RefCell::new(None),
            store,
            session,
        }
    }

    #[must_use]
    pub fn in_session(mut self, session: Session) -> FsHost {
        self.session = Some(session);
        self
    }
}

fn on_disk(path: &Path) -> PathBuf {
    for existing in path.ancestors() {
        let Ok(resolved) = std::fs::canonicalize(existing) else {
            continue;
        };
        return match path.strip_prefix(existing) {
            Ok(rest) if rest.as_os_str().is_empty() => resolved,
            Ok(rest) => resolved.join(rest),
            Err(_) => resolved,
        };
    }
    path.to_path_buf()
}

impl HostRead for FsHost {
    fn machine(&self) -> Result<Option<DocumentId>, StoreError> {
        Ok(self.machine.borrow().clone())
    }

    fn home(&self) -> Result<Option<String>, StoreError> {
        Ok(self.home.clone())
    }

    fn resolve(&self, absolute: &str) -> Result<String, StoreError> {
        on_disk(Path::new(absolute))
            .into_os_string()
            .into_string()
            .map_err(|resolved| StoreError::io(Path::new(&resolved).display(), "is not UTF-8"))
    }

    fn session(&self) -> Option<Session> {
        self.session.clone()
    }

    fn name(&self) -> Option<String> {
        let named = rustix::system::uname();
        named.nodename().to_str().ok().map(str::to_owned)
    }
}

impl Host for FsHost {
    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError> {
        // Another process may have bound this host since the config was read.
        if Config::read(&self.config)?.is_some() {
            return Err(StoreError::io(
                self.config.display(),
                "this host is already bound",
            ));
        }
        let config = Config {
            store: self.store.clone(),
            machine: machine.clone(),
        };
        config.write(&self.config)?;
        self.machine.replace(Some(machine.clone()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{atlas, lamp_session, lantern};

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        (dir, config)
    }

    #[test]
    fn a_host_with_no_config_has_no_machine() {
        let (_dir, config) = scratch();
        let host = FsHost::unbound(config, None, PathBuf::from("/home/desk/store"), None);
        assert_eq!(host.machine(), Ok(None));
        assert_eq!(host.home(), Ok(None));
        assert_eq!(host.session(), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_host_is_named_as_its_operating_system_names_it() {
        let (_dir, config) = scratch();
        let host = FsHost::unbound(config, None, PathBuf::from("/home/desk/store"), None);
        let kernel = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
        assert_eq!(host.name().as_deref(), Some(kernel.trim_end_matches('\n')));
    }

    #[test]
    fn a_host_is_in_the_session_it_is_given() {
        let (_dir, config) = scratch();
        let session = lamp_session();
        let store = PathBuf::from("/home/desk/store");
        let host = FsHost::unbound(config, None, store, Some(session.clone()));
        assert_eq!(host.session(), Some(session));
    }

    #[test]
    fn a_bound_host_answers_its_machine_and_keeps_where_the_store_is() {
        let (dir, config) = scratch();
        let store = dir.path().join("store");
        let home = Some("/home/desk".to_owned());
        let host = FsHost::unbound(config.clone(), home.clone(), store.clone(), None);
        host.bind(&lantern()).unwrap();
        assert_eq!(host.machine(), Ok(Some(lantern())));
        assert_eq!(host.home(), Ok(Some("/home/desk".to_owned())));
        let kept = Config {
            store,
            machine: lantern(),
        };
        assert_eq!(Config::read(&config), Ok(Some(kept.clone())));

        let afresh = FsHost::bound(config.clone(), home, kept.clone(), None);
        assert_eq!(afresh.machine(), Ok(Some(lantern())));
        let refused = afresh.bind(&atlas()).unwrap_err().to_string();
        assert!(
            refused.starts_with(&config.display().to_string()),
            "{refused}"
        );
        assert_eq!(afresh.machine(), Ok(Some(lantern())));
        assert_eq!(Config::read(&config), Ok(Some(kept)));
    }

    #[test]
    fn a_directory_is_resolved_as_far_as_it_exists() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        std::fs::create_dir_all(root.join("projects/lantern")).unwrap();
        std::os::unix::fs::symlink(root.join("projects/lantern"), root.join("link")).unwrap();
        let host = FsHost::unbound(root.join("config.toml"), None, root.join("store"), None);
        let read = |given: &str| {
            let given = root.join(given);
            host.resolve(given.to_str().unwrap()).map(PathBuf::from)
        };

        assert_eq!(on_disk(&root.join("link")), root.join("projects/lantern"));
        assert_eq!(
            on_disk(&root.join("link/case/atlas")),
            root.join("projects/lantern/case/atlas")
        );
        assert_eq!(on_disk(&root.join("projects")), root.join("projects"));
        assert_eq!(on_disk(&root.join("phone/desk")), root.join("phone/desk"));

        assert_eq!(read("link"), Ok(root.join("projects/lantern")));
        assert_eq!(
            read("link/case/atlas"),
            Ok(root.join("projects/lantern/case/atlas"))
        );
        assert_eq!(read("projects/"), Ok(root.join("projects")));
        assert_eq!(read("phone/desk"), Ok(root.join("phone/desk")));
        std::fs::remove_file(root.join("link")).unwrap();
        assert_eq!(read("link/case"), Ok(root.join("link/case")));
        assert_eq!(host.resolve("/"), Ok("/".to_owned()));
    }

    #[test]
    fn a_host_bound_behind_one_that_read_no_config_is_not_bound_again() {
        let (dir, config) = scratch();
        let host = FsHost::unbound(config.clone(), None, dir.path().join("store"), None);
        let kept = Config {
            store: dir.path().join("elsewhere"),
            machine: lantern(),
        };
        kept.write(&config).unwrap();
        assert!(host.bind(&atlas()).is_err());
        assert_eq!(host.machine(), Ok(None));
        assert_eq!(Config::read(&config), Ok(Some(kept)));
    }
}
