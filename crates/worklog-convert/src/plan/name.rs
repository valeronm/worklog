use std::collections::{BTreeMap, BTreeSet};

use worklog::domain::id::DocumentId;
use worklog::domain::schema::{Date, Name};

use super::Frame;
use crate::decisions::{Answer, FormerAnswer};
use crate::key::{DocumentKey, Key, TopicKey};
use crate::question::Open;

pub(crate) fn refusal(text: &str) -> Option<String> {
    if Name::parse(text).is_ok() {
        return None;
    }
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    let why = if let Some(other) = text.chars().find(|c| !allowed(*c)) {
        if other.is_ascii_uppercase() {
            "holds an uppercase letter".to_owned()
        } else {
            format!("holds `{other}`, and a name is lowercase letters, digits and hyphens")
        }
    } else if !text.chars().any(|c| c.is_ascii_lowercase()) {
        "holds no letter".to_owned()
    } else if text.starts_with('-') || text.ends_with('-') {
        "opens or closes with a hyphen".to_owned()
    } else {
        "reads as a date or as a document id".to_owned()
    };
    Some(why)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Scope {
    Topics,
    Facts(DocumentId),
    Entries(Date),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Holder {
    Chain(usize),
    // A topic the file creates, by the name of its table.
    Created(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Carrier {
    pub(crate) holder: Holder,
    // An ended document holds its name against no other, so it has no scope.
    pub(crate) scope: Option<Scope>,
    pub(crate) segments: Vec<String>,
    // Which segment the head of the document is stored under.
    pub(crate) head: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Naming {
    pub(crate) name: Name,
    pub(crate) former: Vec<Name>,
}

// The new store's rule for a rename: a parent's former names, then its name, each once and
// never the name the version has itself.
pub(crate) fn naming(name: Option<Name>, parents: &[Option<&Naming>]) -> Option<Naming> {
    let name = name?;
    let mut former = Vec::new();
    for parent in parents {
        let parent = (*parent)?;
        for held in parent.former.iter().chain([&parent.name]) {
            if *held != name && !former.contains(held) {
                former.push(held.clone());
            }
        }
    }
    Some(Naming { name, former })
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Names {
    of: BTreeMap<Holder, Vec<Option<Name>>>,
    // A name the store could hold stays in every converted version, so the name the file
    // gives in its place is that of one version after the last.
    renamed: BTreeMap<usize, Name>,
}

const SHARED: &str = "is held by several documents";

struct Settling<'f> {
    frame: &'f Frame<'f, 'f>,
    carriers: &'f [Carrier],
    heads: BTreeMap<Holder, Name>,
    // The name each document has when the conversion ends.
    ends: BTreeMap<Holder, Name>,
    // The key of the name each document was answered with.
    answered: BTreeMap<Holder, Key>,
    waiting: BTreeMap<Holder, Open>,
}

impl<'f> Settling<'f> {
    // `None` for an unsettled chain, which carries no name.
    fn key(&self, chain: usize) -> Option<DocumentKey> {
        let own = self.frame.chains.settled(chain)?;
        Some(own.key.clone())
    }

    fn not_a_name(&self, at: &Answer<'_>, answer: &str) {
        let why = refusal(answer).unwrap_or_default();
        self.frame
            .questions
            .ask(at.refused(format!("{answer} {why}")));
    }

    // The name of a topic the file creates is the name of its table, which no other key
    // answers for.
    fn head(&mut self, carrier: &Carrier, name: &str, why: &str) -> Result<Option<Name>, Open> {
        let chain = match &carrier.holder {
            Holder::Chain(chain) => *chain,
            Holder::Created(table) => {
                let key = Key::topic(table, TopicKey::Table);
                let why = format!("{name} {why}");
                return Err(Open::Answer { key, why });
            }
        };
        let Some(document) = self.key(chain) else {
            return Ok(None);
        };
        let open = Open::Name {
            document,
            name: name.to_owned(),
            why: why.to_owned(),
        };
        let answered = self.frame.decisions.answer(&open);
        let Some((at, answer)) = answered.and_then(|at| Some((at, at.text()?))) else {
            return Err(open);
        };
        (self.answered).insert(carrier.holder.clone(), at.key().clone());
        let read = Name::parse(answer).ok();
        if read.is_none() {
            self.not_a_name(&at, answer);
        }
        Ok(read)
    }

    fn own_heads(&mut self) {
        for carrier in self.carriers {
            let Some(segment) = carrier.segments.get(carrier.head) else {
                continue;
            };
            let name = match refusal(segment) {
                None => Name::parse(segment).ok(),
                Some(why) => self.head(carrier, segment, &why).unwrap_or_else(|open| {
                    self.frame.questions.ask(open);
                    None
                }),
            };
            self.heads
                .extend(name.map(|name| (carrier.holder.clone(), name)));
        }
        self.ends = self.heads.clone();
    }

    fn shared(&self) -> Vec<(&'f Carrier, Name)> {
        let held = |carrier: &'f Carrier| {
            let name = self.ends.get(&carrier.holder)?;
            Some((carrier, carrier.scope.as_ref()?, name))
        };
        let held: Vec<(&Carrier, &Scope, &Name)> = self.carriers.iter().filter_map(held).collect();
        let mut holders: BTreeMap<(&Scope, &str), BTreeSet<&Holder>> = BTreeMap::new();
        for (carrier, scope, name) in &held {
            let named = holders.entry((scope, name.as_str())).or_default();
            named.insert(&carrier.holder);
        }
        let shares = |one: &&(&Carrier, &Scope, &Name)| holders[&(one.1, one.2.as_str())].len() > 1;
        let colliding = held.iter().filter(shares);
        colliding.map(|one| (one.0, one.2.clone())).collect()
    }

    // An answer can end one collision and make another, so the round is repeated until no
    // answer is new.
    fn ask(&mut self) -> bool {
        let mut renamed = false;
        for (carrier, name) in self.shared() {
            let holder = carrier.holder.clone();
            if self.answered.contains_key(&holder) || self.waiting.contains_key(&holder) {
                continue;
            }
            match self.head(carrier, name.as_str(), SHARED) {
                Ok(Some(answer)) => {
                    self.ends.insert(holder, answer);
                    renamed = true;
                }
                Ok(None) => {
                    self.ends.remove(&holder);
                }
                Err(open) => {
                    self.waiting.insert(holder, open);
                }
            }
        }
        renamed
    }

    fn still_shared(&self) {
        for (carrier, name) in self.shared() {
            let answered = self.answered.get(&carrier.holder);
            match (self.waiting.get(&carrier.holder), answered) {
                (Some(open), _) => self.frame.questions.ask(open.clone()),
                (None, Some(key)) => {
                    let (key, why) = (key.clone(), format!("{name} {SHARED}"));
                    self.frame.questions.ask(Open::Answer { key, why });
                }
                (None, None) => {}
            }
        }
    }

    fn former(&self, carrier: &Carrier, segment: &str, next: Option<&Name>) -> Option<Name> {
        let Some(why) = refusal(segment) else {
            return Name::parse(segment).ok();
        };
        let Holder::Chain(chain) = carrier.holder else {
            return None;
        };
        let (at, answer) = self.frame.text(Open::FormerName {
            document: self.key(chain)?,
            name: segment.to_owned(),
            why,
        })?;
        match FormerAnswer::of(answer) {
            FormerAnswer::LeftOut => next.cloned(),
            FormerAnswer::Name(answer) => {
                let read = Name::parse(answer).ok();
                if read.is_none() {
                    self.not_a_name(&at, answer);
                }
                read
            }
        }
    }

    // A version stored under the head's segment has the head's name, whatever slug of the
    // chain it sits under; a name left out is the one the document had next.
    fn names(&self, carrier: &Carrier) -> Vec<Option<Name>> {
        let head = self.heads.get(&carrier.holder);
        let last = carrier.segments.get(carrier.head);
        let mut names: Vec<Option<Name>> = Vec::new();
        for segment in carrier.segments.iter().rev() {
            let name = if Some(segment) == last {
                head.cloned()
            } else {
                let next = names.last().map_or(head, Option::as_ref);
                self.former(carrier, segment, next)
            };
            names.push(name);
        }
        names.reverse();
        names
    }
}

impl Names {
    pub(crate) fn settle(frame: &Frame<'_, '_>, carriers: &[Carrier]) -> Names {
        let mut settling = Settling {
            frame,
            carriers,
            heads: BTreeMap::new(),
            ends: BTreeMap::new(),
            answered: BTreeMap::new(),
            waiting: BTreeMap::new(),
        };
        settling.own_heads();
        while settling.ask() {}
        settling.still_shared();
        let of = carriers
            .iter()
            .map(|carrier| (carrier.holder.clone(), settling.names(carrier)));
        let renamed = settling.ends.iter().filter_map(|(holder, end)| {
            let Holder::Chain(chain) = holder else {
                return None;
            };
            let kept = settling.heads.get(holder) == Some(end);
            (!kept).then(|| (*chain, end.clone()))
        });
        Names {
            of: of.collect(),
            renamed: renamed.collect(),
        }
    }

    pub(crate) fn renamed(&self, chain: usize) -> Option<&Name> {
        self.renamed.get(&chain)
    }

    pub(crate) fn name(&self, chain: usize, place: usize) -> Option<Name> {
        self.of.get(&Holder::Chain(chain))?.get(place)?.clone()
    }

    pub(crate) fn created(&self, table: &str) -> Option<Name> {
        let names = self.of.get(&Holder::Created(table.to_owned()))?;
        names.first()?.clone()
    }
}
