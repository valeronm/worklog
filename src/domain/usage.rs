//! One line per command run, in the log a machine keeps beside the store.

use super::machine::MachineName;

/// A command as it ran: what was asked for, from where, and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub written: String,
    pub machine: MachineName,
    /// The command path, `new entry` for a command taking a subcommand.
    pub command: String,
    pub exit: i32,
    /// The working directory, spelled as a claim spells one.
    pub directory: String,
    pub arguments: Vec<String>,
}

/// A tab parts the fields and a newline ends the record, so neither can
/// stand for itself in a value.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

impl Invocation {
    #[must_use]
    pub fn to_line(&self) -> String {
        let mut fields = vec![
            escaped(&self.written),
            escaped(self.machine.as_str()),
            escaped(&self.command),
            self.exit.to_string(),
            escaped(&self.directory),
        ];
        fields.extend(self.arguments.iter().map(|a| escaped(a)));
        format!("{}\n", fields.join("\t"))
    }

    /// A line a sync delivered half-written reads as nothing, rather than
    /// stopping a listing the rest of the file can answer.
    #[must_use]
    pub fn parse_line(line: &str) -> Option<Invocation> {
        let mut fields = line.split('\t');
        let written = unescaped(fields.next()?);
        let machine = MachineName::parse(&unescaped(fields.next()?)).ok()?;
        let command = unescaped(fields.next()?);
        let exit = unescaped(fields.next()?).parse().ok()?;
        let directory = unescaped(fields.next()?);
        Some(Invocation {
            written,
            machine,
            command,
            exit,
            directory,
            arguments: fields.map(unescaped).collect(),
        })
    }

    /// The month the line belongs to, which names the file it lands in.
    #[must_use]
    pub fn month(&self) -> &str {
        self.written.get(..7).unwrap_or(&self.written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ran(command: &str, arguments: &[&str]) -> Invocation {
        Invocation {
            written: "2026-09-04T10:00:00.123456+01:00".into(),
            machine: MachineName::parse("desk").unwrap(),
            command: command.into(),
            exit: 0,
            directory: "~/projects/lantern".into(),
            arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn a_line_round_trips_with_its_arguments() {
        let invocation = ran("facts", &["lantern", "--deep"]);
        let line = invocation.to_line();
        assert!(line.ends_with('\n'));
        assert_eq!(
            line.trim_end(),
            "2026-09-04T10:00:00.123456+01:00\tdesk\tfacts\t0\t~/projects/lantern\tlantern\t--deep"
        );
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(invocation));
    }

    #[test]
    fn a_tab_or_a_newline_in_an_argument_stays_in_its_field() {
        let invocation = ran("search", &["one\ttwo\nthree", "back\\slash"]);
        let line = invocation.to_line();
        assert_eq!(line.matches('\t').count(), 6);
        assert_eq!(line.matches('\n').count(), 1);
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(invocation));
    }

    #[test]
    fn a_torn_or_empty_line_reads_as_nothing() {
        assert_eq!(Invocation::parse_line(""), None);
        assert_eq!(
            Invocation::parse_line("2026-09-04T10:00:00+01:00\tdesk"),
            None
        );
        assert_eq!(
            Invocation::parse_line("2026-09-04T10:00:00+01:00\tdesk\tshow\tlater\t~"),
            None
        );
    }

    #[test]
    fn the_month_names_the_file() {
        assert_eq!(ran("show", &[]).month(), "2026-09");
    }
}
