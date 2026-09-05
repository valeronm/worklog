//! A body as HTML, with each `[[slug]]` a link to that document's page.

use pulldown_cmark::{Options, Parser, html};

use crate::domain::links;

#[must_use]
pub fn to_html(body: &str) -> String {
    let text = links::linked(body, |target| format!("[{target}](/doc/{target})"));
    let parser = Parser::new_ext(
        &text,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    );
    let mut out = String::with_capacity(text.len() * 2);
    html::push_html(&mut out, parser);
    out
}

#[cfg(test)]
mod tests {
    use super::to_html;

    #[test]
    fn links_become_anchors_and_markdown_renders() {
        let html = to_html("## Why\nSee [[lantern/relay-pin-is-fixed]] and `[[quoted]]`.\n");
        assert!(html.contains("<h2>Why</h2>"));
        assert!(html.contains(
            "<a href=\"/doc/lantern/relay-pin-is-fixed\">lantern/relay-pin-is-fixed</a>"
        ));
        assert!(html.contains("<code>[[quoted]]</code>"));
    }
}
