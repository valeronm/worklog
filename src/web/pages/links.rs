//! The places a page links to and the pieces every page is built of.

use std::fmt::Write as _;

use askama::Template;

use crate::app::{Reference, Row};
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::release::{self, Built};
use crate::domain::version::Stamp;
use crate::render::{ended, written};
use crate::web::mark::EndedMark;
use crate::web::markdown::{Markup, marked, summary_html};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Topics,
    Entries,
    Facts,
    Ideas,
    Followups,
    Search,
    Log,
    Stats,
    Check,
    Forks,
}

impl Place {
    const FIRST: Place = Place::Topics;

    // A place no other is followed by, `FIRST` aside, has a route that answers 404.
    const fn following(self) -> Option<Place> {
        match self {
            Place::Topics => Some(Place::Entries),
            Place::Entries => Some(Place::Facts),
            Place::Facts => Some(Place::Ideas),
            Place::Ideas => Some(Place::Followups),
            Place::Followups => Some(Place::Search),
            Place::Search => Some(Place::Log),
            Place::Log => Some(Place::Stats),
            Place::Stats => Some(Place::Check),
            Place::Check => Some(Place::Forks),
            Place::Forks => None,
        }
    }

    fn all() -> impl Iterator<Item = Place> {
        std::iter::successors(Some(Place::FIRST), |place| place.following())
    }

    const IN_THE_NAVIGATION: [Place; 9] = [
        Place::Topics,
        Place::Entries,
        Place::Facts,
        Place::Ideas,
        Place::Followups,
        Place::Log,
        Place::Stats,
        Place::Check,
        Place::Forks,
    ];

    const fn titled(self) -> (&'static str, &'static str) {
        match self {
            Place::Topics => ("Topics", "/"),
            Place::Entries => ("Entries", "/entries"),
            Place::Facts => ("Facts", "/facts"),
            Place::Ideas => ("Ideas", "/ideas"),
            Place::Followups => ("Follow-ups", "/followups"),
            Place::Search => ("Search", "/search"),
            Place::Log => ("Log", "/log"),
            Place::Stats => ("Stats", "/stats"),
            Place::Check => ("Check", "/check"),
            Place::Forks => ("Forks", "/forks"),
        }
    }

    pub const fn title(self) -> &'static str {
        self.titled().0
    }

    pub const fn route(self) -> &'static str {
        self.titled().1
    }

    pub fn at(path: &str) -> Option<Place> {
        let wanted = path.trim_end_matches('/');
        Place::all().find(|place| place.route().trim_end_matches('/') == wanted)
    }

    pub(super) fn links(listed: &[Place], current: Option<Place>) -> Vec<NavLink> {
        (listed.iter())
            .map(|place| NavLink {
                text: place.title(),
                href: place.route(),
                current: current == Some(*place),
            })
            .collect()
    }
}

// What follows it in a path names a document or a version, by anything `show` resolves.
pub const DOC: &str = "/doc/";
// What follows it in a path names a document.
pub const HISTORY: &str = "/history/";

pub struct Link {
    pub text: String,
    pub href: String,
    pub mark: Option<EndedMark>,
}

impl Link {
    pub(super) fn plain(text: String, href: String) -> Link {
        Link {
            text,
            href,
            mark: None,
        }
    }

    pub(super) fn to(document: &DocumentId, label: &str, ended: Option<&str>) -> Link {
        Link {
            text: label.to_owned(),
            href: doc_href(document),
            mark: EndedMark::of(ended),
        }
    }
}

pub enum Text {
    Plain(String),
    Link(Link),
}

impl Text {
    pub(super) fn plain(text: &str) -> Text {
        Text::Plain(text.to_owned())
    }

    pub(super) fn naming(document: &Reference) -> Text {
        Text::Link(doc_link(document))
    }

    pub fn link(&self) -> Option<&Link> {
        match self {
            Text::Plain(_) => None,
            Text::Link(link) => Some(link),
        }
    }

    pub fn words(&self) -> &str {
        match self {
            Text::Plain(words) => words,
            Text::Link(link) => &link.text,
        }
    }
}

pub struct NavLink {
    pub text: &'static str,
    pub href: &'static str,
    pub current: bool,
}

pub struct Layout {
    pub home: &'static str,
    pub search: &'static str,
    pub places: Vec<NavLink>,
    pub version: Built,
}

impl Layout {
    pub(super) fn at(current: Option<Place>) -> Layout {
        Layout {
            home: Place::Topics.route(),
            search: Place::Search.route(),
            places: Place::links(&Place::IN_THE_NAVIGATION, current),
            version: release::current(),
        }
    }
}

/// `route` with each parameter given a value, in the order given, each value encoded.
pub fn with_query(route: &str, parameters: &[(&str, Option<&str>)]) -> String {
    let mut href = route.to_owned();
    let mut sign = '?';
    for (key, value) in parameters {
        if let Some(value) = value {
            let _ = write!(href, "{sign}{key}={}", encoded(value));
            sign = '&';
        }
    }
    href
}

pub fn encoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

pub(super) fn doc_href(document: &DocumentId) -> String {
    format!("{DOC}{document}")
}

pub(super) fn history_href(document: &DocumentId) -> String {
    format!("{HISTORY}{document}")
}

pub(super) fn version_link(version: &VersionId) -> Link {
    Link::plain(version.short().to_owned(), format!("{DOC}{version}"))
}

pub(super) fn doc_link(document: &Reference) -> Link {
    Link::to(&document.document, &document.label, document.ended())
}

pub(super) fn doc_links(documents: &[Reference]) -> Vec<Link> {
    documents.iter().map(doc_link).collect()
}

pub(super) fn marks(forked: bool, ended_for: Option<&str>) -> Vec<String> {
    let forked = forked.then(|| "forked".to_owned());
    forked.into_iter().chain(ended_for.map(ended)).collect()
}

pub struct Line {
    pub title: Link,
    /// The title's text as HTML.
    pub label: Markup,
    pub summary: Markup,
    pub counts: Vec<String>,
    pub marks: Vec<String>,
}

impl From<&Row> for Line {
    fn from(row: &Row) -> Line {
        Line {
            title: Link::to(&row.document, &row.label, row.ended.as_deref()),
            label: marked(&row.label, &[]),
            summary: summary_html(&row.summary, None),
            counts: Vec::new(),
            marks: marks(row.forked, row.ended.as_deref()),
        }
    }
}

pub struct Stamped {
    pub version: Link,
    pub head: bool,
    pub written: String,
    pub written_full: String,
    pub machine: Link,
    pub change: Option<String>,
    pub ended: Option<String>,
}

impl Stamped {
    pub(super) fn new(version: &VersionId, at: &Stamp, machine: &Reference) -> Stamped {
        Stamped {
            version: version_link(version),
            head: false,
            written: written(at),
            written_full: at.to_string(),
            machine: doc_link(machine),
            change: None,
            ended: None,
        }
    }

    /// A reopening writes its reason into the change after a colon.
    pub fn change_cut(&self) -> Option<(&str, &str)> {
        let whole = self.change.as_deref()?;
        let (word, _) = whole.split_once(':')?;
        Some((word, whole))
    }
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorPage {
    pub layout: Layout,
    pub status: u16,
    pub message: String,
}

impl ErrorPage {
    pub fn new(status: u16, message: String) -> ErrorPage {
        ErrorPage {
            layout: Layout::at(None),
            status,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_is_found_at_its_route() {
        let places: Vec<Place> = Place::all().collect();
        for place in &places {
            assert_eq!(Place::at(place.route()), Some(*place), "{place:?}");
        }
        for listed in Place::IN_THE_NAVIGATION {
            assert!(places.contains(&listed), "{listed:?}");
        }
    }

    #[test]
    fn a_link_escapes_what_is_not_a_plain_character() {
        assert_eq!(encoded("lantern-2.b_c~"), "lantern-2.b_c~");
        assert_eq!(encoded("a b/é?&=%+"), "a%20b%2F%C3%A9%3F%26%3D%25%2B");
        assert_eq!(encoded(""), "");
    }
}
