//! The one-time move from the file-per-document store into versions.
//! Every old file becomes a first version stamped `migrate`; follow-up
//! lines become documents; the `PROJECTS` map becomes topics.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use serde::Serialize;

use crate::domain::entry::Entry;
use crate::domain::fact::Fact;
use crate::domain::followup::{Followup, FollowupState};
use crate::domain::machine::MachineName;
use crate::domain::recheck::Recheck;
use crate::domain::slug::{Kind, Slug};
use crate::domain::topic::Topic;
use crate::domain::version::Operation;
use crate::fs::legacy::{self, LegacyEntry, LegacyFact, LegacyProject};

use super::write::first_version;
use super::{Deps, Failure};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub topics: usize,
    pub facts: usize,
    pub entries: usize,
    pub followups: usize,
    /// What needs a hand afterwards, one line each.
    pub notes: Vec<String>,
}

/// `[[2026-08-29-name]]` named an entry by file name; the slug now carries
/// the year directory.
fn rewrite_links(text: &str) -> String {
    static ENTRY_LINK: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"\[\[(\d{4})(-\d{2}-\d{2}-[A-Za-z0-9._-]+)\]\]")
            .expect("a literal pattern")
    });
    ENTRY_LINK.replace_all(text, "[[$1/$1$2]]").into_owned()
}

fn kebab(text: &str, words: usize) -> String {
    let joined: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(words)
        .map(str::to_ascii_lowercase)
        .collect();
    if joined.is_empty() {
        "item".to_owned()
    } else {
        joined.join("-")
    }
}

/// The item text without its recheck marker, and the marker's value.
fn split_recheck(text: &str) -> (String, Option<String>) {
    let Some(start) = text.find("(recheck:") else {
        return (text.trim().to_owned(), None);
    };
    let Some(end) = text[start..].find(')') else {
        return (text.trim().to_owned(), None);
    };
    let value = text[start + 9..start + end].trim().to_owned();
    let mut rest = text[..start].trim_end().to_owned();
    let after = text[start + end + 1..].trim_start();
    if !after.is_empty() {
        rest.push(' ');
        rest.push_str(after);
    }
    (rest.trim().to_owned(), Some(value))
}

fn topics_of(projects: &[LegacyProject]) -> Vec<(String, Topic)> {
    let claims: Vec<(String, Vec<String>)> = projects
        .iter()
        .filter(|p| !p.dirs.is_empty())
        .map(|p| (p.name.clone(), p.dirs.clone()))
        .collect();
    let families: Vec<(String, Vec<String>)> = projects
        .iter()
        .filter(|p| !p.families.is_empty())
        .map(|p| (p.name.clone(), p.families.clone()))
        .collect();
    projects
        .iter()
        .map(|p| {
            let machine = p
                .machine
                .as_deref()
                .and_then(|m| MachineName::parse(m).ok());
            // Every host gets every claim, as the old map applied them: a
            // path absent from a host never matches there.
            let (claims, families) = if machine.is_some() {
                (claims.clone(), families.clone())
            } else {
                (vec![], vec![])
            };
            let topic = Topic {
                summary: p.description.clone().unwrap_or_else(|| p.name.clone()),
                includes: vec![],
                machine,
                claims,
                families,
                unclaimed: vec![],
            };
            (p.name.clone(), topic)
        })
        .collect()
}

/// A recheck value from the old store, or a note about why it was dropped.
fn carried_recheck(value: Option<&str>, at: &str, report: &mut Report) -> Option<Recheck> {
    match value.map(Recheck::parse) {
        Some(Ok(r)) => Some(r),
        Some(Err(e)) => {
            report.notes.push(format!("{at}: recheck dropped, {e}"));
            None
        }
        None => None,
    }
}

fn migrate_fact(deps: &Deps, fact: &LegacyFact, report: &mut Report) -> Result<(), Failure> {
    let slug = match Slug::fact(&fact.project, &fact.name) {
        Ok(slug) => slug,
        Err(e) => {
            report
                .notes
                .push(format!("{}: skipped, {e}", fact.path.display()));
            return Ok(());
        }
    };
    let fields = &fact.fields;
    if fields.optional("scope").is_some() {
        report.notes.push(format!(
            "{slug}: carried `scope: machine`; decide which topic's directories it reaches"
        ));
    }
    let recheck = carried_recheck(fields.optional("recheck"), &slug.to_string(), report);
    for (key, _) in fields.iter() {
        if !["updated", "tags", "scope", "kind", "recheck", "summary"].contains(&key) {
            report.notes.push(format!("{slug}: field `{key}` dropped"));
        }
    }
    let data = Fact {
        tags: fields.list_or_empty("tags"),
        idea: fields.optional("kind") == Some("idea"),
        recheck,
        verified: fields.optional("updated").map(str::to_owned),
        summary: rewrite_links(fields.optional("summary").unwrap_or("(no summary)")),
    };
    first_version(
        deps,
        slug,
        Operation::Migrate,
        data.to_fields(),
        rewrite_links(&fact.body),
    )?;
    report.facts += 1;
    Ok(())
}

fn migrate_entry(
    deps: &Deps,
    entry: &LegacyEntry,
    taken: &mut BTreeSet<String>,
    report: &mut Report,
) -> Result<(), Failure> {
    let day = entry.name.get(..10).unwrap_or(&entry.name).to_owned();
    let name = entry.name.get(11..).unwrap_or_default();
    let slug = match Slug::entry(&day, name) {
        Ok(slug) => slug,
        Err(e) => {
            report
                .notes
                .push(format!("{}: skipped, {e}", entry.path.display()));
            return Ok(());
        }
    };
    let fields = &entry.fields;
    let machine = if let Some(Ok(m)) = fields.optional("machine").map(MachineName::parse) {
        m
    } else {
        report
            .notes
            .push(format!("{slug}: no machine recorded; stamped `unrecorded`"));
        MachineName::parse("unrecorded").expect("a valid placeholder")
    };
    let tags = fields.list_or_empty("tags");
    let data = Entry {
        date: day.clone(),
        machine,
        tags: tags.clone(),
        files_touched: fields.list_or_empty("files_touched"),
        summary: rewrite_links(fields.optional("summary").unwrap_or("(no summary)")),
    };
    first_version(
        deps,
        slug.clone(),
        Operation::Migrate,
        data.to_fields(),
        rewrite_links(&entry.body),
    )?;
    report.entries += 1;
    for item in &entry.items {
        let (text, recheck) = split_recheck(&item.text);
        let at = format!("{slug} item `{}`", kebab(&text, 4));
        let recheck = carried_recheck(recheck.as_deref(), &at, report);
        let base = kebab(&text, 6);
        let mut name = base.clone();
        let mut n = 1;
        while taken.contains(&name) {
            n += 1;
            name = format!("{base}-{n}");
        }
        taken.insert(name.clone());
        let data = Followup {
            entry: slug.clone(),
            tags: tags.clone(),
            recheck,
            state: if item.ticked {
                FollowupState::Done
            } else {
                FollowupState::Open
            },
            summary: rewrite_links(&text),
        };
        first_version(
            deps,
            Slug::followup(&day, &name)?,
            Operation::Migrate,
            data.to_fields(),
            "\n".to_owned(),
        )?;
        report.followups += 1;
    }
    Ok(())
}

/// Reads the old store and writes the new one, which must be empty.
pub fn migrate(
    deps: &Deps,
    entries: &Path,
    facts: &Path,
    projects: &Path,
) -> Result<Report, Failure> {
    for kind in Kind::ALL {
        if !deps.store.slugs(kind)?.is_empty() {
            return Err(Failure::Refused(
                "the store already holds documents; migrate into an empty one".into(),
            ));
        }
    }
    let mut report = Report::default();
    for (name, topic) in topics_of(&legacy::read_projects(projects)?) {
        let slug = match Slug::of_kind(Kind::Topic, &name) {
            Ok(slug) => slug,
            Err(e) => {
                report.notes.push(format!("project {name}: skipped, {e}"));
                continue;
            }
        };
        if topic.machine.is_some() {
            report.notes.push(format!(
                "{slug}: machine topic; every project's paths are claimed on it, prune the ones this host lacks"
            ));
        }
        first_version(
            deps,
            slug,
            Operation::Migrate,
            topic.to_fields(),
            "\n".to_owned(),
        )?;
        report.topics += 1;
    }
    for fact in legacy::read_facts(facts)? {
        migrate_fact(deps, &fact, &mut report)?;
    }
    let mut taken = BTreeSet::new();
    for entry in legacy::read_entries(entries)? {
        migrate_entry(deps, &entry, &mut taken, &mut report)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_gain_the_year_directory() {
        assert_eq!(
            rewrite_links("see [[2026-08-29-x]] and [[lantern/relay]] and `[[2026-01-01-x]]`"),
            "see [[2026/2026-08-29-x]] and [[lantern/relay]] and `[[2026/2026-01-01-x]]`"
        );
    }

    #[test]
    fn recheck_markers_come_off_the_text() {
        let (text, recheck) = split_recheck("Port it (recheck: 2026-10-01 a month) then rest");
        assert_eq!(text, "Port it then rest");
        assert_eq!(recheck.as_deref(), Some("2026-10-01 a month"));
        assert_eq!(split_recheck("Plain").1, None);
        assert_eq!(
            kebab("Add the `second` relay per the schematic above", 6),
            "add-the-second-relay-per-the"
        );
    }
}
