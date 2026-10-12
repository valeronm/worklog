//! How a link to a document that has ended is marked, in a template and in a body alike.

use crate::render;

pub struct EndedMark {
    class: &'static str,
    title: String,
}

impl EndedMark {
    #[must_use]
    pub fn of(reason: Option<&str>) -> Option<EndedMark> {
        reason.map(|reason| EndedMark {
            class: "ended",
            title: render::ended(reason),
        })
    }

    #[must_use]
    pub fn class(&self) -> &'static str {
        self.class
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
}
