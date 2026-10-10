use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use toml::Value;

use crate::app::Failure;
use crate::domain::document::{Document, State};
use crate::domain::id::DocumentId;
use crate::domain::ports::Store;
use crate::domain::schema::address::{Address, Candidate, Found, Holds, choose, displayed, holds};
use crate::domain::schema::{Content, KindOf, Name, Record};
use crate::domain::version::{Fields, Kind, Version};

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

pub(super) fn ended(what: &str) -> Failure {
    Failure::at(what, "is ended; reopen it first")
}

fn no_such_document(what: &str) -> Failure {
    Failure::at(what, "no such document")
}

fn read_or_refuse(head: &Version, what: &str) -> Result<Record, Failure> {
    Record::read(&head.envelope.kind, &head.fields).map_err(|error| Failure::at(what, error))
}

pub(super) fn read_or_skip(head: &Version) -> Option<Record> {
    Record::read(&head.envelope.kind, &head.fields).ok()
}

/// One command's view of the store: each question is put to the store once.
pub struct Lookup<'a> {
    store: &'a dyn Store,
    documents: RefCell<BTreeMap<DocumentId, Rc<Document>>>,
    holding: RefCell<BTreeMap<(String, String), Vec<DocumentId>>>,
}

impl<'a> Lookup<'a> {
    pub fn new(store: &'a dyn Store) -> Lookup<'a> {
        Lookup {
            store,
            documents: RefCell::default(),
            holding: RefCell::default(),
        }
    }

    pub fn document(&self, id: &DocumentId) -> Result<Rc<Document>, Failure> {
        if let Some(held) = self.documents.borrow().get(id) {
            return Ok(Rc::clone(held));
        }
        let held = Rc::new(self.store.document(id)?);
        self.documents
            .borrow_mut()
            .insert(id.clone(), Rc::clone(&held));
        Ok(held)
    }

    /// One record per head, in head order: empty for an absent document, a refusal naming the
    /// document when a head does not read.
    pub fn records(&self, id: &DocumentId) -> Result<Vec<Record>, Failure> {
        self.records_as(id, id.short())
    }

    fn records_as(&self, id: &DocumentId, what: &str) -> Result<Vec<Record>, Failure> {
        self.document(id)?
            .heads()
            .into_iter()
            .map(|head| read_or_refuse(head, what))
            .collect()
    }

    /// The only head and its record; refuses an absent or a forked document, naming `what`.
    pub fn writable(&self, id: &DocumentId, what: &str) -> Result<(Version, Record), Failure> {
        let document = self.document(id)?;
        match document.state() {
            State::Absent => Err(no_such_document(what)),
            State::Forked(_) => Err(forked(what)),
            State::Live(head) => Ok((head.clone(), read_or_refuse(head, what)?)),
        }
    }

    /// A usage failure when `address` is no address; a name is tried before an id prefix.
    pub fn find(&self, address: &str) -> Result<Found, Failure> {
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

    /// Refuses, naming the address, when it means no document or several.
    pub fn one(&self, address: &str) -> Result<DocumentId, Failure> {
        match self.find(address)? {
            Found::One(id) => Ok(id),
            Found::None => Err(Failure::at(address, "names no document")),
            Found::Collision(ids) => {
                let shorts: Vec<&str> = ids.iter().map(DocumentId::short).collect();
                Err(Failure::at(
                    address,
                    format!("is held by several documents: {}", shorts.join(", ")),
                ))
            }
        }
    }

    /// Refuses, naming the address, unless it means one document of the kind, when one is
    /// given, with a head that is not ended.
    pub fn referencable(&self, address: &str, kind: Option<KindOf>) -> Result<DocumentId, Failure> {
        let id = self.one(address)?;
        let records = self.records_as(&id, address)?;
        let Some(first) = records.first() else {
            return Err(no_such_document(address));
        };
        if let Some(kind) = kind
            && first.content.kind() != kind
        {
            return Err(Failure::at(
                address,
                format!("is not {}", with_article(kind.word())),
            ));
        }
        if records.iter().all(|record| record.ending.is_some()) {
            return Err(Failure::at(address, "is ended"));
        }
        Ok(id)
    }

    /// None for a kind without a name, for a fact whose topic cannot be read, and for a
    /// document with no readable head.
    pub fn address(&self, id: &DocumentId) -> Result<Option<String>, Failure> {
        let Some(record) = self.readable(id)?.into_iter().next() else {
            return Ok(None);
        };
        self.address_of(&record.content)
    }

    pub(super) fn address_of(&self, content: &Content) -> Result<Option<String>, Failure> {
        let topic = match content {
            Content::Fact(fact) => self.topic_name(&fact.topic)?,
            _ => None,
        };
        Ok(displayed(content, topic.as_ref()))
    }

    /// The addresses under each former name, with the topic's name and the date it has now.
    pub fn former_addresses(&self, id: &DocumentId) -> Result<Vec<String>, Failure> {
        let (Some(address), Some(record)) =
            (self.address(id)?, self.readable(id)?.into_iter().next())
        else {
            return Ok(Vec::new());
        };
        let Some((name, former)) = record.content.naming() else {
            return Ok(Vec::new());
        };
        let stem = address.strip_suffix(name.as_str()).unwrap_or_default();
        Ok(former.iter().map(|old| format!("{stem}{old}")).collect())
    }

    /// The address, or the short id when it has none.
    pub fn label(&self, id: &DocumentId) -> Result<String, Failure> {
        Ok(self.address(id)?.unwrap_or_else(|| id.short().to_owned()))
    }

    /// The documents of the kind with a head that is not ended and whose field `key` holds
    /// `value`, forked documents included.
    pub fn holders(
        &self,
        kind: KindOf,
        key: &str,
        value: &str,
    ) -> Result<Vec<DocumentId>, Failure> {
        let mut ids = Vec::new();
        for id in self.holding(key, value)? {
            let held = self.document(&id)?.heads().into_iter().any(|head| {
                holds_value(&head.fields, key, value)
                    && read_or_skip(head).is_some_and(|record| {
                        record.content.kind() == kind && record.ending.is_none()
                    })
            });
            if held {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    fn holding(&self, key: &str, value: &str) -> Result<Vec<DocumentId>, Failure> {
        let question = (key.to_owned(), value.to_owned());
        if let Some(answer) = self.holding.borrow().get(&question) {
            return Ok(answer.clone());
        }
        let answer = self.store.holding(key, value)?;
        self.holding.borrow_mut().insert(question, answer.clone());
        Ok(answer)
    }

    pub(super) fn readable(&self, id: &DocumentId) -> Result<Vec<Record>, Failure> {
        Ok(self
            .document(id)?
            .heads()
            .into_iter()
            .filter_map(read_or_skip)
            .collect())
    }

    fn topic_name(&self, topic: &DocumentId) -> Result<Option<Name>, Failure> {
        Ok(self
            .readable(topic)?
            .into_iter()
            .find_map(|record| match record.content {
                Content::Topic(topic) => Some(topic.name),
                _ => None,
            }))
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
            for id in self.holding(key, name.as_str())? {
                if self.document(&id)?.kind().map(Kind::as_str) != Some(kind.word()) {
                    continue;
                }
                let strongest = self
                    .readable(&id)?
                    .iter()
                    .filter(|record| wanted(record))
                    .filter_map(|record| holds(record, name))
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

fn holds_value(fields: &Fields, key: &str, value: &str) -> bool {
    match fields.get(key) {
        Some(Value::String(text)) => text == value,
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(value)),
        _ => false,
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
    use std::cell::Cell;

    use super::*;
    use crate::app::testing::{ENDED, TOPIC, World, fact, found, topic};
    use crate::domain::id::VersionId;
    use crate::domain::ports::StoreError;
    use crate::domain::testing::{MemoryStore, after, first, lantern};

    fn lantern_topic(world: &World, rest: &str) -> DocumentId {
        let version = first(
            &lantern(),
            "topic",
            &format!("name = \"lantern\"\n{TOPIC}{rest}"),
            "\n",
        );
        world.store.put(&version).unwrap();
        lantern()
    }

    fn entry(world: &World, date: &str, name: &str) -> DocumentId {
        world.put(
            "entry",
            &format!(
                "name = \"{name}\"\ndate = {date}\nmachine = \"{}\"\nsummary = \"s\"\n",
                world.host.0.clone().unwrap()
            ),
            "\n",
        )
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
        let today = entry(&world, "2026-10-09", "lamp-driver");
        let earlier = entry(&world, "2026-10-08", "lamp-driver");
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
        let left = after(&[&root], &format!("name = \"lantern\"\n{TOPIC}"), "left\n");
        let right = after(&[&root], &format!("name = \"phone\"\n{TOPIC}"), "right\n");
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
    fn records_are_one_per_head_and_refuse_a_head_that_does_not_read() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let names = |records: Vec<Record>| -> Vec<String> {
            records
                .into_iter()
                .map(|record| match record.content {
                    Content::Topic(topic) => topic.name.to_string(),
                    other => panic!("{other:?}"),
                })
                .collect()
        };
        let lookup = Lookup::new(&world.store);
        let absent = DocumentId::from_bytes([0x42; 16]);
        assert_eq!(lookup.records(&absent).unwrap(), vec![]);
        assert_eq!(names(lookup.records(&lantern).unwrap()), ["lantern"]);

        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let mut heads = Vec::new();
        for name in ["lantern", "phone"] {
            let head = after(&[&root], &format!("name = \"{name}\"\n{TOPIC}"), name);
            world.store.put(&head).unwrap();
            heads.push((head.id.clone(), name));
        }
        heads.sort();
        let in_head_order: Vec<&str> = heads.iter().map(|(_, name)| *name).collect();
        assert_eq!(
            names(Lookup::new(&world.store).records(&lantern).unwrap()),
            in_head_order
        );

        let broken = world.put("topic", "name = \"atlas\"\n", "\n");
        let Err(Failure::Refused(text)) = Lookup::new(&world.store).records(&broken) else {
            panic!("a head that does not read must be refused");
        };
        assert!(text.contains(broken.short()), "{text}");
    }

    #[test]
    fn a_forked_document_prints_the_address_of_its_first_head() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let mut heads = Vec::new();
        for name in ["lantern", "phone"] {
            let head = after(&[&root], &format!("name = \"{name}\"\n{TOPIC}"), name);
            world.store.put(&head).unwrap();
            heads.push((head.id.clone(), name));
        }
        heads.sort();
        let lookup = Lookup::new(&world.store);
        assert_eq!(
            lookup.address(&lantern).unwrap().as_deref(),
            Some(heads[0].1)
        );
    }

    #[test]
    fn a_text_that_is_no_address_is_a_usage_error() {
        let world = World::new();
        let result = Lookup::new(&world.store).find("Lantern");
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    #[test]
    fn one_refuses_none_and_a_collision() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        topic(&world, "lantern", "");
        topic(&world, "lantern", "");
        let lookup = Lookup::new(&world.store);
        let Err(Failure::Refused(text)) = lookup.one("atlas") else {
            panic!("a name held by nothing must be refused");
        };
        assert!(text.contains("names no document"), "{text}");
        let Err(Failure::Refused(text)) = lookup.one("lantern") else {
            panic!("a collision must be refused");
        };
        assert!(text.contains("several documents"), "{text}");
        let phone = topic(&world, "phone", "");
        assert_eq!(Lookup::new(&world.store).one("phone"), Ok(phone));
    }

    #[test]
    fn a_referencable_document_is_of_the_kind_and_has_a_head_that_is_not_ended() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        topic(&world, "atlas", ENDED);
        let driver = entry(&world, "2026-10-09", "lamp-driver");
        let lookup = Lookup::new(&world.store);
        let topic_of = |address| lookup.referencable(address, Some(KindOf::Topic));
        assert_eq!(topic_of("lantern"), Ok(lantern.clone()));
        assert_eq!(
            lookup.referencable("2026-10-09-lamp-driver", None),
            Ok(driver)
        );
        let Err(Failure::Refused(text)) = topic_of("2026-10-09-lamp-driver") else {
            panic!("an entry is no topic");
        };
        assert!(text.contains("is not a topic"), "{text}");
        let Err(Failure::Refused(text)) = lookup.referencable("lantern", Some(KindOf::Entry))
        else {
            panic!("a topic is no entry");
        };
        assert!(text.contains("is not an entry"), "{text}");
        let Err(Failure::Refused(text)) = topic_of("atlas") else {
            panic!("an ended topic must be refused");
        };
        assert!(text.contains("atlas: is ended"), "{text}");
        let Err(Failure::Refused(text)) = topic_of("phone") else {
            panic!("a name held by nothing must be refused");
        };
        assert!(text.contains("phone: names no document"), "{text}");
    }

    #[test]
    fn a_forked_document_is_referencable_while_a_head_is_not_ended() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let left = after(&[&root], &format!("name = \"lantern\"\n{TOPIC}"), "left\n");
        let right = after(
            &[&root],
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "right\n",
        );
        world.store.put(&left).unwrap();
        world.store.put(&right).unwrap();
        assert_eq!(
            Lookup::new(&world.store).referencable("lantern", Some(KindOf::Topic)),
            Ok(lantern.clone())
        );

        let last = after(
            &[&left],
            &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
            "left\n",
        );
        world.store.put(&last).unwrap();
        let Err(Failure::Refused(text)) =
            Lookup::new(&world.store).referencable("lantern", Some(KindOf::Topic))
        else {
            panic!("a fork whose heads are all ended must be refused");
        };
        assert!(text.contains("lantern: is ended"), "{text}");
    }

    #[test]
    fn a_writable_document_has_one_head() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let lookup = Lookup::new(&world.store);
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
            let fork = after(&[&root], &format!("name = \"lantern\"\n{TOPIC}"), body);
            world.store.put(&fork).unwrap();
        }
        let Err(Failure::Refused(text)) = Lookup::new(&world.store).writable(&lantern, "lantern")
        else {
            panic!("a forked document must be refused");
        };
        assert!(
            text.contains("lantern: is forked; resolve it first"),
            "{text}"
        );
    }

    #[test]
    fn a_document_prints_as_its_address_or_its_short_id() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let driver = entry(&world, "2026-10-09", "lamp-driver");
        let followup = world.put(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n"),
            "\n",
        );
        let lookup = Lookup::new(&world.store);
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
    fn holders_are_the_live_documents_of_the_kind() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let pin = fact(&world, &lantern, "lamp-pin", "");
        fact(
            &world,
            &lantern,
            "old-pin",
            "ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n",
        );
        fact(&world, &atlas, "map", "");
        let lookup = Lookup::new(&world.store);
        let mut held = lookup
            .holders(KindOf::Fact, "topic", lantern.as_str())
            .unwrap();
        held.sort();
        let mut expected = vec![relay, pin];
        expected.sort();
        assert_eq!(held, expected);
        assert!(
            lookup
                .holders(KindOf::Topic, "topic", lantern.as_str())
                .unwrap()
                .is_empty()
        );
    }

    struct Counting {
        inner: MemoryStore,
        documents: Cell<usize>,
        holdings: Cell<usize>,
    }

    impl Store for Counting {
        fn document(&self, id: &DocumentId) -> Result<Document, StoreError> {
            self.documents.set(self.documents.get() + 1);
            self.inner.document(id)
        }

        fn put(&self, version: &Version) -> Result<(), StoreError> {
            self.inner.put(version)
        }

        fn of_kind(&self, kind: &Kind) -> Result<Vec<DocumentId>, StoreError> {
            self.inner.of_kind(kind)
        }

        fn holding(&self, key: &str, value: &str) -> Result<Vec<DocumentId>, StoreError> {
            self.holdings.set(self.holdings.get() + 1);
            self.inner.holding(key, value)
        }

        fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
            self.inner.documents_under(prefix)
        }

        fn versions_under(&self, prefix: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError> {
            self.inner.versions_under(prefix)
        }
    }

    #[test]
    fn a_current_name_is_found_with_one_question_and_a_miss_takes_two() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        topic(&world, "phone", "former_names = [\"lantern\", \"lamp\"]\n");
        let atlas = topic(&world, "atlas", ENDED);
        let counting = Counting {
            inner: world.store,
            documents: Cell::new(0),
            holdings: Cell::new(0),
        };
        let asked = |address: &str| {
            let before = counting.holdings.get();
            let found = Lookup::new(&counting).find(address).unwrap();
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
        let counting = Counting {
            inner: world.store,
            documents: Cell::new(0),
            holdings: Cell::new(0),
        };
        let lookup = Lookup::new(&counting);

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
            .holders(KindOf::Fact, "topic", lantern.as_str())
            .unwrap();
        lookup
            .holders(KindOf::Fact, "topic", lantern.as_str())
            .unwrap();
        assert_eq!(counting.documents.get(), documents);
        assert_eq!(counting.holdings.get(), holdings + 1);
    }
}
