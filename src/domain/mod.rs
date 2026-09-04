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
pub mod version;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
