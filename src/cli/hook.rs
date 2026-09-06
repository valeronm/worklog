//! The `SessionStart` hook: one entry in an agent's hooks file that runs
//! `worklog context` when a session opens. Claude Code's `settings.json`
//! and Codex's `hooks.json` hold hooks in the same shape, so one entry and
//! one set of edits serve both.

use serde_json::{Map, Value, json};

use crate::app::Failure;
use crate::fs::Agent;

/// The command the hook runs. The binary's own path rather than a bare
/// name, since a hook runs with whatever PATH the agent was started with;
/// a missing store prints a notice and exits 0, so the `|| true` covers
/// only a binary that is gone.
fn command() -> Result<String, Failure> {
    Ok(format!(
        "\"{}\" context 2>/dev/null || true",
        super::this_binary()?.display()
    ))
}

/// The hook entry as a hooks file holds it.
fn entry() -> Result<Value, Failure> {
    Ok(json!({
        "hooks": [{ "type": "command", "command": command()? }]
    }))
}

/// Whether a command line runs `worklog context`: a `worklog` binary, by
/// any path, quoting or version suffix, followed by `context`.
fn runs_context(command: &str) -> bool {
    let mut words = command
        .split_whitespace()
        .map(|w| w.trim_matches(|c| c == '"' || c == '\''));
    while let Some(word) = words.next() {
        let binary = word.rsplit('/').next().unwrap_or(word);
        let ours = binary == "worklog" || binary.starts_with("worklog-");
        if ours && words.next() == Some("context") {
            return true;
        }
    }
    false
}

fn any_runs_context(session_start: &Value) -> bool {
    session_start["hooks"].as_array().is_some_and(|hooks| {
        hooks
            .iter()
            .any(|h| h["command"].as_str().is_some_and(runs_context))
    })
}

/// The `hooks.SessionStart` entries of a document, the path to them made
/// where missing; a change that then finds nothing to do returns `None`
/// and the made path is never written.
fn entries(root: &mut Value) -> Result<&mut Vec<Value>, String> {
    let object = root
        .as_object_mut()
        .ok_or("the top level is not an object")?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("`hooks` is not an object")?;
    hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "`hooks.SessionStart` is not an array".into())
}

/// Adds the hook to a hooks document and returns it, or `None` when one is
/// there.
fn merge(mut root: Value, entry: Value) -> Result<Option<Value>, String> {
    let entries = entries(&mut root)?;
    if entries.iter().any(any_runs_context) {
        return Ok(None);
    }
    entries.push(entry);
    Ok(Some(root))
}

/// Takes every entry that runs `worklog context` out of a hooks document
/// and returns it, or `None` when there is none. A `SessionStart` left
/// empty goes with them, and a `hooks` left empty with that, since
/// `merge` is what made them.
fn remove(mut root: Value) -> Result<Option<Value>, String> {
    let entries = entries(&mut root)?;
    let before = entries.len();
    entries.retain(|group| !any_runs_context(group));
    if entries.len() == before {
        return Ok(None);
    }
    if entries.is_empty()
        && let Some(hooks) = root["hooks"].as_object_mut()
    {
        hooks.remove("SessionStart");
        if hooks.is_empty()
            && let Some(object) = root.as_object_mut()
        {
            object.remove("hooks");
        }
    }
    Ok(Some(root))
}

/// Puts the current entry in the place of every entry that runs `worklog
/// context`, so a hook that names a binary since moved names this one,
/// and returns the document, or `None` when there is none or none differ.
fn replace(mut root: Value, entry: &Value) -> Result<Option<Value>, String> {
    let entries = entries(&mut root)?;
    let mut changed = false;
    for group in entries.iter_mut().filter(|g| any_runs_context(g)) {
        if group != entry {
            *group = entry.clone();
            changed = true;
        }
    }
    Ok(changed.then_some(root))
}

/// Applies a change to an agent's hooks document, an absent file being an
/// empty one, writes it back when the change returns one, and says
/// whether it did.
fn edit(
    agent: &Agent,
    change: impl FnOnce(Value) -> Result<Option<Value>, String>,
) -> Result<bool, Failure> {
    let refuse = |reason: String| Failure::Refused(format!("{}: {reason}", agent.hooks.display()));
    let text = agent.read_hooks()?.unwrap_or_else(|| "{}".to_owned());
    let root =
        serde_json::from_str(&text).map_err(|e| refuse(format!("not readable as JSON, {e}")))?;
    match change(root).map_err(refuse)? {
        Some(root) => {
            agent.write_hooks(&super::pretty_json(&root)?)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Merges the hook into the agent's hooks file and says whether it was
/// written.
pub fn install(agent: &Agent) -> Result<bool, Failure> {
    let entry = entry()?;
    edit(agent, |root| merge(root, entry))
}

/// Takes the hook out of the agent's hooks file and says whether it was
/// written.
pub fn uninstall(agent: &Agent) -> Result<bool, Failure> {
    edit(agent, remove)
}

/// Brings the hook in the agent's hooks file up to this binary and says
/// whether it was written.
pub fn refresh(agent: &Agent) -> Result<bool, Failure> {
    let entry = entry()?;
    edit(agent, |root| replace(root, &entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> Value {
        json!({ "hooks": [{ "type": "command", "command": "worklog context" }] })
    }

    #[test]
    fn recognises_the_hook_however_it_is_spelled() {
        assert!(runs_context("worklog context"));
        assert!(runs_context(
            "\"$HOME/.local/bin/worklog\" context 2>/dev/null || true"
        ));
        assert!(runs_context("'/opt/bin/worklog' context"));
        assert!(runs_context("worklog-0.2 context"));
        assert!(!runs_context("mylog context"));
        assert!(!runs_context("echo worklog; echo context"));
        assert!(!runs_context("worklog check"));
    }

    #[test]
    fn merges_once_and_keeps_the_rest() {
        let root = merge(json!({}), probe()).unwrap().expect("gained");
        assert_eq!(root["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(merge(root, probe()).unwrap(), None);
        let existing =
            serde_json::from_str::<Value>(r#"{"zeta": 1, "hooks": {"Stop": []}, "alpha": 2}"#)
                .unwrap();
        let root = merge(existing, probe()).unwrap().expect("gained");
        let keys: Vec<&str> = root
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["zeta", "hooks", "alpha"]);
        assert_eq!(root["hooks"]["Stop"], json!([]));
        assert!(merge(json!([]), probe()).is_err());
        assert!(merge(json!({"hooks": 1}), probe()).is_err());
        assert!(merge(json!({"hooks": {"SessionStart": {}}}), probe()).is_err());
    }

    #[test]
    fn removes_only_the_hook_and_what_it_made() {
        let root = merge(json!({"hooks": {"Stop": []}}), probe())
            .unwrap()
            .expect("gained");
        let root = remove(root).unwrap().expect("lost");
        assert_eq!(root, json!({"hooks": {"Stop": []}}));
        assert_eq!(remove(root).unwrap(), None);
        assert_eq!(remove(json!({})).unwrap(), None);
        let root = merge(json!({"model": "x"}), probe())
            .unwrap()
            .expect("gained");
        assert_eq!(remove(root).unwrap(), Some(json!({"model": "x"})));
        let theirs = json!({ "hooks": [{ "type": "command", "command": "date" }] });
        let both = json!({"hooks": {"SessionStart": [theirs.clone(), probe()]}});
        let root = remove(both).unwrap().expect("lost");
        assert_eq!(root, json!({"hooks": {"SessionStart": [theirs]}}));
        assert!(remove(json!({"hooks": {"SessionStart": {}}})).is_err());
    }

    #[test]
    fn replaces_the_hook_in_place_and_only_when_it_differs() {
        let theirs = json!({ "hooks": [{ "type": "command", "command": "date" }] });
        let stale = json!({ "hooks": [{ "type": "command", "command": "/old/worklog context" }] });
        let root = json!({"hooks": {"SessionStart": [stale, theirs.clone()], "Stop": []}});
        let root = replace(root, &probe()).unwrap().expect("replaced");
        assert_eq!(
            root,
            json!({"hooks": {"SessionStart": [probe(), theirs], "Stop": []}})
        );
        assert_eq!(replace(root, &probe()).unwrap(), None);
        assert_eq!(replace(json!({}), &probe()).unwrap(), None);
        assert_eq!(
            replace(json!({"hooks": {"Stop": []}}), &probe()).unwrap(),
            None
        );
    }
}
