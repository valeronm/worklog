//! Names in place of ids: a draft shows a reference as the name a
//! session would type, and a save turns it back into the id.

use std::convert::Infallible;

use toml::Value;

use super::address::Found;
use super::error::SchemaError;
use super::kind::KindOf;
use crate::domain::id::DocumentId;
use crate::domain::version::Fields;

fn each<E>(
    fields: &Fields,
    kind: KindOf,
    turn: impl Fn(&str, &str) -> Result<String, E>,
) -> Result<Fields, E> {
    let mut turned = fields.clone();
    for key in kind.references() {
        match turned.get_mut(*key) {
            Some(Value::String(text)) => *text = turn(key, text)?,
            Some(Value::Array(items)) => {
                for item in items {
                    if let Value::String(text) = item {
                        *text = turn(key, text)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(turned)
}

/// The fields with each reference shown as its document's name; one to
/// a document with no name stays the whole id.
#[must_use]
pub fn to_names(
    fields: &Fields,
    kind: KindOf,
    name_of: impl Fn(&DocumentId) -> Option<String>,
) -> Fields {
    let shown = each(fields, kind, |_, text| {
        let name = DocumentId::parse(text).ok().and_then(|id| name_of(&id));
        Ok::<_, Infallible>(name.unwrap_or_else(|| text.to_owned()))
    });
    match shown {
        Ok(shown) => shown,
        Err(never) => match never {},
    }
}

/// The fields with each reference as an id: a whole id passes, and a
/// name is the document `id_of` finds for it.
pub fn to_ids(
    fields: &Fields,
    kind: KindOf,
    id_of: impl Fn(&str) -> Found,
) -> Result<Fields, SchemaError> {
    each(fields, kind, |key, text| {
        if DocumentId::parse(text).is_ok() {
            return Ok(text.to_owned());
        }
        match id_of(text) {
            Found::One(id) => Ok(id.to_string()),
            Found::None => Err(SchemaError::NoDocument {
                key: key.to_owned(),
                name: text.to_owned(),
            }),
            Found::Collision(_) => Err(SchemaError::Collision {
                key: key.to_owned(),
                name: text.to_owned(),
            }),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::fields;
    use super::*;
    use crate::domain::testing::{ATLAS, LANTERN, atlas, lantern};

    fn name_of(id: &DocumentId) -> Option<String> {
        match id.as_str() {
            LANTERN => Some("lantern".to_owned()),
            ATLAS => Some("atlas".to_owned()),
            _ => None,
        }
    }

    fn id_of(name: &str) -> Found {
        match name {
            "lantern" => Found::One(lantern()),
            "atlas" => Found::One(atlas()),
            "phone" => Found::Collision(vec![lantern(), atlas()]),
            _ => Found::None,
        }
    }

    #[test]
    fn a_kinds_references_turn_to_names_and_back() {
        let stored = fields(&format!(
            "created = 2026-09-04\ntopics = [\"{LANTERN}\", \"{ATLAS}\"]\nabout = \"{ATLAS}\"\n\
             touching = \"{LANTERN}\"\nsummary = \"{LANTERN}\"\n"
        ));
        let shown = to_names(&stored, KindOf::Followup, name_of);
        assert_eq!(
            toml::to_string(&shown).unwrap(),
            format!(
                "created = 2026-09-04\ntopics = [\"lantern\", \"atlas\"]\nabout = \"atlas\"\n\
                 touching = \"lantern\"\nsummary = \"{LANTERN}\"\n"
            ),
            "a summary that looks like an id is no reference"
        );
        assert_eq!(to_ids(&shown, KindOf::Followup, id_of), Ok(stored));

        let fact = fields(&format!("name = \"relay\"\ntopic = \"{LANTERN}\"\n"));
        let shown = to_names(&fact, KindOf::Fact, name_of);
        assert_eq!(shown.get("topic").and_then(Value::as_str), Some("lantern"));
        assert_eq!(to_ids(&shown, KindOf::Fact, id_of), Ok(fact));
    }

    #[test]
    fn a_reference_to_a_document_with_no_name_stays_its_whole_id_both_ways() {
        let unnamed = DocumentId::from_bytes([0xb8; 16]);
        let stored = fields(&format!(
            "entry = \"{unnamed}\"\ntopics = [\"{LANTERN}\"]\n"
        ));
        let shown = to_names(&stored, KindOf::Followup, name_of);
        assert_eq!(
            shown.get("entry").and_then(Value::as_str),
            Some(unnamed.as_str())
        );
        assert_eq!(to_ids(&shown, KindOf::Followup, id_of), Ok(stored));
    }

    #[test]
    fn a_name_no_document_holds_and_one_several_hold_are_refused_by_key() {
        let draft = |topic: &str| fields(&format!("name = \"relay\"\ntopic = \"{topic}\"\n"));
        assert_eq!(
            to_ids(&draft("desk"), KindOf::Fact, id_of),
            Err(SchemaError::NoDocument {
                key: "topic".to_owned(),
                name: "desk".to_owned()
            })
        );
        assert_eq!(
            to_ids(&draft("phone"), KindOf::Fact, id_of),
            Err(SchemaError::Collision {
                key: "topic".to_owned(),
                name: "phone".to_owned()
            })
        );
        let list = fields("topics = [\"lantern\", \"desk\"]\n");
        assert!(matches!(
            to_ids(&list, KindOf::Entry, id_of),
            Err(SchemaError::NoDocument { key, name }) if key == "topics" && name == "desk"
        ));
    }
}
