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

#[test]
fn the_domain_does_no_io() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domain");
    let mut found = Vec::new();
    sources(&root, &mut found);
    assert!(!found.is_empty());
    let forbidden = [
        "std::fs",
        "std::env",
        "std::process",
        "std::io",
        "std::net",
        "crate::fs",
        "crate::cli",
        "crate::app",
    ];
    for (path, text) in found {
        for needle in forbidden {
            assert!(
                !text.contains(needle),
                "{path} reaches outside the domain through `{needle}`"
            );
        }
    }
}
