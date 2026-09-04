//! The `SessionStart` hook: one entry in the agent's settings file that runs
//! `worklog context` when a session opens.

use std::path::Path;

use serde_json::{Map, Value, json};

use crate::app::Failure;
use crate::fs::write_file;

/// The command the hook runs. The binary's own path rather than a bare
/// name, since a hook runs with whatever PATH the agent was started with;
/// a missing store prints a notice and exits 0, so the `|| true` covers
/// only a binary that is gone.
fn command() -> Result<String, Failure> {
    let exe = std::env::current_exe()
        .map_err(|e| Failure::Refused(format!("cannot locate this binary: {e}")))?;
    Ok(format!("\"{}\" context 2>/dev/null || true", exe.display()))
}

/// The hook entry as the settings file holds it.
pub fn entry() -> Result<Value, Failure> {
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

/// What merging the hook into a settings document came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Merged {
    Added(Value),
    Present,
}

/// Adds the hook to a settings document unless one is there. Pure: the
/// caller reads and writes the file.
pub fn merge(mut root: Value, entry: Value) -> Result<Merged, String> {
    let Some(object) = root.as_object_mut() else {
        return Err("the top level is not an object".into());
    };
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err("`hooks` is not an object".into());
    };
    let session_start = hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(entries) = session_start.as_array_mut() else {
        return Err("`hooks.SessionStart` is not an array".into());
    };
    if entries.iter().any(any_runs_context) {
        return Ok(Merged::Present);
    }
    entries.push(entry);
    Ok(Merged::Added(root))
}

/// Merges the hook into the settings file, keeping the rest of it as it
/// is, and says whether anything was written.
pub fn install(settings: &Path) -> Result<Merged, Failure> {
    let refuse = |reason: String| Failure::Refused(format!("{}: {reason}", settings.display()));
    let text = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_owned(),
        Err(e) => return Err(refuse(e.to_string())),
    };
    let root: Value =
        serde_json::from_str(&text).map_err(|e| refuse(format!("not readable as JSON, {e}")))?;
    let merged = merge(root, entry()?).map_err(refuse)?;
    if let Merged::Added(root) = &merged {
        let mut out = serde_json::to_string_pretty(root).map_err(|e| refuse(e.to_string()))?;
        out.push('\n');
        write_file(settings, &out)?;
    }
    Ok(merged)
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
        let added = merge(json!({}), probe()).unwrap();
        let Merged::Added(root) = added else {
            panic!("an empty document gains the hook")
        };
        assert_eq!(root["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(merge(root, probe()).unwrap(), Merged::Present);
        let existing =
            serde_json::from_str::<Value>(r#"{"zeta": 1, "hooks": {"Stop": []}, "alpha": 2}"#)
                .unwrap();
        let Merged::Added(root) = merge(existing, probe()).unwrap() else {
            panic!("a document without the hook gains it")
        };
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
    fn install_leaves_a_settled_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join(".claude/settings.json");
        assert!(matches!(install(&settings).unwrap(), Merged::Added(_)));
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.ends_with("}\n"));
        assert_eq!(install(&settings).unwrap(), Merged::Present);
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), text);
        std::fs::write(&settings, "not json").unwrap();
        assert!(matches!(install(&settings), Err(Failure::Refused(m)) if m.contains("JSON")));
    }
}
