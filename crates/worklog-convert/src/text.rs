pub(crate) fn counted(count: usize, noun: &str) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {noun}{plural}")
}

pub(crate) fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

pub(crate) fn cut(text: &str) -> String {
    match text.char_indices().nth(80) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}
