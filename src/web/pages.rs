//! What each page shows, built from a use case's output and handed to a
//! template. Every link is spelled here, so a template holds no routes.

use askama::Template;

use crate::app::output::{
    Check, Count, Diff, FactListing, FollowupItem, Followups, Forks, Head, History, Listing, Log,
    Problem, Row, Search, Shown, Stamp, Tags, Topics, short,
};
use crate::domain::frontmatter::{self, Fields, Value};
use crate::domain::slug::{Kind, Slug};

use super::markdown;

pub struct Link {
    pub text: String,
    pub href: Option<String>,
}

impl Link {
    fn to(text: &str, href: String) -> Link {
        Link {
            text: text.to_owned(),
            href: Some(href),
        }
    }

    fn plain(text: &str) -> Link {
        Link {
            text: text.to_owned(),
            href: None,
        }
    }
}

/// A topic has a page of its own; every other document is shown by slug.
#[must_use]
pub fn doc_href(kind: &str, slug: &str) -> String {
    if kind == Kind::Topic.dir() {
        format!("/topic/{slug}")
    } else {
        format!("/doc/{slug}")
    }
}

fn history_href(slug: &str) -> String {
    format!("/doc/{slug}/history")
}

fn slug_link(slug: &str) -> Link {
    let kind = Slug::parse(slug).map_or("", |s| s.kind().dir());
    Link::to(slug, doc_href(kind, slug))
}

fn topic_link(name: &str) -> Link {
    Link::to(name, format!("/topic/{name}"))
}

fn tag_link(tag: &str) -> Link {
    Link::to(tag, format!("/tag/{tag}"))
}

fn version_link(id: &str) -> Link {
    Link::to(short(id), format!("/version/{id}"))
}

/// Where a document's pages sit: the nav entry its kind falls under, and
/// the trail between that section and the document, which is a fact's
/// topic and nothing for the other kinds.
struct Place {
    nav: &'static str,
    crumbs: Vec<Link>,
}

fn place(slug: &str) -> Place {
    let parsed = Slug::parse(slug).ok();
    let nav = match parsed.as_ref().map(Slug::kind) {
        Some(Kind::Entry) => "entries",
        Some(Kind::Followup) => "followups",
        _ => "topics",
    };
    let crumbs = parsed
        .as_ref()
        .and_then(|s| s.topic().map(topic_link))
        .into_iter()
        .collect();
    Place { nav, crumbs }
}

/// The topic named like a tag, when there is one.
fn topic_named(topics: &Topics, name: &str) -> Option<Link> {
    topics
        .topics
        .iter()
        .any(|t| t.slug == name)
        .then(|| topic_link(name))
}

pub struct Item {
    pub href: String,
    pub slug: String,
    pub date: String,
    pub summary: String,
    pub tags: Vec<Link>,
}

impl From<&Row> for Item {
    fn from(r: &Row) -> Item {
        Item {
            href: doc_href(&r.kind, &r.slug),
            slug: r.slug.clone(),
            date: r.date.clone(),
            summary: markdown::inline(&r.summary),
            tags: r.tags.iter().map(|t| tag_link(t)).collect(),
        }
    }
}

fn items(rows: &[Row]) -> Vec<Item> {
    rows.iter().map(Item::from).collect()
}

pub struct Open {
    pub href: String,
    pub slug: String,
    pub source: String,
    pub summary: String,
    pub label: String,
    pub due: bool,
    pub closed: bool,
    pub entry: Option<Link>,
}

impl From<&FollowupItem> for Open {
    fn from(i: &FollowupItem) -> Open {
        Open {
            href: doc_href("", &i.slug),
            slug: i.slug.clone(),
            source: i.source.clone(),
            summary: markdown::inline(&i.summary),
            label: i.label.clone(),
            due: i.due,
            closed: i.state.as_deref().is_some_and(|s| s != "open"),
            entry: i.entry.as_deref().map(slug_link),
        }
    }
}

fn opens(items: &[FollowupItem]) -> Vec<Open> {
    items.iter().map(Open::from).collect()
}

pub struct Field {
    pub key: String,
    pub values: Vec<Link>,
    /// Shown as chips rather than a comma list.
    pub chips: bool,
}

fn field_value(key: &str, text: &str) -> Link {
    match key {
        "entry" => slug_link(text),
        "tags" => tag_link(text),
        "includes" | "unclaimed" => topic_link(text),
        _ => Link::plain(text),
    }
}

fn fields(fields: &Fields) -> Vec<Field> {
    fields
        .iter()
        .map(|(key, value)| Field {
            key: key.to_owned(),
            chips: key == "tags",
            values: match value {
                Value::Scalar(s) => vec![field_value(key, s)],
                Value::List(items) => items.iter().map(|s| field_value(key, s)).collect(),
                Value::Map(entries) => entries
                    .iter()
                    .map(|(k, v)| {
                        let text = match v {
                            Value::Scalar(s) => s.clone(),
                            Value::List(items) => items.join(", "),
                            Value::Map(_) => String::new(),
                        };
                        Link::plain(&format!("{k}: {text}"))
                    })
                    .collect(),
            },
        })
        .collect()
}

/// One version's text as a page shows it: its summary as the title, the
/// other fields, then its body.
pub struct HeadView {
    pub short: String,
    pub href: String,
    pub written: String,
    pub written_full: String,
    pub machine: String,
    pub operation: String,
    /// The summary as written, for a window title.
    pub title: String,
    /// The summary as inline HTML.
    pub summary: String,
    pub fields: Vec<Field>,
    pub body: String,
}

impl From<&Head> for HeadView {
    fn from(h: &Head) -> HeadView {
        // A stored version passed the reader, so its text splits.
        let (title, fields_, body) = match frontmatter::parse(&h.text) {
            Ok(mut split) => {
                let title = split
                    .fields
                    .optional("summary")
                    .unwrap_or_default()
                    .to_owned();
                split.fields.remove("summary");
                (title, fields(&split.fields), markdown::to_html(&split.body))
            }
            Err(_) => (String::new(), Vec::new(), markdown::to_html(&h.text)),
        };
        HeadView {
            short: h.stamp.short().to_owned(),
            href: format!("/version/{}", h.stamp.id),
            written: h.stamp.written_to_minute(),
            written_full: h.stamp.written_to_millis(),
            machine: h.stamp.machine.clone(),
            operation: h.stamp.operation.clone(),
            summary: markdown::inline(&title),
            title,
            fields: fields_,
            body,
        }
    }
}

/// The first head's summary, which names the page.
fn summary_of(heads: &[HeadView]) -> String {
    heads.first().map(|h| h.summary.clone()).unwrap_or_default()
}

/// The columns every listed version shows.
pub struct VersionLine {
    pub version: Link,
    pub written: String,
    pub written_full: String,
    pub machine: String,
    pub operation: String,
}

impl From<&Stamp> for VersionLine {
    fn from(s: &Stamp) -> VersionLine {
        VersionLine {
            version: version_link(&s.id),
            written: s.written_to_minute(),
            written_full: s.written_to_millis(),
            machine: s.machine.clone(),
            operation: s.operation.clone(),
        }
    }
}

pub struct TopicLine {
    pub link: Link,
    pub summary: String,
    pub machine: Option<String>,
    pub includes: Vec<Link>,
    pub facts: usize,
    pub ideas: usize,
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct Home {
    pub topics: Vec<TopicLine>,
    pub facts: usize,
    pub ideas: usize,
    pub due: Vec<Open>,
    pub forks: Vec<Link>,
}

impl Home {
    #[must_use]
    pub fn new(topics: &Topics, open: &Followups, forks: &Forks) -> Home {
        let (facts, ideas) = topics
            .topics
            .iter()
            .fold((0, 0), |(f, i), t| (f + t.facts, i + t.ideas));
        Home {
            facts,
            ideas,
            topics: topics
                .topics
                .iter()
                .map(|t| TopicLine {
                    link: topic_link(&t.slug),
                    summary: markdown::inline(&t.summary),
                    machine: t.machine.clone(),
                    includes: t.includes.iter().map(|i| topic_link(i)).collect(),
                    facts: t.facts,
                    ideas: t.ideas,
                })
                .collect(),
            due: open
                .items
                .iter()
                .filter(|i| i.due)
                .map(Open::from)
                .collect(),
            forks: forks.forks.iter().map(|f| slug_link(&f.slug)).collect(),
        }
    }
}

#[derive(Template)]
#[template(path = "topic.html")]
pub struct TopicPage {
    pub name: String,
    pub name_link: Link,
    pub summary: String,
    pub heads: Vec<HeadView>,
    pub history: Option<String>,
    pub facts: Vec<Item>,
    pub ideas: Vec<Item>,
    pub entries: Vec<Item>,
    pub open: Vec<Open>,
}

impl TopicPage {
    #[must_use]
    pub fn new(
        shown: &Shown,
        facts: &FactListing,
        tagged: &Listing,
        open: &Followups,
    ) -> TopicPage {
        let heads: Vec<HeadView> = shown.heads.iter().map(HeadView::from).collect();
        TopicPage {
            name: shown.slug.clone(),
            name_link: Link::plain(&shown.slug),
            summary: summary_of(&heads),
            heads,
            history: Some(history_href(&shown.slug)),
            facts: items(&facts.facts),
            ideas: items(&facts.ideas),
            entries: tagged
                .rows
                .iter()
                .filter(|r| r.kind == Kind::Entry.dir())
                .map(Item::from)
                .collect(),
            open: opens(&open.items),
        }
    }
}

#[derive(Template)]
#[template(path = "doc.html")]
pub struct DocPage {
    pub crumbs: Vec<Link>,
    pub nav: &'static str,
    pub slug: String,
    pub slug_text: Link,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub forked: bool,
    /// A note that a head was written by a newer worklog.
    pub foreign: Option<String>,
    pub history: Option<String>,
    pub heads: Vec<HeadView>,
    pub followups: Vec<Open>,
}

impl From<&Shown> for DocPage {
    fn from(s: &Shown) -> DocPage {
        let heads: Vec<HeadView> = s.heads.iter().map(HeadView::from).collect();
        let place = place(&s.slug);
        DocPage {
            crumbs: place.crumbs,
            nav: place.nav,
            slug: s.slug.clone(),
            slug_text: Link::plain(&s.slug),
            kind: s.kind.clone(),
            title: heads.first().map(|h| h.title.clone()).unwrap_or_default(),
            summary: summary_of(&heads),
            forked: s.forked,
            foreign: s.foreign.clone(),
            history: Some(history_href(&s.slug)),
            heads,
            followups: opens(&s.followups),
        }
    }
}

pub struct HistoryLine {
    pub line: VersionLine,
    pub parents: Vec<Link>,
    /// The slug a version was written under before a rename.
    pub moved_from: Option<Link>,
}

#[derive(Template)]
#[template(path = "history.html")]
pub struct HistoryPage {
    pub crumbs: Vec<Link>,
    pub nav: &'static str,
    pub slug: Link,
    pub slug_plain: Link,
    pub history: Option<String>,
    /// A note that the head was written by a newer worklog.
    pub foreign: Option<String>,
    pub versions: Vec<HistoryLine>,
}

impl From<&History> for HistoryPage {
    fn from(h: &History) -> HistoryPage {
        let mut place = place(&h.slug);
        place.crumbs.push(slug_link(&h.slug));
        HistoryPage {
            crumbs: place.crumbs,
            nav: place.nav,
            slug: slug_link(&h.slug),
            slug_plain: Link::plain(&h.slug),
            history: None,
            foreign: h.foreign.clone(),
            versions: h
                .versions
                .iter()
                .map(|v| HistoryLine {
                    line: VersionLine::from(&v.stamp),
                    parents: v.parents.iter().map(|p| version_link(p)).collect(),
                    moved_from: (v.slug != h.slug).then(|| slug_link(&v.slug)),
                })
                .collect(),
        }
    }
}

pub struct DiffLine {
    pub class: &'static str,
    pub sign: char,
    pub text: String,
}

#[derive(Template)]
#[template(path = "version.html")]
pub struct VersionPage {
    pub crumbs: Vec<Link>,
    pub nav: &'static str,
    pub slug: Link,
    pub short_link: Link,
    pub history: Option<String>,
    pub head: HeadView,
    pub parents: Vec<Link>,
    pub lines: Vec<DiffLine>,
}

impl VersionPage {
    #[must_use]
    pub fn new(slug: &str, head: &Head, diff: &Diff) -> VersionPage {
        let text_diff = similar::TextDiff::from_lines(&diff.before.text, &diff.after.text);
        let lines = text_diff
            .iter_all_changes()
            .map(|change| {
                let (class, sign) = match change.tag() {
                    similar::ChangeTag::Equal => ("same", ' '),
                    similar::ChangeTag::Delete => ("removed", '-'),
                    similar::ChangeTag::Insert => ("added", '+'),
                };
                DiffLine {
                    class,
                    sign,
                    text: change.value().trim_end_matches('\n').to_owned(),
                }
            })
            .collect();
        let mut place = place(slug);
        place.crumbs.push(slug_link(slug));
        place.crumbs.push(Link::to("History", history_href(slug)));
        VersionPage {
            crumbs: place.crumbs,
            nav: place.nav,
            slug: slug_link(slug),
            short_link: Link::plain(head.stamp.short()),
            history: None,
            head: HeadView::from(head),
            parents: head.parents.iter().map(|p| version_link(p)).collect(),
            lines,
        }
    }
}

#[derive(Template)]
#[template(path = "listing.html")]
pub struct ListingPage {
    pub title: String,
    /// The nav entry this listing belongs under.
    pub nav: &'static str,
    /// A count line in place of a heading, on a page the nav already names.
    pub summary: Option<String>,
    /// The topic named like the tag, when the listing is a tag's.
    pub topic: Option<Link>,
    pub rows: Vec<Item>,
}

impl ListingPage {
    #[must_use]
    pub fn entries(listing: &Listing) -> ListingPage {
        ListingPage {
            title: "Entries".to_owned(),
            nav: "entries",
            summary: Some(format!("{} entries, newest first", listing.rows.len())),
            topic: None,
            rows: items(&listing.rows),
        }
    }

    #[must_use]
    pub fn tagged(tag: &str, listing: &Listing, topics: &Topics) -> ListingPage {
        ListingPage {
            title: format!("Tagged {tag}"),
            nav: "tags",
            summary: None,
            topic: topic_named(topics, tag),
            rows: items(&listing.rows),
        }
    }
}

#[derive(Template)]
#[template(path = "facts.html")]
pub struct FactsPage {
    pub facts: Vec<Item>,
    pub ideas: Vec<Item>,
}

impl From<&FactListing> for FactsPage {
    fn from(f: &FactListing) -> FactsPage {
        FactsPage {
            facts: items(&f.facts),
            ideas: items(&f.ideas),
        }
    }
}

#[derive(Template)]
#[template(path = "followups.html")]
pub struct FollowupsPage {
    pub all: bool,
    pub items: Vec<Open>,
    pub open: usize,
    pub entries: usize,
    pub due: usize,
    pub without_recheck: usize,
}

impl FollowupsPage {
    #[must_use]
    pub fn new(f: &Followups, all: bool) -> FollowupsPage {
        FollowupsPage {
            all,
            items: opens(&f.items),
            open: f.open,
            entries: f.entries,
            due: f.due,
            without_recheck: f.without_recheck,
        }
    }
}

pub struct CountLine {
    pub link: Link,
    pub count: usize,
    pub topic: Option<Link>,
}

#[derive(Template)]
#[template(path = "tags.html")]
pub struct TagsPage {
    pub tags: Vec<CountLine>,
}

impl TagsPage {
    #[must_use]
    pub fn new(t: &Tags, topics: &Topics) -> TagsPage {
        TagsPage {
            tags: t
                .tags
                .iter()
                .map(|Count { name, count }| CountLine {
                    link: tag_link(name),
                    count: *count,
                    topic: topic_named(topics, name),
                })
                .collect(),
        }
    }
}

pub struct HitView {
    pub item: Item,
    pub lines: Vec<(usize, String)>,
}

#[derive(Template)]
#[template(path = "search.html")]
pub struct SearchPage {
    pub term: String,
    pub hits: Vec<HitView>,
}

impl From<&Search> for SearchPage {
    fn from(s: &Search) -> SearchPage {
        SearchPage {
            term: s.term.clone(),
            hits: s
                .hits
                .iter()
                .map(|h| HitView {
                    item: Item::from(&h.row),
                    lines: h.lines.clone(),
                })
                .collect(),
        }
    }
}

pub struct LogLine {
    pub line: VersionLine,
    pub slug: Link,
}

#[derive(Template)]
#[template(path = "log.html")]
pub struct LogPage {
    pub versions: Vec<LogLine>,
}

impl From<&Log> for LogPage {
    fn from(l: &Log) -> LogPage {
        LogPage {
            versions: l
                .versions
                .iter()
                .map(|v| LogLine {
                    line: VersionLine::from(&v.stamp),
                    slug: slug_link(&v.slug),
                })
                .collect(),
        }
    }
}

/// What `check` says about one document, a problem or a notice.
pub struct MessageLine {
    pub slug: Link,
    pub message: String,
}

fn message_lines(problems: &[Problem]) -> Vec<MessageLine> {
    problems
        .iter()
        .map(|p| MessageLine {
            slug: slug_link(&p.slug),
            message: p.message.clone(),
        })
        .collect()
}

#[derive(Template)]
#[template(path = "check.html")]
pub struct CheckPage {
    pub problems: Vec<MessageLine>,
    /// Worth a look and no problem.
    pub notices: Vec<MessageLine>,
    pub forks: Vec<Link>,
    pub documents: usize,
    pub links: usize,
}

impl From<&Check> for CheckPage {
    fn from(c: &Check) -> CheckPage {
        CheckPage {
            problems: message_lines(&c.problems),
            notices: message_lines(&c.notices),
            forks: c.forks.iter().map(|f| slug_link(f)).collect(),
            documents: c.documents,
            links: c.links,
        }
    }
}

pub struct ForkLine {
    pub slug: Link,
    pub heads: Vec<Link>,
}

#[derive(Template)]
#[template(path = "forks.html")]
pub struct ForksPage {
    pub forks: Vec<ForkLine>,
}

impl From<&Forks> for ForksPage {
    fn from(f: &Forks) -> ForksPage {
        ForksPage {
            forks: f
                .forks
                .iter()
                .map(|fork| ForkLine {
                    slug: slug_link(&fork.slug),
                    heads: fork.heads.iter().map(|h| version_link(h)).collect(),
                })
                .collect(),
        }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorPage {
    pub status: u16,
    pub message: String,
}
