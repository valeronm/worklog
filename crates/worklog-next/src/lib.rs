//! Documents kept as chains of immutable version files, each under a path
//! made only of ids.
//!
//! `domain` holds the model and its rules and touches nothing outside
//! memory. `app` holds one use case per command over the ports the domain declares.
//! `wiring` builds the adapters a front end runs them over and keeps to itself `fs`, which
//! implements the ports on a directory tree, and `cli` parses a command line and renders
//! what its use case returns. `web` renders what the read use cases return as pages, and
//! `render` holds what `cli` and `web` show the same way.

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
pub mod render;
pub mod web;
pub mod wiring;
