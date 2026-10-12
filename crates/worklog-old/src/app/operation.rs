//! The commands that write a version, as the `operation` a block names;
//! a name outside `is_known` came from a newer worklog.

use crate::domain::version::{RENAME, TOMBSTONE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    New,
    Save,
    Done,
    Drop,
    Recheck,
    Verify,
    Tombstone,
    Rename,
    Resolve,
    Claim,
    Unclaim,
}

impl Operation {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Operation::New => "new",
            Operation::Save => "save",
            Operation::Done => "done",
            Operation::Drop => "drop",
            Operation::Recheck => "recheck",
            Operation::Verify => "verify",
            Operation::Tombstone => TOMBSTONE,
            Operation::Rename => RENAME,
            Operation::Resolve => "resolve",
            Operation::Claim => "claim",
            Operation::Unclaim => "unclaim",
        }
    }

    /// `migrate` heads documents an importer once wrote, and a version's
    /// bytes are never rewritten.
    #[must_use]
    pub fn is_known(name: &str) -> bool {
        matches!(
            name,
            "new"
                | "save"
                | "done"
                | "drop"
                | "recheck"
                | "verify"
                | TOMBSTONE
                | RENAME
                | "resolve"
                | "claim"
                | "unclaim"
                | "migrate"
        )
    }
}
