use std::collections::{BTreeMap, BTreeSet};

use worklog::domain::id::DocumentId;
use worklog::domain::schema::Date;
use worklog::domain::version::Stamp;

use super::Frame;
use super::chain::{Chains, Settled};
use super::walk::{Step, scalar};
use crate::decisions::{Answer, Unasked};
use crate::key::DocumentKey;
use crate::old::{Kind, Store};
use crate::question::Open;

fn digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

fn shaped(bytes: &[u8], shape: &[u8]) -> bool {
    bytes.len() == shape.len()
        && (bytes.iter().zip(shape)).all(|(byte, shape)| match shape {
            b'0' => byte.is_ascii_digit(),
            other => byte.eq_ignore_ascii_case(other),
        })
}

fn is_offset(bytes: &[u8]) -> bool {
    shaped(bytes, b"Z") || shaped(bytes, b"+00:00") || shaped(bytes, b"-00:00")
}

fn is_rfc_3339(text: &str) -> bool {
    let bytes = text.as_bytes();
    let Some((time, rest)) = bytes.split_at_checked(19) else {
        return false;
    };
    let rest = match rest.split_first() {
        Some((b'.', rest)) => {
            let fraction = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
            if !digits(&rest[..fraction]) || fraction > 9 {
                return false;
            }
            &rest[fraction..]
        }
        _ => rest,
    };
    shaped(time, b"0000-00-00T00:00:00") && is_offset(rest)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Written {
    pub(crate) stamp: Stamp,
    pub(crate) day: Date,
}

// A write time is read as RFC 3339 allows, with at most the nine fraction digits a stamp
// keeps.
pub(crate) fn written(text: &str) -> Option<Written> {
    let stamp = Stamp::parse(text).ok().filter(|_| is_rfc_3339(text))?;
    let day = day_of(&stamp)?;
    Some(Written { stamp, day })
}

pub(crate) fn day_of(stamp: &Stamp) -> Option<Date> {
    Date::parse(&stamp.day()).ok()
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Machines {
    holders: BTreeMap<String, BTreeSet<usize>>,
}

impl Machines {
    pub(crate) fn build(store: &Store, chains: &Chains<'_>) -> Machines {
        let mut machines = Machines::default();
        for (index, document) in store.documents.iter().enumerate() {
            let named = (document.versions.iter())
                .filter_map(|version| scalar(version, "machine"))
                .filter(|_| document.kind == Kind::Topic);
            for name in named {
                let holders = machines.holders.entry(name.to_owned()).or_default();
                holders.insert(chains.of[index]);
            }
        }
        machines
    }
}

/// What names a machine: the stamp of a planned version, the `machine` of an entry's
/// planned version, or the stamp of the versions the conversion authors.
#[derive(Clone, Copy)]
pub(crate) enum NamedBy<'s, 'c, 'a> {
    Stamp(Settled<'c, 'a>, &'s Step<'a>),
    Entry(Settled<'c, 'a>, &'s Step<'a>),
    Authored(usize),
}

#[derive(Debug, Default)]
pub(crate) struct Unknown {
    authored: usize,
    // A planned version, by the chain of its document and its key.
    versions: BTreeSet<(usize, String)>,
    entries: BTreeSet<DocumentKey>,
}

impl Unknown {
    fn hold(&mut self, named: NamedBy<'_, '_, '_>) {
        match named {
            NamedBy::Stamp(own, step) => {
                self.versions.insert((own.index, step.key().to_owned()));
            }
            NamedBy::Entry(own, step) => {
                self.versions.insert((own.index, step.key().to_owned()));
                self.entries.insert(own.key.clone());
            }
            NamedBy::Authored(versions) => self.authored += versions,
        }
    }
}

impl Machines {
    pub(crate) fn has(&self, chain: usize) -> bool {
        self.holders
            .values()
            .any(|holders| holders.contains(&chain))
    }
}

impl Frame<'_, '_> {
    pub(crate) fn ask_machines(&self) {
        for (name, unknown) in self.unknown.borrow().iter() {
            self.questions.ask(Open::Machine {
                name: name.clone(),
                versions: unknown.authored + unknown.versions.len(),
                documents: unknown.entries.iter().cloned().collect(),
            });
        }
    }

    // The write time a version the conversion authors has to follow.
    pub(crate) fn latest(&self) -> Option<Stamp> {
        let versions = self
            .store
            .documents
            .iter()
            .flat_map(|document| &document.versions);
        let stamps = versions.filter_map(|version| written(&version.written));
        stamps.map(|written| written.stamp).max()
    }

    fn answered(&self, name: &str, at: &Answer<'_>, among: &BTreeSet<usize>) -> Option<DocumentId> {
        let answer = at.text()?;
        let topic = self.answered_topic(at, answer)?;
        if among.is_empty() || topic.chain.is_some_and(|chain| among.contains(&chain)) {
            return Some(topic.id);
        }
        let why = format!("{answer} is not a topic of the machine {name}");
        self.questions.ask(at.refused(why));
        None
    }

    // The answer is for the machine the entry names, not for the one that stamped a version.
    pub(crate) fn worked_on(
        &self,
        own: Settled<'_, '_>,
        step: &Step<'_>,
        name: &str,
    ) -> Option<DocumentId> {
        let named = NamedBy::Entry(own, step);
        if self.machines.holders.contains_key(name) {
            return self.machine_of(name, named);
        }
        match self.decisions.unasked(Unasked::Machine(own.key)).pop() {
            Some(answer) => self.answered(name, &answer, &BTreeSet::new()),
            None => self.machine_of(name, named),
        }
    }

    // A machine with no topic is one question, whoever names it and however often, so it
    // is asked once the whole plan is made, of everything found to name it.
    pub(crate) fn machine_of(&self, name: &str, named: NamedBy<'_, '_, '_>) -> Option<DocumentId> {
        let none = BTreeSet::new();
        let holders = self.machines.holders.get(name).unwrap_or(&none);
        let asked = match (holders.first(), holders.len()) {
            (Some(only), 1) => return Some(self.chains.chains[*only].id.clone()),
            (None, _) => {
                let answered = self.decisions.machine(name);
                let answered = answered.filter(|at| at.text().is_some());
                if answered.is_none() {
                    let mut unknown = self.unknown.borrow_mut();
                    unknown.entry(name.to_owned()).or_default().hold(named);
                }
                answered
            }
            (Some(_), _) => {
                let settled = holders.iter().map(|chain| self.chains.settled(*chain));
                let slugs = settled.map(|own| Some(own?.key.slug.clone()));
                let mut topics = slugs.collect::<Option<Vec<String>>>()?;
                topics.sort();
                let name = name.to_owned();
                (self.text(Open::MachineTopics { name, topics })).map(|(at, _)| at)
            }
        };
        self.answered(name, &asked?, holders)
    }
}
