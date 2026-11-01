//! The agent session a command runs in.

use alloc::string::String;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub agent: String,
    pub id: String,
}
