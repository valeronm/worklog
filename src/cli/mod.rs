//! Argument parsing and rendering. stdout carries data only; diagnostics go
//! to stderr; exit 1 is a refusal and 2 a usage error.

pub mod args;
pub mod render;

use std::io::{IsTerminal, Write as _};

use clap::Parser;
use serde::Serialize;

use crate::app::write::{Made, NewFollowup};
use crate::app::{Deps, Failure, migrate, read, slug_arg, write};
use crate::domain::machine::MachineName;
use crate::domain::slug::Kind;
use crate::fs::{Config, FileIdentity, FsDrafts, FsStore, Paths, SystemClock};

use args::{Cli, Command, NewWhat, ReadCommand, SlugArg, WriteCommand};

/// One command's stdout, and the exit code `check` sets on problems.
struct Rendered {
    text: String,
    exit: i32,
}

/// The output as text, or as JSON when asked; only the one printed is built.
fn rendered<T: Serialize>(
    json: bool,
    value: &T,
    text: impl FnOnce() -> String,
) -> Result<Rendered, Failure> {
    let text = if json {
        serde_json::to_string_pretty(value)
            .map_err(|e| Failure::Refused(format!("cannot serialise output: {e}")))?
            + "\n"
    } else {
        text()
    };
    Ok(Rendered { text, exit: 0 })
}

fn slug(arg: &SlugArg) -> Result<crate::domain::slug::Slug, Failure> {
    slug_arg(&arg.slug, arg.kind.map(Kind::from))
}

/// The directory a context is asked for, resolved so claims match it.
fn directory(dir: Option<String>) -> Result<String, Failure> {
    let path = match dir {
        Some(dir) => {
            std::fs::canonicalize(&dir).map_err(|e| Failure::Usage(format!("{dir}: {e}")))?
        }
        None => std::env::current_dir()
            .map_err(|e| Failure::Refused(format!("no working directory: {e}")))?,
    };
    Ok(path.display().to_string())
}

/// A note on stderr when a listing came back empty; stdout stays data.
fn note_empty(empty: bool, what: &str, scope: Option<String>, joint: &str) {
    if empty {
        eprintln!(
            "{what}{}",
            scope.map(|s| format!("{joint}{s}")).unwrap_or_default()
        );
    }
}

fn dispatch_read(deps: &Deps, json: bool, command: ReadCommand) -> Result<Rendered, Failure> {
    match command {
        ReadCommand::Show(arg) => {
            let out = read::show(deps, &slug(&arg)?)?;
            rendered(json, &out, || render::shown(&out))
        }
        ReadCommand::History(arg) => {
            let out = read::history(deps, &slug(&arg)?)?;
            rendered(json, &out, || render::history(&out))
        }
        ReadCommand::List { kind } => {
            let out = read::list(deps, kind.into())?;
            rendered(json, &out, || render::listing(&out))
        }
        ReadCommand::Recent { n } => {
            let out = read::recent(deps, n)?;
            rendered(json, &out, || render::listing(&out))
        }
        ReadCommand::Search { term, regex } => {
            let out = read::search(deps, &term.join(" "), regex)?;
            note_empty(
                out.hits.is_empty(),
                "no documents match",
                Some(out.term.clone()),
                ": ",
            );
            rendered(json, &out, || render::search(&out))
        }
        ReadCommand::Tag { tag } => {
            let out = read::tag(deps, &tag)?;
            note_empty(out.rows.is_empty(), "nothing tagged", Some(tag), ": ");
            rendered(json, &out, || render::listing(&out))
        }
        ReadCommand::Tags => {
            let out = read::tags(deps)?;
            rendered(json, &out, || render::tags(&out))
        }
        ReadCommand::Facts { topic, deep } => {
            let out = read::facts(deps, topic.as_deref(), deep)?;
            note_empty(
                out.facts.is_empty() && out.ideas.is_empty(),
                "no facts",
                topic,
                " for: ",
            );
            rendered(json, &out, || render::fact_listing(&out))
        }
        ReadCommand::Ideas { topic, deep } => {
            let out = read::ideas(deps, topic.as_deref(), deep)?;
            note_empty(out.rows.is_empty(), "no ideas", topic, " for: ");
            rendered(json, &out, || render::listing(&out))
        }
        ReadCommand::Topics => {
            let out = read::topics(deps)?;
            rendered(json, &out, || render::topics(&out))
        }
        ReadCommand::Where { topic, machine } => {
            let out = read::where_(deps, &topic, machine.as_deref())?;
            rendered(json, &out, || render::where_(&out))
        }
        ReadCommand::Followups { topic, all } => {
            let out = read::followups(deps, topic.as_deref(), all)?;
            note_empty(
                out.items.is_empty(),
                "no open follow-ups",
                topic,
                " tagged: ",
            );
            rendered(json, &out, || render::followups(&out))
        }
        ReadCommand::Context { dir } => {
            let out = read::context(deps, &directory(dir)?)?;
            rendered(json, &out, || render::context(&out))
        }
        ReadCommand::Forks => {
            let out = read::forks(deps)?;
            rendered(json, &out, || render::forks(&out))
        }
        ReadCommand::Check => {
            let out = read::check(deps)?;
            let mut r = rendered(json, &out, || render::check(&out))?;
            if !out.problems.is_empty() {
                r.exit = 1;
            }
            Ok(r)
        }
        ReadCommand::Diff(arg) => {
            let out = read::diff(deps, &slug(&arg)?)?;
            rendered(json, &out, || render::diff(&out))
        }
        ReadCommand::Drafts => {
            let out = write::drafts(deps)?;
            rendered(json, &out, || render::drafts(&out))
        }
    }
}

fn dispatch_write(deps: &Deps, json: bool, command: WriteCommand) -> Result<Rendered, Failure> {
    match command {
        WriteCommand::New { what } => {
            let out = match what {
                NewWhat::Entry { name, date } => write::new_entry(deps, &name, date.as_deref())?,
                NewWhat::Fact { slug, idea } => write::new_fact(deps, &slug, idea)?,
                NewWhat::Topic { name } => write::new_topic(deps, &name)?,
                NewWhat::Followup {
                    name,
                    entry,
                    summary,
                    recheck,
                    tags,
                } => {
                    let what = NewFollowup {
                        summary: summary.as_deref(),
                        recheck: recheck.as_deref(),
                        tags: tags.as_deref(),
                    };
                    match write::new_followup(deps, &name, &entry, &what)? {
                        Made::Written(out) => {
                            return rendered(json, &out, || render::written(&out));
                        }
                        Made::Draft(out) => out,
                    }
                }
            };
            rendered(json, &out, || render::draft_ref(&out))
        }
        WriteCommand::Checkout(arg) => {
            let out = write::checkout(deps, &slug(&arg)?)?;
            rendered(json, &out, || render::draft_ref(&out))
        }
        WriteCommand::Save { slug: arg, dry_run } => {
            let out = write::save(deps, &slug(&arg)?, dry_run)?;
            rendered(json, &out, || {
                if dry_run {
                    String::new()
                } else {
                    render::written(&out)
                }
            })
        }
        WriteCommand::Discard(arg) => {
            write::discard(deps, &slug(&arg)?)?;
            rendered(json, &(), String::new)
        }
        WriteCommand::Done { slug, note } => {
            let out = write::done(
                deps,
                &slug_arg(&slug, Some(Kind::Followup))?,
                note.as_deref(),
            )?;
            rendered(json, &out, || render::written(&out))
        }
        WriteCommand::Drop { slug, note } => {
            let out = write::drop_(
                deps,
                &slug_arg(&slug, Some(Kind::Followup))?,
                note.as_deref(),
            )?;
            rendered(json, &out, || render::written(&out))
        }
        WriteCommand::Recheck { slug: arg, recheck } => {
            let out = write::recheck(deps, &slug(&arg)?, &recheck.join(" "))?;
            rendered(json, &out, || render::written(&out))
        }
        WriteCommand::Verify { slug } => {
            let out = write::verify(deps, &slug_arg(&slug, Some(Kind::Fact))?)?;
            rendered(json, &out, || render::written(&out))
        }
        WriteCommand::Tombstone(arg) => {
            let out = write::tombstone(deps, &slug(&arg)?)?;
            rendered(json, &out, || render::written(&out))
        }
        WriteCommand::Rename { slug: arg, new } => {
            let out = write::rename(deps, &slug(&arg)?, &new)?;
            rendered(json, &out, || render::written(&out))
        }
        WriteCommand::Resolve(arg) => {
            let out = write::resolve(deps, &slug(&arg)?)?;
            rendered(json, &out, || render::draft_ref(&out))
        }
    }
}

fn migrate_command(
    deps: &Deps,
    json: bool,
    entries: &str,
    facts: &str,
) -> Result<Rendered, Failure> {
    let facts = std::path::Path::new(facts);
    let out = migrate::migrate(
        deps,
        std::path::Path::new(entries),
        facts,
        &facts.join("PROJECTS"),
    )?;
    rendered(json, &out, || {
        let mut text = format!(
            "migrated {} topics, {} facts, {} entries, {} followups\n",
            out.topics, out.facts, out.entries, out.followups
        );
        for note in &out.notes {
            text.push_str(note);
            text.push('\n');
        }
        text
    })
}

/// One answer from the terminal, or the default on an empty line.
fn ask(prompt: &str, default: &str) -> Result<String, Failure> {
    eprint!("{prompt} [{default}]: ");
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| Failure::Refused(format!("cannot read the answer: {e}")))?;
    let answer = line.trim();
    Ok(if answer.is_empty() {
        default.to_owned()
    } else {
        answer.to_owned()
    })
}

/// The hostname as a default to offer, never as the identity itself.
fn hostname() -> Option<String> {
    let out = std::process::Command::new("uname")
        .arg("-n")
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?;
    let name = name.trim().trim_end_matches(".local").to_ascii_lowercase();
    (!name.is_empty()).then_some(name)
}

/// Records the machine name and the store directory, once per host.
fn init(paths: &Paths, machine: Option<&str>, store: Option<&str>) -> Result<(), Failure> {
    if let Some(existing) = Config::read(&paths.config)? {
        return Err(Failure::Refused(format!(
            "this machine is already named {} with its store at {}",
            existing.machine,
            existing.store.display()
        )));
    }
    let (machine, store) = match machine {
        Some(machine) => (machine.to_owned(), store.map(str::to_owned)),
        None if std::io::stdin().is_terminal() => {
            let machine = ask("Machine name", &hostname().unwrap_or_default())?;
            let store = ask(
                "Store directory",
                &paths.default_store.display().to_string(),
            )?;
            (machine, Some(store))
        }
        None => {
            return Err(Failure::Usage(
                "init needs a machine name when not run from a terminal".into(),
            ));
        }
    };
    let machine = MachineName::parse(&machine)?;
    let store = match store.as_deref() {
        Some(dir) => {
            let dir = std::path::Path::new(dir);
            if dir.is_absolute() {
                dir.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|e| Failure::Refused(format!("no working directory: {e}")))?
                    .join(dir)
            }
        }
        None => paths.default_store.clone(),
    };
    Config { machine, store }.write(&paths.config)?;
    Ok(())
}

fn fail(e: &Failure) -> i32 {
    eprintln!("worklog: {e}");
    e.exit_code()
}

/// Runs the command line and returns the process exit code.
#[must_use]
pub fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // clap prints help and version to stdout with exit 0, and a
            // usage error to stderr with exit 2.
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    let paths = match Paths::from_env() {
        Ok(paths) => paths,
        Err(e) => return fail(&e.into()),
    };
    if let Command::Init { machine, store } = &cli.command {
        return match init(&paths, machine.as_deref(), store.as_deref()) {
            Ok(()) => 0,
            Err(e) => fail(&e),
        };
    }
    let Some(store) = paths.store else {
        // The SessionStart hook runs `context` on a host that may not be
        // set up yet, and a notice is what it should see, not a failure.
        if matches!(cli.command, Command::Read(ReadCommand::Context { .. })) {
            println!(
                "No store on this machine: `worklog init <name> [--store <dir>]` before anything reaches a session."
            );
            return 0;
        }
        eprintln!(
            "worklog: no store on this machine: run `worklog init <name> [--store <dir>]` first"
        );
        return 1;
    };
    let store = FsStore::new(store);
    let drafts = FsDrafts::new(paths.drafts);
    let identity = FileIdentity::new(paths.config);
    let deps = Deps {
        store: &store,
        drafts: &drafts,
        identity: &identity,
        clock: &SystemClock,
        home: paths.home.display().to_string(),
    };
    let result = match cli.command {
        Command::Init { .. } => unreachable!("handled before the store was opened"),
        Command::Read(command) => dispatch_read(&deps, cli.json, command),
        Command::Write(command) => dispatch_write(&deps, cli.json, command),
        Command::Migrate { entries, facts } => migrate_command(&deps, cli.json, &entries, &facts),
    };
    match result {
        Ok(r) => {
            let mut stdout = std::io::stdout().lock();
            // A closed pipe downstream, `worklog list | head`, is not an error.
            let _ = stdout.write_all(r.text.as_bytes());
            let _ = stdout.flush();
            r.exit
        }
        Err(e) => fail(&e),
    }
}
