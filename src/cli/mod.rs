//! Argument parsing and rendering. stdout carries data only and refusals and notes go to
//! stderr; exit 1 is a refusal or a store problem and 2 a usage error.

mod agents;
mod args;
mod complete;
mod hook;
mod json;
mod log;
mod prompt;
mod refresh;
mod render;
mod run;
mod sandbox;
mod skill;
mod upgrade;

use std::cell::RefCell;
use std::ffi::OsString;
use std::io::{BufRead, ErrorKind, IsTerminal as _, Read as _, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::app::record::Outcome;
use crate::app::{Failure, setup};
use crate::domain::ports::{Binary, Releases, StoreError};
use crate::domain::release::{self, Built};
use crate::domain::schema::Directory;
use crate::{web, wiring};
use args::{Cli, Command, Invoked, ReadCommand};
use render::{Output, Stopped};

const NAME: &str = "worklog";

// An agent gives a hook a few hundred bytes.
const HOOK_INPUT: u64 = 64 * 1024;

// An agent that never closes a hook's input must not keep its session from opening.
const HOOK_WAIT: Duration = Duration::from_secs(1);

struct Environment<'a> {
    cwd: Option<PathBuf>,
    binary: Option<PathBuf>,
    variable: &'a dyn Fn(&str) -> Option<OsString>,
    version: Built,
    releases: &'a dyn Fn() -> Box<dyn Releases>,
    binary_at: &'a dyn Fn(&Path) -> Box<dyn Binary>,
    /// `None` unless both the input and stderr are a terminal.
    terminal: Option<&'a RefCell<dyn BufRead + 'a>>,
    /// `None` on a terminal and where the input is not closed within `HOOK_WAIT`; otherwise at
    /// most `HOOK_INPUT` bytes of it.
    hook_input: &'a dyn Fn() -> Option<String>,
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

fn piped_input() -> Option<String> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let (given, read) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if (std::io::stdin().take(HOOK_INPUT))
            .read_to_string(&mut text)
            .is_ok()
        {
            let _ = given.send(text);
        }
    });
    read.recv_timeout(HOOK_WAIT).ok()
}

/// Runs the process's own command line and returns its exit code.
#[must_use]
pub fn main() -> i32 {
    let variable = |name: &str| std::env::var_os(name);
    let stdin = std::io::stdin();
    let prompts = stdin.is_terminal() && std::io::stderr().is_terminal();
    // A lock held on an input that is not a terminal would keep the thread that reads a hook's
    // input waiting for it.
    let typed = prompts.then(|| RefCell::new(stdin.lock()));
    let environment = Environment {
        cwd: std::env::current_dir().ok(),
        binary: std::env::current_exe().ok(),
        variable: &variable,
        version: release::current(),
        releases: &|| wiring::releases(&variable),
        binary_at: &|path| wiring::binary(path, NAME),
        terminal: (typed.as_ref()).map(|typed| -> &RefCell<dyn BufRead> { typed }),
        hook_input: &piped_input,
    };
    let args: Vec<OsString> = std::env::args_os().collect();
    let cwd = environment.cwd.as_deref();
    if let Some(code) = complete::answered(&args, cwd, &mut std::io::stderr().lock()) {
        return code;
    }
    main_in(
        args,
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

fn adapters_for(
    command: &Command,
    environment: &Environment,
) -> Result<Option<wiring::Adapters>, Failure> {
    let chosen = match command {
        Command::Init {
            store: Some(store), ..
        } => Some(environment.placed(store)?),
        _ => None,
    };
    let adapters = wiring::wire(environment.variable, chosen)?;
    Ok(match command {
        Command::Read(ReadCommand::Context {
            hook: Some(word), ..
        }) => adapters.map(|adapters| {
            let given = |key: &str| hook::text_under(&(environment.hook_input)()?, key);
            adapters.run_by_hook(word, &given)
        }),
        _ => adapters,
    })
}

fn unbound(command: &Command, json: bool) -> Result<Output, Failure> {
    let not_set_up = Failure::not_set_up();
    match command {
        // A session opens with `context` unasked, on a host set up or not.
        Command::Read(ReadCommand::Context { .. }) => {
            Ok(Output::note(not_set_up.to_string(), json))
        }
        _ => Err(not_set_up),
    }
}

// The store of an `init` given no argument is known only once its question is answered.
fn asked(json: bool, environment: &Environment, err: &mut dyn Write) -> Result<Output, Failure> {
    let answers = environment.terminal.ok_or_else(prompt::no_terminal)?;
    if let Some(bound) = wiring::wire(environment.variable, None)? {
        setup::refuse_bound(&bound.deps())?;
    }
    let mut answers = answers.borrow_mut();
    let mut terminal = prompt::Terminal::new(&mut *answers, err);
    let (store, adapters) = prompt::store(&mut terminal, environment, |store| {
        wiring::wire(environment.variable, Some(store.to_path_buf()))?
            .ok_or_else(Failure::not_set_up)
    })?;
    prompt::init(&adapters.deps(), environment, json, &store, &mut terminal)
}

// A log that stalls must not hold back the run's output.
fn ran(
    command: &Command,
    json: bool,
    environment: &Environment,
    words: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if let Command::Init { name: None, .. } = command {
        let answer = asked(json, environment, err);
        return finished(answer, out, err);
    }
    let adapters = match adapters_for(command, environment) {
        Ok(Some(adapters)) => adapters,
        Ok(None) => return finished(unbound(command, json), out, err),
        Err(failure) => return finished(Err(failure), out, err),
    };
    let deps = adapters.deps();
    let answer = run::run(&deps, environment, json, command);
    let outcome = log::outcome(&answer);
    let code = finished(answer, out, err);
    log::ran(words, environment, &deps, outcome);
    code
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
            &format_args!("{address}: the store is served without a login to {reach}"),
        );
    }
    writeln!(out, "http://{address}")?;
    out.flush()
}

fn wired(environment: &Environment) -> Result<wiring::Adapters, Failure> {
    wiring::wire(environment.variable, None)?.ok_or_else(Failure::not_set_up)
}

// The server ends only with the process.
fn listening(
    bind: &SocketAddr,
    hosts: &[String],
    environment: &Environment,
    words: &[OsString],
) -> Result<web::Server, Failure> {
    let adapters = wired(environment)?;
    let server = web::Server::bind(&bind.to_string(), hosts);
    let outcome = match &server {
        Ok(_) => Outcome::default(),
        Err(failure) => log::failed(failure),
    };
    log::ran(words, environment, &adapters.deps(), outcome);
    server
}

fn serve(
    bind: &SocketAddr,
    hosts: &[String],
    environment: &Environment,
    words: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let server = match listening(bind, hosts, environment, words) {
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
    // A directory store answers header reads from one reading of its tree.
    let reading = wiring::Reading::default();
    let settled = || {
        let adapters = wired(environment)?.continuing(&reading);
        adapters.settle()?;
        Ok(adapters)
    };
    let ended = server.run(None, &|method, target| match settled() {
        Ok(adapters) => web::respond(&adapters.deps().read(), method, target),
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
    let words: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let cli = match Cli::parsed(&words) {
        Ok(cli) => cli,
        Err(error) => {
            // clap sends help and the version to stdout with exit 0.
            if error.use_stderr() {
                let _ = write!(err, "{}", error.render());
                log::refused(&words, environment, &error);
            } else {
                let _ = write!(out, "{}", error.render());
            }
            return error.exit_code();
        }
    };
    let command = match &cli.command {
        Invoked::Run(command) => command,
        Invoked::Serve { bind, hosts } => {
            return serve(bind, hosts, environment, &words, out, err);
        }
        Invoked::Agents { action } => {
            return ended(agents::run(*action, environment, cli.json), out, err);
        }
        Invoked::Refresh => {
            return ended(refresh::run(environment, cli.json), out, err);
        }
        Invoked::Completions { shell } => {
            return finished(complete::registered(*shell, environment), out, err);
        }
        Invoked::Upgrade { check } => {
            return ended(upgrade::run(*check, environment, cli.json), out, err);
        }
    };
    ran(command, cli.json, environment, &words, out, err)
}

fn ended(answer: Result<Output, Stopped>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match answer {
        Ok(output) => finished(Ok(output), out, err),
        Err(Stopped { done, failure }) => {
            if let Some(done) = done {
                finished(Ok(done), out, err);
            }
            finished(Err(failure), out, err)
        }
    }
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
    use crate::app::machines;
    use crate::domain::testing::{MemoryBinary, MemoryReleases, version};

    const UNPUBLISHED: fn() -> Box<dyn Releases> = || Box::new(MemoryReleases::default());

    pub(super) struct Ran {
        pub(super) out: String,
        pub(super) err: String,
        pub(super) code: i32,
    }

    const IN_MEMORY: fn(&Path) -> Box<dyn Binary> = |_| Box::new(MemoryBinary::default());

    pub(super) fn environment(variable: &dyn Fn(&str) -> Option<OsString>) -> Environment<'_> {
        Environment {
            cwd: None,
            binary: None,
            variable,
            version: release::current(),
            releases: &UNPUBLISHED,
            binary_at: &IN_MEMORY,
            terminal: None,
            hook_input: &|| None,
        }
    }

    pub(super) struct Process<'a> {
        pub(super) next: Option<PathBuf>,
        pub(super) home: Option<PathBuf>,
        pub(super) invoked: OsString,
        pub(super) binary: Option<PathBuf>,
        pub(super) path: Option<OsString>,
        pub(super) cwd: Option<PathBuf>,
        pub(super) version: Built,
        pub(super) releases: &'a dyn Fn() -> Box<dyn Releases>,
        pub(super) binary_at: &'a dyn Fn(&Path) -> Box<dyn Binary>,
        pub(super) typed: Option<String>,
    }

    impl<'a> Process<'a> {
        pub(super) fn nowhere() -> Process<'a> {
            Process {
                next: None,
                home: None,
                invoked: OsString::from(NAME),
                binary: None,
                path: None,
                cwd: None,
                version: release::current(),
                releases: &UNPUBLISHED,
                binary_at: &IN_MEMORY,
                typed: None,
            }
        }

        pub(super) fn under(root: &Path) -> Process<'a> {
            Process {
                next: Some(root.join("next")),
                home: Some(root.join("home")),
                binary: Some(root.join(NAME)),
                cwd: Some(root.join("home")),
                ..Process::nowhere()
            }
        }

        pub(super) fn variable(&self, name: &str) -> Option<OsString> {
            let set = |dir: &Option<PathBuf>| dir.clone().map(PathBuf::into_os_string);
            match name {
                "WORKLOG_HOME" => set(&self.next),
                "HOME" => set(&self.home),
                "PATH" => self.path.clone(),
                _ => None,
            }
        }

        pub(super) fn run_to(
            &self,
            line: &[&str],
            out: &mut dyn Write,
            err: &mut dyn Write,
        ) -> i32 {
            let typed = self
                .typed
                .as_deref()
                .map(|typed| RefCell::new(typed.as_bytes()));
            let answers = typed.as_ref().map(|typed| typed as &RefCell<dyn BufRead>);
            let environment = Environment {
                cwd: self.cwd.clone(),
                binary: self.binary.clone(),
                variable: &|name| self.variable(name),
                version: self.version,
                releases: self.releases,
                binary_at: self.binary_at,
                terminal: answers,
                hook_input: &|| None,
            };
            let mut args = vec![self.invoked.clone()];
            args.extend(line.iter().map(OsString::from));
            main_in(args, &environment, out, err)
        }

        pub(super) fn run(&self, line: &[&str]) -> Ran {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = self.run_to(line, &mut out, &mut err);
            Ran {
                out: String::from_utf8(out).unwrap(),
                err: String::from_utf8(err).unwrap(),
                code,
            }
        }
    }

    fn process_in<'a>(root: &Path, cwd: Option<&Path>) -> Process<'a> {
        Process {
            cwd: cwd.map(Path::to_path_buf),
            ..Process::under(root)
        }
    }

    fn run_to(
        root: &Path,
        cwd: Option<&Path>,
        line: &[&str],
        out: &mut dyn Write,
    ) -> (String, i32) {
        let mut err = Vec::new();
        let code = process_in(root, cwd).run_to(line, out, &mut err);
        (String::from_utf8(err).unwrap(), code)
    }

    fn run_in(root: &Path, cwd: Option<&Path>, line: &[&str]) -> Ran {
        process_in(root, cwd).run(line)
    }

    fn run(root: &Path, line: &[&str]) -> Ran {
        Process::under(root).run(line)
    }

    fn init(root: &Path) -> Ran {
        let line = ["init", "desk", "--store", "kept", "--summary", "A machine"];
        run(root, &line)
    }

    struct Found {
        _kept: tempfile::TempDir,
        root: PathBuf,
    }

    impl Found {
        fn new() -> Found {
            let kept = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(kept.path()).unwrap();
            for dir in ["bin", "bench"] {
                std::fs::create_dir_all(root.join(dir)).unwrap();
                std::fs::write(root.join(dir).join(NAME), "").unwrap();
            }
            std::fs::create_dir(root.join("home")).unwrap();
            Found { _kept: kept, root }
        }

        fn how<'a>(&self, dir: &str, path: &[&str], cwd: Option<&Path>) -> Process<'a> {
            let binary = self.root.join(dir).join(NAME);
            let path = path.iter().map(|dir| self.root.join(dir));
            Process {
                invoked: binary.clone().into_os_string(),
                binary: Some(binary),
                path: Some(std::env::join_paths(path).unwrap()),
                ..process_in(&self.root, cwd)
            }
        }

        fn run_as(&self, dir: &str, path: &[&str], cwd: Option<&Path>, line: &[&str]) -> Ran {
            self.how(dir, path, cwd).run(line)
        }

        fn run_by(&self, invoked: &str, path: &[&str], line: &[&str]) -> Ran {
            let home = self.root.join("home");
            let mut how = self.how("bin", path, Some(&home));
            how.invoked = OsString::from(invoked);
            how.run(line)
        }

        fn run(&self, line: &[&str]) -> Ran {
            let home = self.root.join("home");
            self.run_as("bin", &["bin"], Some(&home), line)
        }

        fn logged(&self) -> Vec<serde_json::Value> {
            let dir = self.root.join("home/kept/usage");
            let Ok(files) = std::fs::read_dir(&dir) else {
                return Vec::new();
            };
            let mut files: Vec<PathBuf> = files.map(|file| file.unwrap().path()).collect();
            files.sort();
            let mut logged = Vec::new();
            for file in &files {
                let text = std::fs::read_to_string(file).unwrap();
                assert!(text.ends_with('\n'), "{text}");
                let lines = text.lines();
                logged.extend(
                    lines.map(|line| serde_json::from_str(line).expect("a line that reads")),
                );
            }
            logged
        }

        fn commands(&self) -> Vec<String> {
            let logged = self.logged();
            let command = |line: &serde_json::Value| line["command"].as_str().unwrap().to_owned();
            logged.iter().map(command).collect()
        }
    }

    #[test]
    fn the_name_looked_for_on_the_path_is_the_one_the_binary_was_run_by() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let links = found.root.join("links");
        std::fs::create_dir(&links).unwrap();
        std::os::unix::fs::symlink(found.root.join("bin").join(NAME), links.join("lamp")).unwrap();
        let relative = format!("bin/{NAME}");
        let dotted = format!("./{NAME}");
        let elsewhere = found.root.join("bench").join(NAME);

        for (invoked, path, logged) in [
            (NAME, &["bin"][..], true),
            (relative.as_str(), &["bin"], true),
            (dotted.as_str(), &["bin"], true),
            ("lamp", &["links"], true),
            ("lamp", &["bin"], false),
            (NAME, &["links"], false),
            (NAME, &["bench", "bin"], false),
            (elsewhere.to_str().unwrap(), &["bench"], false),
        ] {
            let before = found.commands().len();
            let ran = found.run_by(invoked, path, &["topics"]);
            assert_eq!((ran.err.as_str(), ran.code), ("", 0), "{invoked}");
            let added = found.commands().len() - before;
            assert_eq!(added, usize::from(logged), "{invoked} with {path:?}");
        }
    }

    #[test]
    fn a_bare_run_and_a_command_lacking_its_subcommand_are_logged_as_refusals() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        for line in [&[][..], &["new"]] {
            let ran = found.run(line);
            assert_eq!((ran.out.as_str(), ran.code), ("", 2), "{line:?}");
        }
        let logged = found.logged();
        assert_eq!(found.commands(), ["", "new"]);
        for line in &logged {
            assert_eq!(line["exit"], 2);
            assert!(
                line["refusal"]
                    .as_str()
                    .is_some_and(|said| !said.is_empty())
            );
        }
    }

    #[test]
    #[cfg_attr(target_os = "macos", ignore = "APFS refuses a non-UTF-8 file name")]
    fn a_working_directory_that_is_not_utf_8_is_logged_with_what_of_it_reads() {
        use std::os::unix::ffi::OsStringExt as _;
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let named = OsString::from_vec(vec![b'l', 0xff]);
        let cwd = found.root.join("home").join(named);
        std::fs::create_dir(&cwd).unwrap();
        let ran = found.run_as("bin", &["bin"], Some(&cwd), &["topics"]);
        assert_eq!((ran.err.as_str(), ran.code), ("", 0));
        let logged = found.logged();
        assert_eq!(found.commands(), ["topics"]);
        assert_eq!(logged[0]["directory"], "~/l\u{fffd}");
    }

    struct Watching {
        log: PathBuf,
        logged_at_first_write: Option<bool>,
        text: Vec<u8>,
    }

    impl Write for Watching {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.logged_at_first_write.get_or_insert(self.log.exists());
            self.text.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_line_the_parser_refuses_is_said_before_it_is_logged() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let home = found.root.join("home");
        let mut err = Watching {
            log: home.join("kept/usage"),
            logged_at_first_write: None,
            text: Vec::new(),
        };
        let how = found.how("bin", &["bin"], Some(&home));
        let line = ["facts", "--no-such-flag"];
        let code = how.run_to(&line, &mut Vec::new(), &mut err);
        assert_eq!(code, 2);
        assert!(err.text.starts_with(b"error: "));
        assert_eq!(err.logged_at_first_write, Some(false));
        assert_eq!(found.commands(), ["facts"]);
    }

    #[test]
    fn a_run_says_what_it_has_to_say_before_it_is_logged() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let home = found.root.join("home");
        let log = home.join("kept/usage");
        let watching = || Watching {
            log: log.clone(),
            logged_at_first_write: None,
            text: Vec::new(),
        };
        let how = found.how("bin", &["bin"], Some(&home));

        let mut out = watching();
        let code = how.run_to(&["topics"], &mut out, &mut Vec::new());
        assert_eq!(code, 0);
        assert!(out.text.ends_with(b"  desk  s\n"));
        assert_eq!(out.logged_at_first_write, Some(false));
        assert_eq!(found.commands(), ["topics"]);

        std::fs::remove_dir_all(&log).unwrap();
        let mut err = watching();
        let line = ["show", "lantern/relay"];
        let code = how.run_to(&line, &mut Vec::new(), &mut err);
        assert_eq!(code, 1);
        assert!(err.text.starts_with(NAME.as_bytes()));
        assert_eq!(err.logged_at_first_write, Some(false));
        assert_eq!(found.commands(), ["show"]);
    }

    #[test]
    fn a_run_is_logged_under_the_version_its_process_was_started_as() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let home = found.root.join("home");
        let ahead = Process {
            version: version("0.4.0").into(),
            ..found.how("bin", &["bin"], Some(&home))
        };
        assert_eq!(ahead.run(&["topics"]).code, 0);
        assert_eq!(found.logged()[0]["version"], "0.4.0");
    }

    struct Removing {
        store: PathBuf,
        text: Vec<u8>,
    }

    impl Write for Removing {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.store.exists() {
                std::fs::remove_dir_all(&self.store)?;
            }
            self.text.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_host_whose_store_is_gone_is_told_so_and_no_store_is_made_to_log_it() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let home = found.root.join("home");
        let store = home.join("kept");
        let how = found.how("bin", &["bin"], Some(&home));
        let mut out = Removing {
            store: store.clone(),
            text: Vec::new(),
        };
        let code = how.run_to(&["topics"], &mut out, &mut Vec::new());
        assert_eq!(code, 0);
        assert!(out.text.ends_with(b"  desk  s\n"));
        assert!(!store.exists());

        let gone = found.run(&["topics"]);
        let said = format!(
            "{NAME}: {}: this host's store is not a directory\n",
            store.display()
        );
        assert_eq!((gone.out.as_str(), gone.err, gone.code), ("", said, 1));
        assert_eq!(found.run(&["facts", "--no-such-flag"]).code, 2);
        assert!(!store.exists());
    }

    #[test]
    fn a_run_with_no_working_directory_logs_nothing() {
        let found = Found::new();
        found.run(&["init", "desk", "--store", "kept", "--summary", "s"]);
        let nowhere = found.run_as("bin", &["bin"], None, &["topics"]);
        assert_eq!((nowhere.err.as_str(), nowhere.code), ("", 0));
        found.run_as("bin", &["bin"], None, &["facts", "--no-such-flag"]);
        assert_eq!(found.commands(), [""; 0]);
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
        let reached = "whatever reaches this address";
        let everywhere = "every network this host is on";
        for (address, line, reach) in [
            ("192.168.7.20:4000", "http://192.168.7.20:4000\n", reached),
            ("10.0.0.5:8080", "http://10.0.0.5:8080\n", reached),
            ("[fe80::1]:8080", "http://[fe80::1]:8080\n", reached),
            ("0.0.0.0:8080", "http://0.0.0.0:8080\n", everywhere),
            ("[::]:4000", "http://[::]:4000\n", everywhere),
        ] {
            let note =
                format!("{NAME}: {address}: the store is served without a login to {reach}\n");
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
                ..environment(&|_| None)
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
            format!("{NAME}: this host is not set up; run `{NAME} init`\n")
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

    fn typing<'a>(root: &Path, answers: &str) -> Process<'a> {
        Process {
            typed: Some(answers.to_owned()),
            ..Process::under(root)
        }
    }

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        (kept, root)
    }

    #[test]
    fn init_alone_with_no_terminal_is_a_usage_error_that_asks_nothing() {
        let (_kept, root) = scratch();
        let ran = run(&root, &["init"]);
        assert_eq!((ran.out.as_str(), ran.code), ("", 2));
        assert_eq!(
            ran.err,
            format!(
                "{NAME}: `init` alone asks its questions on a terminal. A host is set up by its \
                 user, who gives the directory of the store; without a terminal that is \
                 `{NAME} init <NAME> --store <DIR>`\n"
            )
        );
        assert!(!root.join("next").exists());
        assert!(!root.join("home").exists());
    }

    #[test]
    fn init_alone_binds_the_host_over_the_store_it_is_told_of() {
        let (_kept, root) = scratch();
        let store = root.join("home/kept");
        let made = typing(&root, "kept\ny\ndesk\n\n").run(&["init"]);
        assert_eq!((made.out.as_str(), made.code), ("desk created\n", 0));
        let default = root.join("home/worklog");
        let first = format!("Directory of the store [{}]: ", default.display());
        assert!(made.err.starts_with(&first), "{}", made.err);
        let last = format!(
            "The store: {}\nThe machine: desk, a new topic\nSet this host up? [Y/n]: ",
            store.display()
        );
        assert!(made.err.ends_with(&last), "{}", made.err);
        let listed = run(&root, &["topics"]);
        let row = format!("  desk  {}\n", machines::PLACEHOLDER);
        assert!(listed.out.ends_with(&row), "{}", listed.out);

        let (_other, phone) = scratch();
        let answers = format!("{}\ndesk\n\n", store.display());
        let bound = typing(&phone, &answers).run(&["init", "--json"]);
        assert_eq!(bound.code, 0, "{}", bound.err);
        let value: serde_json::Value = serde_json::from_str(&bound.out).expect("one value");
        assert_eq!(
            (&value["label"], &value["created"]),
            (&"desk".into(), &false.into())
        );
        assert_eq!(run(&phone, &["topics"]).out, listed.out);
    }

    #[test]
    fn init_alone_that_is_declined_leaves_no_store_and_no_config() {
        for (answers, why) in [
            ("kept\n\n", "no store was started"),
            ("kept\nn\n", "no store was started"),
            ("kept\ny\ndesk\nn\n", "nothing was written"),
            ("kept\ny\n", "nothing was written"),
            ("", "nothing was written"),
        ] {
            let (_kept, root) = scratch();
            let ran = typing(&root, answers).run(&["init"]);
            assert_eq!((ran.out.as_str(), ran.code), ("", 1), "{answers:?}");
            assert!(ran.err.contains(&format!("{NAME}: ")), "{}", ran.err);
            assert!(ran.err.ends_with(&format!("{why}\n")), "{}", ran.err);
            assert!(!root.join("next").exists(), "{answers:?}");
            assert!(!root.join("home").exists(), "{answers:?}");
        }
    }

    #[test]
    fn init_alone_asks_before_it_starts_a_store_in_a_directory_holding_no_document() {
        for held in [&[][..], &[".stfolder"], &[".stfolder", "usage"]] {
            let (_kept, root) = scratch();
            let store = root.join("home/kept");
            std::fs::create_dir_all(&store).unwrap();
            for dir in held {
                std::fs::create_dir(store.join(dir)).unwrap();
            }
            let asking = format!(
                "{} holds no documents. A store shared between machines may not have synced \
                 to this one yet.\nStart a new store there? [y/N]: ",
                store.display()
            );
            let declined = typing(&root, "kept\n\n").run(&["init"]);
            assert_eq!((declined.out.as_str(), declined.code), ("", 1));
            assert!(declined.err.contains(&asking), "{}", declined.err);
            assert!(!root.join("next").exists(), "{held:?}");
            assert_eq!(std::fs::read_dir(&store).unwrap().count(), held.len());

            let made = typing(&root, "kept\ny\ndesk\n\n").run(&["init"]);
            assert_eq!((made.out.as_str(), made.code), ("desk created\n", 0));
        }
    }

    #[test]
    fn init_alone_reads_a_tilde_as_the_home_directory_and_refuses_what_an_argument_is_refused() {
        let (_kept, root) = scratch();
        let made = typing(&root, "~/sync/kept\ny\ndesk\n\n").run(&["init"]);
        assert_eq!((made.out.as_str(), made.code), ("desk created\n", 0));
        assert!(root.join("home/sync/kept").is_dir());

        let (_other, root) = scratch();
        let project = root.join("home/projects");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("notes.md"), "a person's own").unwrap();
        std::fs::write(root.join("home/file"), "").unwrap();
        let asked_again = typing(&root, "projects\nfile\nkept\ny\ndesk\n\n").run(&["init"]);
        assert_eq!(
            (asked_again.out.as_str(), asked_again.code),
            ("desk created\n", 0)
        );
        let said = format!(
            "{}: is not empty and holds no store\nDirectory of the store [",
            project.display()
        );
        assert!(asked_again.err.contains(&said), "{}", asked_again.err);
        let file = format!("]: {}: ", root.join("home/file").display());
        assert!(asked_again.err.contains(&file), "{}", asked_again.err);
        assert!(root.join("home/kept").is_dir());
        assert_eq!(std::fs::read_dir(&project).unwrap().count(), 1);

        let (_third, root) = scratch();
        let project = root.join("home/projects");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("notes.md"), "a person's own").unwrap();
        let ended = typing(&root, "projects\n").run(&["init"]);
        assert_eq!((ended.out.as_str(), ended.code), ("", 1));
        assert!(!root.join("next").exists());
    }

    #[test]
    fn init_alone_on_a_host_that_is_set_up_is_refused_before_it_asks() {
        let (_kept, root) = scratch();
        init(&root);
        let again = typing(&root, "elsewhere\ny\natlas\n\n").run(&["init"]);
        assert_eq!((again.out.as_str(), again.code), ("", 1));
        assert_eq!(
            again.err,
            format!("{NAME}: this host is already set up as desk\n")
        );
        assert!(!root.join("home/elsewhere").exists());
    }

    #[test]
    fn init_alone_installs_for_the_agents_of_the_host_when_told_to() {
        let (_kept, root) = scratch();
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let made = typing(&root, "kept\ny\ndesk\ny\n\n").run(&["init"]);
        let skill = home.join(".claude/skills").join(NAME).join("SKILL.md");
        let hooks = home.join(".claude/settings.json");
        assert_eq!(
            (made.out, made.code),
            (
                format!("desk created\n{}\n{}\n", skill.display(), hooks.display()),
                0
            )
        );
        assert!(skill.is_file() && hooks.is_file());
    }

    #[test]
    fn a_process_with_no_working_directory_is_refused_only_what_needs_one() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let nowhere = run_in(root, None, &["init", "desk", "--store", "kept"]);
        assert_eq!(nowhere.err, format!("{NAME}: no working directory\n"));
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
        assert_eq!(context.err, nowhere.err);
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
            let named = format!("{NAME}: {}", damaged.display());
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
        assert!(err.starts_with(&format!("{NAME}: stdout: ")), "{err}");
        assert_eq!(code, 1);

        let mut closed = Failing(ErrorKind::BrokenPipe);
        assert_eq!(
            run_to(root, Some(&home), &["topics"], &mut closed),
            (String::new(), 0)
        );
        let mut closed = Failing(ErrorKind::BrokenPipe);
        let (err, code) = run_to(root, Some(&home), &["save", "lantern"], &mut closed);
        assert_eq!((err, code), (format!("{NAME}: lantern: no draft\n"), 1));
    }

    fn run_with(home: Option<&Path>, binary: Option<PathBuf>, line: &[&str]) -> Ran {
        let process = Process {
            home: home.map(Path::to_path_buf),
            binary,
            ..Process::nowhere()
        };
        process.run(line)
    }

    fn printed(ran: &Ran, text: &str) {
        assert_eq!(
            (ran.out.as_str(), ran.err.as_str(), ran.code),
            (text, "", 0)
        );
    }

    fn refused(ran: &Ran, why: &str) {
        let said = format!("{NAME}: {why}\n");
        assert_eq!(
            (ran.out.as_str(), ran.err.as_str(), ran.code),
            ("", said.as_str(), 1)
        );
    }

    #[test]
    fn completions_prints_a_shell_s_line_wherever_the_binary_is_known_by_its_own_name() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        let home = root.join("home");
        let fish = ["completions", "fish"];
        printed(
            &run_with(Some(&home), Some(home.join("bin").join(NAME)), &fish),
            &format!("COMPLETE=fish \"$HOME/bin/{NAME}\" | source\n"),
        );
        let elsewhere = root.join("bench").join(NAME);
        let whole = format!("source <(COMPLETE=zsh \"{}\")\n", elsewhere.display());
        let zsh = ["completions", "zsh"];
        printed(
            &run_with(Some(&home), Some(elsewhere.clone()), &zsh),
            &whole,
        );
        printed(&run_with(None, Some(elsewhere), &zsh), &whole);

        refused(
            &run_with(Some(&home), None, &fish),
            "the path of this binary is unknown",
        );
        assert!(!home.exists());
    }

    #[test]
    fn a_refresh_whose_binary_makes_no_line_for_fish_writes_nothing_for_an_agent_either() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        let home = root.join("home");
        let skill = home.join(".claude/skills").join(NAME).join("SKILL.md");
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, "old").unwrap();
        let fish = home.join(".config/fish/completions");
        std::fs::create_dir_all(&fish).unwrap();
        let refresh = |binary: Option<PathBuf>| run_with(Some(&home), binary, &["refresh"]);

        refused(&refresh(None), "the path of this binary is unknown");
        let quoted = refresh(Some(root.join("a\"b").join(NAME)));
        assert_eq!((quoted.out.as_str(), quoted.code), ("", 1));
        assert!(quoted.err.contains("holds '\"'"), "{}", quoted.err);
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), "old");
        assert_eq!(std::fs::read_dir(&fish).unwrap().count(), 0);

        let named = root.join("bench").join(NAME);
        let file = fish.join(format!("{NAME}.fish"));
        let written = format!("{}\n{}\n", skill.display(), file.display());
        printed(&refresh(Some(named.clone())), &written);
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            format!("COMPLETE=fish \"{}\" | source\n", named.display())
        );
    }

    #[test]
    fn an_install_and_an_uninstall_that_fail_say_what_they_wrote_before_the_failure() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        let home = root.join("home");
        for agent in [".claude", ".codex"] {
            std::fs::create_dir_all(home.join(agent)).unwrap();
        }
        let ran = |action: &str| {
            let binary = Some(root.join("bin").join(NAME));
            run_with(Some(&home), binary, &["agents", action])
        };
        let stopped = |ran: &Ran, written: &[PathBuf], at: &Path| {
            let written: Vec<String> = (written.iter())
                .map(|path| path.display().to_string())
                .collect();
            let lines = agents::lines(&written);
            assert_eq!((ran.out.as_str(), ran.code), (lines.as_str(), 1));
            let named = format!("{NAME}: {}", at.display());
            assert!(ran.err.starts_with(&named), "{}", ran.err);
            assert_eq!(ran.err.lines().count(), 1, "{}", ran.err);
        };
        let skill = home.join(".claude/skills").join(NAME);
        let hooks = home.join(".claude/settings.json");

        let blocked = home.join(".codex/skills");
        std::fs::write(&blocked, "").unwrap();
        stopped(
            &ran("install"),
            &[skill.join("SKILL.md"), hooks.clone()],
            &blocked,
        );

        std::fs::remove_file(&blocked).unwrap();
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join(NAME), "").unwrap();
        stopped(&ran("uninstall"), &[skill, hooks], &blocked.join(NAME));
    }

    #[test]
    fn a_command_that_names_the_binary_in_a_file_is_refused_when_its_path_is_unknown() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        std::fs::create_dir_all(root.join("home/.claude")).unwrap();
        let home = root.join("home");
        let ran = |action: &str| {
            let ran = run_with(Some(&home), None, &["agents", action]);
            assert!(ran.out.is_empty());
            (ran.err, ran.code)
        };
        let unknown = (format!("{NAME}: the path of this binary is unknown\n"), 1);
        assert_eq!(ran("install"), unknown);
        assert_eq!(ran("refresh"), (String::new(), 0));
        assert!(!root.join("home/.claude/settings.json").exists());

        let hooks = serde_json::json!({
            "hooks": {"SessionStart": [{"hooks": [{"command": format!("{NAME} context")}]}]}
        });
        std::fs::write(root.join("home/.claude/settings.json"), hooks.to_string()).unwrap();
        assert_eq!(ran("refresh"), unknown);
        std::fs::remove_file(root.join("home/.claude/settings.json")).unwrap();
        assert_eq!(ran("uninstall"), (String::new(), 0));
        assert!(!root.join("home/.claude/settings.json").exists());
    }
}
