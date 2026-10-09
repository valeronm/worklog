//! The ids a version file is found by: its document's and its own.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdError {
    NotADocumentId(String),
    NotAVersionId(String),
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdError::NotADocumentId(text) => write!(f, "`{text}` is not a document id"),
            IdError::NotAVersionId(text) => write!(f, "`{text}` is not a version id"),
        }
    }
}

pub(crate) fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Minted when a document is created and carried by every version of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(String);

impl DocumentId {
    const LEN: usize = 32;

    pub fn parse(text: &str) -> Result<DocumentId, IdError> {
        if is_hex(text, Self::LEN) {
            Ok(DocumentId(text.to_owned()))
        } else {
            Err(IdError::NotADocumentId(text.to_owned()))
        }
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; 16]) -> DocumentId {
        DocumentId(format!("{:032x}", u128::from_be_bytes(bytes)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn bucket(&self) -> &str {
        &self.0[..2]
    }

    #[must_use]
    pub fn short(&self) -> &str {
        &self.0[..8]
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The hash of a version file's bytes, behind the name of the algorithm.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VersionId(String);

impl VersionId {
    const ALGORITHM: &'static str = "b3-";
    const HEX: usize = 64;

    #[must_use]
    pub fn of(bytes: &[u8]) -> VersionId {
        VersionId(format!(
            "{}{}",
            Self::ALGORITHM,
            blake3::hash(bytes).to_hex()
        ))
    }

    pub fn parse(text: &str) -> Result<VersionId, IdError> {
        match text.strip_prefix(Self::ALGORITHM) {
            Some(hex) if is_hex(hex, Self::HEX) => Ok(VersionId(text.to_owned())),
            _ => Err(IdError::NotAVersionId(text.to_owned())),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The start of the hash, without the algorithm.
    #[must_use]
    pub fn short(&self) -> &str {
        &self.0[Self::ALGORITHM.len()..Self::ALGORITHM.len() + 12]
    }

    /// Whether the hash starts with the text, which may carry the
    /// algorithm before it; no hash starts with an empty text.
    #[must_use]
    pub fn starts_with(&self, prefix: &str) -> bool {
        let prefix = prefix.strip_prefix(Self::ALGORITHM).unwrap_or(prefix);
        !prefix.is_empty() && self.0[Self::ALGORITHM.len()..].starts_with(prefix)
    }
}

impl fmt::Display for VersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT: &str = "7f3a91c05be2446d8a10c3f29b7e6d54";

    #[test]
    fn a_document_id_is_thirty_two_lowercase_hex_characters() {
        let id = DocumentId::parse(DOCUMENT).unwrap();
        assert_eq!(id.as_str(), DOCUMENT);
        assert_eq!(id.to_string(), DOCUMENT);
        assert_eq!(id.bucket(), "7f");
        assert_eq!(id.short(), "7f3a91c0");
        for bad in [
            "",
            "7f3a91c0",
            &DOCUMENT.to_uppercase(),
            &DOCUMENT.replace('7', "g"),
            &format!("{DOCUMENT}0"),
        ] {
            assert_eq!(
                DocumentId::parse(bad),
                Err(IdError::NotADocumentId(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_document_id_is_its_bytes_in_hex() {
        let mut bytes = [0u8; 16];
        bytes[0] = 0x7f;
        bytes[15] = 0x0a;
        assert_eq!(
            DocumentId::from_bytes(bytes).as_str(),
            "7f00000000000000000000000000000a"
        );
    }

    #[test]
    fn a_version_id_names_its_hash() {
        let id = VersionId::of(b"lantern");
        let text = id.as_str();
        assert!(text.starts_with("b3-"), "{text}");
        assert_eq!(text.len(), 3 + 64);
        assert_eq!(VersionId::parse(text), Ok(id.clone()));
        assert_eq!(id.short(), &text[3..15]);
        assert_eq!(VersionId::of(b"lantern"), id);
        assert_ne!(VersionId::of(b"atlas"), id);
    }

    #[test]
    fn a_prefix_is_matched_against_the_hash() {
        let id = VersionId::of(b"lantern");
        let hash = &id.as_str()[3..];
        assert!(id.starts_with(&hash[..1]));
        assert!(id.starts_with(id.short()));
        assert!(id.starts_with(&format!("b3-{}", id.short())));
        assert!(id.starts_with(id.as_str()));
        for all in ["", "b3-"] {
            assert!(!id.starts_with(all), "{all}");
        }
        let other = if hash.starts_with('b') { "c" } else { "b" };
        assert!(!id.starts_with(other));
    }

    #[test]
    fn a_version_id_of_another_shape_is_refused() {
        let id = VersionId::of(b"lantern");
        let hex = &id.as_str()[3..];
        for bad in [
            hex.to_owned(),
            format!("b2-{hex}"),
            format!("b3-{}", hex.to_uppercase()),
            format!("b3-{}", &hex[1..]),
            String::new(),
        ] {
            assert_eq!(
                VersionId::parse(&bad),
                Err(IdError::NotAVersionId(bad.clone())),
                "{bad}"
            );
        }
    }
}
