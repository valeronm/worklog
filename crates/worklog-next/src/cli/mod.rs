//! Argument parsing and rendering. stdout carries data only and refusals and notes go to
//! stderr; exit 1 is a refusal or a store problem and 2 a usage error.

mod args;
mod json;
mod render;
mod run;

use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use clap::Parser as _;

use crate::app::Failure;
use crate::domain::schema::Directory;
use crate::wiring;
use args::{Cli, Command, ReadCommand};
use render::Output;

const NAME: &str = "worklog-next";

struct Environment<'a> {
    cwd: Option<PathBuf>,
    variable: &'a dyn Fn(&str) -> Option<OsString>,
}

impl Environment<'_> {
    fn working(&self) -> Result<&Path, Failure> {
        self.cwd
            .as_deref()
            .ok_or_else(|| Failure::Refused("no working directory".to_owned()))
    }

    fn joined(&self, given: &Path) -> Result<PathBuf, Failure> {
        if given.is_absolute() {
            Ok(given.to_path_buf())
        } else {
            Ok(self.working()?.join(given))
        }
    }

    fn placed(&self, given: &Path) -> Result<PathBuf, Failure> {
        let whole = self.joined(given)?;
        Ok(match whole.to_str() {
            Some(text) => PathBuf::from(Directory::folded(text)),
            None => whole,
        })
    }
}

/// Runs the process's own command line and returns its exit code.
#[must_use]
pub fn main() -> i32 {
    let environment = Environment {
        cwd: std::env::current_dir().ok(),
        variable: &|name| std::env::var_os(name),
    };
    main_in(
        std::env::args_os(),
        &environment,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    )
}

fn exit_code(failure: &Failure) -> i32 {
    match failure {
        Failure::Refused(_) | Failure::Store(_) => 1,
        Failure::Usage(_) => 2,
    }
}

fn command(cli: &Cli, environment: &Environment) -> Result<Output, Failure> {
    let chosen = match &cli.command {
        Command::Init { store, .. } => Some(environment.placed(store)?),
        _ => None,
    };
    let Some(adapters) = wiring::wire(environment.variable, chosen)? else {
        let not_set_up = Failure::not_set_up();
        return match cli.command {
            // A session opens with `context` unasked, on a host set up or not.
            Command::Read(ReadCommand::Context { .. }) => {
                Ok(Output::note(not_set_up.to_string(), cli.json))
            }
            _ => Err(not_set_up),
        };
    };
    run::run(&adapters.deps(), environment, cli.json, &cli.command)
}

fn fail(err: &mut dyn Write, what: &dyn std::fmt::Display) {
    let _ = writeln!(err, "{NAME}: {what}");
}

fn main_in<I, T>(
    args: I,
    environment: &Environment,
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
            // clap sends help and the version to stdout with exit 0.
            let _ = if error.use_stderr() {
                write!(err, "{}", error.render())
            } else {
                write!(out, "{}", error.render())
            };
            return error.exit_code();
        }
    };
    let output = match command(&cli, environment) {
        Ok(output) => output,
        Err(failure) => {
            fail(err, &failure);
            return exit_code(&failure);
        }
    };
    for note in &output.notes {
        fail(err, note);
    }
    match out
        .write_all(output.text.as_bytes())
        .and_then(|()| out.flush())
    {
        // A reader that stops early closes the stream, and the command still did its work.
        Err(error) if error.kind() != ErrorKind::BrokenPipe => {
            fail(err, &format_args!("stdout: {error}"));
            1
        }
        _ => output.exit,
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    struct Ran {
        out: String,
        err: String,
        code: i32,
    }

    fn run_to(
        root: &Path,
        cwd: Option<&Path>,
        line: &[&str],
        out: &mut dyn Write,
    ) -> (String, i32) {
        let variable = |name: &str| match name {
            "WORKLOG_NEXT_HOME" => Some(root.join("next").into_os_string()),
            "HOME" => Some(root.join("home").into_os_string()),
            _ => None,
        };
        let environment = Environment {
            cwd: cwd.map(Path::to_path_buf),
            variable: &variable,
        };
        let mut err = Vec::new();
        let args = [&["worklog-next"], line].concat();
        let code = main_in(args, &environment, out, &mut err);
        (String::from_utf8(err).unwrap(), code)
    }

    fn run_in(root: &Path, cwd: Option<&Path>, line: &[&str]) -> Ran {
        let mut out = Vec::new();
        let (err, code) = run_to(root, cwd, line, &mut out);
        Ran {
            out: String::from_utf8(out).unwrap(),
            err,
            code,
        }
    }

    fn run(root: &Path, line: &[&str]) -> Ran {
        run_in(root, Some(&root.join("home")), line)
    }

    fn init(root: &Path) -> Ran {
        let line = ["init", "desk", "--store", "kept", "--summary", "A machine"];
        run(root, &line)
    }

    struct Failing(ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_path_is_read_from_the_working_directory_unless_it_starts_at_the_root() {
        let at = |cwd: Option<&str>, given: &str| {
            let environment = Environment {
                cwd: cwd.map(PathBuf::from),
                variable: &|_| None,
            };
            environment.placed(Path::new(given))
        };
        let from_desk = |given: &str| at(Some("/home/desk"), given).unwrap();
        assert_eq!(from_desk("store"), PathBuf::from("/home/desk/store"));
        assert_eq!(
            from_desk("./atlas/store"),
            PathBuf::from("/home/desk/atlas/store")
        );
        assert_eq!(
            from_desk("../atlas/./store"),
            PathBuf::from("/home/atlas/store")
        );
        assert_eq!(from_desk("../../../.."), PathBuf::from("/"));
        assert_eq!(
            from_desk("/atlas/case/../store"),
            PathBuf::from("/atlas/store")
        );
        assert_eq!(at(None, "/atlas/store"), Ok(PathBuf::from("/atlas/store")));
        assert_eq!(
            at(None, "store"),
            Err(Failure::Refused("no working directory".to_owned()))
        );
    }

    #[test]
    fn context_on_a_host_that_is_not_set_up_is_a_note_and_no_failure() {
        let scratch = tempfile::tempdir().unwrap();
        let ran = run(scratch.path(), &["context"]);
        assert_eq!(ran.out, "");
        assert_eq!(
            ran.err,
            "worklog-next: this host is not set up; run `worklog-next init`\n"
        );
        assert_eq!(ran.code, 0);
        let json = run(scratch.path(), &["context", "--json"]);
        assert_eq!((json.out.as_str(), json.code), ("null\n", 0));
        assert_eq!(json.err, ran.err);

        let refused = run(scratch.path(), &["check"]);
        assert_eq!((refused.out.as_str(), refused.code), ("", 1));
        assert_eq!(refused.err, ran.err);
    }

    #[test]
    fn an_init_that_is_refused_leaves_no_store_and_no_config() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let ran = run(root, &["init", "desk", "--store", "kept"]);
        assert_eq!(ran.out, "");
        assert!(ran.err.contains("--summary"), "{}", ran.err);
        assert_eq!(ran.code, 2);
        assert!(!root.join("home/kept").exists());
        assert!(!root.join("next").exists());
    }

    #[test]
    fn a_process_with_no_working_directory_is_refused_only_what_needs_one() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let nowhere = run_in(root, None, &["init", "desk", "--store", "kept"]);
        assert_eq!(nowhere.err, "worklog-next: no working directory\n");
        assert_eq!(nowhere.code, 1);

        init(root);
        let listed = run_in(root, None, &["topics"]);
        assert!(
            listed.out.ends_with("  desk  A machine\n"),
            "{}",
            listed.out
        );
        assert_eq!((listed.out.lines().count(), listed.code), (1, 0));
        let context = run_in(root, None, &["context"]);
        assert_eq!(context.err, "worklog-next: no working directory\n");
        assert_eq!((context.out.as_str(), context.code), ("", 1));
    }

    #[test]
    fn a_damaged_config_is_a_store_failure_naming_the_file() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        init(root);
        let damaged = root.join("next/config.toml");
        std::fs::write(&damaged, "store = 4\n").unwrap();
        for line in [&["topics"][..], &["context"]] {
            let ran = run(root, line);
            assert_eq!(ran.out, "");
            let named = format!("worklog-next: {}", damaged.display());
            assert!(ran.err.starts_with(&named), "{}", ran.err);
            assert_eq!(ran.code, 1);
        }
    }

    #[test]
    fn a_stdout_that_fails_is_a_failure_unless_its_reader_closed_it() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        init(root);
        let home = root.join("home");

        let mut full = Failing(ErrorKind::StorageFull);
        let (err, code) = run_to(root, Some(&home), &["topics"], &mut full);
        assert!(err.starts_with("worklog-next: stdout: "), "{err}");
        assert_eq!(code, 1);

        let mut closed = Failing(ErrorKind::BrokenPipe);
        assert_eq!(
            run_to(root, Some(&home), &["topics"], &mut closed),
            (String::new(), 0)
        );
        let mut closed = Failing(ErrorKind::BrokenPipe);
        let (err, code) = run_to(root, Some(&home), &["save", "lantern"], &mut closed);
        assert_eq!(
            (err.as_str(), code),
            ("worklog-next: lantern: no draft\n", 1)
        );
    }
}
