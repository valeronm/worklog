//! The store as read-only pages: a request is answered from what the read use cases return
//! for it, and nothing is kept between requests.

mod mark;
mod markdown;
mod pages;
mod query;
mod server;
#[cfg(test)]
mod tests;

use askama::Template;

use crate::app::show::{self, Shown, ShownDocument};
use crate::app::{Failure, ReadDeps, activity, check, list, search};
use crate::domain::schema::KindOf;

use pages::{
    CheckPage, DOC, DocumentPage, ErrorPage, ForksPage, HISTORY, Held, HistoryPage, Home, Listing,
    LogPage, Place, Scope, SearchPage, Section, StatsPage, TopicPage, VersionPage, shown_link,
};
use query::{Query, decoded};
pub use server::Server;

#[derive(Debug)]
pub struct Page {
    pub status: u16,
    pub html: String,
}

impl Page {
    fn ok(html: String) -> Page {
        Page { status: 200, html }
    }
}

enum Problem {
    NotFound(String),
    BadRequest(String),
    NotAllowed(String),
    Misdirected,
    Broken(String),
    Busy,
}

impl Problem {
    fn said(self) -> (u16, String) {
        match self {
            Problem::BadRequest(message) => (400, message),
            Problem::NotFound(message) => (404, message),
            Problem::NotAllowed(message) => (405, message),
            Problem::Misdirected => (
                421,
                "the request names a host this server does not answer as".to_owned(),
            ),
            Problem::Broken(message) => (500, message),
            Problem::Busy => (
                503,
                "too many responses are still being read; ask again".to_owned(),
            ),
        }
    }

    fn page(self) -> Page {
        let (status, message) = self.said();
        let html = ErrorPage::new(status, message)
            .render()
            .unwrap_or_else(|_| "the page could not be rendered".to_owned());
        Page { status, html }
    }
}

impl From<Failure> for Problem {
    fn from(failure: Failure) -> Problem {
        match failure {
            Failure::Refused(text) => Problem::NotFound(text),
            Failure::Usage(text) => Problem::BadRequest(text),
            Failure::Store(error) => Problem::Broken(error.to_string()),
        }
    }
}

impl From<askama::Error> for Problem {
    fn from(error: askama::Error) -> Problem {
        Problem::Broken(error.to_string())
    }
}

/// The error page for `failure`: a 404 for a refusal, a 400 for a usage failure and a 500 for
/// a store failure.
#[must_use]
pub fn failed(failure: Failure) -> Page {
    Problem::from(failure).page()
}

fn undecodable(text: &str) -> Problem {
    Problem::BadRequest(format!("{text}: an escape in it does not decode"))
}

const LONGEST_TARGET: usize = 8192;

/// The page for a request, whatever its method and target are: `target` is the request line's
/// `/path?query`, and a method other than `GET` and `HEAD` is answered 405.
#[must_use]
pub fn respond(deps: &ReadDeps, method: &str, target: &str) -> Page {
    answer(deps, method, target).unwrap_or_else(Problem::page)
}

fn answer(deps: &ReadDeps, method: &str, target: &str) -> Result<Page, Problem> {
    if !matches!(method, "GET" | "HEAD") {
        return Err(Problem::NotAllowed(format!(
            "{method}: only GET and HEAD are answered"
        )));
    }
    if target.len() > LONGEST_TARGET {
        return Err(Problem::BadRequest("the address is too long".to_owned()));
    }
    if !target.starts_with('/') {
        return Err(Problem::BadRequest(format!("{target}: not a path")));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = decoded(path, false).ok_or_else(|| undecodable(path))?;
    route(deps, &path, &Query::parse(query).map_err(undecodable)?)
}

fn topic_page(deps: &ReadDeps, document: &ShownDocument, query: &Query) -> Result<String, Problem> {
    let section = match query.get("show") {
        None => Section::FIRST,
        Some(show) => Section::named(show)
            .ok_or_else(|| Problem::BadRequest(format!("show: {show} is no section of a topic")))?,
    };
    let id = document.document.to_string();
    let topic = Some(id.as_str());
    let held = Held {
        parts: &list::topics(deps, topic, false)?,
        facts: &list::facts(deps, topic, false)?,
        ideas: &list::ideas(deps, topic, false)?,
        followups: &list::followups(deps, topic, false)?,
        entries: &list::entries(deps, topic, None, None, false)?,
        claims: &list::claims_on(deps, &document.document)?,
    };
    Ok(TopicPage::new(document, &held, section).render()?)
}

fn shown(deps: &ReadDeps, target: &str, query: &Query) -> Result<String, Problem> {
    match show::show(deps, target)? {
        Shown::Version(compared) => Ok(VersionPage::new(&compared).render()?),
        Shown::Document(document) if document.reads_as == Some(KindOf::Topic) => {
            topic_page(deps, &document, query)
        }
        Shown::Document(mut document) => {
            document.linked_from = show::linked_from(deps, &document.document, true)?;
            Ok(DocumentPage::new(&document).render()?)
        }
    }
}

fn searched(deps: &ReadDeps, query: &Query) -> Result<Page, Problem> {
    let asked = search::Query {
        term: query.get("q").unwrap_or_default(),
        regex: query.get("regex") == Some("1"),
        topic: given(query.get("topic")),
        ended: query.get("ended") == Some("1"),
    };
    if asked.term.trim().is_empty() {
        return Ok(Page::ok(SearchPage::form(&asked).render()?));
    }
    match search::search(deps, &asked).map_err(Problem::from) {
        Ok(hits) => {
            let under = (asked.topic)
                .map(|target| show::show(deps, target))
                .transpose()?;
            let under = under.as_ref().map(|shown| shown_link(shown).href);
            Ok(Page::ok(
                SearchPage::found(&asked, &hits, under.as_deref()).render()?,
            ))
        }
        Err(broken @ Problem::Broken(_)) => Err(broken),
        Err(problem) => {
            let (status, why) = problem.said();
            let html = SearchPage::failed(&asked, why).render()?;
            Ok(Page { status, html })
        }
    }
}

const LOGGED: usize = 100;
const MOST_LOGGED: usize = 500;

fn logged(deps: &ReadDeps, query: &Query) -> Result<String, Problem> {
    let limit = match query.get("n") {
        None => LOGGED,
        Some(n) => (n.parse().ok())
            .filter(|n| (1..=MOST_LOGGED).contains(n))
            .ok_or_else(|| Problem::BadRequest(format!("n: a number from 1 to {MOST_LOGGED}")))?,
    };
    Ok(LogPage::new(&search::log(deps, limit, None)?).render()?)
}

fn year(query: &Query) -> Result<Option<i32>, Problem> {
    (query.get("year"))
        .map(|year| {
            (year.parse().ok())
                .ok_or_else(|| Problem::BadRequest(format!("year: {year} is no year")))
        })
        .transpose()
}

// A form sends a field left empty.
fn given(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

fn route(deps: &ReadDeps, path: &str, query: &Query) -> Result<Page, Problem> {
    let page = path.trim_end_matches('/');
    if let Some(target) = page.strip_prefix(DOC) {
        return shown(deps, target, query).map(Page::ok);
    }
    if let Some(target) = page.strip_prefix(HISTORY) {
        return Ok(Page::ok(
            HistoryPage::new(&show::history(deps, target)?).render()?,
        ));
    }
    let topic = given(query.get("topic"));
    let on = given(query.get("on"));
    let ended = query.get("ended") == Some("1");
    // Asked after the listing, whose refusal of the target is the one to show.
    let scope = || -> Result<Scope, Problem> {
        let under = topic.map(|target| show::show(deps, target)).transpose()?;
        Ok(Scope {
            topic,
            under: under.as_ref().map(shown_link),
            on,
            ended,
        })
    };
    let Some(place) = Place::at(page) else {
        return Err(Problem::NotFound(format!("no page at {path}")));
    };
    let html = match place {
        Place::Topics => Home::new(&list::topics(deps, None, false)?).render()?,
        Place::Stats => StatsPage::new(&activity::stats(deps, topic, year(query)?)?).render()?,
        Place::Facts => Listing::facts(&list::facts(deps, topic, ended)?, scope()?).render()?,
        Place::Ideas => Listing::ideas(&list::ideas(deps, topic, ended)?, scope()?).render()?,
        Place::Entries => {
            Listing::entries(&list::entries(deps, topic, on, None, ended)?, scope()?).render()?
        }
        Place::Followups => {
            Listing::followups(&list::followups(deps, topic, ended)?, scope()?).render()?
        }
        Place::Search => return searched(deps, query),
        Place::Log => logged(deps, query)?,
        Place::Check => CheckPage::new(&check::check(deps)?).render()?,
        Place::Forks => ForksPage::new(&list::forks(deps)?).render()?,
    };
    Ok(Page::ok(html))
}
