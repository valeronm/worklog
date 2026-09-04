//! `[[slug]]` references in a body. One inside an inline code span is
//! quoted, not made, which is how a document can talk about the link form.

fn link_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-')
}

fn strip_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_span = false;
    for c in line.chars() {
        if c == '`' {
            in_span = !in_span;
        } else if !in_span {
            out.push(c);
        }
    }
    out
}

/// Every link target in `text`, in order, repeats included.
#[must_use]
pub fn targets(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let line = strip_code_spans(line);
        let mut rest = line.as_str();
        while let Some(start) = rest.find("[[") {
            let after = &rest[start + 2..];
            let end = after.find(|c: char| !link_char(c));
            match end {
                Some(end) if after[end..].starts_with("]]") && end > 0 => {
                    found.push(after[..end].to_owned());
                    rest = &after[end + 2..];
                }
                _ => rest = after,
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_and_quotes_spans() {
        let text = "see [[lantern/relay]] and [[2026/2026-09-04-x]], not `[[quoted]]`\n[[a]] [[b]]";
        assert_eq!(
            targets(text),
            ["lantern/relay", "2026/2026-09-04-x", "a", "b"]
        );
    }

    #[test]
    fn malformed_brackets_are_not_links() {
        assert_eq!(targets("[[]] [[has space]] [[ok]]"), ["ok"]);
    }
}
