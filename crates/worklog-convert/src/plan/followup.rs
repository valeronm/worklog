use worklog::domain::id::DocumentId;
use worklog::domain::schema::{Content, Date, Ending, Followup, Reason, Record, Trigger};

use super::Frame;
use super::chain::Settled;
use super::filing::Filed;
use super::resolve::{Named, Unsettled};
use super::walk::{
    Asked, Dates, Mapped, Mapper, Rechecked, Step, Walked, holds, list, push_once, scalar,
};
use crate::decisions::{Answer, EntryAnswer, TouchingAnswer, Unasked};
use crate::old::{self, FollowupState, Kind, Operation, Recheck, dated, with_note};
use crate::question::{Open, Through};

struct Followups<'c, 'a> {
    own: Settled<'c, 'a>,
    created: Option<Date>,
    summary: String,
    // The topic the last version touches, which an answer stands in for in every version
    // that touches it.
    touched: Option<DocumentId>,
}

fn touched_topic(frame: &Frame<'_, '_>, version: &old::Version) -> Option<DocumentId> {
    let Ok(Recheck::Touching(name)) = Recheck::parse(scalar(version, "recheck")?) else {
        return None;
    };
    match frame.named(&name) {
        Named::One(found) => Some(found.id),
        Named::None | Named::Several(_) | Named::Unsettled => None,
    }
}

#[derive(Clone)]
struct State {
    // The body as stored, which a closing appends its note to.
    body: String,
    closing: Option<Ending>,
}

// The store's writer appends a note trimmed, on a line of its own, and writes none for no
// note; text it could not have appended is no note.
fn noted(before: &str, body: &str) -> Option<String> {
    let note = body.strip_prefix(before)?.trim();
    let appended = !note.is_empty() && with_note(before.to_owned(), Some(note)) == body;
    appended.then(|| note.to_owned())
}

pub(crate) struct Touch<'c, 'a> {
    // The document whose last record the trigger is held to.
    pub(crate) own: Settled<'c, 'a>,
    // Whether the name the store holds is one of the last record.
    pub(crate) last: bool,
}

pub(crate) fn touched(
    frame: &Frame<'_, '_>,
    touch: &Touch<'_, '_>,
    topic: &str,
) -> Result<Option<Trigger>, ()> {
    let document = touch.own.key.clone();
    let through = Through::Touching;
    let held = holds(Some(touch.own), Asked::TOUCHING);
    let open = match frame.named(topic) {
        Named::One(found) if found.ended && held && touch.last => Open::EndedTopic {
            document,
            through,
            topic: found.name,
        },
        Named::One(found) => return Ok(Some(Trigger::Touching(found.id))),
        Named::None => Open::Touching {
            document,
            topic: topic.to_owned(),
        },
        Named::Several(topics) => Open::TopicCase {
            document,
            through,
            name: topic.to_owned(),
            topics,
        },
        Named::Unsettled => return Err(()),
    };
    let (at, answer) = frame.text(open).ok_or(())?;
    let answer = match TouchingAnswer::of(answer) {
        TouchingAnswer::NoTrigger => return Ok(None),
        TouchingAnswer::Topic(answer) => answer,
    };
    let found = frame.answered_unended(&at, answer, held);
    Ok(Some(Trigger::Touching(found.ok_or(())?.id)))
}

pub(crate) fn reason_of(state: FollowupState) -> Option<Reason> {
    match state {
        FollowupState::Open => None,
        FollowupState::Done => Some(Reason::Done),
        FollowupState::Dropped => Some(Reason::Dropped),
    }
}

impl<'c, 'a> Followups<'c, 'a> {
    fn filed(&self) -> Filed<'c, 'a> {
        Filed {
            own: self.own,
            asked: Asked::FOLLOWUP_TOPICS,
        }
    }

    fn entry(&self, frame: &Frame<'_, '_>, step: &Step<'_>) -> Result<Option<DocumentId>, ()> {
        let Some(slug) = scalar(step.source.version, "entry") else {
            frame.missing(&step.source, "entry");
            return Err(());
        };
        match frame.resolver.document(Kind::Entry, slug) {
            Ok(Some(found)) => return Ok(Some(found.id.clone())),
            Ok(None) => {}
            Err(Unsettled) => return Err(()),
        }
        let open = Open::Entry {
            followup: self.own.key.clone(),
            entry: slug.to_owned(),
        };
        let (at, answer) = frame.text(open).ok_or(())?;
        let answer = match EntryAnswer::of(answer) {
            EntryAnswer::NoEntry => return Ok(None),
            EntryAnswer::Entry(answer) => answer,
        };
        match frame.resolver.document(Kind::Entry, answer) {
            Ok(Some(found)) => return Ok(Some(found.id.clone())),
            Ok(None) => (frame.questions).ask(at.refused(format!("{answer} names no entry"))),
            Err(Unsettled) => {}
        }
        Err(())
    }

    fn trigger(&self, frame: &Frame<'_, '_>, step: &Step<'_>) -> Result<Option<Trigger>, ()> {
        let Some(text) = scalar(step.source.version, "recheck") else {
            return Ok(None);
        };
        match frame.recheck(&step.source, text).ok_or(())? {
            Rechecked::Touching(topic) => {
                let last = step.head
                    || (self.touched.is_some()
                        && touched_topic(frame, step.source.version) == self.touched);
                let touch = Touch {
                    own: self.own,
                    last,
                };
                touched(frame, &touch, &topic)
            }
            Rechecked::On { on, why } => Ok(Some(Trigger::LookAgain { on, why })),
        }
    }

    fn topics(
        &self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        trigger: Option<&Trigger>,
    ) -> Option<Vec<DocumentId>> {
        let filed = self.filed().topics(frame, step);
        if !matches!(trigger, Some(Trigger::Touching(_))) {
            self.asks_for_a_list(frame, step);
        }
        let mut topics = filed?;
        if let Some(Trigger::Touching(touched)) = trigger {
            push_once(&mut topics, touched.clone());
        }
        if !topics.is_empty() {
            return Some(topics);
        }
        let unfiled = frame.decisions.unfiled(self.own.key, &self.summary);
        frame.questions.ask(unfiled);
        None
    }

    // A version its tags file under no topic, which touches none, is asked about, and the
    // file's list of topics is then one a question asked for.
    fn asks_for_a_list(&self, frame: &Frame<'_, '_>, step: &Step<'_>) {
        let drops = |tag: &String| {
            let read = frame
                .decisions
                .unasked(Unasked::DropsTag(self.own.key, tag));
            read.first().and_then(Answer::text) == Some("")
        };
        if list(step.source.version, "tags").iter().all(drops)
            && let Some(listed) = frame.overrides.borrow_mut().get_mut(self.own.key)
        {
            listed.asked = true;
        }
    }

    fn state(frame: &Frame<'_, '_>, step: &Step<'_>) -> Option<FollowupState> {
        let Some(text) = scalar(step.source.version, "state") else {
            frame.missing(&step.source, "state");
            return None;
        };
        let state = FollowupState::parse(text);
        if state.is_none() {
            frame.refused(&step.source, "state", text, "is not open, done or dropped");
        }
        state
    }
}

struct Closed {
    // `None` when the day of the closing is not read.
    ending: Option<Ending>,
    body: String,
}

// The store's `done` and `drop` are the operations that append a note to the body.
fn closes(operation: &str) -> bool {
    [Operation::Done, Operation::Drop]
        .iter()
        .any(|closing| closing.as_str() == operation)
}

// A note is taken from the one body a closing operation followed, and an ending of the
// other reason hands its note on to a version that appends none.
fn closing_after(
    version: &old::Version,
    parent: &Mapped<State>,
    reason: Reason,
    on: Option<Date>,
) -> Closed {
    let body = version.body.as_str();
    let same = parent.state.body == body;
    let whole = if same { &parent.body } else { body }.to_owned();
    let before = parent.state.closing.as_ref();
    if let Some(kept) = before.filter(|before| before.reason == reason) {
        return Closed {
            ending: Some(kept.clone()),
            body: whole,
        };
    }
    let appended = (closes(&version.operation))
        .then(|| noted(&parent.state.body, body))
        .flatten();
    let (note, body) = match appended {
        Some(note) => (note, parent.body.clone()),
        None => (
            before.map(|before| before.note.clone()).unwrap_or_default(),
            whole,
        ),
    };
    let ending = on.map(|on| Ending {
        reason,
        on,
        by: None,
        note,
    });
    Closed { ending, body }
}

// A version that follows several takes the one ending its parents hold for its reason, and
// with none or several of them it ends on its own day and keeps its body whole.
fn closing_over(
    version: &old::Version,
    parents: &[&Mapped<State>],
    reason: Reason,
    on: Option<Date>,
) -> Closed {
    let ended = |parent: &&&Mapped<State>| {
        (parent.state.closing.as_ref()).is_some_and(|closing| closing.reason == reason)
    };
    let ended: Vec<&&Mapped<State>> = parents.iter().filter(ended).collect();
    let one = match ended.as_slice() {
        [first, rest @ ..] => {
            let alike = |other: &&&Mapped<State>| other.state.closing == first.state.closing;
            first
                .state
                .closing
                .as_ref()
                .filter(|_| rest.iter().all(alike))
        }
        [] => None,
    };
    let Some(kept) = one else {
        let ending = on.map(|on| Ending {
            reason,
            on,
            by: None,
            note: String::new(),
        });
        let body = version.body.clone();
        return Closed { ending, body };
    };
    let same = ended
        .iter()
        .find(|parent| parent.state.body == version.body);
    Closed {
        ending: Some(kept.clone()),
        body: same.map_or_else(|| version.body.clone(), |parent| parent.body.clone()),
    }
}

fn closing(
    version: &old::Version,
    parents: &[&Mapped<State>],
    reason: Reason,
    on: Option<Date>,
) -> Closed {
    match parents {
        [parent] => closing_after(version, parent, reason, on),
        _ => closing_over(version, parents, reason, on),
    }
}

impl Mapper for Followups<'_, '_> {
    type State = State;

    fn map(
        &mut self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        dates: Dates,
        parents: &[&Mapped<State>],
    ) -> Mapped<State> {
        let version = step.source.version;
        let entry = self.entry(frame, step);
        let trigger = self.trigger(frame, step);
        let topics = match &trigger {
            Ok(trigger) => self.topics(frame, step, trigger.as_ref()),
            Err(()) => None,
        };
        let summary = frame.summary(self.own.key, version);
        let state = Followups::state(frame, step);
        let closed = match state.map(reason_of) {
            Some(Some(reason)) => closing(version, parents, reason, dates.on),
            _ => Closed {
                ending: None,
                body: version.body.clone(),
            },
        };
        let content = || {
            Some(Content::Followup(Followup {
                created: self.created?,
                topics: topics?,
                entry: entry.ok()?,
                about: None,
                trigger: trigger.ok()?,
                summary: summary?,
            }))
        };
        let ending = match state.map(reason_of) {
            None => None,
            Some(None) => Some(None),
            Some(Some(_)) => closed.ending.clone().map(Some),
        };
        let record = content()
            .zip(ending)
            .map(|(content, ending)| Record { content, ending });
        Mapped {
            record,
            body: closed.body,
            state: State {
                body: version.body.clone(),
                closing: closed.ending,
            },
        }
    }
}

pub(crate) fn versions<'c, 'a>(frame: &Frame<'a, '_>, own: Settled<'c, 'a>) -> Walked<'c, 'a> {
    let mut heads: Vec<&old::Version> = own.heads().map(|head| head.source.version).collect();
    heads.sort_by(|one, other| one.id.cmp(&other.id));
    let origin = &frame.store.documents[own.chain.origin];
    let day = dated(Kind::Followup, &origin.slug).map(|(day, _)| day);
    let created = day.and_then(|day| Date::parse(&day).ok());
    if let (None, Some(first)) = (created, own.chain.steps.first()) {
        let why = "opens with no calendar day";
        frame.refused(&first.source, "slug", &origin.slug, why);
    }
    let summary = heads.first().and_then(|head| scalar(head, "summary"));
    let touched = match heads.as_slice() {
        [head] => touched_topic(frame, head),
        _ => None,
    };
    let mut followups = Followups {
        own,
        created,
        summary: summary.unwrap_or_default().to_owned(),
        touched,
    };
    frame.walk(own, &mut followups)
}
