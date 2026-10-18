//! The versions of one document that are held, and what they say about
//! which of them is current.

use std::collections::BTreeSet;

use toml::Value;

use super::id::{DocumentId, VersionId};
use super::version::{Header, Kind, ReadError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreadable {
    pub version: VersionId,
    pub why: ReadError,
}

/// The versions of one document, by their headers.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    id: DocumentId,
    headers: Vec<Header>,
    heads: Vec<usize>,
    unreadable: Vec<Unreadable>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State<'a> {
    Absent,
    Live(&'a Header),
    /// Versions none of which is behind another.
    Forked(Vec<&'a Header>),
}

fn behind(headers: &[Header]) -> BTreeSet<&VersionId> {
    headers
        .iter()
        .flat_map(|header| &header.envelope.ancestors)
        .collect()
}

impl Document {
    #[must_use]
    pub fn new(id: DocumentId, headers: Vec<Header>, mut unreadable: Vec<Unreadable>) -> Document {
        let (mut headers, strays): (Vec<Header>, Vec<Header>) = headers
            .into_iter()
            .partition(|header| header.envelope.document == id);
        unreadable.extend(strays.into_iter().map(|stray| Unreadable {
            why: ReadError::Malformed(format!(
                "a version of document {}, not of {id}",
                stray.envelope.document
            )),
            version: stray.id,
        }));
        headers.sort_by(|a, b| a.id.cmp(&b.id));
        headers.dedup_by(|a, b| a.id == b.id);
        unreadable.sort_by(|a, b| a.version.cmp(&b.version));
        let behind = behind(&headers);
        let heads = (0..headers.len())
            .filter(|at| !behind.contains(&headers[*at].id))
            .collect();
        Document {
            id,
            headers,
            heads,
            unreadable,
        }
    }

    /// The document `new` builds from `headers`, taking `heads` as `new` computed them; a test
    /// build alone checks that.
    #[must_use]
    pub fn as_recorded(id: DocumentId, headers: Vec<Header>, heads: Vec<usize>) -> Document {
        let document = Document {
            id,
            headers,
            heads,
            unreadable: Vec::new(),
        };
        debug_assert_eq!(
            document,
            Document::new(document.id.clone(), document.headers.clone(), Vec::new())
        );
        document
    }

    /// A version already held changes nothing.
    #[must_use]
    pub fn with_version(&self, header: Header) -> Document {
        let mut headers = self.headers.clone();
        headers.push(header);
        Document::new(self.id.clone(), headers, self.unreadable.clone())
    }

    #[must_use]
    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    /// The held versions no held version lists as an ancestor, by id.
    #[must_use]
    pub fn heads(&self) -> Vec<&Header> {
        self.heads.iter().map(|at| &self.headers[*at]).collect()
    }

    /// The held versions by id, which `heads_at` indexes.
    #[must_use]
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    #[must_use]
    pub fn heads_at(&self) -> &[usize] {
        &self.heads
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
    pub fn get(&self, id: &VersionId) -> Option<&Header> {
        self.headers.iter().find(|header| header.id == *id)
    }

    /// Every held version, the one with the most behind it first.
    #[must_use]
    pub fn history(&self) -> Vec<&Header> {
        let mut history: Vec<&Header> = self.headers.iter().collect();
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
        behind(&self.headers)
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
        self.headers.first().map(|header| &header.envelope.kind)
    }

    /// Whether a head's fields carry `key` as one of `values`, or as a list with one of
    /// them in it.
    #[must_use]
    pub fn holds_any(&self, key: &str, values: &BTreeSet<&str>) -> bool {
        let asked = |item: &Value| item.as_str().is_some_and(|text| values.contains(text));
        self.heads
            .iter()
            .any(|at| match self.headers[*at].fields.get(key) {
                Some(Value::Array(items)) => items.iter().any(asked),
                Some(value) => asked(value),
                None => false,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{after, first, lantern};
    use crate::domain::version::{Envelope, Header, Version};

    const RELAY: &str = "name = \"relay\"\ntopics = [\"lantern\"]";

    fn root() -> Version {
        first(&lantern(), "fact", RELAY, "root\n")
    }

    fn document(versions: &[&Version]) -> Document {
        Document::new(
            lantern(),
            versions
                .iter()
                .map(|version| version.header().clone())
                .collect(),
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
        assert_eq!(document(&[&root]).state(), State::Live(root.header()));
        assert_eq!(
            document(&[&root, &left]).state(),
            State::Live(left.header())
        );
        let forked = document(&[&root, &left, &right]);
        let mut heads = vec![left.header(), right.header()];
        heads.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(forked.state(), State::Forked(heads));
        assert_eq!(
            document(&[&root, &left, &right, &merged]).state(),
            State::Live(merged.header())
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
            version: VersionId::of(b"damaged"),
            why: ReadError::Corrupt,
        };
        let (root, left, right) = (root.into_header(), left.into_header(), right.into_header());
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
        assert_eq!(held.state(), State::Live(newest.header()));
        assert_eq!(held.missing(), vec![&middle.id]);
        assert_eq!(held.history(), vec![newest.header(), root.header()]);
        assert!(document(&[&root, &middle, &newest]).missing().is_empty());
    }

    #[test]
    fn a_document_is_of_its_versions_kind_and_gives_one_by_its_id() {
        let root = root();
        let renamed = after(
            &[&root],
            "name = \"relay-pin\"\ntopics = [\"lantern\", \"atlas\"]",
            "renamed\n",
        );
        let held = document(&[&root, &renamed]);
        assert_eq!(held.kind().map(Kind::as_str), Some("fact"));
        assert_eq!(held.get(&root.id), Some(root.header()));
        assert_eq!(document(&[]).kind(), None);
    }

    #[test]
    fn a_document_holds_any_of_the_values_asked_when_a_head_carries_one() {
        let root = root();
        let left = after(
            &[&root],
            "name = \"relay-pin\"\ntopics = [\"atlas\"]",
            "l\n",
        );
        let right = after(&[&root], "name = \"coil\"\ntopics = [\"phone\", 4]", "r\n");
        let forked = document(&[&root, &left, &right]);
        let any =
            |key: &str, values: &[&str]| forked.holds_any(key, &values.iter().copied().collect());
        assert!(any("name", &["coil"]));
        assert!(any("name", &["desk", "relay-pin"]));
        assert!(any("topics", &["desk", "phone"]));
        assert!(any("topics", &["atlas", "phone"]));
        assert!(
            !any("name", &["relay", "atlas"]),
            "only an older version has it"
        );
        assert!(!any("topics", &["lantern", "4"]));
        assert!(!any("summary", &["coil"]));
        assert!(!any("name", &[]));
        assert!(!document(&[]).holds_any("name", &BTreeSet::from(["relay"])));
    }

    #[test]
    fn what_did_not_read_or_is_of_another_document_is_reported_beside_the_rest() {
        let root = root();
        let damaged = Unreadable {
            version: VersionId::of(b"damaged"),
            why: ReadError::Corrupt,
        };
        let atlas = DocumentId::from_bytes([0xa7; 16]);
        let stray = first(&atlas, "topic", "name = \"atlas\"", "\n");
        let held = Document::new(
            lantern(),
            vec![stray.header().clone(), root.header().clone()],
            vec![damaged.clone()],
        );
        assert_eq!(held.state(), State::Live(root.header()));
        assert_eq!(held.kind().map(Kind::as_str), Some("fact"));
        assert_eq!(held.unreadable().len(), 2);
        assert!(held.unreadable().contains(&damaged));
        let reported = held.unreadable().iter().find(|u| u.version == stray.id);
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
                assert_eq!(
                    forward.state(),
                    State::Live(tip.header()),
                    "subset {mask:b}"
                );
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

    #[test]
    fn a_document_as_recorded_is_the_one_built_from_its_headers() {
        let root = root();
        let left = after(&[&root], RELAY, "left\n");
        let right = after(&[&root], "name = \"coil\"\ntopics = [\"phone\"]", "right\n");
        let built = document(&[&root, &left, &right]);
        let recorded = Document::as_recorded(
            lantern(),
            built.headers().to_vec(),
            built.heads_at().to_vec(),
        );
        assert_eq!(recorded, built);
        let heads: Vec<&Header> = built
            .heads_at()
            .iter()
            .map(|at| &built.headers()[*at])
            .collect();
        assert_eq!(heads, built.heads());
    }

    #[test]
    #[should_panic(expected = "assertion")]
    fn a_document_recorded_with_heads_not_its_own_fails_a_test_build() {
        let root = root();
        let left = after(&[&root], RELAY, "left\n");
        let built = document(&[&root, &left]);
        let _ = Document::as_recorded(lantern(), built.headers().to_vec(), vec![0, 1]);
    }
}
