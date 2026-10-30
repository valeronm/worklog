//! The drafts directory among the roots an agent's sandbox lets a session write to.

use std::path::Path;

use toml_edit::{Array, DocumentMut, Item, Table};

use crate::app::Failure;
use crate::wiring::{Agent, SandboxConfig};

const TABLE: &str = "sandbox_workspace_write";
const ROOTS: &str = "writable_roots";

// Codex is not documented to expand `~` or a variable in a writable root.
fn root(drafts: &Path) -> Result<&str, Failure> {
    if !drafts.is_absolute() {
        return Err(Failure::at(drafts.display(), "is not an absolute path"));
    }
    drafts
        .to_str()
        .ok_or_else(|| Failure::at(drafts.display(), "is not UTF-8"))
}

fn add(roots: &mut Array, root: &str) -> bool {
    if roots.iter().any(|held| held.as_str() == Some(root)) {
        return false;
    }
    roots.push(root);
    true
}

fn remove(roots: &mut Array, root: &str) -> bool {
    let before = roots.len();
    roots.retain(|held| held.as_str() != Some(root));
    roots.len() != before
}

type Change = fn(&mut Array, &str) -> bool;

fn roots(document: &mut DocumentMut) -> Result<&mut Array, String> {
    let table = (document.entry(TABLE))
        .or_insert(Item::Table(Table::new()))
        .as_table_like_mut()
        .ok_or_else(|| format!("`{TABLE}` is not a table"))?;
    (table.entry(ROOTS))
        .or_insert(Item::Value(Array::new().into()))
        .as_array_mut()
        .ok_or_else(|| format!("`{TABLE}.{ROOTS}` is not an array"))
}

// An empty array or table left by a removal is one an install may have made.
fn pruned(document: &mut DocumentMut) {
    let Some(table) = document.get_mut(TABLE).and_then(Item::as_table_like_mut) else {
        return;
    };
    if (table.get(ROOTS).and_then(Item::as_array)).is_some_and(Array::is_empty) {
        table.remove(ROOTS);
    }
    if table.is_empty() {
        document.remove(TABLE);
    }
}

// A document left unchanged is dropped with the table and array `roots` may have made in it.
fn edited(text: &str, root: &str, change: Change) -> Result<Option<String>, String> {
    let mut document: DocumentMut = text
        .parse()
        .map_err(|e| format!("not readable as TOML, {e}"))?;
    if !change(roots(&mut document)?, root) {
        return Ok(None);
    }
    pruned(&mut document);
    Ok(Some(document.to_string()))
}

/// An agent's sandbox config with the text it is to be written with.
pub(super) struct Edit {
    pub(super) config: SandboxConfig,
    pub(super) text: String,
}

fn planned(agent: &Agent, drafts: &Path, change: Change) -> Result<Option<Edit>, Failure> {
    let Some(config) = agent.config() else {
        return Ok(None);
    };
    let root = root(drafts)?;
    let held = agent.read_config(&config)?.unwrap_or_default();
    let text = edited(&held, root, change)
        .map_err(|reason| Failure::at(config.path().display(), reason))?;
    Ok(text.map(|text| Edit { config, text }))
}

/// Reads the agent's config and writes nothing: its text with `drafts` among the writable
/// roots, or `None` where it is there or the agent has no config; a refusal where the file is
/// not TOML or its roots are not a table holding an array.
pub(super) fn installed(agent: &Agent, drafts: &Path) -> Result<Option<Edit>, Failure> {
    planned(agent, drafts, add)
}

/// Reads the agent's config and writes nothing: its text with `drafts` taken out of the
/// writable roots, or `None` where it is not there.
pub(super) fn uninstalled(agent: &Agent, drafts: &Path) -> Result<Option<Edit>, Failure> {
    planned(agent, drafts, remove)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAFTS: &str = "/home/desk/.local/state/worklog/drafts";

    fn added(text: &str) -> Option<String> {
        edited(text, DRAFTS, add).unwrap()
    }

    fn taken(text: &str) -> Option<String> {
        edited(text, DRAFTS, remove).unwrap()
    }

    #[test]
    fn an_empty_config_gets_the_table_and_the_root() {
        assert_eq!(
            added("").as_deref(),
            Some(format!("[{TABLE}]\n{ROOTS} = [\"{DRAFTS}\"]\n").as_str())
        );
    }

    #[test]
    fn the_root_is_added_once_after_the_roots_there_and_the_rest_is_kept_as_written() {
        let before = "# mine\nmodel = \"atlas\"  # pinned\n\n[sandbox_workspace_write]\n\
                      network_access = true\nwritable_roots = [\"/lantern\"]\n\n[tui]\nx = 1\n";
        let after = added(before).unwrap();
        assert_eq!(
            after,
            format!(
                "# mine\nmodel = \"atlas\"  # pinned\n\n[sandbox_workspace_write]\n\
                 network_access = true\nwritable_roots = [\"/lantern\", \"{DRAFTS}\"]\n\n\
                 [tui]\nx = 1\n"
            )
        );
        assert_eq!(added(&after), None);
    }

    #[test]
    fn a_dotted_or_inline_table_takes_the_root_where_it_is() {
        let dotted = "sandbox_workspace_write.network_access = true\n";
        let after = added(dotted).unwrap();
        assert!(after.contains(DRAFTS), "{after}");
        assert_eq!(after.matches(TABLE).count(), 2, "{after}");
        let inline = "sandbox_workspace_write = { writable_roots = [] }\n";
        assert_eq!(
            added(inline).unwrap(),
            format!("sandbox_workspace_write = {{ writable_roots = [\"{DRAFTS}\"] }}\n")
        );
    }

    #[test]
    fn a_removal_takes_only_the_root_and_what_it_leaves_empty() {
        let theirs = format!("[{TABLE}]\n{ROOTS} = [\"/lantern\", \"{DRAFTS}\"]\n");
        assert_eq!(
            taken(&theirs).as_deref(),
            Some(format!("[{TABLE}]\n{ROOTS} = [\"/lantern\"]\n").as_str())
        );
        let kept =
            format!("[tui]\nx = 1\n\n[{TABLE}]\nnetwork_access = true\n{ROOTS} = [\"{DRAFTS}\"]\n");
        assert_eq!(
            taken(&kept).as_deref(),
            Some(format!("[tui]\nx = 1\n\n[{TABLE}]\nnetwork_access = true\n").as_str())
        );
        let before = "model = \"atlas\"\n";
        assert_eq!(taken(&added(before).unwrap()).as_deref(), Some(before));
        assert_eq!(taken(before), None);
        assert_eq!(taken(&format!("[{TABLE}]\n")), None);
        assert_eq!(
            taken(&format!("[{TABLE}]\n{ROOTS} = [\"/lantern\"]\n")),
            None
        );
    }

    #[test]
    fn a_config_of_another_shape_is_refused_with_what_is_wrong() {
        for (text, reason) in [
            ("model = ", "not readable as TOML"),
            (
                "sandbox_workspace_write = 4\n",
                "`sandbox_workspace_write` is not a table",
            ),
            (
                "[sandbox_workspace_write]\nwritable_roots = \"/lantern\"\n",
                "`sandbox_workspace_write.writable_roots` is not an array",
            ),
        ] {
            for change in [add as Change, remove] {
                let refused = edited(text, DRAFTS, change).unwrap_err();
                assert!(refused.starts_with(reason), "{text}: {refused}");
            }
        }
    }

    #[test]
    fn a_root_is_an_absolute_utf_8_path() {
        assert_eq!(root(Path::new(DRAFTS)), Ok(DRAFTS));
        let refused = root(Path::new("state/drafts")).unwrap_err().to_string();
        assert_eq!(refused, "state/drafts: is not an absolute path");
    }
}
