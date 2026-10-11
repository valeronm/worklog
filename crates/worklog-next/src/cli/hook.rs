use std::path::Path;

use serde_json::{Map, Value, json};

use super::NAME;
use crate::app::Failure;
use crate::wiring::Agent;

// A shell still interprets these inside double quotes.
const UNQUOTABLE: [char; 4] = ['"', '$', '\\', '`'];

fn refuse_binary(binary: &Path, reason: &str) -> Failure {
    Failure::at(binary.display(), reason)
}

fn spelled<'a>(binary: &'a str, homes: &[&Path]) -> (&'static str, &'a str) {
    for home in homes {
        if let Some(rest) = Path::new(binary)
            .strip_prefix(home)
            .ok()
            .and_then(Path::to_str)
        {
            return ("$HOME/", rest);
        }
    }
    ("", binary)
}

/// The command the hook runs for `binary`, spelled with a leading `$HOME` for a binary under
/// one of `homes`; refuses a path that is not UTF-8 or holds one of `UNQUOTABLE`, and one whose
/// command `runs_context` does not match.
pub(super) fn command(binary: &Path, homes: &[&Path]) -> Result<String, Failure> {
    let Some(text) = binary.to_str() else {
        return Err(refuse_binary(binary, "is not UTF-8"));
    };
    let (home, rest) = spelled(text, homes);
    if let Some(found) = rest.chars().find(|c| UNQUOTABLE.contains(c)) {
        return Err(refuse_binary(
            binary,
            &format!("holds {found:?}, which a hook command cannot quote"),
        ));
    }
    let command = format!("\"{home}{rest}\" context 2>/dev/null || true");
    if !runs_context(&command) {
        return Err(refuse_binary(
            binary,
            &format!("the binary's file name must be {NAME}, or its hook is not found again"),
        ));
    }
    Ok(command)
}

fn agent_command(binary: &Path, agent: &Agent) -> Result<String, Failure> {
    command(binary, &agent.homes())
}

fn entry(command: &str) -> Value {
    json!({
        "hooks": [{ "type": "command", "command": command }]
    })
}

// `None` marks an operator, which ends the adjacency of the words around it.
fn words(command: &str) -> Vec<Option<String>> {
    let mut found = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in command.chars() {
        let operator = ";|&()<>".contains(c);
        match quote {
            Some(open) if c == open => quote = None,
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c.is_whitespace() || operator => {
                if !word.is_empty() {
                    found.push(Some(std::mem::take(&mut word)));
                }
                if operator {
                    found.push(None);
                }
            }
            Some(_) | None => word.push(c),
        }
    }
    if !word.is_empty() {
        found.push(Some(word));
    }
    found
}

fn runs_context(command: &str) -> bool {
    words(command).windows(2).any(|pair| {
        matches!(pair, [Some(binary), Some(next)]
            if binary.rsplit('/').next() == Some(NAME) && next == "context")
    })
}

fn runs_context_hook(hook: &Value) -> bool {
    hook["command"].as_str().is_some_and(runs_context)
}

fn any_runs_context(session_start: &Value) -> bool {
    session_start["hooks"]
        .as_array()
        .is_some_and(|hooks| hooks.iter().any(runs_context_hook))
}

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

fn merge(entries: &mut Vec<Value>, entry: Value) -> bool {
    if entries.iter().any(any_runs_context) {
        return false;
    }
    entries.push(entry);
    true
}

fn remove(entries: &mut Vec<Value>) -> bool {
    let before = entries.len();
    entries.retain(|group| !any_runs_context(group));
    entries.len() != before
}

fn keep_first(list: &mut Vec<Value>, owned: fn(&Value) -> bool) -> bool {
    let before = list.len();
    let mut found = false;
    list.retain(|item| !owned(item) || !std::mem::replace(&mut found, true));
    list.len() != before
}

// A group that runs `context` may hold a person's own keys and hooks beside that hook.
fn replace(entries: &mut Vec<Value>, command: &str) -> bool {
    let mut changed = keep_first(entries, any_runs_context);
    let hooks = entries
        .iter_mut()
        .find(|group| any_runs_context(group))
        .and_then(|group| group.get_mut("hooks"))
        .and_then(Value::as_array_mut);
    if let Some(hooks) = hooks {
        changed |= keep_first(hooks, runs_context_hook);
        if let Some(hook) = hooks.iter_mut().find(|hook| runs_context_hook(hook))
            && hook["command"] != command
        {
            hook["command"] = Value::from(command);
            changed = true;
        }
    }
    changed
}

fn pruned(mut root: Value) -> Value {
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut)
        && hooks
            .get("SessionStart")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
    {
        hooks.remove("SessionStart");
        if hooks.is_empty()
            && let Some(object) = root.as_object_mut()
        {
            object.remove("hooks");
        }
    }
    root
}

fn edited(
    file: &Path,
    mut root: Value,
    change: impl FnOnce(&mut Vec<Value>) -> Result<bool, Failure>,
) -> Result<Option<Value>, Failure> {
    let entries = entries(&mut root).map_err(|reason| Failure::at(file.display(), reason))?;
    Ok(change(entries)?.then(|| pruned(root)))
}

// A hooks file is a person's, not a result of a command.
fn text_of(root: &Value) -> String {
    let mut text = serde_json::to_string_pretty(root).expect("a value with text for keys");
    text.push('\n');
    text
}

fn planned(
    agent: &Agent,
    change: impl FnOnce(&mut Vec<Value>) -> Result<bool, Failure>,
) -> Result<Option<String>, Failure> {
    let file = agent.hooks();
    let text = agent.read_hooks()?.unwrap_or_else(|| "{}".to_owned());
    let root = serde_json::from_str(&text)
        .map_err(|e| Failure::at(file.display(), format!("not readable as JSON, {e}")))?;
    Ok(edited(file, root, change)?.as_ref().map(text_of))
}

/// Reads the agent's hooks file and writes nothing: its text with the hook for `binary` added,
/// or `None` where a hook is there, and a refusal where it is not the shape of a hooks file.
pub(super) fn installed(agent: &Agent, binary: &Path) -> Result<Option<String>, Failure> {
    let command = agent_command(binary, agent)?;
    planned(agent, |entries| Ok(merge(entries, entry(&command))))
}

/// Reads the agent's hooks file and writes nothing: its text with the hook taken out, or `None`
/// where there is none.
pub(super) fn uninstalled(agent: &Agent) -> Result<Option<String>, Failure> {
    planned(agent, |entries| Ok(remove(entries)))
}

/// Reads the agent's hooks file and writes nothing: its text with one hook, running `binary`,
/// or `None` where that is what it holds; `binary` is asked for only where a hook is there.
pub(super) fn refreshed<'p>(
    agent: &Agent,
    binary: impl FnOnce() -> Result<&'p Path, Failure>,
) -> Result<Option<String>, Failure> {
    planned(agent, |entries| {
        if !entries.iter().any(any_runs_context) {
            return Ok(false);
        }
        Ok(replace(entries, &agent_command(binary()?, agent)?))
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn made(binary: &str) -> String {
        command(Path::new(binary), &[Path::new("/home/desk")]).unwrap()
    }

    fn probe() -> Value {
        entry(&made("/opt/bin/worklog-next"))
    }

    fn theirs() -> Value {
        json!({ "hooks": [{ "type": "command", "command": "date" }] })
    }

    fn applied(root: Value, change: impl FnOnce(&mut Vec<Value>) -> bool) -> Option<Value> {
        let file = Path::new("/home/desk/.codex/hooks.json");
        edited(file, root, |entries| Ok(change(entries))).unwrap()
    }

    fn merged(root: Value, entry: Value) -> Option<Value> {
        applied(root, |entries| merge(entries, entry))
    }

    fn removed(root: Value) -> Option<Value> {
        applied(root, remove)
    }

    fn replaced(root: Value, command: &str) -> Option<Value> {
        applied(root, |entries| replace(entries, command))
    }

    #[test]
    fn the_command_spells_a_binary_under_home_with_the_variable() {
        assert_eq!(
            made("/home/desk/.local/bin/worklog-next"),
            "\"$HOME/.local/bin/worklog-next\" context 2>/dev/null || true"
        );
        assert_eq!(
            made("/opt/bin/worklog-next"),
            "\"/opt/bin/worklog-next\" context 2>/dev/null || true"
        );
        assert_eq!(
            made("/home/deskette/worklog-next"),
            "\"/home/deskette/worklog-next\" context 2>/dev/null || true"
        );
        let linked = [Path::new("/home/link"), Path::new("/real/home")];
        assert_eq!(
            command(Path::new("/real/home/bin/worklog-next"), &linked).unwrap(),
            "\"$HOME/bin/worklog-next\" context 2>/dev/null || true"
        );
    }

    #[test]
    fn a_command_made_for_any_quotable_path_is_one_the_recognition_finds() {
        let homes = [Path::new("/home/desk"), Path::new("/real/desk")];
        for binary in [
            "/opt/bin/worklog-next",
            "/atlas/my tools/worklog-next",
            "/atlas/a (b); c & d/worklog-next",
            "/atlas/it's/worklog-next",
            "/atlas/<in>|out/worklog-next",
            "/home/desk/.local/bin/worklog-next",
            "/home/desk/my tools/it's (x)/worklog-next",
            "/real/desk/bin/worklog-next",
        ] {
            let command = command(Path::new(binary), &homes).unwrap();
            assert!(runs_context(&command), "{binary}: {command}");
            let root = merged(json!({}), entry(&command)).unwrap();
            assert_eq!(merged(root.clone(), entry(&command)), None);
            assert_eq!(replaced(root.clone(), &command), None, "{binary}");
            assert_eq!(removed(root), Some(json!({})), "{binary}");
        }
    }

    #[test]
    fn a_binary_the_command_could_not_quote_or_find_again_is_refused_by_name() {
        use std::os::unix::ffi::OsStrExt as _;
        let homes = [Path::new("/home/desk")];
        for (binary, reason) in [
            ("/opt/a\"b/worklog-next", "holds '\"'"),
            ("/opt/$x/worklog-next", "holds '$'"),
            ("/opt/a\\b/worklog-next", "holds '\\\\'"),
            ("/opt/a`b/worklog-next", "holds '`'"),
            ("/home/desk/$x/worklog-next", "holds '$'"),
            (
                "/opt/bin/worklog-next-0.3.0",
                "the binary's file name must be worklog-next",
            ),
            (
                "/opt/bin/worklog",
                "the binary's file name must be worklog-next",
            ),
            (
                "/opt/bin/worklog-next (deleted)",
                "the binary's file name must be worklog-next",
            ),
            (
                "/opt/worklog-next/bin/other",
                "the binary's file name must be worklog-next",
            ),
        ] {
            let Err(Failure::Refused(why)) = command(Path::new(binary), &homes) else {
                panic!("{binary} must be refused");
            };
            assert!(why.starts_with(&format!("{binary}: ")), "{why}");
            assert!(why.contains(reason), "{why}");
        }
        let odd = Path::new(std::ffi::OsStr::from_bytes(b"/opt/\xff/worklog-next"));
        let Err(Failure::Refused(why)) = command(odd, &homes) else {
            panic!("a path that is not UTF-8 must be refused");
        };
        assert!(why.ends_with("is not UTF-8"), "{why}");
    }

    #[test]
    fn the_shell_runs_the_command_that_was_made() {
        use std::os::unix::fs::PermissionsExt as _;
        let kept = tempfile::tempdir().unwrap();
        let home = std::fs::canonicalize(kept.path()).unwrap().join("home");
        for (directory, spelled_with_home) in [
            (home.join("my tools"), true),
            (home.join("bin"), true),
            (kept.path().join("apart (a); b/it's"), false),
        ] {
            fs::create_dir_all(&directory).unwrap();
            let script = directory.join("worklog-next");
            fs::write(
                &script,
                "#!/bin/sh\n[ \"$1\" = context ] && touch \"$(dirname \"$0\")/marker\"\n",
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
            let command = command(&script, &[home.as_path()]).unwrap();
            assert_eq!(command.contains("$HOME/"), spelled_with_home, "{command}");
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .env("HOME", &home)
                .status()
                .unwrap();
            assert!(status.success(), "{command}");
            assert!(directory.join("marker").is_file(), "{command}");
        }
    }

    #[test]
    fn recognizes_the_hook_by_the_binary_s_exact_name_however_it_is_spelled() {
        for ours in [
            "worklog-next context",
            "/opt/bin/worklog-next context",
            "\"$HOME/.local/bin/worklog-next\" context 2>/dev/null || true",
            "'/opt/bin/worklog-next' context",
            "\"/opt/bin/worklog-next\" context",
            "worklog-next context;",
            "worklog-next context>/dev/null",
            "(worklog-next context)",
            "FOO=1 worklog-next context",
            "exec worklog-next context",
            "worklog-next context||true",
            "worklog-next worklog-next context",
            "exec worklog-next context 2>/dev/null || true",
            "\"/atlas/my tools/worklog-next\" context",
            "'/atlas/my tools/worklog-next' context 2>/dev/null",
            "echo worklog-next context",
        ] {
            assert!(runs_context(ours), "{ours}");
        }
        for theirs in [
            "worklog context",
            "\"$HOME/.local/bin/worklog\" context 2>/dev/null || true",
            "worklog-next-0.2 context",
            "worklog-nextish context",
            "worklog-next check",
            "worklog-next",
            "echo worklog-next; echo context",
            "mylog context",
            "/opt/worklog-next/bin/other context",
            "worklog-next check context",
            "worklog-next; context",
            "command -v worklog-next && context",
            "worklog-next | context",
            "\"/a/b worklog-next\" context",
            "\"/atlas/my tools/worklog-next context",
        ] {
            assert!(!runs_context(theirs), "{theirs}");
        }
    }

    #[test]
    fn merges_once_and_after_what_is_there() {
        let root = merged(json!({}), probe()).expect("gained");
        assert_eq!(root["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
        assert_eq!(merged(root, probe()), None);
        let both = json!({"hooks": {"SessionStart": [theirs()]}});
        let root = merged(both, probe()).expect("gained");
        assert_eq!(root["hooks"]["SessionStart"], json!([theirs(), probe()]));
    }

    #[test]
    fn another_binary_s_hook_is_not_this_one() {
        let other = json!({ "hooks": [{ "type": "command", "command": "worklog context" }] });
        let root =
            merged(json!({"hooks": {"SessionStart": [other.clone()]}}), probe()).expect("gained");
        assert_eq!(
            root["hooks"]["SessionStart"],
            json!([other.clone(), probe()])
        );
        let root = removed(root).expect("lost");
        assert_eq!(root, json!({"hooks": {"SessionStart": [other]}}));
    }

    #[test]
    fn removes_only_the_hook_and_what_merging_made() {
        let root = merged(json!({"hooks": {"Stop": []}}), probe()).expect("gained");
        let root = removed(root).expect("lost");
        assert_eq!(root, json!({"hooks": {"Stop": []}}));
        assert_eq!(removed(root), None);
        assert_eq!(removed(json!({})), None);
        let root = merged(json!({"model": "x"}), probe()).expect("gained");
        assert_eq!(removed(root), Some(json!({"model": "x"})));
        let both = json!({"hooks": {"SessionStart": [theirs(), probe()]}});
        let root = removed(both).expect("lost");
        assert_eq!(root, json!({"hooks": {"SessionStart": [theirs()]}}));
    }

    #[test]
    fn replaces_the_command_in_place_and_only_when_it_differs() {
        let stale = entry(&made("/old/worklog-next"));
        let root = json!({"hooks": {"SessionStart": [stale, theirs()], "Stop": []}});
        let root = replaced(root, &made("/opt/bin/worklog-next")).expect("replaced");
        assert_eq!(
            root,
            json!({"hooks": {"SessionStart": [probe(), theirs()], "Stop": []}})
        );
        let command = made("/opt/bin/worklog-next");
        assert_eq!(replaced(root, &command), None);
        assert_eq!(replaced(json!({}), &command), None);
        assert_eq!(replaced(json!({"hooks": {"Stop": []}}), &command), None);
    }

    #[test]
    fn a_refresh_keeps_what_a_person_added_to_the_group() {
        let old = made("/old/worklog-next");
        let new = made("/opt/bin/worklog-next");
        let group = json!({
            "matcher": "startup",
            "hooks": [
                {"type": "command", "command": "date"},
                {"type": "command", "command": old, "timeout": 5},
            ],
        });
        let root = json!({"hooks": {"SessionStart": [group]}});
        let root = replaced(root, &new).expect("replaced");
        assert_eq!(
            root["hooks"]["SessionStart"],
            json!([{
                "matcher": "startup",
                "hooks": [
                    {"type": "command", "command": "date"},
                    {"type": "command", "command": new, "timeout": 5},
                ],
            }])
        );
        assert_eq!(replaced(root, &new), None);
    }

    #[test]
    fn a_refresh_leaves_one_owned_hook_in_the_group_it_keeps() {
        let new = made("/opt/bin/worklog-next");
        let group = json!({
            "hooks": [
                {"type": "command", "command": made("/old/worklog-next")},
                {"type": "command", "command": "date"},
                {"type": "command", "command": "worklog-next context"},
                {"type": "command", "command": new},
            ],
        });
        let root = json!({"hooks": {"SessionStart": [group]}});
        let root = replaced(root, &new).expect("replaced");
        assert_eq!(
            root["hooks"]["SessionStart"],
            json!([{
                "hooks": [
                    {"type": "command", "command": new},
                    {"type": "command", "command": "date"},
                ],
            }])
        );
        assert_eq!(replaced(root, &new), None);
    }

    #[test]
    fn two_owned_groups_become_one_and_the_others_stay() {
        let old = made("/old/worklog-next");
        let new = made("/opt/bin/worklog-next");
        let by_hand = json!({"matcher": "startup", "hooks": [{"type": "command", "command": "worklog-next context"}]});
        let root = json!({"hooks": {"SessionStart": [by_hand, theirs(), entry(&old)]}});
        let root = replaced(root, &new).expect("replaced");
        assert_eq!(
            root["hooks"]["SessionStart"],
            json!([
                {"matcher": "startup", "hooks": [{"type": "command", "command": new}]},
                theirs(),
            ])
        );
        let same = json!({"hooks": {"SessionStart": [entry(&new), entry(&new)]}});
        let root = replaced(same, &new).expect("one dropped");
        assert_eq!(root["hooks"]["SessionStart"], json!([entry(&new)]));
    }

    mod on_disk {
        use std::fs;

        use super::*;
        use crate::wiring::agents;

        fn claude(home: &Path) -> Agent {
            fs::create_dir_all(home.join(".claude")).unwrap();
            let home = home.as_os_str().to_owned();
            let variable = move |name: &str| (name == "HOME").then(|| home.clone());
            agents(&variable).unwrap().remove(0)
        }

        fn binary(home: &Path) -> std::path::PathBuf {
            home.join(".local/bin/worklog-next")
        }

        fn written(agent: &Agent, planned: Result<Option<String>, Failure>) -> bool {
            let planned = planned.unwrap();
            if let Some(text) = &planned {
                agent.write_hooks(text).unwrap();
            }
            planned.is_some()
        }

        fn install(agent: &Agent, binary: &Path) -> bool {
            written(agent, installed(agent, binary))
        }

        fn refresh(agent: &Agent, binary: &Path) -> bool {
            written(agent, refreshed(agent, || Ok(binary)))
        }

        fn uninstall(agent: &Agent) -> bool {
            written(agent, uninstalled(agent))
        }

        #[test]
        fn an_absent_file_is_an_empty_object_and_the_hook_is_written_pretty_with_the_variable() {
            let kept = tempfile::tempdir().unwrap();
            let agent = claude(kept.path());
            assert!(install(&agent, &binary(kept.path())));
            let text = fs::read_to_string(agent.hooks()).unwrap();
            assert_eq!(
                text,
                concat!(
                    "{\n  \"hooks\": {\n    \"SessionStart\": [\n      {\n        \"hooks\": [\n",
                    "          {\n            \"type\": \"command\",\n",
                    "            \"command\": \"\\\"$HOME/.local/bin/worklog-next\\\" context 2>/dev/null || true\"\n",
                    "          }\n        ]\n      }\n    ]\n  }\n}\n"
                )
            );
            assert!(!install(&agent, &binary(kept.path())));
        }

        #[test]
        fn install_refresh_and_uninstall_say_whether_they_wrote() {
            let kept = tempfile::tempdir().unwrap();
            let agent = claude(kept.path());
            assert!(!refresh(&agent, &binary(kept.path())));
            assert!(!uninstall(&agent));
            assert!(!agent.hooks().exists());
            assert!(install(&agent, Path::new("/old/worklog-next")));
            assert!(refresh(&agent, &binary(kept.path())));
            assert!(!refresh(&agent, &binary(kept.path())));
            assert!(uninstall(&agent));
            assert_eq!(fs::read_to_string(agent.hooks()).unwrap(), "{}\n");
            assert!(!uninstall(&agent));
        }

        #[test]
        fn a_file_that_cannot_be_edited_is_refused_with_its_path_and_left_as_it_was() {
            for text in [
                "{ not json",
                "",
                "[]\n",
                "{\"hooks\": 1}",
                "{\"hooks\":   {\"SessionStart\": {}}}",
            ] {
                let kept = tempfile::tempdir().unwrap();
                let agent = claude(kept.path());
                fs::write(agent.hooks(), text).unwrap();
                let binary = binary(kept.path());
                for result in [
                    installed(&agent, &binary),
                    refreshed(&agent, || Ok(&binary)),
                    uninstalled(&agent),
                ] {
                    let Err(Failure::Refused(why)) = result else {
                        panic!("{text:?} must be refused");
                    };
                    assert!(
                        why.starts_with(&agent.hooks().display().to_string()),
                        "{why}"
                    );
                }
                assert_eq!(fs::read_to_string(agent.hooks()).unwrap(), text);
            }
        }

        #[test]
        fn a_binary_that_makes_no_command_is_refused_where_its_command_would_be_written() {
            let kept = tempfile::tempdir().unwrap();
            let agent = claude(kept.path());
            fs::write(agent.hooks(), "{}").unwrap();
            let others = ["/opt/bin/other", "/opt/a\"b/worklog-next"].map(Path::new);
            for other in others {
                assert!(matches!(installed(&agent, other), Err(Failure::Refused(_))));
                assert_eq!(refreshed(&agent, || Ok(other)), Ok(None));
            }
            assert!(install(&agent, &binary(kept.path())));
            for other in others {
                let Err(Failure::Refused(why)) = refreshed(&agent, || Ok(other)) else {
                    panic!("{} must be refused", other.display());
                };
                assert!(why.starts_with(&format!("{}: ", other.display())), "{why}");
            }
        }

        #[test]
        fn what_the_file_holds_beside_the_hook_survives_in_its_order() {
            let kept = tempfile::tempdir().unwrap();
            let agent = claude(kept.path());
            fs::write(
                agent.hooks(),
                "{\"zeta\": 1, \"hooks\": {\"Stop\": []}, \"alpha\": 2}",
            )
            .unwrap();
            assert!(install(&agent, &binary(kept.path())));
            let text = fs::read_to_string(agent.hooks()).unwrap();
            assert!(text.find("zeta") < text.find("hooks"));
            assert!(text.find("hooks") < text.find("alpha"));
            assert!(uninstall(&agent));
            let root: Value =
                serde_json::from_str(&fs::read_to_string(agent.hooks()).unwrap()).unwrap();
            assert_eq!(root, json!({"zeta": 1, "hooks": {"Stop": []}, "alpha": 2}));
        }
    }
}
