use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use worklog::app::check::{Finding, check};
use worklog::wiring::wire;

use super::{Shown, converted, finish, main_in};
use crate::decisions::Decisions;
use crate::fixture::{
    CONVERSION, DAY_1, DAY_2, DAY_3, FILE, OldStore, at, composed, desk, no_host, planned_of, rich,
    scalar, tree,
};
use crate::old;
use crate::plan::{Complete, Outcome, Planned, plan};
use crate::write::Place;

const COUNTS: &str = "read: 18 documents, 40 versions\n\
    written: 19 documents, 44 versions\n\
    \x20 topic: 6 documents, 12 versions\n\
    \x20 fact: 4 documents, 12 versions\n\
    \x20 entry: 3 documents, 8 versions\n\
    \x20 followup: 4 documents, 9 versions\n\
    \x20 claim: 2 documents, 3 versions\n\
    versions written, by what each was made from:\n\
    \x20 a version of the store: 37, of which 3 from the two halves of a rename\n\
    \x20 a version of another document: 4, of which 3 begin a document\n\
    \x20 the decision file alone: 3\n";

const TO_WRITE: &str = "\n## What a run with `--into` writes\n\n```\n\
    read: 18 documents, 40 versions\n\
    to write: 19 documents, 44 versions\n\
    \x20 topic: 6 documents, 12 versions\n\
    \x20 fact: 4 documents, 12 versions\n\
    \x20 entry: 3 documents, 8 versions\n\
    \x20 followup: 4 documents, 9 versions\n\
    \x20 claim: 2 documents, 3 versions\n\
    versions to write, by what each is made from:\n\
    \x20 a version of the store: 37, of which 3 from the two halves of a rename\n\
    \x20 a version of another document: 4, of which 3 begin a document\n\
    \x20 the decision file alone: 3\n```\n";

const WROTE: &str = "\n## What the conversion wrote itself\n\
    \n### Topics created\n\n\
    A topic here that the store already holds under another spelling, or that nothing refers \
    to, may be a `[topics.<name>]` table under a wrong name.\n\n\
    | topic | summary | part_of | uses | referred to by |\n\
    | --- | --- | --- | --- | --- |\n\
    | firmware | The firmware of [[lantern]] | lantern |  | nothing |\n\
    \n### Edges added\n\n\
    Each is in a topic's table and in no `includes` of the store.\n\n\
    | topic | part_of | uses |\n\
    | --- | --- | --- |\n\
    | atlas |  | lantern |\n\
    \n### Facts filed under another topic\n\n\
    Each keeps its topic in every version of the store and has the other in one more.\n\n\
    | fact | from | to |\n\
    | --- | --- | --- |\n\
    | phone/battery | phone | atlas |\n\n";

struct Ran {
    out: String,
    err: String,
    code: i32,
}

fn run_in(cwd: Option<&Path>, line: &[&str]) -> Ran {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = [&["worklog-convert"], line].concat();
    let code = main_in(args, cwd, &mut out, &mut err);
    Ran {
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        code,
    }
}

fn run(line: &[&Path]) -> Ran {
    let line: Vec<&str> = line.iter().map(|part| part.to_str().unwrap()).collect();
    run_in(None, &line)
}

fn flag(name: &str) -> &Path {
    Path::new(name)
}

fn convert(old: &Path, file: Option<&Path>, target: Option<&Path>) -> Ran {
    let mut line = vec![old];
    if let Some(file) = file {
        line.extend([flag("--decisions"), file]);
    }
    if let Some(target) = target {
        line.extend([flag("--into"), target]);
    }
    run(&line)
}

type Tree = BTreeMap<PathBuf, Option<Vec<u8>>>;

fn around(old: &OldStore) -> Tree {
    tree(&old.beside(""))
}

fn answered(old: &OldStore) -> PathBuf {
    let file = old.beside("decisions.toml");
    std::fs::write(&file, FILE).unwrap();
    file
}

fn refusal(ran: &Ran, path: &Path) {
    assert_eq!(ran.out, "");
    assert_eq!(ran.code, 1, "{}", ran.err);
    let named = format!("worklog-convert: {}: ", path.display());
    assert!(ran.err.starts_with(&named), "{}", ran.err);
    assert_eq!(ran.err.lines().count(), 1, "{}", ran.err);
    assert!(ran.err.ends_with('\n'));
}

fn reported(old: &OldStore, decisions: &str) -> String {
    let store = old::read(old.root()).unwrap();
    let decisions = Decisions::parse(decisions).unwrap();
    let planned = plan(&store, &decisions).into_planned();
    Shown::of(&store, &planned).report(&planned, &decisions)
}

fn standing(ran: &Ran) -> &str {
    let first = ran.out.lines().next().unwrap();
    let (standing, _) = first.split_once(" in ").unwrap();
    standing
}

fn notices_of_a_clean_check(target: &Path) -> String {
    let nowhere = no_host(&target.with_extension("no-host"));
    let adapters = wire(&nowhere, Some(target.to_path_buf())).unwrap().unwrap();
    adapters.settle().unwrap();
    let checked = check(&adapters.deps()).unwrap();
    assert_eq!(checked.problems, []);
    assert_eq!(checked.fork_count, 0);
    let notices: Vec<String> = (checked.notices.iter())
        .map(|notice| format!("notice: {}: {}\n", notice.label, notice.what))
        .collect();
    notices.concat()
}

fn summary(target: &Path) -> String {
    let notices = notices_of_a_clean_check(target);
    let target = target.display();
    format!(
        "{COUNTS}{WROTE}{notices}written to {target}\n\
         to bind a host to it: worklog init <its machine topic> --store {target}\n"
    )
}

#[test]
fn a_dry_run_of_a_store_with_questions_prints_the_report_and_writes_nothing() {
    let old = rich();
    let before = around(&old);

    let ran = convert(old.root(), None, None);

    assert_eq!(ran.out, reported(&old, ""));
    assert!(ran.out.starts_with("0 blocked, "), "{}", ran.out);
    assert!(
        ran.out
            .lines()
            .next()
            .unwrap()
            .ends_with(" open, 0 decisions unused in 18 documents, 40 versions"),
        "{}",
        ran.out
    );
    for part in [
        "\n## Open\n",
        "\n### Tags with no answer\n",
        "\n## Stubs\n\n```toml\n",
    ] {
        assert!(ran.out.contains(part), "{part}: {}", ran.out);
    }
    assert_eq!(
        ran.err,
        format!(
            "worklog-convert: {}: the report on stdout lists them\n",
            standing(&ran)
        )
    );
    assert!(standing(&ran).starts_with("0 blocked, "), "{}", ran.err);
    assert_eq!(ran.code, 1);
    assert_eq!(around(&old), before);
}

#[test]
fn a_dry_run_with_every_answer_says_nothing_is_open_and_writes_nothing() {
    let old = rich();
    let file = answered(&old);
    let before = around(&old);

    let ran = convert(old.root(), Some(&file), None);

    assert_eq!(ran.out, format!("{}{TO_WRITE}", reported(&old, FILE)));
    assert!(
        ran.out
            .starts_with("nothing is open in 18 documents, 40 versions\n"),
        "{}",
        ran.out
    );
    let itself = WROTE.replace("conversion wrote itself", "conversion will write itself");
    assert!(ran.out.contains(itself.trim_end()), "{}", ran.out);
    assert!(!ran.out.contains("## Stubs"), "{}", ran.out);
    assert_eq!((ran.err.as_str(), ran.code), ("", 0));
    assert_eq!(around(&old), before);
}

#[test]
fn a_run_into_a_target_with_every_answer_writes_a_store_its_own_check_passes() {
    let old = rich();
    let file = answered(&old);
    let before = tree(old.root());
    let target = old.beside("new");

    let ran = convert(old.root(), Some(&file), Some(&target));

    assert_eq!(ran.err, "");
    assert_eq!(ran.code, 0);
    assert_eq!(ran.out, summary(&target));
    assert_eq!(tree(old.root()), before);
    assert!(!old.beside("new.partial").exists());
    let files = tree(&target).values().flatten().count();
    assert_eq!(files, 44);
}

#[test]
fn the_decisions_come_before_or_after_the_store_and_the_target() {
    let old = rich();
    let file = answered(&old);
    let target = old.beside("new");

    let ran = run(&[
        flag("--into"),
        &target,
        flag("--decisions"),
        &file,
        old.root(),
    ]);

    assert_eq!((ran.err.as_str(), ran.code), ("", 0));
    assert_eq!(ran.out, summary(&target));
}

fn asking_much() -> OldStore {
    let old = rich();
    let phone = at(DAY_1, "phone");
    old.fact("ghost/hum", "It hums", &[], at(DAY_1, "desk"));
    let lamp = composed(
        "2026-09/2026-09-01-unwritten",
        at(DAY_1, "desk"),
        "new",
        &[],
        old.entry("2026-09/2026-09-02-seen", "Seen", &["optics"], phone)
            .fields,
        "\n",
    );
    let orphan = old.followup("2026-09-07-orphan", &lamp, "Find it", &["optics"], phone);
    let nowhere = [("recheck", scalar("touching beacon"))];
    old.changed(&orphan, "recheck", &nowhere, at(DAY_2, "desk"));
    let coil = old.fact("lantern/coil", "Six ohms", &[], at(DAY_1, "desk"));
    let touching = [("recheck", scalar("touching atlas"))];
    old.changed(&coil, "recheck", &touching, at(DAY_2, "desk"));

    let bench = at(DAY_1, "bench");
    let adrift = [("ghost-ship", &["~/projects/ghost"][..])];
    old.machine_topic("bench", &adrift, &[], bench);
    old.machine_topic("workshop", &[], &[], bench);
    old.topic("Shade_One", "A shade", &[], desk(DAY_1));
    let dial = old.topic("Old_Dial", "A dial", &[], desk(DAY_1));
    old.renamed(&dial, "dial", desk(DAY_2));
    let globe = old.topic("globe", "A globe", &[], desk(DAY_1));
    let mut fields = globe.fields.clone();
    fields.set("summary", scalar(""));
    old.write(&composed(
        "globe",
        desk(DAY_2),
        "save",
        &[&globe],
        fields,
        "\n",
    ));
    old.topic("Knob", "A knob", &[], desk(DAY_1));
    old.topic("KNOB", "Another knob", &[], desk(DAY_1));
    old.fact("knob/turns", "Three turns", &[], desk(DAY_1));
    old
}

fn stubs_of(report: &str) -> &str {
    let (_, block) = report.split_once("\n## Stubs\n\n```toml\n").unwrap();
    block.strip_suffix("```\n").unwrap()
}

fn after<'a>(text: &'a str, start: &str) -> Option<&'a str> {
    text.split_once(start).map(|(_, rest)| rest)
}

fn word(text: &str) -> &str {
    let end = text.find([' ', ',', ';', ':']).unwrap_or(text.len());
    &text[..end]
}

enum Answered {
    Line(String),
    Items(Vec<String>),
}

#[derive(Default)]
struct Filling {
    tables: Vec<(String, Vec<(String, Answered)>)>,
    names: usize,
}

impl Filling {
    fn answer(&mut self, table: &str, key: &str, said: &str) -> String {
        let quoted = |text: &str| format!("\"{text}\"");
        if table == "conversion" && key == "written" {
            return "2026-10-20T09:30:00+01:00".to_owned();
        }
        if table == "conversion" {
            return quoted("desk");
        }
        if table == "machines" {
            return quoted(after(said, "the machine of ").map_or("desk", word));
        }
        if table == "tags" && key == "optics" {
            return quoted("lenses");
        }
        if table == "tags" || key.starts_with("tags.") {
            return quoted("lantern");
        }
        match key {
            "ended" => quoted(word(after(said, "one of: ").unwrap())),
            "topics" if said.contains("in another case") => {
                format!("[{}]", quoted(word(after(said, " is ").unwrap())))
            }
            "topics" => "[\"lantern\"]".to_owned(),
            "entry" | "touching" => quoted(""),
            "summary" | "recheck_summary" => quoted("One line"),
            "name" => {
                self.names += 1;
                quoted(&format!("named-{}", self.names))
            }
            _ if key.starts_with("former_names.") => quoted(""),
            _ => panic!("no answer for {key} under [{table}]: {said}"),
        }
    }

    fn table(&mut self, name: &str) -> &mut Vec<(String, Answered)> {
        let at = (self.tables.iter()).position(|(held, _)| held == name);
        let at = at.unwrap_or_else(|| {
            self.tables.push((name.to_owned(), Vec::new()));
            self.tables.len() - 1
        });
        &mut self.tables[at].1
    }

    fn put(&mut self, table: &str, key: &str, answered: Answered) {
        let lines = self.table(table);
        let held = lines.iter_mut().find(|(held, _)| held == key);
        match (held, answered) {
            (Some((_, Answered::Items(held))), Answered::Items(more)) => held.extend(more),
            (Some((_, held)), answered) => *held = answered,
            (None, answered) => lines.push((key.to_owned(), answered)),
        }
    }

    fn take(&mut self, stubs: &str) {
        let mut table: Option<(String, bool)> = None;
        let mut list: Option<(String, Vec<String>)> = None;
        for line in stubs.lines() {
            let bare = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'));
            let added = line
                .strip_prefix("# add under the existing [")
                .and_then(|rest| rest.strip_suffix("]:"));
            if let Some(header) = bare.or(added) {
                table = Some((header.to_owned(), added.is_some()));
                continue;
            }
            if line.is_empty() {
                table = None;
                continue;
            }
            let Some((name, commented)) = &table else {
                continue;
            };
            let line = if *commented {
                line.strip_prefix("# ").unwrap()
            } else {
                line
            };
            let (text, said) = line.split_once("  # ").unwrap_or((line, ""));
            let name = name.clone();
            if let Some((key, mut items)) = list.take() {
                if text == "]" {
                    self.put(&name, &key, Answered::Items(items));
                } else {
                    assert_eq!(text.trim_start(), "\"?\",", "{line}");
                    items.push(word(after(said, "write ").unwrap()).to_owned());
                    list = Some((key, items));
                }
                continue;
            }
            let (key, value) = text.split_once(" = ").unwrap();
            match value {
                "[" => list = Some((key.to_owned(), Vec::new())),
                "[]" => self.put(&name, key, Answered::Items(Vec::new())),
                _ => {
                    assert!(value.contains("\"?\""), "{line}");
                    let answer = self.answer(&name, key, said);
                    self.put(&name, key, Answered::Line(answer));
                }
            }
        }
    }

    fn text(&self) -> String {
        let mut file = String::new();
        for (table, lines) in &self.tables {
            file.push_str(&["[", table, "]\n"].concat());
            for (key, answered) in lines {
                let value = match answered {
                    Answered::Line(value) => value.clone(),
                    Answered::Items(items) => {
                        let items: Vec<String> =
                            items.iter().map(|item| format!("\"{item}\"")).collect();
                        format!("[{}]", items.join(", "))
                    }
                };
                file.push_str(&[key, " = ", &value, "\n"].concat());
            }
        }
        assert!(!file.contains("\"?\""), "{file}");
        file
    }
}

fn headings(report: &str) -> Vec<String> {
    let titles = report.lines().filter_map(|line| line.strip_prefix("### "));
    titles.map(str::to_owned).collect()
}

#[test]
fn the_stubs_of_each_run_filled_and_run_again_end_with_nothing_open_and_a_store_written() {
    let old = asking_much();
    let file = old.beside("decisions.toml");
    let first = convert(old.root(), None, None);
    assert_eq!(first.code, 1);
    assert_eq!(
        headings(&first.out),
        [
            "Tags with no answer",
            "Included topics not placed",
            "Machines with no topic",
            "Machines of several topics",
            "Claims on no topic",
            "Ended documents with no reason",
            "Followups with no topic",
            "Facts under no topic",
            "Names the new store refuses",
            "Former names the new store refuses",
            "Summaries that are not one line",
            "Rechecks with no summary",
            "Followups of no entry",
            "Rechecks touching no topic",
            "Ended topics referred to",
            "Names several topics answer to",
        ]
    );

    let mut filling = Filling::default();
    let mut seen = Vec::new();
    let mut last = first;
    for _ in 0..8 {
        if last.code == 0 {
            break;
        }
        if last.out.contains("| tags.optics | lenses names no topic |") {
            filling.table("topics.lenses");
        }
        if last.out.contains("\n## Stubs\n") {
            filling.take(stubs_of(&last.out));
        }
        std::fs::write(&file, filling.text()).unwrap();
        last = convert(old.root(), Some(&file), None);
        seen.extend(headings(&last.out));
    }

    assert_eq!((last.err.as_str(), last.code), ("", 0), "{}", last.out);
    assert!(last.out.starts_with("nothing is open in "), "{}", last.out);
    for asked in [
        "Answers that name nothing",
        "Topics to create with no summary",
        "The conversion's own stamp",
    ] {
        assert!(seen.iter().any(|title| title == asked), "{asked}: {seen:?}");
    }
    assert!(
        last.out
            .contains("\n| lenses | One line |  |  | 2 documents |\n"),
        "{}",
        last.out
    );

    let target = old.beside("new");
    let written = convert(old.root(), Some(&file), Some(&target));
    assert_eq!((written.err.as_str(), written.code), ("", 0));
    notices_of_a_clean_check(&target);
}

#[test]
fn a_file_filled_in_part_applies_its_answers_and_leaves_the_rest_open() {
    let old = rich();
    let before = around(&old);
    let first = convert(old.root(), None, None);
    let stubs = stubs_of(&first.out);
    assert!(stubs.contains("\n[tags]\n"), "{stubs}");
    let file = old.beside("decisions.toml");

    std::fs::write(&file, stubs).unwrap();
    let pasted = convert(old.root(), Some(&file), None);
    assert_eq!(pasted.code, 1);
    assert_eq!(standing(&pasted), standing(&first));
    assert_eq!(pasted.err, first.err);
    let open = |report: &str| {
        let (open, _) = report.split_once("\n## Stubs\n").unwrap();
        open.to_owned()
    };
    assert_eq!(open(&pasted.out), open(&first.out));
    let again = stubs_of(&pasted.out);
    assert!(
        again.contains("\n# in the file under [tags], not answered:\n# maps = \"?\"  # "),
        "{again}"
    );
    assert!(!again.contains("\n[tags]\n"), "{again}");
    assert!(!again.contains("# add under the existing"), "{again}");

    let some = stubs
        .replace("maps = \"?\"", "maps = \"atlas\"")
        .replace("rust = \"?\"", "rust = \"\"");
    std::fs::write(&file, some).unwrap();
    let partly = convert(old.root(), Some(&file), None);
    assert_eq!(partly.code, 1);
    let tags = after(&partly.out, "\n### Tags with no answer\n").unwrap();
    let tags = &tags[..tags.find("\n### ").unwrap()];
    assert!(tags.contains("\n| relay | "), "{tags}");
    assert!(!tags.contains("| maps |"), "{tags}");
    assert!(!tags.contains("| rust |"), "{tags}");
    let again = stubs_of(&partly.out);
    assert!(
        again.contains("\n# in the file under [tags], not answered:\n# relay = \"?\"  # "),
        "{again}"
    );
    assert!(!again.contains("maps = "), "{again}");

    std::fs::remove_file(&file).unwrap();
    assert_eq!(around(&old), before);
}

#[test]
fn the_stamp_of_the_conversion_is_shown_before_it_is_needed_and_never_unused() {
    let old = rich();
    let first = convert(old.root(), None, None);
    let note = "\n# [conversion] is needed once an answer makes the conversion write a version \
                of its own:\n\
                # [conversion]\n\
                # written = \"?\"  # a date and time with an offset, not quoted\n\
                # machine = \"?\"  # the machine name those versions are stamped with\n";
    assert!(stubs_of(&first.out).contains(note), "{}", first.out);
    assert!(!first.out.contains("The conversion's own stamp"));

    let file = old.beside("decisions.toml");
    std::fs::write(&file, CONVERSION).unwrap();
    let early = convert(old.root(), Some(&file), None);
    assert_eq!(standing(&early), standing(&first));
    assert!(!early.out.contains("Unused decisions"), "{}", early.out);
    assert!(!early.out.contains("[conversion]"), "{}", early.out);
}

#[test]
fn a_tag_that_is_the_name_of_a_topic_of_the_store_says_so_in_its_row() {
    let old = rich();
    let first = convert(old.root(), None, None);

    let tags = after(&first.out, "\n### Tags with no answer\n").unwrap();
    let cell = |tag: &str| {
        let row = tags
            .lines()
            .find(|row| row.starts_with(&format!("| {tag} | ")));
        row.unwrap().rsplit(" | ").next().unwrap()
    };
    assert_eq!(cell("relay"), "yes |");
    assert_eq!(cell("maps"), " |");
}

#[test]
fn an_entry_its_answered_tags_leave_under_no_topic_is_listed() {
    let old = OldStore::new();
    old.machine_topic("desk", &[], &[], desk(DAY_1));
    old.entry(
        "2026-09/2026-09-02-seen",
        "Seen",
        &["rust", "optics"],
        desk(DAY_1),
    );
    old.entry(
        "2026-09/2026-09-03-heard",
        "Heard",
        &["optics"],
        desk(DAY_1),
    );
    let file = old.beside("decisions.toml");

    std::fs::write(&file, "[tags]\nrust = \"\"\n").unwrap();
    let asked = convert(old.root(), Some(&file), None);
    assert!(
        !asked.out.contains("Entries under no topic"),
        "{}",
        asked.out
    );

    std::fs::write(&file, "[tags]\nrust = \"\"\noptics = \"\"\n").unwrap();
    let ran = convert(old.root(), Some(&file), None);
    assert_eq!((ran.err.as_str(), ran.code), ("", 0));
    assert!(
        ran.out.contains(
            "\n## What the conversion will write itself\n\
             \n### Entries under no topic\n\n\
             Each is filed under no topic by the tags of its last version as the file answers \
             them: `tags` or `topics` under the entry's table files it under one.\n\n\
             | entry | tags |\n\
             | --- | --- |\n\
             | entry/2026-09/2026-09-02-seen | rust, optics |\n\
             | entry/2026-09/2026-09-03-heard | optics |\n"
        ),
        "{}",
        ran.out
    );
}

#[test]
fn a_run_into_a_target_with_something_open_prints_the_report_and_writes_nothing() {
    let old = rich();
    let target = old.beside("new");
    let before = around(&old);
    let dry = convert(old.root(), None, None);

    let ran = convert(old.root(), None, Some(&target));

    assert_eq!(ran.out, dry.out);
    assert_eq!(
        ran.err,
        format!(
            "worklog-convert: {}: not written: {}\n",
            target.display(),
            standing(&dry)
        )
    );
    assert!(standing(&dry).ends_with(" open, 0 decisions unused"));
    assert_eq!(ran.code, 1);
    assert_eq!(around(&old), before);

    let more = format!("{FILE}[documents.\"fact/atlas/scale\"]\nsummary = \"One line\"\n");
    let file = old.beside("decisions.toml");
    std::fs::write(&file, more).unwrap();
    let before = around(&old);
    let ran = convert(old.root(), Some(&file), Some(&target));
    assert!(
        ran.out
            .starts_with("0 blocked, 0 open, 1 decision unused in "),
        "{}",
        ran.out
    );
    assert!(
        ran.err
            .ends_with(": not written: 0 blocked, 0 open, 1 decision unused\n"),
        "{}",
        ran.err
    );
    assert_eq!(ran.code, 1);
    assert_eq!(around(&old), before);
}

#[test]
fn a_dry_run_with_an_unused_decision_exits_one() {
    let old = rich();
    let more = format!("{FILE}[documents.\"fact/atlas/scale\"]\nsummary = \"One line\"\n");
    let file = old.beside("decisions.toml");
    std::fs::write(&file, more).unwrap();

    let ran = convert(old.root(), Some(&file), None);

    assert!(ran.out.contains("\n## Unused decisions\n"), "{}", ran.out);
    let silent = "worklog-convert: 0 blocked, 0 open, 1 decision unused: the report on stdout \
                  lists them\n";
    assert_eq!((ran.err.as_str(), ran.code), (silent, 1));
}

#[test]
fn a_placeholder_no_question_asks_for_is_listed_and_stops_the_write() {
    let old = rich();
    let file = old.beside("decisions.toml");
    std::fs::write(&file, format!("{FILE}[machines]\nbench = \"?\"\n")).unwrap();
    let target = old.beside("new");
    let before = around(&old);

    let dry = convert(old.root(), Some(&file), None);
    assert!(
        dry.out
            .contains("\n## Unused decisions\n\n- machines.bench\n"),
        "{}",
        dry.out
    );
    assert_eq!(dry.code, 1);
    let ran = convert(old.root(), Some(&file), Some(&target));
    assert!(
        ran.err
            .ends_with(": not written: 0 blocked, 0 open, 1 decision unused\n"),
        "{}",
        ran.err
    );
    assert_eq!(ran.code, 1);
    assert_eq!(around(&old), before);
}

#[test]
fn an_old_store_that_is_absent_or_no_store_is_refused_by_its_path() {
    let old = rich();
    let target = old.beside("new");
    let before = around(&old);

    let absent = old.beside("absent");
    refusal(&convert(&absent, None, None), &absent);
    refusal(&convert(&absent, None, Some(&target)), &absent);

    let file = answered(&old);
    refusal(&convert(&file, None, None), &file);

    let empty = old.beside("empty");
    std::fs::create_dir(&empty).unwrap();
    let ran = convert(&empty, None, Some(&target));
    refusal(&ran, &empty);
    assert!(
        ran.err.ends_with(
            ": holds no store: none of entry, fact, topic and followup is a directory in it\n"
        ),
        "{}",
        ran.err
    );

    std::fs::remove_dir(&empty).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert_eq!(around(&old), before);
}

#[test]
fn a_decision_file_that_is_absent_is_refused_and_not_read_as_an_empty_one() {
    let old = rich();
    let before = around(&old);
    let absent = old.beside("decisions.toml");

    refusal(&convert(old.root(), Some(&absent), None), &absent);
    let target = old.beside("new");
    refusal(&convert(old.root(), Some(&absent), Some(&target)), &absent);
    refusal(&convert(old.root(), Some(old.root()), None), old.root());
    assert_eq!(around(&old), before);
}

#[test]
fn a_target_that_cannot_take_the_store_is_refused_by_its_path_before_anything_is_planned() {
    let old = rich();
    let file = answered(&old);
    let full = old.beside("full");
    std::fs::create_dir(&full).unwrap();
    std::fs::write(full.join("notes.md"), "kept\n").unwrap();
    let inside = old.root().join("new");
    let before = around(&old);

    for file in [Some(file.as_path()), None] {
        let ran = convert(old.root(), file, Some(&full));
        refusal(&ran, &full);
        assert!(
            ran.err.ends_with(": is not empty: it holds notes.md\n"),
            "{}",
            ran.err
        );
    }
    let ran = convert(old.root(), Some(&file), Some(&inside));
    refusal(&ran, &inside);
    assert!(
        ran.err
            .ends_with(": is inside the store being converted, which is never written to\n"),
        "{}",
        ran.err
    );
    assert_eq!(around(&old), before);
}

#[test]
fn a_store_that_cannot_be_staged_is_refused_by_the_path_in_the_way() {
    let old = rich();
    let file = answered(&old);
    let staging = old.beside("new.partial");
    std::fs::create_dir(&staging).unwrap();
    let before = around(&old);

    let ran = convert(old.root(), Some(&file), Some(&old.beside("new")));

    refusal(&ran, &staging);
    assert_eq!(around(&old), before);
}

#[test]
fn a_decision_file_that_is_no_decision_file_is_listed_by_line_or_key() {
    let old = rich();
    let file = old.beside("decisions.toml");
    let target = old.beside("new");
    let with = |text: &str| {
        std::fs::write(&file, text).unwrap();
        let before = around(&old);
        let dry = convert(old.root(), Some(&file), None);
        let into = convert(old.root(), Some(&file), Some(&target));
        assert_eq!(around(&old), before);
        assert_eq!(dry.code, 1);
        assert_eq!((into.out.as_str(), into.code), (dry.out.as_str(), 1));
        assert_eq!(into.err, dry.err);
        assert_eq!(
            dry.err,
            format!(
                "worklog-convert: {}: is not read as a decision file\n",
                file.display()
            )
        );
        dry.out
    };

    let syntax = with("[tags]\nrelay = \"lantern\"\nmaps = \n");
    assert!(syntax.starts_with("line 3: "), "{syntax}");
    assert_eq!(syntax.lines().count(), 1, "{syntax}");

    let unknown = with(&format!("{FILE}[shelves]\natlas = \"lantern\"\n"));
    assert_eq!(unknown, "shelves: is not a table of a decision file\n");

    let several = with("[tags]\nrelay = 4\nmaps = \"\"\n[conversion]\nmachine = \" \"\n");
    assert_eq!(
        several,
        "tags.relay: is not text\nconversion.machine: is empty\n\
         conversion.written: is missing\n"
    );
}

#[test]
fn a_command_line_that_is_not_the_command_exits_two_with_its_usage() {
    for line in [
        &[][..],
        &["--verbose", "store"],
        &["store", "other"],
        &["store", "--into"],
    ] {
        let ran = run_in(None, line);
        assert_eq!(ran.out, "", "{line:?}");
        assert_eq!(ran.code, 2, "{line:?}");
        assert!(ran.err.starts_with("error: "), "{}", ran.err);
    }
}

#[test]
fn help_says_what_a_run_does_and_what_it_never_touches() {
    let ran = run_in(None, &["--help"]);
    assert_eq!((ran.err.as_str(), ran.code), ("", 0));
    let words: Vec<&str> = ran.out.split_whitespace().collect();
    let text = words.join(" ");
    for said in [
        "Usage: worklog-convert [OPTIONS] <OLD_STORE>",
        "--decisions <FILE>",
        "--into <NEW_STORE>",
        "Without `--into` a run writes nothing",
        "A `\"?\"` in the decision file answers nothing",
        "--into <NEW_STORE> The directory to write the new store to: absent, or a directory holding nothing",
        "The old store is never modified",
        "exit 1 is a refusal or a conversion with something open and 2 a usage error",
    ] {
        assert!(text.contains(said), "{said}: {}", ran.out);
    }
}

#[test]
fn two_runs_into_two_targets_print_the_same_summary() {
    let old = rich();
    let file = answered(&old);
    let into = |name: &str| {
        let target = old.beside(name);
        let ran = convert(old.root(), Some(&file), Some(&target));
        assert_eq!((ran.err.as_str(), ran.code), ("", 0));
        ran.out.replace(target.to_str().unwrap(), "<target>")
    };

    assert_eq!(into("one"), into("two"));
}

#[test]
fn a_path_is_read_from_the_working_directory_unless_it_starts_at_the_root() {
    let old = rich();
    answered(&old);
    let beside = old.beside("");

    let ran = run_in(
        Some(&beside),
        &["store", "--decisions", "decisions.toml", "--into", "new"],
    );
    assert_eq!((ran.err.as_str(), ran.code), ("", 0));
    assert_eq!(ran.out, summary(&beside.join("new")));

    let before = around(&old);
    for line in [
        &["store"][..],
        &[
            old.root().to_str().unwrap(),
            "--decisions",
            "decisions.toml",
        ],
        &[old.root().to_str().unwrap(), "--into", "other"],
    ] {
        let ran = run_in(None, line);
        assert_eq!(ran.out, "");
        assert_eq!(
            (ran.err.as_str(), ran.code),
            ("worklog-convert: no working directory\n", 1)
        );
    }
    assert_eq!(around(&old), before);
}

struct Failing(ErrorKind);

impl Write for Failing {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(self.0))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_stdout_that_fails_is_a_failure_unless_its_reader_closed_it() {
    let old = rich();
    let file = answered(&old);
    let line: Vec<OsString> = [
        "worklog-convert".into(),
        old.root().into(),
        "--decisions".into(),
        file.into(),
    ]
    .to_vec();
    let to = |out: &mut dyn Write| {
        let mut err = Vec::new();
        let code = main_in(line.clone(), None, out, &mut err);
        (String::from_utf8(err).unwrap(), code)
    };

    let (err, code) = to(&mut Failing(ErrorKind::StorageFull));
    assert!(err.starts_with("worklog-convert: stdout: "), "{err}");
    assert_eq!(code, 1);
    assert_eq!(to(&mut Failing(ErrorKind::BrokenPipe)), (String::new(), 0));
}

fn failing(change: impl FnOnce(&mut Planned), into: bool) -> Ran {
    let old = rich();
    let store = old::read(old.root()).unwrap();
    let mut planned = plan(&store, &Decisions::parse(FILE).unwrap()).into_planned();
    change(&mut planned);
    let complete = Complete::unchecked(planned);
    let planned = complete.planned();
    let shown = Shown::of(&store, planned);
    let place = Place::checked(&old.beside("new"), old.root()).unwrap();
    let place = into.then_some(&place);
    let before = around(&old);

    let result = converted(&complete, &store, place, "listed\n", &shown.itself(planned));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = finish(result, &mut out, &mut err);

    assert_eq!(around(&old), before);
    Ran {
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        code,
    }
}

fn versions_of<'p, 'a>(
    planned: &'p mut Planned<'a>,
    kind: old::Kind,
    slug: &str,
) -> &'p mut Vec<crate::plan::Version<'a>> {
    &mut planned_of(planned, &crate::plan::document_id(kind, slug)).versions
}

#[test]
fn a_plan_with_a_key_held_twice_is_a_refusal_of_the_dry_run_and_of_the_write() {
    let coil = crate::plan::document_id(old::Kind::Fact, "relay/coil");
    for into in [false, true] {
        let mut key = String::new();
        let ran = failing(
            |planned| {
                let versions = versions_of(planned, old::Kind::Fact, "relay/coil");
                key.clone_from(&versions[0].key);
                let mut again = versions[0].clone();
                again.parents = vec![key.clone()];
                versions.push(again);
            },
            into,
        );
        let refusal = format!(
            "worklog-convert: the plan cannot be written, which is a defect of the converter \
             and no question to answer: {coil}: has two versions under the key {key}\n"
        );
        assert_eq!(ran.err, refusal);
        let printed = if into { "" } else { "listed\n" };
        assert_eq!((ran.out.as_str(), ran.code), (printed, 1));
    }
}

#[test]
fn a_plan_that_does_not_account_for_the_store_is_a_refusal_of_the_dry_run_and_of_the_write() {
    for into in [false, true] {
        let ran = failing(
            |planned| {
                versions_of(planned, old::Kind::Topic, "lantern").pop();
            },
            into,
        );
        let lines: Vec<&str> = ran.err.lines().collect();
        assert_eq!(
            lines[0],
            "worklog-convert: the plan does not account for the store, which is a defect of \
             the converter and no question to answer: 40 versions were read, and 36 versions \
             were made from them, 3 of those from the two halves of a rename"
        );
        assert_eq!(lines.len(), 2, "{}", ran.err);
        assert!(lines[1].starts_with("  topic/lantern/"), "{}", ran.err);
        assert!(lines[1].ends_with(": became no version"), "{}", ran.err);
        let printed = if into { "" } else { "listed\n" };
        assert_eq!((ran.out.as_str(), ran.code), (printed, 1));
    }
}

#[test]
fn a_store_written_that_is_not_sound_is_a_refusal_and_is_gone() {
    let scale = crate::plan::document_id(old::Kind::Fact, "atlas/scale");
    let unjoined = |planned: &mut Planned| {
        versions_of(planned, old::Kind::Fact, "atlas/scale")[3]
            .parents
            .clear();
    };

    let ran = failing(unjoined, true);
    let lines: Vec<&str> = ran.err.lines().collect();
    assert_eq!(
        lines[0],
        "worklog-convert: the store as planned is not sound, which is a defect of the \
         converter and no question to answer:"
    );
    assert!(
        lines[1].starts_with(&format!("  {scale}: has 3 heads, ")),
        "{}",
        ran.err
    );
    assert_eq!(lines[2..], ["  1 document is forked"]);
    assert_eq!((ran.out.as_str(), ran.code), ("", 1));

    let dry = failing(unjoined, false);
    assert_eq!((dry.err.as_str(), dry.code), ("", 0));
    assert!(
        dry.out
            .starts_with("listed\n\n## What a run with `--into` writes\n")
    );
}

#[test]
fn a_store_with_a_document_to_mend_is_reported_and_never_written() {
    let old = rich();
    let legend = old.fact("atlas/legend", "Blue is water", &[], desk(DAY_1));
    let (one, other) = old.forked(&legend, desk(DAY_2), desk(DAY_3));
    let file = answered(&old);
    let target = old.beside("new");
    let before = around(&old);

    let dry = convert(old.root(), Some(&file), None);
    assert_eq!(standing(&dry), "1 blocked, 0 open, 0 decisions unused");
    let mut heads = [one.id.to_string(), other.id.to_string()];
    heads.sort();
    assert!(
        dry.out.contains(&format!(
            "\n## Blocked\n\n### Forked\n\n\
             Close each fork with `worklog resolve` and `worklog save`, run with a `worklog` 0.4 binary.\n\n\
             | document | heads |\n| --- | --- |\n| fact/atlas/legend | {} |\n",
            heads.join(", ")
        )),
        "{}",
        dry.out
    );
    assert!(!dry.out.contains("--into"), "{}", dry.out);
    let blocked = "1 blocked, 0 open, 0 decisions unused";
    assert_eq!(
        (dry.err.as_str(), dry.code),
        (
            format!("worklog-convert: {blocked}: the report on stdout lists them\n").as_str(),
            1
        )
    );

    let into = convert(old.root(), Some(&file), Some(&target));
    assert_eq!(into.out, dry.out);
    assert_eq!(
        (into.err.as_str(), into.code),
        (
            format!(
                "worklog-convert: {}: not written: {blocked}\n",
                target.display()
            )
            .as_str(),
            1
        )
    );
    assert_eq!(around(&old), before);
}

fn set_writable(root: &Path, writable: bool) {
    use std::os::unix::fs::PermissionsExt;

    let (directory, file) = if writable {
        (0o755, 0o644)
    } else {
        (0o555, 0o444)
    };
    let mut paths: Vec<PathBuf> = tree(root).into_keys().map(|path| root.join(path)).collect();
    paths.push(root.to_path_buf());
    for path in paths {
        let mode = if path.is_dir() { directory } else { file };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
}

fn modified(root: &Path) -> BTreeMap<PathBuf, SystemTime> {
    let at = |path: &Path| std::fs::symlink_metadata(path).unwrap().modified().unwrap();
    let within = tree(root).into_keys().map(|path| root.join(path));
    let paths = within.chain([root.to_path_buf()]);
    paths.map(|path| (path.clone(), at(&path))).collect()
}

#[test]
fn a_store_that_cannot_be_written_to_is_converted_and_no_file_of_it_is_touched() {
    let old = rich();
    let file = answered(&old);
    let target = old.beside("new");
    set_writable(old.root(), false);
    assert!(std::fs::write(old.root().join("probe"), "").is_err());
    let before = (tree(old.root()), modified(old.root()));

    let dry = convert(old.root(), Some(&file), None);
    let ran = convert(old.root(), Some(&file), Some(&target));
    let after = (tree(old.root()), modified(old.root()));
    set_writable(old.root(), true);

    assert_eq!((dry.err.as_str(), dry.code), ("", 0));
    assert_eq!((ran.err.as_str(), ran.code), ("", 0));
    assert_eq!(ran.out, summary(&target));
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
}

#[test]
fn the_summary_lists_what_the_conversion_wrote_and_what_the_new_check_notes() {
    let old = rich();
    let store = old::read(old.root()).unwrap();
    let Outcome::Complete(complete) = plan(&store, &Decisions::parse(FILE).unwrap()) else {
        panic!("something is open");
    };
    let ready = crate::write::ready(&complete, &store).unwrap();
    let notice = |label: &str, what: &str| Finding {
        document: None,
        label: label.to_owned(),
        what: what.to_owned(),
    };
    let notices = [
        notice("atlas", "is placed nowhere"),
        notice("phone", "is old"),
    ];
    let target = Path::new("/atlas/new");

    assert_eq!(
        super::summary(
            ready.counts(),
            WROTE.strip_suffix('\n').unwrap(),
            &notices,
            target
        ),
        format!(
            "{COUNTS}{WROTE}notice: atlas: is placed nowhere\nnotice: phone: is old\n\
             written to /atlas/new\n\
             to bind a host to it: worklog init <its machine topic> --store /atlas/new\n"
        ),
    );
    assert_eq!(
        super::summary(ready.counts(), "", &[], target),
        format!(
            "{COUNTS}written to /atlas/new\n\
             to bind a host to it: worklog init <its machine topic> --store /atlas/new\n"
        ),
    );
}
