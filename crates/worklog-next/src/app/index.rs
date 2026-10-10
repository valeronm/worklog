use std::collections::{BTreeMap, BTreeSet};

use crate::app::Failure;
use crate::app::heads::{all_ended, edges_of, label_or_short, readable_heads, row_head_of};
use crate::app::lookup::Lookup;
use crate::domain::id::DocumentId;
use crate::domain::schema::graph::{self, Edges};
use crate::domain::schema::{Content, KindOf, Name};

struct Entry {
    name: Name,
    summary: String,
    ended: bool,
    edges: Edges,
}

pub(super) struct Topics {
    entries: BTreeMap<DocumentId, Entry>,
    parts: BTreeMap<DocumentId, Vec<DocumentId>>,
}

impl Topics {
    pub(super) fn load(lookup: &Lookup) -> Result<Topics, Failure> {
        let mut entries = BTreeMap::new();
        for document in lookup.of_kind(KindOf::Topic)? {
            let heads = readable_heads(&document);
            let Some((_, shown)) = row_head_of(&heads) else {
                continue;
            };
            let Content::Topic(shown) = &shown.content else {
                continue;
            };
            entries.insert(
                document.id().clone(),
                Entry {
                    name: shown.name.clone(),
                    summary: shown.summary.clone(),
                    ended: all_ended(&heads),
                    edges: edges_of(&heads),
                },
            );
        }
        let mut parts: BTreeMap<DocumentId, Vec<DocumentId>> = BTreeMap::new();
        for (id, entry) in &entries {
            for parent in &entry.edges.part_of {
                parts.entry(parent.clone()).or_default().push(id.clone());
            }
        }
        Ok(Topics { entries, parts })
    }

    pub(super) fn name(&self, id: &DocumentId) -> Option<&Name> {
        self.entries.get(id).map(|entry| &entry.name)
    }

    pub(super) fn label(&self, id: &DocumentId) -> String {
        label_or_short(self.name(id).map(ToString::to_string), id)
    }

    pub(super) fn summary(&self, id: &DocumentId) -> Option<&str> {
        self.entries.get(id).map(|entry| entry.summary.as_str())
    }

    pub(super) fn is_ended(&self, id: &DocumentId) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.ended)
    }

    fn unended(&self, id: &DocumentId) -> Option<&Entry> {
        self.entries.get(id).filter(|entry| !entry.ended)
    }

    pub(super) fn edges(&self, id: &DocumentId) -> Option<&Edges> {
        self.unended(id).map(|entry| &entry.edges)
    }

    pub(super) fn part_of(&self, id: &DocumentId) -> &[DocumentId] {
        self.entries
            .get(id)
            .map_or(&[], |entry| entry.edges.part_of.as_slice())
    }

    pub(super) fn uses(&self, id: &DocumentId) -> &[DocumentId] {
        self.entries
            .get(id)
            .map_or(&[], |entry| entry.edges.uses.as_slice())
    }

    pub(super) fn covered(&self, id: &DocumentId) -> BTreeSet<DocumentId> {
        let mut covered: BTreeSet<DocumentId> = graph::parts(id, |topic| self.direct_parts(topic))
            .into_iter()
            .collect();
        covered.insert(id.clone());
        covered
    }

    pub(super) fn direct_parts(&self, id: &DocumentId) -> Vec<DocumentId> {
        self.parts.get(id).cloned().unwrap_or_default()
    }

    pub(super) fn reach(&self, from: &[DocumentId]) -> Vec<DocumentId> {
        let starts: Vec<DocumentId> = from
            .iter()
            .filter(|id| self.unended(id).is_some())
            .cloned()
            .collect();
        let mut reached = graph::reach(&starts, |id| self.edges(id));
        reached.retain(|id| self.unended(id).is_some());
        reached
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{ENDED, TOPIC, World, head, part_of, topic, uses, written_at};
    use crate::domain::ports::Store;

    fn chain() -> (World, [DocumentId; 4]) {
        let world = World::new();
        topic(&world, "desk", "");
        let root = topic(&world, "lantern", "");
        let middle = topic(&world, "atlas", &part_of(&[&root]));
        let leaf = topic(&world, "phone", &part_of(&[&middle]));
        (
            world,
            [root, middle, leaf, DocumentId::from_bytes([0x42; 16])],
        )
    }

    #[test]
    fn a_topic_covers_itself_and_its_parts_at_every_depth() {
        let (world, [root, middle, leaf, unknown]) = chain();
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(
            topics.covered(&root),
            BTreeSet::from([root.clone(), middle.clone(), leaf.clone()])
        );
        assert_eq!(
            topics.covered(&middle),
            BTreeSet::from([middle.clone(), leaf.clone()])
        );
        assert_eq!(topics.covered(&leaf), BTreeSet::from([leaf.clone()]));
        assert_eq!(topics.covered(&unknown), BTreeSet::from([unknown]));
        assert_eq!(topics.direct_parts(&root), std::slice::from_ref(&middle));
        assert_eq!(topics.direct_parts(&leaf), []);
    }

    #[test]
    fn an_ended_part_is_left_out_with_what_hangs_from_it() {
        let (world, [root, middle, leaf, _]) = chain();
        world.amend(
            &middle,
            &format!("name = \"atlas\"\n{TOPIC}{}{ENDED}", part_of(&[&root])),
            "\n",
        );
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(topics.covered(&root), BTreeSet::from([root.clone()]));
        assert_eq!(topics.direct_parts(&root), []);
        assert!(topics.is_ended(&middle));
        assert_eq!(
            topics.covered(&middle),
            BTreeSet::from([middle.clone(), leaf])
        );
        assert_eq!(topics.part_of(&middle), []);
    }

    #[test]
    fn reach_goes_up_through_part_of_and_uses_each_topic_once() {
        let world = World::new();
        topic(&world, "desk", "");
        let base = topic(&world, "lantern", "");
        let left = topic(&world, "atlas", &part_of(&[&base]));
        let right = topic(&world, "phone", &part_of(&[&base]));
        let top = topic(
            &world,
            "relay",
            &format!("{}{}", part_of(&[&left]), uses(&[&right])),
        );
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(
            topics.reach(std::slice::from_ref(&top)),
            [top.clone(), left.clone(), right.clone(), base.clone()]
        );
        assert_eq!(
            topics.reach(&[left.clone(), top.clone(), left.clone()]),
            [left.clone(), top.clone(), base.clone(), right.clone()]
        );
        assert_eq!(topics.reach(&[]), []);
        let unknown = DocumentId::from_bytes([0x42; 16]);
        assert_eq!(topics.reach(&[unknown]), []);
        assert_eq!(topics.uses(&top), [right]);
        assert_eq!(topics.part_of(&top), [left]);
    }

    #[test]
    fn reach_neither_returns_nor_walks_through_an_ended_topic() {
        let world = World::new();
        topic(&world, "desk", "");
        let base = topic(&world, "lantern", "");
        let middle = topic(&world, "atlas", &part_of(&[&base]));
        let top = topic(&world, "phone", &part_of(&[&middle]));
        world.amend(
            &middle,
            &format!("name = \"atlas\"\n{TOPIC}{}{ENDED}", part_of(&[&base])),
            "\n",
        );
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(
            topics.reach(std::slice::from_ref(&top)),
            std::slice::from_ref(&top)
        );
        assert_eq!(topics.reach(std::slice::from_ref(&middle)), []);
    }

    #[test]
    fn reach_ends_on_a_cycle() {
        let world = World::new();
        topic(&world, "desk", "");
        let first = DocumentId::from_bytes([0x11; 16]);
        let second = DocumentId::from_bytes([0x22; 16]);
        for (id, name, parent) in [(&first, "lantern", &second), (&second, "atlas", &first)] {
            let fields = format!("name = \"{name}\"\n{TOPIC}{}", part_of(&[parent]));
            world
                .store
                .put(&crate::domain::testing::first(id, "topic", &fields, "\n"))
                .unwrap();
        }
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(
            topics.reach(std::slice::from_ref(&first)),
            [first.clone(), second.clone()]
        );
        assert_eq!(
            topics.covered(&first),
            BTreeSet::from([first.clone(), second.clone()])
        );
    }

    #[test]
    fn a_forked_topic_has_the_edges_of_its_unended_heads() {
        let world = World::new();
        topic(&world, "desk", "");
        let one = topic(&world, "lantern", "");
        let two = topic(&world, "atlas", "");
        let three = topic(&world, "phone", "");
        let fork_id = topic(&world, "relay", &part_of(&[&one]));
        let root = head(&world, &fork_id);
        let fields = |rest: String| format!("name = \"relay\"\n{TOPIC}{rest}");
        let left = written_at(
            &world,
            &root,
            &fields(format!("{}{}", part_of(&[&one, &two]), uses(&[&three]))),
            "2026-10-09T10:00:00+01:00",
            "left\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fields(format!("{}{}", part_of(&[&three]), ENDED)),
            "2026-10-09T11:00:00+01:00",
            "right\n",
            None,
        );
        let topics = Topics::load(&world.lookup()).unwrap();
        assert!(!topics.is_ended(&fork_id));
        assert_eq!(topics.part_of(&fork_id), [one.clone(), two.clone()]);
        assert_eq!(topics.uses(&fork_id), std::slice::from_ref(&three));
        assert_eq!(topics.direct_parts(&three), []);
        assert_eq!(topics.direct_parts(&two), std::slice::from_ref(&fork_id));
        assert_eq!(topics.name(&fork_id).unwrap().as_str(), "relay");

        written_at(
            &world,
            &left,
            &fields(format!("{}{}", part_of(&[&two]), ENDED)),
            "2026-10-09T12:00:00+01:00",
            "left again\n",
            None,
        );
        let topics = Topics::load(&world.lookup()).unwrap();
        assert!(topics.is_ended(&fork_id));
        assert_eq!(topics.part_of(&fork_id), []);
    }

    #[test]
    fn a_forked_topic_is_named_by_its_row_head() {
        let world = World::new();
        topic(&world, "desk", "");
        let id = topic(&world, "lantern", "");
        let root = head(&world, &id);
        let fields = |name: &str| format!("name = \"{name}\"\n{TOPIC}");
        written_at(
            &world,
            &root,
            &fields("atlas"),
            "2026-10-09T12:00:00+01:00",
            "a\n",
            None,
        );
        written_at(
            &world,
            &root,
            &fields("phone"),
            "2026-10-09T11:00:00+01:00",
            "b\n",
            None,
        );
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(topics.label(&id), "atlas");
        assert_eq!(topics.name(&id).unwrap().as_str(), "atlas");
        assert_eq!(topics.summary(&id), Some("s"));
    }

    #[test]
    fn an_id_that_is_no_topic_is_known_by_its_short_id() {
        let world = World::new();
        topic(&world, "desk", "");
        let broken = world.put("topic", "name = \"atlas\"\n", "\n");
        let unknown = DocumentId::from_bytes([0x42; 16]);
        let topics = Topics::load(&world.lookup()).unwrap();
        assert_eq!(topics.label(&unknown), unknown.short());
        assert_eq!(topics.name(&unknown), None);
        assert_eq!(topics.summary(&unknown), None);
        assert!(!topics.is_ended(&unknown));
        assert_eq!(topics.part_of(&unknown), []);
        assert_eq!(topics.uses(&unknown), []);
        assert_eq!(topics.name(&broken), None);
    }
}
