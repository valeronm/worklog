/// Compiled in, so the skill an agent is given is the one written for this binary's commands.
pub const SKILL: &str = include_str!("../../skill/SKILL.md");

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use clap::error::ErrorKind;
    use clap::{Arg, Command, CommandFactory as _};

    use super::SKILL;
    use crate::cli::NAME;
    use crate::cli::args::Cli;
    use crate::wiring;

    const HELP: &str = "help";

    const NOT_FOR_A_SESSION: [&str; 10] = [
        "init",
        "join",
        "serve",
        "agents",
        "refresh",
        "usage",
        "completions",
        "upgrade",
        "activity",
        HELP,
    ];

    struct Quoted {
        path: String,
        rest: Vec<String>,
    }

    fn root() -> Command {
        let mut root = Cli::command();
        root.build();
        root
    }

    fn words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut in_quotes = false;
        for c in line.chars() {
            if c == '"' {
                in_quotes = !in_quotes;
            }
            if c.is_whitespace() && !in_quotes {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
        words
    }

    fn is_word(text: &str) -> bool {
        text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !text.starts_with('-')
    }

    fn spans(text: &str) -> Vec<String> {
        let mut inside = false;
        let (mut prose, mut lines) = (String::new(), Vec::new());
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                inside = !inside;
            } else if inside {
                lines.push(line.trim_start().to_owned());
            } else {
                prose.push_str(line);
                prose.push(' ');
            }
        }
        let inline = prose.split('`').skip(1).step_by(2).map(str::to_owned);
        inline.chain(lines).collect()
    }

    fn path_len(root: &Command, words: &[String]) -> usize {
        let nested = (words.first())
            .and_then(|first| root.find_subcommand(first))
            .is_some_and(Command::has_subcommands)
            && words.get(1).is_some_and(|second| is_word(second));
        if nested { 2 } else { 1 }
    }

    fn quoted(text: &str, root: &Command) -> Vec<Quoted> {
        let mut found = Vec::new();
        for span in spans(text) {
            let line = span.strip_prefix(NAME).unwrap_or_default();
            if !line.starts_with(char::is_whitespace) {
                continue;
            }
            let mut words = words(line);
            if words.is_empty() {
                continue;
            }
            let rest = words.split_off(path_len(root, &words));
            found.push(Quoted {
                path: words.join(" "),
                rest,
            });
        }
        found
    }

    fn command<'c>(root: &'c Command, path: &str) -> Option<&'c Command> {
        path.split(' ')
            .try_fold(root, |command, name| command.find_subcommand(name))
    }

    fn mentioned(text: &str) -> BTreeSet<String> {
        (quoted(text, &root()).into_iter())
            .map(|line| line.path)
            .collect()
    }

    fn unknown(text: &str) -> Vec<String> {
        let root = root();
        (mentioned(text).into_iter())
            .filter(|path| command(&root, path).is_none())
            .collect()
    }

    fn flag<'c>(command: &'c Command, word: &str) -> Option<&'c Arg> {
        command.get_arguments().find(|arg| {
            let long = arg.get_long().map(|long| format!("--{long}"));
            let short = arg.get_short().map(|short| format!("-{short}"));
            long.as_deref() == Some(word) || short.as_deref() == Some(word)
        })
    }

    fn unbracketed(word: &str) -> &str {
        word.trim_start_matches('[').trim_end_matches(']')
    }

    fn refusal(path: &str, rest: &[&String]) -> Option<ErrorKind> {
        let given = rest.iter().map(|word| match unbracketed(word) {
            flag if flag.starts_with('-') => flag,
            placeholder if placeholder.starts_with('<') || placeholder != *word => "1",
            value => value.trim_matches('"'),
        });
        let args = [NAME].into_iter().chain(path.split(' '));
        Some(Cli::parsed(args.chain(given)).err()?.kind())
    }

    fn bracketing(rest: &[String]) -> impl Iterator<Item = (&String, bool)> {
        let mut inside = false;
        rest.iter().map(move |word| {
            let bracketed = inside || word.starts_with('[');
            inside = bracketed && !word.ends_with(']');
            (word, bracketed)
        })
    }

    fn unbracketed_words(rest: &[String]) -> Vec<&String> {
        (bracketing(rest).filter(|(_, bracketed)| !bracketed))
            .map(|(word, _)| word)
            .collect()
    }

    // Dropping a positional quoted as optional moves the words after it into its place, which
    // the parse without it accepts.
    fn required_in_brackets<'r>(command: &Command, rest: &'r [String]) -> Option<&'r str> {
        let mut positionals = command.get_positionals().peekable();
        let mut words = bracketing(rest);
        while let Some((word, bracketed)) = words.next() {
            let bare = unbracketed(word);
            if bare.starts_with('-') {
                if flag(command, bare).is_some_and(|arg| arg.get_action().takes_values()) {
                    words.next();
                }
                continue;
            }
            let arg = positionals.peek()?;
            if bracketed && arg.is_required_set() {
                return Some(word);
            }
            let variadic = (arg.get_num_args()).is_some_and(|count| count.max_values() > 1);
            if !variadic {
                positionals.next();
            }
        }
        None
    }

    fn misquoted(text: &str) -> Vec<String> {
        let root = root();
        let mut wrong = Vec::new();
        for Quoted { path, rest } in quoted(text, &root) {
            let Some(named) = command(&root, &path).filter(|_| !rest.is_empty()) else {
                continue;
            };
            let whole: Vec<&String> = rest.iter().collect();
            if let Some(kind) = refusal(&path, &whole) {
                wrong.push(format!("`{path}` is refused: {kind:?}"));
            } else if let Some(kind) = refusal(&path, &unbracketed_words(&rest)) {
                wrong.push(format!(
                    "`{path}` is refused without what is quoted as optional: {kind:?}"
                ));
            } else if let Some(word) = required_in_brackets(named, &rest) {
                wrong.push(format!(
                    "`{path}` requires what is quoted as optional: {word}"
                ));
            }
        }
        wrong
    }

    fn every_command(root: &Command) -> Vec<&Command> {
        let nested = root.get_subcommands().flat_map(Command::get_subcommands);
        root.get_subcommands().chain(nested).collect()
    }

    fn misflagged(text: &str) -> Vec<String> {
        let root = root();
        let mut wrong = Vec::new();
        for span in spans(text) {
            let words = words(&span);
            let Some(first) = words.first().filter(|first| *first != NAME) else {
                continue;
            };
            let path = words[..path_len(&root, &words)].join(" ");
            let named = command(&root, &path).filter(|_| is_word(first));
            let flags = (words.iter().map(|word| unbracketed(word)))
                .filter(|word| word.starts_with('-') && word.contains(char::is_alphabetic));
            for word in flags {
                match named {
                    Some(command) if flag(command, word).is_none() => {
                        wrong.push(format!("`{path}` takes no {word}"));
                    }
                    None if every_command(&root)
                        .iter()
                        .all(|command| flag(command, word).is_none()) =>
                    {
                        wrong.push(format!("no command takes {word}"));
                    }
                    _ => {}
                }
            }
        }
        wrong
    }

    #[test]
    fn every_flag_the_skill_names_is_one_its_command_takes() {
        assert_eq!(misflagged(SKILL), Vec::<String>::new());
    }

    #[test]
    fn a_flag_no_command_takes_is_found_in_a_span_without_the_binary_s_name() {
        for (span, found) in [
            ("search --tag lantern", "`search` takes no --tag"),
            ("facts --regex", "`facts` takes no --regex"),
            (
                "new followup --tags <tags>",
                "`new followup` takes no --tags",
            ),
            ("--tags", "no command takes --tags"),
            ("[--recheck <date>]", "no command takes --recheck"),
            ("-x <count>", "no command takes -x"),
        ] {
            assert_eq!(
                misflagged(&format!("with `{span}` given")),
                [found],
                "{span}"
            );
        }
        let held = "`search --topic`, `--ended`, `-n <count>`, `--by\n  <document>`, `--json`, \
                    `new followup --about <document>`, `topics = [\"lantern\"]`, `due <date>`, \
                    `no\ntrigger` and `worklog facts --tag`";
        assert_eq!(misflagged(held), Vec::<String>::new());
    }

    #[test]
    fn a_command_line_in_a_fenced_block_is_read() {
        let text = "Run:\n\n```\nworklog polish\nworklog rename <target>\n```\n";
        assert_eq!(unknown(text), ["polish"]);
        assert_eq!(
            misquoted(text),
            ["`rename` is refused: MissingRequiredArgument"]
        );
    }

    fn unnamed(text: &str) -> Vec<String> {
        let root = root();
        let named = |word: &String| root.find_subcommand(word).is_some();
        (spans(text).into_iter())
            .filter(|span| {
                matches!(words(span).as_slice(), [first, second, ..]
                    if first.starts_with(NAME) && first != NAME && named(second))
            })
            .collect()
    }

    #[test]
    fn a_command_line_under_another_name_is_found() {
        let text = "Run `worklog-old check` or `worklogish show lantern`, not `show lantern`,\n\
                    `worklog check` or `ls -l`.\n```\nworklog-0.2 log\n```\n";
        assert_eq!(
            unnamed(text),
            [
                "worklog-old check",
                "worklogish show lantern",
                "worklog-0.2 log"
            ]
        );
    }

    #[test]
    fn the_binary_the_skill_and_its_directory_share_one_name() {
        assert_eq!(wiring::SKILL_DIRECTORY, NAME);
        assert!(SKILL.starts_with(&format!("---\nname: {NAME}\ndescription: ")));
        assert_eq!(unnamed(SKILL), Vec::<String>::new());
        assert!(!mentioned(SKILL).is_empty());
    }

    #[test]
    fn every_command_the_skill_names_exists() {
        assert_eq!(unknown(SKILL), Vec::<String>::new());
    }

    fn not_commands<'n>(root: &Command, names: &[&'n str]) -> Vec<&'n str> {
        let absent = |name: &&str| root.find_subcommand(name).is_none();
        names.iter().copied().filter(absent).collect()
    }

    #[test]
    fn a_command_the_skill_is_excused_from_is_a_command() {
        assert_eq!(not_commands(&root(), &NOT_FOR_A_SESSION), [""; 0]);
        assert_eq!(not_commands(&root(), &["init", "polish"]), ["polish"]);
        assert_eq!(not_commands(&root(), &["install"]), ["install"]);
    }

    #[test]
    fn the_skill_covers_the_commands_a_session_uses() {
        let mut every = BTreeSet::new();
        for sub in root().get_subcommands() {
            let name = sub.get_name();
            if NOT_FOR_A_SESSION.contains(&name) {
                continue;
            }
            let nested: Vec<&str> = (sub.get_subcommands().map(Command::get_name))
                .filter(|nested| *nested != HELP)
                .collect();
            if nested.is_empty() {
                every.insert(name.to_owned());
            }
            every.extend(nested.iter().map(|nested| format!("{name} {nested}")));
        }
        let mentioned = mentioned(SKILL);
        let missing: Vec<&String> = every.difference(&mentioned).collect();
        assert_eq!(missing, Vec::<&String>::new());
    }

    #[test]
    fn the_skill_quotes_arguments_as_the_cli_takes_them() {
        assert_eq!(misquoted(SKILL), Vec::<String>::new());
    }

    #[test]
    fn the_bare_word_and_a_line_outside_a_code_span_name_no_command() {
        let text = "`worklog` is the binary, worklog polish is prose, and so are \
                    `worklogish polish` and `/worklog polish`.";
        assert_eq!(mentioned(text), BTreeSet::new());
    }

    #[test]
    fn a_command_is_named_by_its_first_word_and_its_subcommand() {
        let text = "`worklog new\n   fact <topic>/<name>`, `worklog show <target>`, \
                    `worklog polish` and `worklog new widget lantern`";
        let named: Vec<String> = mentioned(text).into_iter().collect();
        assert_eq!(named, ["new fact", "new widget", "polish", "show"]);
        assert_eq!(unknown(text), ["new widget", "polish"]);
    }

    #[test]
    fn a_command_line_the_cli_would_refuse_is_found() {
        for (line, found) in [
            (
                "worklog rename <target>",
                "`rename` is refused: MissingRequiredArgument",
            ),
            (
                "worklog end [target] <reason>",
                "`end` is refused without what is quoted as optional: MissingRequiredArgument",
            ),
            (
                "worklog new followup [--topics <topic>] --summary \"Order a fuse\"",
                "`new followup` is refused without what is quoted as optional: \
                 MissingRequiredArgument",
            ),
            (
                "worklog show <target> <other>",
                "`show` is refused: UnknownArgument",
            ),
            (
                "worklog facts --regex",
                "`facts` is refused: UnknownArgument",
            ),
            (
                "worklog new followup --summary \"Order a fuse\"",
                "`new followup` is refused: MissingRequiredArgument",
            ),
            (
                "worklog entries lantern -n",
                "`entries` is refused: InvalidValue",
            ),
            (
                "worklog new followup --topics lantern --summary --entry <entry>",
                "`new followup` is refused: InvalidValue",
            ),
            (
                "worklog claim <topic> <dir> --anywhere",
                "`claim` is refused: ArgumentConflict",
            ),
            (
                "worklog topics <topic> --ended",
                "`topics` is refused: ArgumentConflict",
            ),
            (
                "worklog new followup --topics <topic> --look-again <date>",
                "`new followup` is refused: MissingRequiredArgument",
            ),
        ] {
            assert_eq!(misquoted(&format!("`{line}`")), [found], "{line}");
        }
    }

    #[test]
    fn a_required_argument_quoted_as_optional_is_found() {
        for (line, found) in [
            (
                "worklog diff [target] <other>",
                "`diff` requires what is quoted as optional: [target]",
            ),
            (
                "worklog trigger [followup] <date> <why>",
                "`trigger` requires what is quoted as optional: [followup]",
            ),
        ] {
            assert_eq!(misquoted(&format!("`{line}`")), [found], "{line}");
        }
    }

    #[test]
    fn a_command_line_the_cli_takes_is_passed() {
        for line in [
            "worklog show",
            "worklog diff <target> [other]",
            "worklog end <target> <reason> [note] [--by <document>]",
            "worklog end [--by <document>] <target> <reason>",
            "worklog drop <followup> <note>",
            "worklog move <from> <to> <targets…>",
            "worklog move lantern atlas lantern/relay lantern/fuse",
            "worklog trigger <followup> none",
            "worklog entries [topic] [-n <count>] --ended",
            "worklog new followup --topics lantern,atlas\n  --summary \"Order a fuse\"",
            "worklog init <name> --store <dir>",
            "worklog context --json",
        ] {
            assert_eq!(misquoted(&format!("`{line}`")), Vec::<String>::new());
        }
    }
}
