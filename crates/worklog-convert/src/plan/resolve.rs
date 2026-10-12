use std::collections::{BTreeMap, BTreeSet};

use worklog_next::domain::id::DocumentId;

use super::Frame;
use super::chain::Chains;
use super::id::created_topic_id;
use super::walk::Role;
use crate::decisions::{Answer, Decisions};
use crate::key::Folded;
use crate::old::{Kind, Store};
use crate::question::{Blocked, Open};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub(crate) id: DocumentId,
    pub(crate) name: String,
    // A rename's tombstone at the head does not end the document.
    pub(crate) ended: bool,
    pub(crate) chain: Option<usize>,
}

/// A document of a chain that goes on under both slugs of a rename. It is blocked where
/// the chains are built, and nothing is asked or planned through its name until the store
/// settles what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unsettled;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Named {
    One(Resolved),
    None,
    Several(Vec<String>),
    /// A topic that is unsettled is among those the name could be.
    Unsettled,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Resolver {
    documents: BTreeMap<(Kind, String), usize>,
    topics: BTreeMap<Folded, Vec<(String, usize)>>,
    // The slug of each topic the store's reader lists as present.
    present: BTreeMap<String, usize>,
    // The store's reader takes a name that is, in any case, a slug a topic was renamed
    // from for that topic. Of such slugs of several topics it takes the last in slug
    // order, which nobody chose, so here such a name is none of them. Which slug an
    // unsettled topic was renamed from is not known, so every slug of it is held here.
    renamed: BTreeMap<Folded, Vec<(String, usize)>>,
    created: BTreeSet<String>,
    chains: Vec<Result<Resolved, Unsettled>>,
}

impl Resolver {
    pub(crate) fn build(store: &Store, chains: &Chains<'_>, decisions: &Decisions) -> Resolver {
        let mut resolver = Resolver::default();
        let mut left: Vec<(&str, usize)> = Vec::new();
        for (index, chain) in chains.chains.iter().enumerate() {
            let topic = chain.kind == Kind::Topic;
            let slugs = (chain.documents()).map(|document| store.documents[document].slug.as_str());
            let Some(own) = chains.settled(index) else {
                resolver.chains.push(Err(Unsettled));
                left.extend(slugs.filter(|_| topic).map(|slug| (slug, index)));
                continue;
            };
            let name = &own.key.slug;
            resolver.chains.push(Ok(Resolved {
                id: chain.id.clone(),
                name: name.clone(),
                ended: own.ended,
                chain: Some(index),
            }));
            let role = own.head().map(|head| head.role);
            let reached = !matches!(role, Some(Role::Removed | Role::CutShort));
            if topic && reached {
                resolver.present.insert(name.clone(), index);
                let before = slugs.filter(|slug| slug != name);
                left.extend(before.map(|slug| (slug, index)));
            }
            for document in chain.documents() {
                let document = &store.documents[document];
                let slug = document.slug.clone();
                if document.kind == Kind::Topic {
                    let named = resolver.topics.entry(Folded::new(&slug)).or_default();
                    named.push((slug.clone(), index));
                }
                resolver.documents.insert((document.kind, slug), index);
            }
        }
        for (slug, index) in left {
            let from = resolver.renamed.entry(Folded::new(slug)).or_default();
            from.push((slug.to_owned(), index));
        }
        // A table whose name a topic of the store has in any case is that topic's table.
        resolver.created = (decisions.topics())
            .filter(|name| !resolver.topics.contains_key(&Folded::new(name)))
            .map(str::to_owned)
            .collect();
        resolver
    }

    pub(crate) fn created(&self) -> impl Iterator<Item = &str> {
        self.created.iter().map(String::as_str)
    }

    pub(crate) fn document(&self, kind: Kind, slug: &str) -> Result<Option<&Resolved>, Unsettled> {
        let chain = self.documents.get(&(kind, slug.to_owned()));
        match chain.and_then(|chain| self.chains.get(*chain)) {
            Some(Ok(found)) => Ok(Some(found)),
            Some(Err(unsettled)) => Err(*unsettled),
            None => Ok(None),
        }
    }

    fn one(&self, chain: Option<&usize>) -> Option<Named> {
        let found = chain.and_then(|chain| self.chains.get(*chain))?;
        Some(found.clone().map_or(Named::Unsettled, Named::One))
    }

    fn among(&self, named: &[(String, usize)], exact: Option<&usize>) -> Named {
        let all: BTreeSet<usize> = named.iter().map(|(_, chain)| *chain).collect();
        if all.iter().any(|chain| self.chains[*chain].is_err()) {
            return Named::Unsettled;
        }
        let chain = match (exact, all.first()) {
            (Some(chain), _) => Some(chain),
            (None, Some(only)) if all.len() == 1 => Some(only),
            (None, _) => None,
        };
        if let Some(found) = self.one(chain) {
            return found;
        }
        let mut slugs: Vec<String> = named.iter().map(|(slug, _)| slug.clone()).collect();
        slugs.sort();
        Named::Several(slugs)
    }

    // A table of the file is a topic under its name in any case.
    fn created_as(&self, name: &str) -> Named {
        let Some(created) = self.created.get(Folded::new(name).as_str()) else {
            return Named::None;
        };
        Named::One(Resolved {
            id: created_topic_id(created),
            name: created.clone(),
            ended: false,
            chain: None,
        })
    }

    // A name spelled as one slug is the topic of that slug, and a name that fits the
    // slugs of several topics in another case names none of them.
    fn compared(&self, name: &str) -> Named {
        let Some(named) = self.topics.get(&Folded::new(name)) else {
            return self.created_as(name);
        };
        let exact = named.iter().find(|(slug, _)| slug == name);
        self.among(named, exact.map(|(_, chain)| chain))
    }

    fn spelled(&self, name: &str) -> Named {
        let spelled = self.documents.get(&(Kind::Topic, name.to_owned()));
        (self.one(spelled)).unwrap_or_else(|| self.created_as(name))
    }

    fn renamed(&self, name: &str) -> Option<Named> {
        let from = self.renamed.get(&Folded::new(name))?;
        Some(self.among(from, None))
    }

    // The store's reader renames a tag and a touched topic before it compares one, in
    // any case, with the topics it lists.
    pub(crate) fn topic(&self, name: &str) -> Named {
        (self.renamed(name)).unwrap_or_else(|| self.compared(name))
    }

    // The store's reader renames an `includes` item, an `unclaimed` item and the topic of
    // a claim, and then takes it for the topic of that very slug.
    pub(crate) fn listed(&self, name: &str) -> Named {
        (self.renamed(name)).unwrap_or_else(|| self.spelled(name))
    }

    /// The places of the items the store's reader leaves out of a list as it renames it:
    /// an item it renames where the list holds, in any case, the name the item is renamed
    /// to, the item itself counted.
    pub(crate) fn left_out(&self, items: &[String]) -> BTreeSet<usize> {
        let now = |name: &str| match self.renamed(name) {
            Some(Named::One(found)) => Some(found.name),
            Some(Named::None | Named::Several(_) | Named::Unsettled) | None => None,
        };
        let mut names: Vec<(usize, String)> = items.iter().cloned().enumerate().collect();
        let mut out = BTreeSet::new();
        let mut at = 0;
        while at < names.len() {
            match now(&names[at].1) {
                Some(now)
                    if names
                        .iter()
                        .any(|(_, held)| held.eq_ignore_ascii_case(&now)) =>
                {
                    out.insert(names.remove(at).0);
                }
                Some(now) => {
                    names[at].1 = now;
                    at += 1;
                }
                None => at += 1,
            }
        }
        out
    }

    // The store's reader files a fact under the topic it lists by the very slug, then
    // under the one renamed from it, and else leaves the name as it is spelled. A name
    // several topics have in another case is asked about as one of them.
    pub(crate) fn fact_topic(&self, segment: &str) -> Named {
        let present = self.present.get(segment);
        let read = (self.one(present)).unwrap_or_else(|| self.listed(segment));
        match (read, self.compared(segment)) {
            (Named::None, several @ Named::Several(_)) => several,
            (read, _) => read,
        }
    }

    // Whether the name is one the store's reader takes for no topic of a list, though a
    // topic has it in another case.
    pub(crate) fn respelled(&self, name: &str) -> bool {
        self.listed(name) == Named::None && self.topic(name) != Named::None
    }

    // An answer is written after every rename, so one that spells a slug means the topic
    // of that slug.
    pub(crate) fn answered(&self, name: &str) -> Named {
        let spelled = self.documents.get(&(Kind::Topic, name.to_owned()));
        (self.one(spelled)).unwrap_or_else(|| self.topic(name))
    }
}

// The name is blocked and names no topic to plan on: several topics hold it, which was
// asked about, or its topic is unsettled.
pub(crate) struct Unclear;

fn in_another_case(name: &str, topics: &[String]) -> String {
    let topics = topics.join(", ");
    format!("{name} is {topics} in another case: spell one as the store does")
}

impl<'d> Frame<'_, 'd> {
    /// The file's answer to `open`, which is asked where the file gives none.
    pub(crate) fn answer(&self, open: Open) -> Option<Answer<'d>> {
        let answer = self.decisions.answer(&open);
        if answer.is_none() {
            self.questions.ask(open);
        }
        answer
    }

    /// The text the file answers `open` with, which is asked where the file gives none.
    pub(crate) fn text(&self, open: Open) -> Option<(Answer<'d>, &'d str)> {
        let answer = self.decisions.answer(&open);
        let read = answer.and_then(|answer| Some((answer, answer.text()?)));
        if read.is_none() {
            self.questions.ask(open);
        }
        read
    }

    pub(crate) fn named(&self, name: &str) -> Named {
        self.resolver.topic(name)
    }

    // An answer can spell a topic as the store does, so one that names several is asked
    // about at its own key.
    pub(crate) fn answered_topic(&self, at: &Answer<'_>, name: &str) -> Option<Resolved> {
        let why = match self.resolver.answered(name) {
            Named::One(found) => return Some(found),
            Named::None => format!("{name} names no topic"),
            Named::Several(topics) => in_another_case(name, &topics),
            Named::Unsettled => return None,
        };
        self.questions.ask(at.refused(why));
        None
    }

    // `held` says the document that gets the answer keeps the topic it names from ending.
    pub(crate) fn answered_unended(
        &self,
        at: &Answer<'_>,
        name: &str,
        held: bool,
    ) -> Option<Resolved> {
        let found = self.answered_topic(at, name)?;
        if found.ended && held {
            self.ended_answer(at, name);
            return None;
        }
        Some(found)
    }

    pub(crate) fn ended_answer(&self, at: &Answer<'_>, name: &str) {
        self.questions.ask(at.refused(format!("{name} is ended")));
    }

    pub(crate) fn included(&self, item: &str) -> Named {
        self.resolver.listed(item)
    }

    // A name the store holds in a list of a topic, which no key of the file answers for.
    pub(crate) fn listed(&self, name: &str) -> Result<Option<Resolved>, Unclear> {
        self.clear(name, self.resolver.listed(name))
    }

    pub(crate) fn topic(&self, name: &str) -> Result<Option<Resolved>, Unclear> {
        self.clear(name, self.resolver.topic(name))
    }

    // No key of the file tells topics apart whose names differ in case alone.
    fn clear(&self, name: &str, named: Named) -> Result<Option<Resolved>, Unclear> {
        match named {
            Named::One(found) => Ok(Some(found)),
            Named::None => Ok(None),
            Named::Several(topics) => {
                self.questions.ask(Blocked::TopicCase {
                    name: name.to_owned(),
                    topics,
                });
                Err(Unclear)
            }
            Named::Unsettled => Err(Unclear),
        }
    }
}
