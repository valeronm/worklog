//! What a store being converted becomes: every document of the new store with its versions,
//! and everything that stands between the store and that result.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{KindOf, Record};
use worklog_next::domain::version::{Links, Stamp};

use crate::decisions::Decisions;
use crate::key::{DocumentField, DocumentKey, Folded, Key};
use crate::old;
use crate::question::{Authored, Blocked, Open, Overridden, Question, kind_of};

mod authored;
mod chain;
mod check;
mod claim;
mod entry;
mod fact;
mod filing;
mod followup;
mod id;
mod links;
mod name;
mod recheck;
mod resolve;
mod stamp;
#[cfg(test)]
mod tests;
mod topic;
mod walk;

pub(crate) use check::{Whole, Written, referenced};
pub(crate) use id::created_topic_id;
#[cfg(test)]
pub(crate) use id::{document_id, generated_id};
pub(crate) use walk::{head, list};

use authored::Authoring;
use chain::Chains;
use name::Names;
use resolve::Resolver;
use stamp::Machines;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stored<'a> {
    pub(crate) document: &'a old::Document,
    pub(crate) version: &'a old::Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Source<'a> {
    pub(crate) document: &'a old::Document,
    /// For the one version a rename becomes, the version it moved under the new slug.
    pub(crate) version: &'a old::Version,
    /// The tombstone a rename left under the old slug, for the one version it becomes.
    pub(crate) stone: Option<Stored<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Origin<'a> {
    /// A version of the document in the store, or the two versions a rename wrote.
    Converted(Source<'a>),
    /// A version of another document in the store, whose stamp and machine it takes.
    Generated {
        cause: Stored<'a>,
        /// The key of the answer its summary is, where the file gave it.
        answered: Option<Key>,
    },
    /// The decision file alone; its stamp and machine are those of `[conversion]`.
    Authored {
        /// The key of the answer its summary is; `None` where it carries the summary and
        /// the body of the version it follows.
        answered: Option<Key>,
    },
}

impl<'a> Origin<'a> {
    pub(crate) fn source(&self) -> Option<Source<'a>> {
        match self {
            Origin::Converted(source) => Some(*source),
            Origin::Generated { .. } | Origin::Authored { .. } => None,
        }
    }
}

pub(crate) const AUTHORED: &str = "conversion";

// The body of a version made from no text of the store.
const BODY: &str = "\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Version<'a> {
    /// Names the version among the versions of its document in every run: the id of the
    /// version of the store it was converted from or generated beside, and `AUTHORED` for
    /// the one the conversion authors.
    pub(crate) key: String,
    /// The keys of the versions it follows, each the key of a version before it in the
    /// document, in the order the store names them.
    pub(crate) parents: Vec<String>,
    /// The ids it names as parents that are no version of the document; never empty
    /// without a blocked question among the plan's.
    pub(crate) outside: Vec<String>,
    /// `None` when the write time is not read, with the question asked.
    pub(crate) stamp: Option<Stamp>,
    /// The topic of the machine that wrote it; `None` with the question asked.
    pub(crate) machine: Option<DocumentId>,
    pub(crate) change: String,
    /// `None` when a question stands between the version and a field the new schema
    /// requires.
    pub(crate) record: Option<Record>,
    pub(crate) body: String,
    /// The document each link of the summary and the body named in the store, under the
    /// link's text as it stands here; `None` for a link that named none.
    pub(crate) links: Links,
    pub(crate) origin: Origin<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Document<'a> {
    pub(crate) id: DocumentId,
    pub(crate) kind: KindOf,
    /// A parent before its child.
    pub(crate) versions: Vec<Version<'a>>,
}

impl<'a> Document<'a> {
    pub(crate) fn head(&self) -> Option<&Version<'a>> {
        walk::head(&self.versions)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Planned<'a> {
    /// By kind in the order the new schema lists the kinds, then by id.
    pub(crate) documents: Vec<Document<'a>>,
    /// In order, each once.
    pub(crate) questions: Vec<Question>,
    /// What the versions of origin `Authored` do, in order, each once.
    pub(crate) authored: Vec<Authored>,
    /// The lists of topics the file gives in place of tags where no question asked for
    /// one, by document.
    pub(crate) overridden: Vec<Overridden>,
    /// The answers of the file the plan asked for nowhere and the placeholders no open
    /// question asks for, in the order of the file.
    pub(crate) unused: Vec<Key>,
}

/// A plan with no question and no unused decision, which only `plan` makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Complete<'a> {
    planned: Planned<'a>,
    documents: Vec<Whole>,
}

impl<'a> Complete<'a> {
    pub(crate) fn planned(&self) -> &Planned<'a> {
        &self.planned
    }

    /// The documents of the plan as they are written, in the plan's order.
    pub(crate) fn documents(&self) -> &[Whole] {
        &self.documents
    }

    #[cfg(test)]
    pub(crate) fn unchecked(planned: Planned<'a>) -> Complete<'a> {
        let whole = |document| check::whole(document).unwrap_or_else(|lacks| panic!("{lacks:?}"));
        let documents = planned.documents.iter().map(whole).collect();
        Complete { planned, documents }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome<'a> {
    Complete(Complete<'a>),
    /// Holds a question or an unused decision.
    Open(Planned<'a>),
}

impl<'a> Outcome<'a> {
    pub(crate) fn planned(&self) -> &Planned<'a> {
        match self {
            Outcome::Complete(complete) => complete.planned(),
            Outcome::Open(planned) => planned,
        }
    }
    #[cfg(test)]
    pub(crate) fn into_planned(self) -> Planned<'a> {
        match self {
            Outcome::Complete(complete) => complete.planned,
            Outcome::Open(planned) => planned,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Questions(RefCell<BTreeSet<Question>>);

impl Questions {
    pub(crate) fn ask(&self, question: impl Into<Question>) {
        self.0.borrow_mut().insert(question.into());
    }

    pub(crate) fn asked(&self) -> Vec<Question> {
        self.0.borrow().iter().cloned().collect()
    }
}

pub(crate) struct Frame<'a, 'd> {
    pub(crate) store: &'a old::Store,
    pub(crate) decisions: &'d Decisions,
    pub(crate) chains: Chains<'a>,
    pub(crate) resolver: Resolver,
    pub(crate) machines: Machines,
    pub(crate) names: Names,
    pub(crate) questions: Questions,
    // The documents carrying each tag the file does not answer.
    pub(crate) tags: RefCell<BTreeMap<Folded, BTreeSet<DocumentKey>>>,
    // The machine names no topic and no answer of `[machines]` holds, with what was found
    // to carry each.
    pub(crate) unknown: RefCell<BTreeMap<String, stamp::Unknown>>,
    pub(crate) overrides: RefCell<BTreeMap<DocumentKey, Override>>,
}

#[derive(Debug, Default)]
pub(crate) struct Override {
    pub(crate) topics: Vec<String>,
    // Whether a version with no topic of its own asked for the list.
    pub(crate) asked: bool,
}

impl<'a, 'd> Frame<'a, 'd> {
    pub(crate) fn build(store: &'a old::Store, decisions: &'d Decisions) -> Frame<'a, 'd> {
        let questions = Questions::default();
        for problem in &store.problems {
            questions.ask(problem.clone());
        }
        let chains = Chains::build(store, &questions);
        let mut frame = Frame {
            store,
            decisions,
            resolver: Resolver::build(store, &chains, decisions),
            machines: Machines::build(store, &chains),
            chains,
            names: Names::default(),
            questions,
            tags: RefCell::default(),
            unknown: RefCell::default(),
            overrides: RefCell::default(),
        };
        frame.names = Names::settle(&frame, &frame.carriers());
        frame
    }
}

impl Frame<'_, '_> {
    fn overridden(&self) -> Vec<Overridden> {
        let overrides = self.overrides.borrow();
        let unasked = overrides.iter().filter(|(_, listed)| !listed.asked);
        let overridden = unasked.map(|(document, listed)| Overridden {
            document: document.to_string(),
            topics: listed.topics.clone(),
        });
        overridden.collect()
    }
}

// A placeholder says an answer is still owed, so one that no open question asks for is
// listed with the answers no question asked for.
fn unasked_placeholders(decisions: &Decisions, questions: &[Question]) -> Vec<Key> {
    let mut offered: BTreeSet<Key> = BTreeSet::new();
    for question in questions {
        let Question::Open(open) = question else {
            continue;
        };
        offered.extend(open.offers().into_iter().map(|offer| offer.key.of_line()));
        if let Open::Tag { tag, carriers } = open {
            let own = |carrier| Key::document(carrier, DocumentField::Tag(tag.clone()));
            offered.extend(carriers.iter().map(own));
        }
        if let Open::Machine { documents, .. } = open {
            let own = |entry| Key::document(entry, DocumentField::Machine);
            offered.extend(documents.iter().map(own));
        }
    }
    let unasked = (decisions.placeholders().iter()).filter(|key| !offered.contains(key));
    unasked.cloned().collect()
}

fn kind_order(kind: KindOf) -> usize {
    (KindOf::ALL.iter())
        .position(|known| *known == kind)
        .unwrap_or(KindOf::ALL.len())
}

// A plan with no blocked question is written as one whose every parent is a version of its
// document.
pub(crate) fn outside_unasked(documents: &[Document<'_>], questions: &Questions) {
    let blocked = |question: &Question| matches!(question, Question::Blocked(_));
    if questions.asked().iter().any(blocked) {
        return;
    }
    for version in documents.iter().flat_map(|document| &document.versions) {
        if let (Some(source), false) = (version.origin.source(), version.outside.is_empty()) {
            questions.ask(Blocked::Waiting {
                document: chain::key_of(source.document),
                missing: version.outside.clone(),
            });
        }
    }
}

/// Reads no file and no clock, so the same store and file give the same plan.
pub(crate) fn plan<'a>(store: &'a old::Store, decisions: &Decisions) -> Outcome<'a> {
    let frame = Frame::build(store, decisions);
    let mut documents = Vec::new();
    let mut authoring = Authoring::default();
    for own in frame.chains.each_settled() {
        let kind = own.chain.kind;
        let walked = match kind {
            old::Kind::Topic => topic::versions(&frame, own),
            old::Kind::Fact => fact::versions(&frame, own),
            old::Kind::Entry => entry::versions(&frame, own),
            old::Kind::Followup => followup::versions(&frame, own),
        };
        match kind {
            old::Kind::Topic => {
                authoring.edges(&frame, own, &walked);
                documents.extend(claim::documents(&frame, own, &walked, &mut authoring));
            }
            old::Kind::Fact => {
                let topic = authoring.refile(&frame, own, &walked);
                documents.extend(recheck::followup(&frame, own, &walked, topic));
            }
            old::Kind::Entry | old::Kind::Followup => {}
        }
        authoring.rename(&frame, own, &walked);
        documents.push(Document {
            id: own.chain.id.clone(),
            kind: kind_of(kind),
            versions: walked.versions,
        });
    }
    authoring.created(&frame);
    let authored = authoring.listed.iter().cloned().collect();
    authoring.write(&frame, &mut documents);
    frame.ask_machines();
    documents.sort_by(|a, b| (kind_order(a.kind), &a.id).cmp(&(kind_order(b.kind), &b.id)));
    frame.ask_tags();
    links::rewrite(&frame, &mut documents);
    outside_unasked(&documents, &frame.questions);
    check::cycles(&documents, &frame.questions);
    let held = check::unheld(&documents, &frame.questions);
    let questions = frame.questions.asked();
    let mut unused = decisions.unused();
    unused.extend(unasked_placeholders(decisions, &questions));
    unused.sort();
    let planned = Planned {
        documents,
        questions,
        authored,
        overridden: frame.overridden(),
        unused,
    };
    match held.filter(|_| planned.unused.is_empty()) {
        Some(documents) => Outcome::Complete(Complete { planned, documents }),
        None => Outcome::Open(planned),
    }
}
