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

#[cfg(test)]
thread_local! {
    static TRIED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn tried() -> usize {
    TRIED.with(std::cell::Cell::get)
}

/// The link `text` opens with: its target, trimmed, and the bytes the link takes. A link is
/// `[[`, then a target on one line that holds no `[` and more than spaces, then the first
/// `]]`. `Err` is how many bytes to pass before a link can start again, in text that holds
/// no code. Either count ends on a character boundary of `text` and is zero only for an
/// empty text.
pub fn link_at(text: &str) -> Result<(&str, usize), usize> {
    #[cfg(test)]
    TRIED.with(|tried| tried.set(tried.get() + 1));
    let Some(inner) = text.strip_prefix("[[") else {
        return Err(text.chars().next().map_or(0, char::len_utf8));
    };
    let Some((inside, _)) = inner.split_once("]]") else {
        return Err(text.len());
    };
    if let Some(last) = inside.rfind('[') {
        // A link that opens earlier than the character before the last `[` would hold that `[`.
        let before = &text[.."[[".len() + last];
        return Err(before.len() - before.chars().next_back().map_or(0, char::len_utf8));
    }
    let target = inside.trim();
    if target.is_empty() || inside.contains('\n') {
        return Err(1);
    }
    Ok((target, inside.len() + 4))
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
        match link_at(rest) {
            Ok((target, taken)) => {
                if !found.iter().any(|held| held == target) {
                    found.push(target.to_owned());
                }
                rest = &rest[taken..];
            }
            Err(passed) => {
                let code = rest[1..passed].find('`').map_or(passed, |at| at + 1);
                rest = &rest[code..];
            }
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
    use super::{link_at, links, tried};

    #[test]
    fn a_link_is_read_where_one_opens_and_what_to_pass_is_told_where_none_does() {
        assert_eq!(link_at("[[atlas]] and more"), Ok(("atlas", 9)));
        assert_eq!(
            link_at("[[ lantern/relay-pin ]]]"),
            Ok(("lantern/relay-pin", 23))
        );
        assert_eq!(link_at("[[a]][[b]]"), Ok(("a", 5)));
        assert_eq!(link_at("[atlas]]"), Err(1));
        assert_eq!(link_at("atlas"), Err(1));
        assert_eq!(link_at("[[ ]] [[a]]"), Err(1));
        assert_eq!(link_at("[[]]"), Err(1));
        assert_eq!(link_at("[[at\nlas]]"), Err(1));
        assert_eq!(link_at("[[atlas and [[desk"), Err(18));
        assert_eq!(link_at("[[[atlas]]]"), Err(1));
        assert_eq!(link_at("[[a [b [[c]] d"), Err(7));
        assert_eq!(link_at(&"[[a [b [[c]] d"[7..]), Ok(("c", 5)));
        assert_eq!(link_at("[[a [b]]"), Err(3));
    }

    #[test]
    fn what_to_pass_never_ends_inside_a_character() {
        assert_eq!(link_at("[[é[x]]"), Err(2));
        assert_eq!(link_at("[[тема[черновик]]"), Err(8));
        assert_eq!(link_at("[[é[[x]]"), Err(4));
        assert_eq!(link_at(&"[[é[[x]]"[4..]), Ok(("x", 5)));
        assert_eq!(link_at("[[ тема ]]é"), Ok(("тема", 14)));
        assert_eq!(link_at("é[[x]]"), Err(2));
        assert_eq!(link_at(""), Err(0));
        for text in [
            "[[é[x]]",
            "[[тема[черновик]]",
            "é[[é]]é",
            "[[é[é[[é]é]]é",
            "[é[[é[é]]é]",
            "[[日[[本]]語[[",
            "[[é\n[ü]]",
            "[[ é ]]",
            "[[é",
        ] {
            for (at, _) in text.char_indices() {
                let rest = &text[at..];
                let count = match link_at(rest) {
                    Ok((_, taken)) => taken,
                    Err(passed) => passed,
                };
                assert!(count > 0 && rest.is_char_boundary(count), "{rest}: {count}");
            }
        }
    }

    #[test]
    fn a_line_holding_characters_of_several_bytes_is_scanned() {
        assert!(links("[[é[x]]\n").is_empty());
        assert_eq!(links("[[тема[черновик]] и [[итог]]\n"), ["итог"]);
        assert_eq!(
            links("é[[é]]é [[é[[x]]é `é[[y]]é` [[ ü ]]\n"),
            ["é", "x", "ü"]
        );
        assert_eq!(links("[[é ` [[b]] ` [[日本]]\n"), ["日本"]);
    }

    #[test]
    fn a_line_of_brackets_is_scanned_without_trying_each_as_a_link() {
        let line = format!("{}]] then [[atlas]]\n", "[".repeat(200_000));
        let before = tried();
        assert_eq!(links(&line), ["atlas"]);
        assert!(tried() - before <= 8, "{} tries", tried() - before);
    }

    #[test]
    fn a_code_span_that_opens_in_what_is_passed_still_quotes() {
        assert!(links("[[a ` [[b]] ` c\n").is_empty());
        assert_eq!(links("[[a ` [[b]] c\n"), ["b"]);
        assert_eq!(links("[[a [ ` [[b]] ` [[c]]\n"), ["c"]);
    }

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
