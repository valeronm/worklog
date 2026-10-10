//! The model and its rules. Nothing here reads a file, a clock or the
//! environment: what a use case needs from outside arrives through a port.

pub mod document;
pub mod draft;
pub mod fence;
pub mod id;
pub mod ports;
pub mod schema;
pub mod version;

#[cfg(any(test, feature = "testing"))]
pub mod testing;
