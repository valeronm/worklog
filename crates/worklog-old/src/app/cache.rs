//! A store that answers from what it has already read.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::domain::ports::{Store, StoreError};
use crate::domain::slug::{Kind, Slug};
use crate::domain::version::{Document, Version, VersionId};

use super::Deps;

struct Snapshot<'a> {
    inner: &'a dyn Store,
    documents: RefCell<BTreeMap<Slug, Document>>,
    slugs: RefCell<BTreeMap<Kind, Vec<Slug>>>,
    prefixes: RefCell<BTreeMap<String, Vec<(Slug, VersionId)>>>,
}

impl Deps<'_> {
    /// Runs `reads` over a store that answers each question once, which
    /// notices nothing another process writes until `reads` returns; a
    /// write through it starts the answers over.
    pub fn cached<T>(&self, reads: impl FnOnce(&Deps) -> T) -> T {
        let snapshot = Snapshot {
            inner: self.store,
            documents: RefCell::default(),
            slugs: RefCell::default(),
            prefixes: RefCell::default(),
        };
        reads(&Deps {
            store: &snapshot,
            home: self.home.clone(),
            ..*self
        })
    }
}

fn answered<K: Ord + Clone, V: Clone>(
    kept: &RefCell<BTreeMap<K, V>>,
    key: &K,
    ask: impl FnOnce() -> Result<V, StoreError>,
) -> Result<V, StoreError> {
    if let Some(answer) = kept.borrow().get(key) {
        return Ok(answer.clone());
    }
    let answer = ask()?;
    kept.borrow_mut().insert(key.clone(), answer.clone());
    Ok(answer)
}

impl Store for Snapshot<'_> {
    fn slugs(&self, kind: Kind) -> Result<Vec<Slug>, StoreError> {
        answered(&self.slugs, &kind, || self.inner.slugs(kind))
    }

    fn document(&self, slug: &Slug) -> Result<Document, StoreError> {
        answered(&self.documents, slug, || self.inner.document(slug))
    }

    fn by_id_prefix(&self, prefix: &str) -> Result<Vec<(Slug, VersionId)>, StoreError> {
        answered(&self.prefixes, &prefix.to_owned(), || {
            self.inner.by_id_prefix(prefix)
        })
    }

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        self.inner.put(version)?;
        // A write leaves every answer given before it stale.
        self.documents.borrow_mut().clear();
        self.slugs.borrow_mut().clear();
        self.prefixes.borrow_mut().clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::app::testing::World;
    use crate::app::{load, write};

    #[test]
    fn a_write_through_a_snapshot_shows_in_the_next_read() {
        let w = World::new("desk");
        w.deps().cached(|deps| {
            load::load(deps.store).unwrap();
            write::put_fact(
                deps,
                "lantern/board-is-rev-c",
                "The board is rev C",
                &[],
                false,
            )
            .unwrap();
            let facts = load::load(deps.store).unwrap().facts;
            assert_eq!(
                facts.iter().map(|f| f.slug.path()).collect::<Vec<_>>(),
                ["lantern/board-is-rev-c"]
            );
        });
    }
}
