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
