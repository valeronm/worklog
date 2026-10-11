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

fn sources(path: &Path, found: &mut Vec<(String, String)>) {
    if path.is_dir() {
        for entry in fs::read_dir(path).expect("readable source directory") {
            sources(&entry.expect("readable entry").path(), found);
        }
    } else if path.extension().is_some_and(|e| e == "rs") {
        found.push((
            path.display().to_string(),
            fs::read_to_string(path).expect("readable source"),
        ));
    }
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

#[test]
fn a_result_is_given_its_json_form_by_the_command_line_alone() {
    for layer in ["domain", "app", "wiring"] {
        reaches_nothing_in(layer, &["serde_json"]);
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
        if *token == "MemoryStore" || (*token == "Store" && !variant) {
            named.push((*token).to_owned());
        }
    }
    named
}

const MAIN: &str = "fn main() { std::process::exit(worklog_next::cli::main()); }";

fn logic_in_main(text: &str) -> Vec<String> {
    if tokens(&uncommented(text)) == tokens(MAIN) {
        Vec::new()
    } else {
        vec![format!("is not `{MAIN}`")]
    }
}

fn violations(root: &Path, rule: fn(&str) -> Vec<String>) -> Vec<String> {
    let mut found = Vec::new();
    sources(root, &mut found);
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
    let calling = "fn main() {\n    std::process::exit(worklog_next::cli::main());\n}\n";
    assert_eq!(logic_in_main(calling), [""; 0]);
    let spread = "// The exit code is the command line's.\nfn main()\n{\n    std::process::exit(\n                          worklog_next::cli::main()\n    );\n}\n";
    assert_eq!(logic_in_main(spread), [""; 0]);

    let deciding = "fn main() {\n    let code = worklog_next::cli::main();\n    if code != 0 {\n        panic!();\n    }\n}\n";
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
    ] {
        assert_eq!(store_port_named(clean), [""; 0], "{clean}");
    }
    for (broken, found) in [
        ("use crate::domain::ports::Store;", "Store"),
        ("use crate::domain::ports::{Host, Store as _};", "Store"),
        ("fn read(store: &dyn Store) {}", "Store"),
        ("let store = MemoryStore::default();", "MemoryStore"),
    ] {
        assert_eq!(store_port_named(broken), [found], "{broken}");
    }
}

const TEST_MODULE: &str = "#[cfg(test)]\nmod tests";

fn wiring_opened(text: &str) -> Vec<String> {
    let text = outside_tests(text);
    let mut found = Vec::new();
    if !text.contains("modfs;") {
        found.push("does not declare `mod fs;`".to_owned());
    }
    for (needle, what) in [
        ("pubmodfs", "declares `pub mod fs`"),
        (")modfs", "declares `mod fs` to others"),
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
    let kept = "mod fs;\n\nuse fs::{FsHost, FsStore};\n\npub struct Adapters {\n    store: FsStore,\n    host: FsHost,\n}\n\n#[cfg(test)]\nmod tests {\n    pub use super::*;\n}\n";
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

fn outside_tests(text: &str) -> String {
    squeezed(text.split(TEST_MODULE).next().unwrap_or_default())
}

fn app() -> PathBuf {
    src().join("app")
}

const WRITES: [&str; 9] = [
    "amend", "bulk", "claim", "draft", "followup", "fork", "live", "save", "setup",
];
const SHARED: [&str; 5] = ["heads", "lookup", "mod", "rules", "testing"];
const WRITES_A_READ_MAY_NOT_USE: [&str; 8] = [
    "amend", "bulk", "claim", "followup", "fork", "live", "save", "setup",
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
fn one_use_case_puts_a_version_in_the_store() {
    let mut found = Vec::new();
    sources(&app(), &mut found);
    let mut writers = Vec::new();
    for (path, text) in found {
        let module = module_of(&path);
        if module == TEST_SUPPORT {
            continue;
        }
        for _ in outside_tests(&text).matches(".store.put(") {
            writers.push(module.clone());
        }
    }
    assert_eq!(writers, ["save"]);
}

#[test]
fn a_read_decides_no_ending_and_writes_nothing() {
    let mut found = Vec::new();
    sources(&app(), &mut found);
    let mut forbidden: Vec<String> = [
        ".ending",
        "Record::read",
        "drafts.write",
        "drafts.delete",
        "store.put",
    ]
    .map(str::to_owned)
    .into();
    for module in WRITES_A_READ_MAY_NOT_USE {
        forbidden.push(format!("crate::app::{module}"));
        forbidden.push(format!("super::{module}"));
    }
    let mut reads = 0;
    for (path, text) in found {
        let module = module_of(&path);
        if WRITES.contains(&module.as_str()) || SHARED.contains(&module.as_str()) {
            continue;
        }
        reads += 1;
        let text = outside_tests(&text);
        for needle in &forbidden {
            assert!(!text.contains(needle.as_str()), "{path} holds `{needle}`");
        }
    }
    assert!(reads > 0);
}
