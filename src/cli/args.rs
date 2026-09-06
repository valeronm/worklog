use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum, ValueHint};

use super::complete;

use crate::domain::slug::Kind;

#[derive(Parser)]
#[command(
    name = "worklog",
    version,
    about = "A store of work done, durable facts, follow-ups and topics",
    long_about = "Every document is a chain of immutable versions; a write is a new version and \
                  nothing is edited in place. stdout carries data only, diagnostics go to stderr, \
                  exit 1 is a refusal and 2 a usage error."
)]
pub struct Cli {
    /// Structured output instead of text
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum KindArg {
    Entry,
    Fact,
    Topic,
    Followup,
}

impl From<KindArg> for Kind {
    fn from(k: KindArg) -> Kind {
        match k {
            KindArg::Entry => Kind::Entry,
            KindArg::Fact => Kind::Fact,
            KindArg::Topic => Kind::Topic,
            KindArg::Followup => Kind::Followup,
        }
    }
}

/// A slug, resolved by shape unless the kind is named.
#[derive(Args)]
pub struct SlugArg {
    #[arg(add = complete::slugs())]
    pub slug: String,
    /// Settle a slug whose shape fits more than one kind
    #[arg(long)]
    pub kind: Option<KindArg>,
}

/// The slug of a draft on this machine, resolved the same way.
#[derive(Args)]
pub struct DraftArg {
    #[arg(add = complete::drafts())]
    pub slug: String,
    /// Settle a slug whose shape fits more than one kind
    #[arg(long)]
    pub kind: Option<KindArg>,
}

/// The commands, grouped by what they need: setup runs before a store
/// exists, reads never write, writes need the machine name.
#[derive(Subcommand)]
pub enum Command {
    #[command(flatten)]
    Setup(SetupCommand),
    #[command(flatten)]
    Store(StoreCommand),
}

/// What needs the store open.
#[derive(Subcommand)]
pub enum StoreCommand {
    #[command(flatten)]
    Read(ReadCommand),
    #[command(flatten)]
    Write(WriteCommand),
    /// The store as read-only web pages, until killed
    Serve {
        /// The address to listen on
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: String,
    },
    /// One-time move of the file-per-document store into this empty one
    Migrate {
        /// The old worklog directory with its year subdirectories
        #[arg(long, value_hint = ValueHint::DirPath)]
        entries: String,
        /// The old facts directory, holding PROJECTS
        #[arg(long, value_hint = ValueHint::DirPath)]
        facts: String,
    },
}

/// What runs before a store is opened.
#[derive(Subcommand)]
pub enum SetupCommand {
    /// Name this machine and place its store; done once per host.
    /// Without a name on a terminal, asks for everything with defaults offered
    Init {
        machine: Option<String>,
        /// The store directory, `~/worklog` when not given
        #[arg(long, value_hint = ValueHint::DirPath)]
        store: Option<String>,
        /// Also install the skill and the session hook for the coding agents
        #[arg(long, conflicts_with = "no_agents")]
        agents: bool,
        /// Leave the coding agents alone, without asking
        #[arg(long)]
        no_agents: bool,
    },
    /// The skill and the `SessionStart` hook for the coding agents on this
    /// host
    Agents {
        #[command(subcommand)]
        what: AgentsWhat,
    },
    /// The line a shell's startup file holds to take completions from
    /// this binary
    Completions { shell: clap_complete::Shell },
    /// Put the latest release in the place of this binary when it is
    /// newer, and bring the completions and the agents up to it
    Upgrade {
        /// Only say whether a newer release exists; exit 1 when one does
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
pub enum AgentsWhat {
    /// Write the skill and merge the hook, once, for every agent present
    Install,
    /// Bring the skill, the hook and the fish completions up to this
    /// binary wherever they already are
    Refresh,
    /// Remove the skill and the hook from every agent that has them
    Uninstall,
}

#[derive(Subcommand)]
pub enum ReadCommand {
    /// Print a document as it stands, or every head of a fork; or one
    /// stored version, given its id or a prefix of it
    Show(SlugArg),
    /// Every version of a document, newest first
    History(SlugArg),
    /// Every live document of a kind
    List {
        #[arg(long, default_value = "entry")]
        kind: KindArg,
    },
    /// The newest entries
    Recent {
        #[arg(default_value_t = 10)]
        n: usize,
    },
    /// The newest versions written anywhere in the store, from any machine
    Log {
        #[arg(default_value_t = 20)]
        n: usize,
        /// Only what this machine wrote
        #[arg(long)]
        machine: Option<String>,
    },
    /// Documents whose text holds the term, facts first
    Search {
        #[arg(required = true)]
        term: Vec<String>,
        /// Read the term as a regular expression
        #[arg(long)]
        regex: bool,
    },
    /// Facts and entries carrying a tag
    Tag {
        #[arg(add = complete::tags())]
        tag: String,
    },
    /// Every tag, most used first
    Tags,
    /// Facts under a topic, ideas apart
    Facts {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// Also the topics it includes
        #[arg(long)]
        deep: bool,
    },
    /// Ideas under a topic
    Ideas {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// Also the topics it includes
        #[arg(long)]
        deep: bool,
    },
    /// Every topic with what it is
    Topics,
    /// Where a topic lives on this machine, or every claim here, with a
    /// directory this host lacks marked
    Where {
        #[arg(add = complete::topics())]
        topic: Option<String>,
        /// Another machine's layout
        #[arg(long)]
        machine: Option<String>,
    },
    /// Open work, oldest first, with each item's recheck state
    Followups {
        /// A topic, or an entry slug for the items that arose in it
        #[arg(add = complete::topics())]
        about: Option<String>,
        /// Closed items too
        #[arg(long)]
        all: bool,
    },
    /// The index a session opens with, for a directory
    Context {
        #[arg(value_hint = ValueHint::DirPath)]
        dir: Option<String>,
    },
    /// Documents with two current versions
    Forks,
    /// Every rule the store has to keep
    Check,
    /// How often each command was run, on this machine and on every
    /// machine that syncs here
    Usage {
        /// Only what this machine ran
        #[arg(long)]
        machine: Option<String>,
        /// Only from this day on, `YYYY-MM-DD`
        #[arg(long)]
        since: Option<String>,
    },
    /// A slug: the draft against the version it came from. An id: that
    /// version against its parent. Two ids: between them, earlier first
    Diff {
        #[command(flatten)]
        first: DraftArg,
        other: Option<String>,
    },
    /// Every draft on this machine
    Drafts,
}

#[derive(Subcommand)]
pub enum WriteCommand {
    /// Open a draft for a new document
    New {
        #[command(subcommand)]
        what: NewWhat,
    },
    /// Open the current version as a draft
    Checkout(SlugArg),
    /// Validate, stamp, hash and store the draft
    Save {
        #[command(flatten)]
        slug: DraftArg,
        /// Validate only
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete a draft without saving
    Discard(DraftArg),
    /// Close a followup as done
    Done {
        #[arg(add = complete::followups())]
        slug: String,
        note: Option<String>,
    },
    /// Close a followup as dropped
    Drop {
        #[arg(add = complete::followups())]
        slug: String,
        note: Option<String>,
    },
    /// Move the recheck of a followup, fact or idea
    Recheck {
        #[command(flatten)]
        slug: SlugArg,
        /// `<date> <why>` or `touching <topic>`
        #[arg(required = true, num_args = 1..)]
        recheck: Vec<String>,
    },
    /// Record that a fact was confirmed today
    Verify {
        #[arg(add = complete::facts())]
        slug: String,
    },
    /// Remove a document; its slug is never reused
    Tombstone {
        #[command(flatten)]
        slug: SlugArg,
        /// Why, naming what ended it and linking to where
        note: String,
    },
    /// Move a document to a new slug
    Rename {
        #[command(flatten)]
        slug: SlugArg,
        new: String,
    },
    /// Open a draft holding every head of a fork
    Resolve(SlugArg),
    /// Claim a directory for a topic on this machine
    Claim(ClaimArg),
    /// Drop a claim from this machine
    Unclaim(ClaimArg),
}

/// A claim named on the command line.
#[derive(Args)]
pub struct ClaimArg {
    #[arg(add = complete::topics())]
    pub topic: String,
    /// The directory, the working directory when not given
    #[arg(value_hint = ValueHint::DirPath)]
    pub dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum NewWhat {
    /// An entry dated today
    Entry {
        name: String,
        #[arg(long)]
        date: Option<String>,
    },
    /// A fact under a topic, as `<topic>/<name>`
    Fact {
        slug: String,
    },
    /// An idea under a topic, as `<topic>/<name>`: a settled design not yet built
    Idea {
        slug: String,
    },
    Topic {
        name: String,
    },
    /// Open work arising from an entry; written at once with --summary
    Followup {
        name: String,
        #[arg(long)]
        entry: String,
        #[arg(long)]
        summary: Option<String>,
        /// `<date> <why>` or `touching <topic>`
        #[arg(long)]
        recheck: Option<String>,
        /// Defaults to the entry's tags
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
    },
}
