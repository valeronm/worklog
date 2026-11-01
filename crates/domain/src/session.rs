//! The agent session a command runs in.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub agent: String,
    pub id: String,
}
