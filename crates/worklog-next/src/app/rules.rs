use std::borrow::Borrow;
use std::fmt;
use std::rc::Rc;

use crate::app::Failure;
use crate::app::heads::{all_ended, kind_of, label_or_short, readable_heads, unended_heads};
use crate::app::lookup::{Lookup, no_such_document, not_a};
use crate::domain::document::{Document, State};
use crate::domain::id::DocumentId;
use crate::domain::schema::graph::{Edges, closes_cycle};
use crate::domain::schema::kind::Reference;
use crate::domain::schema::{Claim, Content, KindOf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Misfit {
    Missing,
    NotA(KindOf),
    Unread,
    Ended,
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Misfit::Missing => f.write_str("is no document"),
            Misfit::NotA(kind) => f.write_str(&not_a(*kind)),
            Misfit::Unread => f.write_str("does not read"),
            Misfit::Ended => f.write_str("is ended"),
        }
    }
}

pub(super) fn holds_topic_open(reference: &Reference) -> bool {
    reference.holds_open && reference.target == Some(KindOf::Topic)
}

pub(super) fn closing_edge<E: Borrow<Edges>>(
    topic: &DocumentId,
    edges: &Edges,
    edges_of: impl Fn(&DocumentId) -> Option<E> + Copy,
) -> Option<&'static str> {
    let closes = |part_of: &[DocumentId], uses: &[DocumentId]| {
        let through = Edges {
            part_of: part_of.to_vec(),
            uses: uses.to_vec(),
        };
        closes_cycle(topic, &through, edges_of)
    };
    if closes(&edges.part_of, &[]) {
        Some("part_of")
    } else if closes(&[], &edges.uses) {
        Some("uses")
    } else {
        None
    }
}

fn same_scope(one: &Content, other: &Content) -> bool {
    match (one, other) {
        (Content::Topic(_), Content::Topic(_)) => true,
        (Content::Fact(one), Content::Fact(other)) => one.topic == other.topic,
        (Content::Entry(one), Content::Entry(other)) => one.date == other.date,
        _ => false,
    }
}

pub(super) fn referencable(lookup: &Lookup, address: &str) -> Result<DocumentId, Failure> {
    let id = lookup.one(address)?;
    match fits(lookup, &id, Some(KindOf::Topic), true)? {
        None => Ok(id),
        Some(Misfit::Missing) => Err(no_such_document(address)),
        Some(misfit) => Err(Failure::at(address, misfit)),
    }
}

pub(super) fn fits(
    lookup: &Lookup,
    id: &DocumentId,
    kind: Option<KindOf>,
    unended: bool,
) -> Result<Option<Misfit>, Failure> {
    let document = lookup.document(id)?;
    if matches!(document.state(), State::Absent) {
        return Ok(Some(Misfit::Missing));
    }
    if let Some(kind) = kind
        && kind_of(&document) != Some(kind)
    {
        return Ok(Some(Misfit::NotA(kind)));
    }
    let heads = readable_heads(&document);
    if heads.is_empty() {
        return Ok(Some(Misfit::Unread));
    }
    if unended && all_ended(&heads) {
        return Ok(Some(Misfit::Ended));
    }
    Ok(None)
}

pub(super) fn other_holders<'i>(
    lookup: &Lookup,
    id: &DocumentId,
    content: &Content,
    candidates: impl IntoIterator<Item = &'i DocumentId>,
) -> Result<Vec<DocumentId>, Failure> {
    let mut holders = Vec::new();
    for candidate in candidates {
        if candidate != id && holds_name_of(lookup, candidate, content)? {
            holders.push(candidate.clone());
        }
    }
    Ok(holders)
}

fn holds_name_of(lookup: &Lookup, holder: &DocumentId, content: &Content) -> Result<bool, Failure> {
    let name = content.naming().map(|(name, _)| name);
    let document = lookup.document(holder)?;
    Ok(unended_heads(&document).iter().any(|(_, other)| {
        same_scope(content, &other.content) && other.content.naming().map(|(name, _)| name) == name
    }))
}

pub(super) fn name_is_free(
    lookup: &Lookup,
    id: &DocumentId,
    content: &Content,
) -> Result<(), Failure> {
    let what = match lookup.address(id)? {
        Some(address) => address,
        None => label_or_short(lookup.address_of(content)?, id),
    };
    name_is_free_as(lookup, id, &what, content)
}

pub(super) fn name_is_free_as(
    lookup: &Lookup,
    id: &DocumentId,
    what: &str,
    content: &Content,
) -> Result<(), Failure> {
    let Some((name, _)) = content.naming() else {
        return Ok(());
    };
    let candidates = lookup.holders(content.kind(), "name", &[name.as_str()], false)?;
    let candidates = candidates.iter().map(|candidate| candidate.id());
    match other_holders(lookup, id, content, candidates)?.first() {
        Some(holder) => Err(Failure::at(
            what,
            format!("name: {name} is taken by {}", lookup.label(holder)?),
        )),
        None => Ok(()),
    }
}

// A forked claim is given once for each distinct topic and directory its unended heads hold.
pub(super) fn claims_of(
    lookup: &Lookup,
    machine: &DocumentId,
) -> Result<Vec<(Rc<Document>, Claim)>, Failure> {
    let to_machine =
        |reference: &Reference| reference.target == Some(KindOf::Topic) && !reference.membership;
    let Some(key) = KindOf::Claim.key_where(to_machine) else {
        return Ok(Vec::new());
    };
    let mut claims = Vec::new();
    for document in lookup.holders(KindOf::Claim, key, &[machine], false)? {
        let mut held: Vec<Claim> = Vec::new();
        for (_, record) in unended_heads(&document) {
            if let Content::Claim(claim) = record.content
                && claim.machine == *machine
                && !held.contains(&claim)
            {
                held.push(claim);
            }
        }
        claims.extend(held.into_iter().map(|claim| (Rc::clone(&document), claim)));
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{ENDED, World, entry, topic, topic_fields};
    use crate::domain::ports::Store;
    use crate::domain::testing::after;

    #[test]
    fn a_referencable_document_is_a_topic_with_a_head_that_is_not_ended() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        topic(&world, "atlas", ENDED);
        entry(&world, "2026-10-09", "lamp-driver", &[]);
        let lookup = world.lookup();
        let topic_of = |address| referencable(&lookup, address);
        assert_eq!(topic_of("lantern"), Ok(lantern.clone()));
        let Err(Failure::Refused(text)) = topic_of("2026-10-09-lamp-driver") else {
            panic!("an entry is no topic");
        };
        assert!(text.contains("is not a topic"), "{text}");
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
    fn a_document_is_referencable_by_the_heads_that_read() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let live = after(&[&root], &topic_fields("lantern", ""), "live\n");
        world.store.put(&live).unwrap();
        world
            .store
            .put(&after(&[&root], "name = \"lantern\"\n", "unread\n"))
            .unwrap();
        assert_eq!(
            referencable(&world.lookup(), "lantern"),
            Ok(lantern.clone())
        );

        let ended = after(&[&live], &topic_fields("lantern", ENDED), "live\n");
        world.store.put(&ended).unwrap();
        let Err(Failure::Refused(text)) = referencable(&world.lookup(), "lantern") else {
            panic!("a document whose readable heads are all ended must be refused");
        };
        assert_eq!(text, "lantern: is ended");

        let broken = world.put("topic", "name = \"atlas\"\n", "\n");
        let Err(Failure::Refused(text)) = referencable(&world.lookup(), broken.as_str()) else {
            panic!("a document no head of which reads must be refused");
        };
        assert_eq!(text, format!("{broken}: does not read"));
    }

    #[test]
    fn a_forked_document_is_referencable_while_a_head_is_not_ended() {
        let world = World::new();
        let _machine = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", "");
        let root = world.store.document(&lantern).unwrap().heads()[0].clone();
        let left = after(&[&root], &topic_fields("lantern", ""), "left\n");
        let right = after(&[&root], &topic_fields("lantern", ENDED), "right\n");
        world.store.put(&left).unwrap();
        world.store.put(&right).unwrap();
        assert_eq!(
            referencable(&world.lookup(), "lantern"),
            Ok(lantern.clone())
        );

        let last = after(&[&left], &topic_fields("lantern", ENDED), "left\n");
        world.store.put(&last).unwrap();
        let Err(Failure::Refused(text)) = referencable(&world.lookup(), "lantern") else {
            panic!("a fork whose heads are all ended must be refused");
        };
        assert!(text.contains("lantern: is ended"), "{text}");
    }
}
