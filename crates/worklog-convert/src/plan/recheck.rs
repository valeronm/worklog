use std::collections::BTreeMap;

use worklog::domain::id::DocumentId;
use worklog::domain::schema::{Content, Followup, KindOf, Record, Trigger};

use super::chain::Settled;
use super::followup::{Touch, touched};
use super::id::generated_id;
use super::stamp::day_of;
use super::walk::{Rechecked, Role, Step, Walked, generated, push_once, scalar};
use super::{Document, Frame};
use crate::key::{DocumentField, Key};
use crate::question::Open;

const RECHECK: &str = "recheck";

// A tombstone and a version that follows several are where a run of versions carrying
// one recheck starts.
fn set_at<'s, 'a>(steps: &'s [Step<'a>], last: &'s Step<'a>, text: &str) -> &'s Step<'a> {
    let by_key: BTreeMap<&str, &Step<'a>> = steps.iter().map(|step| (step.key(), step)).collect();
    let mut first = last;
    while let [parent] = first.parents.as_slice()
        && let Some(before) = by_key.get(parent.as_str())
        && before.role != Role::Removed
        && scalar(before.source.version, RECHECK) == Some(text)
    {
        first = before;
    }
    first
}

pub(crate) fn followup<'a>(
    frame: &Frame<'a, '_>,
    own: Settled<'_, 'a>,
    walked: &Walked<'_, 'a>,
    topic: Option<DocumentId>,
) -> Option<Document<'a>> {
    let chain = own.chain;
    if own.ended {
        return None;
    }
    let (last, _) = walked.head()?;
    let source = &last.source;
    let text = scalar(source.version, RECHECK)?;
    let key = own.key;
    let mut answered = None;
    let (trigger, summary) = match frame.recheck(source, text)? {
        Rechecked::On { on, why } => {
            let summary = why.clone();
            (Ok(Some(Trigger::LookAgain { on, why })), Some(summary))
        }
        Rechecked::Touching(name) => {
            let touch = Touch { own, last: true };
            let summary = frame.text(Open::RecheckSummary {
                fact: key.clone(),
                touching: name.clone(),
            });
            let summary = summary.map(|(_, summary)| summary.to_owned());
            answered = Some(Key::document(key, DocumentField::RecheckSummary));
            (touched(frame, &touch, &name), summary)
        }
    };
    let first = set_at(&chain.steps, last, text);
    let made_at = walked.each().find(|(step, _)| step.key() == first.key())?;
    let created = made_at.1.stamp.as_ref().and_then(day_of);
    let content = || {
        let mut topics = vec![topic?];
        let trigger = trigger.ok()?;
        if let Some(Trigger::Touching(touched)) = &trigger {
            push_once(&mut topics, touched.clone());
        }
        Some(Content::Followup(Followup {
            created: created?,
            topics,
            entry: None,
            about: Some(chain.id.clone()),
            trigger,
            summary: summary?,
        }))
    };
    let record = content().map(|content| Record {
        content,
        ending: None,
    });
    let version = generated(made_at, "new", Vec::new(), record, answered);
    Some(Document {
        id: generated_id(&format!(
            "recheck\0{}",
            frame.store.documents[chain.origin].slug
        )),
        kind: KindOf::Followup,
        versions: vec![version],
    })
}
