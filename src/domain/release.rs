//! A published release of this binary: the version its tag names, the asset built for a
//! target, and the checksum an asset must match.

use std::fmt;

use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};

/// `vX.Y.Z` in a tag and `X.Y.Z` in a manifest, ordered part by part.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion(u32, u32, u32);

impl ReleaseVersion {
    pub fn parse(text: &str) -> Result<ReleaseVersion, String> {
        let digits = text.strip_prefix('v').unwrap_or(text);
        // `u32` reads a sign, which no version carries.
        let part = |part: &str| {
            let plain = part.bytes().all(|byte| byte.is_ascii_digit());
            plain.then(|| part.parse::<u32>().ok()).flatten()
        };
        let mut parts = digits.split('.').map(part);
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) => {
                Ok(ReleaseVersion(major, minor, patch))
            }
            _ => Err(format!("`{text}` is not a version of the form X.Y.Z")),
        }
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl Serialize for ReleaseVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// The version this binary was built as.
///
/// # Panics
///
/// On a manifest version that is not `X.Y.Z`, which is a fact of the build and of no run.
#[must_use]
pub fn current() -> ReleaseVersion {
    ReleaseVersion::parse(env!("CARGO_PKG_VERSION")).expect("the manifest version is X.Y.Z")
}

/// The release asset of the binary `name` built for the target this one was compiled for, or
/// `None` where no release is built.
#[must_use]
pub fn asset(name: &str) -> Option<String> {
    let target = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-linux"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-linux"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-darwin"
    } else {
        return None;
    };
    Some(format!("{name}-{target}"))
}

#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn verify(bytes: &[u8], checksum: &str) -> Result<(), String> {
    let published = checksum
        .split_whitespace()
        .next()
        .ok_or("the checksum file is empty")?;
    if sha256_hex(bytes) == published {
        Ok(())
    } else {
        Err("the checksum does not match the release".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::checksum_file;

    #[test]
    fn a_version_parses_with_or_without_the_tag_prefix_and_orders_by_part() {
        let version = ReleaseVersion::parse("v0.2.10").unwrap();
        assert_eq!(version, ReleaseVersion::parse("0.2.10").unwrap());
        assert_eq!(version.to_string(), "0.2.10");
        assert!(ReleaseVersion::parse("0.2.9").unwrap() < version);
        assert!(version < ReleaseVersion::parse("0.3.0").unwrap());
        assert!(version < ReleaseVersion::parse("1.0.0").unwrap());
        for bad in [
            "0.2",
            "0.2.1.0",
            "v0.2.x",
            "0.2.1-rc1",
            "",
            "vv0.2.1",
            "0.+2.1",
        ] {
            assert_eq!(
                ReleaseVersion::parse(bad),
                Err(format!("`{bad}` is not a version of the form X.Y.Z")),
            );
        }
        assert_eq!(current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn an_asset_is_named_after_the_binary_and_the_target_it_was_built_for() {
        let built = [
            "lantern-x86_64-linux",
            "lantern-aarch64-linux",
            "lantern-aarch64-darwin",
        ];
        let named = asset("lantern");
        assert!(
            named.as_deref().is_none_or(|name| built.contains(&name)),
            "{named:?}"
        );
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(named.as_deref(), Some("lantern-x86_64-linux"));
            assert_eq!(asset("atlas").as_deref(), Some("atlas-x86_64-linux"));
        }
    }

    #[test]
    fn a_checksum_file_holds_the_digest_of_the_bytes_and_only_they_match_it() {
        let bytes = b"a binary";
        let file = checksum_file("lantern-x86_64-linux", bytes);
        assert_eq!(
            checksum_file("lantern-x86_64-linux", b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  \
             lantern-x86_64-linux\n"
        );
        assert_eq!(verify(bytes, &file), Ok(()));
        assert_eq!(
            verify(b"another", &file),
            Err("the checksum does not match the release".to_owned())
        );
        assert_eq!(
            verify(bytes, "\n"),
            Err("the checksum file is empty".to_owned())
        );
    }
}
