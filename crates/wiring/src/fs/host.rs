use std::cell::RefCell;
use std::fs::{File, TryLockError};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use worklog_domain::id::DocumentId;
use worklog_domain::ports::{Host, HostRead, StoreError};
use worklog_domain::session::Session;

use super::{Config, io_error, make_dir};

/// Locks `dir`, the config's directory, until the returned file is dropped; a second setup of
/// this host is refused meanwhile.
pub fn setup_lock(dir: &Path) -> Result<File, StoreError> {
    let held = File::open(dir).map_err(|e| io_error(dir, &e))?;
    held.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => {
            StoreError::io(dir.display(), "another setup of this host is running")
        }
        TryLockError::Error(e) => io_error(dir, &e),
    })?;
    Ok(held)
}

// This host as its config file and its environment described it when the command started.
pub struct FsHost {
    config: PathBuf,
    home: Option<String>,
    machine: RefCell<Option<DocumentId>>,
    store: PathBuf,
    session: Option<Session>,
    setup: RefCell<Option<File>>,
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
            setup: RefCell::new(None),
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
            setup: RefCell::new(None),
        }
    }

    #[must_use]
    pub fn in_session(mut self, session: Session) -> FsHost {
        self.session = Some(session);
        self
    }

    // Another process may have bound this host since the config was read.
    fn unbound_on_disk(&self) -> Result<(), StoreError> {
        if Config::read(&self.config)?.is_some() {
            return Err(StoreError::io(
                self.config.display(),
                "this host is already bound",
            ));
        }
        Ok(())
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

    fn is_directory(&self, absolute: &str) -> Result<bool, StoreError> {
        let path = Path::new(absolute);
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(metadata.is_dir()),
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                Ok(false)
            }
            Err(e) => Err(io_error(path, &e)),
        }
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
    fn hold_setup(&self) -> Result<(), StoreError> {
        // A lock belongs to one open of the directory, not to the process holding it.
        if self.setup.borrow().is_some() {
            return self.unbound_on_disk();
        }
        let Some(dir) = self.config.parent() else {
            return Err(StoreError::io(
                self.config.display(),
                "is not a file's path",
            ));
        };
        make_dir(dir)?;
        let held = setup_lock(dir)?;
        self.unbound_on_disk()?;
        self.setup.replace(Some(held));
        Ok(())
    }

    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError> {
        self.unbound_on_disk()?;
        if self.setup.borrow().is_none() {
            return Err(StoreError::io(
                self.config.display(),
                "this host's setup is not held",
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
    use worklog_domain::testing::{atlas, lamp_session, lantern};

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
        host.hold_setup().unwrap();
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
    fn the_setup_of_a_host_is_held_by_one_port_at_a_time() {
        let (dir, config) = scratch();
        let store = dir.path().join("store");
        let first = FsHost::unbound(config.clone(), None, store.clone(), None);
        let second = FsHost::unbound(config.clone(), None, store, None);
        assert_eq!(first.hold_setup(), Ok(()));
        let running = format!(
            "{}: another setup of this host is running",
            dir.path().display()
        );
        assert_eq!(second.hold_setup().unwrap_err().to_string(), running);
        assert_eq!(first.hold_setup(), Ok(()));
        assert_eq!(second.hold_setup().unwrap_err().to_string(), running);
        drop(first);
        assert_eq!(second.hold_setup(), Ok(()));
    }

    #[test]
    fn a_setup_held_behind_one_that_bound_the_host_is_refused() {
        let (dir, config) = scratch();
        let host = FsHost::unbound(config.clone(), None, dir.path().join("store"), None);
        let kept = Config {
            store: dir.path().join("elsewhere"),
            machine: lantern(),
        };
        kept.write(&config).unwrap();
        let bound = format!("{}: this host is already bound", config.display());
        assert_eq!(host.hold_setup().unwrap_err().to_string(), bound);
    }

    #[test]
    fn a_host_is_bound_only_while_its_setup_is_held() {
        let (dir, config) = scratch();
        let host = FsHost::unbound(config.clone(), None, dir.path().join("store"), None);
        let unheld = format!("{}: this host's setup is not held", config.display());
        assert_eq!(host.bind(&lantern()).unwrap_err().to_string(), unheld);
        assert_eq!(host.machine(), Ok(None));
        assert_eq!(Config::read(&config), Ok(None));
        host.hold_setup().unwrap();
        assert_eq!(host.bind(&lantern()), Ok(()));
        assert_eq!(host.machine(), Ok(Some(lantern())));
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
