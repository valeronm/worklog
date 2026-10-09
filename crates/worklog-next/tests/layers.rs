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
