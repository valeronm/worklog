//! The versions of one document that are held, and what they say about
//! which of them is current.

use std::collections::BTreeSet;

use toml::Value;

use super::id::{DocumentId, VersionId};
use super::version::{Kind, ReadError, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub id: VersionId,
    pub why: ReadError,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    id: DocumentId,
    versions: Vec<Version>,
    heads: Vec<usize>,
    unreadable: Vec<Unreadable>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State<'a> {
    Absent,
    Live(&'a Version),
    /// Versions none of which is behind another.
    Forked(Vec<&'a Version>),
}

fn behind(versions: &[Version]) -> BTreeSet<&VersionId> {
    versions
        .iter()
        .flat_map(|version| &version.envelope.ancestors)
        .collect()
}

impl Document {
    #[must_use]
    pub fn new(
        id: DocumentId,
        versions: Vec<Version>,
        mut unreadable: Vec<Unreadable>,
    ) -> Document {
        let (mut versions, strays): (Vec<Version>, Vec<Version>) = versions
            .into_iter()
            .partition(|version| version.envelope.document == id);
        unreadable.extend(strays.into_iter().map(|stray| Unreadable {
            why: ReadError::Malformed(format!(
                "a version of document {}, not of {id}",
                stray.envelope.document
            )),
            id: stray.id,
        }));
        versions.sort_by(|a, b| a.id.cmp(&b.id));
        versions.dedup_by(|a, b| a.id == b.id);
        unreadable.sort_by(|a, b| a.id.cmp(&b.id));
        let behind = behind(&versions);
        let heads = (0..versions.len())
            .filter(|at| !behind.contains(&versions[*at].id))
            .collect();
        Document {
            id,
            versions,
            heads,
            unreadable,
        }
    }

    /// A version already held changes nothing.
    #[must_use]
    pub fn with_version(&self, version: Version) -> Document {
        let mut versions = self.versions.clone();
        versions.push(version);
        Document::new(self.id.clone(), versions, self.unreadable.clone())
    }

    #[must_use]
    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    /// The held versions no held version lists as an ancestor, by id.
    #[must_use]
    pub fn heads(&self) -> Vec<&Version> {
        self.heads.iter().map(|at| &self.versions[*at]).collect()
    }

    #[must_use]
    pub fn state(&self) -> State<'_> {
        let heads = self.heads();
        match heads[..] {
            [] => State::Absent,
            [only] => State::Live(only),
            _ => State::Forked(heads),
        }
    }

    #[must_use]
    pub fn get(&self, id: &VersionId) -> Option<&Version> {
        self.versions.iter().find(|version| version.id == *id)
    }

    /// Every held version, the one with the most behind it first.
    #[must_use]
    pub fn history(&self) -> Vec<&Version> {
        let mut history: Vec<&Version> = self.versions.iter().collect();
        history.sort_by(|a, b| {
            b.envelope
                .ancestors
                .len()
                .cmp(&a.envelope.ancestors.len())
                .then_with(|| a.id.cmp(&b.id))
        });
        history
    }

    /// The ancestors a held version lists that are not held.
    #[must_use]
    pub fn missing(&self) -> Vec<&VersionId> {
        behind(&self.versions)
            .into_iter()
            .filter(|id| self.get(id).is_none())
            .collect()
    }

    #[must_use]
    pub fn unreadable(&self) -> &[Unreadable] {
        &self.unreadable
    }

    #[must_use]
    pub fn kind(&self) -> Option<&Kind> {
        self.versions.first().map(|version| &version.envelope.kind)
    }

    /// Whether a head's fields carry `key` as this text, or as a list
    /// with this text in it.
    #[must_use]
    pub fn holds(&self, key: &str, value: &str) -> bool {
        self.heads().iter().any(|head| match head.fields.get(key) {
            Some(Value::String(text)) => text == value,
            Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(value)),
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{after, first, lantern};
    use crate::domain::version::Envelope;

    const RELAY: &str = "name = \"relay\"\ntopics = [\"lantern\"]";

    fn root() -> Version {
        first(&lantern(), "fact", RELAY, "root\n")
    }

    fn document(versions: &[&Version]) -> Document {
        Document::new(
            lantern(),
            versions.iter().copied().cloned().collect(),
            vec![],
        )
    }

    #[test]
    fn the_state_follows_the_heads() {
        let root = root();
        let left = after(&[&root], RELAY, "left\n");
        let right = after(&[&root], RELAY, "right\n");
        let merged = after(&[&left, &right], RELAY, "merged\n");
        assert_eq!(document(&[]).state(), State::Absent);
        assert_eq!(document(&[&root]).state(), State::Live(&root));
        assert_eq!(document(&[&root, &left]).state(), State::Live(&left));
        let forked = document(&[&root, &left, &right]);
        let mut heads = vec![&left, &right];
        heads.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(forked.state(), State::Forked(heads));
        assert_eq!(
            document(&[&root, &left, &right, &merged]).state(),
            State::Live(&merged)
        );
    }

    #[test]
    fn a_version_following_two_heads_lists_both_and_all_behind_them() {
        let root = root();
        let left = after(&[&root], RELAY, "left\n");
        let right = after(&[&root], RELAY, "right\n");
        let merged = after(&[&left, &right], RELAY, "merged\n");
        let mut parents = vec![left.id.clone(), right.id.clone()];
        parents.sort();
        let mut ancestors = vec![root.id.clone(), left.id.clone(), right.id.clone()];
        ancestors.sort();
        assert_eq!(merged.envelope.parents, parents);
        assert_eq!(merged.envelope.ancestors, ancestors);
        assert_eq!(merged.envelope.document, lantern());
        let stamp = root.envelope.written.clone();
        assert_eq!(Envelope::following(&[], stamp, lantern(), "save"), None);
    }

    #[test]
    fn a_document_with_one_more_version_is_the_one_built_from_them_all() {
        let root = root();
        let left = after(&[&root], RELAY, "left\n");
        let right = after(&[&root], RELAY, "right\n");
        let damaged = Unreadable {
            id: VersionId::of(b"damaged"),
            why: ReadError::Corrupt,
        };
        let held = Document::new(
            lantern(),
            vec![root.clone(), left.clone()],
            vec![damaged.clone()],
        );
        let forked = held.with_version(right.clone());
        assert_eq!(
            forked,
            Document::new(lantern(), vec![root, left.clone(), right], vec![damaged])
        );
        assert!(matches!(forked.state(), State::Forked(_)));
        assert_eq!(held.with_version(left), held);
    }

    #[test]
    fn a_chain_missing_its_middle_is_live_at_its_newest() {
        let root = root();
        let middle = after(&[&root], RELAY, "middle\n");
        let newest = after(&[&middle], RELAY, "newest\n");
        let held = document(&[&root, &newest]);
        assert_eq!(held.state(), State::Live(&newest));
        assert_eq!(held.missing(), vec![&middle.id]);
        assert_eq!(held.history(), vec![&newest, &root]);
        assert!(document(&[&root, &middle, &newest]).missing().is_empty());
    }

    #[test]
    fn a_document_holds_what_a_head_carries() {
        let root = root();
        let renamed = after(
            &[&root],
            "name = \"relay-pin\"\ntopics = [\"lantern\", \"atlas\"]",
            "renamed\n",
        );
        let held = document(&[&root, &renamed]);
        assert!(held.holds("name", "relay-pin"));
        assert!(held.holds("topics", "atlas"));
        assert!(!held.holds("name", "relay"), "only an older version has it");
        assert!(!held.holds("topics", "phone"));
        assert!(!held.holds("summary", "relay-pin"));
        assert_eq!(held.kind().map(Kind::as_str), Some("fact"));
        assert_eq!(held.get(&root.id), Some(&root));
        assert_eq!(document(&[]).kind(), None);
    }

    #[test]
    fn what_did_not_read_or_is_of_another_document_is_reported_beside_the_rest() {
        let root = root();
        let damaged = Unreadable {
            id: VersionId::of(b"damaged"),
            why: ReadError::Corrupt,
        };
        let atlas = DocumentId::from_bytes([0xa7; 16]);
        let stray = first(&atlas, "topic", "name = \"atlas\"", "\n");
        let held = Document::new(
            lantern(),
            vec![stray.clone(), root.clone()],
            vec![damaged.clone()],
        );
        assert_eq!(held.state(), State::Live(&root));
        assert_eq!(held.kind().map(Kind::as_str), Some("fact"));
        assert_eq!(held.unreadable().len(), 2);
        assert!(held.unreadable().contains(&damaged));
        let reported = held.unreadable().iter().find(|u| u.id == stray.id);
        match reported.map(|u| &u.why) {
            Some(ReadError::Malformed(why)) => assert!(why.contains(atlas.as_str()), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    fn every_subset_has_one_answer(versions: &[&Version], tip: Option<&Version>) {
        for mask in 1..(1usize << versions.len()) {
            let held: Vec<&Version> = (0..versions.len())
                .filter(|at| mask & (1 << at) != 0)
                .map(|at| versions[at])
                .collect();
            let forward = document(&held);
            let backward = document(&held.iter().rev().copied().collect::<Vec<_>>());
            assert_eq!(forward, backward, "subset {mask:b}");
            assert_ne!(forward.state(), State::Absent, "subset {mask:b}");
            if let Some(tip) = tip.filter(|tip| held.contains(tip)) {
                assert_eq!(forward.state(), State::Live(tip), "subset {mask:b}");
            }
        }
    }

    #[test]
    fn every_subset_of_a_documents_files_has_a_defined_state() {
        let root = root();
        let second = after(&[&root], RELAY, "second\n");
        let third = after(&[&second], RELAY, "third\n");
        let fourth = after(&[&third], RELAY, "fourth\n");
        every_subset_has_one_answer(&[&root, &second, &third, &fourth], Some(&fourth));

        let left = after(&[&root], RELAY, "left\n");
        let right = after(&[&root], RELAY, "right\n");
        every_subset_has_one_answer(&[&root, &left, &right], None);
        assert!(matches!(
            document(&[&left, &right]).state(),
            State::Forked(_)
        ));

        let merged = after(&[&left, &right], RELAY, "merged\n");
        every_subset_has_one_answer(&[&root, &left, &right, &merged], Some(&merged));
    }
}
