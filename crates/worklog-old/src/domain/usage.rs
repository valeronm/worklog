//! One line per command run, in the log a machine keeps beside the store.

use serde_json::{Map, Value};

use super::machine::MachineName;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub agent: String,
    pub id: String,
}

/// A command as it ran: what was asked for, from where, and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub written: String,
    pub machine: MachineName,
    /// The command path, `new entry` for a command taking a subcommand.
    pub command: String,
    pub exit: i32,
    /// The message of a run that failed.
    pub refusal: Option<String>,
    /// How many documents a `search` matched.
    pub hits: Option<usize>,
    /// The working directory, spelled as a claim spells one.
    pub directory: String,
    pub arguments: Vec<String>,
    /// The release of the binary that ran.
    pub version: Option<String>,
    /// Absent at a terminal.
    pub session: Option<Session>,
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
        let mut fields = Map::new();
        fields.insert("written".into(), self.written.clone().into());
        fields.insert("machine".into(), self.machine.as_str().into());
        if let Some(version) = &self.version {
            fields.insert("version".into(), version.clone().into());
        }
        if let Some(session) = &self.session {
            fields.insert("agent".into(), session.agent.clone().into());
            fields.insert("session".into(), session.id.clone().into());
        }
        fields.insert("command".into(), self.command.clone().into());
        fields.insert("exit".into(), self.exit.into());
        if let Some(refusal) = &self.refusal {
            fields.insert("refusal".into(), refusal.clone().into());
        }
        if let Some(hits) = self.hits {
            fields.insert("hits".into(), hits.into());
        }
        fields.insert("directory".into(), self.directory.clone().into());
        fields.insert("arguments".into(), self.arguments.clone().into());
        format!("{}\n", Value::Object(fields))
    }

    /// A line a sync delivered half-written reads as nothing, rather than
    /// stopping a listing the rest of the file can answer.
    #[must_use]
    pub fn parse_line(line: &str) -> Option<Invocation> {
        let Value::Object(fields) = serde_json::from_str(line).ok()? else {
            return None;
        };
        let text = |key: &str| Some(fields.get(key)?.as_str()?.to_owned());
        Some(Invocation {
            written: text("written")?,
            machine: MachineName::parse(&text("machine")?).ok()?,
            command: text("command")?,
            exit: i32::try_from(fields.get("exit")?.as_i64()?).ok()?,
            refusal: text("refusal"),
            hits: fields
                .get("hits")
                .and_then(Value::as_u64)
                .and_then(|hits| usize::try_from(hits).ok()),
            directory: text("directory")?,
            arguments: fields
                .get("arguments")?
                .as_array()?
                .iter()
                .map(|a| Some(a.as_str()?.to_owned()))
                .collect::<Option<_>>()?,
            version: text("version"),
            session: text("agent")
                .zip(text("session"))
                .map(|(agent, id)| Session { agent, id }),
        })
    }

    /// Older binaries write this form, and their files stay in the store.
    #[must_use]
    pub fn parse_tab_line(line: &str) -> Option<Invocation> {
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
            refusal: None,
            hits: None,
            directory,
            arguments: fields.map(unescaped).collect(),
            version: None,
            session: None,
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

    fn invocation(command: &str, arguments: &[&str]) -> Invocation {
        Invocation {
            written: "2026-09-04T10:00:00.123456+01:00".into(),
            machine: MachineName::parse("desk").unwrap(),
            command: command.into(),
            exit: 0,
            refusal: None,
            hits: None,
            directory: "~/projects/lantern".into(),
            arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
            version: Some("1.2.3".into()),
            session: Some(Session {
                agent: "Lamp".into(),
                id: "s-1".into(),
            }),
        }
    }

    #[test]
    fn a_line_round_trips_with_its_arguments() {
        let invocation = invocation("facts", &["lantern", "--deep"]);
        let line = invocation.to_line();
        assert_eq!(
            line,
            concat!(
                r#"{"written":"2026-09-04T10:00:00.123456+01:00","machine":"desk","#,
                r#""version":"1.2.3","agent":"Lamp","session":"s-1","command":"facts","exit":0,"#,
                r#""directory":"~/projects/lantern","arguments":["lantern","--deep"]}"#,
                "\n"
            )
        );
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(invocation));
    }

    #[test]
    fn a_run_outside_a_session_carries_no_session_key() {
        let invocation = Invocation {
            session: None,
            ..invocation("show", &[])
        };
        let line = invocation.to_line();
        assert!(
            !line.contains("session") && !line.contains("agent"),
            "{line}"
        );
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(invocation));
    }

    #[test]
    fn an_agent_or_a_session_alone_reads_as_neither() {
        let line = invocation("show", &[]).to_line();
        let read = Invocation::parse_line(&line.replace(r#""agent":"Lamp","#, ""));
        assert_eq!(read.expect("a line").session, None);
    }

    #[test]
    fn a_newline_in_an_argument_stays_on_the_line() {
        let invocation = invocation("search", &["one\ntwo"]);
        let line = invocation.to_line();
        assert_eq!(line.matches('\n').count(), 1);
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(invocation));
    }

    #[test]
    fn a_refusal_and_a_hit_count_sit_beside_the_exit_code() {
        let refused = Invocation {
            exit: 1,
            refusal: Some("no document: lantern/nothing".into()),
            ..invocation("show", &["lantern/nothing"])
        };
        let line = refused.to_line();
        assert!(
            line.contains(r#""exit":1,"refusal":"no document: lantern/nothing","directory""#),
            "{line}"
        );
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(refused));
        let searched = Invocation {
            hits: Some(0),
            ..invocation("search", &["relay"])
        };
        let line = searched.to_line();
        assert!(line.contains(r#""exit":0,"hits":0,"#), "{line}");
        assert_eq!(Invocation::parse_line(line.trim_end()), Some(searched));
    }

    #[test]
    fn a_key_this_binary_does_not_know_is_passed_over() {
        let line = invocation("show", &[]).to_line();
        let later = line.trim_end().replacen('{', r#"{"elapsed":12,"#, 1);
        assert_eq!(
            Invocation::parse_line(&later),
            Some(invocation("show", &[]))
        );
    }

    #[test]
    fn a_torn_or_empty_line_reads_as_nothing() {
        let line = invocation("show", &[]).to_line();
        assert_eq!(Invocation::parse_line(""), None);
        assert_eq!(Invocation::parse_line(&line[..line.len() / 2]), None);
        assert_eq!(
            Invocation::parse_line(&line.replace(r#""exit":0"#, r#""exit":"later""#)),
            None
        );
    }

    #[test]
    fn a_tab_separated_line_reads_without_a_version_or_a_session() {
        let read = Invocation::parse_tab_line(
            "2026-09-04T10:00:00.123456+01:00\tdesk\tsearch\t0\t~/projects/lantern\tone\\ttwo\tback\\\\slash",
        );
        assert_eq!(
            read,
            Some(Invocation {
                version: None,
                session: None,
                ..invocation("search", &["one\ttwo", "back\\slash"])
            })
        );
        assert_eq!(Invocation::parse_tab_line(""), None);
        assert_eq!(
            Invocation::parse_tab_line("2026-09-04T10:00:00+01:00\tdesk\tshow\tlater\t~"),
            None
        );
    }

    #[test]
    fn the_month_names_the_file() {
        assert_eq!(invocation("show", &[]).month(), "2026-09");
    }
}
