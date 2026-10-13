use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::LazyLock;

use std::net::SocketAddr;

use clap::error::ErrorKind;
use clap::{CommandFactory as _, FromArgMatches as _, Parser, Subcommand, ValueEnum, ValueHint};

use super::{NAME, complete};
use crate::domain::release;
use crate::domain::schema::{KindOf, Reason};
use crate::web::Server;

fn reasons() -> String {
    let mut text = "The reasons a document ends for, by its kind:".to_owned();
    for (word, reasons) in KindOf::endings_by_word() {
        let words: Vec<&str> = reasons.iter().map(Reason::word).collect();
        let _ = write!(text, "\n  {word:<10}{}", words.join(", "));
    }
    text
}

// A command that prints no result has no JSON form.
const NO_JSON: [&str; 2] = ["serve", "completions"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Fish,
    Bash,
    Zsh,
}

static VERSION: LazyLock<String> = LazyLock::new(|| release::current().to_string());

#[derive(Parser)]
#[command(
    name = NAME,
    version = VERSION.as_str(),
    about = "A store of work done, durable facts, follow-ups and topics",
    long_about = "Every document is a chain of immutable versions; a write is a new version and \
                  nothing is edited in place. A document is given by its name, a former name or \
                  the start of its id. stdout carries data only, diagnostics go to stderr, exit \
                  1 is a refusal or a store problem and 2 a usage error."
)]
pub struct Cli {
    /// Structured output instead of text
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Invoked,
}

impl Cli {
    /// The definition a shell is answered from, where `--json` is not offered beside a command
    /// of `NO_JSON`.
    pub fn asked_by_a_shell() -> clap::Command {
        let mut command = Cli::command();
        // A global flag is in a command's own arguments only once the definition is built.
        command.build();
        for name in NO_JSON {
            command = command.mut_subcommand(name, |refusing| {
                refusing.mut_arg("json", |json| json.hide(true))
            });
        }
        command
    }

    /// `--json` is refused beside a command of `NO_JSON`.
    pub fn parsed<I, T>(args: I) -> Result<Cli, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let matches = Cli::command().try_get_matches_from(args)?;
        let cli =
            Cli::from_arg_matches(&matches).map_err(|error| error.format(&mut Cli::command()))?;
        let named = matches.subcommand_name();
        if let Some(command) = named.filter(|name| cli.json && NO_JSON.contains(name)) {
            return Err(Cli::command().error(
                ErrorKind::ArgumentConflict,
                format!("the argument '--json' cannot be used with '{command}'"),
            ));
        }
        Ok(cli)
    }
}

#[derive(Subcommand)]
pub enum Invoked {
    #[command(flatten)]
    Run(Command),
    /// Serve the store as read-only pages until stopped, printing the address to open
    Serve {
        /// The address and port to listen on
        #[arg(long, value_name = "ADDR:PORT", default_value = "127.0.0.1:8080")]
        bind: SocketAddr,
        /// A name a browser may reach this server by, beside localhost and an IP address
        #[arg(long = "host", value_name = "NAME", value_parser = Server::host_name)]
        hosts: Vec<String>,
    },
    /// Configure the coding agents on this host to use this tool, or take that back
    Agents {
        #[command(subcommand)]
        action: AgentsAction,
    },
    /// Bring what `agents install` placed and the fish completions already there up to this
    /// binary, and write nothing else
    Refresh,
    /// The line a shell's startup file holds to take its completions from this binary
    Completions { shell: Shell },
    /// Put the latest release in the place of this binary when it is newer, and bring the
    /// skill, the hook and the fish completions already there up to it
    Upgrade {
        /// Only say whether a newer release exists
        #[arg(long)]
        check: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Subcommand)]
pub enum AgentsAction {
    /// Configure every agent on this host to use this tool
    Install,
    /// Bring what an install placed up to this binary, and write nothing else
    Refresh,
    /// Take back from every agent what an install placed
    Uninstall,
}

#[derive(Subcommand)]
pub enum Command {
    /// Bind this host to its machine topic and record where the store is; done once per host
    ///
    /// With no argument it asks for both on a terminal.
    Init {
        /// The machine topic: an existing topic to bind to, or the name of a new one
        #[arg(requires = "store")]
        name: Option<String>,
        /// The store directory
        #[arg(long, value_hint = ValueHint::DirPath, requires = "name")]
        store: Option<PathBuf>,
        /// What the machine is, for a topic that does not exist yet
        #[arg(long, requires = "name")]
        summary: Option<String>,
        /// Also configure every coding agent on this host to use this tool
        #[arg(long, requires = "name")]
        agents: bool,
    },
    #[command(flatten)]
    Draft(DraftCommand),
    #[command(flatten)]
    Change(ChangeCommand),
    #[command(flatten)]
    Read(ReadCommand),
    #[command(flatten)]
    List(ListCommand),
}

#[derive(Subcommand)]
pub enum DraftCommand {
    /// Open a draft for a new document, or store a followup at once
    New {
        #[command(subcommand)]
        what: NewWhat,
    },
    /// Open a document as it stands as a draft to edit
    Checkout {
        #[arg(add = complete::documents())]
        target: String,
    },
    /// Check a draft and store it as the document's next version
    Save {
        #[arg(add = complete::drafts())]
        target: String,
    },
    /// Delete a draft without storing it
    Discard {
        #[arg(add = complete::drafts())]
        target: String,
    },
    /// Open a draft over every head of a forked document
    Resolve {
        #[arg(add = complete::forked())]
        target: String,
    },
}

#[derive(Subcommand)]
pub enum NewWhat {
    /// A topic: a project, a device, a tool or a subject
    Topic { name: String },
    /// A fact under a topic, as `<topic>/<name>`
    Fact { address: String },
    /// An idea under a topic, as `<topic>/<name>`: a settled design not yet built
    Idea { address: String },
    /// An entry for work done, dated today
    Entry {
        name: String,
        /// Another day, `YYYY-MM-DD`
        #[arg(long)]
        date: Option<String>,
    },
    /// Open work under one or more topics; stored at once with --summary
    Followup {
        /// The topics it belongs to, separated by commas
        #[arg(long, required = true, value_delimiter = ',', add = complete::topics())]
        topics: Vec<String>,
        /// What is open, in one line; given, the followup is stored at once
        #[arg(long)]
        summary: Option<String>,
        /// The rest of it, stored as the body beside --summary
        #[arg(long, requires = "summary")]
        body: Option<String>,
        /// The day to look at it again, `YYYY-MM-DD`
        #[arg(long, requires = "why", conflicts_with = "touching")]
        look_again: Option<String>,
        /// What to look for on that day
        #[arg(long, requires = "look_again")]
        why: Option<String>,
        /// One of its topics, to raise it when work next touches that topic
        #[arg(long, add = complete::topics())]
        touching: Option<String>,
        /// The entry it arose in
        #[arg(long, add = complete::entries())]
        entry: Option<String>,
        /// The document it is about
        #[arg(long, add = complete::documents())]
        about: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ChangeCommand {
    /// Give a topic, a fact or an entry a new name; the old one keeps working
    Rename {
        #[arg(add = complete::renamable())]
        target: String,
        name: String,
    },
    /// File documents under another topic
    Move {
        /// The topic they are under
        #[arg(add = complete::topics())]
        from: String,
        /// The topic to put them under
        #[arg(add = complete::topics())]
        to: String,
        #[arg(required = true, add = complete::movable())]
        targets: Vec<String>,
    },
    /// Record that a fact was confirmed today
    Verify {
        #[arg(add = complete::facts())]
        fact: String,
    },
    /// End a document for a reason its kind has, with a note saying why
    #[command(after_help = reasons())]
    End {
        #[arg(add = complete::documents())]
        target: String,
        /// One of the reasons the document's kind ends for
        #[arg(add = complete::reasons())]
        reason: String,
        /// Why; only `done` goes without one
        note: Option<String>,
        /// The document that ended it
        #[arg(long, add = complete::documents())]
        by: Option<String>,
    },
    /// Bring an ended document back
    Reopen {
        #[arg(add = complete::ended())]
        target: String,
        why: String,
    },
    /// End a followup as done; the note is optional
    Done {
        #[arg(add = complete::followups())]
        followup: String,
        note: Option<String>,
    },
    /// End a followup as dropped, with a note saying why
    Drop {
        #[arg(add = complete::followups())]
        followup: String,
        note: Option<String>,
    },
    /// Set when a followup comes up again: `<date> <why>`, `touching <topic>`, or `none`
    Trigger {
        #[arg(add = complete::followups())]
        followup: String,
        /// A day, `YYYY-MM-DD`, the word `touching`, or `none` to clear the trigger
        #[arg(add = complete::triggers())]
        when: String,
        /// Why on that day, or the topic
        what: Option<String>,
    },
    /// Place a topic in a directory on this machine
    Claim {
        #[arg(add = complete::topics())]
        topic: String,
        /// The working directory when not given
        #[arg(value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
        /// In no directory: it loads in a session no claimed directory covers
        #[arg(long, conflicts_with = "dir")]
        anywhere: bool,
    },
    /// End this machine's claim of a directory for a topic
    Unclaim {
        #[arg(add = complete::topics())]
        topic: String,
        /// The working directory when not given
        #[arg(value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
        /// The claim made in no directory
        #[arg(long, conflicts_with = "dir")]
        anywhere: bool,
    },
}

#[derive(Subcommand)]
pub enum ReadCommand {
    /// Print a document as it stands, every head of a fork, or one version by its id
    Show {
        #[arg(add = complete::documents())]
        target: String,
    },
    /// Every version of a document
    History {
        #[arg(add = complete::documents())]
        target: String,
    },
    /// A draft against what it started from, a version against its parents, or two versions
    Diff {
        #[arg(add = complete::drafts())]
        target: String,
        other: Option<String>,
    },
    /// The newest versions written anywhere in the store
    Log {
        #[arg(default_value_t = 20)]
        n: usize,
        /// Only what this machine topic wrote
        #[arg(long, add = complete::topics())]
        machine: Option<String>,
    },
    /// Documents whose name, summary or text holds the term, facts first
    Search {
        term: String,
        /// Read the term as a regular expression
        #[arg(long)]
        regex: bool,
        /// Only what is under this topic
        #[arg(long, add = complete::topics())]
        topic: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// What a session starting in a directory is opened with
    Context {
        /// The working directory when not given
        #[arg(value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
        /// The agent whose session hook runs this, its input holding the session
        #[arg(long, value_name = "AGENT", hide = true)]
        hook: Option<String>,
    },
    /// The followups that are due for a session in a directory
    Due {
        /// The working directory when not given
        #[arg(value_hint = ValueHint::DirPath)]
        dir: Option<PathBuf>,
    },
    /// Every rule the store has to keep; exit 1 when one is broken
    Check,
    /// How often each command was run, on this machine and on every machine that syncs here
    Usage {
        /// Only what this machine topic ran
        #[arg(long, add = complete::topics())]
        machine: Option<String>,
        /// Only from this day on, `YYYY-MM-DD`
        #[arg(long)]
        since: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ListCommand {
    /// Every draft on this host
    Drafts,
    /// Every topic with what it is, or the topics that are part of one
    Topics {
        /// A topic, to list the topics that are part of it
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// The ended ones too
        #[arg(long, conflicts_with = "topic")]
        ended: bool,
    },
    /// The facts, or those under a topic and its parts
    Facts {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// The ideas, or those under a topic and its parts
    Ideas {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// The entries, newest first, or those under a topic and its parts
    Entries {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// At most this many
        #[arg(short)]
        n: Option<usize>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// Open work, due items first
    Followups {
        /// A topic, or an entry for the followups that name it
        #[arg(add = complete::topics_and_entries())]
        about: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// Where topics are placed on this machine
    Where {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// Another machine topic's placements
        #[arg(long, add = complete::topics())]
        machine: Option<String>,
    },
    /// Documents with more than one current version
    Forks,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Result<Cli, clap::Error> {
        Cli::parsed(line.split(' '))
    }

    #[test]
    fn the_definition_is_one_clap_accepts() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_of_the_list_parses() {
        for line in [
            "worklog init",
            "worklog init --json",
            "worklog init desk --store /atlas/store",
            "worklog init desk --store store --summary desk",
            "worklog new topic lantern",
            "worklog new fact lantern/relay",
            "worklog new idea lantern/dimmer",
            "worklog new entry wiring",
            "worklog new entry wiring --date 2026-10-09",
            "worklog new followup --topics lantern,atlas",
            "worklog new followup --topics lantern --summary s --look-again 2026-11-01 --why w",
            "worklog new followup --topics lantern --touching lantern --entry e --about a",
            "worklog checkout lantern",
            "worklog save lantern",
            "worklog discard lantern",
            "worklog drafts",
            "worklog diff lantern",
            "worklog diff 0a1b 2c3d",
            "worklog resolve lantern",
            "worklog rename lantern lamp",
            "worklog move lantern atlas lantern/relay lantern/fuse",
            "worklog verify lantern/relay",
            "worklog end lantern retired",
            "worklog end lantern/relay superseded note --by lantern/fuse",
            "worklog reopen lantern why",
            "worklog done 0a1b",
            "worklog done 0a1b note",
            "worklog drop 0a1b",
            "worklog drop 0a1b note",
            "worklog trigger 0a1b 2026-11-01 why",
            "worklog trigger 0a1b touching lantern",
            "worklog trigger 0a1b none",
            "worklog claim lantern --anywhere",
            "worklog unclaim lantern --anywhere",
            "worklog claim lantern",
            "worklog claim lantern /atlas/lantern",
            "worklog unclaim lantern",
            "worklog unclaim lantern /atlas/lantern",
            "worklog where",
            "worklog where lantern --machine desk",
            "worklog show lantern",
            "worklog history lantern",
            "worklog log",
            "worklog log 5 --machine desk",
            "worklog search relay",
            "worklog search relay --regex --topic lantern --ended",
            "worklog topics",
            "worklog topics --ended",
            "worklog topics lantern",
            "worklog facts",
            "worklog facts lantern --ended",
            "worklog ideas",
            "worklog ideas lantern --ended",
            "worklog entries",
            "worklog entries lantern -n 3 --ended",
            "worklog followups",
            "worklog followups lantern --ended",
            "worklog forks",
            "worklog context",
            "worklog context /atlas/lantern",
            "worklog due",
            "worklog due /atlas/lantern",
            "worklog check",
            "worklog usage",
            "worklog usage --machine desk --since 2026-10-09",
            "worklog serve",
            "worklog serve --bind 127.0.0.1:0",
            "worklog serve --bind [::1]:8080 --host desk.local --host atlas",
            "worklog completions fish",
            "worklog completions bash",
            "worklog completions zsh",
            "worklog upgrade",
            "worklog upgrade --check",
            "worklog upgrade --check --json",
        ] {
            assert!(parsed(line).is_ok(), "{line}");
        }
    }

    #[test]
    fn json_is_taken_before_and_after_the_command() {
        assert!(parsed("worklog --json topics").unwrap().json);
        assert!(parsed("worklog topics --json").unwrap().json);
        assert!(!parsed("worklog topics").unwrap().json);
    }

    #[test]
    fn the_topics_of_a_followup_split_at_commas() {
        let cli = parsed("worklog new followup --topics lantern,atlas").unwrap();
        let Invoked::Run(Command::Draft(DraftCommand::New {
            what: NewWhat::Followup { topics, .. },
        })) = cli.command
        else {
            panic!("expected a new followup");
        };
        assert_eq!(topics, ["lantern", "atlas"]);
    }

    #[test]
    fn a_log_with_no_count_shows_twenty() {
        let Invoked::Run(Command::Read(ReadCommand::Log { n, .. })) =
            parsed("worklog log").unwrap().command
        else {
            panic!("expected a log");
        };
        assert_eq!(n, 20);
    }

    fn refused(line: &str) -> ErrorKind {
        match parsed(line) {
            Ok(_) => panic!("{line} parsed"),
            Err(error) => error.kind(),
        }
    }

    #[test]
    fn a_command_line_missing_a_part_is_refused() {
        for line in [
            "worklog init desk",
            "worklog init --store kept",
            "worklog init --summary desk",
            "worklog init --agents",
            "worklog new followup",
            "worklog new followup --topics lantern --look-again 2026-11-01",
            "worklog new followup --topics lantern --why w",
            "worklog move lantern atlas",
            "worklog trigger 0a1b",
            "worklog end lantern",
            "worklog rename lantern",
        ] {
            assert_eq!(refused(line), ErrorKind::MissingRequiredArgument, "{line}");
        }
        assert_eq!(
            refused(
                "worklog new followup --topics lantern --look-again 2026-11-01 --why w \
                 --touching lantern"
            ),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            refused("worklog entries -n many"),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            refused("worklog claim lantern /atlas/lantern --anywhere"),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            refused("worklog topics lantern --ended"),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(refused("worklog polish"), ErrorKind::InvalidSubcommand);
        assert_eq!(
            refused("worklog completions"),
            ErrorKind::MissingRequiredArgument
        );
        for shell in ["tcsh", "elvish", "powershell"] {
            assert_eq!(
                refused(&format!("worklog completions {shell}")),
                ErrorKind::InvalidValue
            );
        }
    }

    #[test]
    fn a_shell_is_not_offered_json_where_it_is_refused() {
        let asked = Cli::asked_by_a_shell();
        let hidden = |command: &str| {
            let command = asked.find_subcommand(command).expect("the command");
            let json = command.get_arguments().find(|arg| arg.get_id() == "json");
            json.expect("the global flag").is_hide_set()
        };
        assert!(hidden("serve") && hidden("completions"));
        assert!(!hidden("topics") && !hidden("agents") && !hidden("upgrade"));
        let mut parsed = Cli::command();
        parsed.build();
        let flags = |command: &clap::Command| -> Vec<(String, Vec<String>)> {
            let ids = |sub: &clap::Command| {
                let mut ids: Vec<String> = (sub.get_arguments())
                    .map(|arg| arg.get_id().to_string())
                    .collect();
                ids.sort();
                (sub.get_name().to_owned(), ids)
            };
            let mut every: Vec<_> = command.get_subcommands().map(ids).collect();
            every.sort();
            every
        };
        assert_eq!(flags(&asked), flags(&parsed));
    }

    #[test]
    fn json_is_refused_beside_a_command_with_no_json_form() {
        for line in [
            "worklog serve --json",
            "worklog --json serve --bind 127.0.0.1:0",
            "worklog completions fish --json",
            "worklog --json completions zsh",
        ] {
            let Err(error) = parsed(line) else {
                panic!("{line} parsed");
            };
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{line}");
            let command = if line.contains("serve") {
                "serve"
            } else {
                "completions"
            };
            let said = format!("the argument '--json' cannot be used with '{command}'");
            assert!(error.to_string().contains(&said), "{error}");
        }
    }

    #[test]
    fn serve_listens_on_the_loopback_unless_told_and_takes_each_host_it_is_given() {
        let served = |line: &str| match parsed(line).unwrap().command {
            Invoked::Serve { bind, hosts } => (bind.to_string(), hosts),
            Invoked::Run(_)
            | Invoked::Agents { .. }
            | Invoked::Refresh
            | Invoked::Completions { .. }
            | Invoked::Upgrade { .. } => {
                panic!("{line}: expected a serve")
            }
        };
        assert_eq!(
            served("worklog serve"),
            ("127.0.0.1:8080".to_owned(), Vec::new())
        );
        assert_eq!(
            served("worklog serve --host desk.local --bind [::]:0 --host atlas"),
            (
                "[::]:0".to_owned(),
                vec!["desk.local".to_owned(), "atlas".to_owned()]
            )
        );
        for line in [
            "worklog serve --bind desk",
            "worklog serve --bind localhost:8080",
            "worklog serve --bind 127.0.0.1",
            "worklog serve --bind 127.0.0.1:port",
            "worklog serve --bind 127.0.0.1:99999",
        ] {
            assert_eq!(refused(line), ErrorKind::ValueValidation, "{line}");
        }
        for name in [
            "desk.local:8080",
            "http://desk.local",
            "desk.local/",
            "[::1]",
            "",
            " ",
            "desk local",
            "desk.local\t",
        ] {
            let refusal = Cli::try_parse_from(["worklog", "serve", "--host", name])
                .err()
                .map(|error| error.kind());
            assert_eq!(refusal, Some(ErrorKind::ValueValidation), "{name:?}");
        }
        for line in [
            "worklog serve --json",
            "worklog --json serve --bind 127.0.0.1:0",
        ] {
            assert_eq!(refused(line), ErrorKind::ArgumentConflict, "{line}");
        }
    }

    #[test]
    fn the_help_of_end_lists_the_reasons_of_every_kind() {
        let mut command = Cli::command();
        let end = command.find_subcommand_mut("end").expect("the command");
        let help = end.render_long_help().to_string();
        for line in [
            "  topic     retired, merged",
            "  fact      false, moved, superseded",
            "  idea      built, abandoned",
            "  followup  done, dropped",
        ] {
            assert!(help.contains(line), "{line}: {help}");
        }
    }

    #[test]
    fn agents_takes_an_action_and_init_an_optional_flag() {
        for (word, action) in [
            ("install", AgentsAction::Install),
            ("refresh", AgentsAction::Refresh),
            ("uninstall", AgentsAction::Uninstall),
        ] {
            let Invoked::Agents { action: given } =
                parsed(&format!("worklog agents {word}")).unwrap().command
            else {
                panic!("{word}: expected agents");
            };
            assert_eq!(given, action);
        }
        assert!(parsed("worklog agents").is_err());
        assert!(parsed("worklog agents remove").is_err());
        let init = |line: &str| match parsed(line).unwrap().command {
            Invoked::Run(Command::Init { agents, .. }) => agents,
            _ => panic!("{line}: expected an init"),
        };
        assert!(!init("worklog init desk --store kept"));
        assert!(init("worklog init desk --store kept --agents"));
    }

    #[test]
    fn init_takes_its_name_with_its_store_or_neither() {
        let init = |line: &str| match parsed(line).unwrap().command {
            Invoked::Run(Command::Init { name, store, .. }) => (name, store),
            _ => panic!("{line}: expected an init"),
        };
        assert_eq!(init("worklog init"), (None, None));
        assert_eq!(
            init("worklog init desk --store kept"),
            (Some("desk".to_owned()), Some(PathBuf::from("kept")))
        );
    }

    #[test]
    fn upgrade_replaces_the_binary_unless_it_is_only_to_check() {
        let check = |line: &str| match parsed(line).unwrap().command {
            Invoked::Upgrade { check } => check,
            _ => panic!("{line}: expected an upgrade"),
        };
        assert!(!check("worklog upgrade"));
        assert!(check("worklog upgrade --check"));
        assert_eq!(
            refused("worklog upgrade lantern"),
            ErrorKind::UnknownArgument
        );
    }
}
