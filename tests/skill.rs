//! The skill ships with the binary and names its commands, so a command
//! renamed or removed without the skill following is caught here.

use std::collections::BTreeSet;

use clap::CommandFactory;

use worklog::cli::SKILL;
use worklog::cli::args::Cli;

/// Every `worklog <sub> [<sub>]` path the CLI accepts, and the commands
/// that take a subcommand.
fn commands() -> (BTreeSet<String>, BTreeSet<String>) {
    let mut paths = BTreeSet::new();
    let mut parents = BTreeSet::new();
    for sub in Cli::command().get_subcommands() {
        paths.insert(sub.get_name().to_owned());
        for nested in sub.get_subcommands() {
            parents.insert(sub.get_name().to_owned());
            paths.insert(format!("{} {}", sub.get_name(), nested.get_name()));
        }
    }
    (paths, parents)
}

/// Every `worklog …` mention in the skill, as the command path it names.
fn mentioned(parents: &BTreeSet<String>) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    // A mention may wrap inside its code span, so any whitespace follows
    // the word, not just a space.
    for after in SKILL.split("`worklog").skip(1) {
        let Some(after) = after.strip_prefix(char::is_whitespace) else {
            continue;
        };
        let mut words = after
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .filter(|w| !w.is_empty());
        let Some(first) = words.next() else { continue };
        if first.starts_with("--") || !first.chars().any(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let second = words
            .next()
            .filter(|w| parents.contains(first) && !w.starts_with('<'));
        found.insert(match second {
            Some(second) => format!("{first} {second}"),
            None => first.to_owned(),
        });
    }
    found
}

#[test]
fn every_command_the_skill_names_exists() {
    let (paths, parents) = commands();
    let unknown: Vec<String> = mentioned(&parents)
        .into_iter()
        .filter(|m| !paths.contains(m))
        .collect();
    assert!(
        unknown.is_empty(),
        "the skill names commands the CLI lacks: {unknown:?}"
    );
}

#[test]
fn the_skill_covers_the_commands_a_session_uses() {
    let (_, parents) = commands();
    let m = mentioned(&parents);
    for needed in [
        "context",
        "show",
        "facts",
        "new entry",
        "new fact",
        "new idea",
        "new topic",
        "new followup",
        "checkout",
        "diff",
        "save",
        "discard",
        "done",
        "drop",
        "recheck",
        "verify",
        "tombstone",
        "rename",
        "followups",
        "search",
        "check",
        "forks",
        "resolve",
        "drafts",
    ] {
        assert!(
            m.contains(needed),
            "the skill never mentions `worklog {needed}`"
        );
    }
}

#[test]
fn the_skill_has_frontmatter() {
    assert!(SKILL.starts_with("---\nname: worklog\ndescription: "));
}
