//! A body as HTML: Markdown with the raw HTML in it shown as the text it is, and each
//! `[[text]]` as a link to the document the version's links hold for it.

use std::fmt;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, html};

use super::mark::EndedMark;
use crate::app::show::Linked;
use crate::domain::id::DocumentId;
use crate::domain::schema::links::link_at;

// Holds only what `to_html` made: a template writes it without escaping it.
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

struct Rendering<'b, 'l> {
    links: &'l [Linked],
    href: fn(&DocumentId) -> String,
    out: Vec<Event<'b>>,
    // The parser hands one run of text over in pieces, a bracket at a time.
    pending: String,
    in_code: bool,
    // Whether each link or image that is open was written as a link.
    open: Vec<bool>,
}

impl<'b> Rendering<'b, '_> {
    fn text(&mut self, text: &str) {
        if !text.is_empty() {
            self.out.push(Event::Text(CowStr::from(text.to_owned())));
        }
    }

    fn link(&mut self, bracketed: &str, target: &str) {
        let held = self.links.iter().find(|link| link.text == target);
        let Some(reference) = held.and_then(|link| link.target.as_ref()) else {
            let open = CowStr::Borrowed("<span class=\"unresolved\">");
            self.out.push(Event::InlineHtml(open));
            self.text(bracketed);
            self.out
                .push(Event::InlineHtml(CowStr::Borrowed("</span>")));
            return;
        };
        let mark = EndedMark::of(reference.ended());
        let title = mark.as_ref().map(|mark| mark.title().to_owned());
        let start = Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url: CowStr::from((self.href)(&reference.document)),
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
        self.text(&reference.label);
        self.out.push(Event::End(TagEnd::Link));
    }

    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        let mut rest = pending.as_str();
        let mut from = 0;
        while let Some(at) = rest[from..].find("[[").map(|at| at + from) {
            match link_at(&rest[at..]) {
                Ok((target, taken)) => {
                    self.text(&rest[..at]);
                    self.link(&rest[at..at + taken], target);
                    rest = &rest[at + taken..];
                    from = 0;
                }
                Err(passed) => from = at + passed,
            }
        }
        self.text(rest);
    }

    fn opened(&mut self, address: &str) -> bool {
        let linked = self.open.iter().all(|linked| !linked) && followable(address);
        self.open.push(linked);
        linked
    }

    fn take(&mut self, event: Event<'b>) {
        match event {
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if self.in_code || !self.open.is_empty() {
                    self.out.push(Event::Text(text));
                } else {
                    self.pending.push_str(&text);
                }
                return;
            }
            _ => self.flush(),
        }
        match event {
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

pub fn to_html(body: &str, links: &[Linked], href: fn(&DocumentId) -> String) -> Markup {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut rendering = Rendering {
        links,
        href,
        out: Vec::new(),
        pending: String::new(),
        in_code: false,
        open: Vec::new(),
    };
    for event in Parser::new_ext(body, options) {
        rendering.take(event);
    }
    rendering.flush();
    let mut out = String::with_capacity(body.len() * 2);
    html::push_html(&mut out, rendering.out.into_iter());
    Markup(out)
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
}
