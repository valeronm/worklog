//! The directory a claim places its topic in.

use std::fmt;

use serde::Serialize;

/// `~`, a path under `~/`, or an absolute path, held without a trailing slash.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Directory(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotADirectory;

impl fmt::Display for NotADirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("is neither under `~/` nor an absolute path")
    }
}

fn fold_home(path: &str, home: Option<&str>) -> String {
    let under = home
        .filter(|home| home.starts_with('/') && path.starts_with('/'))
        .and_then(|home| path.strip_prefix(home.trim_end_matches('/')));
    match under {
        Some(rest) if rest.trim_end_matches('/').is_empty() => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

impl Directory {
    pub fn parse(text: &str) -> Result<Directory, NotADirectory> {
        if text == "~" || text.starts_with("~/") || text.starts_with('/') {
            let spelled = match text.trim_end_matches('/') {
                "" => "/",
                spelled => spelled,
            };
            Ok(Directory(spelled.to_owned()))
        } else {
            Err(NotADirectory)
        }
    }

    /// A path from the root with `.` and `..` folded as typed, following no link: `..` drops
    /// the component before it and stops at the root, and a slash that ends the path or
    /// repeats goes. Any other text is returned as given.
    #[must_use]
    pub fn folded(path: &str) -> String {
        if !path.starts_with('/') {
            return path.to_owned();
        }
        let mut kept: Vec<&str> = Vec::new();
        for component in path.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    kept.pop();
                }
                other => kept.push(other),
            }
        }
        format!("/{}", kept.join("/"))
    }

    /// `given` as a claim carries it on a host whose home directory is `home`: `~` for the
    /// home itself, `~/rest` for a path under it by whole components, and `given` for any
    /// other, for a path or a home that is not absolute and for no home.
    pub fn on_host(given: &str, home: Option<&str>) -> Result<Directory, NotADirectory> {
        Directory::parse(&fold_home(given, home))
    }

    /// The number of components of this directory when `other` is it or under it by whole
    /// components; a directory under `~` and an absolute one never cover each other.
    #[must_use]
    pub fn covers(&self, other: &Directory) -> Option<usize> {
        let covering = self.components();
        other
            .components()
            .starts_with(&covering)
            .then_some(covering.len())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn components(&self) -> Vec<&str> {
        self.0.trim_end_matches('/').split('/').collect()
    }
}

impl fmt::Display for Directory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(text: &str) -> Directory {
        Directory::parse(text).unwrap()
    }

    #[test]
    fn a_directory_is_under_home_or_absolute() {
        for good in ["~", "~/projects/lantern", "/srv/lantern", "/"] {
            assert_eq!(directory(good).as_str(), good);
            assert_eq!(directory(good).to_string(), good);
        }
        for bad in ["", "projects/lantern", "~lantern", "./lantern"] {
            assert_eq!(Directory::parse(bad), Err(NotADirectory), "{bad}");
            assert_eq!(
                Directory::on_host(bad, Some("/home/desk")),
                Err(NotADirectory)
            );
        }
    }

    #[test]
    fn a_directory_is_spelled_without_a_trailing_slash() {
        for (given, spelled) in [
            ("~/lantern/", "~/lantern"),
            ("~/lantern//", "~/lantern"),
            ("~/lantern/src/", "~/lantern/src"),
            ("/srv/lantern/", "/srv/lantern"),
            ("~/", "~"),
            ("~", "~"),
            ("/", "/"),
            ("//", "/"),
        ] {
            assert_eq!(directory(given).as_str(), spelled, "{given}");
            assert_eq!(directory(given).to_string(), spelled, "{given}");
            assert_eq!(directory(given), directory(spelled), "{given}");
        }
        assert_ne!(directory("~/lantern/"), directory("~/lantern/src"));
        assert_eq!(directory("//lantern").as_str(), "//lantern");
    }

    #[test]
    fn directories_are_equal_exactly_when_each_covers_the_other() {
        let spellings = [
            "~",
            "~/",
            "~/lantern",
            "~/lantern/",
            "/",
            "/lantern",
            "/lantern/",
        ];
        for a in spellings.map(directory) {
            for b in spellings.map(directory) {
                let both = a.covers(&b).is_some() && b.covers(&a).is_some();
                assert_eq!(a == b, both, "{a} and {b}");
            }
        }
    }

    #[test]
    fn a_path_at_or_under_home_folds_to_the_form_a_claim_carries() {
        let home = Some("/home/desk");
        for (path, folded) in [
            ("/home/desk", "~"),
            ("/home/desk/", "~"),
            ("/home/desk/lantern", "~/lantern"),
            ("/home/desk/lantern/src/", "~/lantern/src/"),
            ("/home/desktop/x", "/home/desktop/x"),
            ("/home", "/home"),
            ("/srv/lantern", "/srv/lantern"),
            ("~/lantern", "~/lantern"),
            ("lantern", "lantern"),
            ("", ""),
        ] {
            assert_eq!(fold_home(path, home), folded, "{path}");
            assert_eq!(fold_home(path, None), path);
        }
        assert_eq!(
            fold_home("/home/desk/lantern", Some("/home/desk/")),
            "~/lantern"
        );
        for no_home in ["", "desk", "~"] {
            assert_eq!(fold_home("/home/desk", Some(no_home)), "/home/desk");
        }
        for home in ["/", "/home/desk", "", "desk", "~"] {
            for path in ["", "desk/lantern", "~", "~/lantern", "./lantern"] {
                assert_eq!(fold_home(path, Some(home)), path, "{path:?} with {home:?}");
            }
        }
        assert_eq!(fold_home("/srv", Some("/")), "~/srv");
        assert_eq!(fold_home("/", Some("/")), "~");
    }

    #[test]
    fn a_path_from_the_root_is_folded_as_it_is_typed() {
        for (typed, folded) in [
            ("/a/./b", "/a/b"),
            ("/a/../b", "/b"),
            ("/a/b/..", "/a"),
            ("/a/b/../..", "/"),
            ("/..", "/"),
            ("/../../a", "/a"),
            ("/a/b/", "/a/b"),
            ("/a//b", "/a/b"),
            ("/", "/"),
            ("/a/b", "/a/b"),
            ("a/../b", "a/../b"),
            ("~/a/../b", "~/a/../b"),
            ("./a", "./a"),
            ("", ""),
        ] {
            assert_eq!(Directory::folded(typed), folded, "{typed}");
        }
    }

    #[test]
    fn a_given_path_is_folded_before_it_is_a_directory() {
        let home = Some("/home/desk");
        assert_eq!(
            Directory::on_host("/home/desk/lantern", home),
            Ok(directory("~/lantern"))
        );
        assert_eq!(Directory::on_host("/home/desk", home), Ok(directory("~")));
        assert_eq!(
            Directory::on_host("/home/desk/lantern", None),
            Ok(directory("/home/desk/lantern"))
        );
        assert_eq!(
            Directory::on_host("~/lantern", home),
            Ok(directory("~/lantern"))
        );
        let slashed = Directory::on_host("/home/desk/lantern/src/", home).unwrap();
        assert_eq!(slashed.as_str(), "~/lantern/src");
        assert_eq!(
            Directory::on_host("/home/desk/", home).unwrap().as_str(),
            "~"
        );
    }

    #[test]
    fn a_directory_covers_itself_and_what_is_under_it_by_whole_components() {
        let covers = |covering: &str, other: &str| directory(covering).covers(&directory(other));
        assert_eq!(covers("/work", "/work"), Some(2));
        assert_eq!(covers("/work", "/work/lantern/src"), Some(2));
        assert_eq!(covers("/work/lantern/", "/work/lantern"), Some(3));
        assert_eq!(covers("/work/lantern", "/work/lantern/"), Some(3));
        assert_eq!(covers("/work/lan", "/work/lantern"), None);
        assert_eq!(covers("/work/lantern", "/work"), None);
        assert_eq!(covers("/", "/work"), Some(1));
        assert_eq!(covers("/", "/"), Some(1));
        assert_eq!(covers("/", "~/work"), None);
        assert_eq!(covers("~", "~/work"), Some(1));
        assert_eq!(covers("~", "~"), Some(1));
        assert_eq!(covers("~", "/work"), None);
        assert_eq!(covers("~/work", "~/work/lantern"), Some(2));
    }
}
