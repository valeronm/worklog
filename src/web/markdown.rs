//! A body and a summary as HTML: Markdown with the raw HTML in it shown as the text it is, and
//! each `[[text]]` as a link to the document the version's links hold for it.

use std::fmt;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, html};

use super::mark::EndedMark;
use crate::app::show::Linked;
use crate::domain::id::DocumentId;
use crate::domain::schema::links::link_at;

// Holds only what this module made: a template writes it without escaping it.
pub struct Markup(String);

impl fmt::Display for Markup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl askama::filters::HtmlSafe for Markup {}

const SCHEMES: [&str; 3] = ["http", "https", "mailto"];

// A browser reads an address past the spaces and control characters around and inside it.
fn followable(address: &str) -> bool {
    if address.chars().any(char::is_control) {
        return false;
    }
    let address = address.trim();
    let named = address.find([':', '/', '?', '#']);
    match named {
        Some(colon) if address[colon..].starts_with(':') => {
            let scheme = &address[..colon];
            SCHEMES
                .iter()
                .any(|known| scheme.eq_ignore_ascii_case(known))
        }
        _ => true,
    }
}

const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS);

type Linking<'l> = (&'l [Linked], fn(&DocumentId) -> String);

// What a run of text is given: its `[[text]]` made links, or byte ranges of the source
// marked, in order and apart.
#[derive(Clone, Copy)]
enum Runs<'l> {
    Linked(Linking<'l>),
    Marked(&'l [Range<usize>]),
}

impl<'l> Runs<'l> {
    fn spans(self) -> &'l [Range<usize>] {
        match self {
            Runs::Marked(spans) => spans,
            Runs::Linked(_) => &[],
        }
    }
}

// Where a piece starts in the text holding it and, when it is the source as written, in the
// source.
type Piece = (usize, Option<usize>);

struct Rendering<'b, 'l> {
    source: &'b str,
    runs: Runs<'l>,
    out: Vec<Event<'b>>,
    // The parser hands one run of text over in pieces, a bracket at a time.
    pending: String,
    pieces: Vec<Piece>,
    in_code: bool,
    // Whether each link or image that is open was written as a link.
    open: Vec<bool>,
}

impl<'b> Rendering<'b, '_> {
    fn markup(&mut self, html: &'static str) {
        self.out.push(Event::InlineHtml(CowStr::Borrowed(html)));
    }

    // A span is marked only when it falls whole within one piece of `text`.
    fn text(&mut self, text: &str, pieces: &[Piece]) {
        let spans = self.runs.spans();
        let mut marks = Vec::new();
        for (next, &(start, written)) in pieces.iter().enumerate().map(|(at, p)| (at + 1, p)) {
            let Some(written) = written else { continue };
            let end = pieces.get(next).map_or(text.len(), |piece| piece.0);
            let within =
                |span: &&Range<usize>| written <= span.start && span.end <= written + end - start;
            let shifted =
                |span: &Range<usize>| span.start + start - written..span.end + start - written;
            marks.extend(spans.iter().filter(within).map(shifted));
        }
        let piece = |range: Range<usize>| Event::Text(CowStr::from(text[range].to_owned()));
        let mut from = 0;
        for mark in marks {
            if from < mark.start {
                self.out.push(piece(from..mark.start));
            }
            self.markup("<mark>");
            self.out.push(piece(mark.clone()));
            self.markup("</mark>");
            from = mark.end;
        }
        if from < text.len() {
            self.out.push(piece(from..text.len()));
        }
    }

    fn link(&mut self, (links, href): Linking, bracketed: &str, target: &str) {
        let held = links.iter().find(|link| link.text == target);
        let Some(reference) = held.and_then(|link| link.target.as_ref()) else {
            self.markup("<span class=\"unresolved\">");
            self.text(bracketed, &[]);
            self.markup("</span>");
            return;
        };
        let mark = EndedMark::of(reference.ended());
        let title = mark.as_ref().map(|mark| mark.title().to_owned());
        let start = Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url: CowStr::from(href(&reference.document)),
            title: CowStr::from(title.unwrap_or_default()),
            id: CowStr::Borrowed(""),
        });
        match mark {
            // The writer escapes an address and a title, and gives a link no class.
            Some(mark) => {
                let mut tag = String::new();
                html::push_html(&mut tag, std::iter::once(start));
                let classed = format!("<a class=\"{}\" ", mark.class());
                let tag = tag.replacen("<a ", &classed, 1);
                self.out.push(Event::InlineHtml(CowStr::from(tag)));
            }
            None => self.out.push(start),
        }
        self.text(&reference.label, &[]);
        self.out.push(Event::End(TagEnd::Link));
    }

    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        let pieces = std::mem::take(&mut self.pieces);
        let linking = match self.runs {
            Runs::Linked(linking) => linking,
            Runs::Marked(_) => return self.text(&pending, &pieces),
        };
        let mut rest = pending.as_str();
        let mut from = 0;
        while let Some(at) = rest[from..].find("[[").map(|at| at + from) {
            match link_at(&rest[at..]) {
                Ok((target, taken)) => {
                    self.text(&rest[..at], &[]);
                    self.link(linking, &rest[at..at + taken], target);
                    rest = &rest[at + taken..];
                    from = 0;
                }
                Err(passed) => from = at + passed,
            }
        }
        self.text(rest, &[]);
    }

    fn opened(&mut self, address: &str) -> bool {
        let linked = self.open.iter().all(|linked| !linked) && followable(address);
        self.open.push(linked);
        linked
    }

    // Where `text`, read from `range`, starts in `source` when it reads as written there.
    fn written_at(&self, text: &str, range: Range<usize>) -> Option<usize> {
        let start = range.start;
        (self.source.get(range) == Some(text)).then_some(start)
    }

    fn take(&mut self, event: Event<'b>, range: Range<usize>) {
        match event {
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                let at = self.written_at(&text, range);
                if self.in_code || !self.open.is_empty() {
                    self.text(&text, &[(0, at)]);
                } else {
                    let length = self.pending.len();
                    let follows = self.pieces.last().is_some_and(|&(start, written)| {
                        at.is_some() && written.map(|written| written + length - start) == at
                    });
                    if !follows {
                        self.pieces.push((length, at));
                    }
                    self.pending.push_str(&text);
                }
                return;
            }
            _ => self.flush(),
        }
        match event {
            Event::Code(code) => {
                // A code span's text starts past its backticks and the one space that may pad it.
                let written = self.source.get(range.clone()).unwrap_or_default();
                let ticks = written.len() - written.trim_start_matches('`').len();
                let at = [0, 1]
                    .map(|pad| range.start + ticks + pad)
                    .into_iter()
                    .find_map(|start| self.written_at(&code, start..start + code.len()));
                self.markup("<code>");
                self.text(&code, &[(0, at)]);
                self.markup("</code>");
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                self.in_code = true;
                // The writer makes a class of the info string up to its first space alone.
                let kind = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let word = info.split_whitespace().next().unwrap_or_default();
                        CodeBlockKind::Fenced(CowStr::from(word.to_owned()))
                    }
                    CodeBlockKind::Indented => kind,
                };
                self.out.push(Event::Start(Tag::CodeBlock(kind)));
            }
            Event::End(TagEnd::CodeBlock) => {
                self.in_code = false;
                self.out.push(event);
            }
            Event::Start(Tag::Link { ref dest_url, .. }) => {
                if self.opened(dest_url) {
                    self.out.push(event);
                }
            }
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                if self.opened(&dest_url) {
                    self.out.push(Event::Start(Tag::Link {
                        link_type: LinkType::Inline,
                        dest_url,
                        title,
                        id: CowStr::Borrowed(""),
                    }));
                }
            }
            Event::End(TagEnd::Link | TagEnd::Image) => {
                if self.open.pop() == Some(true) {
                    self.out.push(Event::End(TagEnd::Link));
                }
            }
            other => self.out.push(other),
        }
    }
}

fn rendered<'b>(
    source: &'b str,
    events: impl Iterator<Item = (Event<'b>, Range<usize>)>,
    runs: Runs,
) -> Markup {
    let mut rendering = Rendering {
        source,
        runs,
        out: Vec::new(),
        pending: String::new(),
        pieces: Vec::new(),
        in_code: false,
        open: Vec::new(),
    };
    for (event, range) in events {
        rendering.take(event, range);
    }
    rendering.flush();
    let mut out = String::with_capacity(source.len() * 2);
    html::push_html(&mut out, rendering.out.into_iter());
    Markup(out)
}

/// `text` as HTML, with each of `spans`, byte ranges of it in order and apart, marked.
pub fn marked(text: &str, spans: &[Range<usize>]) -> Markup {
    let whole = std::iter::once((Event::Text(CowStr::Borrowed(text)), 0..text.len()));
    rendered(text, whole, Runs::Marked(spans))
}

pub fn to_html(body: &str, links: &[Linked], href: fn(&DocumentId) -> String) -> Markup {
    let events = Parser::new_ext(body, OPTIONS).into_offset_iter();
    rendered(body, events, Runs::Linked((links, href)))
}

/// A summary as HTML with no block around it: the inline Markdown of the one paragraph it
/// reads as, and its text as written where it reads as anything else. Without `linking` each
/// `[[text]]` stays text.
pub fn summary_html(summary: &str, linking: Option<Linking>) -> Markup {
    summary_rendered(summary, linking.map_or(Runs::Marked(&[]), Runs::Linked))
}

/// A summary as `summary_html` gives it with no `linking`, with each of `spans`, byte ranges
/// of it in order and apart, marked where it falls within one run of text or one code span.
pub fn summary_marked(summary: &str, spans: &[Range<usize>]) -> Markup {
    summary_rendered(summary, Runs::Marked(spans))
}

fn summary_rendered(summary: &str, runs: Runs) -> Markup {
    let mut events: Vec<(Event, Range<usize>)> = Parser::new_ext(summary, OPTIONS)
        .into_offset_iter()
        .collect();
    let one_paragraph = matches!(
        events.as_slice(),
        [(Event::Start(Tag::Paragraph), _), inline @ .., (Event::End(TagEnd::Paragraph), _)]
            if !inline.iter().any(|(event, _)| *event == Event::End(TagEnd::Paragraph))
    );
    if one_paragraph {
        let inline = 1..events.len() - 1;
        rendered(summary, events.drain(inline), runs)
    } else {
        marked(summary, runs.spans())
    }
}

#[cfg(test)]
mod tests {
    use pulldown_cmark::{Event, Parser};

    use super::*;
    use crate::app::testing::{to, to_ended};
    use crate::domain::schema::links::tried;

    fn atlas() -> DocumentId {
        DocumentId::from_bytes([0xa7; 16])
    }

    fn doc(document: &DocumentId) -> String {
        format!("/doc/{document}")
    }

    fn linked(text: &str, label: Option<&str>) -> Linked {
        Linked {
            text: text.to_owned(),
            target: label.map(|label| to(&atlas(), label)),
        }
    }

    fn ended(text: &str, label: &str, reason: &str) -> Linked {
        Linked {
            text: text.to_owned(),
            target: Some(to_ended(&atlas(), label, reason)),
        }
    }

    fn links() -> Vec<Linked> {
        vec![
            linked("atlas", Some("atlas")),
            linked("atlas & co", Some("<b>atlas</b> & co")),
            linked("lantern/relay-pin", Some("a7a7a7a7")),
            linked("phone", None),
        ]
    }

    fn html(body: &str) -> String {
        to_html(body, &links(), doc).to_string()
    }

    const ATLAS: &str = "<a href=\"/doc/a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7\">atlas</a>";

    #[track_caller]
    fn no_anchor(body: &str) -> String {
        let html = html(body);
        for markup in ["<a", "href", "<img", "src="] {
            assert!(!html.contains(markup), "{body}: `{markup}` in {html}");
        }
        html
    }

    fn summary(text: &str) -> String {
        summary_html(text, Some((&links(), doc))).to_string()
    }

    #[test]
    fn a_summary_is_its_inline_markdown_with_no_block_around_it() {
        assert_eq!(
            summary("Pin `GPIO 5` is **fixed**, ~~not~~ *free*"),
            "Pin <code>GPIO 5</code> is <strong>fixed</strong>, <del>not</del> <em>free</em>"
        );
        assert_eq!(summary("See [[atlas]]"), format!("See {ATLAS}"));
        assert_eq!(
            summary("A <b>lamp</b> & co"),
            "A &lt;b&gt;lamp&lt;/b&gt; &amp; co"
        );
        assert_eq!(summary(""), "");
    }

    #[test]
    fn a_summary_given_no_links_keeps_each_link_in_brackets_as_text() {
        let row = summary_html("See [[atlas]] and `x`", None).to_string();
        assert_eq!(row, "See [[atlas]] and <code>x</code>");
    }

    #[test]
    fn a_summary_that_reads_as_anything_but_one_paragraph_is_its_text() {
        for text in [
            "# Why",
            "- one",
            "1. one",
            "> said",
            "one\n\ntwo",
            "| a |\n| - |\n| b |",
        ] {
            let html = summary(text);
            assert!(!html.contains('<'), "{text}: {html}");
        }
        assert_eq!(summary("# [[atlas]] & <b>"), "# [[atlas]] &amp; &lt;b&gt;");
    }

    #[test]
    fn a_body_is_rendered_as_markdown() {
        assert_eq!(
            html("## Why\n\nA *lamp* that ~~hums~~ **dims**.\n\n- one\n- two\n"),
            "<h2>Why</h2>\n<p>A <em>lamp</em> that <del>hums</del> <strong>dims</strong>.</p>\n\
             <ul>\n<li>one</li>\n<li>two</li>\n</ul>\n"
        );
        let table = html("| pin | use |\n| --- | --- |\n| 4 | relay |\n");
        assert!(table.contains("<td>relay</td>"), "{table}");
        assert_eq!(html(""), "");
        assert_eq!(html("\n"), "");
    }

    #[test]
    fn raw_html_is_shown_as_the_text_it_is() {
        assert_eq!(
            html("<script>alert(1)</script>\n"),
            "&lt;script&gt;alert(1)&lt;/script&gt;\n"
        );
        assert_eq!(
            html("A <b>bold</b> <img src=x onerror=alert(1)> word.\n"),
            "<p>A &lt;b&gt;bold&lt;/b&gt; &lt;img src=x onerror=alert(1)&gt; word.</p>\n"
        );
        for body in [
            "<div onclick=\"alert(1)\">\nlamp\n</div>\n",
            "<!-- a note -->\n",
            "<style>body { display: none }</style>\n",
            "<iframe src=\"https://example.org\"></iframe>\n",
            "Inline <!-- note --> and <a href=\"javascript:alert(1)\">x</a>.\n",
            "<svg/onload=alert(1)>\n",
            "- <input autofocus onfocus=alert(1)>\n",
            "> <details open ontoggle=alert(1)>\n",
            "| a |\n| - |\n| <script>alert(1)</script> |\n",
            "# <u>heading</u>\n",
        ] {
            let html = html(body);
            assert!(
                !html.contains("<script")
                    && !html.contains("<div")
                    && !html.contains("<!--")
                    && !html.contains("<style")
                    && !html.contains("<iframe")
                    && !html.contains("<a ")
                    && !html.contains("<svg")
                    && !html.contains("<input")
                    && !html.contains("<details")
                    && !html.contains("<u>"),
                "{body}: {html}"
            );
            assert!(html.contains("&lt;"), "{body}: {html}");
        }
    }

    #[test]
    fn a_link_in_brackets_is_a_link_to_the_document_it_resolved_to() {
        assert_eq!(
            html("See [[atlas]] and [[ lantern/relay-pin ]].\n"),
            format!(
                "<p>See {ATLAS} and \
                 <a href=\"/doc/a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7\">a7a7a7a7</a>.</p>\n"
            )
        );
        assert_eq!(
            html("# About [[atlas]]\n\n- *in [[atlas]]*\n"),
            format!("<h1>About {ATLAS}</h1>\n<ul>\n<li><em>in {ATLAS}</em></li>\n</ul>\n")
        );
        assert_eq!(
            html("[[atlas &amp; co]]\n"),
            "<p><a href=\"/doc/a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7\">\
             &lt;b&gt;atlas&lt;/b&gt; &amp; co</a></p>\n"
        );
    }

    #[test]
    fn a_link_to_an_ended_document_is_an_anchor_marked_with_the_reason() {
        let links = [
            linked("atlas", Some("atlas")),
            ended("lantern", "lantern", "done"),
            ended("lantern/relay-pin", "<b>relay</b>", "superseded"),
            ended("desk", "desk", "a\"b<c>&d"),
            linked("phone", None),
        ];
        let html = |body: &str| to_html(body, &links, doc).to_string();
        assert_eq!(
            html("See [[lantern]], [[atlas]] and [[ lantern/relay-pin ]].\n"),
            format!(
                "<p>See <a class=\"ended\" href=\"/doc/a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7\" \
                 title=\"ended done\">lantern</a>, {ATLAS} and \
                 <a class=\"ended\" href=\"/doc/a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7\" \
                 title=\"ended superseded\">&lt;b&gt;relay&lt;/b&gt;</a>.</p>\n"
            )
        );
        assert_eq!(
            html("[[desk]]\n"),
            "<p><a class=\"ended\" href=\"/doc/a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7a7\" \
             title=\"ended a&quot;b&lt;c&gt;&amp;d\">desk</a></p>\n"
        );
        assert_eq!(
            html("[[phone]]\n"),
            "<p><span class=\"unresolved\">[[phone]]</span></p>\n"
        );
        assert_eq!(
            html("`[[lantern]]` and [x [[lantern]]](https://example.org)\n"),
            "<p><code>[[lantern]]</code> and \
             <a href=\"https://example.org\">x [[lantern]]</a></p>\n"
        );
    }

    #[test]
    fn a_link_that_resolved_to_nothing_or_is_not_held_is_marked_text() {
        assert_eq!(
            html("See [[phone]], [[ nowhere ]] and [[Atlas]].\n"),
            "<p>See <span class=\"unresolved\">[[phone]]</span>, \
             <span class=\"unresolved\">[[ nowhere ]]</span> and \
             <span class=\"unresolved\">[[Atlas]]</span>.</p>\n"
        );
        assert_eq!(
            html("[[<b>]]\n"),
            "<p><span class=\"unresolved\">[[&lt;b&gt;]]</span></p>\n"
        );
    }

    #[test]
    fn brackets_in_code_are_left_alone() {
        assert_eq!(
            html("Write `[[atlas]]` or ``[[atlas]]``.\n"),
            "<p>Write <code>[[atlas]]</code> or <code>[[atlas]]</code>.</p>\n"
        );
        assert_eq!(
            html("```\n[[atlas]]\n```\n\n~~~text\n[[atlas]]\n~~~\n\n    [[atlas]]\n"),
            "<pre><code>[[atlas]]\n</code></pre>\n\
             <pre><code class=\"language-text\">[[atlas]]\n</code></pre>\n\
             <pre><code>[[atlas]]\n</code></pre>\n"
        );
    }

    #[test]
    fn brackets_in_the_text_of_a_link_are_left_alone() {
        assert_eq!(
            html("[see [[atlas]]](https://example.org) and [[atlas]]\n"),
            format!("<p><a href=\"https://example.org\">see [[atlas]]</a> and {ATLAS}</p>\n")
        );
        assert_eq!(
            html("![the [[atlas]]](https://example.org/a.png)\n"),
            "<p><a href=\"https://example.org/a.png\">the [[atlas]]</a></p>\n"
        );
    }

    #[test]
    fn a_link_the_parser_hands_over_in_pieces_is_found() {
        let body = "See [[atlas]] now.\n";
        let texts = Parser::new(body)
            .filter(|event| matches!(event, Event::Text(_)))
            .count();
        assert!(texts > 1, "{texts} text events");
        assert_eq!(html(body), format!("<p>See {ATLAS} now.</p>\n"));
        assert_eq!(
            html("a [[atlas]][[atlas]] [ [[atlas]] ] b\n"),
            format!("<p>a {ATLAS}{ATLAS} [ {ATLAS} ] b</p>\n")
        );
    }

    #[test]
    fn what_is_a_link_on_a_page_is_what_a_save_finds_as_one() {
        for body in [
            "See [[lantern/relay-pin]] and [[ atlas ]], then [[lantern/relay-pin]] again.\n\
             Also [[7f3a91c0]].\n",
            "Nothing here.\n",
            "Write `[[lantern]]` to link, as [[atlas]] does.\n",
            "Or ``[[lantern]]`` with two, as [[atlas]] does.\n",
            "A lone ` quotes nothing, so [[atlas]] links.\n",
            "Before [[phone]].\n```\n[[lantern]]\n~~~\n[[lantern]]\n```\nAfter [[desk]].\n\
             ~~~text\n[[atlas]]\n~~~\n",
            "```rust``` opens no fence, so [[atlas]] links.\nAnd [[desk]] after it.\n",
            "An empty [[ ]] is none.\n",
            "Open [[lantern and never closed.\n",
            "[[[lantern]]] and [single] [[atlas]]\n",
            "## In a heading [[atlas]]\n\n- in a list [[desk]]\n\n> quoted [[phone]]\n",
            "Тема [[тема]] и [[ ключ ]], é[[é]]é, 日[[日本]]本.\n",
            "[[é[x]]\n",
            "[[тема[черновик]] затем [[итог]]\n",
            "é[é[[é]é]]é и [[ü]]é\n",
            "[[é[[x]]é и é[[[y]]]é\n",
            "`é[[x]]é` и é[[y]]é\n",
        ] {
            let html = to_html(body, &[], doc).to_string();
            let mut marked: Vec<&str> = Vec::new();
            for rest in html.split("<span class=\"unresolved\">[[").skip(1) {
                let target = rest
                    .split_once("]]</span>")
                    .expect("a closed mark")
                    .0
                    .trim();
                if !marked.contains(&target) {
                    marked.push(target);
                }
            }
            assert_eq!(marked, crate::domain::schema::links::links(body), "{body}");
        }
    }

    #[test]
    fn a_bracket_after_a_character_of_several_bytes_is_rendered() {
        assert_eq!(html("[[é[x]]\n"), "<p>[[é[x]]</p>\n");
        assert_eq!(
            html("[[тема[черновик]] и [[atlas]]é\n"),
            format!("<p>[[тема[черновик]] и {ATLAS}é</p>\n")
        );
        assert_eq!(
            html("<div>\n[[é[x]] é[[atlas]]\n</div>\n"),
            format!("&lt;div&gt;\n[[é[x]] é{ATLAS}\n&lt;/div&gt;\n")
        );
    }

    #[test]
    fn brackets_that_close_nothing_are_no_link() {
        for body in [
            "An empty [[ ]] is none.\n",
            "Open [[atlas and never closed.\n",
            "Closed ]] and opened [[.\n",
            "[[at\nlas]]\n",
            "[[atlas]\n]\n",
            "[atlas] and [[at[las]]\n",
        ] {
            let html = html(body);
            assert!(
                !html.contains("<a") && !html.contains("<span"),
                "{body}: {html}"
            );
        }
        assert_eq!(html("[[[atlas]]]\n"), format!("<p>[{ATLAS}]</p>\n"));
        assert_eq!(html("[[ [[atlas]] ]]\n"), format!("<p>[[ {ATLAS} ]]</p>\n"));
    }

    #[test]
    fn a_link_keeps_an_address_that_is_http_mail_or_relative() {
        for (body, href) in [
            (
                "[x](https://example.org/a?b=1#c)",
                "https://example.org/a?b=1#c",
            ),
            ("[x](http://example.org)", "http://example.org"),
            ("[x](HTTPS://example.org)", "HTTPS://example.org"),
            ("[x](mailto:desk@example.org)", "mailto:desk@example.org"),
            ("<https://example.org>", "https://example.org"),
            ("<desk@example.org>", "mailto:desk@example.org"),
            ("[x](#part)", "#part"),
            ("[x](notes/plan.md)", "notes/plan.md"),
            ("[x](/facts)", "/facts"),
            ("[x](../up)", "../up"),
            ("[x](./a:b)", "./a:b"),
            ("[x](?q=a:b)", "?q=a:b"),
            ("[x](//example.org/a)", "//example.org/a"),
            ("[x][r]\n\n[r]: https://example.org", "https://example.org"),
        ] {
            let html = html(body);
            assert!(
                html.contains(&format!("<a href=\"{href}\">")),
                "{body}: {html}"
            );
        }
        assert_eq!(
            html("[the *plan*](notes/plan.md \"Plan\")\n"),
            "<p><a href=\"notes/plan.md\" title=\"Plan\">the <em>plan</em></a></p>\n"
        );
    }

    #[test]
    fn a_link_to_any_other_address_is_its_text_alone() {
        for body in [
            "[x](javascript:alert(1))",
            "[x](JaVaScRiPt:alert(1))",
            "[x](< javascript:alert(1)>)",
            "[x](&#106;avascript:alert(1))",
            "[x](java&#9;script:alert(1))",
            "[x](&#1;javascript:alert(1))",
            "[x](javascript&colon;alert(1))",
            "[x](data:text/html,<script>alert(1)</script>)",
            "[x](vbscript:msgbox(1))",
            "[x](file:///etc/passwd)",
            "[x](ftp://example.org/a)",
            "[x](lantern:pin)",
            "[x](x-atlas+map.v1:pin)",
            "[x][r]\n\n[r]: javascript:alert(1)",
        ] {
            assert_eq!(no_anchor(body), "<p>x</p>\n", "{body}");
        }
        assert_eq!(
            no_anchor("<javascript:alert(1)>"),
            "<p>javascript:alert(1)</p>\n"
        );
        assert_eq!(
            html("A [*bold* x](javascript:alert(1)) link.\n"),
            "<p>A <em>bold</em> x link.</p>\n"
        );
        assert_eq!(
            html("[x](javascript:alert(1)) then [[atlas]]\n"),
            format!("<p>x then {ATLAS}</p>\n")
        );
    }

    #[test]
    fn what_a_destination_or_a_title_holds_cannot_leave_its_attribute() {
        assert_eq!(
            html("[x](https://example.org/a\"b>c \"T\\\"i<t>le\")\n"),
            "<p><a href=\"https://example.org/a%22b%3Ec\" title=\"T&quot;i&lt;t&gt;le\">x</a></p>\n"
        );
        assert_eq!(
            html("![x](img/a\"b>c 'T\"i<t>le')\n"),
            "<p><a href=\"img/a%22b%3Ec\" title=\"T&quot;i&lt;t&gt;le\">x</a></p>\n"
        );
    }

    #[test]
    fn an_entity_that_spells_markup_stays_text() {
        assert_eq!(
            html("&lt;script&gt;alert(1)&lt;/script&gt; and &#x3c;script>\n"),
            "<p>&lt;script&gt;alert(1)&lt;/script&gt; and &lt;script&gt;</p>\n"
        );
    }

    #[test]
    fn a_script_left_open_does_not_swallow_the_markdown_after_it_as_markup() {
        let html = html("<script>\n\n# After [[atlas]]\n\n*lamp*\n");
        assert!(!html.contains("<script"), "{html}");
        assert!(html.starts_with("&lt;script&gt;\n"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
    }

    #[test]
    fn a_table_a_task_list_and_a_numbered_list_write_only_fixed_attributes() {
        assert_eq!(
            html("| l | c | r | n |\n| :-- | :-: | --: | --- |\n| a | b | c | [[atlas]] |\n"),
            format!(
                "<table><thead><tr><th style=\"text-align: left\">l</th>\
                 <th style=\"text-align: center\">c</th>\
                 <th style=\"text-align: right\">r</th><th>n</th></tr></thead><tbody>\n\
                 <tr><td style=\"text-align: left\">a</td>\
                 <td style=\"text-align: center\">b</td>\
                 <td style=\"text-align: right\">c</td><td>{ATLAS}</td></tr>\n\
                 </tbody></table>\n"
            )
        );
        assert_eq!(
            html("- [ ] open\n- [x] done\n"),
            "<ul>\n<li><input disabled=\"\" type=\"checkbox\"/>\nopen</li>\n\
             <li><input disabled=\"\" type=\"checkbox\" checked=\"\"/>\ndone</li>\n</ul>\n"
        );
        assert_eq!(
            html("7. seven\n8. eight\n"),
            "<ol start=\"7\">\n<li>seven</li>\n<li>eight</li>\n</ol>\n"
        );
    }

    #[test]
    fn a_fence_s_info_string_is_a_class_of_its_first_word_alone() {
        assert_eq!(
            html("```rust ignore\nlet x;\n```\n"),
            "<pre><code class=\"language-rust\">let x;\n</code></pre>\n"
        );
        assert_eq!(
            html("```rust\tonclick=x\nlet x;\n```\n"),
            "<pre><code class=\"language-rust\">let x;\n</code></pre>\n"
        );
        assert_eq!(
            html("~~~a\"><script>alert(1)</script> b\nx\n~~~\n"),
            "<pre><code class=\"language-a&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;\">x\n\
             </code></pre>\n"
        );
        assert_eq!(html("```   \nx\n```\n"), "<pre><code>x\n</code></pre>\n");
    }

    #[test]
    fn a_link_in_brackets_inside_an_html_block_is_a_link() {
        assert_eq!(
            html("<div>\nSee [[atlas]] and [[phone]]\n</div>\n"),
            format!(
                "&lt;div&gt;\nSee {ATLAS} and <span class=\"unresolved\">[[phone]]</span>\n\
                 &lt;/div&gt;\n"
            )
        );
        let split = html("<div>\n[[at\nlas]]\n</div>\n");
        assert!(!split.contains("<a") && !split.contains("<span"), "{split}");
    }

    #[test]
    fn a_run_of_brackets_is_searched_without_trying_each_as_a_link() {
        let body = format!("{}]] then [[atlas]]\n", "[".repeat(200_000));
        let before = tried();
        let html = html(&body);
        let tries = tried() - before;
        assert!(tries <= 8, "{tries} tries");
        assert!(
            html.ends_with(&format!("]] then {ATLAS}</p>\n")),
            "{}",
            &html[html.len() - 80..]
        );
        assert_eq!(
            html.len(),
            "<p>".len() + 200_000 + "]] then ".len() + ATLAS.len() + "</p>\n".len()
        );
    }

    #[test]
    fn an_image_is_a_link_to_its_source_and_never_loaded() {
        assert_eq!(
            html("![a *lamp*](https://example.org/lamp.png \"Lit\")\n"),
            "<p><a href=\"https://example.org/lamp.png\" title=\"Lit\">a <em>lamp</em></a></p>\n"
        );
        assert_eq!(
            html("![lamp](img/lamp.png)\n"),
            "<p><a href=\"img/lamp.png\">lamp</a></p>\n"
        );
        for body in [
            "![x](javascript:alert(1))",
            "![x](data:image/png;base64,AAAA)",
            "![x](file:///etc/passwd)",
        ] {
            assert_eq!(no_anchor(body), "<p>x</p>\n", "{body}");
        }
        assert!(!html("![x](https://example.org/a.png)").contains("<img"));
    }

    #[test]
    fn a_link_inside_a_link_is_its_text() {
        assert_eq!(
            html("[![lamp](https://example.org/lamp.png)](https://example.org)\n"),
            "<p><a href=\"https://example.org\">lamp</a></p>\n"
        );
        assert_eq!(
            html("![a [lamp](https://example.org/b) lit](https://example.org/lamp.png)\n"),
            "<p><a href=\"https://example.org/lamp.png\">a lamp lit</a></p>\n"
        );
        assert_eq!(
            html("[![lamp](https://example.org/lamp.png)](javascript:alert(1))\n"),
            "<p><a href=\"https://example.org/lamp.png\">lamp</a></p>\n"
        );
    }

    fn marked_at(summary: &str, term: &str) -> String {
        let spans: Vec<Range<usize>> = (summary.match_indices(term))
            .map(|(at, found)| at..at + found.len())
            .collect();
        summary_marked(summary, &spans).to_string()
    }

    #[test]
    fn a_summary_marks_a_span_within_one_run_of_text_or_one_code_span() {
        for (summary, term, shown) in [
            (
                "A **bold** relay",
                "relay",
                "A <strong>bold</strong> <mark>relay</mark>",
            ),
            ("a [b c] d", "a [b c", "<mark>a [b c</mark>] d"),
            (
                "The `relay` pin",
                "relay",
                "The <code><mark>relay</mark></code> pin",
            ),
            ("x <b> y", "<b>", "x <mark>&lt;b&gt;</mark> y"),
            ("# a relay", "relay", "# a <mark>relay</mark>"),
            (
                "pin and pin",
                "pin",
                "<mark>pin</mark> and <mark>pin</mark>",
            ),
            (
                "relay \\* relay",
                "relay",
                "<mark>relay</mark> * <mark>relay</mark>",
            ),
            (
                "a ` relay ` b",
                "relay",
                "a <code><mark>relay</mark></code> b",
            ),
            ("a `` ` `` b", "`", "a <code><mark>`</mark></code> b"),
        ] {
            assert_eq!(marked_at(summary, term), shown, "{summary}");
        }
    }

    #[test]
    fn a_summary_marks_nothing_of_a_span_that_crosses_its_markup() {
        for (summary, term) in [
            ("a **b** c", "a **b"),
            ("The `relay` pin", "`relay`"),
            ("a \\* b", "a \\* b"),
        ] {
            assert!(!marked_at(summary, term).contains("<mark>"), "{summary}");
        }
    }

    #[test]
    fn text_marks_its_spans_and_escapes_the_rest() {
        assert_eq!(
            marked("a<b>c&", &[1..4, 5..6]).to_string(),
            "a<mark>&lt;b&gt;</mark>c<mark>&amp;</mark>"
        );
        assert_eq!(marked("a&b", &[]).to_string(), "a&amp;b");
    }
}
