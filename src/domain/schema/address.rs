//! How a document is named on a command line and in a link, and which
//! document a name means when several hold it.

use super::error::SchemaError;
use super::field::{Date, Name, dated};
use super::kind::{Content, Record};
use crate::domain::id::{DocumentId, is_id_prefix};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    Topic(Name),
    Fact {
        topic: Name,
        name: Name,
    },
    Entry {
        date: Date,
        name: Name,
    },
    /// The start of a document's or a version's id.
    Id(String),
}

impl Address {
    pub fn parse(text: &str) -> Result<Address, SchemaError> {
        let wrong = || SchemaError::NotAnAddress(text.to_owned());
        if let Some((topic, name)) = text.split_once('/') {
            return Ok(Address::Fact {
                topic: Name::parse(topic).map_err(|_| wrong())?,
                name: Name::parse(name).map_err(|_| wrong())?,
            });
        }
        if let Some((date, rest)) = dated(text) {
            let name = rest.strip_prefix('-').ok_or_else(wrong)?;
            return Ok(Address::Entry {
                date,
                name: Name::parse(name).map_err(|_| wrong())?,
            });
        }
        match Name::parse(text) {
            Ok(name) => Ok(Address::Topic(name)),
            Err(_) if is_id_prefix(text) => Ok(Address::Id(text.to_owned())),
            Err(_) => Err(wrong()),
        }
    }

    #[must_use]
    pub fn id_prefix(text: &str) -> Option<&str> {
        is_id_prefix(text).then_some(text)
    }
}

/// A fact's address needs its topic's name, and a followup and a claim
/// have none.
#[must_use]
pub fn displayed(content: &Content, topic: Option<&Name>) -> Option<String> {
    match content {
        Content::Topic(topic) => Some(topic.name.to_string()),
        Content::Fact(fact) => topic.map(|topic| format!("{topic}/{}", fact.name)),
        Content::Entry(entry) => Some(format!("{}-{}", entry.date, entry.name)),
        Content::Followup(_) | Content::Claim(_) => None,
    }
}

/// The variants are ordered strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Holds {
    Current,
    /// As the current name of an ended document.
    Ended,
    Former,
}

#[must_use]
pub fn holds(record: &Record, name: &Name) -> Option<Holds> {
    let (current, former) = record.content.naming()?;
    if current == name {
        Some(if record.ending.is_some() {
            Holds::Ended
        } else {
            Holds::Current
        })
    } else {
        former.contains(name).then_some(Holds::Former)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub id: DocumentId,
    pub holds: Holds,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    None,
    One(DocumentId),
    Collision(Vec<DocumentId>),
}

/// The document a name means among those holding it: the strongest
/// way of holding it decides, and several holding it that way collide.
#[must_use]
pub fn choose(candidates: &[Candidate]) -> Found {
    let Some(best) = candidates.iter().map(|candidate| candidate.holds).min() else {
        return Found::None;
    };
    let mut held: Vec<DocumentId> = candidates
        .iter()
        .filter(|candidate| candidate.holds == best)
        .map(|candidate| candidate.id.clone())
        .collect();
    held.sort();
    held.dedup();
    match held.len() {
        1 => Found::One(held.remove(0)),
        _ => Found::Collision(held),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::record;
    use super::*;
    use crate::domain::testing::{LANTERN, lantern};

    fn name(text: &str) -> Name {
        Name::parse(text).unwrap()
    }

    fn fact(rest: &str) -> Record {
        record(
            "fact",
            &format!(
                "name = \"relay-pin\"\nformer_names = [\"relay\"]\ntopic = \"{LANTERN}\"\n\
                 created = 2026-09-04\nconfirmed = 2026-09-04\nsummary = \"s\"\n{rest}"
            ),
        )
    }

    #[test]
    fn each_shape_of_address_parses_to_its_kind() {
        assert_eq!(
            Address::parse("lantern"),
            Ok(Address::Topic(name("lantern")))
        );
        assert_eq!(
            Address::parse("lantern/relay-pin"),
            Ok(Address::Fact {
                topic: name("lantern"),
                name: name("relay-pin")
            })
        );
        assert_eq!(
            Address::parse("2026-10-09-lamp-driver"),
            Ok(Address::Entry {
                date: Date::parse("2026-10-09").unwrap(),
                name: name("lamp-driver")
            })
        );
        for id in ["2026", "00112233", LANTERN] {
            assert_eq!(Address::parse(id), Ok(Address::Id(id.to_owned())));
        }
        for bad in [
            "",
            "Lantern",
            "lantern/",
            "a/b/c",
            "2026-10-09",
            "2026-10-09lamp",
            "b3-",
            "x y",
        ] {
            assert_eq!(
                Address::parse(bad),
                Err(SchemaError::NotAnAddress(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_name_that_is_also_hex_is_a_name_first_and_an_id_to_fall_back_on() {
        for both in ["decade", "7f3a91c0", "b3-0a1b2c"] {
            assert_eq!(Address::parse(both), Ok(Address::Topic(name(both))));
            assert_eq!(Address::id_prefix(both), Some(both));
        }
        assert_eq!(Address::id_prefix("lantern"), None);
    }

    #[test]
    fn a_named_kind_prints_its_address() {
        let topic = record(
            "topic",
            "name = \"lantern\"\ncreated = 2026-09-04\nsummary = \"s\"\n",
        );
        let entry = record(
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-09-04\nmachine = \"{LANTERN}\"\nsummary = \"s\"\n"
            ),
        );
        let followup = record(
            "followup",
            &format!("created = 2026-09-04\ntopics = [\"{LANTERN}\"]\nsummary = \"s\"\n"),
        );
        let lantern = name("lantern");
        assert_eq!(displayed(&topic.content, None).unwrap(), "lantern");
        assert_eq!(
            displayed(&fact("").content, Some(&lantern)).unwrap(),
            "lantern/relay-pin"
        );
        assert_eq!(displayed(&fact("").content, None), None);
        assert_eq!(
            displayed(&entry.content, None).unwrap(),
            "2026-09-04-lamp-driver"
        );
        assert_eq!(displayed(&followup.content, Some(&lantern)), None);
    }

    #[test]
    fn a_record_holds_its_name_its_former_names_and_no_other() {
        let ended = fact("ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n");
        assert_eq!(holds(&fact(""), &name("relay-pin")), Some(Holds::Current));
        assert_eq!(holds(&ended, &name("relay-pin")), Some(Holds::Ended));
        assert_eq!(holds(&fact(""), &name("relay")), Some(Holds::Former));
        assert_eq!(holds(&ended, &name("relay")), Some(Holds::Former));
        assert_eq!(holds(&fact(""), &name("lamp")), None);
    }

    #[test]
    fn the_strongest_holder_is_chosen_and_equals_collide() {
        let atlas = DocumentId::from_bytes([0xa7; 16]);
        let phone = DocumentId::from_bytes([0xb8; 16]);
        let held = |id: &DocumentId, holds| Candidate {
            id: id.clone(),
            holds,
        };
        assert_eq!(choose(&[]), Found::None);
        assert_eq!(
            choose(&[held(&lantern(), Holds::Former)]),
            Found::One(lantern())
        );
        assert_eq!(
            choose(&[
                held(&lantern(), Holds::Former),
                held(&atlas, Holds::Ended),
                held(&phone, Holds::Current)
            ]),
            Found::One(phone.clone())
        );
        assert_eq!(
            choose(&[held(&lantern(), Holds::Former), held(&atlas, Holds::Ended)]),
            Found::One(atlas.clone())
        );
        assert_eq!(
            choose(&[
                held(&phone, Holds::Current),
                held(&lantern(), Holds::Current)
            ]),
            Found::Collision(vec![lantern(), phone.clone()])
        );
        assert_eq!(
            choose(&[held(&atlas, Holds::Former), held(&phone, Holds::Former)]),
            Found::Collision(vec![atlas, phone])
        );
    }
}
