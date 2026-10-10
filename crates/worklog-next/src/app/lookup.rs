use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::app::heads::{is_holder, kind_of, label_of, labeling_topic, readable_heads, row_head};
use crate::app::{Failure, kind_named};
use crate::domain::document::{Document, State};
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::ports::{Store, StoreError};
use crate::domain::schema::address::{Address, Candidate, Found, Holds, choose, displayed, holds};
use crate::domain::schema::{Content, KindOf, Name, Record};
use crate::domain::version::{Kind, Version};

pub(super) fn with_article(word: &str) -> String {
    let article = if word.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    format!("{article} {word}")
}

pub(super) fn unreadable(what: &str) -> Failure {
    Failure::at(what, "holds a version this worklog cannot read")
}

pub(super) fn forked(what: &str) -> Failure {
    Failure::at(what, "is forked; resolve it first")
}

pub(super) fn no_head_reads(what: &str) -> Failure {
    Failure::at(
        what,
        "no head of this fork reads; a newer worklog may be needed",
    )
}

pub(super) fn refuse_ended(record: &Record, what: &str) -> Result<(), Failure> {
    if record.ending.is_some() {
        return Err(Failure::at(what, "is ended; reopen it first"));
    }
    Ok(())
}

pub(super) fn names_no_document(address: &str) -> Failure {
    Failure::at(address, "names no document")
}

pub(super) fn held_by_several<'i>(
    address: &str,
    ids: impl IntoIterator<Item = &'i DocumentId>,
) -> Failure {
    let shorts: Vec<&str> = ids.into_iter().map(DocumentId::short).collect();
    Failure::at(
        address,
        format!("is held by several documents: {}", shorts.join(", ")),
    )
}

pub(super) fn no_such_document(what: &str) -> Failure {
    Failure::at(what, "no such document")
}

pub(super) fn not_a(kind: KindOf) -> String {
    format!("is not {}", with_article(kind.word()))
}

fn read_or_refuse(head: &Version, what: &str) -> Result<Record, Failure> {
    Record::read(&head.envelope.kind, &head.fields).map_err(|error| Failure::at(what, error))
}

/// Only `put` reaches the store past a `Lookup`.
#[derive(Clone, Copy)]
pub struct Stored<'a>(&'a dyn Store);

impl<'a> Stored<'a> {
    #[must_use]
    pub fn new(store: &'a dyn Store) -> Stored<'a> {
        Stored(store)
    }

    pub(super) fn put(&self, version: &Version) -> Result<(), StoreError> {
        self.0.put(version)
    }
}

type Question = (String, Vec<String>);

pub(super) struct Unknown {
    pub(super) kind: Kind,
    pub(super) documents: Vec<Rc<Document>>,
}

// One command's view of the store: each question is put to the store once.
pub(super) struct Lookup<'a> {
    store: &'a dyn Store,
    documents: RefCell<BTreeMap<DocumentId, Rc<Document>>>,
    kinds: RefCell<BTreeMap<&'static str, Vec<Rc<Document>>>>,
    holding: RefCell<BTreeMap<Question, Vec<DocumentId>>>,
}

impl<'a> Lookup<'a> {
    pub(super) fn new(stored: Stored<'a>) -> Lookup<'a> {
        Lookup {
            store: stored.0,
            documents: RefCell::default(),
            kinds: RefCell::default(),
            holding: RefCell::default(),
        }
    }

    pub(super) fn document(&self, id: &DocumentId) -> Result<Rc<Document>, Failure> {
        if let Some(held) = self.documents.borrow().get(id) {
            return Ok(Rc::clone(held));
        }
        let held = Rc::new(self.store.document(id)?);
        self.documents
            .borrow_mut()
            .insert(id.clone(), Rc::clone(&held));
        Ok(held)
    }

    fn seeded(&self, documents: Vec<Document>) -> Vec<Rc<Document>> {
        let mut held = self.documents.borrow_mut();
        documents
            .into_iter()
            .map(|document| {
                let kept = held
                    .entry(document.id().clone())
                    .or_insert_with(|| Rc::new(document));
                Rc::clone(kept)
            })
            .collect()
    }

    pub(super) fn of_kind(&self, kind: KindOf) -> Result<Vec<Rc<Document>>, Failure> {
        if let Some(answer) = self.kinds.borrow().get(kind.word()) {
            return Ok(answer.clone());
        }
        let answer = self.seeded(self.store.of_kind(&kind_named(kind))?);
        self.kinds.borrow_mut().insert(kind.word(), answer.clone());
        Ok(answer)
    }

    pub(super) fn writable(
        &self,
        id: &DocumentId,
        what: &str,
    ) -> Result<(Version, Record), Failure> {
        if !self.document(id)?.unreadable().is_empty() {
            return Err(unreadable(what));
        }
        self.only_head(id, what)
    }

    pub(super) fn only_head(
        &self,
        id: &DocumentId,
        what: &str,
    ) -> Result<(Version, Record), Failure> {
        let document = self.document(id)?;
        match document.state() {
            State::Absent => Err(no_such_document(what)),
            State::Forked(_) => Err(forked(what)),
            State::Live(head) => Ok((head.clone(), read_or_refuse(head, what)?)),
        }
    }

    // A name is tried before an id prefix.
    pub(super) fn find(&self, address: &str) -> Result<Found, Failure> {
        let parsed = Address::parse(address).map_err(|error| Failure::Usage(error.to_string()))?;
        let named = match &parsed {
            Address::Topic(name) => self.topic_named(name)?,
            Address::Fact { topic, name } => match self.topic_named(topic)? {
                Found::One(topic) => self.candidates(
                    KindOf::Fact,
                    name,
                    |record| matches!(&record.content, Content::Fact(fact) if fact.topic == topic),
                )?,
                other => return Ok(other),
            },
            Address::Entry { date, name } => self.candidates(
                KindOf::Entry,
                name,
                |record| matches!(&record.content, Content::Entry(entry) if entry.date == *date),
            )?,
            Address::Id(_) => Found::None,
        };
        if named != Found::None {
            return Ok(named);
        }
        match Address::id_prefix(address) {
            Some(prefix) => Ok(by_prefix(self.store.documents_under(prefix)?)),
            None => Ok(Found::None),
        }
    }

    pub(super) fn versions_under(
        &self,
        prefix: &str,
    ) -> Result<Vec<(DocumentId, VersionId)>, Failure> {
        Ok(self.store.versions_under(prefix)?)
    }

    pub(super) fn one(&self, address: &str) -> Result<DocumentId, Failure> {
        match self.find(address)? {
            Found::One(id) => Ok(id),
            Found::None => Err(names_no_document(address)),
            Found::Collision(ids) => Err(held_by_several(address, &ids)),
        }
    }

    pub(super) fn holding_unreadable(&self) -> Result<Vec<Rc<Document>>, Failure> {
        Ok(self.seeded(self.store.unreadable()?))
    }

    pub(super) fn is_kind(&self, id: &DocumentId, kind: KindOf) -> Result<bool, Failure> {
        let document = self.document(id)?;
        Ok(kind_of(&document) == Some(kind) && row_head(&document).is_some())
    }

    pub(super) fn everything(&self) -> Result<Vec<Rc<Document>>, Failure> {
        let mut documents = Vec::new();
        for kind in KindOf::ALL {
            documents.extend(self.of_kind(kind)?);
        }
        Ok(documents)
    }

    pub(super) fn forks(&self) -> Result<Vec<Rc<Document>>, Failure> {
        let mut forked = self.seeded(self.store.forked()?);
        forked.retain(|document| kind_of(document).is_some());
        Ok(forked)
    }

    pub(super) fn unknown(&self) -> Result<Vec<Unknown>, Failure> {
        let mut unknown = Vec::new();
        for kind in self.store.kinds()? {
            if KindOf::of(&kind).is_err() {
                let documents = self.seeded(self.store.of_kind(&kind)?);
                unknown.push(Unknown { kind, documents });
            }
        }
        Ok(unknown)
    }

    pub(super) fn one_topic(&self, address: &str) -> Result<DocumentId, Failure> {
        let id = self.one(address)?;
        if self.is_kind(&id, KindOf::Topic)? {
            Ok(id)
        } else {
            Err(Failure::at(address, not_a(KindOf::Topic)))
        }
    }

    pub(super) fn address(&self, id: &DocumentId) -> Result<Option<String>, Failure> {
        let document = self.document(id)?;
        let Some((_, record)) = row_head(&document) else {
            return Ok(None);
        };
        self.address_of(&record.content)
    }

    pub(super) fn address_of(&self, content: &Content) -> Result<Option<String>, Failure> {
        Ok(displayed(content, self.topic_name_for(content)?.as_ref()))
    }

    fn topic_name_for(&self, content: &Content) -> Result<Option<Name>, Failure> {
        match content {
            Content::Fact(fact) => self.topic_name(&fact.topic),
            _ => Ok(None),
        }
    }

    // A former address differs from the current one in the name alone.
    pub(super) fn former_addresses(&self, id: &DocumentId) -> Result<Vec<String>, Failure> {
        let document = self.document(id)?;
        let Some((_, record)) = row_head(&document) else {
            return Ok(Vec::new());
        };
        let (Some(address), Some((name, former))) =
            (self.address_of(&record.content)?, record.content.naming())
        else {
            return Ok(Vec::new());
        };
        let stem = address.strip_suffix(name.as_str()).unwrap_or_default();
        Ok(former.iter().map(|old| format!("{stem}{old}")).collect())
    }

    pub(super) fn label(&self, id: &DocumentId) -> Result<String, Failure> {
        let document = self.document(id)?;
        match row_head(&document) {
            Some((_, record)) => self.label_of(&record.content, id),
            None => Ok(id.short().to_owned()),
        }
    }

    pub(super) fn label_of(&self, content: &Content, id: &DocumentId) -> Result<String, Failure> {
        let topic_name = match labeling_topic(content) {
            Some(topic) => self.topic_name(topic)?,
            None => None,
        };
        Ok(label_of(content, topic_name.as_ref(), id))
    }

    pub(super) fn holders<V: AsRef<str>>(
        &self,
        kind: KindOf,
        key: &str,
        values: &[V],
        ended: bool,
    ) -> Result<Vec<Rc<Document>>, Failure> {
        let values: BTreeSet<&str> = values.iter().map(AsRef::as_ref).collect();
        let mut holders = Vec::new();
        for id in self.holding(key, &values)? {
            let document = self.document(&id)?;
            if is_holder(&document, kind, key, &values, ended) {
                holders.push(document);
            }
        }
        Ok(holders)
    }

    fn holding(&self, key: &str, values: &BTreeSet<&str>) -> Result<Vec<DocumentId>, Failure> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let question: Question = (
            key.to_owned(),
            values.iter().map(|value| (*value).to_owned()).collect(),
        );
        if let Some(answer) = self.holding.borrow().get(&question) {
            return Ok(answer.clone());
        }
        let asked: Vec<&str> = values.iter().copied().collect();
        let answer: Vec<DocumentId> = self
            .seeded(self.store.holding(key, &asked)?)
            .iter()
            .map(|document| document.id().clone())
            .collect();
        self.holding.borrow_mut().insert(question, answer.clone());
        Ok(answer)
    }

    pub(super) fn topic_name(&self, topic: &DocumentId) -> Result<Option<Name>, Failure> {
        let document = self.document(topic)?;
        Ok(
            row_head(&document).and_then(|(_, record)| match record.content {
                Content::Topic(topic) => Some(topic.name),
                _ => None,
            }),
        )
    }

    fn topic_named(&self, name: &Name) -> Result<Found, Failure> {
        self.candidates(KindOf::Topic, name, |_| true)
    }

    fn candidates(
        &self,
        kind: KindOf,
        name: &Name,
        wanted: impl Fn(&Record) -> bool,
    ) -> Result<Found, Failure> {
        let mut candidates = Vec::new();
        for key in ["name", "former_names"] {
            for id in self.holding(key, &BTreeSet::from([name.as_str()]))? {
                let document = self.document(&id)?;
                if kind_of(&document) != Some(kind) {
                    continue;
                }
                let strongest = readable_heads(&document)
                    .iter()
                    .filter(|(_, record)| wanted(record))
                    .filter_map(|(_, record)| holds(record, name))
                    .min();
                if let Some(held) = strongest {
                    candidates.push(Candidate { id, holds: held });
                }
            }
            if candidates
                .iter()
                .any(|candidate| candidate.holds == Holds::Current)
            {
                break;
            }
        }
        Ok(choose(&candidates))
    }
}

fn by_prefix(mut ids: Vec<DocumentId>) -> Found {
    match ids.len() {
        0 => Found::None,
        1 => Found::One(ids.remove(0)),
        _ => Found::Collision(ids),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{
        Counting, ENDED, ENDED_FACT, TOPIC, World, claim_fields, entry, fact, fork, found, topic,
        topic_fields,
    };
    use crate::domain::testing::{after, first, lantern};
    use crate::domain::version::ReadError;

    fn holder_ids<V: AsRef<str>>(
        lookup: &Lookup,
        kind: KindOf,
        key: &str,
        values: &[V],
    ) -> Result<Vec<DocumentId>, Failure> {
        let holders = lookup.holders(kind, key, values, false)?;
        Ok(holders.iter().map(|held| held.id().clone()).collect())
    }

    fn lantern_topic(world: &World, rest: &str) -> DocumentId {
        let version = first(&lantern(), "topic", &topic_fields("lantern", rest), "\n");
        world.store.put(&version).unwrap();
        lantern()
    }

    #[test]
    fn a_topic_is_found_by_name_former_name_or_id_prefix() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = lantern_topic(&world, "former_names = [\"lamp\"]\n");
        assert_eq!(found(&world, "lantern"), Found::One(lantern.clone()));
        assert_eq!(found(&world, "lamp"), Found::One(lantern.clone()));
        let prefix = &lantern.as_str()[..4];
        assert_eq!(found(&world, prefix), Found::One(lantern.clone()));
        assert_eq!(found(&world, "unknown"), Found::None);
    }

    #[test]
    fn a_fact_is_found_under_its_topic() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "former_names = [\"lamp\"]\n");
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let other = fact(&world, &atlas, "relay-pin", "");
        assert_eq!(
            found(&world, "lantern/relay-pin"),
            Found::One(relay.clone())
        );
        assert_eq!(found(&world, "lamp/relay-pin"), Found::One(relay));
        assert_eq!(found(&world, "atlas/relay-pin"), Found::One(other));
        assert_eq!(found(&world, "lantern/unknown"), Found::None);
        assert_eq!(found(&world, "unknown/relay-pin"), Found::None);
    }

    #[test]
    fn a_fact_under_a_colliding_topic_is_that_collision() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let first = topic(&world, "lantern", "");
        let second = topic(&world, "lantern", "");
        fact(&world, &first, "relay-pin", "");
        assert_eq!(
            found(&world, "lantern/relay-pin"),
            Found::Collision(vec![first, second])
        );
    }

    #[test]
    fn an_entry_is_found_by_date_and_name() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let today = entry(&world, "2026-10-09", "lamp-driver", &[]);
        let earlier = entry(&world, "2026-10-08", "lamp-driver", &[]);
        assert_eq!(found(&world, "2026-10-09-lamp-driver"), Found::One(today));
        assert_eq!(found(&world, "2026-10-08-lamp-driver"), Found::One(earlier));
        assert_eq!(found(&world, "2026-10-07-lamp-driver"), Found::None);
    }

    #[test]
    fn the_strongest_holder_of_a_name_is_found() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let first = topic(&world, "lantern", "");
        let second = topic(&world, "lantern", "");
        assert_eq!(
            found(&world, "lantern"),
            Found::Collision(vec![first.clone(), second.clone()])
        );

        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let former = topic(&world, "phone", "former_names = [\"lantern\"]\n");
        let current = topic(&world, "lantern", "");
        assert_ne!(former, current);
        assert_eq!(found(&world, "lantern"), Found::One(current));

        let world = World::new();
        let _machine = topic(&world, "desk", "");
        topic(&world, "lantern", ENDED);
        let live = topic(&world, "lantern", "");
        assert_eq!(found(&world, "lantern"), Found::One(live));
    }

    #[test]
    fn a_name_that_is_also_an_id_prefix_is_a_name_first() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = lantern_topic(&world, "");
        let prefix = &lantern.as_str()[..6];
        assert_eq!(found(&world, prefix), Found::One(lantern.clone()));
        let named = topic(&world, prefix, "");
        assert_eq!(found(&world, prefix), Found::One(named));
    }

    #[test]
    fn a_fork_holds_a_name_if_any_head_does() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let left = after(&[&root], &topic_fields("lantern", ""), "left\n");
        let right = after(&[&root], &topic_fields("phone", ""), "right\n");
        world.store.put(&left).unwrap();
        world.store.put(&right).unwrap();
        assert_eq!(found(&world, "lantern"), Found::One(lantern.clone()));
        assert_eq!(found(&world, "phone"), Found::One(lantern));
    }

    #[test]
    fn a_document_with_no_readable_head_is_never_a_candidate() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        world.put("topic", "name = \"lantern\"\n", "\n");
        assert_eq!(found(&world, "lantern"), Found::None);
    }

    #[test]
    fn a_renamed_topic_answers_to_its_new_name_and_its_old_one() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let renamed = world.amend(
            &lantern,
            &format!("name = \"phone\"\nformer_names = [\"lantern\"]\n{TOPIC}"),
            "\n",
        );
        let lookup = Lookup::new(world.deps().store);
        assert_eq!(lookup.writable(&lantern, "phone").unwrap().0.id, renamed);
        assert_eq!(lookup.find("phone").unwrap(), Found::One(lantern.clone()));
        assert_eq!(lookup.find("lantern").unwrap(), Found::One(lantern));
    }

    #[test]
    fn a_forked_document_prints_the_address_of_its_row_head() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let mut heads = Vec::new();
        for name in ["lantern", "phone"] {
            let head = after(&[&root], &topic_fields(name, ""), name);
            world.store.put(&head).unwrap();
            heads.push((head.id.clone(), name));
        }
        heads.sort();
        let lookup = world.lookup();
        assert_eq!(
            lookup.address(&lantern).unwrap().as_deref(),
            Some(heads[1].1)
        );
    }

    #[test]
    fn a_text_that_is_no_address_is_a_usage_error() {
        let world = World::new();
        let result = world.lookup().find("Lantern");
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    #[test]
    fn one_refuses_none_and_a_collision() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        topic(&world, "lantern", "");
        topic(&world, "lantern", "");
        let lookup = world.lookup();
        let Err(Failure::Refused(text)) = lookup.one("atlas") else {
            panic!("a name held by nothing must be refused");
        };
        assert!(text.contains("names no document"), "{text}");
        let Err(Failure::Refused(text)) = lookup.one("lantern") else {
            panic!("a collision must be refused");
        };
        assert!(text.contains("several documents"), "{text}");
        let phone = topic(&world, "phone", "");
        assert_eq!(world.lookup().one("phone"), Ok(phone));
    }

    #[test]
    fn a_writable_document_has_one_head() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let lookup = world.lookup();
        let (version, record) = lookup.writable(&lantern, "lantern").unwrap();
        assert_eq!(version.envelope.document, lantern);
        assert_eq!(record.content.kind(), KindOf::Topic);

        let absent = DocumentId::from_bytes([0x42; 16]);
        let Err(Failure::Refused(text)) = lookup.writable(&absent, "phone") else {
            panic!("an absent document must be refused");
        };
        assert!(text.contains("phone: no such document"), "{text}");

        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        for body in ["left\n", "right\n"] {
            let fork = after(&[&root], &topic_fields("lantern", ""), body);
            world.store.put(&fork).unwrap();
        }
        let Err(Failure::Refused(text)) = world.lookup().writable(&lantern, "lantern") else {
            panic!("a forked document must be refused");
        };
        assert!(
            text.contains("lantern: is forked; resolve it first"),
            "{text}"
        );
    }

    #[test]
    fn a_document_holding_a_file_that_does_not_read_is_not_writable() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        world
            .store
            .plant_unreadable(&lantern, VersionId::of(b"damaged"), ReadError::Corrupt);
        let lookup = world.lookup();
        let Err(Failure::Refused(text)) = lookup.writable(&lantern, "lantern") else {
            panic!("a document holding an unreadable file must be refused");
        };
        assert!(
            text.contains("lantern: holds a version this worklog cannot read"),
            "{text}"
        );
        assert!(lookup.only_head(&lantern, "lantern").is_ok());
    }

    #[test]
    fn a_document_prints_as_its_address_or_its_short_id() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let driver = entry(&world, "2026-10-09", "lamp-driver", &[]);
        let followup = world.put(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n"),
            "\n",
        );
        let lookup = world.lookup();
        assert_eq!(
            lookup.address(&lantern).unwrap().as_deref(),
            Some("lantern")
        );
        assert_eq!(
            lookup.address(&relay).unwrap().as_deref(),
            Some("lantern/relay-pin")
        );
        assert_eq!(
            lookup.address(&driver).unwrap().as_deref(),
            Some("2026-10-09-lamp-driver")
        );
        assert_eq!(lookup.address(&followup).unwrap(), None);
        assert_eq!(lookup.label(&followup).unwrap(), followup.short());
        assert_eq!(lookup.label(&relay).unwrap(), "lantern/relay-pin");

        let orphan = fact(&world, &DocumentId::from_bytes([0x42; 16]), "relay", "");
        assert_eq!(lookup.address(&orphan).unwrap(), None);
        assert_eq!(
            lookup.address(&DocumentId::from_bytes([0x43; 16])).unwrap(),
            None
        );
    }

    #[test]
    fn a_claim_is_labeled_by_its_topic_and_its_directory_and_found_by_its_id() {
        let world = World::new();
        let desk = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let placed = world.put("claim", &claim_fields(&desk, &lantern, "~/lantern"), "\n");
        let anywhere = world.put("claim", &claim_fields(&desk, &lantern, ""), "\n");
        let lookup = world.lookup();
        assert_eq!(lookup.label(&placed).unwrap(), "lantern at ~/lantern");
        assert_eq!(lookup.label(&anywhere).unwrap(), "lantern anywhere");
        assert_eq!(lookup.address(&placed).unwrap(), None);
        assert_eq!(found(&world, placed.short()), Found::One(placed));
        assert!(lookup.find("lantern at ~/lantern").is_err());
        assert!(lookup.find("lantern anywhere").is_err());
    }

    #[test]
    fn a_claim_s_label_keeps_the_name_of_an_ended_topic_and_the_short_id_of_one_not_read() {
        let world = World::new();
        let desk = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let placed = world.put("claim", &claim_fields(&desk, &lantern, "~/lantern"), "\n");
        world.amend(&lantern, &topic_fields("lantern", ENDED), "\n");
        let gone = DocumentId::from_bytes([0x42; 16]);
        let orphan = world.put("claim", &claim_fields(&desk, &gone, ""), "\n");
        let lookup = world.lookup();
        assert_eq!(lookup.label(&placed).unwrap(), "lantern at ~/lantern");
        assert_eq!(
            lookup.label(&orphan).unwrap(),
            format!("{} anywhere", gone.short())
        );
    }

    #[test]
    fn a_forked_claim_is_labeled_from_its_row_head() {
        let world = World::new();
        let desk = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let fields = claim_fields(&desk, &lantern, "~/lantern");
        let placed = world.put("claim", &fields, "\n");
        fork(&world, &placed, &fields);
        assert_eq!(
            world.lookup().label(&placed).unwrap(),
            "lantern at ~/lantern"
        );
    }

    #[test]
    fn holders_are_the_live_documents_of_the_kind() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let pin = fact(&world, &lantern, "lamp-pin", "");
        fact(&world, &lantern, "old-pin", ENDED_FACT);
        fact(&world, &atlas, "map", "");
        let lookup = world.lookup();
        let mut held = holder_ids(&lookup, KindOf::Fact, "topic", &[&lantern]).unwrap();
        held.sort();
        let mut expected = vec![relay, pin];
        expected.sort();
        assert_eq!(held, expected);
        assert!(
            lookup
                .holders(KindOf::Topic, "topic", &[&lantern], false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn holders_of_several_values_are_found_with_one_question() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let phone = topic(&world, "phone", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let map = fact(&world, &atlas, "map", "");
        fact(&world, &atlas, "old-map", ENDED_FACT);
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));
        let topics = [&lantern, &atlas, &phone];
        let held = holder_ids(&lookup, KindOf::Fact, "topic", &topics).unwrap();
        assert_eq!(held, [relay, map]);
        assert_eq!(counting.holdings.get(), 1);
        assert_eq!(counting.documents.get(), 0);
        lookup
            .holders(KindOf::Fact, "topic", &topics, false)
            .unwrap();
        assert_eq!(counting.holdings.get(), 1);
        let none: [&DocumentId; 0] = [];
        assert!(
            lookup
                .holders(KindOf::Fact, "topic", &none, false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(counting.holdings.get(), 1);
    }

    #[test]
    fn one_set_of_values_in_two_orders_is_one_question() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let map = fact(&world, &atlas, "map", "");
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));
        let forward = holder_ids(&lookup, KindOf::Fact, "topic", &[&lantern, &atlas]);
        let backward = holder_ids(&lookup, KindOf::Fact, "topic", &[&atlas, &lantern, &atlas]);
        assert_eq!(backward, forward);
        assert_eq!(forward.unwrap(), [relay, map]);
        assert_eq!(counting.holdings.get(), 1);
    }

    #[test]
    fn a_document_of_a_kind_is_not_asked_for_again() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        topic(&world, "atlas", ENDED);
        fact(&world, &lantern, "relay-pin", "");
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));
        let topics = lookup.of_kind(KindOf::Topic).unwrap();
        assert_eq!(topics.len(), 3);
        let listed = topics
            .iter()
            .find(|document| document.id() == &lantern)
            .unwrap();
        assert!(Rc::ptr_eq(listed, &lookup.document(&lantern).unwrap()));
        assert_eq!(lookup.label(&lantern).unwrap(), "lantern");
        assert_eq!(counting.documents.get(), 0);
        let again = lookup.of_kind(KindOf::Topic).unwrap();
        assert!(Rc::ptr_eq(&again[0], &topics[0]));
        assert_eq!(counting.kinds.get(), 1);
        lookup.of_kind(KindOf::Fact).unwrap();
        assert_eq!(counting.kinds.get(), 2);
    }

    #[test]
    fn a_current_name_is_found_with_one_question_and_a_miss_takes_two() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        topic(&world, "phone", "former_names = [\"lantern\", \"lamp\"]\n");
        let atlas = topic(&world, "atlas", ENDED);
        let counting = Counting::over(world.store);
        let asked = |address: &str| {
            let before = counting.holdings.get();
            let found = Lookup::new(Stored::new(&counting)).find(address).unwrap();
            (found, counting.holdings.get() - before)
        };
        assert_eq!(asked("lantern"), (Found::One(lantern), 1));
        assert_eq!(asked("unknown"), (Found::None, 2));
        assert_eq!(asked("atlas"), (Found::One(atlas), 2));
        assert!(matches!(asked("lamp"), (Found::One(_), 2)));
    }

    #[test]
    fn the_store_is_asked_each_question_once() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let counting = Counting::over(world.store);
        let lookup = Lookup::new(Stored::new(&counting));

        let first = lookup.document(&lantern).unwrap();
        let second = lookup.document(&lantern).unwrap();
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(counting.documents.get(), 1);

        lookup.find("lantern").unwrap();
        let documents = counting.documents.get();
        let holdings = counting.holdings.get();
        assert!(holdings > 0);
        lookup.find("lantern").unwrap();
        lookup
            .holders(KindOf::Fact, "topic", &[&lantern], false)
            .unwrap();
        lookup
            .holders(KindOf::Fact, "topic", &[&lantern], false)
            .unwrap();
        assert_eq!(counting.documents.get(), documents);
        assert_eq!(counting.holdings.get(), holdings + 1);
    }
}
