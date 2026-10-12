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
        "Failure::Store(StoreError::io(\"desk\", \"gone\"))",
        "store: Stored::new(&FailingStore),",
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

// `write_version` applies no rule of the store.
fn rule_free_write_named(text: &str) -> Vec<String> {
    let text = uncommented(text.split(TEST_MODULE).next().unwrap_or_default());
    let tokens = tokens(&text);
    let mut named = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        let defined = at >= 1 && tokens[at - 1] == "fn";
        if *token == "write_version" && !defined {
            named.push("names `write_version`".to_owned());
        }
    }
    named
}

#[test]
fn no_file_of_the_crate_writes_a_version_past_the_rules_of_the_store() {
    assert_eq!(violations(&src(), rule_free_write_named), [""; 0]);
}

#[test]
fn a_file_that_names_the_write_past_the_rules_is_found() {
    for clean in [
        "pub fn write_version(store: &Path, version: &Version) -> Result<(), Failure> {\n}\n",
        "/// As `write_version` does.\npub fn wire() {}\n",
        "pub fn wire() {} // not write_version\n",
        "pub fn wire() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    fn put() {\n        write_version(&store, &version).unwrap();\n    }\n}\n",
    ] {
        assert_eq!(rule_free_write_named(clean), [""; 0], "{clean}");
    }
    for broken in [
        "crate::wiring::write_version(&store, &version)?;",
        "use crate::wiring::write_version;",
        "use crate::wiring::{wire, write_version};",
        "versions.iter().try_for_each(|version| write_version(root, version))",
        "let write = wiring::write_version;",
        "pub fn write_version(store: &Path) {\n    write_version(store)\n}\n",
    ] {
        assert_eq!(
            rule_free_write_named(broken),
            ["names `write_version`"],
            "{broken}"
        );
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
    let mut forbidden: Vec<String> = ["Record::read", "drafts.write", "drafts.delete", "store.put"]
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
        assert_eq!(ending_read(&text), [""; 0], "{path}");
        for needle in &forbidden {
            assert!(!text.contains(needle.as_str()), "{path} holds `{needle}`");
        }
    }
    assert!(reads > 0);
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
    if tokens(&uncommented(text)).contains(&"Deps") {
        found.push("names `Deps`".to_owned());
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
