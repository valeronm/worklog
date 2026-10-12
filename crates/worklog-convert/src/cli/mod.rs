//! stdout carries the report or the summary only and refusals go to stderr; exit 1 is a
//! refusal or a conversion with something open and 2 a usage error.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use clap::Parser;
use worklog_next::app::check::Finding;
use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{Content, Record};

use crate::decisions::Decisions;
use crate::key::{DocumentKey, Folded};
use crate::old;
use crate::plan::{Complete, Document, Outcome, Planned, created_topic_id, list, plan, referenced};
use crate::question::{Authored, Unfiled};
use crate::report::{self, Itself, Run, Standing, report, written_itself};
use crate::write::{Counts, Place, ready};

#[cfg(test)]
mod tests;

const NAME: &str = "worklog-convert";

#[derive(Parser)]
#[command(
    name = NAME,
    about = "Converts a worklog store to the store the rebuilt worklog keeps",
    long_about = "Reads a whole worklog store with its history and plans the store the rebuilt \
                  worklog keeps. Whatever the old store leaves unsaid that the new one needs is \
                  listed as a question, with the lines of a decision file that answer it.\n\n\
                  Without `--into` a run writes nothing: it prints what is blocked, open or \
                  unused and exits 0 when nothing is, with what a run with `--into` would \
                  write. A `\"?\"` in the decision file answers nothing, so the file can be \
                  filled in sittings. With `--into` the new store is written and held to its \
                  own check once nothing is open. The old store is never modified.\n\n\
                  stdout carries the report or the summary only, diagnostics go to stderr, exit \
                  1 is a refusal or a conversion with something open and 2 a usage error."
)]
struct Cli {
    /// The directory of the store to convert, which is only read
    old_store: PathBuf,
    /// The file of answers to what a run lists as open; without it nothing is answered
    #[arg(long, value_name = "FILE")]
    decisions: Option<PathBuf>,
    /// The directory to write the new store to: absent, or a directory holding nothing
    #[arg(long, value_name = "NEW_STORE")]
    into: Option<PathBuf>,
}

struct Ran {
    text: String,
    refusal: Option<String>,
}

impl Ran {
    fn refused(why: String) -> Ran {
        Ran {
            text: String::new(),
            refusal: Some(why),
        }
    }
}

fn joined(cwd: Option<&Path>, given: &Path) -> Result<PathBuf, Ran> {
    if given.is_absolute() {
        return Ok(given.to_path_buf());
    }
    match cwd {
        Some(cwd) => Ok(cwd.join(given)),
        None => Err(Ran::refused("no working directory".to_owned())),
    }
}

fn decisions(file: Option<&Path>) -> Result<Decisions, Ran> {
    let Some(file) = file else {
        return Ok(Decisions::default());
    };
    let text = std::fs::read_to_string(file)
        .map_err(|error| Ran::refused(format!("{}: {error}", file.display())))?;
    Decisions::parse(&text).map_err(|refused| {
        let lines: Vec<String> = refused.iter().map(|line| format!("{line}\n")).collect();
        Ran {
            text: lines.concat(),
            refusal: Some(format!(
                "{}: is not read as a decision file",
                file.display()
            )),
        }
    })
}

// The rebuilt tool's command line is not reachable from this crate, so nothing holds these
// words to it.
const NEXT: &str = "worklog-next";
const BIND: &str = "init";
const STORE: &str = "--store";

fn binding(target: &Path) -> String {
    format!(
        "to bind a host to it: {NEXT} {BIND} <its machine topic> {STORE} {}",
        target.display()
    )
}

fn summary(counts: &Counts, wrote: &str, notices: &[Finding], target: &Path) -> String {
    let mut text = format!("{}\n", counts.shown(true));
    if !wrote.is_empty() {
        let _ = writeln!(text, "{wrote}");
    }
    for notice in notices {
        let _ = writeln!(text, "notice: {}: {}", notice.label, notice.what);
    }
    let _ = writeln!(text, "written to {}\n{}", target.display(), binding(target));
    text
}

fn to_write(counts: &Counts) -> String {
    format!(
        "\n## What a run with `--into` writes\n\n```\n{}\n```\n",
        counts.shown(false)
    )
}

struct Shown {
    topics: BTreeSet<Folded>,
    referred: BTreeMap<String, usize>,
    unfiled: Vec<Unfiled>,
    read: report::Counts,
}

fn referred(planned: &Planned) -> BTreeMap<String, usize> {
    let created: BTreeMap<String, &String> = (planned.authored.iter())
        .filter_map(|authored| match authored {
            Authored::Topic { name, .. } => Some((created_topic_id(name).to_string(), name)),
            _ => None,
        })
        .collect();
    let mut referred: BTreeMap<String, usize> = (created.values())
        .map(|name| ((*name).clone(), 0))
        .collect();
    if created.is_empty() {
        return referred;
    }
    for document in &planned.documents {
        let mut names = BTreeSet::new();
        for version in &document.versions {
            let fields = version.record.as_ref().map(Record::fields);
            let held = (fields.iter()).flat_map(|fields| referenced(document.kind, fields));
            let ids = held.map(|referred| referred.id);
            let ids = ids.chain(version.machine.as_ref().map(DocumentId::as_str));
            let others = ids.filter(|id| *id != document.id.as_str());
            names.extend(others.filter_map(|id| created.get(id).copied()));
        }
        for name in names {
            *referred.entry(name.clone()).or_default() += 1;
        }
    }
    referred
}

fn unfiled(document: &Document) -> Option<Unfiled> {
    let last = document.head()?.record.as_ref()?;
    let Content::Entry(entry) = &last.content else {
        return None;
    };
    if last.ending.is_some() || !entry.topics.is_empty() {
        return None;
    }
    let mut sources = document.versions.iter().rev();
    let source = sources.find_map(|version| version.origin.source())?;
    Some(Unfiled {
        entry: DocumentKey::new(source.document.kind, &source.document.slug).to_string(),
        tags: list(source.version, "tags").to_vec(),
    })
}

impl Shown {
    fn of(store: &old::Store, planned: &Planned) -> Shown {
        let ended = [old::State::Removed, old::State::Renamed];
        let topics = (store.documents.iter())
            .filter(|document| document.kind == old::Kind::Topic)
            .filter(|document| !ended.contains(&document.state))
            .map(|document| Folded::new(&document.slug));
        Shown {
            topics: topics.collect(),
            referred: referred(planned),
            unfiled: planned.documents.iter().filter_map(unfiled).collect(),
            read: report::Counts {
                documents: store.documents.len(),
                versions: (store.documents.iter())
                    .map(|document| document.versions.len())
                    .sum(),
            },
        }
    }

    fn itself<'a>(&'a self, planned: &'a Planned) -> Itself<'a> {
        Itself {
            authored: &planned.authored,
            overridden: &planned.overridden,
            referred: &self.referred,
            unfiled: &self.unfiled,
        }
    }

    fn report(&self, planned: &Planned, decisions: &Decisions) -> String {
        report(&Run {
            questions: &planned.questions,
            wrote: self.itself(planned),
            unused: &planned.unused,
            held: decisions.tables(),
            unanswered: decisions.unanswered(),
            topics: &self.topics,
            read: self.read,
        })
    }
}

fn converted(
    complete: &Complete,
    store: &old::Store,
    place: Option<&Place>,
    listed: &str,
    wrote: &Itself,
) -> Result<Ran, Ran> {
    let refused = |failure: crate::write::Failure| Ran::refused(failure.to_string());
    let ready = ready(complete, store).map_err(|failure| Ran {
        text: place.map_or_else(|| listed.to_owned(), |_| String::new()),
        ..refused(failure)
    })?;
    let Some(place) = place else {
        return Ok(Ran {
            text: format!("{listed}{}", to_write(ready.counts())),
            refusal: None,
        });
    };
    let notices = ready.write(place).map_err(refused)?;
    let wrote = written_itself(wrote);
    Ok(Ran {
        text: summary(ready.counts(), &wrote, &notices, place.target()),
        refusal: None,
    })
}

fn ran(cli: &Cli, cwd: Option<&Path>) -> Result<Ran, Ran> {
    let old = joined(cwd, &cli.old_store)?;
    let file = (cli.decisions.as_deref())
        .map(|file| joined(cwd, file))
        .transpose()?;
    let target = (cli.into.as_deref())
        .map(|target| joined(cwd, target))
        .transpose()?;
    let store = old::read(&old).map_err(|failure| Ran::refused(failure.to_string()))?;
    let place = (target.as_deref())
        .map(|target| Place::checked(target, &old))
        .transpose()
        .map_err(|failure| Ran::refused(failure.to_string()))?;
    let decisions = decisions(file.as_deref())?;
    let outcome = plan(&store, &decisions);
    let planned = outcome.planned();
    let shown = Shown::of(&store, planned);
    let listed = shown.report(planned, &decisions);
    if let Outcome::Complete(complete) = &outcome {
        let wrote = shown.itself(planned);
        return converted(complete, &store, place.as_ref(), &listed, &wrote);
    }
    let standing = Standing::of(&planned.questions, &planned.unused);
    let refusal = match place {
        Some(place) => format!("{}: not written: {standing}", place.target().display()),
        None => format!("{standing}: the report on stdout lists them"),
    };
    Err(Ran {
        text: listed,
        refusal: Some(refusal),
    })
}

fn fail(err: &mut dyn Write, what: &dyn std::fmt::Display) {
    let _ = writeln!(err, "{NAME}: {what}");
}

pub(crate) fn main_in<I, T>(
    args: I,
    cwd: Option<&Path>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // clap sends help to stdout with exit 0.
            let _ = if error.use_stderr() {
                write!(err, "{}", error.render())
            } else {
                write!(out, "{}", error.render())
            };
            return error.exit_code();
        }
    };
    finish(ran(&cli, cwd), out, err)
}

fn finish(ran: Result<Ran, Ran>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (ran, exit) = match ran {
        Ok(ran) => (ran, 0),
        Err(ran) => (ran, 1),
    };
    let printed = out
        .write_all(ran.text.as_bytes())
        .and_then(|()| out.flush());
    if let Some(refusal) = &ran.refusal {
        fail(err, refusal);
    }
    match printed {
        // A reader that stops early closes the stream, and the run still did its work.
        Err(error) if error.kind() != ErrorKind::BrokenPipe => {
            fail(err, &format_args!("stdout: {error}"));
            1
        }
        _ => exit,
    }
}
