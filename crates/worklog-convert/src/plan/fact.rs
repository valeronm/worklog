use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{Content, Date, Fact, Record};

use super::Frame;
use super::chain::Settled;
use super::name::{Carrier, Holder, Naming, Scope, naming};
use super::resolve::{Named, Resolved};
use super::walk::{Asked, Dates, Mapped, Mapper, Step, Walked, holds, refuses, scalar};
use crate::decisions::Answer;
use crate::question::{Open, Through};

struct Facts<'c, 'a> {
    own: Settled<'c, 'a>,
}

#[derive(Clone)]
struct State {
    naming: Option<Naming>,
    summary: Option<String>,
    changed: Option<Date>,
}

fn segments(slug: &str) -> (&str, &str) {
    slug.split_once('/').unwrap_or(("", slug))
}

pub(crate) enum Refiled {
    Stays,
    // The topic its last slug names is ended, and a question stands before another.
    Asked,
    To { from: Resolved, to: Resolved },
}

impl Facts<'_, '_> {
    // A version keeps the topic its slug names, ended or not; an answer stands in for a
    // name that resolves to no topic.
    fn topic(&self, frame: &Frame<'_, '_>, segment: &str) -> Option<DocumentId> {
        let open = match frame.resolver.fact_topic(segment) {
            Named::One(found) => return Some(found.id),
            Named::None => Open::FactTopic {
                fact: self.own.key.clone(),
                topic: segment.to_owned(),
            },
            Named::Several(topics) => Open::TopicCase {
                document: self.own.key.clone(),
                through: Through::FactTopic,
                name: segment.to_owned(),
                topics,
            },
            Named::Unsettled => return None,
        };
        let (at, name) = Facts::one_topic(frame, open)?;
        let held = holds(Some(self.own), Asked::FACT_TOPIC);
        let named = frame.answered_unended(&at, name, held)?;
        Some(named.id)
    }

    fn one_topic<'d>(frame: &Frame<'_, 'd>, open: Open) -> Option<(Answer<'d>, &'d str)> {
        let at = frame.answer(open)?;
        match at.one_topic() {
            Ok(name) => Some((at, name)),
            Err(open) => {
                frame.questions.ask(open);
                None
            }
        }
    }

    // Only the topic the last slug names is held to not being ended, and only for a fact
    // that is not ended itself.
    fn refiled(&self, frame: &Frame<'_, '_>) -> Refiled {
        let last = segments(&self.own.key.slug).0;
        let from = match frame.resolver.fact_topic(last) {
            Named::One(found) if refuses(Some(self.own), Asked::FACT_TOPIC, &found) => found,
            Named::One(_) | Named::None | Named::Several(_) | Named::Unsettled => {
                return Refiled::Stays;
            }
        };
        let open = Open::EndedTopic {
            document: self.own.key.clone(),
            through: Through::FactTopic,
            topic: from.name.clone(),
        };
        let Some((at, name)) = Facts::one_topic(frame, open) else {
            return Refiled::Asked;
        };
        match frame.answered_unended(&at, name, true) {
            Some(to) => Refiled::To { from, to },
            None => Refiled::Asked,
        }
    }
}

fn latest(parents: &[&Mapped<State>]) -> Option<Date> {
    let mut latest = None;
    for parent in parents {
        let changed = parent.state.changed?;
        latest = Some(latest.map_or(changed, |latest: Date| latest.max(changed)));
    }
    latest
}

impl Mapper for Facts<'_, '_> {
    type State = State;

    fn map(
        &mut self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        dates: Dates,
        parents: &[&Mapped<State>],
    ) -> Mapped<State> {
        let version = step.source.version;
        let topic = self.topic(frame, segments(step.slug()).0);
        let name = frame.names.name(self.own.index, step.place);
        let before: Vec<Option<&Naming>> = (parents.iter())
            .map(|parent| parent.state.naming.as_ref())
            .collect();
        let naming = naming(name, &before);
        let idea = match scalar(version, "idea") {
            None => Some(false),
            Some("true") => Some(true),
            Some(other) => {
                frame.refused(&step.source, "idea", other, "is not `true`");
                None
            }
        };
        let verified = scalar(version, "verified").map(|day| {
            let read = Date::parse(day).ok();
            if read.is_none() {
                frame.refused(&step.source, "verified", day, "is not a day");
            }
            read
        });
        let summary = frame.summary(self.own.key, version);
        let body = version.body.clone();
        // A version that joins several heads and holds the text of one of them changed
        // nothing.
        let same = |parent: &&Mapped<State>| parent.state.summary == summary && parent.body == body;
        let changed = if parents.iter().any(same) {
            latest(parents)
        } else {
            dates.on
        };
        let confirmed = match verified {
            Some(verified) => verified.zip(changed).map(|(day, changed)| day.max(changed)),
            None => changed,
        };
        let content = || {
            let Naming { name, former } = naming.clone()?;
            Some(Content::Fact(Fact {
                name,
                former_names: former,
                topic: topic?,
                created: dates.created?,
                confirmed: confirmed?,
                idea: idea?,
                summary: summary.clone()?,
            }))
        };
        let record = content().map(|content| Record {
            content,
            ending: None,
        });
        Mapped {
            record,
            body,
            state: State {
                naming,
                summary,
                changed,
            },
        }
    }
}

pub(crate) fn carrier(frame: &Frame<'_, '_>, own: Settled<'_, '_>) -> Carrier {
    let facts = Facts { own };
    let name = |document: usize| segments(&frame.store.documents[document].slug).1.to_owned();
    // A fact holds its name against the facts of the topic it has at the end.
    let scope = || {
        let under = match facts.refiled(frame) {
            Refiled::To { to, .. } => to.id,
            Refiled::Stays | Refiled::Asked => facts.topic(frame, segments(&own.key.slug).0)?,
        };
        (!own.ended).then_some(Scope::Facts(under))
    };
    Carrier {
        holder: Holder::Chain(own.index),
        scope: scope(),
        segments: own.chain.documents().map(name).collect(),
        head: own.place(),
    }
}

pub(crate) fn refiled(frame: &Frame<'_, '_>, own: Settled<'_, '_>) -> Refiled {
    Facts { own }.refiled(frame)
}

pub(crate) fn versions<'c, 'a>(frame: &Frame<'a, '_>, own: Settled<'c, 'a>) -> Walked<'c, 'a> {
    frame.walk(own, &mut Facts { own })
}
