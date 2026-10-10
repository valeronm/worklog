//! How topics reach each other: upward through `part_of` and `uses`,
//! and downward through `part_of` alone.

use std::collections::VecDeque;

use crate::domain::id::DocumentId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub part_of: Vec<DocumentId>,
    pub uses: Vec<DocumentId>,
}

fn walk(roots: &[DocumentId], next: impl Fn(&DocumentId) -> Vec<DocumentId>) -> Vec<DocumentId> {
    let mut reached: Vec<DocumentId> = Vec::new();
    let mut queue: VecDeque<DocumentId> = roots.iter().cloned().collect();
    while let Some(topic) = queue.pop_front() {
        if reached.contains(&topic) {
            continue;
        }
        queue.extend(next(&topic));
        reached.push(topic);
    }
    reached
}

/// The roots and every topic they reach through either edge, breadth
/// first, each once.
#[must_use]
pub fn reach(
    roots: &[DocumentId],
    edges: impl Fn(&DocumentId) -> Option<Edges>,
) -> Vec<DocumentId> {
    walk(roots, |topic| {
        edges(topic)
            .map(|edges| [edges.part_of, edges.uses].concat())
            .unwrap_or_default()
    })
}

/// Whether giving `topic` these edges would let it reach itself.
#[must_use]
pub fn closes_cycle(
    topic: &DocumentId,
    proposed: &Edges,
    edges: impl Fn(&DocumentId) -> Option<Edges>,
) -> bool {
    let targets = [proposed.part_of.as_slice(), proposed.uses.as_slice()].concat();
    reach(&targets, edges).contains(topic)
}

/// Every topic that is part of `topic`, directly or through its parts.
#[must_use]
pub fn parts(
    topic: &DocumentId,
    parts_of: impl Fn(&DocumentId) -> Vec<DocumentId>,
) -> Vec<DocumentId> {
    let mut reached = walk(std::slice::from_ref(topic), parts_of);
    reached.retain(|part| part != topic);
    reached
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn id(byte: u8) -> DocumentId {
        DocumentId::from_bytes([byte; 16])
    }

    fn graph() -> BTreeMap<DocumentId, Edges> {
        let (settle, server, web, rust, postgres) = (id(1), id(2), id(3), id(4), id(5));
        BTreeMap::from([
            (settle.clone(), Edges::default()),
            (
                server,
                Edges {
                    part_of: vec![settle.clone()],
                    uses: vec![rust.clone(), postgres.clone()],
                },
            ),
            (
                web,
                Edges {
                    part_of: vec![settle],
                    uses: vec![rust.clone()],
                },
            ),
            (rust, Edges::default()),
            (postgres, Edges::default()),
        ])
    }

    #[test]
    fn a_walk_goes_up_both_edges_and_meets_each_topic_once() {
        let graph = graph();
        let edges = |topic: &DocumentId| graph.get(topic).cloned();
        assert_eq!(reach(&[id(2)], edges), [id(2), id(1), id(4), id(5)]);
        assert_eq!(
            reach(&[id(2), id(3)], edges),
            [id(2), id(3), id(1), id(4), id(5)]
        );
        assert_eq!(reach(&[id(1)], edges), [id(1)]);
        assert_eq!(reach(&[id(9)], edges), [id(9)], "a topic nobody holds");
        assert!(reach(&[], edges).is_empty());
    }

    #[test]
    fn a_cycle_already_in_the_data_ends_the_walk() {
        let mut graph = graph();
        graph.get_mut(&id(1)).unwrap().uses.push(id(2));
        let edges = |topic: &DocumentId| graph.get(topic).cloned();
        assert_eq!(reach(&[id(2)], edges), [id(2), id(1), id(4), id(5)]);
    }

    #[test]
    fn an_edge_to_a_topic_that_reaches_back_closes_a_cycle() {
        let graph = graph();
        let edges = |topic: &DocumentId| graph.get(topic).cloned();
        let through = |part_of: Vec<DocumentId>, uses: Vec<DocumentId>| Edges { part_of, uses };
        assert!(closes_cycle(&id(1), &through(vec![id(2)], vec![]), edges));
        assert!(closes_cycle(&id(4), &through(vec![], vec![id(3)]), edges));
        assert!(closes_cycle(&id(1), &through(vec![id(1)], vec![]), edges));
        assert!(!closes_cycle(&id(4), &through(vec![], vec![id(5)]), edges));
        assert!(!closes_cycle(
            &id(2),
            &through(vec![id(1)], vec![id(4)]),
            edges
        ));
    }

    #[test]
    fn a_rollup_covers_parts_of_parts_and_not_what_only_uses() {
        let mut graph = graph();
        graph.insert(
            id(6),
            Edges {
                part_of: vec![id(2)],
                uses: vec![],
            },
        );
        let parts_of = |topic: &DocumentId| -> Vec<DocumentId> {
            graph
                .iter()
                .filter(|(_, edges)| edges.part_of.contains(topic))
                .map(|(part, _)| part.clone())
                .collect()
        };
        assert_eq!(parts(&id(1), parts_of), [id(2), id(3), id(6)]);
        assert_eq!(parts(&id(2), parts_of), [id(6)]);
        assert!(parts(&id(4), parts_of).is_empty());
    }
}
