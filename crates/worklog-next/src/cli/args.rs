use std::fmt::Write as _;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::domain::schema::{KindOf, Reason};

fn reasons() -> String {
    let mut text = "The reasons a document ends for, by its kind:".to_owned();
    for (word, reasons) in KindOf::endings_by_word() {
        let words: Vec<&str> = reasons.iter().map(Reason::word).collect();
        let _ = write!(text, "\n  {word:<10}{}", words.join(", "));
    }
    text
}

#[derive(Parser)]
#[command(
    name = "worklog-next",
    version,
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
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Bind this host to its machine topic and record where the store is; done once per host
    Init {
        /// The machine topic: an existing topic to bind to, or the name of a new one
        name: String,
        /// The store directory
        #[arg(long)]
        store: PathBuf,
        /// What the machine is, for a topic that does not exist yet
        #[arg(long)]
        summary: Option<String>,
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
    Checkout { target: String },
    /// Check a draft and store it as the document's next version
    Save { target: String },
    /// Delete a draft without storing it
    Discard { target: String },
    /// Open a draft over every head of a forked document
    Resolve { target: String },
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
        #[arg(long, required = true, value_delimiter = ',')]
        topics: Vec<String>,
        /// What is open, in one line; given, the followup is stored at once
        #[arg(long)]
        summary: Option<String>,
        /// The day to look at it again, `YYYY-MM-DD`
        #[arg(long, requires = "why", conflicts_with = "touching")]
        look_again: Option<String>,
        /// What to look for on that day
        #[arg(long, requires = "look_again")]
        why: Option<String>,
        /// One of its topics, to raise it when work next touches that topic
        #[arg(long)]
        touching: Option<String>,
        /// The entry it arose in
        #[arg(long)]
        entry: Option<String>,
        /// The document it is about
        #[arg(long)]
        about: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ChangeCommand {
    /// Give a topic, a fact or an entry a new name; the old one keeps working
    Rename { target: String, name: String },
    /// File documents under another topic
    Move {
        /// The topic they are under
        from: String,
        /// The topic to put them under
        to: String,
        #[arg(required = true)]
        targets: Vec<String>,
    },
    /// Record that a fact was confirmed today
    Verify { fact: String },
    /// End a document for a reason its kind has, with a note saying why
    #[command(after_help = reasons())]
    End {
        target: String,
        /// One of the reasons the document's kind ends for
        reason: String,
        /// Why; only `done` goes without one
        note: Option<String>,
        /// The document that ended it
        #[arg(long)]
        by: Option<String>,
    },
    /// Bring an ended document back
    Reopen { target: String, why: String },
    /// End a followup as done; the note is optional
    Done {
        followup: String,
        note: Option<String>,
    },
    /// End a followup as dropped, with a note saying why
    Drop {
        followup: String,
        note: Option<String>,
    },
    /// Set when a followup comes up again: `<date> <why>`, `touching <topic>`, or `none`
    Trigger {
        followup: String,
        /// A day, `YYYY-MM-DD`, the word `touching`, or `none` to clear the trigger
        when: String,
        /// Why on that day, or the topic
        what: Option<String>,
    },
    /// Place a topic in a directory on this machine
    Claim {
        topic: String,
        /// The working directory when not given
        dir: Option<PathBuf>,
        /// In no directory: it loads in a session no claimed directory covers
        #[arg(long, conflicts_with = "dir")]
        anywhere: bool,
    },
    /// End this machine's claim of a directory for a topic
    Unclaim {
        topic: String,
        /// The working directory when not given
        dir: Option<PathBuf>,
        /// The claim made in no directory
        #[arg(long, conflicts_with = "dir")]
        anywhere: bool,
    },
}

#[derive(Subcommand)]
pub enum ReadCommand {
    /// Print a document as it stands, every head of a fork, or one version by its id
    Show { target: String },
    /// Every version of a document
    History { target: String },
    /// A draft against what it started from, a version against its parents, or two versions
    Diff {
        target: String,
        other: Option<String>,
    },
    /// The newest versions written anywhere in the store
    Log {
        #[arg(default_value_t = 20)]
        n: usize,
        /// Only what this machine topic wrote
        #[arg(long)]
        machine: Option<String>,
    },
    /// Documents whose name, summary or text holds the term, facts first
    Search {
        term: String,
        /// Read the term as a regular expression
        #[arg(long)]
        regex: bool,
        /// Only what is under this topic
        #[arg(long)]
        topic: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// What a session starting in a directory is opened with
    Context {
        /// The working directory when not given
        dir: Option<PathBuf>,
    },
    /// Every rule the store has to keep; exit 1 when one is broken
    Check,
}

#[derive(Subcommand)]
pub enum ListCommand {
    /// Every draft on this host
    Drafts,
    /// Every topic with what it is
    Topics {
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// The facts, or those under a topic and its parts
    Facts {
        topic: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// The ideas, or those under a topic and its parts
    Ideas {
        topic: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// The entries, newest first, or those under a topic and its parts
    Entries {
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
        about: Option<String>,
        /// The ended ones too
        #[arg(long)]
        ended: bool,
    },
    /// Where topics are placed on this machine
    Where {
        topic: Option<String>,
        /// Another machine topic's placements
        #[arg(long)]
        machine: Option<String>,
    },
    /// Documents with more than one current version
    Forks,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;
    use clap::error::ErrorKind;

    use super::*;

    fn parsed(line: &str) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(line.split(' '))
    }

    #[test]
    fn the_definition_is_one_clap_accepts() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_of_the_list_parses() {
        for line in [
            "worklog-next init desk --store /atlas/store",
            "worklog-next init desk --store store --summary desk",
            "worklog-next new topic lantern",
            "worklog-next new fact lantern/relay",
            "worklog-next new idea lantern/dimmer",
            "worklog-next new entry wiring",
            "worklog-next new entry wiring --date 2026-10-09",
            "worklog-next new followup --topics lantern,atlas",
            "worklog-next new followup --topics lantern --summary s --look-again 2026-11-01 --why w",
            "worklog-next new followup --topics lantern --touching lantern --entry e --about a",
            "worklog-next checkout lantern",
            "worklog-next save lantern",
            "worklog-next discard lantern",
            "worklog-next drafts",
            "worklog-next diff lantern",
            "worklog-next diff 0a1b 2c3d",
            "worklog-next resolve lantern",
            "worklog-next rename lantern lamp",
            "worklog-next move lantern atlas lantern/relay lantern/fuse",
            "worklog-next verify lantern/relay",
            "worklog-next end lantern retired",
            "worklog-next end lantern/relay superseded note --by lantern/fuse",
            "worklog-next reopen lantern why",
            "worklog-next done 0a1b",
            "worklog-next done 0a1b note",
            "worklog-next drop 0a1b",
            "worklog-next drop 0a1b note",
            "worklog-next trigger 0a1b 2026-11-01 why",
            "worklog-next trigger 0a1b touching lantern",
            "worklog-next trigger 0a1b none",
            "worklog-next claim lantern --anywhere",
            "worklog-next unclaim lantern --anywhere",
            "worklog-next claim lantern",
            "worklog-next claim lantern /atlas/lantern",
            "worklog-next unclaim lantern",
            "worklog-next unclaim lantern /atlas/lantern",
            "worklog-next where",
            "worklog-next where lantern --machine desk",
            "worklog-next show lantern",
            "worklog-next history lantern",
            "worklog-next log",
            "worklog-next log 5 --machine desk",
            "worklog-next search relay",
            "worklog-next search relay --regex --topic lantern --ended",
            "worklog-next topics",
            "worklog-next topics --ended",
            "worklog-next facts",
            "worklog-next facts lantern --ended",
            "worklog-next ideas",
            "worklog-next ideas lantern --ended",
            "worklog-next entries",
            "worklog-next entries lantern -n 3 --ended",
            "worklog-next followups",
            "worklog-next followups lantern --ended",
            "worklog-next forks",
            "worklog-next context",
            "worklog-next context /atlas/lantern",
            "worklog-next check",
        ] {
            assert!(parsed(line).is_ok(), "{line}");
        }
    }

    #[test]
    fn json_is_taken_before_and_after_the_command() {
        assert!(parsed("worklog-next --json topics").unwrap().json);
        assert!(parsed("worklog-next topics --json").unwrap().json);
        assert!(!parsed("worklog-next topics").unwrap().json);
    }

    #[test]
    fn the_topics_of_a_followup_split_at_commas() {
        let cli = parsed("worklog-next new followup --topics lantern,atlas").unwrap();
        let Command::Draft(DraftCommand::New {
            what: NewWhat::Followup { topics, .. },
        }) = cli.command
        else {
            panic!("expected a new followup");
        };
        assert_eq!(topics, ["lantern", "atlas"]);
    }

    #[test]
    fn a_log_with_no_count_shows_twenty() {
        let Command::Read(ReadCommand::Log { n, .. }) = parsed("worklog-next log").unwrap().command
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
            "worklog-next init desk",
            "worklog-next new followup",
            "worklog-next new followup --topics lantern --look-again 2026-11-01",
            "worklog-next new followup --topics lantern --why w",
            "worklog-next move lantern atlas",
            "worklog-next trigger 0a1b",
            "worklog-next end lantern",
            "worklog-next rename lantern",
        ] {
            assert_eq!(refused(line), ErrorKind::MissingRequiredArgument, "{line}");
        }
        assert_eq!(
            refused(
                "worklog-next new followup --topics lantern --look-again 2026-11-01 --why w \
                 --touching lantern"
            ),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            refused("worklog-next entries -n many"),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            refused("worklog-next claim lantern /atlas/lantern --anywhere"),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(refused("worklog-next serve"), ErrorKind::InvalidSubcommand);
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
}
