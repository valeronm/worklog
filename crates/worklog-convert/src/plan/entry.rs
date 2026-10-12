use worklog_next::domain::schema::{Content, Date, Entry, Record};

use super::Frame;
use super::chain::Settled;
use super::filing::Filed;
use super::name::{Carrier, Holder, Naming, Scope, naming};
use super::walk::{Asked, Dates, Mapped, Mapper, Step, Walked, scalar};
use crate::old::{Kind, dated};

struct Entries<'c, 'a> {
    own: Settled<'c, 'a>,
}

fn name_of(slug: &str) -> String {
    dated(Kind::Entry, slug).map_or_else(|| slug.to_owned(), |(_, name)| name)
}

impl Mapper for Entries<'_, '_> {
    type State = Option<Naming>;

    fn map(
        &mut self,
        frame: &Frame<'_, '_>,
        step: &Step<'_>,
        _: Dates,
        parents: &[&Mapped<Self::State>],
    ) -> Mapped<Self::State> {
        let version = step.source.version;
        let name = frame.names.name(self.own.index, step.place);
        let before: Vec<Option<&Naming>> =
            parents.iter().map(|parent| parent.state.as_ref()).collect();
        let naming = naming(name, &before);
        let date = frame.day(&step.source, "date");
        let machine = scalar(version, "machine");
        if machine.is_none() {
            frame.missing(&step.source, "machine");
        }
        let machine = machine.and_then(|name| frame.worked_on(self.own, step, name));
        let filed = Filed {
            own: self.own,
            asked: Asked::ENTRY_TOPICS,
        };
        let topics = filed.topics(frame, step);
        let summary = frame.summary(self.own.key, version);
        let content = || {
            let Naming { name, former } = naming.clone()?;
            Some(Content::Entry(Entry {
                name,
                former_names: former,
                date: date?,
                machine: machine?,
                topics: topics?,
                summary: summary?,
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

// The new store holds an entry's name against the entries of its date.
pub(crate) fn carrier(frame: &Frame<'_, '_>, own: Settled<'_, '_>) -> Carrier {
    let first = own.heads().min_by_key(|head| head.key());
    let date = first
        .and_then(|head| scalar(head.source.version, "date"))
        .and_then(|date| Date::parse(date).ok());
    let scope = date.filter(|_| !own.ended);
    let name = |document: usize| name_of(&frame.store.documents[document].slug);
    Carrier {
        holder: Holder::Chain(own.index),
        scope: scope.map(Scope::Entries),
        segments: own.chain.documents().map(name).collect(),
        head: own.place(),
    }
}

pub(crate) fn versions<'c, 'a>(frame: &Frame<'a, '_>, own: Settled<'c, 'a>) -> Walked<'c, 'a> {
    frame.walk(own, &mut Entries { own })
}
