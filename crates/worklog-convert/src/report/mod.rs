//! The text a run prints of what stands between a store and its conversion, and of what
//! the conversion writes that no version of the store holds.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::mem::discriminant;

use crate::key::{DocumentKey, Folded, Key};
use crate::old::Kind;
use crate::question::{
    Authored, Blocked, Open, Overridden, Question, Through, Unfiled, Unjoined, examples, reasons,
};
use crate::text::{counted, cut, first_line};

mod stub;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) documents: usize,
    pub(crate) versions: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct Itself<'a> {
    pub(crate) authored: &'a [Authored],
    pub(crate) overridden: &'a [Overridden],
    // The documents that refer to each topic the file creates, by the topic's name.
    pub(crate) referred: &'a BTreeMap<String, usize>,
    pub(crate) unfiled: &'a [Unfiled],
}

#[derive(Clone, Copy)]
pub(crate) struct Run<'a> {
    pub(crate) questions: &'a [Question],
    pub(crate) wrote: Itself<'a>,
    pub(crate) unused: &'a [Key],
    // The headers of the tables the file holds, as a stub writes them.
    pub(crate) held: &'a BTreeSet<String>,
    pub(crate) unanswered: &'a BTreeSet<Key>,
    // The names of the topics of the store that are not ended.
    pub(crate) topics: &'a BTreeSet<Folded>,
    pub(crate) read: Counts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Standing {
    pub(crate) blocked: usize,
    pub(crate) open: usize,
    pub(crate) unused: usize,
}

impl Standing {
    pub(crate) fn of(questions: &[Question], unused: &[Key]) -> Standing {
        let (blocked, open) = ordered(questions);
        Standing::counted(&blocked, &open, &in_order(unused))
    }

    fn counted(blocked: &[Blocked], open: &[Open], unused: &[Key]) -> Standing {
        Standing {
            blocked: blocked.len(),
            open: open.len(),
            unused: unused.len(),
        }
    }

    pub(crate) fn is_clear(&self) -> bool {
        self.blocked + self.open + self.unused == 0
    }
}

impl fmt::Display for Standing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unused = counted(self.unused, "decision");
        write!(
            f,
            "{} blocked, {} open, {unused} unused",
            self.blocked, self.open
        )
    }
}

struct Head {
    title: &'static str,
    advice: &'static str,
    columns: &'static [&'static str],
}

struct Row {
    head: &'static Head,
    count: usize,
    cells: Vec<String>,
}

const fn head(title: &'static str, advice: &'static str, columns: &'static [&'static str]) -> Head {
    Head {
        title,
        advice,
        columns,
    }
}

const FORKED: Head = head(
    "Forked",
    "Close each fork with `worklog resolve` and `worklog save`.",
    &["document", "heads"],
);
const WAITING: Head = head(
    "Waiting for versions",
    "Sync with the machine that holds the missing versions, until `worklog check` passes.",
    &["document", "missing"],
);
const UNREADABLE: Head = head(
    "Unreadable",
    "Bring each file back from a machine that reads it, until `worklog check` passes.",
    &["path", "reason"],
);
const SKIPPED: Head = head(
    "Not read",
    "Move each path out of the store: no command reads it.",
    &["path"],
);
const RENAME_CUT_SHORT: Head = head(
    "Renames cut short",
    "Sync with the machine that renamed the document, which holds the versions under the new slug.",
    &["document", "renamed to"],
);
const WRITE_TIME: Head = head(
    "Write times not read",
    "No command rewrites a stored version: the converter has to read this form first.",
    &["document", "version", "written"],
);
const FOREIGN_KEYS: Head = head(
    "Keys outside the grammar",
    "No command rewrites a stored version: the converter has to place these keys first.",
    &["document", "version", "keys"],
);
const RENAME_UNJOINED: Head = head(
    "Renames not joined",
    "No command rewrites a stored rename: the converter has to join this case first.",
    &["document", "other slugs", "case"],
);
const VALUE: Head = head(
    "Stored values the conversion cannot place",
    "No command rewrites a stored version: the converter has to place each of these first.",
    &["document", "version", "field", "value", "why"],
);

const CONVERSION: Head = head(
    "The conversion's own stamp",
    "Under `[conversion]`: `written`, a date and time with an offset, and `machine`, a machine name.",
    &["versions the conversion authors"],
);
const TAG: Head = head(
    "Tags with no answer",
    "Under `[tags]`, or under a document's `tags`: a topic name, or `\"\"` to drop the tag.",
    &["tag", "documents", "among them", "a topic of this name"],
);
const INCLUDE: Head = head(
    "Included topics not placed",
    "Under `[topics.<topic>]`: the item in `part_of`, `uses` or `drop`.",
    &["topic", "item", "names a topic"],
);
const TOPIC_SUMMARY: Head = head(
    "Topics to create with no summary",
    "Under `[topics.<topic>]`: `summary`, one line; a topic not meant to be created is a table under a wrong name.",
    &["topic"],
);
const MACHINE: Head = head(
    "Machines with no topic",
    "Under `[machines]`: the name of the machine's topic; or `machine` under a document's table, for that document alone.",
    &["machine", "versions", "among them"],
);
const MACHINE_TOPICS: Head = head(
    "Machines of several topics",
    "Under `[machines]`: the name of the one topic that is the machine.",
    &["machine", "topics"],
);
const CLAIM: Head = head(
    "Claims on no topic",
    "Under `[topics.<machine topic>]`: the item in `unclaim`; or a `[topics.<item>]` table that creates the topic.",
    &["machine topic", "item"],
);
const ENDED: Head = head(
    "Ended documents with no reason",
    "Under the document's table: `ended`, one of the reasons, and `ended_by` where a document replaced it.",
    &["document", "kind", "already", "note", "reasons"],
);
const NO_TOPIC: Head = head(
    "Followups with no topic",
    "Under the document's table: `topics`, the names of the topics it is filed under.",
    &["document", "summary"],
);
const FACT_TOPIC: Head = head(
    "Facts under no topic",
    "Under the fact's table: `topics`, the name of its one topic.",
    &["fact", "topic"],
);
const NAME: Head = head(
    "Names the new store refuses",
    "Under the document's table: `name`.",
    &["document", "name", "why"],
);
const SUMMARY: Head = head(
    "Summaries that are not one line",
    "Under the document's table: `summary`, one line.",
    &["document"],
);
const RECHECK_SUMMARY: Head = head(
    "Rechecks with no summary",
    "Under the fact's table: `recheck_summary`, one line for the followup that rechecks it.",
    &["fact", "touching"],
);
const ENTRY: Head = head(
    "Followups of no entry",
    "Under the followup's table: `entry`, an entry's slug, or `\"\"` for none.",
    &["followup", "entry"],
);
const TOUCHING: Head = head(
    "Rechecks touching no topic",
    "Under the table of the followup, or of the fact a recheck is on: `touching`, a topic's name, or `\"\"` for no trigger.",
    &["document", "touching"],
);
const ENDED_TOPIC: Head = head(
    "Ended topics referred to",
    "Under the document's table: `topics` for a fact, `tags` for a tag, `touching` for a recheck; under the topic's table, `drop` for an `includes` item and `unclaim` for a claim.",
    &["document", "through", "topic"],
);
const OPEN_TOPIC_CASE: Head = head(
    "Names several topics answer to",
    "Under the document's table, name the one meant as the store spells it: `topics` for a fact, `touching` for a recheck.",
    &["document", "through", "name", "topics"],
);
const CYCLE: Head = head(
    "Topic cycles",
    "Change a placement under one of the topics: a cycle has no line of its own.",
    &["topics"],
);
const ANSWER: Head = head(
    "Answers that name nothing",
    "Correct the line under its key. Where a name names no topic: correct the name, or create the topic with a `[topics.<name>]` table holding a `summary`.",
    &["key", "why"],
);

const CREATED: Head = head(
    "Topics created",
    "A topic here that the store already holds under another spelling, or that nothing refers to, may be a `[topics.<name>]` table under a wrong name.",
    &["topic", "summary", "part_of", "uses", "referred to by"],
);
const EDGES: Head = head(
    "Edges added",
    "Each is in a topic's table and in no `includes` of the store.",
    &["topic", "part_of", "uses"],
);

const TOMBSTONE: Head = head(
    "Tombstones that follow no one version",
    "No command rewrites a stored version: the converter has to read this case first.",
    &["document", "version", "follows"],
);
const DROPPED: Head = head(
    "Edges dropped",
    "Each is to an ended topic, kept in every version of the store and left out of one more.",
    &["topic", "part_of", "uses"],
);
const REFILED: Head = head(
    "Facts filed under another topic",
    "Each keeps its topic in every version of the store and has the other in one more.",
    &["fact", "from", "to"],
);
const UNCLAIMED: Head = head(
    "Claims ended",
    "Each is on an ended topic and ends in one more version.",
    &["machine topic", "topic", "directory"],
);
const RENAMED: Head = head(
    "Documents renamed",
    "Each keeps its name in every version of the store and has the other in one more.",
    &["document", "from", "to"],
);
const OVERRIDDEN: Head = head(
    "Topics listed in place of tags",
    "Each list stands in every version of the document, whatever tags its versions carry, and no question asked for it.",
    &["document", "topics"],
);
const UNFILED: Head = head(
    "Entries under no topic",
    "Each is filed under no topic by the tags of its last version as the file answers them: `tags` or `topics` under the entry's table files it under one.",
    &["entry", "tags"],
);
const UNHELD: Head = head(
    "References the plan does not hold",
    "The converter planned a document that refers to one it did not plan as the new store wants it: this is a defect of the converter.",
    &["kind", "document", "field", "target", "why"],
);
const TOPIC_CASE: Head = head(
    "Names of several topics",
    "No key of the file tells topics apart that differ in case alone. Where one of them is a live topic under that name, give it another name with `worklog rename`, and the name in any case is then the topic renamed, as the store reads it. Where none is, a line of the file can name a renamed topic by the name it has now; a name a version of the store holds, or the name of a removed topic, waits for the converter to read this case.",
    &["name", "topics"],
);
const FORMER_NAME: Head = head(
    "Former names the new store refuses",
    "Under the document's table: `former_names`, the old name with a name the new store takes, or `\"\"` to leave it out.",
    &["document", "name", "why"],
);

// A control character other than a line break ends no line and has no place in one.
fn spaced(line: &str) -> String {
    let spaced = |c: char| if c.is_control() { ' ' } else { c };
    line.chars().map(spaced).collect()
}

fn cell(text: &str) -> String {
    let lines: Vec<String> = (text.lines())
        .map(|line| spaced(&line.replace('|', "\\|")))
        .collect();
    lines.join("<br>")
}

fn row(head: &'static Head, cells: Vec<String>) -> Row {
    let count = 0;
    Row { head, count, cells }
}

// A table and the stubs of its rows list the most used first.
fn weight(open: &Open) -> usize {
    match open {
        Open::Tag { carriers, .. } => carriers.len(),
        Open::Machine { versions, .. } => *versions,
        _ => 0,
    }
}

fn blocked_row(blocked: &Blocked) -> Row {
    let of = DocumentKey::to_string;
    match blocked {
        Blocked::Forked { document, heads } => row(&FORKED, vec![of(document), heads.join(", ")]),
        Blocked::Waiting { document, missing } => {
            row(&WAITING, vec![of(document), missing.join(", ")])
        }
        Blocked::Unreadable { path, reason } => {
            row(&UNREADABLE, vec![path.clone(), reason.clone()])
        }
        Blocked::Skipped { path } => row(&SKIPPED, vec![path.clone()]),
        Blocked::RenameCutShort { document, target } => {
            row(&RENAME_CUT_SHORT, vec![of(document), target.clone()])
        }
        Blocked::WriteTime {
            document,
            version,
            text,
        } => row(
            &WRITE_TIME,
            vec![of(document), version.clone(), text.clone()],
        ),
        Blocked::ForeignKeys {
            document,
            version,
            keys,
        } => row(
            &FOREIGN_KEYS,
            vec![of(document), version.clone(), keys.join(", ")],
        ),
        Blocked::RenameUnjoined {
            document,
            others,
            case,
        } => {
            let case = match case {
                Unjoined::NoTombstone => "renamed from a slug no rename of which is its parent",
                Unjoined::Met => "several renames into this slug",
                Unjoined::Parted => "renamed to several slugs",
                Unjoined::Circle => "renames that lead back to this slug",
            };
            let cells = vec![of(document), others.join(", "), case.to_owned()];
            row(&RENAME_UNJOINED, cells)
        }
        Blocked::Value {
            document,
            version,
            field,
            value,
            why,
        } => {
            let cells = [version, field, value, why].map(String::clone);
            let mut row = row(&VALUE, vec![of(document)]);
            row.cells.extend(cells);
            row
        }
        Blocked::Tombstone {
            document,
            version,
            follows,
        } => {
            let follows = if follows.is_empty() {
                "no version".to_owned()
            } else {
                follows.join(", ")
            };
            row(&TOMBSTONE, vec![of(document), version.clone(), follows])
        }
        Blocked::TopicCase { name, topics } => {
            row(&TOPIC_CASE, vec![name.clone(), topics.join(", ")])
        }
        Blocked::Unheld {
            kind,
            document,
            field,
            target,
            why,
        } => {
            let cells = [kind, document, field, target, why].map(String::clone);
            row(&UNHELD, cells.to_vec())
        }
    }
}

fn through_cell(through: &Through) -> String {
    match through {
        Through::FactTopic => "its topic".to_owned(),
        Through::Includes => "includes".to_owned(),
        Through::Tag(tag) => format!("tag {tag}"),
        Through::Touching => "touching".to_owned(),
        Through::Claim => "a claim".to_owned(),
    }
}

fn open_row(open: &Open, topics: &BTreeSet<Folded>) -> Row {
    let mut row = open_cells(open, topics);
    row.count = weight(open);
    row
}

fn names_a_topic(missing: bool, ended: bool, unread: bool) -> &'static str {
    match (missing, ended, unread) {
        (true, _, _) => "no",
        (false, true, _) => "yes, an ended one",
        (false, false, true) => "yes, though `worklog` leaves the item out as it renames it",
        (false, false, false) => "yes",
    }
}

fn machine_row(name: &str, versions: usize, documents: &[DocumentKey]) -> Row {
    let among = examples(documents).join(", ");
    let cells = vec![name.to_owned(), versions.to_string(), among];
    row(&MACHINE, cells)
}

fn open_cells(open: &Open, topics: &BTreeSet<Folded>) -> Row {
    let of = DocumentKey::to_string;
    let text = Folded::to_string;
    match open {
        Open::Conversion { versions } => row(&CONVERSION, vec![versions.to_string()]),
        Open::Tag { tag, carriers } => {
            let named = if topics.contains(tag) { "yes" } else { "" };
            let cells = vec![
                text(tag),
                carriers.len().to_string(),
                examples(carriers).join(", "),
                named.to_owned(),
            ];
            row(&TAG, cells)
        }
        Open::Include {
            topic,
            item,
            missing,
            ended,
            unread,
        } => {
            let names = names_a_topic(*missing, *ended, *unread);
            row(&INCLUDE, vec![text(topic), text(item), names.to_owned()])
        }
        Open::TopicSummary { topic } => row(&TOPIC_SUMMARY, vec![text(topic)]),
        Open::Machine {
            name,
            versions,
            documents,
        } => machine_row(name, *versions, documents),
        Open::MachineTopics { name, topics } => {
            row(&MACHINE_TOPICS, vec![name.clone(), topics.join(", ")])
        }
        Open::Claim {
            machine_topic,
            item,
        } => row(&CLAIM, vec![text(machine_topic), text(item)]),
        Open::Ended {
            document,
            idea,
            note,
            already,
        } => {
            let kind = if *idea && document.kind == Kind::Fact {
                "idea"
            } else {
                document.kind.word()
            };
            let reasons = reasons(document.kind, *idea).join(", ");
            let note = cut(first_line(note));
            let already = already.clone().unwrap_or_default();
            row(
                &ENDED,
                vec![of(document), kind.to_owned(), already, note, reasons],
            )
        }
        Open::NoTopic { document, summary } => row(&NO_TOPIC, vec![of(document), cut(summary)]),
        Open::FactTopic { fact, topic } => row(&FACT_TOPIC, vec![of(fact), topic.clone()]),
        Open::Name {
            document,
            name,
            why,
        } => row(&NAME, vec![of(document), name.clone(), why.clone()]),
        Open::FormerName {
            document,
            name,
            why,
        } => row(&FORMER_NAME, vec![of(document), name.clone(), why.clone()]),
        Open::Summary { document } => row(&SUMMARY, vec![of(document)]),
        Open::RecheckSummary { fact, touching } => {
            row(&RECHECK_SUMMARY, vec![of(fact), touching.clone()])
        }
        Open::Entry { followup, entry } => row(&ENTRY, vec![of(followup), entry.clone()]),
        Open::Touching { document, topic } => row(&TOUCHING, vec![of(document), topic.clone()]),
        Open::EndedTopic {
            document,
            through,
            topic,
        } => {
            let cells = vec![of(document), through_cell(through), topic.clone()];
            row(&ENDED_TOPIC, cells)
        }
        Open::TopicCase {
            document,
            through,
            name,
            topics,
        } => {
            let cells = vec![
                of(document),
                through_cell(through),
                name.clone(),
                topics.join(", "),
            ];
            row(&OPEN_TOPIC_CASE, cells)
        }
        Open::Cycle { topics } => row(&CYCLE, vec![topics.join(", ")]),
        Open::Answer { key, why } => row(&ANSWER, vec![key.to_string(), why.clone()]),
    }
}

fn authored_row(authored: &Authored, referred: &BTreeMap<String, usize>) -> Row {
    match authored {
        Authored::Topic {
            name,
            summary,
            part_of,
            uses,
        } => {
            let by = match referred.get(name).copied().unwrap_or(0) {
                0 => "nothing".to_owned(),
                documents => counted(documents, "document"),
            };
            let cells = vec![
                name.clone(),
                cut(summary),
                part_of.join(", "),
                uses.join(", "),
                by,
            ];
            row(&CREATED, cells)
        }
        Authored::Edges {
            topic,
            part_of,
            uses,
        } => row(
            &EDGES,
            vec![topic.clone(), part_of.join(", "), uses.join(", ")],
        ),
        Authored::Dropped {
            topic,
            part_of,
            uses,
        } => row(
            &DROPPED,
            vec![topic.clone(), part_of.join(", "), uses.join(", ")],
        ),
        Authored::Refiled { fact, from, to } => {
            row(&REFILED, vec![fact.clone(), from.clone(), to.clone()])
        }
        Authored::Unclaimed {
            machine_topic,
            topic,
            directory,
        } => row(
            &UNCLAIMED,
            vec![machine_topic.clone(), topic.clone(), directory.clone()],
        ),
        Authored::Renamed { document, from, to } => {
            row(&RENAMED, vec![document.clone(), from.clone(), to.clone()])
        }
    }
}

fn overridden_row(overridden: &Overridden) -> Row {
    let cells = vec![overridden.document.clone(), overridden.topics.join(", ")];
    row(&OVERRIDDEN, cells)
}

fn unfiled_row(unfiled: &Unfiled) -> Row {
    row(
        &UNFILED,
        vec![unfiled.entry.clone(), unfiled.tags.join(", ")],
    )
}

fn line(cells: impl Iterator<Item = String>) -> String {
    let cells: Vec<String> = cells.collect();
    format!("| {} |\n", cells.join(" | "))
}

// A table's rows are those of one variant, which `same` tells of two neighbors.
fn tables<T>(of: &[T], same: impl Fn(&T, &T) -> bool, row: impl Fn(&T) -> Row) -> String {
    let mut text = String::new();
    for variant in of.chunk_by(same) {
        let mut rows: Vec<Row> = variant.iter().map(&row).collect();
        rows.sort_by_key(|row| Reverse(row.count));
        let Some(Row { head, .. }) = rows.first() else {
            continue;
        };
        let _ = write!(text, "\n### {}\n\n{}\n\n", head.title, head.advice);
        text.push_str(&line(
            head.columns.iter().map(|column| (*column).to_owned()),
        ));
        text.push_str(&line(head.columns.iter().map(|_| "---".to_owned())));
        for row in &rows {
            text.push_str(&line(row.cells.iter().map(|text| cell(text))));
        }
    }
    text
}

fn part(title: &str, tables: &str) -> String {
    if tables.is_empty() {
        String::new()
    } else {
        format!("\n## {title}\n{tables}")
    }
}

fn in_order<T: Ord + Clone>(of: &[T]) -> Vec<T> {
    let ordered: BTreeSet<&T> = of.iter().collect();
    ordered.into_iter().cloned().collect()
}

// One tag asked for on several documents is one question with every carrier.
fn ordered(questions: &[Question]) -> (Vec<Blocked>, Vec<Open>) {
    let mut blocked = Vec::new();
    let mut open = Vec::new();
    let mut tags: BTreeMap<Folded, Vec<DocumentKey>> = BTreeMap::new();
    for question in in_order(questions) {
        match question {
            Question::Blocked(question) => blocked.push(question),
            Question::Open(Open::Tag { tag, carriers }) => {
                tags.entry(tag).or_default().extend(carriers);
            }
            Question::Open(question) => open.push(question),
        }
    }
    open.extend((tags.iter()).map(|(tag, carriers)| Open::tag(tag.as_str(), carriers)));
    open.sort();
    (blocked, open)
}

fn itself(title: &str, wrote: &Itself) -> String {
    let authored = in_order(wrote.authored);
    let same = |a: &Authored, b: &Authored| discriminant(a) == discriminant(b);
    let mut text = tables(&authored, same, |authored| {
        authored_row(authored, wrote.referred)
    });
    text.push_str(&tables(
        &in_order(wrote.overridden),
        |_, _| true,
        overridden_row,
    ));
    text.push_str(&tables(&in_order(wrote.unfiled), |_, _| true, unfiled_row));
    part(title, &text)
}

// The same bytes for the same questions, versions and keys in any order.
pub(crate) fn report(run: &Run) -> String {
    let (blocked, open) = ordered(run.questions);
    let unused = in_order(run.unused);
    let standing = Standing::counted(&blocked, &open, &unused);
    let read = format!(
        "in {}, {}",
        counted(run.read.documents, "document"),
        counted(run.read.versions, "version")
    );
    let mut text = if standing.is_clear() {
        format!("nothing is open {read}\n")
    } else {
        format!("{standing} {read}\n")
    };
    let same = |a: &Blocked, b: &Blocked| discriminant(a) == discriminant(b);
    text.push_str(&part("Blocked", &tables(&blocked, same, blocked_row)));
    let same = |a: &Open, b: &Open| discriminant(a) == discriminant(b);
    let rows = tables(&open, same, |open| open_row(open, run.topics));
    text.push_str(&part("Open", &rows));
    text.push_str(&itself("What the conversion will write itself", &run.wrote));
    if !unused.is_empty() {
        text.push_str("\n## Unused decisions\n\n");
        for key in unused {
            let _ = writeln!(text, "- {key}");
        }
    }
    let stubs = stub::with_stamp(&stub::block(&open, run), &open, run);
    if !stubs.is_empty() {
        let _ = write!(text, "\n## Stubs\n\n```toml\n{stubs}```\n");
    }
    text
}

pub(crate) fn written_itself(wrote: &Itself) -> String {
    itself("What the conversion wrote itself", wrote)
}
