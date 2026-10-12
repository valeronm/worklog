use std::collections::BTreeSet;

use worklog::domain::id::DocumentId;

use super::Frame;
use super::chain::Settled;
use super::walk::{Asked, Step, holds, list, push_once, refuses};
use crate::decisions::{Answer, TagAnswer, Unasked};
use crate::key::{DocumentKey, Folded};
use crate::question::{Open, Through};

pub(crate) struct Filed<'c, 'a> {
    pub(crate) own: Settled<'c, 'a>,
    // The key the document names its topics under.
    pub(crate) asked: Asked,
}

impl Filed<'_, '_> {
    // An answer of the file stands in every version of the document, its head included.
    pub(crate) fn answered(
        &self,
        frame: &Frame<'_, '_>,
        at: &Answer<'_>,
        name: &str,
    ) -> Option<DocumentId> {
        let held = holds(Some(self.own), self.asked);
        let found = frame.answered_unended(at, name, held)?;
        Some(found.id)
    }

    // A tag answered with an ended topic stops no version but the head of a document that
    // is not ended and holds its topics open.
    fn tagged(
        &self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        tag: &str,
    ) -> Result<Option<DocumentId>, ()> {
        let folded = Folded::new(tag);
        let key = self.own.key;
        let Some((at, answer)) = frame.tag(key, tag) else {
            let mut tags = frame.tags.borrow_mut();
            tags.entry(folded).or_default().insert(key.clone());
            return Err(());
        };
        let name = match answer {
            TagAnswer::Dropped => return Ok(None),
            TagAnswer::Topic(name) => name,
        };
        let found = frame.answered_topic(&at, name).ok_or(())?;
        if refuses(Some(self.own), self.asked, &found) && step.head {
            frame.questions.ask(Open::EndedTopic {
                document: key.clone(),
                through: Through::Tag(folded),
                topic: found.name,
            });
            return Err(());
        }
        Ok(Some(found.id))
    }

    fn listed(
        &self,
        frame: &Frame<'_, '_>,
        at: &Answer<'_>,
        names: &[String],
    ) -> Option<Vec<DocumentId>> {
        let mut topics = Some(Vec::new());
        for name in names {
            match (self.answered(frame, at, name), &mut topics) {
                (Some(id), Some(topics)) => push_once(topics, id),
                (Some(_), None) => {}
                (None, _) => topics = None,
            }
        }
        topics
    }

    // The file's list stands for the tags of every version of the document.
    pub(crate) fn topics(&self, frame: &Frame<'_, '_>, step: &Step<'_>) -> Option<Vec<DocumentId>> {
        let key = self.own.key;
        let listed = frame.decisions.unasked(Unasked::Topics(key)).pop();
        if let Some((at, names)) = listed.and_then(|at| Some((at, at.topics()?))) {
            let mut overrides = frame.overrides.borrow_mut();
            overrides.entry(key.clone()).or_default().topics = names.to_vec();
            drop(overrides);
            return self.listed(frame, &at, names);
        }
        let mut seen = BTreeSet::new();
        let mut topics = Some(Vec::new());
        for tag in list(step.source.version, "tags") {
            if !seen.insert(Folded::new(tag)) {
                continue;
            }
            match (self.tagged(frame, step, tag), &mut topics) {
                (Ok(Some(id)), Some(topics)) => push_once(topics, id),
                (Ok(_), _) => {}
                (Err(()), _) => topics = None,
            }
        }
        topics
    }
}

impl<'d> Frame<'_, 'd> {
    // A document's own answer for a tag stands before the one of `[tags]`.
    fn tag(&self, carrier: &DocumentKey, tag: &str) -> Option<(Answer<'d>, TagAnswer<'d>)> {
        let read = |at: Answer<'d>| Some((at, TagAnswer::of(at.text()?)));
        let own = self.decisions.unasked(Unasked::Tag(carrier, tag)).pop();
        own.and_then(read).or_else(|| {
            let open = Open::tag(tag, std::slice::from_ref(carrier));
            self.decisions.answer(&open).and_then(read)
        })
    }

    // One question a tag, with every document found carrying it unanswered.
    pub(crate) fn ask_tags(&self) {
        for (tag, carriers) in self.tags.borrow().iter() {
            let carriers: Vec<DocumentKey> = carriers.iter().cloned().collect();
            self.questions.ask(Open::tag(tag.as_str(), &carriers));
        }
    }
}
