use std::cmp::Ordering;

use serde::Serialize;

use super::Failure;
use crate::domain::ports::{Binary, Refreshed, Releases};
use crate::domain::release::{Built, ReleaseVersion, verify};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Checked {
    pub current: Built,
    pub latest: ReleaseVersion,
    pub newer: bool,
}

fn outcome(current: Built, latest: ReleaseVersion) -> Outcome {
    match latest.cmp(&current.release) {
        Ordering::Greater => Outcome::Upgraded,
        Ordering::Equal if current.build.is_some() => Outcome::Ahead,
        Ordering::Equal => Outcome::Current,
        Ordering::Less => Outcome::Ahead,
    }
}

impl Checked {
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        outcome(self.current, self.latest)
    }

    #[must_use]
    pub fn upgrade(&self, written: Vec<String>) -> Upgrade {
        Upgrade {
            current: self.current,
            latest: self.latest,
            outcome: self.outcome(),
            written,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Current,
    Ahead,
    Upgraded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Upgrade {
    pub current: Built,
    pub latest: ReleaseVersion,
    pub outcome: Outcome,
    pub written: Vec<String>,
}

/// The latest release beside the version running, with the tag that release's assets are
/// asked for under.
pub fn check(releases: &dyn Releases, current: Built) -> Result<(Checked, String), Failure> {
    let tag = releases.latest()?;
    let latest = ReleaseVersion::parse(&tag).map_err(Failure::Refused)?;
    let checked = Checked {
        current,
        latest,
        newer: outcome(current, latest) == Outcome::Upgraded,
    };
    Ok((checked, tag))
}

fn replace(releases: &dyn Releases, binary: &dyn Binary, tag: &str) -> Result<String, Failure> {
    let asset = binary.asset().ok_or_else(|| {
        let unbuilt = "no release is built for this operating system and architecture";
        Failure::Refused(unbuilt.to_owned())
    })?;
    let checksum = releases.fetch(tag, &format!("{asset}.sha256"))?;
    let bytes = releases.fetch(tag, &asset)?;
    verify(&bytes, &String::from_utf8_lossy(&checksum)).map_err(|why| Failure::at(&asset, why))?;
    Ok(binary.replace(&bytes)?)
}

/// What an install wrote, the binary's own path first, and what the binary's refresh of this
/// host noted, with why that refresh stopped where it did not finish.
#[derive(Debug, PartialEq, Eq)]
pub struct Installed {
    pub written: Vec<String>,
    pub notes: Vec<String>,
    pub ended: Result<(), Failure>,
}

/// Puts the release under `tag` in the place of the binary and has the binary then in place
/// bring this host up to itself. The asset is the one built for the binary's target, refused
/// where none is, and its `.sha256` beside it must match before anything is replaced.
pub fn install(
    releases: &dyn Releases,
    binary: &dyn Binary,
    tag: &str,
) -> Result<Installed, Failure> {
    let replaced = replace(releases, binary, tag)?;
    let Refreshed {
        written,
        notes,
        ended,
    } = binary.refresh();
    Ok(Installed {
        written: [vec![replaced], written].concat(),
        notes,
        ended: ended.map_err(Failure::Store),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::refused;
    use crate::domain::ports::StoreError;
    use crate::domain::testing::{
        ASSET, BINARY, MemoryBinary, REFRESHED, build, published, version,
    };

    fn tag(releases: &dyn Releases, current: &str) -> String {
        check(releases, version(current).into()).unwrap().1
    }

    #[test]
    fn a_release_replaces_the_binary_with_the_asset_built_for_it_and_refreshes_nothing() {
        let releases = published("v0.3.0", ASSET, b"new", b"new");
        let binary = MemoryBinary::built_as(ASSET);
        let replaced = replace(&releases, &binary, &tag(&releases, "0.2.1"));
        assert_eq!(replaced, Ok(BINARY.to_owned()));
        assert_eq!(binary.replaced_with.borrow().as_deref(), Some(&b"new"[..]));
        assert_eq!(*binary.calls.borrow(), ["replace"]);
    }

    #[test]
    fn the_assets_are_asked_for_under_the_tag_as_the_release_spells_it() {
        for tag in ["v0.3.0", "0.3.0"] {
            let releases = published(tag, ASSET, b"new", b"new");
            let found = self::tag(&releases, "0.2.1");
            assert_eq!(found, tag);
            replace(&releases, &MemoryBinary::built_as(ASSET), &found).unwrap();
            assert_eq!(releases.fetched.take().as_deref(), Some(tag));
        }
    }

    #[test]
    fn an_install_replaces_the_binary_and_then_has_it_refresh_whatever_the_refresh_comes_to() {
        let releases = published("v0.3.0", ASSET, b"new", b"new");
        let tag = tag(&releases, "0.2.1");
        let binary = MemoryBinary::built_as(ASSET);
        let written = vec![BINARY.to_owned(), REFRESHED.to_owned()];
        let installed = Installed {
            written: written.clone(),
            notes: Vec::new(),
            ended: Ok(()),
        };
        assert_eq!(install(&releases, &binary, &tag), Ok(installed));
        assert_eq!(*binary.calls.borrow(), ["replace", "refresh"]);
        let stopped = StoreError::io("lantern refresh", "no agents here");
        let failing = MemoryBinary {
            stops: Some(stopped.clone()),
            notes: vec!["fish left out".to_owned()],
            ..MemoryBinary::built_as(ASSET)
        };
        let installed = Installed {
            written,
            notes: vec!["fish left out".to_owned()],
            ended: Err(Failure::Store(stopped)),
        };
        assert_eq!(install(&releases, &failing, &tag), Ok(installed));
        assert_eq!(*failing.calls.borrow(), ["replace", "refresh"]);
    }

    #[test]
    fn a_checksum_that_does_not_match_or_a_target_with_no_release_replaces_nothing() {
        let tampered = published("v0.3.0", ASSET, b"new", b"other");
        let releases = published("v0.3.0", ASSET, b"new", b"new");
        let mut unsummed = published("v0.3.0", ASSET, b"new", b"new");
        unsummed.assets.remove(&format!("{ASSET}.sha256"));
        let missing = StoreError::io(format!("{ASSET}.sha256"), "no such asset");
        for (releases, asset, failure) in [
            (
                &tampered,
                Some(ASSET),
                Failure::at(ASSET, "the checksum does not match the release"),
            ),
            (
                &releases,
                None,
                Failure::Refused(
                    "no release is built for this operating system and architecture".to_owned(),
                ),
            ),
            (&unsummed, Some(ASSET), Failure::Store(missing)),
        ] {
            let binary = MemoryBinary {
                asset: asset.map(str::to_owned),
                ..MemoryBinary::default()
            };
            let stopped = replace(releases, &binary, &tag(releases, "0.2.1"));
            assert_eq!(stopped, Err(failure));
            assert_eq!(*binary.calls.borrow(), [""; 0]);
        }
    }

    #[test]
    fn a_build_is_replaced_only_by_a_release_of_a_higher_number_than_its_own() {
        let releases = published("v0.10.0", ASSET, b"", b"");
        for (current, newer, outcome) in [
            ("0.9.0", true, Outcome::Upgraded),
            ("0.10.0", false, Outcome::Ahead),
            ("0.11.0", false, Outcome::Ahead),
        ] {
            let (checked, _) = check(&releases, build(current)).unwrap();
            assert_eq!(checked.newer, newer, "{current}");
            assert_eq!(checked.outcome(), outcome, "{current}");
        }
    }

    #[test]
    fn a_check_gives_the_latest_release_beside_the_version_running() {
        let releases = published("v0.10.0", ASSET, b"", b"");
        for (current, newer, outcome) in [
            ("0.9.0", true, Outcome::Upgraded),
            ("0.10.0", false, Outcome::Current),
            ("0.11.0", false, Outcome::Ahead),
        ] {
            let (checked, tag) = check(&releases, version(current).into()).unwrap();
            assert_eq!(
                checked,
                Checked {
                    current: version(current).into(),
                    latest: version("0.10.0"),
                    newer,
                }
            );
            assert_eq!(tag, "v0.10.0");
            assert_eq!(checked.outcome(), outcome, "{current}");
            assert_eq!(
                checked.upgrade(vec![BINARY.to_owned()]),
                Upgrade {
                    current: version(current).into(),
                    latest: version("0.10.0"),
                    outcome,
                    written: vec![BINARY.to_owned()],
                }
            );
        }
        let untagged = published("nightly", ASSET, b"", b"");
        assert_eq!(
            refused(check(&untagged, version("0.2.1").into())),
            "`nightly` is not a version of the form X.Y.Z"
        );
    }
}
