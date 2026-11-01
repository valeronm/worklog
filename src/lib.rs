//! Documents kept as chains of immutable version files, each under a path
//! made only of ids.
//!
//! `domain` holds the model and its rules and touches nothing outside
//! memory. `app` holds one use case per command over the ports the domain declares.
//! `wiring` builds the adapters a front end runs them over and keeps to itself `fs`, which
//! implements the ports on a directory tree, and `net`, the one port that reaches the
//! network. `cli` parses a command line and renders what its use case returns. `web` renders
//! what the read use cases return as pages, and `render` holds what `cli` and `web` show the
//! same way.

pub mod app;
pub mod cli;
pub use worklog_domain as domain;
pub mod render;
pub mod web;
pub mod wiring;
