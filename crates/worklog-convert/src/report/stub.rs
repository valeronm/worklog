use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write;

use super::{Run, spaced, weight};
use crate::key::{ConversionKey, DocumentField, Folded, Key, PLACEHOLDER};
use crate::question::Open;

const HEADING: &str =
    "# A table is written once: move a line under the table the file already holds.\n";

const STAMP: &str = "\n# [conversion] is needed once an answer makes the conversion write a \
    version of its own:\n\
    # [conversion]\n\
    # written = \"?\"  # a date and time with an offset, not quoted\n\
    # machine = \"?\"  # the machine name those versions are stamped with\n";

enum Shape {
    Text,
    WholeList,
    Items,
}

fn shape(key: &Key) -> Shape {
    match key {
        Key::Document(_, DocumentField::Topics) => Shape::WholeList,
        _ if key.item().is_some() => Shape::Items,
        _ => Shape::Text,
    }
}

// A comment ends at a line break.
fn commented(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    spaced(&lines.join(" "))
}

// The keys of one table share it, in the order of the file's tables.
fn table_of(key: &Key) -> Key {
    key.of_table().unwrap_or_else(|| match key {
        Key::Tag(_) => Key::Tag(Folded::new("")),
        Key::Machine(_) => Key::Machine(String::new()),
        _ => Key::Conversion(ConversionKey::Written),
    })
}

#[derive(Default)]
struct Line {
    weight: usize,
    evidence: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    New,
    Added,
    // The file holds the line with a placeholder.
    Unanswered,
}

impl Standing {
    fn of(key: &Key, run: &Run) -> Standing {
        if run.unanswered.contains(key) {
            Standing::Unanswered
        } else if run.held.contains(&key.table()) {
            Standing::Added
        } else {
            Standing::New
        }
    }
}

fn lines_of(open: &[Open], run: &Run) -> BTreeMap<Key, Line> {
    let mut lines: BTreeMap<Key, Line> = BTreeMap::new();
    for question in open {
        for offer in question.offers() {
            let line = lines.entry(offer.key.of_line()).or_default();
            line.weight = line.weight.max(weight(question));
            line.evidence
                .extend(offer.evidence.as_deref().map(commented));
        }
    }
    for (key, line) in &mut lines {
        if matches!(key, Key::Tag(tag) if run.topics.contains(tag)) {
            line.evidence.push("a topic of this name exists".to_owned());
        }
    }
    lines
}

// A header written twice stops the whole file from reading, and so does a key, so what
// the file holds already is a comment.
pub(super) fn block(open: &[Open], run: &Run) -> String {
    let lines = lines_of(open, run);
    let mut ordered: Vec<(Key, Standing, &Key, &Line)> = (lines.iter())
        .filter(|(key, _)| key.line().is_some())
        .map(|(key, line)| (table_of(key), Standing::of(key, run), key, line))
        .collect();
    ordered.sort_by(|a, b| (&a.0, a.1, Reverse(a.3.weight)).cmp(&(&b.0, b.1, Reverse(b.3.weight))));
    let mut block = String::new();
    let mut under = None;
    for (of, standing, key, line) in &ordered {
        let Some(written) = key.line() else {
            continue;
        };
        if block.is_empty() {
            block.push_str(HEADING);
        }
        let table = key.table();
        let mark = if *standing == Standing::New { "" } else { "# " };
        if under != Some((of, *standing)) {
            let _ = match standing {
                Standing::New => writeln!(block, "\n[{table}]"),
                Standing::Added => writeln!(block, "\n# add under the existing [{table}]:"),
                Standing::Unanswered => {
                    writeln!(block, "\n# in the file under [{table}], not answered:")
                }
            };
            under = Some((of, *standing));
        }
        let evidence = &line.evidence;
        let said = evidence.join("; ");
        let _ = match shape(key) {
            Shape::Text => writeln!(block, "{mark}{written} = \"{PLACEHOLDER}\"  # {said}"),
            Shape::WholeList => {
                writeln!(block, "{mark}{written} = [\"{PLACEHOLDER}\"]  # {said}")
            }
            Shape::Items if evidence.is_empty() => writeln!(block, "{mark}{written} = []"),
            Shape::Items => {
                let _ = writeln!(block, "{mark}{written} = [");
                for said in evidence {
                    let _ = writeln!(block, "{mark}  \"{PLACEHOLDER}\",  # {said}");
                }
                writeln!(block, "{mark}]")
            }
        };
    }
    block
}

// The stamp is asked for only once the conversion authors a version, and a file may hold
// it before.
pub(super) fn with_stamp(block: &str, open: &[Open], run: &Run) -> String {
    let asked = open
        .iter()
        .any(|open| matches!(open, Open::Conversion { .. }));
    let held = run
        .held
        .contains(&Key::Conversion(ConversionKey::Written).table());
    match block.strip_prefix(HEADING) {
        Some(rest) if !asked && !held => format!("{HEADING}{STAMP}{rest}"),
        _ => block.to_owned(),
    }
}
