//! Which runs reach the store's usage log, and what each is logged as.

use std::ffi::OsString;
use std::path::Path;

use clap::{CommandFactory as _, Subcommand};

use super::args::{Cli, Command, Invoked};
use super::render::Output;
use super::{Environment, exit_code};
use crate::app::record::{Outcome, Run, record};
use crate::app::{Deps, Failure};
use crate::wiring;

const SERVE: &str = "serve";
const INIT: &str = "init";
const JOIN: &str = "join";

// A use of the store is a `Command` or `serve`, and binding a host to one is none; words that
// name no command are a use refused.
fn is_logged(command: &str) -> bool {
    let first = command.split(' ').next().unwrap_or_default();
    let of_the_store =
        !matches!(first, INIT | JOIN) && <Command as Subcommand>::has_subcommand(first);
    first == SERVE || of_the_store || !<Invoked as Subcommand>::has_subcommand(first)
}

struct Line {
    command: String,
    arguments: Vec<String>,
}

fn logged(argv: &[OsString], environment: &Environment) -> Option<Line> {
    let typed = typed(argv);
    let first = typed.iter().find(|word| !word.starts_with('-'));
    if !is_logged(first.map_or("", String::as_str)) || !found_on_path(argv, environment) {
        return None;
    }
    let command = typed_command(&typed);
    Some(Line {
        arguments: arguments(&typed, &command),
        command,
    })
}

fn write(line: &Line, environment: &Environment, deps: &Deps, outcome: Outcome) {
    let Some(directory) = environment.cwd.as_deref() else {
        return;
    };
    // A command that worked does not fail because its line could not be written.
    let _ = record(
        deps,
        &Run {
            version: environment.version,
            command: &line.command,
            arguments: &line.arguments,
            working_directory: &directory.to_string_lossy(),
        },
        outcome,
    );
}

/// Logs the command line `argv` under the command its words get as far as naming.
pub(super) fn ran(argv: &[OsString], environment: &Environment, deps: &Deps, outcome: Outcome) {
    if let Some(line) = logged(argv, environment) {
        write(&line, environment, deps, outcome);
    }
}

// A build under test is not use.
fn found_on_path(argv: &[OsString], environment: &Environment) -> bool {
    let name = argv
        .first()
        .and_then(|invoked| Path::new(invoked).file_name());
    let path = (environment.variable)("PATH");
    match (environment.binary.as_deref(), name, path) {
        (Some(binary), Some(name), Some(path)) => wiring::found_on_path(binary, name, &path),
        _ => false,
    }
}

pub(super) fn failed(failure: &Failure) -> Outcome {
    Outcome {
        exit: exit_code(failure),
        refusal: Some(failure.to_string()),
        hits: None,
    }
}

pub(super) fn outcome(answer: &Result<Output, Failure>) -> Outcome {
    match answer {
        Ok(output) => Outcome {
            exit: output.exit,
            refusal: None,
            hits: output.hits,
        },
        Err(failure) => failed(failure),
    }
}

pub(super) fn refused(argv: &[OsString], environment: &Environment, error: &clap::Error) {
    let Some(line) = logged(argv, environment) else {
        return;
    };
    let Ok(Some(adapters)) = wiring::wire(environment.variable, None) else {
        return;
    };
    let said = error.to_string();
    let first = said.lines().next().unwrap_or_default();
    let outcome = Outcome {
        exit: error.exit_code(),
        refusal: Some(first.strip_prefix("error: ").unwrap_or(first).to_owned()),
        hits: None,
    };
    write(&line, environment, &adapters.deps(), outcome);
}

fn typed(argv: &[OsString]) -> Vec<String> {
    let words = argv.iter().skip(1);
    words
        .map(|word| word.to_string_lossy().into_owned())
        .collect()
}

fn typed_command(typed: &[String]) -> String {
    let root = Cli::command();
    let mut at = &root;
    let mut path = Vec::new();
    for word in typed.iter().filter(|word| !word.starts_with('-')) {
        let Some(named) = at.find_subcommand(word) else {
            break;
        };
        path.push(named.get_name());
        at = named;
    }
    path.join(" ")
}

// A global flag counts wherever it was typed.
fn arguments(typed: &[String], command: &str) -> Vec<String> {
    let mut words = command.split(' ');
    let mut word = words.next();
    let rest = typed.iter().filter(|argument| {
        let named = word == Some(argument.as_str());
        if named {
            word = words.next();
        }
        !named
    });
    rest.cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::NAME;
    use crate::cli::tests::Process;

    fn words(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    fn spelled_one_way(command: &clap::Command) {
        let name = command.get_name();
        assert_eq!(
            command.get_all_aliases().count(),
            0,
            "`{name}` has an alias"
        );
        assert!(
            command.get_short_flag().is_none() && command.get_long_flag().is_none(),
            "`{name}` is spelled as a flag"
        );
        if command.has_subcommands() {
            for argument in command.get_arguments() {
                assert!(
                    !argument.get_action().takes_values(),
                    "`{name}` takes `{}` before its subcommand",
                    argument.get_id()
                );
            }
        }
        command.get_subcommands().for_each(spelled_one_way);
    }

    fn walked_to_its_path(command: &clap::Command, path: &[String]) {
        for sub in command.get_subcommands() {
            let mut deeper = path.to_vec();
            deeper.push(sub.get_name().to_owned());
            assert_eq!(typed_command(&deeper), deeper.join(" "));
            walked_to_its_path(sub, &deeper);
        }
    }

    #[test]
    fn a_command_is_named_by_the_words_typed_for_it() {
        let all = Cli::command();
        spelled_one_way(&all);
        walked_to_its_path(&all, &[]);
        for name in [SERVE, INIT, JOIN] {
            assert!(all.find_subcommand(name).is_some(), "{name}");
        }
        assert!(Cli::parsed([NAME, "topic"]).is_err());
    }

    #[test]
    fn a_command_is_logged_unless_it_is_one_of_these() {
        let all = Cli::command();
        let unlogged: Vec<&str> = (all.get_subcommands().map(clap::Command::get_name))
            .filter(|name| !is_logged(name))
            .collect();
        assert_eq!(
            unlogged,
            [INIT, JOIN, "agents", "refresh", "completions", "upgrade"]
        );
    }

    fn lines_that_parse(command: &clap::Command, path: &[String], found: &mut Vec<Vec<String>>) {
        if !command.has_subcommands() {
            let mut line = path.to_vec();
            let required = command
                .get_arguments()
                .filter(|given| given.is_required_set());
            for argument in required {
                line.extend(argument.get_long().map(|long| format!("--{long}")));
                line.push("lantern".to_owned());
            }
            found.push(line);
        }
        for sub in command.get_subcommands() {
            let mut deeper = path.to_vec();
            deeper.push(sub.get_name().to_owned());
            lines_that_parse(sub, &deeper, found);
        }
    }

    #[test]
    fn every_command_of_the_store_says_a_host_holding_the_config_before_the_rebuild_is_not_set_up()
    {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let left = root.join("home/.config").join(NAME).join("config");
        std::fs::create_dir_all(left.parent().unwrap()).unwrap();
        let text = "machine: desk\nstore: /home/desk/store\n";
        std::fs::write(&left, text).unwrap();
        let process = Process {
            next: None,
            ..Process::under(root)
        };
        let mut lines = Vec::new();
        let all = Cli::command();
        for command in all.get_subcommands() {
            let name = command.get_name();
            if is_logged(name) {
                lines_that_parse(command, &[name.to_owned()], &mut lines);
            }
        }
        for held in [
            &["serve"][..],
            &["topics"],
            &["new", "followup", "--topics", "lantern"],
            &["move", "lantern", "lantern", "lantern"],
        ] {
            assert!(lines.iter().any(|line| line == held), "{held:?}");
        }
        let not_set_up = format!("{NAME}: this host is not set up; run `{NAME} init`\n");
        for line in &lines {
            let words: Vec<&str> = line.iter().map(String::as_str).collect();
            assert!(
                Cli::parsed([&[NAME], &words[..]].concat()).is_ok(),
                "{line:?}"
            );
            let ran = process.run(&words);
            let refused = i32::from(words != ["context"]);
            assert_eq!(
                (ran.out.as_str(), ran.err.as_str(), ran.code),
                ("", not_set_up.as_str(), refused),
                "{line:?}"
            );
        }
        assert_eq!(std::fs::read_to_string(&left).unwrap(), text);
        assert!(!left.with_file_name("config.toml").exists());
    }

    #[test]
    fn a_line_is_named_as_far_as_its_words_name_a_command_parsed_or_refused() {
        for (line, command) in [
            ("facts --no-such-flag", "facts"),
            ("--json facts lantern", "facts"),
            ("new entry", "new entry"),
            ("new widget lantern", "new"),
            ("serve --json", "serve"),
            ("no-such-command", ""),
            ("--no-such-flag", ""),
            ("init --no-such-flag", "init"),
            ("agents", "agents"),
            ("agents remove", "agents"),
            ("completions", "completions"),
            ("completions fish --no-such-flag", "completions"),
            ("upgrade --check", "upgrade"),
            ("upgrade --no-such-flag", "upgrade"),
        ] {
            assert_eq!(typed_command(&words(line)), command, "{line}");
        }
        assert_eq!(typed_command(&[]), "");
        for (line, command) in [
            ("topics", "topics"),
            ("--json facts lantern", "facts"),
            ("new entry wiring --date 2026-10-09", "new entry"),
            ("new followup --topics lantern", "new followup"),
            ("init desk --store kept", "init"),
            ("join desk --store kept", "join"),
            ("agents refresh", "agents refresh"),
            ("refresh", "refresh"),
            ("serve --bind 127.0.0.1:0", "serve"),
            ("completions fish", "completions"),
            ("upgrade --check", "upgrade"),
        ] {
            let typed = words(line);
            assert!(Cli::parsed([&[NAME.to_owned()], &typed[..]].concat()).is_ok());
            assert_eq!(typed_command(&typed), command, "{line}");
        }
        for (command, logged) in [
            ("facts", true),
            ("new entry", true),
            ("", true),
            ("init", false),
            ("join", false),
            ("agents refresh", false),
            ("refresh", false),
            ("completions", false),
            ("upgrade", false),
        ] {
            assert_eq!(is_logged(command), logged, "{command}");
        }
    }

    #[test]
    fn the_arguments_are_everything_typed_but_the_command_s_own_words() {
        for (line, command, rest) in [
            (
                "facts lantern --ended",
                "facts",
                &["lantern", "--ended"][..],
            ),
            ("--json topics", "topics", &["--json"]),
            ("topics --json", "topics", &["--json"]),
            ("new entry wiring", "new entry", &["wiring"]),
            ("new entry new", "new entry", &["new"]),
            (
                "--json new --date 2026-10-09 entry",
                "new entry",
                &["--json", "--date", "2026-10-09"],
            ),
            ("no-such-command", "", &["no-such-command"]),
            ("topics", "topics", &[]),
        ] {
            assert_eq!(arguments(&words(line), command), rest, "{line}");
        }
    }

    #[test]
    fn an_argument_that_is_not_utf_8_is_logged_with_what_of_it_reads() {
        use std::os::unix::ffi::OsStringExt as _;
        let argv = [
            OsString::from(format!("/bench/{NAME}")),
            OsString::from("show"),
            OsString::from_vec(vec![b'l', 0xff]),
        ];
        assert_eq!(typed(&argv), ["show", "l\u{fffd}"]);
        assert_eq!(typed(&[]), [""; 0]);
    }
}
