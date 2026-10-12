use std::collections::BTreeSet;

use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::{Content, Record, Topic};

use super::Frame;
use super::chain::Settled;
use super::name::{Carrier, Holder, Naming, Scope, naming};
use super::resolve::{Named, Resolved};
use super::walk::{Asked, Dates, Mapped, Mapper, Step, Walked, list, push_once, refuses};
use crate::decisions::Answer;
use crate::key::{Edges, Folded, Key, Placement};
use crate::question::Open;

struct Topics<'c, 'a> {
    own: Settled<'c, 'a>,
    // The ended topics the last version includes, each of which a place alone leaves to
    // the ended-topic question.
    ended: BTreeSet<DocumentId>,
    // The topics the store's reader leaves every item of the last version that names
    // them out of the list.
    unread: BTreeSet<DocumentId>,
}

impl Topics<'_, '_> {
    // An item that names no topic is left out where the file drops it, whatever the file
    // places under its name; one that is, in another case, the one topic another item of
    // the list names is no item of its own.
    fn unnamed(&self, frame: &Frame<'_, '_>, item: &str, items: &[String]) -> Option<Placed> {
        let folded = Folded::new(item);
        let in_any_case = match frame.named(item) {
            Named::One(found) => Some(found.id),
            Named::None | Named::Several(_) | Named::Unsettled => None,
        };
        let again = |other: &String| match frame.included(other) {
            Named::One(found) => Some(found.id) == in_any_case,
            Named::None | Named::Several(_) | Named::Unsettled => false,
        };
        if items.iter().any(again) {
            return Some(Placed::Nowhere);
        }
        let open = Open::Include {
            topic: Folded::new(&self.own.key.slug),
            item: folded.clone(),
            missing: true,
            ended: false,
            unread: false,
        };
        let answers = frame.decisions.answers(&open);
        let dropped = |answer: &Answer<'_>| answer.placement() == Some(Placement::Drop);
        let refused = match answers.first() {
            None => open,
            Some(_) if answers.iter().any(dropped) => return Some(Placed::Nowhere),
            Some(placed) if frame.resolver.respelled(item) => placed.refused(format!(
                "{item} names no topic in that spelling: write {folded} in drop"
            )),
            Some(placed) => placed.refused(format!("{folded} names no topic")),
        };
        frame.questions.ask(refused);
        None
    }

    // An item that names a topic is asked about under the name that topic has now, so two
    // spellings of one topic are one placement.
    fn placed(&self, frame: &Frame<'_, '_>, item: &str, items: &[String]) -> Option<Placed> {
        let topic = Folded::new(&self.own.key.slug);
        let ask = |open: Open| {
            frame.questions.ask(open);
            None
        };
        let Some(found) = frame.listed(item).ok()? else {
            return self.unnamed(frame, item, items);
        };
        let name = found.name.clone();
        let itself = found.chain == Some(self.own.index);
        let open = Open::Include {
            topic,
            item: Folded::new(&name),
            missing: false,
            ended: self.ended.contains(&found.id),
            unread: self.unread.contains(&found.id),
        };
        let answer = frame.decisions.answer(&open);
        match answer.and_then(|answer| answer.placement()) {
            Some(Placement::Drop) => Some(Placed::Nowhere),
            _ if itself => {
                let key = Key::placed(&self.own.key.slug, Placement::Drop, &name);
                let why = format!("{name} includes itself: write it in drop");
                ask(Open::Answer { key, why })
            }
            Some(placement) => Some(Placed::In(placement, found)),
            None => ask(open),
        }
    }

    fn edges(&self, frame: &Frame<'_, '_>, step: &Step<'_>) -> Option<Edges<DocumentId>> {
        let mut edges = Some(Edges::default());
        let items = list(step.source.version, "includes");
        for item in items {
            match (self.placed(frame, item, items), &mut edges) {
                (Some(Placed::In(placement, found)), Some(edges)) => {
                    if let Some(ids) = edges.of(placement) {
                        push_once(ids, found.id);
                    }
                }
                (Some(_), _) => {}
                (None, _) => edges = None,
            }
        }
        edges
    }
}

enum Placed {
    In(Placement, Resolved),
    Nowhere,
}

impl Mapper for Topics<'_, '_> {
    type State = Option<Naming>;

    fn map(
        &mut self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        dates: Dates,
        parents: &[&Mapped<Self::State>],
    ) -> Mapped<Self::State> {
        let version = step.source.version;
        let name = frame.names.name(self.own.index, step.place);
        let before: Vec<Option<&Naming>> =
            parents.iter().map(|parent| parent.state.as_ref()).collect();
        let naming = naming(name, &before);
        let summary = frame.summary(self.own.key, version);
        let edges = self.edges(frame, step);
        let content = || {
            let Naming { name, former } = naming.clone()?;
            let edges = edges?;
            Some(Content::Topic(Topic {
                name,
                former_names: former,
                created: dates.created?,
                summary: summary?,
                part_of: edges.part_of,
                uses: edges.uses,
            }))
        };
        let record = content().map(|content| Record {
            content,
            ending: None,
        });
        Mapped {
            record,
            body: version.body.clone(),
            state: naming,
        }
    }
}

pub(crate) fn carrier(frame: &Frame<'_, '_>, own: Settled<'_, '_>) -> Carrier {
    let slugs =
        (own.chain.documents()).map(|document| frame.store.documents[document].slug.clone());
    Carrier {
        holder: Holder::Chain(own.index),
        scope: (!own.ended).then_some(Scope::Topics),
        segments: slugs.collect(),
        head: own.place(),
    }
}

fn ended_at_the_last(frame: &Frame<'_, '_>, own: Settled<'_, '_>) -> BTreeSet<DocumentId> {
    let Some(head) = own.head() else {
        return BTreeSet::new();
    };
    let refused = |found: &Resolved| {
        let under = |asked| refuses(Some(own), asked, found);
        under(Asked::PART_OF) || under(Asked::USES)
    };
    let named = list(head.source.version, "includes")
        .iter()
        .filter_map(|item| match frame.included(item) {
            Named::One(found) if refused(&found) => Some(found.id),
            Named::One(_) | Named::None | Named::Several(_) | Named::Unsettled => None,
        });
    named.collect()
}

fn left_out_at_the_last(frame: &Frame<'_, '_>, own: Settled<'_, '_>) -> BTreeSet<DocumentId> {
    let Some(head) = own.head() else {
        return BTreeSet::new();
    };
    let items = list(head.source.version, "includes");
    let out = frame.resolver.left_out(items);
    let (mut left, mut read) = (BTreeSet::new(), BTreeSet::new());
    for (place, item) in items.iter().enumerate() {
        match frame.included(item) {
            Named::One(found) if out.contains(&place) => drop(left.insert(found.id)),
            Named::One(found) => drop(read.insert(found.id)),
            Named::None | Named::Several(_) | Named::Unsettled => {}
        }
    }
    left.difference(&read).cloned().collect()
}

pub(crate) fn versions<'c, 'a>(frame: &Frame<'a, '_>, own: Settled<'c, 'a>) -> Walked<'c, 'a> {
    let mut topics = Topics {
        own,
        ended: ended_at_the_last(frame, own),
        unread: left_out_at_the_last(frame, own),
    };
    frame.walk(own, &mut topics)
}
