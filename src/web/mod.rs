//! The store as read-only pages. A request is one read use case and a
//! template over its output, nothing kept between requests, so a page
//! shows what a command run at that moment would.

pub mod markdown;
pub mod pages;

use askama::Template;

use crate::app::output::Search;
use crate::app::{Deps, Failure, read};
use crate::domain::slug::{Kind, Slug};

use pages::{
    CheckPage, DocPage, ErrorPage, FactsPage, FollowupsPage, ForksPage, HistoryPage, Home,
    ListingPage, LogPage, SearchPage, TagsPage, TopicPage, VersionPage,
};

pub struct Page {
    pub status: u16,
    pub html: String,
}

enum Problem {
    NotFound(String),
    BadRequest(String),
    Broken(String),
}

impl From<Failure> for Problem {
    fn from(e: Failure) -> Problem {
        match e {
            Failure::Refused(text) => Problem::NotFound(text),
            Failure::Usage(text) => Problem::BadRequest(text),
            Failure::Store(e) => Problem::Broken(e.to_string()),
        }
    }
}

impl From<askama::Error> for Problem {
    fn from(e: askama::Error) -> Problem {
        Problem::Broken(e.to_string())
    }
}

/// The query string as pairs, `+` and percent escapes undone.
struct Query(Vec<(String, String)>);

impl Query {
    fn parse(text: &str) -> Query {
        Query(
            text.split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                    (decode(key), decode(value))
                })
                .collect(),
        )
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = text.get(i + 1..i + 3).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The page for a request target, `/path?query`, whatever it names.
#[must_use]
pub fn respond(deps: &Deps, target: &str) -> Page {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = Query::parse(query);
    match route(deps, path, &query) {
        Ok(html) => Page { status: 200, html },
        Err(problem) => {
            let (status, message) = match problem {
                Problem::NotFound(m) => (404, m),
                Problem::BadRequest(m) => (400, m),
                Problem::Broken(m) => (500, m),
            };
            let page = ErrorPage { status, message };
            let html = page.render().unwrap_or(page.message);
            Page { status, html }
        }
    }
}

fn route(deps: &Deps, path: &str, query: &Query) -> Result<String, Problem> {
    let path = path.trim_end_matches('/');
    if let Some(name) = path.strip_prefix("/topic/") {
        return topic(deps, name);
    }
    if let Some(tag) = path.strip_prefix("/tag/") {
        let out = read::tag(deps, tag)?;
        let topics = read::topics(deps)?;
        return Ok(ListingPage::tagged(tag, &out, &topics).render()?);
    }
    if let Some(rest) = path.strip_prefix("/doc/") {
        return match rest.strip_suffix("/history") {
            Some(slug) => {
                let slug = Slug::parse(slug).map_err(Failure::from)?;
                Ok(HistoryPage::from(&read::history(deps, &slug)?).render()?)
            }
            None => Ok(DocPage::from(&read::show(deps, rest, None)?).render()?),
        };
    }
    if let Some(id) = path.strip_prefix("/version/") {
        return version(deps, id);
    }
    match path {
        "" => home(deps),
        "/entries" => Ok(ListingPage::entries(&read::list(deps, Kind::Entry)?).render()?),
        "/facts" => Ok(FactsPage::from(&read::facts(deps, None, false)?).render()?),
        "/followups" => {
            let all = query.get("all").is_some();
            Ok(FollowupsPage::new(&read::followups(deps, None, all)?, all).render()?)
        }
        "/tags" => Ok(TagsPage::new(&read::tags(deps)?, &read::topics(deps)?).render()?),
        "/search" => {
            let term = query.get("q").unwrap_or("").trim();
            if term.is_empty() {
                return Ok(SearchPage::from(&Search::default()).render()?);
            }
            Ok(SearchPage::from(&read::search(deps, term, false)?).render()?)
        }
        "/log" => Ok(LogPage::from(&read::log(deps, 100, None)?).render()?),
        "/check" => Ok(CheckPage::from(&read::check(deps)?).render()?),
        "/forks" => Ok(ForksPage::from(&read::forks(deps)?).render()?),
        _ => Err(Problem::NotFound(format!("no page at {path}"))),
    }
}

fn home(deps: &Deps) -> Result<String, Problem> {
    let topics = read::topics(deps)?;
    let open = read::followups(deps, None, false)?;
    let forks = read::forks(deps)?;
    Ok(Home::new(&topics, &open, &forks).render()?)
}

fn topic(deps: &Deps, name: &str) -> Result<String, Problem> {
    let shown = read::show(deps, name, Some(Kind::Topic))?;
    let facts = read::facts(deps, Some(name), false)?;
    let tagged = read::tag(deps, name)?;
    let open = read::followups(deps, Some(name), false)?;
    Ok(TopicPage::new(&shown, &facts, &tagged, &open).render()?)
}

fn version(deps: &Deps, id: &str) -> Result<String, Problem> {
    let shown = read::show(deps, id, None)?;
    let diff = read::diff(deps, id, None, None)?;
    let head = shown
        .heads
        .first()
        .ok_or_else(|| Problem::NotFound(format!("{id} names no version")))?;
    Ok(VersionPage::new(&shown.slug, head, &diff).render()?)
}

/// A bound socket, answering one request at a time on the calling thread.
pub struct Server {
    inner: tiny_http::Server,
}

impl Server {
    pub fn bind(addr: &str) -> Result<Server, Failure> {
        let inner = tiny_http::Server::http(addr)
            .map_err(|e| Failure::Refused(format!("cannot listen on {addr}: {e}")))?;
        Ok(Server { inner })
    }

    /// Where it listens, with the port the system chose for a `:0`.
    #[must_use]
    pub fn address(&self) -> String {
        self.inner
            .server_addr()
            .to_ip()
            .map_or_else(|| "?".to_owned(), |a| a.to_string())
    }

    /// Serves until the process ends.
    #[allow(
        clippy::missing_panics_doc,
        reason = "the one expect is on a header spelled as a literal"
    )]
    pub fn run(&self, deps: &Deps) {
        let html = tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
            .expect("a well-formed header");
        for request in self.inner.incoming_requests() {
            let page = match request.method() {
                tiny_http::Method::Get | tiny_http::Method::Head => respond(deps, request.url()),
                _ => Page {
                    status: 405,
                    html: String::from("only GET"),
                },
            };
            let response = tiny_http::Response::from_string(page.html)
                .with_status_code(page.status)
                .with_header(html.clone());
            // A client that hung up before the page was written is not
            // the server's problem.
            let _ = request.respond(response);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::World;
    use crate::app::write;

    fn seeded() -> World {
        let w = World::new("desk");
        let deps = w.deps();
        write::put_topic(
            &deps,
            "lantern",
            "A Rust app that dims a lamp",
            &[],
            Some("\nWhat to know first: see [[lantern/relay-pin-is-fixed]].\n"),
        )
        .unwrap();
        write::put_topic(&deps, "phone", "A test phone", &[], None).unwrap();
        write::put_fact(
            &deps,
            "lantern/relay-pin-is-fixed",
            "The relay pin is fixed on the board",
            &["lantern"],
            false,
        )
        .unwrap();
        write::put_fact(
            &deps,
            "phone/beta-builds",
            "Move the phone to beta",
            &["phone"],
            true,
        )
        .unwrap();
        write::put_entry(
            &deps,
            "2026-09/2026-09-01-lamp-driver",
            "2026-09-01",
            "Wired the lamp driver",
            &["lantern"],
        )
        .unwrap();
        write::put_followup(
            &deps,
            "2026-09-01-port",
            "2026-09/2026-09-01-lamp-driver",
            "Add the second relay",
            &["lantern"],
            Some("2026-09-03 the board is back"),
        )
        .unwrap();
        w
    }

    fn page(w: &World, target: &str) -> Page {
        respond(&w.deps(), target)
    }

    fn ok(w: &World, target: &str) -> String {
        let p = page(w, target);
        assert_eq!(p.status, 200, "{target}: {}", p.html);
        p.html
    }

    #[test]
    fn home_lists_topics_with_counts_and_what_is_due() {
        let w = seeded();
        let html = ok(&w, "/");
        assert!(
            html.contains("<a href=\"/topic/lantern\">lantern</a>"),
            "{html}"
        );
        assert!(html.contains("A Rust app that dims a lamp"));
        assert!(html.contains("Add the second relay"), "due item: {html}");
        assert!(html.contains("href=\"/doc/2026-09-01-port\""));
    }

    #[test]
    fn a_topic_page_shows_its_facts_entries_and_open_work() {
        let w = seeded();
        let html = ok(&w, "/topic/lantern");
        assert!(html.contains("The relay pin is fixed on the board"));
        assert!(html.contains("href=\"/doc/lantern/relay-pin-is-fixed\""));
        assert!(html.contains("Wired the lamp driver"));
        assert!(html.contains("href=\"/doc/2026-09/2026-09-01-lamp-driver\""));
        assert!(html.contains("Add the second relay"));
        assert!(
            html.contains("What to know first"),
            "the body renders: {html}"
        );
    }

    #[test]
    fn a_document_page_renders_fields_body_and_derived_followups() {
        let w = seeded();
        let html = ok(&w, "/doc/2026-09/2026-09-01-lamp-driver");
        assert!(html.contains("<h2>What</h2>"), "{html}");
        assert!(html.contains("href=\"/tag/lantern\""));
        assert!(
            html.contains("Add the second relay"),
            "derived follow-ups: {html}"
        );
        assert!(html.contains("/doc/2026-09/2026-09-01-lamp-driver/history"));
        let html = ok(&w, "/doc/2026-09-01-port");
        assert!(
            html.contains("href=\"/doc/2026-09/2026-09-01-lamp-driver\""),
            "the entry field links: {html}"
        );
    }

    #[test]
    fn history_and_version_pages_follow_the_chain() {
        let w = seeded();
        let html = ok(&w, "/doc/lantern/relay-pin-is-fixed/history");
        let start = html.find("href=\"/version/").expect("a version link") + "href=\"".len();
        let end = html[start..].find('"').unwrap() + start;
        let href = html[start..end].to_owned();
        let html = ok(&w, &href);
        assert!(
            html.contains("class=\"added\""),
            "a first version is all additions: {html}"
        );
        assert!(html.contains("The relay pin is fixed on the board"));
    }

    #[test]
    fn listings_search_and_the_rest_answer() {
        let w = seeded();
        assert!(ok(&w, "/entries").contains("Wired the lamp driver"));
        let facts = ok(&w, "/facts");
        assert!(facts.contains("relay-pin-is-fixed") && facts.contains("beta-builds"));
        assert!(ok(&w, "/followups").contains("Add the second relay"));
        assert!(ok(&w, "/followups?all=1").contains("Add the second relay"));
        assert!(ok(&w, "/tags").contains("href=\"/tag/lantern\""));
        assert!(ok(&w, "/tag/lantern").contains("Wired the lamp driver"));
        assert!(ok(&w, "/search?q=relay+pin").contains("relay-pin-is-fixed"));
        assert!(ok(&w, "/search").contains("<form"));
        assert!(ok(&w, "/log").contains("lantern/relay-pin-is-fixed"));
        assert!(ok(&w, "/check").contains("documents"));
        assert!(ok(&w, "/forks").contains("fork"));
    }

    #[test]
    fn a_missing_document_is_404_and_a_bad_slug_400() {
        let w = seeded();
        assert_eq!(page(&w, "/doc/lantern/nothing").status, 404);
        assert_eq!(page(&w, "/nowhere").status, 404);
        assert_eq!(page(&w, "/doc/not a slug").status, 400);
    }

    #[test]
    fn the_query_decodes_escapes() {
        assert_eq!(decode("a+b%20c%2Fd"), "a b c/d");
        assert_eq!(decode("100%"), "100%");
        let q = Query::parse("q=x&all");
        assert_eq!(q.get("q"), Some("x"));
        assert_eq!(q.get("all"), Some(""));
        assert_eq!(q.get("none"), None);
    }
}
