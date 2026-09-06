//! A published release of this binary: the version its tag names, the
//! asset built for a target, and the checksum an asset must match.

use std::fmt;

use sha2::{Digest, Sha256};

/// A release version, as a tag `vX.Y.Z` or a manifest `X.Y.Z` spells it.
/// Releases are plain triples, so ordering is by component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion(u32, u32, u32);

impl ReleaseVersion {
    pub fn parse(text: &str) -> Result<ReleaseVersion, String> {
        let mut parts = text
            .strip_prefix('v')
            .unwrap_or(text)
            .split('.')
            .map(|part| part.parse::<u32>().ok());
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

/// The version this binary was built as.
///
/// # Panics
/// On a manifest version that is not `X.Y.Z`, which is a fact of the
/// build rather than of any run.
#[must_use]
pub fn current() -> ReleaseVersion {
    ReleaseVersion::parse(env!("CARGO_PKG_VERSION")).expect("the manifest version is X.Y.Z")
}

/// The release asset built for the target this binary was compiled for,
/// or `None` where no release is built.
#[must_use]
pub const fn asset() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("worklog-x86_64-linux")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("worklog-aarch64-linux")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("worklog-aarch64-darwin")
    } else {
        None
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The `.sha256` file for an asset, `<hex>  <name>` as `shasum` writes it.
#[must_use]
pub fn checksum_file(asset: &str, bytes: &[u8]) -> String {
    format!("{}  {asset}\n", hex_digest(bytes))
}

/// Whether the bytes are what a `.sha256` file says the asset is.
pub fn verify(bytes: &[u8], checksum: &str) -> Result<(), String> {
    let published = checksum
        .split_whitespace()
        .next()
        .ok_or("the checksum file is empty")?;
    if hex_digest(bytes) == published {
        Ok(())
    } else {
        Err("the checksum does not match the release".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_with_or_without_the_tag_prefix_and_order_by_part() {
        let v = ReleaseVersion::parse("v0.2.10").unwrap();
        assert_eq!(v, ReleaseVersion::parse("0.2.10").unwrap());
        assert_eq!(v.to_string(), "0.2.10");
        assert!(ReleaseVersion::parse("0.2.9").unwrap() < v);
        assert!(v < ReleaseVersion::parse("0.3.0").unwrap());
        assert!(v < ReleaseVersion::parse("1.0.0").unwrap());
        for bad in ["0.2", "0.2.1.0", "v0.2.x", "0.2.1-rc1", ""] {
            assert!(ReleaseVersion::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_checksum_line_names_the_bytes() {
        let bytes = b"a binary";
        let line = checksum_file("worklog-x86_64-linux", bytes);
        assert!(line.ends_with("  worklog-x86_64-linux\n"), "{line}");
        assert_eq!(verify(bytes, &line), Ok(()));
        assert!(verify(b"another", &line).is_err());
        assert!(verify(bytes, "\n").is_err());
    }
}
