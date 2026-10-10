use std::ffi::OsString;
use std::path::PathBuf;

use crate::domain::ports::StoreError;

const OVERRIDE: &str = "WORKLOG_NEXT_HOME";
const APP: &str = "worklog-next";

/// Where this host keeps what is never synced, and its home directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub drafts: PathBuf,
    pub home: Option<String>,
}

impl Paths {
    /// The places under `WORKLOG_NEXT_HOME` when `variable` holds it, otherwise under the
    /// XDG base directories, which default to the home directory. A variable set to nothing
    /// counts as not set; no `HOME` without `WORKLOG_NEXT_HOME` is refused, and so is a
    /// `HOME` that is not UTF-8.
    pub fn resolved(variable: impl Fn(&str) -> Option<OsString>) -> Result<Paths, StoreError> {
        let set = |name: &str| {
            variable(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let home = match set("HOME").map(|home| home.into_os_string().into_string()) {
            Some(Ok(home)) => Some(home),
            Some(Err(_)) => return Err(StoreError::io("$HOME", "is not UTF-8")),
            None => None,
        };
        if let Some(root) = set(OVERRIDE) {
            return Ok(Paths {
                config: root.join("config.toml"),
                drafts: root.join("drafts"),
                home,
            });
        }
        let Some(home) = home else {
            return Err(StoreError::io(
                "$HOME",
                format!("not set, and neither is ${OVERRIDE}"),
            ));
        };
        let base = |name: &str, default: &str| {
            set(name).unwrap_or_else(|| PathBuf::from(&home).join(default))
        };
        Ok(Paths {
            config: base("XDG_CONFIG_HOME", ".config")
                .join(APP)
                .join("config.toml"),
            drafts: base("XDG_STATE_HOME", ".local/state")
                .join(APP)
                .join("drafts"),
            home: Some(home),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(set: &[(&str, &str)]) -> Result<Paths, StoreError> {
        Paths::resolved(|name| {
            set.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        })
    }

    fn places(set: &[(&str, &str)]) -> (String, String) {
        let paths = resolved(set).unwrap();
        (
            paths.config.display().to_string(),
            paths.drafts.display().to_string(),
        )
    }

    #[test]
    fn the_config_and_the_drafts_default_to_the_home_directory() {
        let paths = resolved(&[("HOME", "/home/desk")]).unwrap();
        assert_eq!(
            paths.config,
            PathBuf::from("/home/desk/.config/worklog-next/config.toml")
        );
        assert_eq!(
            paths.drafts,
            PathBuf::from("/home/desk/.local/state/worklog-next/drafts")
        );
        assert_eq!(paths.home, Some("/home/desk".to_owned()));
    }

    #[test]
    fn each_base_directory_the_environment_gives_replaces_its_default() {
        let home = ("HOME", "/home/desk");
        assert_eq!(
            places(&[home, ("XDG_CONFIG_HOME", "/atlas/config")]),
            (
                "/atlas/config/worklog-next/config.toml".to_owned(),
                "/home/desk/.local/state/worklog-next/drafts".to_owned()
            )
        );
        assert_eq!(
            places(&[home, ("XDG_STATE_HOME", "/atlas/state")]),
            (
                "/home/desk/.config/worklog-next/config.toml".to_owned(),
                "/atlas/state/worklog-next/drafts".to_owned()
            )
        );
        assert_eq!(
            places(&[
                home,
                ("XDG_CONFIG_HOME", "/atlas/config"),
                ("XDG_STATE_HOME", "/atlas/state")
            ]),
            (
                "/atlas/config/worklog-next/config.toml".to_owned(),
                "/atlas/state/worklog-next/drafts".to_owned()
            )
        );
    }

    #[test]
    fn one_directory_given_for_everything_holds_the_config_and_the_drafts() {
        let set = [
            ("HOME", "/home/desk"),
            ("XDG_CONFIG_HOME", "/atlas/config"),
            ("XDG_STATE_HOME", "/atlas/state"),
            ("WORKLOG_NEXT_HOME", "/lantern"),
        ];
        let paths = resolved(&set).unwrap();
        assert_eq!(paths.config, PathBuf::from("/lantern/config.toml"));
        assert_eq!(paths.drafts, PathBuf::from("/lantern/drafts"));
        assert_eq!(paths.home, Some("/home/desk".to_owned()));

        let alone = resolved(&[("WORKLOG_NEXT_HOME", "/lantern")]).unwrap();
        assert_eq!(alone.config, PathBuf::from("/lantern/config.toml"));
        assert_eq!(alone.home, None);
    }

    #[test]
    fn a_variable_set_to_nothing_is_one_not_set() {
        let set = [
            ("HOME", "/home/desk"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_STATE_HOME", ""),
            ("WORKLOG_NEXT_HOME", ""),
        ];
        assert_eq!(places(&set), places(&[("HOME", "/home/desk")]));
        let homeless = resolved(&[("HOME", ""), ("WORKLOG_NEXT_HOME", "/lantern")]).unwrap();
        assert_eq!(homeless.home, None);
    }

    #[test]
    fn a_home_directory_that_is_not_utf_8_is_refused_wherever_the_rest_is_kept() {
        use std::os::unix::ffi::OsStringExt as _;
        for given in [None, Some("/lantern")] {
            let refused = Paths::resolved(|name| match name {
                "HOME" => Some(OsString::from_vec(vec![b'/', 0xff])),
                "WORKLOG_NEXT_HOME" => given.map(OsString::from),
                _ => None,
            });
            assert_eq!(refused, Err(StoreError::io("$HOME", "is not UTF-8")));
        }
    }

    #[test]
    fn no_home_directory_and_no_directory_given_instead_is_refused() {
        for set in [
            &[][..],
            &[("HOME", "")],
            &[
                ("XDG_CONFIG_HOME", "/atlas/config"),
                ("XDG_STATE_HOME", "/atlas/state"),
            ],
        ] {
            let refused = resolved(set).unwrap_err().to_string();
            assert!(refused.contains("HOME"), "{refused}");
        }
    }
}
