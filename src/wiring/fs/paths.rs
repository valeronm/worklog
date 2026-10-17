use std::ffi::OsString;
use std::path::PathBuf;

use crate::domain::ports::StoreError;

const OVERRIDE: &str = "WORKLOG_HOME";
const APP: &str = "worklog";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub drafts: PathBuf,
    /// The directory of this host's cache of each store.
    pub cache: PathBuf,
    pub home: Option<String>,
    /// `XDG_CONFIG_HOME` or the home directory's `.config`, whatever `WORKLOG_HOME` holds.
    pub config_home: Option<PathBuf>,
}

/// A variable set to nothing is one not set.
pub fn set(variable: impl Fn(&str) -> Option<OsString>, name: &str) -> Option<OsString> {
    variable(name).filter(|value| !value.is_empty())
}

impl Paths {
    /// Refuses a `HOME` that is not UTF-8.
    pub fn home(variable: impl Fn(&str) -> Option<OsString>) -> Result<Option<String>, StoreError> {
        match set(&variable, "HOME") {
            Some(home) => match home.into_string() {
                Ok(home) => Ok(Some(home)),
                Err(_) => Err(StoreError::io("$HOME", "is not UTF-8")),
            },
            None => Ok(None),
        }
    }

    pub fn resolved(variable: impl Fn(&str) -> Option<OsString>) -> Result<Paths, StoreError> {
        let set = |name: &str| set(&variable, name).map(PathBuf::from);
        let home = Paths::home(&variable)?;
        // The base directory specification has a relative path ignored.
        let given = set("XDG_CONFIG_HOME").filter(|given| given.is_absolute());
        let config_home = given.or_else(|| {
            home.as_ref()
                .map(|home| PathBuf::from(home).join(".config"))
        });
        if let Some(root) = set(OVERRIDE) {
            return Ok(Paths {
                config: root.join("config.toml"),
                drafts: root.join("drafts"),
                cache: root.join("cache"),
                home,
                config_home,
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
            cache: base("XDG_CACHE_HOME", ".cache").join(APP),
            home: Some(home),
            config_home,
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
            PathBuf::from("/home/desk/.config/worklog/config.toml")
        );
        assert_eq!(
            paths.drafts,
            PathBuf::from("/home/desk/.local/state/worklog/drafts")
        );
        assert_eq!(paths.home, Some("/home/desk".to_owned()));
    }

    #[test]
    fn the_cache_is_under_the_cache_base_directory_or_the_one_given_for_everything() {
        let cache = |set: &[(&str, &str)]| resolved(set).unwrap().cache;
        let home = ("HOME", "/home/desk");
        assert_eq!(cache(&[home]), PathBuf::from("/home/desk/.cache/worklog"));
        assert_eq!(
            cache(&[home, ("XDG_CACHE_HOME", "/atlas/cache")]),
            PathBuf::from("/atlas/cache/worklog")
        );
        assert_eq!(
            cache(&[
                home,
                ("XDG_CACHE_HOME", "/atlas/cache"),
                ("WORKLOG_HOME", "/lantern")
            ]),
            PathBuf::from("/lantern/cache")
        );
        assert_eq!(
            cache(&[home, ("XDG_CACHE_HOME", "")]),
            PathBuf::from("/home/desk/.cache/worklog")
        );
    }

    #[test]
    fn each_base_directory_the_environment_gives_replaces_its_default() {
        let home = ("HOME", "/home/desk");
        assert_eq!(
            places(&[home, ("XDG_CONFIG_HOME", "/atlas/config")]),
            (
                "/atlas/config/worklog/config.toml".to_owned(),
                "/home/desk/.local/state/worklog/drafts".to_owned()
            )
        );
        assert_eq!(
            places(&[home, ("XDG_STATE_HOME", "/atlas/state")]),
            (
                "/home/desk/.config/worklog/config.toml".to_owned(),
                "/atlas/state/worklog/drafts".to_owned()
            )
        );
        assert_eq!(
            places(&[
                home,
                ("XDG_CONFIG_HOME", "/atlas/config"),
                ("XDG_STATE_HOME", "/atlas/state")
            ]),
            (
                "/atlas/config/worklog/config.toml".to_owned(),
                "/atlas/state/worklog/drafts".to_owned()
            )
        );
    }

    #[test]
    fn one_directory_given_for_everything_holds_the_config_and_the_drafts() {
        let set = [
            ("HOME", "/home/desk"),
            ("XDG_CONFIG_HOME", "/atlas/config"),
            ("XDG_STATE_HOME", "/atlas/state"),
            ("WORKLOG_HOME", "/lantern"),
        ];
        let paths = resolved(&set).unwrap();
        assert_eq!(paths.config, PathBuf::from("/lantern/config.toml"));
        assert_eq!(paths.drafts, PathBuf::from("/lantern/drafts"));
        assert_eq!(paths.home, Some("/home/desk".to_owned()));

        let alone = resolved(&[("WORKLOG_HOME", "/lantern")]).unwrap();
        assert_eq!(alone.config, PathBuf::from("/lantern/config.toml"));
        assert_eq!(alone.home, None);
    }

    #[test]
    fn the_config_of_other_tools_stays_under_home_whatever_holds_this_tool_s() {
        let under = |set: &[(&str, &str)]| resolved(set).unwrap().config_home;
        let home = ("HOME", "/home/desk");
        let next = ("WORKLOG_HOME", "/lantern");
        assert_eq!(under(&[home]), Some(PathBuf::from("/home/desk/.config")));
        assert_eq!(
            under(&[home, next]),
            Some(PathBuf::from("/home/desk/.config"))
        );
        let given = ("XDG_CONFIG_HOME", "/atlas/config");
        assert_eq!(under(&[home, given]), Some(PathBuf::from("/atlas/config")));
        assert_eq!(
            under(&[home, next, given]),
            Some(PathBuf::from("/atlas/config"))
        );
        assert_eq!(under(&[next, given]), Some(PathBuf::from("/atlas/config")));
        assert_eq!(under(&[next]), None);
        for relative in ["config", "./config", "../atlas/config"] {
            let given = ("XDG_CONFIG_HOME", relative);
            assert_eq!(
                under(&[home, given]),
                Some(PathBuf::from("/home/desk/.config")),
                "{relative}"
            );
            assert_eq!(under(&[next, given]), None, "{relative}");
        }
        assert_eq!(
            under(&[home, next, ("XDG_CONFIG_HOME", "")]),
            Some(PathBuf::from("/home/desk/.config"))
        );
    }

    #[test]
    fn a_variable_set_to_nothing_is_one_not_set() {
        let set = [
            ("HOME", "/home/desk"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_STATE_HOME", ""),
            ("WORKLOG_HOME", ""),
        ];
        assert_eq!(places(&set), places(&[("HOME", "/home/desk")]));
        let homeless = resolved(&[("HOME", ""), ("WORKLOG_HOME", "/lantern")]).unwrap();
        assert_eq!(homeless.home, None);
    }

    #[test]
    fn a_home_directory_that_is_not_utf_8_is_refused_wherever_the_rest_is_kept() {
        use std::os::unix::ffi::OsStringExt as _;
        for given in [None, Some("/lantern")] {
            let refused = Paths::resolved(|name| match name {
                "HOME" => Some(OsString::from_vec(vec![b'/', 0xff])),
                "WORKLOG_HOME" => given.map(OsString::from),
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
