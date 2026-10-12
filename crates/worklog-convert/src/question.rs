//! Everything that stops a conversion, and for what a decision file answers, the keys the
//! answer goes under.

use worklog_next::domain::schema::{KindOf, Reason};

use crate::key::{
    ConversionKey, DocumentField, DocumentKey, Folded, Key, Placement, TopicKey, segment,
};
use crate::old::{Kind, Problem};
use crate::text::{counted, cut, first_line};

/// Ordered by class, then variant, then its fields.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Question {
    Blocked(Blocked),
    Open(Open),
}

/// What is settled in the store being converted, with the tool that wrote it; no decision
/// file answers it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Blocked {
    Forked {
        document: DocumentKey,
        heads: Vec<String>,
    },
    Waiting {
        document: DocumentKey,
        missing: Vec<String>,
    },
    Unreadable {
        path: String,
        reason: String,
    },
    Skipped {
        path: String,
    },
    /// A tombstone names a slug the store holds no version under.
    RenameCutShort {
        document: DocumentKey,
        target: String,
    },
    WriteTime {
        document: DocumentKey,
        version: String,
        text: String,
    },
    /// A version block holds keys outside the grammar of the store being converted.
    ForeignKeys {
        document: DocumentKey,
        version: String,
        keys: Vec<String>,
    },
    /// A rename that does not join two slugs into one chain.
    RenameUnjoined {
        document: DocumentKey,
        /// The slugs on the other side of the rename.
        others: Vec<String>,
        case: Unjoined,
    },
    /// A stored value the new schema cannot take.
    Value {
        document: DocumentKey,
        version: String,
        field: String,
        value: String,
        why: String,
    },
    /// A tombstone that follows no version of its document, or several, so the fields and
    /// body it ends are not those of one version.
    Tombstone {
        document: DocumentKey,
        version: String,
        follows: Vec<String>,
    },
    /// A name several topics hold when case is ignored, written in a case none of them has.
    TopicCase {
        name: String,
        topics: Vec<String>,
    },
    /// A planned document lacks what a written version has, or refers to what the plan
    /// does not hold as the new schema wants it.
    Unheld {
        /// The kind word of the planned document.
        kind: String,
        /// The id of the planned document.
        document: String,
        field: String,
        target: String,
        why: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Unjoined {
    /// A version says it was renamed from a slug no rename of which is its parent.
    NoTombstone,
    /// Several renames moved a version to the one slug.
    Met,
    /// One document was renamed to several slugs.
    Parted,
    /// The renames lead back to the slug they started from.
    Circle,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Through {
    /// The topic a fact is filed under.
    FactTopic,
    /// An item of a topic's `includes` the file places in `part_of` or `uses`.
    Includes,
    /// A tag of an entry or a followup the file answers with the topic.
    Tag(Folded),
    /// The topic a recheck touches, a followup's or a fact's.
    Touching,
    /// A claim a machine topic holds on the topic when its history ends.
    Claim,
}

/// What a decision file answers, or for `Cycle` and `Answer`, what a change to an answer
/// it already holds settles.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Open {
    /// The conversion authors versions and the file gives them no write time and machine.
    Conversion { versions: usize },
    Tag {
        tag: Folded,
        /// Every document found carrying the tag, in order.
        carriers: Vec<DocumentKey>,
    },
    /// An item of a topic's `includes` the file places in no list.
    Include {
        topic: Folded,
        item: Folded,
        /// Whether the item names no topic.
        missing: bool,
        /// Whether the item names an ended topic that the topic's last version still
        /// includes, so a place alone leaves the ended-topic question to follow.
        ended: bool,
        /// Whether the store's reader leaves the item out of the last version's list as it
        /// renames the list, so the tool that wrote the store acts on no such edge.
        unread: bool,
    },
    /// A topic the file creates and gives no summary.
    TopicSummary { topic: Folded },
    /// A machine name no machine topic holds, on the versions it stamped and the entries
    /// whose `machine` it is.
    Machine {
        name: String,
        /// The versions that hold the name with no answer.
        versions: usize,
        /// The entries among them whose own `machine` it is, in order.
        documents: Vec<DocumentKey>,
    },
    /// A machine name several topics hold.
    MachineTopics { name: String, topics: Vec<String> },
    /// A name in a machine topic's `claims` or `unclaimed` that resolves to no topic.
    Claim { machine_topic: Folded, item: Folded },
    Ended {
        document: DocumentKey,
        /// Whether a fact is flagged as an idea, which ends for other reasons.
        idea: bool,
        note: String,
        /// How and when the document had ended before the tombstone, where it had.
        already: Option<String>,
    },
    /// A followup its tags file under no topic.
    NoTopic {
        document: DocumentKey,
        summary: String,
    },
    /// A fact whose slug opens with a name that resolves to no topic.
    FactTopic { fact: DocumentKey, topic: String },
    /// The name a document has at its head, which the new schema refuses or another
    /// document of its scope holds too.
    Name {
        document: DocumentKey,
        name: String,
        why: String,
    },
    /// A name the document had before a rename, which the new schema refuses.
    FormerName {
        document: DocumentKey,
        name: String,
        why: String,
    },
    /// A summary that is missing or not one line.
    Summary { document: DocumentKey },
    /// A fact's recheck of the touching form, which gives its followup no summary.
    RecheckSummary { fact: DocumentKey, touching: String },
    /// A followup's `entry` that resolves to no entry.
    Entry {
        followup: DocumentKey,
        entry: String,
    },
    /// A recheck, a followup's or a fact's, touching a name that resolves to no topic
    /// that is not ended.
    Touching {
        /// The followup or the fact that carries the recheck.
        document: DocumentKey,
        topic: String,
    },
    /// A document refers to an ended topic through a field the new store wants unended.
    EndedTopic {
        document: DocumentKey,
        through: Through,
        topic: String,
    },
    /// A document refers to a topic by a name several topics hold when case is ignored,
    /// through a field whose answer can spell one of them as the store does.
    TopicCase {
        document: DocumentKey,
        through: Through,
        name: String,
        topics: Vec<String>,
    },
    /// The topics of a cycle in the graph the placements make, in the order of its edges.
    Cycle { topics: Vec<String> },
    /// An answer of the file that names something that does not exist.
    Answer { key: Key, why: String },
}

/// A version the conversion authors because the decision file says so, with no version of
/// the store behind it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Authored {
    /// A topic a `[topics.<name>]` table creates.
    Topic {
        name: String,
        summary: String,
        part_of: Vec<String>,
        uses: Vec<String>,
    },
    /// Edges the file adds to a topic whose `includes` never held them.
    Edges {
        topic: String,
        part_of: Vec<String>,
        uses: Vec<String>,
    },
    /// Edges to ended topics the file drops from a topic that is not ended.
    Dropped {
        topic: String,
        part_of: Vec<String>,
        uses: Vec<String>,
    },
    /// A fact the file files under another topic, the one it was under being ended.
    Refiled {
        fact: String,
        from: String,
        to: String,
    },
    /// A claim on an ended topic the file ends; `directory` is empty for a claim with
    /// none.
    Unclaimed {
        machine_topic: String,
        topic: String,
        directory: String,
    },
    /// A document the file gives another name, the one it had being held by another
    /// document of its scope; `document` is its key in the file.
    Renamed {
        document: String,
        from: String,
        to: String,
    },
}

/// A `topics` line of the file that stands in for the tags of an entry or a followup where
/// no question asked for one; `document` is its key in the file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Overridden {
    pub(crate) document: String,
    pub(crate) topics: Vec<String>,
}

/// An entry the tags of its last version, as the file answers them, file under no topic;
/// `entry` is its key in the file.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Unfiled {
    pub(crate) entry: String,
    pub(crate) tags: Vec<String>,
}

impl From<Problem> for Blocked {
    fn from(problem: Problem) -> Blocked {
        match problem {
            Problem::Forked { kind, slug, heads } => Blocked::Forked {
                document: DocumentKey { kind, slug },
                heads,
            },
            Problem::Waiting {
                kind,
                slug,
                missing,
            } => Blocked::Waiting {
                document: DocumentKey { kind, slug },
                missing,
            },
            Problem::Unreadable { path, reason } => Blocked::Unreadable { path, reason },
            Problem::Skipped { path } => Blocked::Skipped { path },
        }
    }
}

impl From<Problem> for Question {
    fn from(problem: Problem) -> Question {
        Question::Blocked(problem.into())
    }
}

impl From<Blocked> for Question {
    fn from(blocked: Blocked) -> Question {
        Question::Blocked(blocked)
    }
}

impl From<Open> for Question {
    fn from(open: Open) -> Question {
        Question::Open(open)
    }
}

/// The reason words a document of the kind ends for in the new store; `idea` counts for a
/// fact alone.
pub(crate) fn reasons(kind: Kind, idea: bool) -> Vec<&'static str> {
    (kind_of(kind).endings(idea).iter())
        .map(Reason::word)
        .collect()
}

pub(crate) fn kind_of(kind: Kind) -> KindOf {
    match kind {
        Kind::Entry => KindOf::Entry,
        Kind::Fact => KindOf::Fact,
        Kind::Topic => KindOf::Topic,
        Kind::Followup => KindOf::Followup,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Offer {
    pub(crate) key: Key,
    /// What the question knows of the answer; `None` on a list the stub shows empty, as a
    /// place the item can go.
    pub(crate) evidence: Option<String>,
}

fn offer(key: Key, evidence: String) -> Offer {
    let evidence = Some(evidence);
    Offer { key, evidence }
}

fn conversion_offers(versions: usize) -> Vec<Offer> {
    let authored = counted(versions, "version");
    let written = format!(
        "the write time of the {authored} the conversion authors: a date and time with an \
         offset, not quoted"
    );
    let machine = "the machine name they are stamped with".to_owned();
    vec![
        offer(Key::Conversion(ConversionKey::Written), written),
        offer(Key::Conversion(ConversionKey::Machine), machine),
    ]
}

/// The first documents of a list, as many as a row or a stub names.
pub(crate) fn examples(documents: &[DocumentKey]) -> Vec<String> {
    (documents.iter().take(3))
        .map(DocumentKey::to_string)
        .collect()
}

fn first_few(documents: &[DocumentKey]) -> String {
    let mut shown = examples(documents);
    if documents.len() > shown.len() {
        shown.push("…".to_owned());
    }
    shown.join(", ")
}

fn tag_evidence(carriers: &[DocumentKey]) -> String {
    let carried = counted(carriers.len(), "document");
    format!("{carried}: {}; \"\" drops the tag", first_few(carriers))
}

fn machine_offers(name: &str, versions: usize, documents: &[DocumentKey]) -> Vec<Offer> {
    let mut evidence = format!("{}: the machine's topic", counted(versions, "version"));
    if !documents.is_empty() {
        let among = first_few(documents);
        evidence = format!("{evidence}; a document's table may hold `machine` instead: {among}");
    }
    vec![offer(Key::Machine(name.to_owned()), evidence)]
}

fn include_offers(
    topic: &Folded,
    item: &Folded,
    missing: bool,
    ended: bool,
    unread: bool,
) -> Vec<Offer> {
    let evidence = if missing {
        format!("write {item} here, or in uses, or in drop: it names no topic")
    } else if ended {
        format!(
            "write {item} here, or in uses, or in drop: it is ended. In drop alone, the edge is \
             left out of every converted version; in part_of or uses and in drop as well, the \
             edge stays in every converted version and one version the conversion writes \
             removes it"
        )
    } else if unread {
        format!(
            "write {item} here, or in uses, or in drop: worklog does not act on this item, \
             which it leaves out of the list as it renames it"
        )
    } else {
        format!("write {item} here, or in uses, or in drop")
    };
    let list = |placement| Key::Topic(topic.clone(), TopicKey::Placed(placement, item.clone()));
    let shown = |placement| Offer {
        key: list(placement),
        evidence: None,
    };
    vec![
        offer(list(Placement::PartOf), evidence),
        shown(Placement::Uses),
        shown(Placement::Drop),
    ]
}

fn ended_topic_offers(document: &DocumentKey, through: &Through, topic: &str) -> Vec<Offer> {
    let (key, evidence) = match through {
        Through::FactTopic => (
            Key::document(document, DocumentField::Topics),
            format!("{topic} is ended: the one topic of the fact"),
        ),
        Through::Includes => (
            Key::placed(&document.slug, Placement::Drop, topic),
            format!(
                "write {topic} here: it is ended. Left in part_of or uses as well, the edge \
                 stays in every converted version and one version the conversion writes \
                 removes it; in drop alone, the edge is left out of every converted version"
            ),
        ),
        Through::Tag(tag) => (
            Key::document(document, DocumentField::Tag(tag.clone())),
            format!("{topic} is ended: another topic for this document, or \"\" to drop the tag"),
        ),
        Through::Touching => (
            Key::document(document, DocumentField::Touching),
            format!("{topic} is ended: another topic, or \"\" for no trigger"),
        ),
        Through::Claim => (
            Key::unclaim(&document.slug, topic),
            format!(
                "write {topic} here: it is ended, and every claim on it ends in one version \
                 the conversion writes"
            ),
        ),
    };
    vec![offer(key, evidence)]
}

fn claim_offers(machine_topic: &Folded, item: &Folded) -> Vec<Offer> {
    let key = Key::Topic(machine_topic.clone(), TopicKey::Unclaim(item.clone()));
    let evidence = format!(
        "write {item} here: it is claimed and names no topic; or a [topics.{}] table creates it",
        segment(item.as_str())
    );
    vec![offer(key, evidence)]
}

fn topic_case_offers(
    document: &DocumentKey,
    through: &Through,
    name: &str,
    topics: &[String],
) -> Vec<Offer> {
    let held = format!("{name} is {} in another case", topics.join(", "));
    let (field, evidence) = match through {
        Through::FactTopic => (
            DocumentField::Topics,
            format!("{held}: the one topic of the fact, spelled as the store does"),
        ),
        Through::Tag(tag) => (
            DocumentField::Tag(tag.clone()),
            format!("{held}: one of them spelled as the store does, or \"\" to drop the tag"),
        ),
        Through::Touching => (
            DocumentField::Touching,
            format!("{held}: one of them spelled as the store does, or \"\" for no trigger"),
        ),
        Through::Includes | Through::Claim => return Vec::new(),
    };
    vec![offer(Key::document(document, field), evidence)]
}

impl Open {
    pub(crate) fn tag(tag: &str, carriers: &[DocumentKey]) -> Open {
        let mut carriers = carriers.to_vec();
        carriers.sort();
        carriers.dedup();
        let tag = Folded::new(tag);
        Open::Tag { tag, carriers }
    }

    #[cfg(test)]
    pub(crate) fn ended(document: &DocumentKey, idea: bool, note: &str) -> Open {
        Open::Ended {
            document: document.clone(),
            idea: idea && document.kind == Kind::Fact,
            note: note.to_owned(),
            already: None,
        }
    }

    /// The keys of a decision file that answer the question, the first to try first; none
    /// for a question a change to another answer settles.
    pub(crate) fn offers(&self) -> Vec<Offer> {
        let field = |document: &DocumentKey, field, evidence: String| {
            vec![offer(Key::document(document, field), evidence)]
        };
        match self {
            Open::Conversion { versions } => conversion_offers(*versions),
            Open::Tag { tag, carriers } => {
                vec![offer(Key::Tag(tag.clone()), tag_evidence(carriers))]
            }
            Open::Include {
                topic,
                item,
                missing,
                ended,
                unread,
            } => include_offers(topic, item, *missing, *ended, *unread),
            Open::TopicSummary { topic } => {
                let key = Key::Topic(topic.clone(), TopicKey::Summary);
                let evidence = "one line for the topic the file creates".to_owned();
                vec![offer(key, evidence)]
            }
            Open::Machine {
                name,
                versions,
                documents,
            } => machine_offers(name, *versions, documents),
            Open::MachineTopics { name, topics } => {
                let evidence = format!("the machine of {}: one of them", topics.join(", "));
                vec![offer(Key::Machine(name.clone()), evidence)]
            }
            Open::Claim {
                machine_topic,
                item,
            } => claim_offers(machine_topic, item),
            Open::Ended {
                document,
                idea,
                note,
                already,
            } => {
                let reasons = reasons(document.kind, *idea).join(", ");
                let note = cut(first_line(note));
                let mut evidence = format!("one of: {reasons}");
                if let Some(already) = already {
                    evidence = format!("{evidence}; already {already}");
                }
                if !note.is_empty() {
                    evidence = format!("{evidence}; note: {note}");
                }
                field(document, DocumentField::Ended, evidence)
            }
            Open::NoTopic { document, summary } => {
                field(document, DocumentField::Topics, cut(summary))
            }
            Open::FactTopic { fact, topic } => {
                let evidence = format!("{topic} names no topic: the one topic of the fact");
                field(fact, DocumentField::Topics, evidence)
            }
            Open::Name {
                document,
                name,
                why,
            } => field(document, DocumentField::Name, format!("{name}: {why}")),
            Open::FormerName {
                document,
                name,
                why,
            } => {
                let evidence = format!("{name}: {why}; \"\" leaves it out");
                field(document, DocumentField::FormerName(name.clone()), evidence)
            }
            Open::Summary { document } => {
                field(document, DocumentField::Summary, "one line".to_owned())
            }
            Open::RecheckSummary { fact, touching } => {
                let evidence =
                    format!("one line for the followup that rechecks it, touching {touching}");
                field(fact, DocumentField::RecheckSummary, evidence)
            }
            Open::Entry { followup, entry } => {
                let evidence = format!("{entry} names no entry; \"\" for none");
                field(followup, DocumentField::Entry, evidence)
            }
            Open::Touching { document, topic } => {
                let evidence =
                    format!("{topic} names no topic that is not ended; \"\" for no trigger");
                field(document, DocumentField::Touching, evidence)
            }
            Open::EndedTopic {
                document,
                through,
                topic,
            } => ended_topic_offers(document, through, topic),
            Open::TopicCase {
                document,
                through,
                name,
                topics,
            } => topic_case_offers(document, through, name, topics),
            Open::Cycle { .. } | Open::Answer { .. } => Vec::new(),
        }
    }
}
