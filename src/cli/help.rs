//! The top-level help page, with the commands under headings.

use std::fmt::Write as _;

use clap::{Command, Subcommand as _};

use super::args::{ReadCommand, SetupCommand, WriteCommand};

type Augment = fn(Command) -> Command;

/// The headings and the enum whose variants sit under each; a command
/// from none of them, `serve`, `migrate` or `help`, goes under `Other`.
const GROUPS: &[(&str, Augment)] = &[
    ("Setup", SetupCommand::augment_subcommands),
    ("Reads", ReadCommand::augment_subcommands),
    ("Writes", WriteCommand::augment_subcommands),
];

/// The command with its help page listing the commands by group; the
/// rest of the page stays clap's.
#[must_use]
pub fn grouped(mut cmd: Command) -> Command {
    cmd.build();
    let listing = listing(&cmd);
    cmd.after_help(listing).help_template(
        "{about-with-newline}\n{usage-heading} {usage}{after-help}\n\nOptions:\n{options}",
    )
}

fn listing(cmd: &Command) -> String {
    let width = cmd
        .get_subcommands()
        .map(|c| c.get_name().len())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    let mut section = |heading: &str, members: Vec<&Command>| {
        if !out.is_empty() {
            out.push('\n');
        }
        let _ = writeln!(out, "{heading}:");
        for sub in members {
            let about = sub.get_about().map(ToString::to_string).unwrap_or_default();
            let _ = writeln!(out, "  {:width$}  {about}", sub.get_name());
        }
    };
    let mut left: Vec<&Command> = cmd.get_subcommands().collect();
    for (heading, augment) in GROUPS {
        let names: Vec<String> = augment(Command::new(""))
            .get_subcommands()
            .map(|c| c.get_name().to_string())
            .collect();
        let (mine, rest) = left
            .into_iter()
            .partition(|c| names.contains(&c.get_name().to_string()));
        left = rest;
        section(heading, mine);
    }
    section("Other", left);
    out.trim_end().to_string()
}
