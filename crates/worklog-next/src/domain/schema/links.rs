//! The `[[target]]` references in a body.

/// Backticks opening a line with more backticks after them are an
/// inline span, not a fence.
fn fence(line: &str) -> Option<char> {
    let line = line.trim_start();
    ['`', '~'].into_iter().find(|marker| {
        let rest = line.trim_start_matches(*marker);
        line.len() - rest.len() >= 3 && !(*marker == '`' && rest.contains('`'))
    })
}

fn past_closing(text: &str, run: usize) -> Option<&str> {
    let mut rest = text;
    while let Some(at) = rest.find('`') {
        let after = rest[at..].trim_start_matches('`');
        if rest.len() - at - after.len() == run {
            return Some(after);
        }
        rest = after;
    }
    None
}

fn in_line(line: &str, found: &mut Vec<String>) {
    let mut rest = line;
    while let Some(at) = rest.find(['`', '[']) {
        rest = &rest[at..];
        if rest.starts_with('`') {
            let after = rest.trim_start_matches('`');
            // A run with no closing run of its length quotes nothing.
            rest = past_closing(after, rest.len() - after.len()).unwrap_or(after);
            continue;
        }
        let target = rest
            .strip_prefix("[[")
            .and_then(|inner| inner.split_once("]]"))
            .map(|(target, after)| (target.trim(), after))
            .filter(|(target, _)| !target.is_empty() && !target.contains('['));
        match target {
            Some((target, after)) => {
                if !found.iter().any(|held| held == target) {
                    found.push(target.to_owned());
                }
                rest = after;
            }
            None => rest = &rest[1..],
        }
    }
}

/// Each target once, in the order first met, leaving out what sits in
/// an inline code span or a fenced block.
#[must_use]
pub fn links(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut open = None;
    for line in body.lines() {
        match (fence(line), open) {
            (Some(marker), None) => open = Some(marker),
            (Some(marker), Some(opened)) if marker == opened => open = None,
            (_, Some(_)) => {}
            (None, None) => in_line(line, &mut found),
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::links;

    #[test]
    fn each_target_is_found_once_in_order() {
        assert_eq!(
            links(
                "See [[lantern/relay-pin]] and [[ atlas ]], then [[lantern/relay-pin]] again.\nAlso [[7f3a91c0]].\n"
            ),
            ["lantern/relay-pin", "atlas", "7f3a91c0"]
        );
        assert!(links("Nothing here.\n").is_empty());
    }

    #[test]
    fn what_is_quoted_as_code_is_not_a_link() {
        assert_eq!(
            links("Write `[[lantern]]` to link, as [[atlas]] does.\n"),
            ["atlas"]
        );
        assert_eq!(
            links("Or ``[[lantern]]`` with two, as [[atlas]] does.\n"),
            ["atlas"]
        );
        assert_eq!(
            links("A lone ` quotes nothing, so [[atlas]] links.\n"),
            ["atlas"]
        );
    }

    #[test]
    fn a_fenced_block_is_skipped_to_the_fence_that_closes_it() {
        assert_eq!(
            links(
                "Before [[phone]].\n```\n[[lantern]]\n~~~\n[[lantern]]\n```\nAfter [[desk]].\n~~~text\n[[atlas]]\n~~~\n"
            ),
            ["phone", "desk"]
        );
        assert_eq!(
            links("```rust``` opens no fence, so [[atlas]] links.\nAnd [[desk]] after it.\n"),
            ["atlas", "desk"]
        );
    }

    #[test]
    fn brackets_that_close_nothing_are_no_link() {
        assert!(links("An empty [[ ]] is none.\n").is_empty());
        assert!(links("Open [[lantern and never closed.\n").is_empty());
        assert_eq!(
            links("[[[lantern]]] and [single] [[atlas]]\n"),
            ["lantern", "atlas"]
        );
    }
}
