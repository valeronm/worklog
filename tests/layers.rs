//! The domain touches nothing outside memory, and a convention nothing
//! checks is one a routine edit falsifies.

use std::fs;
use std::path::{Path, PathBuf};

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

// A chain rustfmt breaks across lines matches only with all whitespace removed.
fn squeezed(text: &str) -> String {
    text.split_whitespace().collect()
}

fn files(path: &Path, extension: &str, found: &mut Vec<(String, String)>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("readable source directory") {
            files(&entry.expect("readable entry").path(), extension, found);
        }
    } else if path.extension().is_some_and(|e| e == extension) {
        found.push((
            path.display().to_string(),
            fs::read_to_string(path).expect("readable source"),
        ));
    }
}

fn sources(path: &Path, found: &mut Vec<(String, String)>) {
    files(path, "rs", found);
}

fn reaches_nothing_in(layer: &str, forbidden: &[&str]) {
    let mut found = Vec::new();
    sources(&src().join(layer), &mut found);
    assert!(!found.is_empty());
    for (path, text) in found {
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{path} reaches outside `{layer}` through `{needle}`"
            );
        }
    }
}

#[test]
fn the_domain_does_no_io() {
    reaches_nothing_in(
        "domain",
        &[
            "std::fs",
            "std::env",
            "std::process",
            "std::io",
            "std::net",
            "std::time",
            "crate::wiring",
            "getrandom",
        ],
    );
}

#[test]
fn no_module_outside_the_schema_names_it() {
    let src = src();
    let mut found = Vec::new();
    for entry in fs::read_dir(src.join("domain")).expect("readable source directory") {
        let path = entry.expect("readable entry").path();
        if path.file_name().is_some_and(|name| name != "schema") {
            sources(&path, &mut found);
        }
    }
    let in_the_domain = found.len();
    sources(&src.join("wiring").join("fs"), &mut found);
    assert!(in_the_domain > 0 && found.len() > in_the_domain);
    for (path, text) in found {
        // Declaring the module is the one mention its parent needs.
        let text = if path.ends_with("domain/mod.rs") {
            text.replacen("pub mod schema;", "", 1)
        } else {
            text
        };
        assert!(!text.contains("schema"), "{path} names the schema");
    }
}

// A line of the usage log is a format of the store, worded by the file that writes it.
const WORDS_ITS_OWN_LINE: &str = "wiring/fs/usage.rs";

fn json_named(file: &str, text: &str) -> Vec<String> {
    let layer = file.split('/').next().unwrap_or_default();
    let barred = ["domain", "app", "wiring"].contains(&layer) && file != WORDS_ITS_OWN_LINE;
    if barred && text.contains("serde_json") {
        vec!["names `serde_json`".to_owned()]
    } else {
        Vec::new()
    }
}

#[test]
fn a_result_is_given_its_json_form_by_the_command_line_alone() {
    let mut found = Vec::new();
    sources(&src(), &mut found);
    let mut broken = Vec::new();
    let mut wording_a_line = Vec::new();
    for (path, text) in found {
        let file = Path::new(&path).strip_prefix(src()).expect("a file of it");
        let file = file.display().to_string();
        for what in json_named(&file, &text) {
            broken.push(format!("{file}: {what}"));
        }
        if file == WORDS_ITS_OWN_LINE && text.contains("serde_json") {
            wording_a_line.push(file);
        }
    }
    assert_eq!(broken, [""; 0]);
    assert_eq!(wording_a_line, [WORDS_ITS_OWN_LINE]);
}

#[test]
fn a_file_below_the_command_line_that_names_json_is_found_but_the_one_wording_a_stored_line() {
    let naming = "use serde_json::Value;
let line = serde_json::to_string(&line);";
    for clean in [
        "wiring/fs/usage.rs",
        "cli/json.rs",
        "cli/usage.rs",
        "web/mod.rs",
    ] {
        assert_eq!(json_named(clean, naming), [""; 0], "{clean}");
    }
    for broken in [
        "wiring/fs/store.rs",
        "wiring/fs/mod.rs",
        "wiring/fs/usage/line.rs",
        "wiring/usage.rs",
        "wiring/mod.rs",
        "wiring/net.rs",
        "app/usage.rs",
        "app/record.rs",
        "app/wiring/fs/usage.rs",
        "domain/usage.rs",
        "domain/testing.rs",
    ] {
        assert_eq!(
            json_named(broken, naming),
            ["names `serde_json`"],
            "{broken}"
        );
        assert_eq!(json_named(broken, "use serde::Serialize;"), [""; 0]);
    }
}

#[test]
fn a_result_s_json_carries_every_field_under_the_convention() {
    let mut found = Vec::new();
    sources(&src(), &mut found);
    assert!(!found.is_empty());
    for (path, text) in found {
        let text = squeezed(&text);
        for needle in [
            "serde(skip",
            "skip_serializing",
            "serde(flatten",
            "serde(untagged",
        ] {
            assert!(!text.contains(needle), "{path} holds `{needle}`");
        }
        for (at, _) in text.match_indices("rename_all") {
            let cased = &text[at..];
            assert!(
                cased.starts_with("rename_all=\"snake_case\""),
                "{path} renames its keys to another case"
            );
        }
    }
}

fn json_written_by_hand(text: &str) -> Vec<String> {
    let text = uncommented(text);
    let tokens = tokens(&text);
    let mut written = Vec::new();
    for at in 0..tokens.len().saturating_sub(2) {
        if tokens[at] == "Serialize" && tokens[at + 1] == "for" {
            written.push(tokens[at + 2].to_owned());
        }
    }
    written
}

#[test]
fn a_result_s_json_is_written_by_hand_for_these_alone() {
    let mut by_hand = violations(&src().join("app"), json_written_by_hand);
    by_hand.sort();
    assert_eq!(
        by_hand,
        [
            format!("{}: Made", app().join("followup.rs").display()),
            format!("{}: Compared", app().join("show.rs").display()),
        ]
    );
}

#[test]
fn a_result_that_writes_its_own_json_is_found() {
    for (text, found) in [
        ("impl Serialize for Checked {", &["Checked"][..]),
        ("impl\n    Serialize\n    for Checked {", &["Checked"]),
        ("impl serde::Serialize for Made {}", &["Made"]),
        ("#[derive(Serialize)]\npub struct Checked {", &[]),
        ("// impl Serialize for Checked {", &[]),
        ("impl<S: Serialize> Render for S {", &[]),
    ] {
        assert_eq!(json_written_by_hand(text), found, "{text}");
    }
}

#[test]
fn the_app_reaches_the_host_only_through_ports() {
    reaches_nothing_in(
        "app",
        &[
            "std::fs",
            "std::env",
            "std::process",
            "std::net",
            "std::time",
            "crate::wiring",
        ],
    );
}

fn tokens(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(first) = rest.chars().next() {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let length = if first.is_whitespace() {
            rest.find(|c: char| !c.is_whitespace())
                .unwrap_or(rest.len())
        } else if word(first) {
            rest.find(|c: char| !word(c)).unwrap_or(rest.len())
        } else if rest.starts_with("::") {
            2
        } else {
            first.len_utf8()
        };
        if !first.is_whitespace() {
            found.push(&rest[..length]);
        }
        rest = &rest[length..];
    }
    found
}

fn uncommented(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

fn store_port_named(text: &str) -> Vec<String> {
    let tokens = tokens(text);
    let mut named = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let variant = at >= 2 && tokens[at - 1] == "::" && tokens[at - 2] == "Failure";
        if *token == "MemoryStore" || ((matches!(*token, "Store" | "StoreRead")) && !variant) {
            named.push((*token).to_owned());
        }
    }
    named
}

const MAIN: &str = "fn main() { std::process::exit(worklog::cli::main()); }";

fn logic_in_main(text: &str) -> Vec<String> {
    if tokens(&uncommented(text)) == tokens(MAIN) {
        Vec::new()
    } else {
        vec![format!("is not `{MAIN}`")]
    }
}

fn violations(root: &Path, rule: fn(&str) -> Vec<String>) -> Vec<String> {
    violations_in(root, "rs", rule)
}

fn violations_in(root: &Path, extension: &str, rule: fn(&str) -> Vec<String>) -> Vec<String> {
    let mut found = Vec::new();
    files(root, extension, &mut found);
    assert!(!found.is_empty());
    let mut broken = Vec::new();
    for (path, text) in found {
        for what in rule(&text) {
            broken.push(format!("{path}: {what}"));
        }
    }
    broken
}

#[test]
fn the_binary_s_main_only_calls_the_command_line() {
    assert_eq!(violations(&src().join("main.rs"), logic_in_main), [""; 0]);
}

#[test]
fn a_main_that_decides_anything_is_found() {
    let calling = "fn main() {\n    std::process::exit(worklog::cli::main());\n}\n";
    assert_eq!(logic_in_main(calling), [""; 0]);
    let spread = "// The exit code is the command line's.\nfn main()\n{\n    std::process::exit(\n                          worklog::cli::main()\n    );\n}\n";
    assert_eq!(logic_in_main(spread), [""; 0]);

    let deciding = "fn main() {\n    let code = worklog::cli::main();\n    if code != 0 {\n        panic!();\n    }\n}\n";
    assert_eq!(logic_in_main(deciding), [format!("is not `{MAIN}`")]);
}

#[test]
fn the_command_line_names_no_store_in_its_tests_either() {
    assert_eq!(violations(&src().join("cli"), store_port_named), [""; 0]);
}

#[test]
fn a_command_line_file_that_names_the_store_is_found() {
    for clean in [
        "use crate::app::{Deps, Failure, Stored};\n",
        "Failure::Refused(_) | Failure::Store(_) => 1,",
        "Failure::Store(StoreError::io(\"desk\", \"gone\"))",
        "store: Stored::new(&FailingStore),",
    ] {
        assert_eq!(store_port_named(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        ("use crate::domain::ports::Store;", "Store"),
        ("use crate::domain::ports::{Host, Store as _};", "Store"),
        ("fn read(store: &dyn Store) {}", "Store"),
        ("use crate::domain::ports::StoreRead;", "StoreRead"),
        ("let store = MemoryStore::default();", "MemoryStore"),
    ] {
        assert_eq!(store_port_named(broken), [found], "{broken}");
    }
}

const TEST_MODULE: &str = "#[cfg(test)]\nmod tests";

const TERMINAL: [&str; 4] = ["stdin", "Stdin", "is_terminal", "IsTerminal"];

fn process_read(text: &str) -> Vec<String> {
    let text = text.split(TEST_MODULE).next().unwrap_or_default();
    let mut read = modules_named(text, "std", &["env"]);
    let code = uncommented(text);
    let mut terminal: Vec<&str> = tokens(&code)
        .into_iter()
        .filter(|token| TERMINAL.contains(token))
        .collect();
    terminal.sort_unstable();
    terminal.dedup();
    read.extend(terminal.iter().map(|name| format!("names `{name}`")));
    read
}

#[test]
fn the_command_line_reads_its_process_where_it_starts_and_where_a_shell_asks() {
    let cli = src().join("cli");
    let mut found = Vec::new();
    sources(&cli, &mut found);
    let mut readers = Vec::new();
    for (path, text) in found {
        let file = Path::new(&path).strip_prefix(&cli).expect("a file of it");
        for read in process_read(&text) {
            readers.push(format!("{}: {read}", file.display()));
        }
    }
    readers.sort();
    assert_eq!(
        readers,
        [
            "complete.rs: names `std::env` as `std::env::var_os`",
            "mod.rs: names `IsTerminal`",
            "mod.rs: names `is_terminal`",
            "mod.rs: names `std::env` as `std::env::args_os`",
            "mod.rs: names `std::env` as `std::env::current_dir`",
            "mod.rs: names `std::env` as `std::env::current_exe`",
            "mod.rs: names `std::env` as `std::env::var_os`",
            "mod.rs: names `stdin`",
        ]
    );
}

#[test]
fn a_command_line_file_that_reads_the_process_is_found() {
    for clean in [
        "let cwd = environment.cwd.as_deref();\nlet path = (environment.variable)(\"PATH\");",
        "// std::env::var_os is read where the process starts.\nlet env = 1;",
        "let name = env!(\"CARGO_PKG_NAME\");",
        "pub fn run() {}\n\n#[cfg(test)]\nmod tests {\n    fn joined() {\n        std::env::join_paths(dirs).unwrap();\n    }\n}\n",
    ] {
        assert_eq!(process_read(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "let home = std::env::var_os(\"HOME\");",
            "names `std::env` as `std::env::var_os`",
        ),
        (
            "use std::env;\nlet cwd = env::current_dir();",
            "names `std::env` as `std::env`",
        ),
        (
            "use std::{env::args_os, io};",
            "names `std::env` as `std::env::args_os`",
        ),
        (
            "let words: Vec<_> = std :: env :: args().collect();",
            "names `std::env` as `std::env::args`",
        ),
        (
            "let mut line = String::new();\nstd::io::stdin().read_line(&mut line);",
            "names `stdin`",
        ),
        ("use std::io::IsTerminal as _;", "names `IsTerminal`"),
        (
            "let asked = std::io::stderr().is_terminal();",
            "names `is_terminal`",
        ),
    ] {
        let named = process_read(broken);
        assert_eq!(named.first().map(String::as_str), Some(found), "{broken}");
    }
}

const ADAPTERS: [&str; 2] = ["fs", "net"];

fn wiring_opened(text: &str) -> Vec<String> {
    let text = outside_tests(text);
    let mut found = Vec::new();
    for module in ADAPTERS {
        if !text.contains(&format!("mod{module};")) {
            found.push(format!("does not declare `mod {module};`"));
        }
        if text.contains(&format!("pubmod{module}")) {
            found.push(format!("declares `pub mod {module}`"));
        }
        if text.contains(&format!(")mod{module}")) {
            found.push(format!("declares `mod {module}` to others"));
        }
    }
    for (needle, what) in [
        ("pubuse", "has a `pub use`"),
        (")use", "has a `pub use` to others"),
    ] {
        if text.contains(needle) {
            found.push(what.to_owned());
        }
    }
    let fields = text
        .split_once("structAdapters{")
        .and_then(|(_, rest)| rest.split_once('}'));
    match fields {
        Some((fields, _)) if fields.split(',').any(|field| field.starts_with("pub")) => {
            found.push("`Adapters` has a `pub` field".to_owned());
        }
        Some(_) => {}
        None => found.push("has no `Adapters` with fields".to_owned()),
    }
    found
}

#[test]
fn the_wiring_keeps_its_adapters_to_itself() {
    let wiring = src().join("wiring").join("mod.rs");
    assert_eq!(violations(&wiring, wiring_opened), [""; 0]);
}

#[test]
fn a_wiring_that_lets_an_adapter_out_is_found() {
    let kept = "mod fs;\nmod net;\n\nuse fs::{FsHost, FsStore};\nuse net::GitHubReleases;\n\npub struct Adapters {\n    store: FsStore,\n    host: FsHost,\n}\n\n#[cfg(test)]\nmod tests {\n    pub use super::*;\n}\n";
    assert_eq!(wiring_opened(kept), [""; 0]);
    for (from, to, found) in [
        ("mod fs;", "pub mod fs;", "declares `pub mod fs`"),
        ("mod fs;", "pub  mod\nfs;", "declares `pub mod fs`"),
        (
            "mod fs;",
            "pub(crate) mod fs;",
            "declares `mod fs` to others",
        ),
        ("mod fs;", "", "does not declare `mod fs;`"),
        ("mod net;", "pub mod net;", "declares `pub mod net`"),
        (
            "mod net;",
            "pub(super) mod\n    net;",
            "declares `mod net` to others",
        ),
        ("mod net;", "", "does not declare `mod net;`"),
        (
            "use net::GitHubReleases;",
            "pub use net::GitHubReleases;",
            "has a `pub use`",
        ),
        ("use fs::{", "pub use fs::{", "has a `pub use`"),
        (
            "use fs::{",
            "pub(crate) use fs::{",
            "has a `pub use` to others",
        ),
        (
            "    host: FsHost,",
            "    pub host: FsHost,",
            "`Adapters` has a `pub` field",
        ),
        (
            "    store: FsStore,",
            "    pub(crate) store: FsStore,",
            "`Adapters` has a `pub` field",
        ),
        (
            "pub struct Adapters {",
            "pub struct Ports {",
            "has no `Adapters` with fields",
        ),
    ] {
        assert!(kept.contains(from), "{from}");
        let broken = kept.replacen(from, to, 1);
        assert_eq!(wiring_opened(&broken), [found], "{broken}");
    }
}

fn network_reached(text: &str) -> Vec<String> {
    let text = uncommented(text);
    let mut found = Vec::new();
    for path in paths(&text) {
        let named = path.join("::");
        // From a file under `wiring`, `super` reaches its modules as `wiring` does.
        if (path.windows(2)).any(|pair| pair == ["wiring", "net"] || pair == ["super", "net"]) {
            found.push(format!("names `wiring::net` as `{named}`"));
        }
        if path.contains(&"ureq") {
            found.push(format!("names `ureq` as `{named}`"));
        }
    }
    found
}

#[test]
fn the_network_is_reached_by_one_module_the_wiring_alone_names() {
    let reader = src().join("wiring").join("net.rs");
    let mut found = Vec::new();
    sources(&src(), &mut found);
    let mut broken = Vec::new();
    let mut readers = 0;
    for (path, text) in found {
        if Path::new(&path) == reader {
            readers += usize::from(tokens(&uncommented(&text)).contains(&"ureq"));
            continue;
        }
        for what in network_reached(&text) {
            broken.push(format!("{path}: {what}"));
        }
    }
    assert_eq!(readers, 1);
    assert_eq!(broken, [""; 0]);
}

#[test]
fn a_file_that_reaches_the_network_past_the_wiring_is_found() {
    for clean in [
        "mod fs;\nmod net;\n\nuse fs::{DirReleases, FsBinary};\nuse net::GitHubReleases;",
        "None => Box::new(GitHubReleases::new()),",
        "use std::net::{SocketAddr, TcpListener};\nlet bound = std::net::TcpStream::connect(to);",
        "// ureq reads them, in crate::wiring::net.\nlet request = 1; let net = wiring.net;",
        "let releases = wiring::releases(environment.variable);",
    ] {
        assert_eq!(network_reached(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        ("use ureq::Agent;", "names `ureq` as `ureq::Agent`"),
        (
            "let agent = ureq::Agent::new_with_defaults();",
            "names `ureq` as `ureq::Agent::new_with_defaults`",
        ),
        ("use {serde::Serialize, ureq};", "names `ureq` as `ureq`"),
        ("extern crate ureq;", "names `ureq` as `ureq`"),
        (
            "let body = ::ureq::get(url).call()?;",
            "names `ureq` as `ureq::get`",
        ),
        (
            "use crate::wiring::net::GitHubReleases;",
            "names `wiring::net` as `crate::wiring::net::GitHubReleases`",
        ),
        (
            "use crate::wiring::{self, net};",
            "names `wiring::net` as `crate::wiring::net`",
        ),
        (
            "let releases = super::net::GitHubReleases::new();",
            "names `wiring::net` as `super::net::GitHubReleases::new`",
        ),
        (
            "use super :: super :: net :: GitHubReleases;",
            "names `wiring::net` as `super::super::net::GitHubReleases`",
        ),
        (
            "let releases = wiring::net::GitHubReleases::new();",
            "names `wiring::net` as `wiring::net::GitHubReleases::new`",
        ),
    ] {
        assert_eq!(network_reached(broken), [found], "{broken}");
    }
}

fn network_reader_made(text: &str) -> Vec<String> {
    let text = uncommented(text);
    let tokens = tokens(&text);
    let mut found = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let called = tokens.get(at + 1) == Some(&"(");
        let defined_or_a_method = at >= 1 && matches!(tokens[at - 1], "fn" | ".");
        if *token == "releases" && called && !defined_or_a_method {
            found.push("calls `releases`".to_owned());
        }
        if *token == "GitHubReleases" {
            found.push("names `GitHubReleases`".to_owned());
        }
    }
    found
}

#[test]
fn the_network_reader_is_made_where_the_process_starts_and_by_no_unit_test() {
    let mut found = Vec::new();
    sources(&src(), &mut found);
    let mut makers = Vec::new();
    for (path, text) in found {
        let file = Path::new(&path).strip_prefix(src()).expect("a file of it");
        let (shipped, tests) = text.split_once(TEST_MODULE).unwrap_or((&text, ""));
        for (part, where_) in [(shipped, ""), (tests, " in its tests")] {
            for what in network_reader_made(part) {
                makers.push(format!("{}{where_}: {what}", file.display()));
            }
        }
    }
    makers.sort();
    makers.dedup();
    assert_eq!(
        makers,
        [
            "cli/mod.rs: calls `releases`",
            "wiring/mod.rs: names `GitHubReleases`",
            "wiring/net.rs: names `GitHubReleases`",
        ]
    );
}

#[test]
fn a_file_that_makes_the_network_reader_is_found() {
    for clean in [
        "pub fn releases(variable: &dyn Fn(&str) -> Option<OsString>) -> Box<dyn Releases> {",
        "let releases = (environment.releases)();\nlet current = environment.version;",
        "version: release::current(),\nreleases: &UNPUBLISHED,",
        "// wiring::releases(&variable) makes a GitHubReleases.\nlet dir = host.releases();",
        "let held = release_directory(&given).expect(\"a directory\");",
    ] {
        assert_eq!(network_reader_made(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "releases: &|| wiring::releases(&variable),",
            "calls `releases`",
        ),
        ("let held = releases(&given);", "calls `releases`"),
        (
            "let source = crate::wiring :: releases (&|_| None);",
            "calls `releases`",
        ),
        (
            "let reader = GitHubReleases::new();",
            "names `GitHubReleases`",
        ),
        (
            "None => Box::new(net::GitHubReleases::new()),",
            "names `GitHubReleases`",
        ),
    ] {
        assert_eq!(network_reader_made(broken), [found], "{broken}");
    }
}

const RUNS_A_PROGRAM: [&str; 4] = ["output", "spawn", "status", "assert"];

// A binary run without `RELEASES` set reads its releases from the network, and one run
// without `HOME_OF_THE_TOOL` set keeps its cache under the home directory.
fn run_past_the_releases(text: &str) -> Vec<String> {
    let text = uncommented(text);
    let tokens = tokens(&text);
    let starts: Vec<usize> = (0..tokens.len())
        .filter(|at| tokens[*at] == "fn" && tokens.get(at + 1).is_some_and(|name| word(name)))
        .collect();
    let mut found = Vec::new();
    for (nth, start) in starts.iter().enumerate() {
        let end = starts.get(nth + 1).copied().unwrap_or(tokens.len());
        let body = &tokens[*start..end];
        let runs = body
            .windows(3)
            .any(|call| call[0] == "." && RUNS_A_PROGRAM.contains(&call[1]) && call[2] == "(");
        for given in ["RELEASES", "HOME_OF_THE_TOOL"] {
            if runs && !body.contains(&given) {
                found.push(format!("`{}` runs a program without `{given}`", body[1]));
            }
        }
    }
    found
}

#[test]
fn every_scenario_runs_the_binary_with_its_releases_given() {
    let scenarios = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli.rs");
    assert_eq!(violations(&scenarios, run_past_the_releases), [""; 0]);
    let text = fs::read_to_string(&scenarios).expect("the scenarios");
    assert!(text.contains("\nconst RELEASES: &str = \"WORKLOG_RELEASES\";\n"));
    assert!(text.contains("\nconst HOME_OF_THE_TOOL: &str = \"WORKLOG_HOME\";\n"));
}

#[test]
fn a_helper_that_runs_the_binary_without_its_releases_is_found() {
    for clean in [
        "fn run_as(&self, mut program: Command) -> Ran {\n    program.env(RELEASES, self.unpublished());\n    program.env(HOME_OF_THE_TOOL, self.root.join(\"next\"));\n    program.output()\n}\n",
        "fn spawned(&self) -> Child {\n    program\n        .env(RELEASES, dir)\n        .env(HOME_OF_THE_TOOL, dir)\n        .spawn()\n        .expect(\"the binary runs\")\n}\n",
        "fn run(&self, args: &[&str]) -> Ran {\n    self.run_in(&self.home(), args)\n}\n",
        "fn listed(list: fn(&Host) -> Ran, host: &Host) -> Ran {\n    list(host)\n}\n",
        "fn waited() {\n    // program.output() is run_as's.\n    let output = ran.output;\n}\n",
    ] {
        assert_eq!(run_past_the_releases(clean), [""; 0], "{clean}");
    }
    let without = |name: &str| {
        [
            format!("`{name}` runs a program without `RELEASES`"),
            format!("`{name}` runs a program without `HOME_OF_THE_TOOL`"),
        ]
    };
    for (broken, found) in [
        (
            "fn bare(program: &mut Command) -> Output {\n    program.output().unwrap()\n}\n",
            without("bare").to_vec(),
        ),
        (
            "fn run_as() {\n    program.env(RELEASES, dir).env(HOME_OF_THE_TOOL, dir).output();\n}\n\nfn served(program: &mut Command) -> Child {\n    program\n        . spawn ()\n        .unwrap()\n}\n",
            without("served").to_vec(),
        ),
        (
            "impl Host {\n    fn checked(&self) {\n        Command::cargo_bin(\"lantern\").unwrap().assert().success();\n    }\n}\n",
            without("checked").to_vec(),
        ),
        (
            "fn run_as(&self, mut program: Command) -> Ran {\n    program.env(RELEASES, self.unpublished());\n    program.output()\n}\n",
            vec!["`run_as` runs a program without `HOME_OF_THE_TOOL`".to_owned()],
        ),
        (
            "fn exits(program: &mut Command) -> bool {\n    program.status().unwrap().success()\n}\n",
            without("exits").to_vec(),
        ),
    ] {
        assert_eq!(run_past_the_releases(broken), found, "{broken}");
    }
}

fn outside_tests(text: &str) -> String {
    squeezed(text.split(TEST_MODULE).next().unwrap_or_default())
}

fn app() -> PathBuf {
    src().join("app")
}

const WRITES: [&str; 11] = [
    "amend", "bulk", "claim", "draft", "followup", "fork", "live", "record", "save", "setup",
    "upgrade",
];
const SHARED: [&str; 5] = ["heads", "lookup", "mod", "rules", "testing"];
const WRITES_A_READ_MAY_NOT_USE: [&str; 10] = [
    "amend", "bulk", "claim", "followup", "fork", "live", "record", "save", "setup", "upgrade",
];

const TEST_SUPPORT: &str = "testing";

fn module_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("a module name")
        .to_owned()
}

#[test]
fn a_reference_is_made_by_the_lookup_and_the_topic_index_alone() {
    let mut found = Vec::new();
    sources(&app(), &mut found);
    let mut makers = Vec::new();
    for (path, text) in found {
        let module = module_of(&path);
        if module == TEST_SUPPORT {
            continue;
        }
        for _ in outside_tests(&text).matches("Reference::to(") {
            makers.push(module.clone());
        }
    }
    makers.sort();
    makers.dedup();
    assert_eq!(makers, ["index", "lookup"]);
}

const APPENDS: &str = "appends to the log";
const READS: &str = "reads the log";
const HANDS_ON: &str = "hands the log on";

// The log is the field `usage` of whatever holds the ports.
fn log_reached(text: &str) -> Vec<String> {
    let text = uncommented(text.split(TEST_MODULE).next().unwrap_or_default());
    let tokens = tokens(&text);
    let mut found = Vec::new();
    for at in 1..tokens.len() {
        if tokens[at] == "usage" && tokens[at - 1] == "." {
            let called = tokens.get(at + 1) == Some(&".") && tokens.get(at + 3) == Some(&"(");
            found.push(match tokens.get(at + 2) {
                Some(&"record") if called => APPENDS,
                Some(&"read") if called => READS,
                _ => HANDS_ON,
            });
        }
        let by_path = tokens[at] == "record" && tokens[at - 1] == "::";
        let of_the_port =
            tokens[..at - 1].ends_with(&["Usage"]) || tokens[..at - 1].ends_with(&["Usage", ">"]);
        if by_path && of_the_port {
            found.push(APPENDS);
        }
    }
    found.into_iter().map(str::to_owned).collect()
}

#[test]
fn one_use_case_appends_to_the_usage_log() {
    let mut found = Vec::new();
    sources(&src(), &mut found);
    let mut reached = Vec::new();
    for (path, text) in found {
        if module_of(&path) == TEST_SUPPORT {
            continue;
        }
        let file = Path::new(&path).strip_prefix(src()).expect("a file of it");
        for what in log_reached(&text) {
            reached.push(format!("{}: {what}", file.display()));
        }
    }
    reached.sort();
    assert_eq!(
        reached,
        [
            format!("app/mod.rs: {HANDS_ON}"),
            format!("app/record.rs: {APPENDS}"),
            format!("app/usage.rs: {READS}"),
            format!("app/usage.rs: {READS}"),
            format!("wiring/mod.rs: {HANDS_ON}"),
        ]
    );
}

#[test]
fn a_file_that_reaches_the_usage_log_is_found() {
    for clean in [
        "let counted = usage::usage(deps, machine.as_deref(), since.as_deref())?;",
        "use crate::app::{setup, show, usage};\nlet usage = page(&world, target); usage.status",
        "recorder.record(&deps, outcome);\nlet record = read_or_skip(head)?;",
        "// deps.usage.record(&run) is the recorder's.\nlet usage = 1;",
        "pub fn usage() {}\n\n#[cfg(test)]\nmod tests {\n    fn logged() {\n        world.usage.record(&run).unwrap();\n    }\n}\n",
    ] {
        assert_eq!(log_reached(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        ("deps.usage.record(&invocation)?;", APPENDS),
        (
            "Ok(deps\n    .usage\n    .record(&Invocation {\n        exit: 0,\n    })?)",
            APPENDS,
        ),
        ("let _ = adapters.deps() . usage . record (run);", APPENDS),
        ("Usage::record(log, &invocation)?;", APPENDS),
        ("<dyn Usage>::record(log, &invocation)?;", APPENDS),
        ("for run in deps.usage.read(&kept)? {", READS),
        ("let log = deps.usage;", HANDS_ON),
        ("let record = deps.usage.record;", HANDS_ON),
        ("logged(self.usage, &run)", HANDS_ON),
        ("usage: &self.usage,", HANDS_ON),
    ] {
        assert_eq!(log_reached(broken), [found], "{broken}");
    }
}

fn write_dependencies_named(text: &str) -> Vec<String> {
    tokens(&uncommented(text))
        .into_iter()
        .filter(|token| *token == "Deps")
        .map(|token| format!("names `{token}`"))
        .collect()
}

#[test]
fn a_read_takes_no_write_dependencies_and_decides_no_ending() {
    let mut found = Vec::new();
    sources(&app(), &mut found);
    let mut forbidden: Vec<String> = ["Record::read"].map(str::to_owned).into();
    for module in WRITES_A_READ_MAY_NOT_USE {
        forbidden.push(format!("crate::app::{module}"));
        forbidden.push(format!("super::{module}"));
    }
    let mut reads = 0;
    for (path, text) in found {
        let relative = Path::new(&path).strip_prefix(app()).expect("an app source");
        let root = relative.components().next().expect("a module");
        let module = module_of(&root.as_os_str().to_string_lossy());
        if WRITES.contains(&module.as_str()) || SHARED.contains(&module.as_str()) {
            continue;
        }
        reads += 1;
        let text = outside_tests(&text);
        assert_eq!(write_dependencies_named(&text), [""; 0], "{path}");
        assert_eq!(ending_read(&text), [""; 0], "{path}");
        for needle in &forbidden {
            assert!(!text.contains(needle.as_str()), "{path} holds `{needle}`");
        }
    }
    assert!(reads > 0);
}

#[test]
fn a_read_that_names_write_dependencies_is_found() {
    for clean in [
        "use crate::app::{ReadDeps, Failure};",
        "fn read(deps: &ReadDeps) {}",
        "// A write takes Deps.\nlet deps = read;",
    ] {
        assert_eq!(write_dependencies_named(clean), [""; 0], "{clean}");
    }
    for broken in [
        "fn read(deps: &Deps) { deps.drafts.write(draft)?; }",
        "use crate::app::{Deps as Writing, Failure};",
        "fn read(deps: &crate::app::Deps) { deps.host.bind(machine)?; }",
        "type Writing<'a> = crate::app::Deps<'a>;",
    ] {
        assert_eq!(
            write_dependencies_named(broken),
            ["names `Deps`"],
            "{broken}"
        );
    }
}

fn word(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

fn use_tree<'a>(
    tokens: &[&'a str],
    mut at: usize,
    prefix: &[&'a str],
    found: &mut Vec<Vec<&'a str>>,
) -> usize {
    let mut path = prefix.to_vec();
    while at < tokens.len() {
        match tokens[at] {
            "{" => {
                at += 1;
                while at < tokens.len() && !matches!(tokens[at], "}" | ";") {
                    at = use_tree(tokens, at, &path, found);
                    if tokens.get(at) == Some(&",") {
                        at += 1;
                    }
                }
                return at + 1;
            }
            "," | "}" | ";" => break,
            "as" => at += 2,
            segment => {
                if word(segment) {
                    path.push(segment);
                }
                at += 1;
            }
        }
    }
    found.push(path);
    at
}

// An import grouped in braces names a module apart from the path before the braces.
fn paths(text: &str) -> Vec<Vec<&str>> {
    let tokens = tokens(text);
    let mut found = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        if tokens[at] == "use" {
            at = use_tree(&tokens, at + 1, &[], &mut found);
        } else if word(tokens[at]) {
            let mut path = vec![tokens[at]];
            at += 1;
            while tokens.get(at) == Some(&"::") && tokens.get(at + 1).is_some_and(|t| word(t)) {
                path.push(tokens[at + 1]);
                at += 2;
            }
            found.push(path);
        } else {
            at += 1;
        }
    }
    found
}

fn modules_named(text: &str, parent: &str, modules: &[&str]) -> Vec<String> {
    let text = uncommented(text);
    let mut named = Vec::new();
    for path in paths(&text) {
        for module in modules {
            // From a file under `src`, `super` reaches the crate's modules as `crate` does.
            let under = path.windows(2).any(|pair| {
                pair == [parent, *module] || (parent == "crate" && pair == ["super", *module])
            });
            if under || (path.len() > 1 && path[0] == *module) {
                named.push(format!(
                    "names `{parent}::{module}` as `{}`",
                    path.join("::")
                ));
            }
        }
    }
    named
}

fn write_use_case_named(text: &str) -> Vec<String> {
    modules_named(text, "app", &WRITES)
}

fn wiring_named(text: &str) -> Vec<String> {
    modules_named(text, "crate", &["wiring"])
}

fn web() -> PathBuf {
    src().join("web")
}

#[test]
fn the_web_view_names_no_write_use_case_in_its_tests_either() {
    assert_eq!(violations(&web(), write_use_case_named), [""; 0]);
}

#[test]
fn a_web_file_that_names_a_write_use_case_is_found() {
    for clean in [
        "use crate::app::show::{self, Shown};\nuse crate::app::{Deps, Failure, check, list, search};\n",
        "use crate::app::testing::{\n    Counting, World, entry, fact, followup, fork, head,\n};\n",
        "let heads = fork(&world, &lantern, &topic_fields(\"lantern\", \"\"));",
        "followup(&world, &[&lantern], \"open\", \"\");",
        "use crate::app::{FollowupRow, Reference, Row, TriggerShown, counted};",
        "// crate::app::save::save stores it.\nlet save = 1;",
        "let draft = saved.draft; let claim = row.claim;",
        "use crate::app::usage::{self, MachineUsage};\nlet record = shown.record;",
    ] {
        assert_eq!(write_use_case_named(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "use crate::app::save;",
            "names `app::save` as `crate::app::save`",
        ),
        (
            "use crate::app::{Deps, Failure, check, draft, list};",
            "names `app::draft` as `crate::app::draft`",
        ),
        (
            "use crate::app::{\n    Deps,\n    show::{self, Shown},\n    followup::{\n        new_followup,\n    },\n};",
            "names `app::followup` as `crate::app::followup::new_followup`",
        ),
        (
            "use crate::app::followup::set_trigger as set;",
            "names `app::followup` as `crate::app::followup::set_trigger`",
        ),
        (
            "use crate::app::{self, amend as change};",
            "names `app::amend` as `crate::app::amend`",
        ),
        (
            "let stored = crate::app::save::save(deps, target)?;",
            "names `app::save` as `crate::app::save::save`",
        ),
        (
            "use crate::app;\nlet claimed = app :: claim :: claim(deps, topic, None)?;",
            "names `app::claim` as `app::claim::claim`",
        ),
        (
            "use crate::app::*;\nlet opened = fork::resolve(deps, target)?;",
            "names `app::fork` as `fork::resolve`",
        ),
        (
            "let moved = bulk::move_to(deps, &targets, from, to)?;",
            "names `app::bulk` as `bulk::move_to`",
        ),
        (
            "setup::init(deps, name, None)?;",
            "names `app::setup` as `setup::init`",
        ),
        (
            "use super::super::app::live;",
            "names `app::live` as `super::super::app::live`",
        ),
        (
            "use crate::app::record::record;",
            "names `app::record` as `crate::app::record::record`",
        ),
        (
            "let path = crate::app::upgrade::replace(releases, binary, &checked)?;",
            "names `app::upgrade` as `crate::app::upgrade::replace`",
        ),
    ] {
        assert_eq!(write_use_case_named(broken), [found], "{broken}");
    }
}

#[test]
fn the_web_view_is_handed_its_ports_and_wires_none() {
    assert_eq!(violations(&web(), wiring_named), [""; 0]);
}

#[test]
fn a_web_file_that_names_the_wiring_is_found() {
    for clean in [
        "use crate::app::{Deps, Failure};",
        "// crate::wiring builds them.\nlet wiring = 1;",
        "let wiring = request.wiring;",
    ] {
        assert_eq!(wiring_named(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "use crate::wiring;",
            "names `crate::wiring` as `crate::wiring`",
        ),
        (
            "use crate::{app::Deps, wiring::{self, Adapters}};",
            "names `crate::wiring` as `crate::wiring::self`",
        ),
        (
            "let adapters = crate::wiring::wire(variable, None)?;",
            "names `crate::wiring` as `crate::wiring::wire`",
        ),
        (
            "use crate::*;\nlet adapters = wiring::wire(variable, None)?;",
            "names `crate::wiring` as `wiring::wire`",
        ),
        (
            "let adapters = super::wiring::wire(variable, None)?;",
            "names `crate::wiring` as `super::wiring::wire`",
        ),
    ] {
        let named = wiring_named(broken);
        assert_eq!(named.first().map(String::as_str), Some(found), "{broken}");
    }
}

#[test]
fn the_web_view_names_no_store_in_its_tests_but_the_one_that_fails() {
    assert_eq!(violations(&web(), store_port_named), [""; 0]);
}

fn ending_read(text: &str) -> Vec<String> {
    squeezed(text)
        .matches(".ending")
        .map(|found| format!("holds `{found}`"))
        .collect()
}

#[test]
fn the_web_view_decides_no_ending_in_its_tests_either() {
    assert_eq!(violations(&web(), ending_read), [""; 0]);
}

#[test]
fn a_file_that_reads_an_ending_is_found() {
    for clean in [
        "change: change_or_ending(&version.change, version.ended.as_deref()),",
        "if line.terminator.is_empty() {",
        "let ending = row.ended.clone();",
        "ended: head.ended.clone(),",
    ] {
        assert_eq!(ending_read(clean), [""; 0], "{clean}");
    }
    for broken in [
        "if record.ending.is_none() {",
        "let ended = shown\n    .record\n    .ending\n    .is_some();",
        "heads.iter().filter(|(_, record)| record . ending.is_none())",
    ] {
        assert_eq!(ending_read(broken), ["holds `.ending`"], "{broken}");
    }
}

const ESCAPING_OFF_IN_A_TEMPLATE: [&str; 6] = [
    "|safe",
    "|escape(",
    "|e(",
    "{%filter",
    "{%-filter",
    "{%~filter",
];

fn escaping_turned_off(text: &str) -> Vec<String> {
    let text = squeezed(text);
    let mut found = Vec::new();
    for needle in ESCAPING_OFF_IN_A_TEMPLATE {
        for (at, _) in text.match_indices(needle) {
            // After a second bar the name is what an `or` is given, not a filter.
            if !(needle.starts_with('|') && text[..at].ends_with('|')) {
                found.push(format!("holds `{needle}`"));
            }
        }
    }
    found
}

#[test]
fn no_template_prints_a_value_unescaped() {
    let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
    assert_eq!(
        violations_in(&templates, "html", escaping_turned_off),
        [""; 0]
    );
}

#[test]
fn a_template_that_turns_escaping_off_is_found() {
    for clean in [
        "<dd>{{ field.value }}</dd>",
        "{{ row.summary|truncate(80) }} is safe",
        "{% if page.safe || page.kept %}",
    ] {
        assert_eq!(escaping_turned_off(clean), [""; 0], "{clean}");
    }
    for broken in [
        "<dd>{{ field.value|safe }}</dd>",
        "<dd>{{ field.value | safe }}</dd>",
        "<dd>{{ field.value\n   |\n   safe }}</dd>",
        "{{ body|truncate(80)|safe }}",
    ] {
        assert_eq!(escaping_turned_off(broken), ["holds `|safe`"], "{broken}");
    }
}

#[test]
fn a_template_that_escapes_its_own_way_is_found() {
    for clean in [
        "{% if page.kept || e(page) %}",
        "{% if page.kept || safe %}",
        "{{ row.name|escape }} and {{ row.name|e }}",
        "<p>the filter is kept</p>{% for filter in filters %}{% endfor %}",
    ] {
        assert_eq!(escaping_turned_off(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        ("{{ body|escape(\"none\") }}", "holds `|escape(`"),
        ("{{ body | e ( \"none\" ) }}", "holds `|e(`"),
        (
            "{% filter safe %}{{ body }}{% endfilter %}",
            "holds `{%filter`",
        ),
        (
            "{%- filter lower %}{{ body }}{% endfilter %}",
            "holds `{%-filter`",
        ),
        (
            "{%~ filter lower %}{{ body }}{% endfilter %}",
            "holds `{%~filter`",
        ),
        ("{{ a || b|safe }}", "holds `|safe`"),
    ] {
        assert_eq!(escaping_turned_off(broken), [found], "{broken}");
    }
}

fn unescaped_in_code(text: &str) -> Vec<String> {
    let text = uncommented(text);
    let tokens = tokens(&text);
    let mut found = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if *token == "HtmlSafe" && tokens.get(at + 1) == Some(&"for") {
            let named = tokens.get(at + 2).copied().unwrap_or_default();
            found.push(format!("implements `HtmlSafe` for `{named}`"));
        }
    }
    for path in paths(&text) {
        if path.windows(2).any(|pair| pair == ["filters", "Safe"]) {
            found.push(format!("names `{}`", path.join("::")));
        }
    }
    let squeezed = squeezed(&text);
    for attribute in squeezed.split("#[template(").skip(1) {
        let inside = attribute.split(")]").next().unwrap_or_default();
        if inside.contains("escape=") {
            found.push("sets `escape` on a template".to_owned());
        }
    }
    found
}

#[test]
fn one_type_alone_is_printed_unescaped() {
    let found = violations(&src(), unescaped_in_code);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].ends_with("web/markdown.rs: implements `HtmlSafe` for `Markup`"),
        "{found:?}"
    );
}

#[test]
fn code_that_opens_another_way_past_escaping_is_found() {
    for clean in [
        "#[derive(Template)]\n#[template(path = \"listing.html\")]\npub struct Listing {",
        "// impl askama::filters::HtmlSafe for Link {}\nlet escape = 1;",
        "fn shown<T: HtmlSafe>(value: &T) -> String {",
        "let safe = Safe::new(); let escape = true;",
    ] {
        assert_eq!(unescaped_in_code(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "impl askama::filters::HtmlSafe for Link {}",
            "implements `HtmlSafe` for `Link`",
        ),
        (
            "use askama::filters::HtmlSafe as Trusted;\nimpl HtmlSafe\n    for Markup\n{}",
            "implements `HtmlSafe` for `Markup`",
        ),
        (
            "let shown = askama::filters::Safe(&row.summary);",
            "names `askama::filters::Safe`",
        ),
        (
            "use askama::filters::{HtmlSafe, Safe};",
            "names `askama::filters::Safe`",
        ),
        (
            "#[template(path = \"listing.html\", escape = \"none\")]",
            "sets `escape` on a template",
        ),
        (
            "#[template(\n    source = \"{{ body }}\",\n    ext = \"txt\",\n    escape=\"none\"\n)]",
            "sets `escape` on a template",
        ),
    ] {
        assert_eq!(unescaped_in_code(broken), [found], "{broken}");
    }
}

fn render_reach(text: &str) -> Vec<String> {
    let mut found = modules_named(text, "std", &["fs", "env", "process", "net", "time"]);
    found.extend(modules_named(text, "crate", &["web", "cli", "wiring"]));
    found.extend(modules_named(text, "domain", &["ports"]));
    // A use case takes the ports, so a module that cannot name them runs none.
    for token in tokens(&uncommented(text)) {
        if matches!(token, "Deps" | "ReadDeps") {
            found.push(format!("names `{token}`"));
        }
    }
    found
}

#[test]
fn what_both_renderers_share_reaches_the_domain_and_what_the_use_cases_return() {
    assert_eq!(violations(&src().join("render"), render_reach), [""; 0]);
}

#[test]
fn a_shared_renderer_that_reaches_a_renderer_the_ports_or_the_host_is_found() {
    for clean in [
        "use similar::{ChangeTag, TextDiff};\nuse crate::domain::version::Stamp;",
        "pub mod diff;\nlet app = 1; let process = web.cli;",
        "// crate::web::pages links them, over the Deps of a request.",
        "let name = env!(\"CARGO_PKG_NAME\");",
        "use crate::app::show::{Diff, Side};\nuse crate::app::{FollowupRow, counted};",
    ] {
        assert_eq!(render_reach(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        ("use std::fs;", "names `std::fs` as `std::fs`"),
        (
            "use std::{fmt, env::var};",
            "names `std::env` as `std::env::var`",
        ),
        (
            "std::process::exit(1);",
            "names `std::process` as `std::process::exit`",
        ),
        (
            "use std::net::SocketAddr;",
            "names `std::net` as `std::net::SocketAddr`",
        ),
        ("use crate::app::{Deps, show};", "names `Deps`"),
        ("use crate::app::{ReadDeps, show};", "names `ReadDeps`"),
        (
            "let shown = show::show(&crate::app::Deps { store }, target)?;",
            "names `Deps`",
        ),
        (
            "use crate::{domain::id::VersionId, web::pages};",
            "names `crate::web` as `crate::web::pages`",
        ),
        (
            "let text = crate::cli::render(&diff);",
            "names `crate::cli` as `crate::cli::render`",
        ),
        (
            "use crate::wiring;",
            "names `crate::wiring` as `crate::wiring`",
        ),
        (
            "use super::web::pages;",
            "names `crate::web` as `super::web::pages`",
        ),
        (
            "let text = super::super::cli::render(&diff);",
            "names `crate::cli` as `super::super::cli::render`",
        ),
        (
            "use crate::domain::ports::Clock;",
            "names `domain::ports` as `crate::domain::ports::Clock`",
        ),
        (
            "use crate::domain::{id::VersionId, ports::{Clock, Host}};",
            "names `domain::ports` as `crate::domain::ports::Clock`",
        ),
        (
            "let now = std::time::SystemTime::now();",
            "names `std::time` as `std::time::SystemTime::now`",
        ),
    ] {
        let named = render_reach(broken);
        assert_eq!(named.first().map(String::as_str), Some(found), "{broken}");
    }
}

fn renderer_named(text: &str) -> Vec<String> {
    modules_named(text, "crate", &["render"])
}

#[test]
fn a_use_case_names_no_renderer_in_its_tests_either() {
    assert_eq!(violations(&app(), renderer_named), [""; 0]);
}

#[test]
fn a_use_case_file_that_names_the_shared_renderer_is_found() {
    for clean in [
        "use crate::app::{Deps, Failure};\nuse crate::domain::version::Stamp;",
        "// crate::render words it for both renderers.\nlet render = 1;",
        "let render = row.render; let diff = render.diff;",
    ] {
        assert_eq!(renderer_named(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "use crate::render;",
            "names `crate::render` as `crate::render`",
        ),
        (
            "use crate::{domain::id::VersionId, render::{ended, written}};",
            "names `crate::render` as `crate::render::ended`",
        ),
        (
            "let words = crate::render::ended(reason);",
            "names `crate::render` as `crate::render::ended`",
        ),
        (
            "use crate::*;\nlet lines = render::diff::hunks(before, after);",
            "names `crate::render` as `render::diff::hunks`",
        ),
        (
            "let words = super::super::render::ended(reason);",
            "names `crate::render` as `super::super::render::ended`",
        ),
        (
            "use super::super::render::{ended, written};",
            "names `crate::render` as `super::super::render::ended`",
        ),
    ] {
        let named = renderer_named(broken);
        assert_eq!(named.first().map(String::as_str), Some(found), "{broken}");
    }
}

fn host_reached(text: &str) -> Vec<String> {
    modules_named(text, "std", &["fs", "env", "process"])
}

#[test]
fn the_web_view_reaches_no_file_environment_or_process_in_its_tests_either() {
    assert_eq!(violations(&web(), host_reached), [""; 0]);
}

#[test]
fn a_web_file_that_reaches_the_host_is_found() {
    for clean in [
        "use std::net::{Ipv4Addr, Ipv6Addr};\nuse std::panic::{AssertUnwindSafe, catch_unwind};",
        "use std::io::{Read, Write};\nuse std::net::TcpStream;\nuse std::time::Duration;",
        "std::thread::scope(|writers| {",
        "let process = 1; let fs = page.env;",
    ] {
        assert_eq!(host_reached(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        (
            "let text = std::fs::read_to_string(path)?;",
            "names `std::fs` as `std::fs::read_to_string`",
        ),
        ("use std::{env, io};", "names `std::env` as `std::env`"),
        (
            "use std::process::Command;",
            "names `std::process` as `std::process::Command`",
        ),
        ("use std::fs as files;", "names `std::fs` as `std::fs`"),
    ] {
        assert_eq!(host_reached(broken), [found], "{broken}");
    }
}

// A recalled version is checked against nothing, so only the cache that read its text once
// builds one.
#[test]
fn a_version_is_recalled_only_by_the_cache() {
    let mut found = Vec::new();
    sources(&src(), &mut found);
    let mut callers: Vec<String> = found
        .iter()
        .filter(|(_, text)| text.contains("recalled("))
        .map(|(path, _)| {
            let file = Path::new(path).strip_prefix(src()).expect("a file of it");
            file.display().to_string()
        })
        .collect();
    callers.sort();
    assert_eq!(callers, ["domain/version.rs", "wiring/fs/store/cache.rs"]);
}
