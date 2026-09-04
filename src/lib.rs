//! A store of work done, durable facts, follow-ups and topics, kept as
//! append-only versioned documents and read and written only through this
//! crate.
//!
//! `domain` holds the model and its rules and touches nothing outside memory;
//! `app` is one use case per command over the ports the domain declares;
//! `fs` implements those ports on a directory tree; `cli` parses arguments
//! and renders outputs.

#![allow(
    clippy::missing_errors_doc,
    reason = "every error type is an enum whose variants are the documentation"
)]
#![allow(
    clippy::module_name_repetitions,
    reason = "a `Slug` in `slug` and a `Version` in `version` read better than invented names"
)]

pub mod app;
pub mod cli;
pub mod domain;
pub mod fs;
