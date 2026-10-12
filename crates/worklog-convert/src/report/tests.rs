use std::collections::{BTreeMap, BTreeSet};

use clap::CommandFactory;
use worklog::domain::schema::{KindOf, Reason};
use worklog_old::cli::args::Cli;

use super::{Counts, Itself, Run, Standing, ordered, stub, written_itself};
use crate::decisions::{Decisions, Unasked};
use crate::fixture::{LAMP_DRIVER, entry_key as entry, fact_key as fact, followup_key as followup};
use crate::key::{ConversionKey, DocumentField, DocumentKey, Folded, Key};
use crate::old::{Kind, Problem};
use crate::question::{
    Authored, Blocked, Open, Overridden, Question, Through, Unfiled, Unjoined, reasons,
};

struct Sets {
    held: BTreeSet<String>,
    unanswered: BTreeSet<Key>,
    topics: BTreeSet<Folded>,
    referred: BTreeMap<String, usize>,
}

impl Sets {
    fn none() -> Sets {
        Sets {
            held: BTreeSet::new(),
            unanswered: BTreeSet::new(),
            topics: BTreeSet::new(),
            referred: BTreeMap::new(),
        }
    }

    fn of(file: &Decisions) -> Sets {
        Sets {
            held: file.tables().clone(),
            unanswered: file.unanswered().clone(),
            ..Sets::none()
        }
    }

    fn run<'a>(&'a self, questions: &'a [Question]) -> Run<'a> {
        Run {
            questions,
            wrote: Itself {
                authored: &[],
                overridden: &[],
                referred: &self.referred,
                unfiled: &[],
            },
            unused: &[],
            held: &self.held,
            unanswered: &self.unanswered,
            topics: &self.topics,
            read: read(1, 1),
        }
    }
}

fn report(
    questions: &[Question],
    authored: &[Authored],
    overridden: &[Overridden],
    unused: &[Key],
    held: &BTreeSet<String>,
    read: Counts,
) -> String {
    let sets = Sets::none();
    let mut run = sets.run(questions);
    run.wrote.authored = authored;
    run.wrote.overridden = overridden;
    (run.unused, run.held, run.read) = (unused, held, read);
    super::report(&run)
}

fn stubs(questions: &[Question], held: &BTreeSet<String>) -> String {
    let sets = Sets::none();
    let mut run = sets.run(questions);
    run.held = held;
    stub::block(&ordered(questions).1, &run)
}

fn text(text: &str) -> String {
    text.to_owned()
}

fn read(documents: usize, versions: usize) -> Counts {
    Counts {
        documents,
        versions,
    }
}

macro_rules! samples {
    ($of:ident, $every:ident, $named:ident, $test:ident: $($variant:ident => $sample:expr,)*) => {
        fn $every() -> Vec<$of> {
            vec![$($sample),*]
        }

        fn $named(sample: &$of) -> &'static str {
            match sample {
                $($of::$variant { .. } => stringify!($variant),)*
            }
        }

        #[test]
        fn $test() {
            let names: Vec<&str> = $every().iter().map($named).collect();
            assert_eq!(names, [$(stringify!($variant)),*]);
        }
    };
}

samples! {
    Blocked, blocked_samples, blocked_variant, each_blocked_sample_is_of_its_variant:
    Forked => Blocked::Forked {
        document: entry(LAMP_DRIVER),
        heads: vec![text("aa11"), text("bb22")],
    },
    Waiting => Blocked::Waiting {
        document: fact("lantern/relay-pin"),
        missing: vec![text("cc33")],
    },
    Unreadable => Blocked::Unreadable {
        path: text("topic/lantern/dd44.md"),
        reason: text("the name is not the hash"),
    },
    Skipped => Blocked::Skipped {
        path: text("notes.txt"),
    },
    RenameCutShort => Blocked::RenameCutShort {
        document: fact("lantern/relay"),
        target: text("lantern/relay-pin"),
    },
    WriteTime => Blocked::WriteTime {
        document: followup("2026-09-04-port"),
        version: text("ee55"),
        text: text("yesterday"),
    },
    ForeignKeys => Blocked::ForeignKeys {
        document: DocumentKey::new(Kind::Topic, "atlas"),
        version: text("ff66"),
        keys: vec![text("signed"), text("sealed")],
    },
    RenameUnjoined => Blocked::RenameUnjoined {
        document: DocumentKey::new(Kind::Topic, "phone"),
        others: vec![text("handset"), text("mobile")],
        case: Unjoined::Parted,
    },
    Value => Blocked::Value {
        document: fact("lantern/relay-pin"),
        version: text("ab77"),
        field: text("verified"),
        value: text("2026-02-31"),
        why: text("is not a day"),
    },
    Tombstone => Blocked::Tombstone {
        document: DocumentKey::new(Kind::Topic, "beacon"),
        version: text("cd88"),
        follows: vec![text("aa11"), text("bb22")],
    },
    TopicCase => Blocked::TopicCase {
        name: text("phone"),
        topics: vec![text("PHONE"), text("Phone")],
    },
    Unheld => Blocked::Unheld {
        kind: text("fact"),
        document: text("00000000000000000000000000000001"),
        field: text("topic"),
        target: text("00000000000000000000000000000002"),
        why: text("is not planned"),
    },
}

samples! {
    Open, every_open, open_variant, each_open_sample_is_of_its_variant:
    Conversion => Open::Conversion { versions: 2 },
    Tag => Open::tag("relay", &[entry(LAMP_DRIVER)]),
    Include => Open::Include {
        topic: Folded::new("lantern"),
        item: Folded::new("atlas"),
        missing: false,
        ended: false,
        unread: false,
    },
    TopicSummary => Open::TopicSummary {
        topic: Folded::new("lantern-firmware"),
    },
    Machine => Open::Machine {
        name: text("desk-old"),
        versions: 12,
        documents: Vec::new(),
    },
    MachineTopics => Open::MachineTopics {
        name: text("phone"),
        topics: vec![text("atlas"), text("phone")],
    },
    Claim => Open::Claim {
        machine_topic: Folded::new("desk"),
        item: Folded::new("phone"),
    },
    Ended => Open::ended(
        &fact("lantern/relay-pin"),
        false,
        "Moved to the atlas\nwith the wiring",
    ),
    NoTopic => Open::NoTopic {
        document: followup("2026-09-04-port"),
        summary: text("Port the driver"),
    },
    FactTopic => Open::FactTopic {
        fact: fact("lantrn/dimmer"),
        topic: text("lantrn"),
    },
    Name => Open::Name {
        document: DocumentKey::new(Kind::Topic, "Lantern_Old"),
        name: text("Lantern_Old"),
        why: text("holds an uppercase letter"),
    },
    FormerName => Open::FormerName {
        document: DocumentKey::new(Kind::Topic, "Lantern_Old"),
        name: text("Lamp.1"),
        why: text("holds `.`, and a name is lowercase letters, digits and hyphens"),
    },
    Summary => Open::Summary {
        document: entry(LAMP_DRIVER),
    },
    RecheckSummary => Open::RecheckSummary {
        fact: fact("lantern/relay-pin"),
        touching: text("lantern"),
    },
    Entry => Open::Entry {
        followup: followup("2026-09-04-port"),
        entry: text("2026-09/2026-09-01-gone"),
    },
    Touching => Open::Touching {
        document: followup("2026-09-04-port"),
        topic: text("desk"),
    },
    EndedTopic => Open::EndedTopic {
        document: entry(LAMP_DRIVER),
        through: Through::Tag(Folded::new("relay")),
        topic: text("lantern"),
    },
    TopicCase => Open::TopicCase {
        document: fact("phone/battery"),
        through: Through::FactTopic,
        name: text("phone"),
        topics: vec![text("PHONE"), text("Phone")],
    },
    Cycle => Open::Cycle {
        topics: vec![text("lantern"), text("atlas")],
    },
    Answer => Open::Answer {
        key: Key::tag("firmware"),
        why: text("names no topic: lantern-firmwar"),
    },
}

fn every_blocked() -> Vec<Question> {
    blocked_samples().into_iter().map(Question::from).collect()
}

fn questions(open: &[Open]) -> Vec<Question> {
    open.iter().cloned().map(Question::from).collect()
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

fn filled(open: &Open, stub: &str) -> Option<String> {
    let with = |value: &str| Some(stub.replace("\"?\"", value));
    match open {
        Open::Conversion { .. } => Some(
            stub.replace("written = \"?\"", "written = 2026-10-20T09:30:00+01:00")
                .replace("\"?\"", "\"desk\""),
        ),
        Open::Tag { .. } | Open::Entry { .. } => with("\"\""),
        Open::Include { .. } | Open::EndedTopic { .. } => with("\"atlas\""),
        Open::Claim { .. } => with("\"phone\""),
        Open::Machine { .. } | Open::MachineTopics { .. } => with("\"desk\""),
        Open::Ended { .. } => with("\"moved\""),
        Open::NoTopic { .. }
        | Open::FactTopic { .. }
        | Open::Touching { .. }
        | Open::TopicCase { .. } => with("\"lantern\""),
        Open::TopicSummary { .. } | Open::Summary { .. } | Open::RecheckSummary { .. } => {
            with("\"One line\"")
        }
        Open::Name { .. } | Open::FormerName { .. } => with("\"lantern-old\""),
        Open::Cycle { .. } | Open::Answer { .. } => None,
    }
}

fn answered(decisions: &Decisions, open: &Open) -> bool {
    let read = decisions.answer(open);
    match open {
        Open::Conversion { .. } => [ConversionKey::Written, ConversionKey::Machine]
            .into_iter()
            .all(|key| !decisions.unasked(Unasked::Conversion(key)).is_empty()),
        Open::Ended { document, idea, .. } => {
            read.is_some_and(|read| read.one_of(&reasons(document.kind, *idea)).is_ok())
        }
        Open::NoTopic { .. } => read
            .and_then(|read| read.topics())
            .is_some_and(|topics| !topics.is_empty()),
        Open::FactTopic { .. } => read.is_some_and(|read| read.one_topic().is_ok()),
        _ => read.is_some(),
    }
}

const EVERY_STUB: &str = r#"# A table is written once: move a line under the table the file already holds.

[conversion]
written = "?"  # the write time of the 2 versions the conversion authors: a date and time with an offset, not quoted
machine = "?"  # the machine name they are stamped with

[tags]
relay = "?"  # 1 document: entry/2026-09/2026-09-04-lamp-driver; "" drops the tag

[topics.desk]
unclaim = [
  "?",  # write phone here: it is claimed and names no topic; or a [topics.phone] table creates it
]

[topics.lantern]
part_of = [
  "?",  # write atlas here, or in uses, or in drop
]
uses = []
drop = []

[topics.lantern-firmware]
summary = "?"  # one line for the topic the file creates

[machines]
desk-old = "?"  # 12 versions: the machine's topic
phone = "?"  # the machine of atlas, phone: one of them

[documents."entry/2026-09/2026-09-04-lamp-driver"]
tags.relay = "?"  # lantern is ended: another topic for this document, or "" to drop the tag
summary = "?"  # one line

[documents."fact/lantern/relay-pin"]
recheck_summary = "?"  # one line for the followup that rechecks it, touching lantern
ended = "?"  # one of: false, moved, superseded; note: Moved to the atlas

[documents."fact/lantrn/dimmer"]
topics = ["?"]  # lantrn names no topic: the one topic of the fact

[documents."fact/phone/battery"]
topics = ["?"]  # phone is PHONE, Phone in another case: the one topic of the fact, spelled as the store does

[documents."topic/Lantern_Old"]
name = "?"  # Lantern_Old: holds an uppercase letter
former_names."Lamp.1" = "?"  # Lamp.1: holds `.`, and a name is lowercase letters, digits and hyphens; "" leaves it out

[documents."followup/2026-09-04-port"]
topics = ["?"]  # Port the driver
entry = "?"  # 2026-09/2026-09-01-gone names no entry; "" for none
touching = "?"  # desk names no topic that is not ended; "" for no trigger
"#;

#[test]
fn nothing_open_prints_one_line() {
    assert_eq!(
        report(&[], &[], &[], &[], &none(), read(8, 14)),
        "nothing is open in 8 documents, 14 versions\n"
    );
    assert_eq!(
        report(&[], &[], &[], &[], &none(), read(1, 1)),
        "nothing is open in 1 document, 1 version\n"
    );
    assert_eq!(stubs(&[], &none()), "");
}

#[test]
fn a_small_report_is_this_text() {
    let carriers = [
        followup("2026-09-04-c"),
        entry("2026-09/2026-09-05-b"),
        entry("2026-09/2026-09-04-a"),
        followup("2026-09-06-d"),
    ];
    let questions = [
        Open::Summary {
            document: fact("lantern/relay-pin"),
        }
        .into(),
        Open::tag("firmware", &carriers[2..3]).into(),
        Open::tag("Relay", &carriers[..2]).into(),
        Open::tag("relay", &carriers[1..]).into(),
        Open::Answer {
            key: Key::document(&fact("lantern/relay-pin"), DocumentField::EndedBy),
            why: text("names no document"),
        }
        .into(),
        Problem::Skipped {
            path: text("notes.txt"),
        }
        .into(),
    ];
    let authored = [
        Authored::Edges {
            topic: text("lantern"),
            part_of: vec![],
            uses: vec![text("desk"), text("phone")],
        },
        Authored::Topic {
            name: text("lantrn"),
            summary: text("The lantern"),
            part_of: vec![text("atlas")],
            uses: vec![],
        },
    ];
    let unused = [Key::tag("rust")];

    let expected = r#"1 blocked, 4 open, 1 decision unused in 8 documents, 14 versions

## Blocked

### Not read

Move each path out of the store: no command reads it.

| path |
| --- |
| notes.txt |

## Open

### Tags with no answer

Under `[tags]`, or under a document's `tags`: a topic name, or `""` to drop the tag.

| tag | documents | among them | a topic of this name |
| --- | --- | --- | --- |
| relay | 4 | entry/2026-09/2026-09-04-a, entry/2026-09/2026-09-05-b, followup/2026-09-04-c |  |
| firmware | 1 | entry/2026-09/2026-09-04-a |  |

### Summaries that are not one line

Under the document's table: `summary`, one line.

| document |
| --- |
| fact/lantern/relay-pin |

### Answers that name nothing

Correct the line under its key. Where a name names no topic: correct the name, or create the topic with a `[topics.<name>]` table holding a `summary`.

| key | why |
| --- | --- |
| documents."fact/lantern/relay-pin".ended_by | names no document |

## What the conversion will write itself

### Topics created

A topic here that the store already holds under another spelling, or that nothing refers to, may be a `[topics.<name>]` table under a wrong name.

| topic | summary | part_of | uses | referred to by |
| --- | --- | --- | --- | --- |
| lantrn | The lantern | atlas |  | nothing |

### Edges added

Each is in a topic's table and in no `includes` of the store.

| topic | part_of | uses |
| --- | --- | --- |
| lantern |  | desk, phone |

## Unused decisions

- tags.rust

## Stubs

```toml
# A table is written once: move a line under the table the file already holds.

# [conversion] is needed once an answer makes the conversion write a version of its own:
# [conversion]
# written = "?"  # a date and time with an offset, not quoted
# machine = "?"  # the machine name those versions are stamped with

[tags]
relay = "?"  # 4 documents: entry/2026-09/2026-09-04-a, entry/2026-09/2026-09-05-b, followup/2026-09-04-c, …; "" drops the tag
firmware = "?"  # 1 document: entry/2026-09/2026-09-04-a; "" drops the tag

[documents."fact/lantern/relay-pin"]
summary = "?"  # one line
```
"#;
    assert_eq!(
        report(&questions, &authored, &[], &unused, &none(), read(8, 14)),
        expected
    );
}

#[test]
fn each_thing_the_conversion_writes_has_its_table() {
    let names = |names: &[&str]| names.iter().map(|name| text(name)).collect::<Vec<_>>();
    let authored = [
        Authored::Unclaimed {
            machine_topic: text("desk"),
            topic: text("phone"),
            directory: String::new(),
        },
        Authored::Refiled {
            fact: text("phone/battery"),
            from: text("phone"),
            to: text("atlas"),
        },
        Authored::Dropped {
            topic: text("lantern"),
            part_of: names(&["phone"]),
            uses: names(&["handset", "beacon"]),
        },
        Authored::Unclaimed {
            machine_topic: text("desk"),
            topic: text("phone"),
            directory: text("~/projects/phone"),
        },
        Authored::Renamed {
            document: text("fact/lamp/relay"),
            from: text("relay"),
            to: text("relay-two"),
        },
    ];

    assert_eq!(
        report(&[], &authored, &[], &[], &none(), read(2, 3)),
        "nothing is open in 2 documents, 3 versions\n\n\
         ## What the conversion will write itself\n\n\
         ### Edges dropped\n\n\
         Each is to an ended topic, kept in every version of the store and left out of one \
         more.\n\n\
         | topic | part_of | uses |\n| --- | --- | --- |\n| lantern | phone | handset, beacon |\n\n\
         ### Facts filed under another topic\n\n\
         Each keeps its topic in every version of the store and has the other in one more.\n\n\
         | fact | from | to |\n| --- | --- | --- |\n| phone/battery | phone | atlas |\n\n\
         ### Claims ended\n\n\
         Each is on an ended topic and ends in one more version.\n\n\
         | machine topic | topic | directory |\n| --- | --- | --- |\n\
         | desk | phone |  |\n| desk | phone | ~/projects/phone |\n\n\
         ### Documents renamed\n\n\
         Each keeps its name in every version of the store and has the other in one more.\n\n\
         | document | from | to |\n| --- | --- | --- |\n\
         | fact/lamp/relay | relay | relay-two |\n"
    );
}

#[test]
fn a_list_of_topics_nothing_asked_for_has_its_table_under_what_the_conversion_writes() {
    let overridden = [
        Overridden {
            document: text("followup/2026-09-01-port"),
            topics: vec![text("lantern"), text("atlas")],
        },
        Overridden {
            document: text("entry/2026-09/2026-09-04-lamp-driver"),
            topics: vec![],
        },
    ];
    let table = "### Topics listed in place of tags\n\n\
         Each list stands in every version of the document, whatever tags its versions carry, \
         and no question asked for it.\n\n\
         | document | topics |\n| --- | --- |\n\
         | entry/2026-09/2026-09-04-lamp-driver |  |\n\
         | followup/2026-09-01-port | lantern, atlas |\n";

    assert_eq!(
        report(&[], &[], &overridden, &[], &none(), read(2, 3)),
        format!(
            "nothing is open in 2 documents, 3 versions\n\n\
             ## What the conversion will write itself\n\n{table}"
        )
    );
    let authored = [Authored::Edges {
        topic: text("lantern"),
        part_of: vec![text("atlas")],
        uses: vec![],
    }];
    assert_eq!(
        report(&[], &authored, &overridden, &[], &none(), read(2, 3)),
        format!(
            "nothing is open in 2 documents, 3 versions\n\n\
             ## What the conversion will write itself\n\n\
             ### Edges added\n\n\
             Each is in a topic's table and in no `includes` of the store.\n\n\
             | topic | part_of | uses |\n| --- | --- | --- |\n| lantern | atlas |  |\n\n{table}"
        )
    );
}

#[test]
fn every_blocked_variant_has_its_table() {
    let expected = r"12 blocked, 0 open, 0 decisions unused in 5 documents, 9 versions

## Blocked

### Forked

Close each fork with `worklog resolve` and `worklog save`, run with a `worklog` 0.4 binary.

| document | heads |
| --- | --- |
| entry/2026-09/2026-09-04-lamp-driver | aa11, bb22 |

### Waiting for versions

Sync with the machine that holds the missing versions, until `worklog check` passes, run with a `worklog` 0.4 binary.

| document | missing |
| --- | --- |
| fact/lantern/relay-pin | cc33 |

### Unreadable

Bring each file back from a machine that reads it, until `worklog check` passes, run with a `worklog` 0.4 binary.

| path | reason |
| --- | --- |
| topic/lantern/dd44.md | the name is not the hash |

### Not read

Move each path out of the store: no command reads it.

| path |
| --- |
| notes.txt |

### Renames cut short

Sync with the machine that renamed the document, which holds the versions under the new slug.

| document | renamed to |
| --- | --- |
| fact/lantern/relay | lantern/relay-pin |

### Write times not read

No command rewrites a stored version: the converter has to read this form first.

| document | version | written |
| --- | --- | --- |
| followup/2026-09-04-port | ee55 | yesterday |

### Keys outside the grammar

No command rewrites a stored version: the converter has to place these keys first.

| document | version | keys |
| --- | --- | --- |
| topic/atlas | ff66 | signed, sealed |

### Renames not joined

No command rewrites a stored rename: the converter has to join this case first.

| document | other slugs | case |
| --- | --- | --- |
| topic/phone | handset, mobile | renamed to several slugs |

### Stored values the conversion cannot place

No command rewrites a stored version: the converter has to place each of these first.

| document | version | field | value | why |
| --- | --- | --- | --- | --- |
| fact/lantern/relay-pin | ab77 | verified | 2026-02-31 | is not a day |

### Tombstones that follow no one version

No command rewrites a stored version: the converter has to read this case first.

| document | version | follows |
| --- | --- | --- |
| topic/beacon | cd88 | aa11, bb22 |

### Names of several topics

No key of the file tells topics apart that differ in case alone. Where one of them is a live topic under that name, give it another name with `worklog rename`, run with a `worklog` 0.4 binary, and the name in any case is then the topic renamed, as the store reads it. Where none is, a line of the file can name a renamed topic by the name it has now; a name a version of the store holds, or the name of a removed topic, waits for the converter to read this case.

| name | topics |
| --- | --- |
| phone | PHONE, Phone |

### References the plan does not hold

The converter planned a document that refers to one it did not plan as the new store wants it: this is a defect of the converter.

| kind | document | field | target | why |
| --- | --- | --- | --- | --- |
| fact | 00000000000000000000000000000001 | topic | 00000000000000000000000000000002 | is not planned |
";
    assert_eq!(
        report(&every_blocked(), &[], &[], &[], &none(), read(5, 9)),
        expected
    );
}

#[test]
fn the_commands_the_blocked_tables_name_are_commands_of_the_tool() {
    let printed = report(&every_blocked(), &[], &[], &[], &none(), read(5, 9));
    let tool = Cli::command();

    let named: BTreeSet<&str> = (printed.split("`worklog ").skip(1))
        .map(|rest| rest.split('`').next().unwrap_or(""))
        .collect();

    assert_eq!(
        named,
        BTreeSet::from(["check", "rename", "resolve", "save"])
    );
    for name in named {
        assert!(tool.find_subcommand(name).is_some(), "{name}");
    }
}

const EVERY_OPEN_TABLE: &str = r#"## Open

### The conversion's own stamp

Under `[conversion]`: `written`, a date and time with an offset, and `machine`, a machine name.

| versions the conversion authors |
| --- |
| 2 |

### Tags with no answer

Under `[tags]`, or under a document's `tags`: a topic name, or `""` to drop the tag.

| tag | documents | among them | a topic of this name |
| --- | --- | --- | --- |
| relay | 1 | entry/2026-09/2026-09-04-lamp-driver |  |

### Included topics not placed

Under `[topics.<topic>]`: the item in `part_of`, `uses` or `drop`.

| topic | item | names a topic |
| --- | --- | --- |
| lantern | atlas | yes |

### Topics to create with no summary

Under `[topics.<topic>]`: `summary`, one line; a topic not meant to be created is a table under a wrong name.

| topic |
| --- |
| lantern-firmware |

### Machines with no topic

Under `[machines]`: the name of the machine's topic; or `machine` under a document's table, for that document alone.

| machine | versions | among them |
| --- | --- | --- |
| desk-old | 12 |  |

### Machines of several topics

Under `[machines]`: the name of the one topic that is the machine.

| machine | topics |
| --- | --- |
| phone | atlas, phone |

### Claims on no topic

Under `[topics.<machine topic>]`: the item in `unclaim`; or a `[topics.<item>]` table that creates the topic.

| machine topic | item |
| --- | --- |
| desk | phone |

### Ended documents with no reason

Under the document's table: `ended`, one of the reasons, and `ended_by` where a document replaced it.

| document | kind | already | note | reasons |
| --- | --- | --- | --- | --- |
| fact/lantern/relay-pin | fact |  | Moved to the atlas | false, moved, superseded |

### Followups with no topic

Under the document's table: `topics`, the names of the topics it is filed under.

| document | summary |
| --- | --- |
| followup/2026-09-04-port | Port the driver |

### Facts under no topic

Under the fact's table: `topics`, the name of its one topic.

| fact | topic |
| --- | --- |
| fact/lantrn/dimmer | lantrn |

### Names the new store refuses

Under the document's table: `name`.

| document | name | why |
| --- | --- | --- |
| topic/Lantern_Old | Lantern_Old | holds an uppercase letter |

### Former names the new store refuses

Under the document's table: `former_names`, the old name with a name the new store takes, or `""` to leave it out.

| document | name | why |
| --- | --- | --- |
| topic/Lantern_Old | Lamp.1 | holds `.`, and a name is lowercase letters, digits and hyphens |

### Summaries that are not one line

Under the document's table: `summary`, one line.

| document |
| --- |
| entry/2026-09/2026-09-04-lamp-driver |

### Rechecks with no summary

Under the fact's table: `recheck_summary`, one line for the followup that rechecks it.

| fact | touching |
| --- | --- |
| fact/lantern/relay-pin | lantern |

### Followups of no entry

Under the followup's table: `entry`, an entry's slug, or `""` for none.

| followup | entry |
| --- | --- |
| followup/2026-09-04-port | 2026-09/2026-09-01-gone |

### Rechecks touching no topic

Under the table of the followup, or of the fact a recheck is on: `touching`, a topic's name, or `""` for no trigger.

| document | touching |
| --- | --- |
| followup/2026-09-04-port | desk |

### Ended topics referred to

Under the document's table: `topics` for a fact, `tags` for a tag, `touching` for a recheck; under the topic's table, `drop` for an `includes` item and `unclaim` for a claim.

| document | through | topic |
| --- | --- | --- |
| entry/2026-09/2026-09-04-lamp-driver | tag relay | lantern |

### Names several topics answer to

Under the document's table, name the one meant as the store spells it: `topics` for a fact, `touching` for a recheck.

| document | through | name | topics |
| --- | --- | --- | --- |
| fact/phone/battery | its topic | phone | PHONE, Phone |

### Topic cycles

Change a placement under one of the topics: a cycle has no line of its own.

| topics |
| --- |
| lantern, atlas |

### Answers that name nothing

Correct the line under its key. Where a name names no topic: correct the name, or create the topic with a `[topics.<name>]` table holding a `summary`.

| key | why |
| --- | --- |
| tags.firmware | names no topic: lantern-firmwar |

## Stubs

"#;

#[test]
fn every_open_variant_has_its_table() {
    let printed = report(
        &questions(&every_open()),
        &[],
        &[],
        &[],
        &none(),
        read(5, 9),
    );

    let opens = "0 blocked, 20 open, 0 decisions unused in 5 documents, 9 versions\n\n";
    assert_eq!(
        printed,
        format!("{opens}{EVERY_OPEN_TABLE}```toml\n{EVERY_STUB}```\n")
    );
}

#[test]
fn a_stub_as_printed_answers_nothing_and_answers_its_question_once_filled() {
    for open in every_open() {
        let stub = stubs(&[open.clone().into()], &none());
        let Some(filled) = filled(&open, &stub) else {
            assert_eq!(stub, "", "{open:?}");
            assert!(open.offers().is_empty(), "{open:?}");
            continue;
        };

        let pasted = Decisions::parse(&stub).unwrap_or_else(|refused| panic!("{refused:?}"));
        let keys: BTreeSet<String> = (open.offers().iter())
            .map(|offer| {
                format!(
                    "{}.{}",
                    offer.key.table(),
                    offer.key.line().unwrap_or_default()
                )
            })
            .collect();
        let unanswered: BTreeSet<String> = (pasted.unanswered().iter())
            .map(|key| format!("{}.{}", key.table(), key.line().unwrap_or_default()))
            .collect();
        assert_eq!(unanswered, keys, "{stub}");
        assert!(!answered(&pasted, &open), "{stub}");
        assert_eq!(pasted.unused(), [], "{stub}");

        let decisions =
            Decisions::parse(&filled).unwrap_or_else(|refused| panic!("{refused:?}\n{filled}"));
        assert!(
            answered(&decisions, &open),
            "{open:?} is not answered by\n{filled}"
        );
        assert_eq!(decisions.unused(), [], "{filled}");
    }
}

#[test]
fn every_stub_together_reads_as_a_file_that_answers_nothing() {
    let pasted = Decisions::parse(EVERY_STUB).expect("the stubs as printed read");

    assert_eq!(pasted.unanswered().len(), 21, "{:?}", pasted.unanswered());
    assert!(pasted.unused().iter().all(Key::is_table));
    for open in every_open() {
        assert!(!answered(&pasted, &open), "{open:?}");
    }
}

#[test]
fn a_placeholder_the_file_holds_is_listed_apart_from_the_lines_to_add() {
    let file = "[tags]\nrust = \"\"\nrelay = \"?\"\n\n\
                [topics.lantern]\npart_of = [\"?\"]\nuses = []\ndrop = []\n\n\
                [documents.\"entry/2026-09/2026-09-04-lamp-driver\"]\ntags.relay = \"?\"\n";
    let held = Decisions::parse(file).expect("the file reads");
    let asked = questions(&[
        Open::tag("relay", &[entry(LAMP_DRIVER)]),
        Open::tag("maps", &[entry(LAMP_DRIVER)]),
        Open::Include {
            topic: Folded::new("lantern"),
            item: Folded::new("atlas"),
            missing: false,
            ended: false,
            unread: false,
        },
        Open::Summary {
            document: entry(LAMP_DRIVER),
        },
        Open::EndedTopic {
            document: entry(LAMP_DRIVER),
            through: Through::Tag(Folded::new("relay")),
            topic: text("lantern"),
        },
    ]);
    let sets = Sets::of(&held);
    let printed = stub::block(&ordered(&asked).1, &sets.run(&asked));

    assert_eq!(
        printed,
        r#"# A table is written once: move a line under the table the file already holds.

# add under the existing [tags]:
# maps = "?"  # 1 document: entry/2026-09/2026-09-04-lamp-driver; "" drops the tag

# in the file under [tags], not answered:
# relay = "?"  # 1 document: entry/2026-09/2026-09-04-lamp-driver; "" drops the tag

# in the file under [topics.lantern], not answered:
# part_of = [
#   "?",  # write atlas here, or in uses, or in drop
# ]
# uses = []
# drop = []

# add under the existing [documents."entry/2026-09/2026-09-04-lamp-driver"]:
# summary = "?"  # one line

# in the file under [documents."entry/2026-09/2026-09-04-lamp-driver"], not answered:
# tags.relay = "?"  # lantern is ended: another topic for this document, or "" to drop the tag
"#
    );
}

#[test]
fn the_stamp_of_the_conversion_is_a_comment_until_it_is_asked_for() {
    let note = "```toml\n\
        # A table is written once: move a line under the table the file already holds.\n\n\
        # [conversion] is needed once an answer makes the conversion write a version of its \
        own:\n\
        # [conversion]\n\
        # written = \"?\"  # a date and time with an offset, not quoted\n\
        # machine = \"?\"  # the machine name those versions are stamped with\n\n\
        [tags]\n";
    let tag = Open::tag("relay", &[entry(LAMP_DRIVER)]);
    let asked = questions(std::slice::from_ref(&tag));
    let printed = report(&asked, &[], &[], &[], &none(), read(1, 1));
    assert!(printed.contains(note), "{printed}");

    let live = questions(&[tag, Open::Conversion { versions: 2 }]);
    let printed = report(&live, &[], &[], &[], &none(), read(1, 1));
    assert!(!printed.contains("is needed once"), "{printed}");
    assert!(
        printed.contains("\n[conversion]\nwritten = \"?\"  # the write time of the 2 versions"),
        "{printed}"
    );

    let stamp = "[conversion]\nwritten = 2026-10-20T09:30:00+01:00\nmachine = \"desk\"\n";
    let held = Decisions::parse(stamp).expect("the file reads");
    let printed = report(&asked, &[], &[], &[], held.tables(), read(1, 1));
    assert!(!printed.contains("[conversion]"), "{printed}");

    let nothing = report(&[], &[], &[], &[], &none(), read(1, 1));
    assert_eq!(nothing, "nothing is open in 1 document, 1 version\n");
}

#[test]
fn a_tag_that_is_the_name_of_a_topic_says_so() {
    let asked = questions(&[
        Open::tag("relay", &[entry(LAMP_DRIVER)]),
        Open::tag("maps", &[entry(LAMP_DRIVER)]),
    ]);
    let mut sets = Sets::none();
    sets.topics.insert(Folded::new("Relay"));
    let printed = super::report(&sets.run(&asked));

    assert!(
        printed.contains(
            "| tag | documents | among them | a topic of this name |\n\
             | --- | --- | --- | --- |\n\
             | maps | 1 | entry/2026-09/2026-09-04-lamp-driver |  |\n\
             | relay | 1 | entry/2026-09/2026-09-04-lamp-driver | yes |\n"
        ),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\nmaps = \"?\"  # 1 document: entry/2026-09/2026-09-04-lamp-driver; \"\" drops the \
             tag\n\
             relay = \"?\"  # 1 document: entry/2026-09/2026-09-04-lamp-driver; \"\" drops the \
             tag; a topic of this name exists\n"
        ),
        "{printed}"
    );
}

#[test]
fn what_the_conversion_writes_names_the_topics_nothing_refers_to_and_the_entries_under_none() {
    let created = |name: &str| Authored::Topic {
        name: text(name),
        summary: text("One line"),
        part_of: vec![text("lantern")],
        uses: Vec::new(),
    };
    let authored = [created("firmware"), created("lantren")];
    let unfiled = [
        Unfiled {
            entry: text("entry/2026-09/2026-09-05-b"),
            tags: vec![text("rust")],
        },
        Unfiled {
            entry: text("entry/2026-09/2026-09-04-a"),
            tags: vec![text("rust"), text("optics")],
        },
    ];
    let mut sets = Sets::none();
    sets.referred.insert(text("firmware"), 1);
    let mut run = sets.run(&[]);
    run.wrote.authored = &authored;
    run.wrote.unfiled = &unfiled;
    let tables = "\n\
        ### Topics created\n\n\
        A topic here that the store already holds under another spelling, or that nothing \
        refers to, may be a `[topics.<name>]` table under a wrong name.\n\n\
        | topic | summary | part_of | uses | referred to by |\n\
        | --- | --- | --- | --- | --- |\n\
        | firmware | One line | lantern |  | 1 document |\n\
        | lantren | One line | lantern |  | nothing |\n\n\
        ### Entries under no topic\n\n\
        Each is filed under no topic by the tags of its last version as the file answers them: \
        `tags` or `topics` under the entry's table files it under one.\n\n\
        | entry | tags |\n\
        | --- | --- |\n\
        | entry/2026-09/2026-09-04-a | rust, optics |\n\
        | entry/2026-09/2026-09-05-b | rust |\n";

    assert_eq!(
        super::report(&run),
        format!(
            "nothing is open in 1 document, 1 version\n\n\
             ## What the conversion will write itself\n{tables}"
        )
    );
    assert_eq!(
        written_itself(&run.wrote),
        format!("\n## What the conversion wrote itself\n{tables}")
    );
    assert_eq!(written_itself(&sets.run(&[]).wrote), "");
}

#[test]
fn what_stands_is_counted_once_for_the_report_and_for_a_refusal() {
    let lamp = [entry(LAMP_DRIVER)];
    let other = [fact("lantern/relay-pin")];
    let mut asked = every_blocked();
    asked.extend(questions(&[
        Open::tag("relay", &lamp),
        Open::tag("Relay", &other),
        Open::Summary {
            document: entry(LAMP_DRIVER),
        },
    ]));
    let unused = [Key::tag("rust"), Key::tag("rust"), Key::tag("maps")];

    let standing = Standing::of(&asked, &unused);
    assert_eq!(
        standing,
        Standing {
            blocked: 12,
            open: 2,
            unused: 2
        }
    );
    assert!(!standing.is_clear());
    assert_eq!(
        standing.to_string(),
        "12 blocked, 2 open, 2 decisions unused"
    );
    let printed = report(&asked, &[], &[], &unused, &none(), read(5, 9));
    assert!(
        printed.starts_with(&format!("{standing} in 5 documents, 9 versions\n")),
        "{printed}"
    );
    let clear = Standing::of(&[], &[]);
    assert!(clear.is_clear());
    assert_eq!(clear.to_string(), "0 blocked, 0 open, 0 decisions unused");
}

#[test]
fn each_open_item_of_a_topic_has_its_own_line_in_the_stub() {
    let include = |item: &str, missing, ended| {
        Question::from(Open::Include {
            topic: Folded::new("lantern"),
            item: Folded::new(item),
            missing,
            ended,
            unread: false,
        })
    };
    let questions = [
        include("phone", true, false),
        include("atlas", false, false),
        include("handset", false, true),
        Open::Claim {
            machine_topic: Folded::new("lantern"),
            item: Folded::new("desk"),
        }
        .into(),
    ];

    assert_eq!(
        stubs(&questions, &none()),
        "# A table is written once: move a line under the table the file already holds.\n\n\
         [topics.lantern]\n\
         part_of = [\n\
         \x20 \"?\",  # write atlas here, or in uses, or in drop\n\
         \x20 \"?\",  # write handset here, or in uses, or in drop: it is ended. In drop alone, \
         the edge is left out of every converted version; in part_of or uses and in drop as \
         well, the edge stays in every converted version and one version the conversion writes \
         removes it\n\
         \x20 \"?\",  # write phone here, or in uses, or in drop: it names no topic\n\
         ]\n\
         uses = []\n\
         drop = []\n\
         unclaim = [\n\
         \x20 \"?\",  # write desk here: it is claimed and names no topic; or a [topics.desk] \
         table creates it\n\
         ]\n"
    );
    let table = report(&questions, &[], &[], &[], &none(), read(1, 1));
    assert!(
        table.contains(
            "| topic | item | names a topic |\n| --- | --- | --- |\n\
             | lantern | atlas | yes |\n| lantern | handset | yes, an ended one |\n\
             | lantern | phone | no |\n"
        ),
        "{table}"
    );
}

#[test]
fn an_item_the_store_leaves_out_as_it_renames_says_so_in_its_row_and_its_line() {
    let asked = questions(&[Open::Include {
        topic: Folded::new("lantern"),
        item: Folded::new("atlas"),
        missing: false,
        ended: false,
        unread: true,
    }]);
    let printed = super::report(&Sets::none().run(&asked));

    assert!(
        printed.contains(
            "| topic | item | names a topic |\n\
             | --- | --- | --- |\n\
             | lantern | atlas | yes, though `worklog` 0.4 leaves the item out as it renames it |\n"
        ),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\n  \"?\",  # write atlas here, or in uses, or in drop: `worklog` 0.4 does not act \
             on this item, which it leaves out of the list as it renames it\n"
        ),
        "{printed}"
    );
}

#[test]
fn unused_decisions_alone_are_listed() {
    let unused = [
        Key::document(&fact("lantern/x"), DocumentField::Ended),
        Key::document(&entry("typo"), DocumentField::Table),
        Key::Machine(text("desk-old")),
    ];

    assert_eq!(
        report(&[], &[], &[], &unused, &none(), read(2, 3)),
        "0 blocked, 0 open, 3 decisions unused in 2 documents, 3 versions\n\n\
         ## Unused decisions\n\n\
         - machines.desk-old\n\
         - documents.\"entry/typo\"\n\
         - documents.\"fact/lantern/x\".ended\n"
    );
}

#[test]
fn a_count_orders_its_table_most_used_first() {
    let machine = |name: &str, versions| {
        Question::from(Open::Machine {
            name: text(name),
            versions,
            documents: Vec::new(),
        })
    };
    let questions = [
        machine("atlas", 2),
        machine("desk", 40),
        machine("phone", 2),
    ];

    let printed = report(&questions, &[], &[], &[], &none(), read(1, 44));

    assert!(
        printed.contains("| desk | 40 |  |\n| atlas | 2 |  |\n| phone | 2 |  |\n"),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\n[machines]\n\
             desk = \"?\"  # 40 versions: the machine's topic\n\
             atlas = \"?\"  # 2 versions: the machine's topic\n\
             phone = \"?\"  # 2 versions: the machine's topic\n"
        ),
        "{printed}"
    );
}

#[test]
fn a_cell_is_escaped_and_a_summary_is_cut() {
    let no_topic = |slug: &str, summary: &str| {
        Question::from(Open::NoTopic {
            document: followup(slug),
            summary: text(summary),
        })
    };
    let long = "x".repeat(100);
    let accented = "é".repeat(100);
    let bar_kept = format!("{}|{long}", "x".repeat(79));
    let bar_cut = format!("{}|{long}", "x".repeat(80));
    let questions = [
        no_topic("a", "a|b\nc\r\nd"),
        no_topic("b", &long),
        no_topic("c", &accented),
        no_topic("d", &bar_kept),
        no_topic("e", &bar_cut),
        Open::ended(
            &fact("lantern/relay-pin"),
            false,
            &format!("{long}\nsecond"),
        )
        .into(),
        Blocked::Unreadable {
            path: text("fact/a|b"),
            reason: text("one\ntwo"),
        }
        .into(),
    ];

    let printed = report(&questions, &[], &[], &[], &none(), read(1, 1));
    let cut = format!("{}…", "x".repeat(80));
    let row = |slug: &str, cell: &str| format!("| followup/{slug} | {cell} |\n");

    assert!(
        printed.contains("| fact/a\\|b | one<br>two |\n"),
        "{printed}"
    );
    assert!(printed.contains(&row("a", "a\\|b<br>c<br>d")), "{printed}");
    assert!(printed.contains(&row("b", &cut)), "{printed}");
    assert!(
        printed.contains(&row("c", &format!("{}…", "é".repeat(80)))),
        "{printed}"
    );
    assert!(
        printed.contains(&row("d", &format!("{}\\|…", "x".repeat(79)))),
        "{printed}"
    );
    assert!(printed.contains(&row("e", &cut)), "{printed}");
    assert!(
        printed.contains(&format!("| fact/lantern/relay-pin | fact |  | {cut} | ")),
        "{printed}"
    );
    assert!(
        printed.contains("topics = [\"?\"]  # a|b c d\n"),
        "{printed}"
    );
    assert!(
        printed.contains(&format!("topics = [\"?\"]  # {cut}\n")),
        "{printed}"
    );
    assert!(
        printed.contains(&format!("superseded; note: {cut}\n")),
        "{printed}"
    );
}

#[test]
fn the_same_input_in_any_order_prints_the_same_bytes() {
    let mut questions = every_blocked();
    questions.extend(self::questions(&every_open()));
    let unused = [Key::Machine(text("phone")), Key::tag("rust")];
    let authored = [
        Authored::Edges {
            topic: text("lantern"),
            part_of: vec![],
            uses: vec![text("desk")],
        },
        Authored::Topic {
            name: text("atlas"),
            summary: text("The atlas"),
            part_of: vec![],
            uses: vec![],
        },
    ];
    let printed = report(&questions, &authored, &[], &unused, &none(), read(5, 9));

    let mut shuffled = questions.clone();
    shuffled.reverse();
    shuffled.extend(questions.iter().take(4).cloned());
    let reversed = [unused[1].clone(), unused[0].clone(), unused[1].clone()];
    let swapped = [authored[1].clone(), authored[0].clone()];

    assert!(printed.starts_with("12 blocked, 20 open, 2 decisions unused in "));
    assert_eq!(
        report(&shuffled, &swapped, &[], &reversed, &none(), read(5, 9)),
        printed
    );
}

#[test]
fn an_idea_counts_for_a_fact_alone() {
    let lamp = entry(LAMP_DRIVER);

    assert_eq!(Open::ended(&lamp, true, ""), Open::ended(&lamp, false, ""));
    assert_ne!(
        Open::ended(&fact("a/b"), true, ""),
        Open::ended(&fact("a/b"), false, "")
    );
}

#[test]
fn the_reasons_are_the_new_schemas() {
    let words =
        |kind: KindOf, idea| -> Vec<&str> { kind.endings(idea).iter().map(Reason::word).collect() };

    assert_eq!(reasons(Kind::Fact, true), words(KindOf::Fact, true));
    assert_eq!(reasons(Kind::Fact, false), words(KindOf::Fact, false));
    assert_eq!(reasons(Kind::Entry, true), words(KindOf::Entry, false));
    assert_eq!(reasons(Kind::Topic, false), words(KindOf::Topic, false));
    assert_eq!(
        reasons(Kind::Followup, false),
        words(KindOf::Followup, false)
    );
    assert_ne!(reasons(Kind::Fact, true), reasons(Kind::Fact, false));
}

#[test]
fn every_case_of_a_rename_not_joined_has_its_words() {
    let of = |case| {
        Question::from(Blocked::RenameUnjoined {
            document: DocumentKey::new(Kind::Topic, "lamp"),
            others: vec![text("lantern")],
            case,
        })
    };
    let questions = [
        of(Unjoined::NoTombstone),
        of(Unjoined::Met),
        of(Unjoined::Parted),
        of(Unjoined::Circle),
    ];

    let printed = report(&questions, &[], &[], &[], &none(), read(1, 1));

    assert!(
        printed.contains(
            "| topic/lamp | lantern | renamed from a slug no rename of which is its parent |\n\
             | topic/lamp | lantern | several renames into this slug |\n\
             | topic/lamp | lantern | renamed to several slugs |\n\
             | topic/lamp | lantern | renames that lead back to this slug |\n"
        ),
        "{printed}"
    );
}

#[test]
fn an_ended_topic_is_answered_where_the_reference_is() {
    let through = |document: DocumentKey, through| {
        Question::from(Open::EndedTopic {
            document,
            through,
            topic: text("atlas"),
        })
    };
    let questions = [
        through(fact("atlas/relay-pin"), Through::FactTopic),
        through(DocumentKey::new(Kind::Topic, "Lantern"), Through::Includes),
        through(
            followup("2026-09-04-port"),
            Through::Tag(Folded::new("maps")),
        ),
        through(followup("2026-09-05-wire"), Through::Touching),
        through(DocumentKey::new(Kind::Topic, "desk"), Through::Claim),
    ];

    assert_eq!(
        stubs(&questions, &none()),
        "# A table is written once: move a line under the table the file already holds.\n\n\
         [topics.desk]\n\
         unclaim = [\n\
         \x20 \"?\",  # write atlas here: it is ended, and every claim on it ends in one version \
         the conversion writes\n\
         ]\n\n\
         [topics.lantern]\n\
         drop = [\n\
         \x20 \"?\",  # write atlas here: it is ended. Left in part_of or uses as well, the edge \
         stays in every converted version and one version the conversion writes removes it; in \
         drop alone, the edge is left out of every converted version\n\
         ]\n\n\
         [documents.\"fact/atlas/relay-pin\"]\n\
         topics = [\"?\"]  # atlas is ended: the one topic of the fact\n\n\
         [documents.\"followup/2026-09-04-port\"]\n\
         tags.maps = \"?\"  # atlas is ended: another topic for this document, or \"\" to drop the tag\n\n\
         [documents.\"followup/2026-09-05-wire\"]\n\
         touching = \"?\"  # atlas is ended: another topic, or \"\" for no trigger\n"
    );
    let printed = report(&questions, &[], &[], &[], &none(), read(1, 1));
    assert!(
        printed.contains(
            "| fact/atlas/relay-pin | its topic | atlas |\n\
             | topic/Lantern | includes | atlas |\n\
             | topic/desk | a claim | atlas |\n\
             | followup/2026-09-04-port | tag maps | atlas |\n\
             | followup/2026-09-05-wire | touching | atlas |\n"
        ),
        "{printed}"
    );
}

#[test]
fn a_table_the_file_holds_is_a_comment_in_the_stubs() {
    let file = "[tags]\nrust = \"\"\n\n\
                [documents.\"entry/2026-09/2026-09-04-lamp-driver\"]\nname = \"lamp-driver\"\n\n\
                [topics.Lantern]\nuses = []\n";
    let held = Decisions::parse(file).expect("the file reads");
    let printed = stubs(&questions(&every_open()), held.tables());

    assert!(
        printed.contains(
            "\n# add under the existing [tags]:\n\
             # relay = \"?\"  # 1 document: entry/2026-09/2026-09-04-lamp-driver; \"\" drops the tag\n"
        ),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\n# add under the existing [topics.lantern]:\n\
             # part_of = [\n\
             #   \"?\",  # write atlas here, or in uses, or in drop\n\
             # ]\n\
             # uses = []\n\
             # drop = []\n"
        ),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\n# add under the existing [documents.\"entry/2026-09/2026-09-04-lamp-driver\"]:\n\
             # tags.relay = \"?\""
        ),
        "{printed}"
    );
    assert!(
        printed.contains("\n[machines]\ndesk-old = \"?\""),
        "{printed}"
    );

    let pasted = format!("{file}\n{printed}");
    let read_back = Decisions::parse(&pasted).expect("the stubs pasted under the file read");
    assert_eq!(
        read_back.unanswered().len(),
        16,
        "{:?}",
        read_back.unanswered()
    );

    let again = report(&[], &[], &[], &[], held.tables(), read(1, 1));
    assert_eq!(again, "nothing is open in 1 document, 1 version\n");
    let with = report(
        &questions(&every_open()),
        &[],
        &[],
        &[],
        held.tables(),
        read(1, 1),
    );
    assert!(
        with.contains("\n# add under the existing [tags]:\n"),
        "{with}"
    );
}

#[test]
fn a_tombstone_that_follows_nothing_says_so() {
    let alone = Blocked::Tombstone {
        document: DocumentKey::new(Kind::Topic, "beacon"),
        version: text("cd88"),
        follows: Vec::new(),
    };
    let printed = report(&[alone.into()], &[], &[], &[], &none(), read(1, 1));
    assert!(
        printed.contains("| topic/beacon | cd88 | no version |\n"),
        "{printed}"
    );
}

#[test]
fn a_reason_is_asked_for_with_how_the_document_had_already_ended() {
    let Open::Ended {
        document,
        idea,
        note,
        ..
    } = Open::ended(&followup("2026-09-04-port"), false, "Was never ours")
    else {
        panic!("not an ending");
    };
    let already = Some(text("done on 2026-09-06"));
    let asked = [Question::from(Open::Ended {
        document,
        idea,
        note,
        already,
    })];

    assert_eq!(
        stubs(&asked, &none()),
        "# A table is written once: move a line under the table the file already holds.\n\n\
         [documents.\"followup/2026-09-04-port\"]\n\
         ended = \"?\"  # one of: done, dropped; already done on 2026-09-06; note: Was never ours\n"
    );
    let printed = report(&asked, &[], &[], &[], &none(), read(1, 1));
    assert!(
        printed.contains(
            "| document | kind | already | note | reasons |\n\
             | --- | --- | --- | --- | --- |\n\
             | followup/2026-09-04-port | followup | done on 2026-09-06 | Was never ours | done, dropped |\n"
        ),
        "{printed}"
    );
}

#[test]
fn a_machine_with_no_topic_names_the_entries_that_can_answer_for_themselves() {
    let documents = vec![entry("a"), entry("b"), entry("c"), entry("d")];
    let questions = [Question::from(Open::Machine {
        name: text("bench"),
        versions: 5,
        documents,
    })];

    let printed = report(&questions, &[], &[], &[], &none(), read(4, 5));

    assert!(
        printed.contains(
            "\n| machine | versions | among them |\n| --- | --- | --- |\n\
             | bench | 5 | entry/a, entry/b, entry/c |\n"
        ),
        "{printed}"
    );
    assert!(
        printed.ends_with(
            "\n[machines]\n\
             bench = \"?\"  # 5 versions: the machine's topic; a document's table may hold \
             `machine` instead: entry/a, entry/b, entry/c, …\n```\n"
        ),
        "{printed}"
    );
}
