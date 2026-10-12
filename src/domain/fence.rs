//! The shape versions and drafts share: a TOML header between `+++`
//! lines, then a body.

use std::fmt;

const FENCE: &str = "+++";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceError {
    NoOpening,
    NoClosing,
    /// A reader ends the header at its first fence line.
    InHeader,
}

impl fmt::Display for FenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FenceError::NoOpening => "no opening fence",
            FenceError::NoClosing => "no closing fence",
            FenceError::InHeader => "a field holds a line that is only `+++`",
        })
    }
}

/// The header between the fences and the body after them, byte for byte.
pub fn split(text: &str) -> Result<(&str, &str), FenceError> {
    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| rest.strip_prefix('\n'))
        .ok_or(FenceError::NoOpening)?;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.strip_suffix('\n').unwrap_or(line) == FENCE {
            return Ok((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    Err(FenceError::NoClosing)
}

pub fn join(header: &str, body: &str) -> Result<String, FenceError> {
    if header.lines().any(|line| line == FENCE) {
        return Err(FenceError::InHeader);
    }
    let end = if header.is_empty() || header.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Ok(format!("{FENCE}\n{header}{end}{FENCE}\n{body}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_joined_header_and_body_split_back() {
        for (header, body) in [
            ("name = \"relay\"\n", "The relay.\n"),
            ("", ""),
            ("name = \"relay\"\n", "before\n+++\nafter\n"),
        ] {
            let joined = join(header, body).unwrap();
            assert_eq!(split(&joined), Ok((header, body)));
        }
        assert_eq!(
            join("name = 1", "body").unwrap(),
            "+++\nname = 1\n+++\nbody"
        );
        assert_eq!(split("+++\nname = 1\n+++"), Ok(("name = 1\n", "")));
    }

    #[test]
    fn a_header_holding_a_fence_line_is_refused() {
        assert_eq!(
            join("note = \"\"\"\nbefore\n+++\nafter\"\"\"\n", "body"),
            Err(FenceError::InHeader)
        );
        assert!(join("note = \"a +++ b\"\n", "body").is_ok());
    }

    #[test]
    fn a_missing_fence_is_named() {
        assert_eq!(split("name = 1\n"), Err(FenceError::NoOpening));
        assert_eq!(split("+++name = 1\n+++\n"), Err(FenceError::NoOpening));
        assert_eq!(split("+++\nname = 1\n"), Err(FenceError::NoClosing));
        assert_eq!(split("+++\nname = 1\n +++\n"), Err(FenceError::NoClosing));
    }
}
