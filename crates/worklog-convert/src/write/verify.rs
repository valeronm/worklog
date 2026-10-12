use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;

use worklog::app::Deps;
use worklog::app::check::{Finding, check};
use worklog::app::show::history;
use worklog::domain::id::{DocumentId, VersionId};
use worklog::wiring::wire;

use super::compose::{Composed, file_of};
use super::rules;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    /// The staged store does not open or read.
    Store(String),
    /// A planned file or directory the staging directory lacks.
    Absent {
        path: String,
    },
    /// A file or directory of the staging directory no planned version accounts for.
    Stray {
        path: String,
    },
    /// A planned document the new store does not find.
    Missing {
        document: DocumentId,
        why: String,
    },
    /// A version file the new store does not read.
    Unreadable {
        document: DocumentId,
        version: VersionId,
        why: String,
    },
    /// The versions the new store reads of a document are not the planned ones.
    Versions {
        document: DocumentId,
        held: Vec<VersionId>,
        planned: Vec<VersionId>,
    },
    /// The heads of a document are not the one head of the plan.
    Heads {
        document: DocumentId,
        heads: Vec<VersionId>,
        planned: VersionId,
    },
    /// A document whose plan ends with no one version.
    Unended {
        document: DocumentId,
        heads: Vec<VersionId>,
    },
    /// A rule the new store's writes keep and its check does not look at, broken by a
    /// written version.
    Version {
        document: DocumentId,
        version: VersionId,
        what: String,
    },
    /// A problem as the new store's check reports it.
    Problem(Finding),
    Forks(usize),
}

fn listed(ids: &[VersionId]) -> String {
    let ids: Vec<&str> = ids.iter().map(VersionId::as_str).collect();
    ids.join(", ")
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Store(why) => f.write_str(why),
            Fault::Absent { path } => write!(f, "{path}: was not written"),
            Fault::Stray { path } => write!(f, "{path}: is no part of the plan"),
            Fault::Missing { document, why } => write!(f, "{document}: is not found: {why}"),
            Fault::Unreadable {
                document,
                version,
                why,
            } => write!(f, "{document}: {version} does not read: {why}"),
            Fault::Versions {
                document,
                held,
                planned,
            } => write!(
                f,
                "{document}: reads as the versions {}, and the plan has {}",
                listed(held),
                listed(planned)
            ),
            Fault::Heads {
                document,
                heads,
                planned,
            } => write!(
                f,
                "{document}: has {} heads, {}, and the plan ends at {planned} alone",
                heads.len(),
                listed(heads)
            ),
            Fault::Unended { document, heads } => write!(
                f,
                "{document}: has {} heads, {}, and the plan ends at no one version",
                heads.len(),
                listed(heads)
            ),
            Fault::Version {
                document,
                version,
                what,
            } => write!(f, "{document}: {version}: {what}"),
            Fault::Problem(finding) => {
                let document = finding.document.as_ref();
                let document = document.map(|id| format!(" ({id})")).unwrap_or_default();
                write!(f, "{}{document}: {}", finding.label, finding.what)
            }
            Fault::Forks(1) => f.write_str("1 document is forked"),
            Fault::Forks(count) => write!(f, "{count} documents are forked"),
        }
    }
}

// No config is at a path under a directory this run made.
const NO_HOST: &str = ".no-host";
const HOME_OF_THE_TOOL: &str = "WORKLOG_HOME";

fn walk(root: &Path, under: &str, found: &mut BTreeSet<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root.join(under))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if under.is_empty() {
            name
        } else {
            format!("{under}/{name}")
        };
        if entry.file_type()?.is_dir() {
            walk(root, &path, found)?;
        }
        found.insert(path);
    }
    Ok(())
}

fn tree_faults(composed: &[Composed], staging: &Path) -> Vec<Fault> {
    let mut planned = BTreeSet::new();
    for version in composed.iter().flat_map(|document| &document.versions) {
        let file = file_of(version);
        let within = Path::new(&file).ancestors().filter_map(Path::to_str);
        planned.extend(within.filter(|path| !path.is_empty()).map(str::to_owned));
    }
    let mut held = BTreeSet::new();
    if let Err(error) = walk(staging, "", &mut held) {
        return vec![Fault::Store(format!("{}: {error}", staging.display()))];
    }
    let absent = planned.difference(&held).cloned();
    let stray = held.difference(&planned).cloned();
    absent
        .map(|path| Fault::Absent { path })
        .chain(stray.map(|path| Fault::Stray { path }))
        .collect()
}

fn document_faults(deps: &Deps, document: &Composed) -> Vec<Fault> {
    let id = &document.id;
    let read = match history(deps, id.as_str()) {
        Ok(read) => read,
        Err(error) => {
            return vec![Fault::Missing {
                document: id.clone(),
                why: error.to_string(),
            }];
        }
    };
    let mut faults: Vec<Fault> = (read.unreadable.iter())
        .map(|unread| Fault::Unreadable {
            document: id.clone(),
            version: unread.version.clone(),
            why: unread.why.text().to_owned(),
        })
        .collect();
    let mut held: Vec<VersionId> = read
        .versions
        .iter()
        .map(|row| row.version.clone())
        .collect();
    let mut planned: Vec<VersionId> = (document.versions.iter())
        .map(|version| version.id.clone())
        .collect();
    held.sort();
    planned.sort();
    if read.document != *id || held != planned {
        faults.push(Fault::Versions {
            document: id.clone(),
            held,
            planned,
        });
    }
    let mut heads: Vec<VersionId> = (read.versions.iter())
        .filter(|row| row.head)
        .map(|row| row.version.clone())
        .collect();
    heads.sort();
    match document.head().map(|head| &head.id) {
        Some(planned) if heads == std::slice::from_ref(planned) => {}
        Some(planned) => faults.push(Fault::Heads {
            document: id.clone(),
            heads,
            planned: planned.clone(),
        }),
        None => faults.push(Fault::Unended {
            document: id.clone(),
            heads,
        }),
    }
    faults
}

// Reads the staged store and writes nothing.
pub(super) fn verify(composed: &[Composed], staging: &Path) -> Result<Vec<Finding>, Vec<Fault>> {
    let mut faults = tree_faults(composed, staging);
    faults.extend(rules::faults(composed, staging));
    let no_host = staging.join(NO_HOST).into_os_string();
    let variable =
        |name: &str| -> Option<OsString> { (name == HOME_OF_THE_TOOL).then(|| no_host.clone()) };
    let adapters = match wire(&variable, Some(staging.to_path_buf())) {
        Ok(Some(adapters)) => adapters,
        Ok(None) => {
            faults.push(Fault::Store("the staged store was not opened".to_owned()));
            return Err(faults);
        }
        Err(error) => {
            faults.push(Fault::Store(error.to_string()));
            return Err(faults);
        }
    };
    if let Err(error) = adapters.settle() {
        faults.push(Fault::Store(error.to_string()));
        return Err(faults);
    }
    let deps = adapters.deps();
    for document in composed {
        faults.extend(document_faults(&deps, document));
    }
    let notices = match check(&deps) {
        Ok(found) => {
            faults.extend(found.problems.into_iter().map(Fault::Problem));
            if found.fork_count > 0 {
                faults.push(Fault::Forks(found.fork_count));
            }
            found.notices
        }
        Err(error) => {
            faults.push(Fault::Store(error.to_string()));
            Vec::new()
        }
    };
    if faults.is_empty() {
        Ok(notices)
    } else {
        Err(faults)
    }
}
