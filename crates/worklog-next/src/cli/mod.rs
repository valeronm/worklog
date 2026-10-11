//! Argument parsing and rendering. stdout carries data only and refusals and notes go to
//! stderr; exit 1 is a refusal or a store problem and 2 a usage error.

mod agents;
mod args;
mod hook;
mod json;
mod render;
mod run;
mod skill;

use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use crate::app::Failure;
use crate::domain::ports::StoreError;
use crate::domain::schema::Directory;
use crate::{web, wiring};
use args::{Cli, Command, Invoked, ReadCommand};
use render::Output;

const NAME: &str = "worklog-next";

struct Environment<'a> {
    cwd: Option<PathBuf>,
    binary: Option<PathBuf>,
    variable: &'a dyn Fn(&str) -> Option<OsString>,
}

impl Environment<'_> {
    fn working(&self) -> Result<&Path, Failure> {
        self.cwd
            .as_deref()
            .ok_or_else(|| Failure::Refused("no working directory".to_owned()))
    }

    fn binary(&self) -> Result<&Path, Failure> {
        self.binary
            .as_deref()
            .ok_or_else(|| Failure::Refused("the path of this binary is unknown".to_owned()))
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
        binary: std::env::current_exe().ok(),
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

fn answered(command: &Command, json: bool, environment: &Environment) -> Result<Output, Failure> {
    let chosen = match command {
        Command::Init { store, .. } => Some(environment.placed(store)?),
        _ => None,
    };
    let Some(adapters) = wiring::wire(environment.variable, chosen)? else {
        let not_set_up = Failure::not_set_up();
        return match command {
            // A session opens with `context` unasked, on a host set up or not.
            Command::Read(ReadCommand::Context { .. }) => {
                Ok(Output::note(not_set_up.to_string(), json))
            }
            _ => Err(not_set_up),
        };
    };
    run::run(&adapters.deps(), environment, json, command)
}

fn fail(err: &mut dyn Write, what: &dyn std::fmt::Display) {
    let _ = writeln!(err, "{NAME}: {what}");
}

fn announce(
    address: &str,
    bind: &SocketAddr,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> std::io::Result<()> {
    let reach = if bind.ip().is_unspecified() {
        Some("every network this host is on")
    } else if bind.ip().is_loopback() {
        None
    } else {
        Some("whatever reaches this address")
    };
    if let Some(reach) = reach {
        fail(
            err,
            &format_args!(
                "{address}: the store is served without a login to {reach}, and a client that \
                 stalls can block the server"
            ),
        );
    }
    writeln!(out, "http://{address}")?;
    out.flush()
}

fn wired(environment: &Environment) -> Result<wiring::Adapters, Failure> {
    wiring::wire(environment.variable, None)?.ok_or_else(Failure::not_set_up)
}

fn listening(
    bind: &SocketAddr,
    hosts: &[String],
    environment: &Environment,
) -> Result<web::Server, Failure> {
    wired(environment)?;
    web::Server::bind(&bind.to_string(), hosts)
}

fn serve(
    bind: &SocketAddr,
    hosts: &[String],
    environment: &Environment,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let server = match listening(bind, hosts, environment) {
        Ok(server) => server,
        Err(failure) => {
            fail(err, &failure);
            return exit_code(&failure);
        }
    };
    let address = server.address();
    match announce(&address, bind, out, err) {
        // A reader that stops early closes the stream, and the pages are still served.
        Err(error) if error.kind() != ErrorKind::BrokenPipe => {
            fail(err, &format_args!("stdout: {error}"));
            return 1;
        }
        _ => {}
    }
    // A directory store answers from one reading of its tree, so each request has its own.
    let settled = || {
        let adapters = wired(environment)?;
        adapters.settle()?;
        Ok(adapters)
    };
    let ended = server.run(None, &|method, target| match settled() {
        Ok(adapters) => web::respond(&adapters.deps(), method, target),
        Err(Failure::Refused(why)) => web::failed(Failure::Store(StoreError::io("serve", why))),
        Err(failure) => web::failed(failure),
    });
    if let Err(error) = ended {
        fail(err, &format_args!("{address}: {error}"));
    }
    1
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
    let cli = match Cli::parsed(args) {
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
    let command = match &cli.command {
        Invoked::Run(command) => command,
        Invoked::Serve { bind, hosts } => return serve(bind, hosts, environment, out, err),
        Invoked::Agents { action } => {
            return finished(agents::run(*action, environment, cli.json), out, err);
        }
    };
    finished(answered(command, cli.json, environment), out, err)
}

fn finished(answer: Result<Output, Failure>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let output = match answer {
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
            binary: Some(root.join("worklog-next")),
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
    fn serve_prints_the_address_to_open_and_notes_a_bind_to_every_address() {
        let announced = |address: &str| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let bind: SocketAddr = address.parse().unwrap();
            announce(address, &bind, &mut out, &mut err).unwrap();
            (
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        for (address, line) in [
            ("127.0.0.1:8080", "http://127.0.0.1:8080\n"),
            ("127.8.4.1:8080", "http://127.8.4.1:8080\n"),
            ("[::1]:8080", "http://[::1]:8080\n"),
        ] {
            assert_eq!(announced(address), (line.to_owned(), String::new()));
        }
        for (address, line) in [
            ("192.168.7.20:4000", "http://192.168.7.20:4000\n"),
            ("10.0.0.5:8080", "http://10.0.0.5:8080\n"),
            ("[fe80::1]:8080", "http://[fe80::1]:8080\n"),
        ] {
            let note = format!(
                "worklog-next: {address}: the store is served without a login to whatever \
                 reaches this address, and a client that stalls can block the server\n"
            );
            assert_eq!(announced(address), (line.to_owned(), note));
        }
        for (address, line) in [
            ("0.0.0.0:8080", "http://0.0.0.0:8080\n"),
            ("[::]:4000", "http://[::]:4000\n"),
        ] {
            let note = format!(
                "worklog-next: {address}: the store is served without a login to every network \
                 this host is on, and a client that stalls can block the server\n"
            );
            assert_eq!(announced(address), (line.to_owned(), note));
        }

        let bind: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut full = Failing(ErrorKind::StorageFull);
        assert!(announce("127.0.0.1:8080", &bind, &mut full, &mut Vec::new()).is_err());
    }

    #[test]
    fn a_path_is_read_from_the_working_directory_unless_it_starts_at_the_root() {
        let at = |cwd: Option<&str>, given: &str| {
            let environment = Environment {
                cwd: cwd.map(PathBuf::from),
                binary: None,
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

    #[test]
    fn a_command_that_names_the_binary_in_a_file_is_refused_when_its_path_is_unknown() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        std::fs::create_dir_all(root.join("home/.claude")).unwrap();
        let variable = |name: &str| match name {
            "HOME" => Some(root.join("home").into_os_string()),
            _ => None,
        };
        let environment = Environment {
            cwd: None,
            binary: None,
            variable: &variable,
        };
        let ran = |action: &str| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let args = ["worklog-next", "agents", action];
            let code = main_in(args, &environment, &mut out, &mut err);
            assert!(out.is_empty());
            (String::from_utf8(err).unwrap(), code)
        };
        let unknown = (
            "worklog-next: the path of this binary is unknown\n".to_owned(),
            1,
        );
        assert_eq!(ran("install"), unknown);
        assert_eq!(ran("refresh"), (String::new(), 0));
        assert!(!root.join("home/.claude/settings.json").exists());

        let hooks =
            r#"{"hooks": {"SessionStart": [{"hooks": [{"command": "worklog-next context"}]}]}}"#;
        std::fs::write(root.join("home/.claude/settings.json"), hooks).unwrap();
        assert_eq!(ran("refresh"), unknown);
        std::fs::remove_file(root.join("home/.claude/settings.json")).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args = ["worklog-next", "agents", "uninstall"];
        assert_eq!(main_in(args, &environment, &mut out, &mut err), 0);
        assert!(!root.join("home/.claude/settings.json").exists());
    }
}
