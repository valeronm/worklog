use std::collections::BTreeSet;
use std::fmt;

use worklog_next::domain::id::DocumentId;
use worklog_next::domain::schema::KindOf;
use worklog_next::domain::version::{Envelope, Kind, Version};

use crate::plan::{Complete, Whole, Written, head};

/// A plan the planner should not have made: no answer in a decision file mends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Defect {
    Twice {
        document: DocumentId,
    },
    KeyTwice {
        document: DocumentId,
        version: String,
    },
    Refused {
        document: DocumentId,
        version: String,
        why: String,
    },
}

impl fmt::Display for Defect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Defect::Twice { document } => write!(f, "{document}: is planned twice"),
            Defect::KeyTwice { document, version } => {
                write!(f, "{document}: has two versions under the key {version}")
            }
            Defect::Refused {
                document,
                version,
                why,
            } => write!(f, "{document}: the version {version} is no version: {why}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Composed {
    pub(crate) id: DocumentId,
    pub(crate) kind: KindOf,
    pub(crate) versions: Vec<Version>,
}

impl Composed {
    pub(crate) fn head(&self) -> Option<&Version> {
        head(&self.versions)
    }
}

pub(crate) fn file_of(version: &Version) -> String {
    let document = &version.envelope.document;
    format!("{}/{document}/{}.md", document.bucket(), version.id)
}

fn version_of(
    document: &Whole,
    kind: &Kind,
    planned: &Written,
    made: &[Version],
) -> Result<Version, Defect> {
    let parents: Vec<&Version> = planned.parents().iter().map(|at| &made[*at]).collect();
    let (written, machine) = (planned.stamp.clone(), planned.machine.clone());
    let change = &planned.change;
    let envelope = Envelope::following(&parents, written.clone(), machine.clone(), change)
        .unwrap_or_else(|| {
            Envelope::first(document.id.clone(), kind.clone(), written, machine, change)
        });
    let (links, body) = (planned.links.clone(), planned.body.clone());
    Version::compose(envelope, planned.record.fields(), links, body).map_err(|error| {
        Defect::Refused {
            document: document.id.clone(),
            version: planned.key.clone(),
            why: error.to_string(),
        }
    })
}

fn document_of(document: &Whole) -> Result<Composed, Defect> {
    let id = &document.id;
    let kind = Kind::parse(document.kind.word()).map_err(|error| Defect::Refused {
        document: id.clone(),
        version: (document.versions().first()).map_or_else(String::new, |first| first.key.clone()),
        why: error.to_string(),
    })?;
    let mut keys = BTreeSet::new();
    let mut made = Vec::new();
    for planned in document.versions() {
        if !keys.insert(planned.key.as_str()) {
            return Err(Defect::KeyTwice {
                document: id.clone(),
                version: planned.key.clone(),
            });
        }
        made.push(version_of(document, &kind, planned, &made)?);
    }
    Ok(Composed {
        id: id.clone(),
        kind: document.kind,
        versions: made,
    })
}

pub(crate) fn compose(plan: &Complete) -> Result<Vec<Composed>, Defect> {
    let mut seen = BTreeSet::new();
    let mut composed = Vec::new();
    for document in plan.documents() {
        if !seen.insert(&document.id) {
            return Err(Defect::Twice {
                document: document.id.clone(),
            });
        }
        composed.push(document_of(document)?);
    }
    Ok(composed)
}
