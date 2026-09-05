//! The domain touches nothing outside memory. A convention nothing checks
//! is one a routine edit falsifies, so this reads the sources.

use std::fs;
use std::path::Path;

fn sources(dir: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("readable source directory") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push((
                path.display().to_string(),
                fs::read_to_string(&path).expect("readable source"),
            ));
        }
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
            "crate::cli",
            "crate::app",
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
            "crate::cli",
        ],
    );
}
