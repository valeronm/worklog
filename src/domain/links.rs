//! `[[slug]]` references in a body. One inside an inline code span is
//! quoted, not made, which is how a document can talk about the link form.

fn link_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-')
}

/// The text with every link replaced by what `make` returns for its
/// target; code spans and everything else pass through as they are.
pub fn linked(text: &str, mut make: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        let mut in_span = false;
        loop {
            let tick = rest.find('`');
            let open = if in_span { None } else { rest.find("[[") };
            match (tick, open) {
                (Some(t), o) if o.is_none_or(|o| t < o) => {
                    out.push_str(&rest[..=t]);
                    in_span = !in_span;
                    rest = &rest[t + 1..];
                }
                (_, Some(o)) => {
                    out.push_str(&rest[..o]);
                    let after = &rest[o + 2..];
                    let end = after.find(|c: char| !link_char(c)).unwrap_or(after.len());
                    if end > 0 && after[end..].starts_with("]]") {
                        out.push_str(&make(&after[..end]));
                        rest = &after[end + 2..];
                    } else {
                        out.push_str("[[");
                        rest = after;
                    }
                }
                _ => {
                    out.push_str(rest);
                    break;
                }
            }
        }
    }
    out
}

/// Every link target in `text`, in order, repeats included.
#[must_use]
pub fn targets(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    linked(text, |target| {
        found.push(target.to_owned());
        String::new()
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_and_quotes_spans() {
        let text =
            "see [[lantern/relay]] and [[2026-09/2026-09-04-x]], not `[[quoted]]`\n[[a]] [[b]]";
        assert_eq!(
            targets(text),
            ["lantern/relay", "2026-09/2026-09-04-x", "a", "b"]
        );
    }

    #[test]
    fn malformed_brackets_are_not_links() {
        assert_eq!(targets("[[]] [[has space]] [[ok]]"), ["ok"]);
    }

    #[test]
    fn replaces_links_and_keeps_everything_else() {
        let text = "see [[a/b]], not `[[q]]` or [[bad one]]\n[[c]]";
        assert_eq!(
            linked(text, |t| format!("<{t}>")),
            "see <a/b>, not `[[q]]` or [[bad one]]\n<c>"
        );
    }
}
