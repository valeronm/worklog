//! `upgrade`: put the latest release in the place of this binary when it
//! is newer, and have the new binary bring what this host takes from it
//! up to itself.

use std::cmp::Ordering;

use crate::domain::ports::{Binary, Releases};
use crate::domain::release::{ReleaseVersion, verify};

use super::Failure;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Current,
    /// The binary is newer than the latest release, as a build from
    /// source is.
    Ahead(ReleaseVersion),
    Upgraded(ReleaseVersion),
}

/// The latest release, as its tag and the version the tag names.
fn latest(releases: &dyn Releases) -> Result<(String, ReleaseVersion), Failure> {
    let tag = releases.latest()?;
    let version = ReleaseVersion::parse(&tag).map_err(Failure::Refused)?;
    Ok((tag, version))
}

/// The latest release's version.
pub fn check(releases: &dyn Releases) -> Result<ReleaseVersion, Failure> {
    latest(releases).map(|(_, version)| version)
}

/// Upgrades when the latest release is newer and returns what happened
/// with what was written, one path per line: the binary, then what the
/// new binary's refresh placed. Nothing is written when the binary stays,
/// and its own refresh is then the caller's.
pub fn run(
    releases: &dyn Releases,
    binary: &dyn Binary,
    current: ReleaseVersion,
    asset: Option<&str>,
) -> Result<(Outcome, String), Failure> {
    let (tag, latest) = latest(releases)?;
    match latest.cmp(&current) {
        Ordering::Greater => {
            let asset = asset.ok_or_else(|| {
                Failure::Refused(
                    "no release is built for this operating system and architecture".into(),
                )
            })?;
            let checksum = releases.fetch(&tag, &format!("{asset}.sha256"))?;
            let bytes = releases.fetch(&tag, asset)?;
            verify(&bytes, &String::from_utf8_lossy(&checksum))
                .map_err(|reason| Failure::Refused(format!("{asset}: {reason}")))?;
            let written = binary.replace(&bytes)? + "\n" + &binary.refresh()?;
            Ok((Outcome::Upgraded(latest), written))
        }
        Ordering::Equal => Ok((Outcome::Current, String::new())),
        Ordering::Less => Ok((Outcome::Ahead(latest), String::new())),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::release::checksum_file;
    use crate::domain::testing::{MemoryBinary, MemoryReleases};

    use super::*;

    const ASSET: &str = "worklog-x86_64-linux";

    fn release(tag: &str, bytes: &[u8], checksum_of: &[u8]) -> MemoryReleases {
        let mut assets = BTreeMap::new();
        assets.insert(ASSET.to_owned(), bytes.to_vec());
        assets.insert(
            format!("{ASSET}.sha256"),
            checksum_file(ASSET, checksum_of).into_bytes(),
        );
        MemoryReleases {
            latest: tag.to_owned(),
            assets,
        }
    }

    fn v(text: &str) -> ReleaseVersion {
        ReleaseVersion::parse(text).unwrap()
    }

    #[test]
    fn upgrades_only_to_a_newer_release_and_refreshes_only_then() {
        let binary = MemoryBinary::default();
        let (outcome, written) = run(
            &release("v0.3.0", b"new", b"new"),
            &binary,
            v("0.2.1"),
            Some(ASSET),
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Upgraded(v("0.3.0")));
        assert_eq!(binary.replaced_with.borrow().as_deref(), Some(&b"new"[..]));
        assert_eq!(
            written,
            "/home/u/.local/bin/worklog\n/home/u/.config/fish/completions/worklog.fish\n"
        );

        let binary = MemoryBinary::default();
        let (outcome, written) = run(
            &release("v0.3.0", b"new", b"new"),
            &binary,
            v("0.3.0"),
            Some(ASSET),
        )
        .unwrap();
        assert_eq!((outcome, written.as_str()), (Outcome::Current, ""));
        let (outcome, _) = run(
            &release("v0.3.0", b"new", b"new"),
            &binary,
            v("0.4.0"),
            Some(ASSET),
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Ahead(v("0.3.0")));
        assert!(binary.replaced_with.borrow().is_none());
        assert!(!binary.refreshed.get());
    }

    #[test]
    fn a_bad_checksum_or_target_replaces_nothing() {
        let binary = MemoryBinary::default();
        let err = run(
            &release("v0.3.0", b"new", b"tampered"),
            &binary,
            v("0.2.1"),
            Some(ASSET),
        )
        .unwrap_err();
        assert!(
            matches!(&err, Failure::Refused(m) if m.contains("checksum")),
            "{err}"
        );
        let err = run(
            &release("v0.3.0", b"new", b"new"),
            &binary,
            v("0.2.1"),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(&err, Failure::Refused(m) if m.contains("no release")),
            "{err}"
        );
        let err = check(&release("nightly", b"", b"")).unwrap_err();
        assert!(
            matches!(&err, Failure::Refused(m) if m.contains("not a version")),
            "{err}"
        );
        assert!(binary.replaced_with.borrow().is_none());
        assert!(!binary.refreshed.get());
    }
}
