//! The domain touches nothing outside memory. A convention nothing checks
//! is one a routine edit falsifies, so this reads the sources.

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
            "crate::fs",
            "crate::net",
            "crate::cli",
            "crate::app",
            "ureq",
        ],
    );
}

/// A version's fields are opaque to whatever stores, chains and drafts it,
/// so a kind can gain a field without the store changing; the two meet
/// only in `app`.
#[test]
fn the_store_reads_no_kind() {
    let kinds = ["::entry", "::fact", "::topic", "::followup", "kind_keys"];
    for layer in ["domain/version.rs", "domain/draft.rs", "fs"] {
        reaches_nothing_in(layer, &kinds);
    }
}

/// A use case reaches the host only through the ports, so what a command
/// needs from outside is named in one place and swapped in a test.
#[test]
fn the_app_reaches_the_host_only_through_ports() {
    reaches_nothing_in(
        "app",
        &[
            "std::fs",
            "std::env",
            "std::process",
            "std::net",
            "crate::fs",
            "crate::net",
            "ureq",
        ],
    );
}

/// The pages are a rendering of the reads; what they need from the host
/// is wired in `cli`, so a page cannot read a file the reads do not.
#[test]
fn the_web_reads_only_through_app() {
    reaches_nothing_in(
        "web",
        &[
            "std::fs",
            "std::env",
            "std::process",
            "crate::fs",
            "crate::net",
            "crate::cli",
        ],
    );
}
