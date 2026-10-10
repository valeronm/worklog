//! Documents kept as chains of immutable version files, each under a path
//! made only of ids.
//!
//! `domain` holds the model and its rules and touches nothing outside
//! memory; `fs` implements the ports the domain declares on a directory
//! tree. `app` holds one use case per command over those ports, `wiring` builds the `fs`
//! adapters a front end runs them over, and `cli` parses a command line and renders what
//! its use case returns.

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
pub mod fs;
pub mod wiring;
