//! A body as HTML, with each `[[slug]]` a link to that document's page.

use pulldown_cmark::{Event, Options, Parser, html};

use crate::domain::links;

/// Raw HTML in a document is shown as the text it is, never run.
fn render(text: &str) -> String {
    let parser = Parser::new_ext(
        text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
    .map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });
    let mut out = String::with_capacity(text.len() * 2);
    html::push_html(&mut out, parser);
    out
}

#[must_use]
pub fn to_html(body: &str) -> String {
    render(&links::linked(body, |target| {
        format!("[{target}](/doc/{target})")
    }))
}

/// A one-line summary as the inline HTML it holds, for a place that is
/// itself a link: a `[[slug]]` becomes a code span, since an anchor
/// cannot hold another.
#[must_use]
pub fn inline(summary: &str) -> String {
    let mut html = render(&links::linked(summary, |target| format!("`{target}`")));
    let trimmed = html.trim_end().len();
    html.truncate(trimmed);
    if html.starts_with("<p>") && html.ends_with("</p>") {
        html.truncate(html.len() - "</p>".len());
        html.drain(.."<p>".len());
    }
    html
}

#[cfg(test)]
mod tests {
    use super::{inline, to_html};

    #[test]
    fn links_become_anchors_and_markdown_renders() {
        let html = to_html("## Why\nSee [[lantern/relay-pin-is-fixed]] and `[[quoted]]`.\n");
        assert!(html.contains("<h2>Why</h2>"));
        assert!(html.contains(
            "<a href=\"/doc/lantern/relay-pin-is-fixed\">lantern/relay-pin-is-fixed</a>"
        ));
        assert!(html.contains("<code>[[quoted]]</code>"));
    }

    #[test]
    fn a_summary_renders_inline_without_anchors() {
        assert_eq!(
            inline("**Bold** `code` and [[lantern/relay]] <b>"),
            "<strong>Bold</strong> <code>code</code> and <code>lantern/relay</code> &lt;b&gt;"
        );
        assert_eq!(inline("plain"), "plain");
    }
}
