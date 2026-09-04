use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// One-time move of the file-per-document store into this empty one
    Migrate {
        /// The old worklog directory with its year subdirectories
        #[arg(long)]
        entries: String,
        /// The old facts directory, holding PROJECTS
        #[arg(long)]
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
        #[arg(long)]
        store: Option<String>,
        /// Also install the agent skill
        #[arg(long, conflicts_with = "no_skill")]
        skill: bool,
        /// Leave the agent skill alone, without asking
        #[arg(long)]
        no_skill: bool,
        /// Also add the session hook to the agent's settings
        #[arg(long, conflicts_with = "no_hook")]
        hook: bool,
        /// Leave the agent's settings alone, without asking
        #[arg(long)]
        no_hook: bool,
    },
    /// The agent skill that ships with this binary
    Skill {
        #[command(subcommand)]
        what: SkillWhat,
    },
    /// The `SessionStart` hook that opens every session with `worklog context`
    Hook {
        #[command(subcommand)]
        what: HookWhat,
    },
    /// Shell completions for the commands of this binary, to source
    Completions { shell: clap_complete::Shell },
}

#[derive(Subcommand)]
pub enum SkillWhat {
    /// Write the skill where the agent reads it
    Install {
        /// Another skills directory than the agent's
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Print the skill
    Show,
}

#[derive(Subcommand)]
pub enum HookWhat {
    /// Merge the hook into the settings file, once
    Install {
        /// Another settings file than the agent's
        #[arg(long)]
        settings: Option<PathBuf>,
    },
    /// Print the hook entry
    Show,
}

#[derive(Subcommand)]
pub enum ReadCommand {
    /// Print a document as it stands, or every head of a fork
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
    Tag { tag: String },
    /// Every tag, most used first
    Tags,
    /// Facts under a topic, ideas apart
    Facts {
        topic: Option<String>,
        /// Also the topics it includes
        #[arg(long)]
        deep: bool,
    },
    /// Ideas under a topic
    Ideas {
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
        topic: Option<String>,
        /// Another machine's layout
        #[arg(long)]
        machine: Option<String>,
    },
    /// Open work, oldest first, with each item's recheck state
    Followups {
        /// A topic, or an entry slug for the items that arose in it
        about: Option<String>,
        /// Closed items too
        #[arg(long)]
        all: bool,
    },
    /// The index a session opens with, for a directory
    Context { dir: Option<String> },
    /// Documents with two current versions
    Forks,
    /// Every rule the store has to keep
    Check,
    /// The draft against the version it came from
    Diff(SlugArg),
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
        slug: SlugArg,
        /// Validate only
        #[arg(long)]
        dry_run: bool,
    },
    /// Delete a draft without saving
    Discard(SlugArg),
    /// Close a followup as done
    Done { slug: String, note: Option<String> },
    /// Close a followup as dropped
    Drop { slug: String, note: Option<String> },
    /// Move the recheck of a followup, fact or idea
    Recheck {
        #[command(flatten)]
        slug: SlugArg,
        /// `<date> <why>` or `touching <topic>`
        #[arg(required = true, num_args = 1..)]
        recheck: Vec<String>,
    },
    /// Record that a fact was confirmed today
    Verify { slug: String },
    /// Remove a document; its slug is never reused
    Tombstone(SlugArg),
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
    pub topic: String,
    /// The directory, the working directory when not given
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
