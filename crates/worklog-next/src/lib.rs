//! Documents kept as chains of immutable version files, each under a path
//! made only of ids.
//!
//! `domain` holds the model and its rules and touches nothing outside
//! memory; `fs` implements the ports the domain declares on a directory
//! tree. `app` holds one use case per command over those ports.

#![allow(
    clippy::missing_errors_doc,
    reason = "every error type is an enum whose variants are the documentation"
)]
#![allow(
    clippy::module_name_repetitions,
    reason = "a `Version` in `version` and a `Document` in `document` read better than invented names"
)]

pub mod app;
pub mod domain;
pub mod fs;
