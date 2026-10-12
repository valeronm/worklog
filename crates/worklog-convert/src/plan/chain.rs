use std::collections::{BTreeMap, BTreeSet};

use worklog_next::domain::id::DocumentId;

use super::Questions;
use super::id::document_id;
use super::walk::{Role, Step, head, heads, steps};
use crate::key::DocumentKey;
use crate::old::{Document, Kind, Store, Version};
use crate::question::{Blocked, Unjoined};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Join {
    from: usize,
    stone: usize,
    to: usize,
    moved: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chain<'a> {
    pub(crate) kind: Kind,
    pub(crate) origin: usize,
    pub(crate) later: Vec<usize>,
    pub(crate) renames: Vec<(String, String)>,
    pub(crate) id: DocumentId,
    pub(crate) steps: Vec<Step<'a>>,
    standing: Standing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Standing {
    Settled { key: DocumentKey, ended: bool },
    Unsettled,
}

impl Chain<'_> {
    pub(crate) fn documents(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(self.origin).chain(self.later.iter().copied())
    }
}

/// A chain the store holds as one document, with what that document is. A chain that goes
/// on under both slugs of a rename is none: it is blocked where the chains are built, and
/// until the store settles it nothing is known of its key or of whether it is ended.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settled<'c, 'a> {
    /// Where the chain stands among the chains.
    pub(crate) index: usize,
    pub(crate) chain: &'c Chain<'a>,
    /// The document as a decision file names it: by the slug its head is stored under,
    /// and by the slug of its last rename where it ends with no one version.
    pub(crate) key: &'c DocumentKey,
    /// Whether the head is a tombstone that removed the document.
    pub(crate) ended: bool,
}

impl<'c, 'a> Settled<'c, 'a> {
    pub(crate) fn heads(&self) -> impl Iterator<Item = &'c Step<'a>> + use<'c, 'a> {
        heads(&self.chain.steps)
    }

    pub(crate) fn head(&self) -> Option<&'c Step<'a>> {
        head(&self.chain.steps)
    }

    /// Which of the chain's documents the key names.
    pub(crate) fn place(&self) -> usize {
        self.head()
            .map_or(self.chain.later.len(), |head| head.place)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chains<'a> {
    pub(crate) chains: Vec<Chain<'a>>,
    pub(crate) of: Vec<usize>,
}

pub(crate) fn key_of(document: &Document) -> DocumentKey {
    DocumentKey::new(document.kind, &document.slug)
}

fn holds(document: &Document, id: &str) -> bool {
    document.versions.iter().any(|version| version.id == id)
}

fn is_rename_stone(version: &Version) -> bool {
    version.tombstone && version.superseded_by.is_some()
}

struct Joining<'a, 'q> {
    store: &'a Store,
    questions: &'q Questions,
    index: BTreeMap<(Kind, &'a str), usize>,
}

impl<'a> Joining<'a, '_> {
    fn unjoined(&self, document: usize, other: usize, case: Unjoined) {
        let documents = &self.store.documents;
        self.questions.ask(Blocked::RenameUnjoined {
            document: key_of(&documents[document]),
            others: vec![documents[other].slug.clone()],
            case,
        });
    }

    fn stone_of(&self, to: usize, moved: usize) -> Option<Join> {
        let document = &self.store.documents[to];
        let version = &document.versions[moved];
        let old = version.renamed_from.as_deref()?;
        let from = self.index.get(&(document.kind, old)).copied();
        let stones = from.map(|from| &self.store.documents[from].versions);
        let stone = stones.into_iter().flatten().position(|stone| {
            is_rename_stone(stone)
                && stone.superseded_by.as_deref() == Some(document.slug.as_str())
                && version.parents.contains(&stone.id)
        });
        if let (Some(from), Some(stone)) = (from, stone) {
            return Some(Join {
                from,
                stone,
                to,
                moved,
            });
        }
        let lacks_a_parent = (version.parents.iter())
            .any(|parent| !from.is_some_and(|from| holds(&self.store.documents[from], parent)));
        if !lacks_a_parent {
            self.questions.ask(Blocked::RenameUnjoined {
                document: key_of(document),
                others: vec![old.to_owned()],
                case: Unjoined::NoTombstone,
            });
        }
        None
    }

    fn found(&self) -> Vec<Join> {
        let mut joins = Vec::new();
        for (to, document) in self.store.documents.iter().enumerate() {
            for moved in 0..document.versions.len() {
                joins.extend(self.stone_of(to, moved));
            }
        }
        joins
    }

    fn cut_short(&self, joins: &[Join]) {
        let joined: BTreeSet<(usize, usize)> =
            joins.iter().map(|join| (join.from, join.stone)).collect();
        for (from, document) in self.store.documents.iter().enumerate() {
            for (stone, version) in document.versions.iter().enumerate() {
                let Some(target) = version.superseded_by.as_ref().filter(|_| version.tombstone)
                else {
                    continue;
                };
                if !joined.contains(&(from, stone)) {
                    self.questions.ask(Blocked::RenameCutShort {
                        document: key_of(document),
                        target: target.clone(),
                    });
                }
            }
        }
    }

    fn single(&self, joins: &[Join]) -> Vec<Join> {
        let count = |of: fn(&Join) -> usize| {
            let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
            for join in joins {
                *counts.entry(of(join)).or_default() += 1;
            }
            counts
        };
        let (into, out_of) = (count(|join| join.to), count(|join| join.from));
        let mut kept = Vec::new();
        for join in joins {
            if into[&join.to] > 1 {
                self.unjoined(join.to, join.from, Unjoined::Met);
            } else if out_of[&join.from] > 1 {
                self.unjoined(join.from, join.to, Unjoined::Parted);
            } else {
                kept.push(*join);
            }
        }
        kept
    }

    fn chain(&self, origin: usize, next: &BTreeMap<usize, Join>) -> Chain<'a> {
        let documents = &self.store.documents;
        let (mut later, mut renames) = (Vec::new(), Vec::new());
        let mut at = origin;
        while let Some(join) = next.get(&at) {
            let stone = documents[join.from].versions[join.stone].id.clone();
            let moved = documents[join.to].versions[join.moved].id.clone();
            renames.push((stone, moved));
            later.push(join.to);
            at = join.to;
        }
        let members: Vec<usize> = std::iter::once(origin)
            .chain(later.iter().copied())
            .collect();
        let steps = steps(self.store, &members, &renames);
        let kind = documents[origin].kind;
        let standing = if self.parted(&members, &steps) {
            Standing::Unsettled
        } else {
            let head = head(&steps);
            let under = head.map_or(&documents[at], |head| head.source.document);
            Standing::Settled {
                key: DocumentKey::new(kind, &under.slug),
                ended: head.is_some_and(|head| head.role == Role::Removed),
            }
        };
        Chain {
            kind,
            origin,
            later,
            renames,
            id: document_id(kind, &documents[origin].slug),
            steps,
            standing,
        }
    }

    // A version that follows a rename's tombstone and is not the version the rename moved
    // joins the rename with what was written under the slug it left. The store then holds
    // a document under each slug, which are one document only where that version is the
    // head and the one of its kind.
    fn parted(&self, members: &[usize], steps: &[Step<'_>]) -> bool {
        let mut joins = Vec::new();
        for renamed in steps {
            let Some(stone) = renamed.source.stone else {
                continue;
            };
            let follows = |step: &&Step<'_>| {
                let named = &step.source.version.parents;
                step.key() != renamed.key() && named.contains(&stone.version.id)
            };
            let joined = steps.iter().filter(follows);
            joins.extend(joined.map(|step| (step, renamed, stone)));
        }
        let alone = match joins.as_slice() {
            [(only, _, _)] => head(steps).is_some_and(|head| head.key() == only.key()),
            _ => false,
        };
        if joins.is_empty() || alone {
            return false;
        }
        // The store holds each slug of the chain as a document with heads of its own.
        let under = |held: fn(&Version) -> bool| -> Vec<String> {
            let documents = members.iter().map(|member| &self.store.documents[*member]);
            let with = documents.filter(|document| document.heads().into_iter().any(held));
            with.map(|document| document.slug.clone()).collect()
        };
        let removed = under(|head| head.tombstone && head.superseded_by.is_none());
        let live = under(|head| !head.tombstone);
        for (step, renamed, stone) in joins {
            let mut both = [stone.document.slug.clone(), renamed.slug().to_owned()];
            both.sort();
            let of_the_rename = |slugs: &[String]| {
                let mut slugs = slugs.to_vec();
                slugs.sort();
                slugs == both
            };
            let after = match (removed.as_slice(), live.as_slice()) {
                ([], live) if of_the_rename(live) => "goes on under both slugs".to_owned(),
                ([], live) => format!("goes on under {}", live.join(", ")),
                (removed, []) if of_the_rename(removed) => {
                    "was removed under both slugs after it".to_owned()
                }
                (removed, []) => format!("was removed under {} after it", removed.join(", ")),
                (removed, live) => format!(
                    "was removed under {} after it and goes on under {}",
                    removed.join(", "),
                    live.join(", ")
                ),
            };
            self.questions.ask(Blocked::Value {
                document: key_of(step.source.document),
                version: step.source.version.id.clone(),
                field: "parents".to_owned(),
                value: stone.version.id.clone(),
                why: format!("is the tombstone of a rename, and the document {after}"),
            });
        }
        true
    }

    fn chains(&self, joins: &[Join]) -> Chains<'a> {
        let next: BTreeMap<usize, Join> = joins.iter().map(|join| (join.from, *join)).collect();
        let continued: BTreeSet<usize> = joins.iter().map(|join| join.to).collect();
        let count = self.store.documents.len();
        let mut chains = Chains {
            chains: Vec::new(),
            of: vec![usize::MAX; count],
        };
        for origin in (0..count).filter(|origin| !continued.contains(origin)) {
            chains.push(self.chain(origin, &next));
        }
        for circled in 0..count {
            if chains.of[circled] != usize::MAX {
                continue;
            }
            if let Some(join) = joins.iter().find(|join| join.to == circled) {
                self.unjoined(join.to, join.from, Unjoined::Circle);
            }
            chains.push(self.chain(circled, &BTreeMap::new()));
        }
        chains.sorted()
    }
}

impl<'a> Chains<'a> {
    pub(crate) fn settled(&self, index: usize) -> Option<Settled<'_, 'a>> {
        let chain = &self.chains[index];
        match &chain.standing {
            Standing::Settled { key, ended } => Some(Settled {
                index,
                chain,
                key,
                ended: *ended,
            }),
            Standing::Unsettled => None,
        }
    }

    pub(crate) fn each_settled(&self) -> impl Iterator<Item = Settled<'_, 'a>> {
        (0..self.chains.len()).filter_map(|index| self.settled(index))
    }

    fn push(&mut self, chain: Chain<'a>) {
        for document in chain.documents() {
            self.of[document] = self.chains.len();
        }
        self.chains.push(chain);
    }

    fn sorted(mut self) -> Chains<'a> {
        self.chains.sort_by_key(|chain| chain.origin);
        for (index, chain) in self.chains.iter().enumerate() {
            for document in chain.documents() {
                self.of[document] = index;
            }
        }
        self
    }

    pub(crate) fn build(store: &'a Store, questions: &Questions) -> Chains<'a> {
        let index = (store.documents.iter().enumerate())
            .map(|(at, document)| ((document.kind, document.slug.as_str()), at))
            .collect();
        let joining = Joining {
            store,
            questions,
            index,
        };
        let found = joining.found();
        joining.cut_short(&found);
        let kept = joining.single(&found);
        joining.chains(&kept)
    }
}
