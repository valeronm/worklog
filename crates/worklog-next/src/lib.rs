//! Documents kept as chains of immutable version files, each under a path
//! made only of ids.
//!
//! `domain` holds the model and its rules and touches nothing outside
//! memory. `app` holds one use case per command over the ports the domain declares.
//! `wiring` builds the adapters a front end runs them over and keeps to itself `fs`, which
//! implements the ports on a directory tree, and `cli` parses a command line and renders
//! what its use case returns.

#![allow(
    clippy::missing_errors_doc,
    reason = "every error type is an enum whose variants are the documentation"
)]
#![allow(
    clippy::module_name_repetitions,
    reason = "a `Version` in `version` and a `Document` in `document` read better than invented names"
)]

pub mod app;
pub mod cli;
pub mod domain;
pub mod wiring;
