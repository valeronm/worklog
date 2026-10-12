use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use worklog::domain::schema::KindOf;

use crate::old;
use crate::plan::{Complete, Origin};
use crate::text::counted as counted_noun;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Kinded {
    pub(crate) kind: KindOf,
    pub(crate) documents: usize,
    pub(crate) versions: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) old_documents: usize,
    pub(crate) old_versions: usize,
    /// Every kind of the new schema, in its order.
    pub(crate) kinds: Vec<Kinded>,
    /// The new versions made from a version of the old store.
    pub(crate) converted: usize,
    /// The converted versions that stand for two old ones, the halves of a rename.
    pub(crate) renames: usize,
    /// The versions made beside a version of another document.
    pub(crate) generated: usize,
    /// The documents whose first version is a generated one.
    pub(crate) generated_documents: usize,
    /// The versions the decision file alone made.
    pub(crate) authored: usize,
}

impl Counts {
    /// The block a run prints, of what was written or of what a run with a target writes.
    pub(crate) fn shown(&self, written: bool) -> String {
        let both = |documents, versions| {
            format!(
                "{}, {}",
                counted_noun(documents, "document"),
                counted_noun(versions, "version")
            )
        };
        let (wrote, made) = if written {
            ("written", "versions written, by what each was made from")
        } else {
            ("to write", "versions to write, by what each is made from")
        };
        let documents = self.kinds.iter().map(|kind| kind.documents).sum();
        let versions = self.kinds.iter().map(|kind| kind.versions).sum();
        let mut text = format!(
            "read: {}\n{wrote}: {}\n",
            both(self.old_documents, self.old_versions),
            both(documents, versions)
        );
        for kind in &self.kinds {
            let word = kind.kind.word();
            let _ = writeln!(text, "  {word}: {}", both(kind.documents, kind.versions));
        }
        let _ = write!(
            text,
            "{made}:\n  \
             a version of the store: {}, of which {} from the two halves of a rename\n  \
             a version of another document: {}, of which {} begin a document\n  \
             the decision file alone: {}",
            self.converted, self.renames, self.generated, self.generated_documents, self.authored
        );
        text
    }
}

/// The versions of the old store and the converted versions of a plan do not match one to
/// one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Miscount {
    pub(crate) counts: Counts,
    /// The old versions no planned version was made from, as `kind/slug/id`.
    pub(crate) lost: Vec<String>,
    /// The old versions several planned versions were made from, and the versions one was
    /// made from that the old store lacks.
    pub(crate) surplus: Vec<String>,
}

impl fmt::Display for Miscount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let counts = &self.counts;
        write!(
            f,
            "{} versions were read, and {} versions were made from them, {} of those from the \
             two halves of a rename",
            counts.old_versions, counts.converted, counts.renames
        )?;
        for lost in &self.lost {
            write!(f, "\n  {lost}: became no version")?;
        }
        for surplus in &self.surplus {
            write!(
                f,
                "\n  {surplus}: is not one version of the store made into one"
            )?;
        }
        Ok(())
    }
}

fn named(document: &old::Document, version: &old::Version) -> String {
    format!("{}/{}/{}", document.kind.word(), document.slug, version.id)
}

pub(crate) fn counted(old: &old::Store, plan: &Complete) -> Result<Counts, Box<Miscount>> {
    let mut held_and_made: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut counts = Counts {
        old_documents: old.documents.len(),
        old_versions: 0,
        kinds: (KindOf::ALL.iter())
            .map(|kind| Kinded {
                kind: *kind,
                documents: 0,
                versions: 0,
            })
            .collect(),
        converted: 0,
        renames: 0,
        generated: 0,
        generated_documents: 0,
        authored: 0,
    };
    for document in &old.documents {
        counts.old_versions += document.versions.len();
        for version in &document.versions {
            held_and_made.entry(named(document, version)).or_default().0 += 1;
        }
    }
    for document in &plan.planned().documents {
        for kinded in &mut counts.kinds {
            if kinded.kind == document.kind {
                kinded.documents += 1;
                kinded.versions += document.versions.len();
            }
        }
        let first = document.versions.first().map(|version| &version.origin);
        if matches!(first, Some(Origin::Generated { .. })) {
            counts.generated_documents += 1;
        }
        for version in &document.versions {
            match &version.origin {
                Origin::Converted(source) => {
                    counts.converted += 1;
                    held_and_made
                        .entry(named(source.document, source.version))
                        .or_default()
                        .1 += 1;
                    if let Some(stone) = source.stone {
                        counts.renames += 1;
                        held_and_made
                            .entry(named(stone.document, stone.version))
                            .or_default()
                            .1 += 1;
                    }
                }
                Origin::Generated { .. } => counts.generated += 1,
                Origin::Authored { .. } => counts.authored += 1,
            }
        }
    }
    let (mut lost, mut surplus) = (Vec::new(), Vec::new());
    for (name, used) in held_and_made {
        match used {
            (1, 1) => {}
            (_, 0) => lost.push(name),
            _ => surplus.push(name),
        }
    }
    let adds_up = counts.old_versions == counts.converted + counts.renames;
    if adds_up && lost.is_empty() && surplus.is_empty() {
        Ok(counts)
    } else {
        Err(Box::new(Miscount {
            counts,
            lost,
            surplus,
        }))
    }
}
