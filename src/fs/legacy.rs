//! The store as it was before versions: entries by year directory, facts at
//! depth two, a `PROJECTS` map. Read once by `migrate` and never written.

use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::frontmatter::{self, Fields, Value};
use crate::domain::ports::StoreError;

use super::{corrupt, io_error};

pub struct LegacyItem {
    pub ticked: bool,
    /// The item's lines joined with a space, recheck marker included.
    pub text: String,
}

pub struct LegacyEntry {
    pub path: PathBuf,
    /// `<date>-<name>`, the file name without `.md`.
    pub name: String,
    pub fields: Fields,
    /// The body without its Follow-ups section.
    pub body: String,
    pub items: Vec<LegacyItem>,
}

pub struct LegacyFact {
    pub path: PathBuf,
    pub project: String,
    pub name: String,
    pub fields: Fields,
    pub body: String,
}

#[derive(Default)]
pub struct LegacyProject {
    pub name: String,
    pub description: Option<String>,
    pub dirs: Vec<String>,
    pub families: Vec<String>,
    pub machine: Option<String>,
}

fn read_split(path: &Path) -> Result<frontmatter::Split, StoreError> {
    let text = fs::read_to_string(path).map_err(|e| io_error(path, &e))?;
    parse_loose(&text).map_err(|reason| corrupt(path, reason))
}

fn unquote(value: &str) -> String {
    let inner = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
    match inner {
        Some(inner) => inner.replace("\\\"", "\""),
        None => value.to_owned(),
    }
}

/// The hand-written frontmatter of the old store, which was YAML read by
/// awk: scalars possibly quoted, flow lists, and block lists whose items
/// may wrap onto deeper-indented lines. A comma inside a block item splits
/// it, since the strict grammar's list items cannot hold one and the
/// items in question are paths listed two to a line.
fn parse_loose(text: &str) -> Result<frontmatter::Split, String> {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(|l| l.trim_end_matches('\n')) != Some("---") {
        return Err("no `---` fence opens the file".into());
    }
    let mut fields = Fields::default();
    let mut consumed = 4;
    let mut closed = false;
    let mut open_list: Option<(String, Vec<String>)> = None;
    for raw in lines {
        consumed += raw.len();
        let line = raw.trim_end_matches('\n');
        if line == "---" {
            closed = true;
            break;
        }
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            let Some((_, items)) = &mut open_list else {
                return Err(format!("cannot read `{line}`"));
            };
            let content = line.trim();
            if let Some(item) = content.strip_prefix("- ") {
                items.push(unquote(item.trim()));
            } else if let Some(last) = items.last_mut() {
                last.push(' ');
                last.push_str(content);
            } else {
                return Err(format!("cannot read `{line}`"));
            }
            continue;
        }
        if let Some((key, items)) = open_list.take() {
            fields.push(&key, split_items(&items));
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("cannot read `{line}`"));
        };
        let value = value.trim();
        if value.is_empty() {
            open_list = Some((key.to_owned(), Vec::new()));
        } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let items = inner
                .split(',')
                .map(|s| unquote(s.trim()))
                .filter(|s| !s.is_empty())
                .collect();
            fields.push(key, Value::List(items));
        } else {
            fields.push(key, Value::Scalar(unquote(value)));
        }
    }
    if let Some((key, items)) = open_list.take() {
        fields.push(&key, split_items(&items));
    }
    if !closed {
        return Err("the `---` fence is never closed".into());
    }
    Ok(frontmatter::Split {
        fields,
        body: text[consumed.min(text.len())..].to_owned(),
    })
}

fn split_items(items: &[String]) -> Value {
    Value::List(
        items
            .iter()
            .flat_map(|item| item.split(',').map(|s| s.trim().to_owned()))
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// Splits a body into the text without its Follow-ups section and the
/// items that section held, by the rules the bash tool applied: the
/// section runs from `## Follow-ups` to the next `## ` heading, an item
/// opens with `- [ ]` or `- [x]` and continues on indented lines.
fn split_followups(body: &str) -> (String, Vec<LegacyItem>) {
    let mut kept = String::new();
    let mut items: Vec<LegacyItem> = Vec::new();
    let mut in_section = false;
    let mut current: Option<LegacyItem> = None;
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some(item) = current.take() {
                items.push(item);
            }
            in_section = heading.trim_start().starts_with("Follow-ups");
            if !in_section {
                kept.push_str(line);
                kept.push('\n');
            }
            continue;
        }
        if !in_section {
            kept.push_str(line);
            kept.push('\n');
            continue;
        }
        let opened = line
            .strip_prefix("- [ ]")
            .map(|rest| (false, rest))
            .or_else(|| line.strip_prefix("- [x]").map(|rest| (true, rest)))
            .or_else(|| line.strip_prefix("- [X]").map(|rest| (true, rest)));
        if let Some((ticked, rest)) = opened {
            if let Some(item) = current.take() {
                items.push(item);
            }
            current = Some(LegacyItem {
                ticked,
                text: rest.trim().to_owned(),
            });
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            if let Some(item) = &mut current {
                item.text.push(' ');
                item.text.push_str(line.trim());
            }
        } else {
            if let Some(item) = current.take() {
                items.push(item);
            }
            // Prose inside the section that is not an item stays with the
            // entry, since dropping words nobody sees again is not migration.
            if !line.trim().is_empty() {
                kept.push_str(line);
                kept.push('\n');
            }
        }
    }
    if let Some(item) = current.take() {
        items.push(item);
    }
    // Trailing blank lines that the removed section left behind.
    while kept.ends_with("\n\n") {
        kept.pop();
    }
    (kept, items)
}

/// Every entry under `root/<year>/<date>-<name>.md`, oldest first.
pub fn read_entries(root: &Path) -> Result<Vec<LegacyEntry>, StoreError> {
    let mut entries = Vec::new();
    let years = fs::read_dir(root).map_err(|e| io_error(root, &e))?;
    for year in years {
        let year = year.map_err(|e| io_error(root, &e))?.path();
        let is_year = year.is_dir()
            && year.file_name().is_some_and(|n| {
                n.len() == 4 && n.to_string_lossy().bytes().all(|b| b.is_ascii_digit())
            });
        if !is_year {
            continue;
        }
        for file in fs::read_dir(&year).map_err(|e| io_error(&year, &e))? {
            let path = file.map_err(|e| io_error(&year, &e))?.path();
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".md"))
            else {
                continue;
            };
            let split = read_split(&path)?;
            let (body, items) = split_followups(&split.body);
            entries.push(LegacyEntry {
                name: name.to_owned(),
                path,
                fields: split.fields,
                body,
                items,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

/// Every fact at `root/<project>/<name>.md`.
pub fn read_facts(root: &Path) -> Result<Vec<LegacyFact>, StoreError> {
    let mut facts = Vec::new();
    for project in fs::read_dir(root).map_err(|e| io_error(root, &e))? {
        let dir = project.map_err(|e| io_error(root, &e))?.path();
        if !dir.is_dir() {
            continue;
        }
        let project = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        for file in fs::read_dir(&dir).map_err(|e| io_error(&dir, &e))? {
            let path = file.map_err(|e| io_error(&dir, &e))?.path();
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".md"))
            else {
                continue;
            };
            let split = read_split(&path)?;
            facts.push(LegacyFact {
                name: name.to_owned(),
                project: project.clone(),
                path,
                fields: split.fields,
                body: split.body,
            });
        }
    }
    facts.sort_by(|a, b| (&a.project, &a.name).cmp(&(&b.project, &b.name)));
    Ok(facts)
}

/// The `PROJECTS` map: `<project> <claim>…` lines, continuation lines of
/// more claims, and an em-dash line describing the project.
pub fn read_projects(path: &Path) -> Result<Vec<LegacyProject>, StoreError> {
    let text = fs::read_to_string(path).map_err(|e| io_error(path, &e))?;
    let mut projects: Vec<LegacyProject> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let continuation = line.starts_with(char::is_whitespace);
        let content = line.trim();
        if continuation {
            let Some(project) = projects.last_mut() else {
                continue;
            };
            if let Some(description) = content.strip_prefix('—') {
                project.description = Some(description.trim().to_owned());
            } else {
                for claim in content.split_whitespace() {
                    add_claim(project, claim);
                }
            }
            continue;
        }
        let mut words = content.split_whitespace();
        let Some(name) = words.next() else { continue };
        let mut project = LegacyProject {
            name: name.to_owned(),
            ..LegacyProject::default()
        };
        for claim in words {
            add_claim(&mut project, claim);
        }
        projects.push(project);
    }
    Ok(projects)
}

fn add_claim(project: &mut LegacyProject, claim: &str) {
    if let Some(host) = claim.strip_prefix('@') {
        project.machine = Some(host.to_owned());
    } else if claim == "-" {
    } else if claim.ends_with('-') {
        project.families.push(claim.to_owned());
    } else {
        project.dirs.push(claim.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn followups_are_split_off_the_body() {
        let body = "\n## What\nDid it.\n\n## Follow-ups\n- [ ] Port it (recheck: 2026-10-01\n      why)\n- [x] Done thing\n  with a note\n\n## Notes\nLater.\n";
        let (kept, items) = split_followups(body);
        assert_eq!(kept, "\n## What\nDid it.\n\n## Notes\nLater.\n");
        assert_eq!(items.len(), 2);
        assert!(!items[0].ticked);
        assert_eq!(items[0].text, "Port it (recheck: 2026-10-01 why)");
        assert!(items[1].ticked);
        assert_eq!(items[1].text, "Done thing with a note");
    }

    #[test]
    fn loose_frontmatter() {
        let text = "---\ndate: 2026-08-27\ntags: [a, b]\nfiles_touched:\n  - db/migrate/x.rb\n  - app/a.rb, app/b.rb\n  - lib/long\n    wrapped\nsummary: \"Quoted: yes\"\n---\nbody\n";
        let split = parse_loose(text).unwrap();
        assert_eq!(split.fields.scalar("date"), Some("2026-08-27"));
        assert_eq!(split.fields.list("tags").unwrap(), ["a", "b"]);
        assert_eq!(
            split.fields.list("files_touched").unwrap(),
            [
                "db/migrate/x.rb",
                "app/a.rb",
                "app/b.rb",
                "lib/long wrapped"
            ]
        );
        assert_eq!(split.fields.scalar("summary"), Some("Quoted: yes"));
        assert_eq!(split.body, "body\n");
    }

    #[test]
    fn projects_map() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PROJECTS");
        fs::write(
            &path,
            "# comment\nlantern  ~/projects/lantern  ~/projects/firmware\n                  — Rust app\nlaptop  @laptop  ~/projects/lab-\n                  ~/src/bench\n                  — A laptop\nphone-a  -\n                  — A phone\n",
        )
        .unwrap();
        let projects = read_projects(&path).unwrap();
        assert_eq!(projects.len(), 3);
        assert_eq!(
            projects[0].dirs,
            ["~/projects/lantern", "~/projects/firmware"]
        );
        assert_eq!(projects[0].description.as_deref(), Some("Rust app"));
        assert_eq!(projects[1].machine.as_deref(), Some("laptop"));
        assert_eq!(projects[1].families, ["~/projects/lab-"]);
        assert_eq!(projects[1].dirs, ["~/src/bench"]);
        assert!(projects[2].dirs.is_empty());
    }
}
