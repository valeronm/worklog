//! What a document's fields mean: the kinds, how one ends, which step
//! from a parent is allowed, how one is addressed. Nothing here asks a
//! store anything.

pub mod address;
pub mod ending;
pub mod error;
pub mod field;
pub mod graph;
pub mod kind;
pub mod links;
pub mod step;
pub mod translate;

#[cfg(test)]
mod testing;

pub use address::{Address, Candidate, Found, Holds};
pub use ending::{Ending, Reason};
pub use error::SchemaError;
pub use field::{Date, Name};
pub use kind::{Claim, Content, Entry, Fact, Followup, KindOf, Record, Topic, Trigger};
