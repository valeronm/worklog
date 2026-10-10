//! The five kinds of document and the fields each carries.

use super::directory::Directory;
use super::ending::{Ending, Reason};
use super::error::SchemaError;
use super::field::{Date, Name, Reader, Writer};
use crate::domain::id::DocumentId;
use crate::domain::version::{Fields, Kind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindOf {
    Topic,
    Fact,
    Entry,
    Followup,
    Claim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reference {
    pub key: &'static str,
    /// The kind the target must be; `None` for any document.
    pub target: Option<KindOf>,
    /// Whether a reference newly made must point at a document that is not ended.
    pub unended_when_made: bool,
    /// Whether a document holding this reference keeps its target from being ended.
    pub holds_open: bool,
    /// Whether the document belongs to the topics this field names.
    pub membership: bool,
}

impl Reference {
    const fn filed(mut self) -> Reference {
        self.membership = true;
        self
    }
}

const fn reference(
    key: &'static str,
    target: Option<KindOf>,
    unended_when_made: bool,
    holds_open: bool,
) -> Reference {
    Reference {
        key,
        target,
        unended_when_made,
        holds_open,
        membership: false,
    }
}

const TOPIC: Option<KindOf> = Some(KindOf::Topic);
const TOPIC_REFERENCES: [Reference; 2] = [
    reference("part_of", TOPIC, true, true),
    reference("uses", TOPIC, true, true),
];
const FACT_REFERENCES: [Reference; 1] = [reference("topic", TOPIC, true, true).filed()];
const ENTRY_REFERENCES: [Reference; 2] = [
    reference("machine", TOPIC, true, false),
    reference("topics", TOPIC, true, false).filed(),
];
const FOLLOWUP_REFERENCES: [Reference; 4] = [
    reference("topics", TOPIC, true, true).filed(),
    reference("entry", Some(KindOf::Entry), false, false),
    reference("about", None, false, false),
    reference("touching", TOPIC, true, true),
];
const CLAIM_REFERENCES: [Reference; 2] = [
    reference("machine", TOPIC, true, true),
    reference("topic", TOPIC, true, true).filed(),
];

impl KindOf {
    pub const ALL: [KindOf; 5] = [
        KindOf::Topic,
        KindOf::Fact,
        KindOf::Entry,
        KindOf::Followup,
        KindOf::Claim,
    ];

    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            KindOf::Topic => "topic",
            KindOf::Fact => "fact",
            KindOf::Entry => "entry",
            KindOf::Followup => "followup",
            KindOf::Claim => "claim",
        }
    }

    #[must_use]
    pub fn plural(self) -> &'static str {
        match self {
            KindOf::Topic => "topics",
            KindOf::Fact => "facts",
            KindOf::Entry => "entries",
            KindOf::Followup => "followups",
            KindOf::Claim => "claims",
        }
    }

    #[must_use]
    pub fn references(self) -> &'static [Reference] {
        match self {
            KindOf::Topic => &TOPIC_REFERENCES,
            KindOf::Fact => &FACT_REFERENCES,
            KindOf::Entry => &ENTRY_REFERENCES,
            KindOf::Followup => &FOLLOWUP_REFERENCES,
            KindOf::Claim => &CLAIM_REFERENCES,
        }
    }

    /// The key of the kind's first reference `wanted` picks.
    #[must_use]
    pub fn key_where(self, wanted: impl Fn(&Reference) -> bool) -> Option<&'static str> {
        self.references()
            .iter()
            .find(|reference| wanted(reference))
            .map(|reference| reference.key)
    }

    /// The keys that never change after a document's first version.
    #[must_use]
    pub fn set_once(self) -> &'static [&'static str] {
        match self {
            KindOf::Topic | KindOf::Fact => &["created"],
            KindOf::Entry => &["date", "machine"],
            KindOf::Followup => &["created", "entry", "about"],
            KindOf::Claim => &["machine", "topic", "directory"],
        }
    }

    pub fn of(kind: &Kind) -> Result<KindOf, SchemaError> {
        KindOf::ALL
            .into_iter()
            .find(|known| known.word() == kind.as_str())
            .ok_or_else(|| SchemaError::UnknownKind(kind.to_string()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Topic {
    pub name: Name,
    pub former_names: Vec<Name>,
    pub created: Date,
    pub summary: String,
    pub part_of: Vec<DocumentId>,
    pub uses: Vec<DocumentId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub name: Name,
    pub former_names: Vec<Name>,
    pub topic: DocumentId,
    pub created: Date,
    /// The last day the claim was checked against its subject and held.
    pub confirmed: Date,
    pub idea: bool,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: Name,
    pub former_names: Vec<Name>,
    pub date: Date,
    pub machine: DocumentId,
    pub topics: Vec<DocumentId>,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trigger {
    LookAgain {
        on: Date,
        why: String,
    },
    /// One of the followup's own topics.
    Touching(DocumentId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Followup {
    pub created: Date,
    pub topics: Vec<DocumentId>,
    pub entry: Option<DocumentId>,
    pub about: Option<DocumentId>,
    pub trigger: Option<Trigger>,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    pub machine: DocumentId,
    pub topic: DocumentId,
    /// Absent for a topic loaded where no claim's directory matches.
    pub directory: Option<Directory>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Topic(Topic),
    Fact(Fact),
    Entry(Entry),
    Followup(Followup),
    Claim(Claim),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub content: Content,
    pub ending: Option<Ending>,
}

fn trigger(reader: &mut Reader, topics: &[DocumentId]) -> Result<Option<Trigger>, SchemaError> {
    let on = reader.optional_date("look_again")?;
    let why = reader.optional_line("why")?;
    let touching = reader.optional_id("touching")?;
    match (on, why, touching) {
        (None, None, None) => Ok(None),
        (Some(_), _, Some(_)) => Err(SchemaError::field(
            "touching",
            "is set beside `look_again`; a followup has one or the other",
        )),
        (Some(on), Some(why), None) if !why.is_empty() => Ok(Some(Trigger::LookAgain { on, why })),
        (Some(_), _, None) => Err(SchemaError::field("why", "is missing beside `look_again`")),
        (None, Some(_), _) => Err(SchemaError::field("why", "is set without `look_again`")),
        (None, None, Some(topic)) if topics.contains(&topic) => Ok(Some(Trigger::Touching(topic))),
        (None, None, Some(_)) => Err(SchemaError::field(
            "touching",
            "names a topic that is not among `topics`",
        )),
    }
}

fn directory(reader: &mut Reader) -> Result<Option<Directory>, SchemaError> {
    reader
        .optional_line("directory")?
        .map(|path| Directory::parse(&path).map_err(|error| SchemaError::field("directory", error)))
        .transpose()
}

fn content(kind: KindOf, reader: &mut Reader) -> Result<Content, SchemaError> {
    Ok(match kind {
        KindOf::Topic => Content::Topic(Topic {
            name: reader.name("name")?,
            former_names: reader.names("former_names")?,
            created: reader.date("created")?,
            summary: reader.line("summary")?,
            part_of: reader.ids("part_of")?,
            uses: reader.ids("uses")?,
        }),
        KindOf::Fact => Content::Fact(Fact {
            name: reader.name("name")?,
            former_names: reader.names("former_names")?,
            topic: reader.id("topic")?,
            created: reader.date("created")?,
            confirmed: reader.date("confirmed")?,
            idea: reader.flag("idea")?,
            summary: reader.line("summary")?,
        }),
        KindOf::Entry => Content::Entry(Entry {
            name: reader.name("name")?,
            former_names: reader.names("former_names")?,
            date: reader.date("date")?,
            machine: reader.id("machine")?,
            topics: reader.ids("topics")?,
            summary: reader.line("summary")?,
        }),
        KindOf::Followup => {
            let topics = reader.ids("topics")?;
            if topics.is_empty() {
                return Err(SchemaError::field("topics", "names no topic"));
            }
            Content::Followup(Followup {
                created: reader.date("created")?,
                entry: reader.optional_id("entry")?,
                about: reader.optional_id("about")?,
                trigger: trigger(reader, &topics)?,
                topics,
                summary: reader.line("summary")?,
            })
        }
        KindOf::Claim => Content::Claim(Claim {
            machine: reader.id("machine")?,
            topic: reader.id("topic")?,
            directory: directory(reader)?,
        }),
    })
}

impl Content {
    #[must_use]
    pub fn naming(&self) -> Option<(&Name, &[Name])> {
        match self {
            Content::Topic(topic) => Some((&topic.name, &topic.former_names)),
            Content::Fact(fact) => Some((&fact.name, &fact.former_names)),
            Content::Entry(entry) => Some((&entry.name, &entry.former_names)),
            Content::Followup(_) | Content::Claim(_) => None,
        }
    }

    pub(super) fn former_names_mut(&mut self) -> Option<&mut Vec<Name>> {
        match self {
            Content::Topic(topic) => Some(&mut topic.former_names),
            Content::Fact(fact) => Some(&mut fact.former_names),
            Content::Entry(entry) => Some(&mut entry.former_names),
            Content::Followup(_) | Content::Claim(_) => None,
        }
    }

    #[must_use]
    pub fn kind(&self) -> KindOf {
        match self {
            Content::Topic(_) => KindOf::Topic,
            Content::Fact(_) => KindOf::Fact,
            Content::Entry(_) => KindOf::Entry,
            Content::Followup(_) => KindOf::Followup,
            Content::Claim(_) => KindOf::Claim,
        }
    }

    #[must_use]
    pub fn endings(&self) -> &'static [Reason] {
        match self {
            Content::Topic(_) => &[Reason::Retired, Reason::Merged],
            Content::Fact(fact) if fact.idea => &[Reason::Built, Reason::Abandoned],
            Content::Fact(_) => &[Reason::False, Reason::Moved, Reason::Superseded],
            Content::Entry(_) => &[Reason::Removed, Reason::Merged],
            Content::Followup(_) => &[Reason::Done, Reason::Dropped],
            Content::Claim(_) => &[Reason::Removed],
        }
    }

    fn write(&self, writer: Writer) -> Writer {
        match self {
            Content::Topic(topic) => writer
                .text("name", topic.name.as_str())
                .names("former_names", &topic.former_names)
                .date("created", topic.created)
                .text("summary", &topic.summary)
                .ids("part_of", &topic.part_of)
                .ids("uses", &topic.uses),
            Content::Fact(fact) => writer
                .text("name", fact.name.as_str())
                .names("former_names", &fact.former_names)
                .id("topic", &fact.topic)
                .date("created", fact.created)
                .date("confirmed", fact.confirmed)
                .flag("idea", fact.idea)
                .text("summary", &fact.summary),
            Content::Entry(entry) => writer
                .text("name", entry.name.as_str())
                .names("former_names", &entry.former_names)
                .date("date", entry.date)
                .id("machine", &entry.machine)
                .ids("topics", &entry.topics)
                .text("summary", &entry.summary),
            Content::Followup(followup) => {
                let writer = writer
                    .date("created", followup.created)
                    .ids("topics", &followup.topics)
                    .optional_id("entry", followup.entry.as_ref())
                    .optional_id("about", followup.about.as_ref());
                match &followup.trigger {
                    None => writer,
                    Some(Trigger::LookAgain { on, why }) => {
                        writer.date("look_again", *on).text("why", why)
                    }
                    Some(Trigger::Touching(topic)) => writer.id("touching", topic),
                }
                .text("summary", &followup.summary)
            }
            Content::Claim(claim) => writer
                .id("machine", &claim.machine)
                .id("topic", &claim.topic)
                .optional_text("directory", claim.directory.as_ref().map(Directory::as_str)),
        }
    }
}

impl Record {
    pub fn read(kind: &Kind, fields: &Fields) -> Result<Record, SchemaError> {
        let mut reader = Reader::new(fields);
        let content = content(KindOf::of(kind)?, &mut reader)?;
        let ending = Ending::read(&mut reader)?;
        reader.finish()?;
        Ok(Record { content, ending })
    }

    #[must_use]
    pub fn fields(&self) -> Fields {
        let writer = self.content.write(Writer::default());
        match &self.ending {
            Some(ending) => ending.write(writer),
            None => writer,
        }
        .done()
    }

    /// Refuses a second ending, a reason of another kind, and an empty
    /// note for any reason but done.
    pub fn end(self, ending: Ending) -> Result<Record, SchemaError> {
        if self.ending.is_some() {
            return Err(SchemaError::Ended);
        }
        if !self.content.endings().contains(&ending.reason) {
            return Err(SchemaError::ReasonOfAnotherKind {
                reason: ending.reason.word().to_owned(),
                kind: self.content.kind().word(),
            });
        }
        if ending.note.trim().is_empty() && ending.reason != Reason::Done {
            return Err(SchemaError::NoNote);
        }
        Ok(Record {
            ending: Some(ending),
            ..self
        })
    }

    pub fn reopen(self) -> Result<Record, SchemaError> {
        if self.ending.is_none() {
            return Err(SchemaError::NotEnded);
        }
        Ok(Record {
            ending: None,
            ..self
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::id::DocumentId;
    use crate::domain::testing::{ATLAS, LANTERN};

    const DESK: &str = "de5c0000000000000000000000000002";

    fn kind(word: &str) -> Kind {
        Kind::parse(word).unwrap()
    }

    fn read(word: &str, toml: &str) -> Result<Record, SchemaError> {
        Record::read(&kind(word), &toml.parse().unwrap())
    }

    fn fact() -> String {
        format!(
            "name = \"relay-pin\"\nformer_names = [\"relay\"]\ntopic = \"{LANTERN}\"\n\
             created = 2026-09-04\nconfirmed = 2026-10-09\n\
             summary = \"The relay pin is fixed\"\n"
        )
    }

    fn followup(trigger: &str) -> String {
        format!(
            "created = 2026-09-04\ntopics = [\"{LANTERN}\", \"{ATLAS}\"]\n{trigger}\
             summary = \"Add the second relay\"\n"
        )
    }

    fn ending(reason: Reason, note: &str) -> Ending {
        Ending {
            reason,
            on: Date::parse("2026-10-09").unwrap(),
            by: None,
            note: note.to_owned(),
        }
    }

    fn fixtures() -> Vec<(&'static str, String)> {
        let topic = format!(
            "name = \"lantern\"\nformer_names = [\"lamp\"]\ncreated = 2026-09-04\n\
             summary = \"A lamp controller\"\npart_of = [\"{ATLAS}\"]\nuses = [\"{DESK}\"]\n"
        );
        let entry = format!(
            "name = \"lamp-driver\"\nformer_names = [\"lamp\"]\ndate = 2026-09-04\n\
             machine = \"{DESK}\"\ntopics = [\"{LANTERN}\"]\nsummary = \"Wired the lamp driver\"\n"
        );
        let dated = followup("look_again = 2026-11-01\nwhy = \"the board arrives\"\n");
        let touching = followup(&format!(
            "entry = \"{DESK}\"\nabout = \"{ATLAS}\"\ntouching = \"{ATLAS}\"\n"
        ));
        let claim = format!(
            "machine = \"{DESK}\"\ntopic = \"{LANTERN}\"\ndirectory = \"~/projects/lantern\"\n"
        );
        let unplaced = format!("machine = \"{DESK}\"\ntopic = \"{LANTERN}\"\n");
        vec![
            ("topic", topic),
            ("fact", fact()),
            ("fact", fact().replace("summary", "idea = true\nsummary")),
            ("entry", entry),
            ("followup", dated),
            ("followup", touching),
            ("followup", followup("")),
            ("claim", claim),
            ("claim", unplaced),
        ]
    }

    #[test]
    fn each_kind_reads_and_writes_back_as_it_was() {
        for (word, toml) in fixtures() {
            let record = read(word, &toml).unwrap();
            assert_eq!(record.content.kind().word(), word);
            assert_eq!(toml::to_string(&record.fields()).unwrap(), toml, "{word}");
        }
    }

    #[test]
    fn every_kind_has_a_plural() {
        assert_eq!(
            KindOf::ALL.map(KindOf::plural),
            ["topics", "facts", "entries", "followups", "claims"]
        );
    }

    #[test]
    fn a_kind_s_word_is_the_singular_of_its_plural() {
        assert_eq!(
            KindOf::ALL.map(KindOf::word),
            ["topic", "fact", "entry", "followup", "claim"]
        );
    }

    #[test]
    fn a_kinds_key_lists_name_the_keys_it_writes() {
        let holds_ids = |value: &toml::Value| match value {
            toml::Value::String(text) => DocumentId::parse(text).is_ok(),
            toml::Value::Array(items) => items.iter().all(|item| {
                item.as_str()
                    .is_some_and(|text| DocumentId::parse(text).is_ok())
            }),
            _ => false,
        };
        for kind in KindOf::ALL {
            let mut written: Vec<String> = Vec::new();
            let mut ids: Vec<String> = Vec::new();
            for (_, toml) in fixtures().iter().filter(|(word, _)| *word == kind.word()) {
                let fields: Fields = toml.parse().unwrap();
                for (key, value) in &fields {
                    written.push(key.clone());
                    if holds_ids(value) {
                        ids.push(key.clone());
                    }
                }
            }
            ids.sort();
            ids.dedup();
            let mut references: Vec<&str> = kind.references().iter().map(|r| r.key).collect();
            references.sort_unstable();
            assert_eq!(ids, references, "{}", kind.word());
            assert!(
                kind.references()
                    .iter()
                    .all(|reference| !reference.holds_open
                        || (reference.target == Some(KindOf::Topic)
                            && reference.unended_when_made)),
                "{}",
                kind.word()
            );
            for key in kind.set_once() {
                assert!(
                    written.iter().any(|held| held == key),
                    "{}: {key}",
                    kind.word()
                );
            }
        }
    }

    #[test]
    fn a_key_is_found_by_what_its_reference_is_for() {
        let filed = |kind: KindOf| kind.key_where(|reference| reference.membership);
        assert_eq!(
            KindOf::ALL.map(filed),
            [
                None,
                Some("topic"),
                Some("topics"),
                Some("topics"),
                Some("topic")
            ]
        );
        assert_eq!(
            KindOf::Followup.key_where(|reference| reference.target == Some(KindOf::Entry)),
            Some("entry")
        );
        assert_eq!(
            KindOf::Claim.key_where(|reference| reference.target == TOPIC && !reference.membership),
            Some("machine")
        );
        assert_eq!(
            KindOf::Topic.key_where(|reference| reference.target.is_none()),
            None
        );
    }

    #[test]
    fn a_kind_refuses_what_is_not_its_own() {
        assert_eq!(
            read("note", &fact()),
            Err(SchemaError::UnknownKind("note".to_owned()))
        );
        assert_eq!(
            read("fact", &format!("{}tags = [\"x\"]\n", fact())),
            Err(SchemaError::UnknownKey("tags".to_owned()))
        );
        assert!(matches!(
            read("fact", &fact().replace("confirmed = 2026-10-09\n", "")),
            Err(SchemaError::Field { key, .. }) if key == "confirmed"
        ));
    }

    #[test]
    fn a_claim_stored_with_a_trailing_slash_reads_as_the_directory_without_it() {
        let claim = |directory: &str| {
            read(
                "claim",
                &format!(
                    "machine = \"{DESK}\"\ntopic = \"{LANTERN}\"\ndirectory = \"{directory}\"\n"
                ),
            )
            .unwrap()
        };
        assert_eq!(claim("~/lantern/"), claim("~/lantern"));
        assert_eq!(
            claim("~/lantern/").fields().get("directory"),
            Some(&"~/lantern".into())
        );
    }

    #[test]
    fn a_claims_directory_is_under_home_or_absolute() {
        let claim = |directory: &str| {
            read(
                "claim",
                &format!(
                    "machine = \"{DESK}\"\ntopic = \"{LANTERN}\"\ndirectory = \"{directory}\"\n"
                ),
            )
        };
        for good in ["~", "~/projects/lantern", "/srv/lantern"] {
            assert!(claim(good).is_ok(), "{good}");
        }
        for bad in ["", "projects/lantern", "~lantern", "./lantern"] {
            assert!(
                matches!(claim(bad), Err(SchemaError::Field { ref key, .. }) if key == "directory"),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_followup_has_a_topic_and_at_most_one_trigger() {
        let refused = |toml: &str, key: &str, why: &str| match read("followup", toml) {
            Err(SchemaError::Field {
                key: named,
                why: said,
            }) => {
                assert_eq!(named, key, "{said}");
                assert!(said.contains(why), "{said}");
            }
            other => panic!("{other:?}"),
        };
        refused(
            "created = 2026-09-04\nsummary = \"Add the second relay\"\n",
            "topics",
            "no topic",
        );
        refused(
            &followup(&format!(
                "look_again = 2026-11-01\nwhy = \"x\"\ntouching = \"{ATLAS}\"\n"
            )),
            "touching",
            "one or the other",
        );
        refused(&followup("look_again = 2026-11-01\n"), "why", "missing");
        refused(
            &followup("look_again = 2026-11-01\nwhy = \"\"\n"),
            "why",
            "missing",
        );
        refused(&followup("why = \"x\"\n"), "why", "without `look_again`");
        refused(
            &followup(&format!("touching = \"{DESK}\"\n")),
            "touching",
            "not among `topics`",
        );
    }

    #[test]
    fn a_document_ends_for_a_reason_of_its_kind() {
        let record = read("fact", &fact()).unwrap();
        let ended = record
            .clone()
            .end(ending(Reason::False, "the board was rewired"))
            .unwrap();
        let written = toml::to_string(&ended.fields()).unwrap();
        assert!(
            written.ends_with(
                "ended = \"false\"\nended_on = 2026-10-09\nnote = \"the board was rewired\"\n"
            ),
            "{written}"
        );
        assert_eq!(read("fact", &written), Ok(ended.clone()));
        assert_eq!(
            record.clone().end(ending(Reason::Built, "x")),
            Err(SchemaError::ReasonOfAnotherKind {
                reason: "built".to_owned(),
                kind: "fact"
            })
        );
        assert_eq!(
            record.end(ending(Reason::Moved, " ")),
            Err(SchemaError::NoNote)
        );
        assert_eq!(
            ended.clone().end(ending(Reason::Moved, "x")),
            Err(SchemaError::Ended)
        );
        assert_eq!(ended.reopen().unwrap().ending, None);
    }

    #[test]
    fn each_kind_has_its_own_reasons() {
        let idea = read("fact", &fact().replace("summary", "idea = true\nsummary")).unwrap();
        assert!(idea.clone().end(ending(Reason::Built, "x")).is_ok());
        assert!(idea.end(ending(Reason::False, "x")).is_err());
        let open = read("followup", &followup("")).unwrap();
        assert!(open.clone().end(ending(Reason::Done, "")).is_ok());
        assert_eq!(
            open.clone().end(ending(Reason::Dropped, "")),
            Err(SchemaError::NoNote)
        );
        assert_eq!(open.reopen(), Err(SchemaError::NotEnded));
    }

    #[test]
    fn an_ending_is_read_whole_and_a_word_from_a_newer_release_still_ends() {
        let ended = format!(
            "{}ended = \"archived\"\nended_on = 2026-10-09\nended_by = \"{ATLAS}\"\n",
            fact()
        );
        let record = read("fact", &ended).unwrap();
        let ending = record.ending.clone().unwrap();
        assert_eq!(ending.reason, Reason::Other("archived".to_owned()));
        assert_eq!(ending.by.unwrap().as_str(), ATLAS);
        assert_eq!(ending.note, "");
        assert_eq!(toml::to_string(&record.fields()).unwrap(), ended);

        for (stray, key) in [
            ("ended_on = 2026-10-09\n", "ended_on"),
            ("note = \"x\"\n", "note"),
            ("ended = \"false\"\n", "ended_on"),
        ] {
            assert!(
                matches!(
                    read("fact", &format!("{}{stray}", fact())),
                    Err(SchemaError::Field { key: named, .. }) if named == key
                ),
                "{stray}"
            );
        }
    }
}
