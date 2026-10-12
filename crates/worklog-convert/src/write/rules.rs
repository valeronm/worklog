use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use worklog_next::domain::id::{DocumentId, VersionId};
use worklog_next::domain::schema::{KindOf, Record};
use worklog_next::domain::version::Version;

use super::Fault;
use super::compose::{Composed, file_of};
use crate::plan::referenced;

struct Read<'c> {
    document: &'c Composed,
    version: &'c Version,
    record: Option<Record>,
}

impl Read<'_> {
    fn fault(&self, what: String) -> Fault {
        Fault::Version {
            document: self.document.id.clone(),
            version: self.version.id.clone(),
            what,
        }
    }
}

fn read<'c>(
    document: &'c Composed,
    version: &'c Version,
    staging: &Path,
) -> (Read<'c>, Vec<Fault>) {
    let path = staging.join(file_of(version));
    let mut read = Read {
        document,
        version,
        record: None,
    };
    let stored = std::fs::read_to_string(&path)
        .map_err(|error| error.to_string())
        .and_then(|text| Version::read(&version.id, &text).map_err(|error| error.to_string()));
    let what = match stored {
        Ok(stored) if stored == *version => {
            match Record::read(&version.envelope.kind, &version.fields) {
                Ok(record) => {
                    read.record = Some(record);
                    return (read, Vec::new());
                }
                Err(error) => format!("does not read as a {}: {error}", document.kind.word()),
            }
        }
        Ok(_) => "does not read as the version that was composed".to_owned(),
        Err(error) => format!("does not read: {error}"),
    };
    let fault = read.fault(what);
    (read, vec![fault])
}

fn misplaced(written: &BTreeMap<&str, KindOf>, id: &str, wanted: Option<KindOf>) -> Option<String> {
    match (written.get(id), wanted) {
        (None, _) => Some(format!("{id} is no written document")),
        (Some(kind), Some(wanted)) if *kind != wanted => Some(format!(
            "{id} is written as {}, not as {}",
            kind.word(),
            wanted.word()
        )),
        _ => None,
    }
}

fn references(read: &Read, written: &BTreeMap<&str, KindOf>) -> Vec<Fault> {
    let referred = referenced(read.document.kind, &read.version.fields);
    let faults = referred.iter().filter_map(|referred| {
        let why = misplaced(written, referred.id, referred.wanted)?;
        Some(read.fault(format!("{}: {why}", referred.key)))
    });
    faults.collect()
}

fn set_once(reads: &[Read]) -> Vec<Fault> {
    let Some(first) = reads.first() else {
        return Vec::new();
    };
    let mut faults = Vec::new();
    for key in first.document.kind.set_once() {
        let set = first.version.fields.get(*key);
        for later in &reads[1..] {
            if later.version.fields.get(*key) != set {
                faults.push(later.fault(format!(
                    "{key}: is not what the first version {} has",
                    first.version.id
                )));
            }
        }
    }
    faults
}

fn names_kept(reads: &[Read]) -> Vec<Fault> {
    let by_id: BTreeMap<&VersionId, &Read> =
        reads.iter().map(|read| (&read.version.id, read)).collect();
    let naming = |read: &Read| -> Option<BTreeSet<String>> {
        let (name, former) = read.record.as_ref()?.content.naming()?;
        let names = former.iter().chain([name]);
        Some(names.map(|name| name.as_str().to_owned()).collect())
    };
    let mut faults = Vec::new();
    for read in reads {
        let Some(names) = naming(read) else {
            continue;
        };
        let behind = (read.version.envelope.ancestors.iter())
            .filter_map(|id| naming(by_id.get(id)?))
            .flatten();
        let lost: BTreeSet<String> = behind.filter(|name| !names.contains(name)).collect();
        if !lost.is_empty() {
            let lost: Vec<String> = lost.into_iter().collect();
            let what = format!(
                "former_names: lacks {}, a name of a version behind it",
                lost.join(", ")
            );
            faults.push(read.fault(what));
        }
    }
    faults
}

fn ending(read: &Read) -> Option<Fault> {
    let record = read.record.as_ref()?;
    let reason = &record.ending.as_ref()?.reason;
    if record.content.endings().contains(reason) {
        return None;
    }
    let idea = record.content.endings() != read.document.kind.endings(false);
    let kind = if idea {
        "idea"
    } else {
        read.document.kind.word()
    };
    let what = format!("ended: {} is no reason of the kind {kind}", reason.word());
    Some(read.fault(what))
}

pub(crate) fn faults(composed: &[Composed], staging: &Path) -> Vec<Fault> {
    let written: BTreeMap<&str, KindOf> = (composed.iter())
        .map(|document| (document.id.as_str(), document.kind))
        .collect();
    let topic = |id: &DocumentId| misplaced(&written, id.as_str(), Some(KindOf::Topic));
    let mut faults = Vec::new();
    for document in composed {
        let mut reads = Vec::new();
        for version in &document.versions {
            let (read, unread) = read(document, version, staging);
            faults.extend(unread);
            if let Some(why) = topic(&version.envelope.machine) {
                faults.push(read.fault(format!("the machine that wrote it: {why}")));
            }
            faults.extend(ending(&read));
            reads.push(read);
        }
        let head = document.head().map(|head| &head.id);
        if let Some(head) = reads.iter().find(|read| Some(&read.version.id) == head) {
            faults.extend(references(head, &written));
        }
        faults.extend(set_once(&reads));
        faults.extend(names_kept(&reads));
    }
    faults
}
