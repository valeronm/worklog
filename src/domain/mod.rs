//! The model and its rules. Nothing here reads a file, a clock or the
//! environment: what a use case needs from outside arrives through `ports`.

pub mod draft;
pub mod entry;
pub mod fact;
pub mod followup;
pub mod frontmatter;
pub mod graph;
pub mod links;
pub mod machine;
pub mod ports;
pub mod recheck;
pub mod slug;
pub mod topic;
pub mod usage;
pub mod version;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// The fields a kind's front matter may carry.
#[must_use]
pub fn kind_keys(kind: slug::Kind) -> &'static [&'static str] {
    match kind {
        slug::Kind::Entry => &entry::KEYS,
        slug::Kind::Fact => &fact::KEYS,
        slug::Kind::Topic => &topic::KEYS,
        slug::Kind::Followup => &followup::KEYS,
    }
}
