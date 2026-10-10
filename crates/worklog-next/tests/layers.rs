//! The domain touches nothing outside memory, and a convention nothing
//! checks is one a routine edit falsifies.

use std::fs;
use std::path::Path;

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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(layer);
    let mut found = Vec::new();
    sources(&root, &mut found);
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
            "crate::fs",
            "getrandom",
        ],
    );
}

#[test]
fn no_module_outside_the_schema_names_it() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in fs::read_dir(src.join("domain")).expect("readable source directory") {
        let path = entry.expect("readable entry").path();
        if path.file_name().is_some_and(|name| name != "schema") {
            sources(&path, &mut found);
        }
    }
    sources(&src.join("fs"), &mut found);
    assert!(!found.is_empty());
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
    for layer in ["domain", "app", "fs"] {
        reaches_nothing_in(layer, &["serde_json"]);
    }
}

#[test]
fn a_result_s_json_carries_every_field_under_the_convention() {
    let mut found = Vec::new();
    sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut found,
    );
    assert!(!found.is_empty());
    for (path, text) in found {
        let text: String = text.split_whitespace().collect();
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
            "crate::fs",
        ],
    );
}

#[test]
fn the_command_line_builds_no_adapter() {
    reaches_nothing_in("cli", &["crate::fs"]);
}

// A chain rustfmt breaks across lines matches only with all whitespace removed.
fn outside_tests(text: &str) -> String {
    text.split("#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect()
}

fn app() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("app")
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
