//! What each page shows, built from a use case's result and handed to a template. Every link
//! is spelled here, so a template holds no routes.

mod document;
mod home;
mod links;
mod listing;
mod stats;
#[cfg(test)]
mod testing;
mod versions;

pub use document::{DocumentPage, Held, Section, TopicPage};
pub use home::Home;
pub use links::{DOC, ErrorPage, HISTORY, Place};
pub use listing::{CheckPage, Listing, Scope, SearchPage, shown_link};
pub use stats::StatsPage;
pub use versions::{ForksPage, HistoryPage, LogPage, VersionPage};
