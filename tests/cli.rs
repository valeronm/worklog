use std::ffi::OsString;
use std::fs;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::Value;

const SUMMARY: (&str, &str) = ("summary = \"\"", "summary = \"Pin four\"");
const LANTERN_HERE: &str = "lantern — claimed at ~/projects/lantern: A thing\n  (no facts)\n";
const DESK: &str = "desk — this machine: A machine\n  (no facts)\n";
const DUE: &str = "due now — `worklog show <id>` for one, `worklog due`, run here, for all:\n";
const CLOSING: &str = "Durable facts and ideas, by name where they fit and counted where not —\n\
     `worklog facts <topic>` for a topic's facts,\n\
     `worklog ideas <topic>` for a topic's ideas,\n\
     `worklog show <topic>/<name>` for one,\n\
     `worklog followups <topic>` for open work.\n";
const NOT_SET_UP: &str = "worklog: this host is not set up; run `worklog init`\n";
const RELEASES: &str = "WORKLOG_RELEASES";
const HOME_OF_THE_TOOL: &str = "WORKLOG_HOME";
const SESSIONS: [(&str, &str); 2] = [
    ("Claude Code", "CLAUDE_CODE_SESSION_ID"),
    ("Codex", "CODEX_THREAD_ID"),
];

struct Ran {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

struct Host {
    _kept: tempfile::TempDir,
    root: PathBuf,
}

impl Host {
    fn new() -> Host {
        let kept = tempfile::tempdir().expect("a temp dir");
        let root = fs::canonicalize(kept.path()).expect("the temp dir's own path");
        fs::create_dir_all(root.join("home/projects/lantern")).unwrap();
        let host = Host { _kept: kept, root };
        fs::create_dir(host.unpublished()).unwrap();
        host
    }

    fn unpublished(&self) -> PathBuf {
        self.root.join("unpublished")
    }

    fn set_up(name: &str) -> Host {
        let host = Host::new();
        let store = host.store();
        host.created_on(name, &store)
    }

    fn sharing(&self, name: &str) -> Host {
        Host::new().created_on(name, &self.store())
    }

    fn created_on(self, name: &str, store: &Path) -> Host {
        assert_eq!(self.init(name, store), format!("{name} created\n"));
        self
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn store(&self) -> PathBuf {
        self.root.join("store")
    }

    fn init(&self, name: &str, store: &Path) -> String {
        let store = store.to_str().unwrap();
        self.ok(&["init", name, "--store", store, "--summary", "A machine"])
    }

    fn run_in(&self, dir: &Path, args: &[&str]) -> Ran {
        self.run_as(
            Command::cargo_bin("worklog").expect("the binary"),
            dir,
            args,
        )
    }

    fn run_as(&self, mut program: Command, dir: &Path, args: &[&str]) -> Ran {
        for (_, variable) in SESSIONS {
            if !program.get_envs().any(|(name, _)| name == variable) {
                program.env_remove(variable);
            }
        }
        if !program
            .get_envs()
            .any(|(name, _)| name == "XDG_CONFIG_HOME")
        {
            program.env_remove("XDG_CONFIG_HOME");
        }
        if !program.get_envs().any(|(name, _)| name == RELEASES) {
            program.env(RELEASES, self.unpublished());
        }
        if !program.get_envs().any(|(name, _)| name == HOME_OF_THE_TOOL) {
            program.env(HOME_OF_THE_TOOL, self.root.join("next"));
        }
        let output = program
            .env("HOME", self.home())
            .env_remove("XDG_STATE_HOME")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("the binary runs");
        Ran {
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
            code: output.status.code(),
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        self.run_in(&self.home(), args)
    }

    fn ok_in(&self, dir: &Path, args: &[&str]) -> String {
        let ran = self.run_in(dir, args);
        assert_eq!(ran.stderr, "", "{args:?}");
        assert_eq!(ran.code, Some(0), "{args:?}");
        ran.stdout
    }

    fn ok(&self, args: &[&str]) -> String {
        self.ok_in(&self.home(), args)
    }

    fn drafted(&self, args: &[&str], edits: &[(&str, &str)]) -> PathBuf {
        let printed = self.ok(args);
        let path = PathBuf::from(printed.strip_suffix('\n').expect("one line"));
        assert!(path.starts_with(self.root.join("next/drafts")), "{printed}");
        let mut text = fs::read_to_string(&path).expect("the draft");
        for (from, to) in edits {
            assert!(text.contains(from), "{from:?} in {text}");
            text = text.replacen(from, to, 1);
        }
        fs::write(&path, text).unwrap();
        path
    }

    fn json_in(&self, dir: &Path, args: &[&str]) -> Value {
        let printed = self.ok_in(dir, &[args, &["--json"]].concat());
        one_value(&printed).unwrap_or_else(|| panic!("{args:?} printed {printed:?}"))
    }

    fn json(&self, args: &[&str]) -> Value {
        self.json_in(&self.home(), args)
    }

    fn id(&self, target: &str) -> String {
        let shown = self.json(&["show", target]);
        shown["document"].as_str().expect("a document")[..8].to_owned()
    }

    fn row(&self, target: &str, rest: &str) -> String {
        format!("{}  {rest}\n", self.id(target))
    }

    fn claim_of(&self, topic: &str) -> String {
        let placed = self.json(&["where", topic]);
        placed[0]["document"].as_str().expect("a claim")[..8].to_owned()
    }

    fn topic(&self, name: &str) -> String {
        self.drafted(
            &["new", "topic", name],
            &[("summary = \"\"", "summary = \"A thing\"")],
        );
        self.ok(&["save", name])
    }

    fn fact(&self, kind: &str, address: &str) {
        let confirmed = ("confirmed = ", "confirmed = 2026-01-05 # ");
        self.drafted(&["new", kind, address], &[SUMMARY, confirmed]);
        self.ok(&["save", address]);
    }

    fn stocked() -> Host {
        static BUILT: OnceLock<Stocked> = OnceLock::new();
        let built = BUILT.get_or_init(|| {
            let host = Host::set_up("desk");
            host.topic("lantern");
            host.topic("atlas");
            host.fact("fact", "lantern/relay");
            host.fact("fact", "lantern/fuse");
            host.fact("idea", "lantern/dimmer");
            Stocked {
                store: host.store().to_str().unwrap().to_owned(),
                files: (snapshot(&host).into_iter())
                    .filter_map(|(path, bytes)| Some((path, bytes?)))
                    .collect(),
            }
        });
        let host = Host::new();
        for (path, bytes) in &built.files {
            let target = host.root.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, bytes).unwrap();
        }
        let config = host.root.join("next/config.toml");
        let held = fs::read_to_string(&config).expect("the config");
        assert!(held.contains(&built.store), "{held}");
        let moved = held.replace(&built.store, host.store().to_str().unwrap());
        fs::write(&config, moved).unwrap();
        host
    }

    fn followup(&self, summary: &str) -> String {
        self.followup_with(summary, &[])
    }

    fn followup_with(&self, summary: &str, rest: &[&str]) -> String {
        let line = [
            "new",
            "followup",
            "--topics",
            "lantern",
            "--summary",
            summary,
        ];
        let stored = self.ok(&[&line[..], rest].concat());
        let (id, version) = halves(&stored);
        assert!(is_hex(id, 8), "{stored}");
        assert!(is_hex(version, 12), "{stored}");
        id.to_owned()
    }

    fn due(&self, summary: &str, rest: &[&str]) -> String {
        let dated = ["--look-again", "2000-01-01", "--why", "The fuse blew"];
        self.followup_with(summary, &[&dated, rest].concat())
    }

    fn entry(&self, name: &str) -> String {
        let label = format!("2026-10-09-{name}");
        self.drafted(
            &["new", "entry", name, "--date", "2026-10-09"],
            &[SUMMARY, ("topics = []", "topics = [\"lantern\"]")],
        );
        assert_stored(&self.ok(&["save", &label]), &label);
        label
    }

    fn version_files(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        version_files(&self.store(), &mut found);
        found
    }

    fn file_of(&self, version: &str) -> PathBuf {
        (self.version_files().into_iter())
            .find(|file| file.to_str().unwrap().contains(version))
            .unwrap_or_else(|| panic!("the file of version {version}"))
    }
}

struct Stocked {
    store: String,
    files: Vec<(PathBuf, Vec<u8>)>,
}

fn opened(sections: &[&str]) -> String {
    [sections, &[CLOSING]].concat().join("\n")
}

fn one_value(printed: &str) -> Option<Value> {
    let value = printed.strip_suffix('\n')?;
    if value.trim() != value {
        return None;
    }
    serde_json::from_str(value).ok()
}

fn fill(opened: &Value, edits: &[(&str, &str)]) {
    let path = opened["path"].as_str().expect("a draft's path");
    let mut text = fs::read_to_string(path).expect("the draft");
    for (from, to) in edits {
        assert!(text.contains(from), "{from:?} in {text}");
        text = text.replacen(from, to, 1);
    }
    fs::write(path, text).unwrap();
}

fn assert_written(written: &Value, label: &str) {
    let keys: Vec<&String> = written.as_object().expect("an object").keys().collect();
    assert_eq!(keys, ["document", "label", "version"], "{written}");
    assert!(
        is_hex(written["document"].as_str().unwrap(), 32),
        "{written}"
    );
    assert_eq!(written["label"], label, "{written}");
    let hash = written["version"]
        .as_str()
        .and_then(|id| id.strip_prefix("b3-"));
    assert!(hash.is_some_and(|hash| is_hex(hash, 64)), "{written}");
}

fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn halves(line: &str) -> (&str, &str) {
    line.strip_suffix('\n')
        .and_then(|line| line.rsplit_once(' '))
        .unwrap_or_else(|| panic!("two words on one line: {line:?}"))
}

fn undated(listed: &str) -> String {
    listed
        .split_inclusive('\n')
        .map(|line| line.split_once("  ").expect("a date, then the row").1)
        .collect()
}

fn assert_stored(printed: &str, label: &str) {
    let (shown, version) = halves(printed);
    assert_eq!(shown, label, "{printed}");
    assert!(is_hex(version, 12), "{printed}");
}

fn assert_refused(ran: &Ran, code: i32, stderr: &str) {
    assert_eq!(ran.stdout, "");
    assert_eq!(ran.stderr, stderr);
    assert_eq!(ran.code, Some(code));
}

fn version_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            version_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "md") {
            found.push(path);
        }
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

#[test]
fn init_makes_the_machine_topic_and_a_second_init_is_refused() {
    let host = Host::new();
    let store = host.store();
    assert_eq!(host.init("desk", &store), "desk created\n");
    assert!(store.is_dir());
    assert_eq!(host.ok(&["topics"]), host.row("desk", "desk  A machine"));

    let again = host.run(&["init", "atlas", "--store", "elsewhere"]);
    assert_refused(&again, 1, "worklog: this host is already set up as desk\n");
    assert!(!host.home().join("elsewhere").exists());
}

#[test]
fn init_alone_with_no_terminal_is_a_usage_error_naming_the_form_with_arguments() {
    let host = Host::new();
    let before = snapshot(&host);
    let alone = host.run(&["init"]);
    assert_refused(
        &alone,
        2,
        "worklog: `init` alone asks its questions on a terminal. A host is set up by its user, \
         who gives the directory of the store; without a terminal that is `worklog init <NAME> \
         --store <DIR>`\n",
    );
    assert!(!host.root.join("next").exists());
    assert_eq!(snapshot(&host), before);
    assert_refused(&host.run(&["topics"]), 1, NOT_SET_UP);

    for half in [&["init", "desk"][..], &["init", "--store", "kept"]] {
        let ran = host.run(half);
        assert_eq!((ran.stdout.as_str(), ran.code), ("", Some(2)), "{half:?}");
        assert!(ran.stderr.starts_with("error: "), "{}", ran.stderr);
    }
    assert!(!host.home().join("kept").exists());
}

#[test]
fn init_refuses_a_directory_of_other_files_and_writes_nothing() {
    let host = Host::new();
    let project = host.home().join("projects/lantern");
    fs::write(project.join("notes.md"), "a person's own").unwrap();
    let before = snapshot(&host);

    let line = ["init", "desk", "--store", "projects/lantern"];
    let refused = host.run(&[&line[..], &["--summary", "A machine"]].concat());
    assert_refused(
        &refused,
        1,
        &format!(
            "worklog: {}: is not empty and holds no store\n",
            project.display()
        ),
    );
    assert!(!host.root.join("next").exists());
    assert_eq!(snapshot(&host), before);
    assert_refused(&host.run(&["topics"]), 1, NOT_SET_UP);
}

#[test]
fn init_makes_a_store_in_a_directory_of_dot_named_entries_alone() {
    let host = Host::new();
    let synced = host.root.join("synced");
    fs::create_dir_all(synced.join(".stfolder")).unwrap();
    assert_eq!(host.init("desk", &synced), "desk created\n");
    assert_eq!(host.ok(&["topics"]), host.row("desk", "desk  A machine"));

    let phone = Host::new();
    let ignoring = phone.root.join("ignoring");
    fs::create_dir(&ignoring).unwrap();
    fs::write(ignoring.join(".stignore"), ".DS_Store\n").unwrap();
    assert_eq!(phone.init("phone", &ignoring), "phone created\n");

    let atlas = Host::new();
    let mixed = atlas.root.join("mixed");
    fs::create_dir_all(mixed.join(".stfolder")).unwrap();
    fs::write(mixed.join("notes.txt"), "a person's own").unwrap();
    let line = ["init", "atlas", "--store", mixed.to_str().unwrap()];
    let refused = atlas.run(&[&line[..], &["--summary", "A machine"]].concat());
    assert_refused(
        &refused,
        1,
        &format!(
            "worklog: {}: is not empty and holds no store\n",
            mixed.display()
        ),
    );
    assert!(!atlas.root.join("next").exists());
}

#[test]
fn a_second_host_is_bound_to_a_store_a_sync_tool_keeps_files_beside() {
    let desk = Host::set_up("desk");
    fs::create_dir(desk.store().join(".stfolder")).unwrap();
    fs::write(desk.store().join(".stignore"), ".DS_Store\n").unwrap();
    let phone = Host::new();
    let store = desk.store();
    let bound = phone.ok(&["init", "desk", "--store", store.to_str().unwrap()]);
    assert_eq!(bound, "desk bound\n");
    assert_eq!(phone.ok(&["topics"]), desk.ok(&["topics"]));
}

#[test]
fn a_store_named_from_the_working_directory_is_kept_from_the_root() {
    let host = Host::new();
    let made = host.ok(&["init", "desk", "--store", "kept", "--summary", "A machine"]);
    assert_eq!(made, "desk created\n");
    assert!(host.home().join("kept").is_dir());
    let elsewhere = host.home().join("projects");
    assert_eq!(
        host.ok_in(&elsewhere, &["topics"]),
        host.row("desk", "desk  A machine")
    );
}

#[test]
fn a_command_before_init_is_refused() {
    let host = Host::new();
    for args in [&["topics"][..], &["new", "topic", "lantern"], &["check"]] {
        assert_refused(&host.run(args), 1, NOT_SET_UP);
    }
    assert_refused(&host.run(&["context"]), 0, NOT_SET_UP);
    let json = host.run(&["context", "--json"]);
    assert_eq!(json.stdout, "null\n");
    assert_eq!((json.stderr.as_str(), json.code), (NOT_SET_UP, Some(0)));
}

#[test]
fn a_draft_is_opened_listed_stored_and_discarded() {
    let host = Host::set_up("desk");
    let draft = host.drafted(
        &["new", "topic", "lantern"],
        &[("summary = \"\"", "summary = \"A lamp\"")],
    );
    assert_eq!(
        host.ok(&["drafts"]),
        format!("lantern {}\n", draft.display())
    );
    assert_stored(&host.ok(&["save", "lantern"]), "lantern");
    assert_eq!(host.ok(&["drafts"]), "");
    assert!(!draft.exists());

    let reopened = host.drafted(&["checkout", "lantern"], &[]);
    assert_eq!(reopened, draft);
    assert_eq!(host.ok(&["discard", "lantern"]), "");
    assert_eq!(host.ok(&["drafts"]), "");
}

#[test]
fn each_kind_of_new_document_is_stored_from_its_draft() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let summary = ("summary = \"\"", "summary = \"Pin four\"");
    let confirmed = ("confirmed = ", "confirmed = 2026-01-05 # ");

    host.drafted(&["new", "fact", "lantern/relay"], &[summary, confirmed]);
    assert_stored(&host.ok(&["save", "lantern/relay"]), "lantern/relay");
    host.drafted(&["new", "idea", "lantern/dimmer"], &[summary, confirmed]);
    assert_stored(&host.ok(&["save", "lantern/dimmer"]), "lantern/dimmer");
    let entry = ["new", "entry", "wiring", "--date", "2026-10-09"];
    host.drafted(
        &entry,
        &[summary, ("topics = []", "topics = [\"lantern\"]")],
    );
    assert_stored(
        &host.ok(&["save", "2026-10-09-wiring"]),
        "2026-10-09-wiring",
    );
    let open = host.drafted(&["new", "followup", "--topics", "lantern"], &[summary]);
    let id = open.file_stem().unwrap().to_str().unwrap();
    assert_stored(&host.ok(&["save", id]), &id[..8]);

    assert_eq!(
        host.ok(&["facts", "lantern"]),
        format!(
            "2026-01-05  {}",
            host.row("lantern/relay", "lantern/relay  Pin four")
        )
    );
    assert_eq!(
        host.ok(&["ideas"]),
        format!(
            "2026-01-05  {}",
            host.row("lantern/dimmer", "lantern/dimmer  Pin four")
        )
    );
    assert_eq!(
        host.ok(&["entries"]),
        format!(
            "2026-10-09  {}",
            host.row("2026-10-09-wiring", "2026-10-09-wiring  Pin four")
        )
    );
    assert_eq!(
        host.ok(&["followups"]),
        format!("no trigger  {}  Pin four\n", &id[..8])
    );
}

#[test]
fn a_document_is_renamed_moved_verified_ended_and_reopened() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    host.topic("atlas");
    host.drafted(
        &["new", "fact", "lantern/relay"],
        &[
            ("summary = \"\"", "summary = \"Pin four\""),
            ("confirmed = ", "confirmed = 2026-01-05 # "),
        ],
    );
    host.ok(&["save", "lantern/relay"]);
    host.drafted(
        &["new", "fact", "lantern/fuse"],
        &[("summary = \"\"", "summary = \"Two amps\"")],
    );
    host.ok(&["save", "lantern/fuse"]);

    assert_stored(&host.ok(&["verify", "lantern/relay"]), "lantern/relay");
    assert_stored(
        &host.ok(&["rename", "lantern/relay", "relay-pin"]),
        "lantern/relay-pin",
    );
    let moved = host.ok(&["move", "lantern", "atlas", "lantern/relay", "lantern/fuse"]);
    let lines: Vec<&str> = moved.split_inclusive('\n').collect();
    assert_eq!(lines.len(), 2, "{moved}");
    assert_stored(lines[0], "atlas/relay-pin");
    assert_stored(lines[1], "atlas/fuse");

    let ended = host.ok(&[
        "end",
        "atlas/fuse",
        "superseded",
        "The relay holds it",
        "--by",
        "atlas/relay-pin",
    ]);
    assert_stored(&ended, "atlas/fuse");
    let relay = host.row("atlas/relay-pin", "atlas/relay-pin  Pin four");
    assert_eq!(undated(&host.ok(&["facts"])), relay);
    assert_stored(
        &host.ok(&["reopen", "atlas/fuse", "Still wired"]),
        "atlas/fuse",
    );
    assert_eq!(
        undated(&host.ok(&["facts"])),
        host.row("atlas/fuse", "atlas/fuse  Two amps") + &relay
    );
}

#[test]
fn a_followup_is_stored_at_once_triggered_done_and_dropped() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let first = host.followup("Check the driver");
    let second = host.followup("Order a fuse");

    assert_stored(
        &host.ok(&["trigger", &first, "2026-11-01", "The part arrives"]),
        &first,
    );
    assert_stored(
        &host.ok(&["trigger", &second, "touching", "lantern"]),
        &second,
    );
    assert_stored(&host.ok(&["done", &first, "Driver holds"]), &first);
    assert_stored(&host.ok(&["drop", &second, "No longer wanted"]), &second);
    assert_eq!(host.ok(&["followups"]), "");

    let dated = host.ok(&[
        "new",
        "followup",
        "--topics",
        "lantern",
        "--summary",
        "Look at the lens",
        "--look-again",
        "2999-11-01",
        "--why",
        "The lens ships",
    ]);
    let (id, _) = halves(&dated);
    assert_eq!(
        host.ok(&["followups", "lantern"]),
        format!("by 2999-11-01  {id}  Look at the lens\n")
    );
}

#[test]
fn a_followup_stored_at_once_takes_a_body_only_beside_a_summary() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let id = host.followup_with(
        "Check the driver",
        &["--body", "It runs hot; see [[lantern]]."],
    );
    let shown = host.ok(&["show", &id]);
    assert!(
        shown.ends_with("+++\n\nIt runs hot; see [[lantern]].\n"),
        "{shown}"
    );

    let bare = host.run(&[
        "new",
        "followup",
        "--topics",
        "lantern",
        "--body",
        "The rest.",
    ]);
    assert_eq!(bare.stdout, "");
    assert_eq!(bare.code, Some(2), "{}", bare.stderr);
}

#[test]
fn a_new_summary_past_the_limit_is_refused() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let long = "a".repeat(101);
    assert_refused(
        &host.run(&["new", "followup", "--topics", "lantern", "--summary", &long]),
        1,
        "worklog: new followup: `summary` is 101 characters long; at most 100 are stored\n",
    );
}

#[test]
fn a_claim_with_no_directory_takes_the_working_directory() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let project = host.home().join("projects/lantern");

    let claimed = host.ok_in(&project, &["claim", "lantern"]);
    assert_stored(&claimed, "lantern at ~/projects/lantern");
    let id = host.claim_of("lantern");
    assert_eq!(
        host.ok(&["where"]),
        format!("lantern  ~/projects/lantern  {id}\n")
    );
    assert_eq!(
        host.ok_in(&project, &["context"]),
        opened(&[LANTERN_HERE, DESK])
    );

    let unclaimed = host.ok_in(
        &host.home().join("projects"),
        &["unclaim", "lantern", "lantern"],
    );
    assert_stored(&unclaimed, "lantern at ~/projects/lantern");
    assert_eq!(host.ok(&["where"]), "");

    let again = host.ok(&["claim", "lantern", "projects/lantern"]);
    assert_stored(&again, "lantern at ~/projects/lantern");
    let again = host.claim_of("lantern");
    assert_ne!(again, id);
    assert_eq!(
        host.ok(&["where"]),
        format!("lantern  ~/projects/lantern  {again}\n")
    );
}

#[test]
fn a_session_opens_on_what_is_due_then_the_topics_of_its_directory() {
    let host = Host::stocked();
    let project = host.home().join("projects/lantern");
    host.ok_in(&project, &["claim", "lantern"]);
    host.followup("Check the driver");
    let due = host.due("Order a fuse", &[]);
    host.ok(&["new", "topic", "phone"]);

    let text = host.ok_in(&project, &["context"]);
    assert_eq!(
        text,
        opened(&[
            &format!("{DUE}due 2000-01-01  {due}  Order a fuse\n"),
            "1 draft left open on this machine — `worklog drafts`\n",
            "1 open followup not due — `worklog followups <topic>`\n",
            "lantern — claimed at ~/projects/lantern: A thing\n\
             \x20 fuse, relay\n\
             Ideas — unbuilt, kept with their settled design; opened like a fact:\n\
             \x20 dimmer\n",
            DESK,
        ])
    );
    assert_eq!(
        host.ok(&["show", "lantern/fuse"])
            .matches("Pin four")
            .count(),
        1
    );

    let whole = host.json_in(&project, &["context"]);
    assert_eq!(whole["topics"][0]["facts"][0]["label"], "lantern/fuse");
    assert_eq!(whole["due"][0]["row"]["summary"], "Order a fuse", "{whole}");
    assert_eq!(whole["draft_count"], 1, "{whole}");
}

#[test]
fn what_is_due_in_a_directory_is_listed_as_its_session_opens_on_it() {
    let host = Host::stocked();
    let project = host.home().join("projects/lantern");
    host.ok_in(&project, &["claim", "lantern"]);
    assert_eq!(host.ok_in(&project, &["due"]), "");
    assert_eq!(host.json_in(&project, &["due"]), Value::Array(Vec::new()));

    host.followup("Check the driver");
    let fuse = host.due("Order a fuse", &[]);
    let lens = host.followup("Fit the lens");
    host.ok(&["trigger", &lens, "touching", "lantern"]);
    let listed = format!(
        "due 2000-01-01  {fuse}  Order a fuse\n\
         touching lantern  {lens}  Fit the lens\n"
    );
    assert_eq!(host.ok_in(&project, &["due"]), listed);
    assert_eq!(host.ok(&["due", "projects/lantern"]), listed);
    assert_eq!(host.ok(&["due"]), "");
    let opened = host.ok_in(&project, &["context"]);
    assert!(opened.starts_with(&format!("{DUE}{listed}\n")), "{opened}");

    let due = host.json_in(&project, &["due"]);
    assert_eq!(due.as_array().map(Vec::len), Some(2), "{due}");
    assert_eq!(due, host.json_in(&project, &["context"])["due"]);
    assert_refused(&Host::new().run(&["due"]), 1, NOT_SET_UP);
}

#[test]
fn a_session_opens_on_what_is_due_under_a_part_of_its_topic() {
    let host = Host::stocked();
    let project = host.home().join("projects/lantern");
    host.ok_in(&project, &["claim", "lantern"]);
    let edges = "summary = \"A part\"\npart_of = [\"lantern\"]";
    host.drafted(&["new", "topic", "phone"], &[("summary = \"\"", edges)]);
    host.ok(&["save", "phone"]);
    let stored = host.ok(&[
        "new",
        "followup",
        "--topics",
        "phone",
        "--summary",
        "Charge the phone",
        "--look-again",
        "2000-01-01",
        "--why",
        "The cell ran flat",
    ]);
    let (id, _) = halves(&stored);

    let listed = format!("due 2000-01-01  {id}  Charge the phone\n");
    assert_eq!(host.ok_in(&project, &["due"]), listed);
    let opened = host.ok_in(&project, &["context"]);
    assert!(opened.starts_with(&format!("{DUE}{listed}\n")), "{opened}");
    assert!(!opened.contains("not due"), "{opened}");
    let whole = host.json_in(&project, &["context"]);
    assert_eq!(whole["due"][0]["row"]["summary"], "Charge the phone");
    assert_eq!(whole["open_count"], 0, "{whole}");
    assert_eq!(whole["topics"][0]["parts"][0]["open_count"], 1, "{whole}");
}

#[test]
fn a_session_over_an_empty_store_opens_on_the_machine_topic_alone() {
    let host = Host::set_up("desk");
    assert_eq!(host.ok(&["context"]), opened(&[DESK]));
    assert_eq!(host.ok_in(&host.root, &["context"]), opened(&[DESK]));
}

#[test]
fn a_directory_is_claimed_and_unclaimed_as_the_file_system_resolves_it() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let project = host.home().join("projects/lantern");
    std::os::unix::fs::symlink(&project, host.home().join("link")).unwrap();

    host.ok(&["claim", "lantern", "link"]);
    let placed = || {
        format!(
            "lantern  ~/projects/lantern  {}\n",
            host.claim_of("lantern")
        )
    };
    assert_eq!(host.ok(&["where"]), placed());
    assert_eq!(host.ok(&["context", "link"]), opened(&[LANTERN_HERE, DESK]));
    host.ok(&["unclaim", "lantern", "link"]);
    assert_eq!(host.ok(&["where"]), "");

    host.ok(&["claim", "lantern", "link/../projects/lantern"]);
    fs::remove_file(host.home().join("link")).unwrap();
    fs::remove_dir(&project).unwrap();
    assert_eq!(host.ok(&["where"]), placed());
    host.ok(&["unclaim", "lantern", "projects/lantern"]);
    assert_eq!(host.ok(&["where"]), "");
}

#[test]
fn a_session_in_a_linked_directory_is_opened_by_the_claim_on_the_directory_itself() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let project = host.home().join("projects/lantern");
    fs::create_dir(project.join("case")).unwrap();
    std::os::unix::fs::symlink(&project, host.root.join("link")).unwrap();
    host.ok_in(&project, &["claim", "lantern"]);

    let here = opened(&[LANTERN_HERE, DESK]);
    let link = host.root.join("link");
    assert_eq!(host.ok(&["context", link.to_str().unwrap()]), here);
    assert_eq!(host.ok_in(&link.join("case"), &["context"]), here);
    assert_eq!(
        host.ok_in(&host.root, &["context", "link/case/later"]),
        here
    );
    assert_eq!(host.ok_in(&host.root, &["context"]), opened(&[DESK]));
}

#[test]
fn a_home_that_is_a_link_still_holds_what_is_under_it_in_the_home_form() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let real = host.root.join("real");
    fs::rename(host.home(), &real).unwrap();
    std::os::unix::fs::symlink(&real, host.home()).unwrap();

    host.ok_in(&real.join("projects/lantern"), &["claim", "lantern"]);
    let placed = format!(
        "lantern  ~/projects/lantern  {}\n",
        host.claim_of("lantern")
    );
    assert_eq!(host.ok(&["where"]), placed);
    assert_eq!(
        host.ok(&["context", "projects/lantern"]),
        opened(&[LANTERN_HERE, DESK])
    );
    let home = host.home();
    let through = home.join("projects/lantern");
    assert_stored(
        &host.ok(&["unclaim", "lantern", through.to_str().unwrap()]),
        "lantern at ~/projects/lantern",
    );
    assert_eq!(host.ok(&["where"]), "");
}

#[test]
fn a_store_that_is_gone_is_refused_and_not_read_as_empty() {
    let host = Host::set_up("desk");
    let moved = host.root.join("moved");
    fs::rename(host.store(), &moved).unwrap();
    let gone = format!(
        "worklog: {}: this host's store is not a directory\n",
        host.store().display()
    );
    for args in [&["topics"][..], &["new", "topic", "lantern"], &["context"]] {
        assert_refused(&host.run(args), 1, &gone);
    }
    assert!(!host.store().exists());

    fs::rename(&moved, host.store()).unwrap();
    assert_eq!(host.ok(&["topics"]), host.row("desk", "desk  A machine"));
}

#[test]
fn a_fork_is_resolved_through_a_draft() {
    let desk = Host::set_up("desk");
    desk.topic("lantern");
    let phone = Host::new();
    copy_tree(&desk.store(), &phone.store());
    assert_eq!(
        phone.ok(&["init", "desk", "--store", phone.store().to_str().unwrap()]),
        "desk bound\n"
    );

    desk.ok(&["rename", "lantern", "lamp"]);
    phone.ok(&["rename", "lantern", "torch"]);
    copy_tree(&phone.store(), &desk.store());
    let id = desk.id("lamp");
    let forked = desk.ok(&["forks"]);
    let lines: Vec<&str> = forked.lines().collect();
    assert_eq!(lines.len(), 3, "{forked}");
    let shown = lines[0]
        .strip_prefix(&format!("{id}  "))
        .and_then(|rest| rest.strip_suffix("  A thing  forked"))
        .unwrap_or_else(|| panic!("one forked topic: {forked:?}"));
    assert!(["lamp", "torch"].contains(&shown), "{shown}");
    for head in &lines[1..] {
        let fields: Vec<&str> = head.split("  ").collect();
        assert_eq!(fields.len(), 5, "{head:?}");
        assert_eq!((fields[0], fields[1], fields[4]), ("", "", "desk"));
        assert!(is_hex(fields[2], 12), "{head:?}");
    }

    let both = desk.run(&["show", "lamp"]);
    assert_eq!(
        both.stderr,
        format!("worklog: {shown}: forked: resolve it\n")
    );
    assert_eq!(both.stdout.matches("==== ").count(), 2, "{}", both.stdout);
    assert!(both.stdout.contains("name = \"lamp\""), "{}", both.stdout);
    assert!(both.stdout.contains("name = \"torch\""), "{}", both.stdout);
    assert_eq!(both.code, Some(0));

    desk.drafted(&["resolve", "lamp"], &[]);
    assert_stored(&desk.ok(&["save", "torch"]), shown);
    assert_eq!(desk.ok(&["forks"]), "");
    assert_eq!(
        desk.ok(&["topics"]),
        desk.row("desk", "desk  A machine") + &format!("{id}  {shown}  A thing\n")
    );
    assert!(!desk.ok(&["show", "lamp"]).contains("===="));
}

#[test]
fn a_refusal_is_on_stderr_alone_with_exit_one() {
    let host = Host::set_up("desk");
    assert_refused(
        &host.run(&["save", "lantern"]),
        1,
        "worklog: lantern: no draft\n",
    );
    assert_refused(
        &host.run(&["show", "lantern"]),
        1,
        "worklog: lantern: names no document\n",
    );
}

#[test]
fn a_usage_error_exits_two() {
    let host = Host::set_up("desk");
    let named = host.run(&["new", "topic", "Not_A_Name"]);
    assert_eq!(named.stdout, "");
    assert!(named.stderr.starts_with("worklog: "), "{}", named.stderr);
    assert_eq!(named.code, Some(2));

    for args in [
        &["new", "entry", "wiring", "--date", "tomorrow"][..],
        &["polish"],
        &["rename", "desk"],
        &["entries", "-n", "many"],
        &[],
    ] {
        let ran = host.run(args);
        assert_eq!(ran.stdout, "", "{args:?}");
        assert_ne!(ran.stderr, "", "{args:?}");
        assert_eq!(ran.code, Some(2), "{args:?}");
    }
}

#[test]
fn a_bad_trigger_date_is_a_usage_error() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let id = host.followup("Check the driver");
    let ran = host.run(&["trigger", &id, "someday", "The part arrives"]);
    assert_refused(&ran, 2, "worklog: `someday` is not a date (YYYY-MM-DD)\n");
}

#[test]
fn help_and_the_version_go_to_stdout_with_exit_zero() {
    let host = Host::new();
    let help = host.ok(&["--help"]);
    assert!(help.contains("Usage: worklog"), "{help}");
    for command in ["init", "new", "checkout", "trigger", "context", "check"] {
        assert!(
            help.contains(&format!("\n  {command} ")),
            "{command}: {help}"
        );
    }
    assert!(host.ok(&["--version"]).starts_with("worklog "));
    assert!(host.ok(&["new", "--help"]).contains("followup"));
}

#[test]
fn json_is_accepted_on_either_side_of_the_command() {
    let host = Host::set_up("desk");
    assert_eq!(host.run(&["--json", "topics"]).code, Some(0));
    assert_eq!(host.run(&["topics", "--json"]).code, Some(0));

    let before = host.ok(&["--json", "topics"]);
    assert_eq!(host.ok(&["topics", "--json"]), before);
    let listed = one_value(&before).unwrap_or_else(|| panic!("one value: {before:?}"));
    assert_eq!(listed[0]["row"]["label"], "desk", "{before}");
    assert_eq!(listed.as_array().map(Vec::len), Some(1), "{before}");
    assert_ne!(host.ok(&["topics"]), before);
}

#[test]
fn a_command_that_opens_lists_or_drops_a_draft_prints_one_json_value() {
    let host = Host::new();
    let store = host.store();
    let bound = host.json(&[
        "init",
        "desk",
        "--store",
        store.to_str().unwrap(),
        "--summary",
        "A machine",
    ]);
    assert_eq!(bound["label"], "desk", "{bound}");
    assert_eq!(bound["created"], true, "{bound}");
    assert_eq!(bound["written"], serde_json::json!([]), "{bound}");
    assert!(is_hex(bound["document"].as_str().unwrap(), 32), "{bound}");

    let opened = host.json(&["new", "topic", "lantern"]);
    let keys: Vec<&String> = opened.as_object().expect("an object").keys().collect();
    assert_eq!(keys, ["document", "path"], "{opened}");
    fill(&opened, &[SUMMARY]);
    let drafts = host.json(&["drafts"]);
    assert_eq!(drafts[0]["path"], opened["path"], "{drafts}");
    assert_eq!(drafts[0]["label"], "lantern", "{drafts}");
    assert!(host.json(&["diff", "lantern"])["after"].is_string());
    let saved = host.json(&["save", "lantern"]);
    assert_written(&saved, "lantern");
    assert_eq!(saved["document"], opened["document"]);
    assert_eq!(host.json(&["drafts"]), Value::Array(Vec::new()));

    assert_eq!(host.json(&["checkout", "lantern"]), opened);
    assert_eq!(host.json(&["discard", "lantern"]), Value::Null);

    fill(&host.json(&["new", "fact", "lantern/relay"]), &[SUMMARY]);
    assert_written(&host.json(&["save", "lantern/relay"]), "lantern/relay");
    fill(&host.json(&["new", "idea", "lantern/dimmer"]), &[SUMMARY]);
    assert_written(&host.json(&["save", "lantern/dimmer"]), "lantern/dimmer");
    let entry = host.json(&["new", "entry", "wiring", "--date", "2026-10-09"]);
    fill(
        &entry,
        &[SUMMARY, ("topics = []", "topics = [\"lantern\"]")],
    );
    assert_written(
        &host.json(&["save", "2026-10-09-wiring"]),
        "2026-10-09-wiring",
    );

    let drafted = host.json(&["new", "followup", "--topics", "lantern"]);
    let keys: Vec<&String> = drafted.as_object().expect("an object").keys().collect();
    assert_eq!(keys, ["document", "path"], "{drafted}");
    fill(&drafted, &[SUMMARY]);
    let id = &drafted["document"].as_str().unwrap()[..8];
    assert_written(&host.json(&["save", id]), id);
    let stored = host.json(&[
        "new",
        "followup",
        "--topics",
        "lantern",
        "--summary",
        "Check the driver",
    ]);
    let label = stored["label"].as_str().expect("a label").to_owned();
    assert!(is_hex(&label, 8), "{stored}");
    assert_written(&stored, &label);
}

#[test]
fn a_command_that_changes_a_document_prints_its_stored_version_as_json() {
    let host = Host::stocked();
    let (first, second) = (host.followup("Check the driver"), host.followup("s"));

    assert_written(&host.json(&["verify", "lantern/relay"]), "lantern/relay");
    assert_written(
        &host.json(&["rename", "lantern/relay", "relay-pin"]),
        "lantern/relay-pin",
    );
    let moved = host.json(&[
        "move",
        "lantern",
        "atlas",
        "lantern/relay-pin",
        "lantern/fuse",
    ]);
    assert_eq!(moved.as_array().map(Vec::len), Some(2), "{moved}");
    assert_written(&moved[0], "atlas/relay-pin");
    assert_written(&moved[1], "atlas/fuse");
    assert_written(
        &host.json(&["end", "atlas/fuse", "false", "It was three amps"]),
        "atlas/fuse",
    );
    assert_written(
        &host.json(&["reopen", "atlas/fuse", "Still wired"]),
        "atlas/fuse",
    );
    assert_written(
        &host.json(&["trigger", &first, "2026-11-01", "The part arrives"]),
        &first,
    );
    assert_written(
        &host.json(&["trigger", &second, "touching", "lantern"]),
        &second,
    );
    assert_written(&host.json(&["done", &first]), &first);
    assert_written(&host.json(&["drop", &second, "No longer wanted"]), &second);

    let project = host.home().join("projects/lantern");
    let claim = "lantern at ~/projects/lantern";
    assert_written(&host.json_in(&project, &["claim", "lantern"]), claim);
    assert_written(&host.json_in(&project, &["unclaim", "lantern"]), claim);
    let anywhere = host.json(&["claim", "lantern", "--anywhere"]);
    assert_written(&anywhere, "lantern anywhere");
}

#[test]
fn a_command_that_reads_the_store_prints_one_json_value() {
    let host = Host::stocked();
    let followup = host.followup("Check the driver");
    let project = host.home().join("projects/lantern");
    host.ok_in(&project, &["claim", "lantern"]);

    let placed = host.json(&["where"]);
    assert_eq!(placed[0]["directory"], "~/projects/lantern", "{placed}");
    assert_eq!(host.json(&["where", "atlas"]), Value::Array(Vec::new()));
    let context = host.json_in(&project, &["context"]);
    assert_eq!(context["topics"][0]["via"]["type"], "claim", "{context}");
    assert_eq!(context["topics"][1]["via"]["type"], "machine", "{context}");

    let shown = host.json(&["show", "lantern"]);
    assert_eq!(shown["type"], "document", "{shown}");
    let version = shown["heads"][0]["version"].as_str().unwrap();
    assert_eq!(host.json(&["show", version])["type"], "version");
    assert_eq!(host.json(&["diff", version])["label"], "lantern");
    let history = host.json(&["history", "lantern/relay"]);
    assert_eq!(history["versions"].as_array().map(Vec::len), Some(1));
    for (args, count) in [
        (&["log", "2"][..], 2),
        (&["log", "--machine", "desk"], 8),
        (&["search", "pin"], 3),
        (
            &["search", "^pin", "--regex", "--topic", "atlas", "--ended"],
            0,
        ),
        (&["topics"], 3),
        (&["topics", "--ended"], 3),
        (&["facts"], 2),
        (&["facts", "atlas"], 0),
        (&["ideas"], 1),
        (&["entries"], 0),
        (&["entries", "-n", "1"], 0),
        (&["followups"], 1),
        (&["followups", "atlas"], 0),
        (&["forks"], 0),
    ] {
        let listed = host.json(args);
        assert_eq!(listed.as_array().map(Vec::len), Some(count), "{args:?}");
    }

    let clean = host.json(&["check"]);
    assert_eq!(clean["problems"], Value::Array(Vec::new()), "{clean}");
    let stored = host.ok(&["history", &followup]);
    fs::write(host.file_of(&stored[..12]), "not a version\n").unwrap();
    let ran = host.run(&["check", "--json"]);
    assert_eq!((ran.stderr.as_str(), ran.code), ("", Some(1)));
    let found = one_value(&ran.stdout).unwrap_or_else(|| panic!("one value: {}", ran.stdout));
    assert_eq!(found["problems"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_fork_is_listed_and_resolved_under_json() {
    let desk = Host::set_up("desk");
    desk.topic("lantern");
    let phone = Host::new();
    copy_tree(&desk.store(), &phone.store());
    phone.ok(&["init", "desk", "--store", phone.store().to_str().unwrap()]);
    desk.ok(&["rename", "lantern", "lamp"]);
    phone.ok(&["rename", "lantern", "torch"]);
    copy_tree(&phone.store(), &desk.store());

    let forks = desk.json(&["forks"]);
    assert_eq!(forks.as_array().map(Vec::len), Some(1), "{forks}");
    assert_eq!(forks[0]["row"]["forked"], true, "{forks}");
    assert_eq!(forks[0]["heads"].as_array().map(Vec::len), Some(2));
    let opened = desk.json(&["resolve", "lamp"]);
    assert_eq!(opened["document"], forks[0]["document"], "{opened}");
    assert!(Path::new(opened["path"].as_str().unwrap()).is_file());
}

#[test]
fn a_failure_under_json_is_text_on_stderr_and_nothing_on_stdout() {
    let bare = Host::new();
    assert_refused(&bare.run(&["topics", "--json"]), 1, NOT_SET_UP);

    let host = Host::set_up("desk");
    assert_refused(
        &host.run(&["--json", "save", "lantern"]),
        1,
        "worklog: lantern: no draft\n",
    );
    assert_refused(
        &host.run(&["show", "lantern", "--json"]),
        1,
        "worklog: lantern: names no document\n",
    );
    assert_refused(
        &host.run(&["new", "entry", "wiring", "--date", "soon", "--json"]),
        2,
        "worklog: `soon` is not a date (YYYY-MM-DD)\n",
    );
    let unknown = host.run(&["--json", "polish"]);
    assert_eq!((unknown.stdout.as_str(), unknown.code), ("", Some(2)));
}

#[test]
fn two_homes_on_one_store_are_two_machines_that_see_each_other() {
    let desk = Host::set_up("desk");
    let phone = Host::new();
    assert_eq!(phone.init("phone", &desk.store()), "phone created\n");

    desk.topic("lantern");
    phone.topic("atlas");

    let listed = [
        desk.row("atlas", "atlas  A thing"),
        desk.row("desk", "desk  A machine"),
        desk.row("lantern", "lantern  A thing"),
        desk.row("phone", "phone  A machine"),
    ]
    .concat();
    assert_eq!(desk.ok(&["topics"]), listed);
    assert_eq!(phone.ok(&["topics"]), listed);

    assert_stored(&phone.ok(&["rename", "lantern", "lamp"]), "lamp");
    assert!(desk.ok(&["show", "lantern"]).contains("name = \"lamp\""));

    let by_desk = desk.ok(&["log", "--machine", "desk"]);
    let by_phone = desk.ok(&["log", "--machine", "phone"]);
    assert_eq!(by_desk.lines().count(), 2, "{by_desk}");
    assert_eq!(by_phone.lines().count(), 3, "{by_phone}");
    assert!(by_desk.lines().all(|line| line.contains("  desk  ")));
    assert!(by_phone.lines().all(|line| line.contains("  phone  ")));
}

#[test]
fn check_exits_one_on_a_problem_and_zero_without() {
    let host = Host::set_up("desk");
    assert_eq!(host.ok(&["check"]), "0 problems, 0 notices, 0 forks\n");

    let files = host.version_files();
    assert_eq!(files.len(), 1);
    fs::write(&files[0], "not a version\n").unwrap();

    let ran = host.run(&["check"]);
    assert_eq!(ran.stderr, "");
    assert_eq!(ran.code, Some(1));
    let lines: Vec<&str> = ran.stdout.lines().collect();
    assert!(lines.len() >= 2, "{}", ran.stdout);
    assert!(lines.last().unwrap().ends_with(" 0 notices, 0 forks"));
    assert!(!lines.last().unwrap().starts_with("0 problems"));
}

#[test]
fn a_link_written_before_its_target_resolves_once_the_target_is_stored() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    let draft = host.drafted(
        &["new", "topic", "phone"],
        &[("summary = \"\"", "summary = \"A thing\"")],
    );
    let mut text = fs::read_to_string(&draft).unwrap();
    text.push_str("See [[lantern/relay-pin]] and [[atlas]].\n");
    fs::write(&draft, text).unwrap();
    host.ok(&["save", "phone"]);

    let targets = |host: &Host| -> Vec<Value> {
        let shown = host.json(&["show", "phone"]);
        let links = shown["heads"][0]["links"].as_array().expect("links");
        links.iter().map(|link| link["target"].clone()).collect()
    };
    assert_eq!(targets(&host), [Value::Null, Value::Null]);
    let before = host.ok(&["check"]);
    assert!(
        before.contains("link atlas: resolves to nothing")
            && before.contains("link lantern/relay-pin: resolves to nothing")
            && before.ends_with("0 problems, 2 notices, 0 forks\n"),
        "{before}"
    );

    host.fact("fact", "lantern/relay-pin");
    host.topic("atlas");
    let labels: Vec<Value> = (targets(&host).iter())
        .map(|target| target["label"].clone())
        .collect();
    assert_eq!(labels, ["atlas", "lantern/relay-pin"]);
    let after = host.ok(&["check"]);
    assert!(!after.contains("resolves to nothing"), "{after}");
    assert_eq!(after, "0 problems, 0 notices, 0 forks\n");
}

#[test]
fn a_link_to_a_document_that_ended_is_shown_as_ended_in_json_and_on_the_page() {
    let host = Host::stocked();
    let draft = host.drafted(
        &["new", "topic", "phone"],
        &[("summary = \"\"", "summary = \"A thing\"")],
    );
    let mut text = fs::read_to_string(&draft).unwrap();
    text.push_str("See [[lantern/fuse]] and [[lantern/relay]].\n");
    fs::write(&draft, text).unwrap();
    host.ok(&["save", "phone"]);

    let links = |host: &Host| -> Vec<(Value, Value)> {
        let shown = host.json(&["show", "phone"]);
        let links = shown["heads"][0]["links"].as_array().expect("links");
        (links.iter())
            .map(|link| {
                let target = &link["target"];
                (target["label"].clone(), target["ended"].clone())
            })
            .collect()
    };
    let live = [
        (Value::from("lantern/fuse"), Value::Null),
        (Value::from("lantern/relay"), Value::Null),
    ];
    assert_eq!(links(&host), live);
    let before = host.ok(&["show", "phone"]);

    host.ok(&["end", "lantern/fuse", "superseded", "The relay holds it"]);
    assert_eq!(
        links(&host),
        [
            (Value::from("lantern/fuse"), Value::from("superseded")),
            live[1].clone(),
        ]
    );
    assert_eq!(host.ok(&["show", "phone"]), before);

    let id = |target: &str| {
        let shown = host.json(&["show", target]);
        shown["document"].as_str().expect("a document").to_owned()
    };
    let (fuse, relay) = (id("lantern/fuse"), id("lantern/relay"));
    let mut serving = host.serve(&[]);
    let page = serving.page("/doc/phone");
    let body = format!(
        "<p>See <a class=\"ended\" href=\"/doc/{fuse}\" title=\"ended superseded\">\
         lantern/fuse</a> and <a href=\"/doc/{relay}\">lantern/relay</a>.</p>"
    );
    assert!(page.contains(&body), "{page}");
    let target = serving.page(&format!("/doc/{fuse}"));
    assert!(target.contains("<h1>lantern/fuse</h1>"), "{target}");
    serving.running();
}

#[test]
fn the_topics_of_a_topic_are_its_parts_with_what_is_filed_on_each() {
    let host = Host::stocked();
    let part = |name: &str| {
        let edges = "summary = \"A part\"\npart_of = [\"lantern\"]";
        host.drafted(&["new", "topic", name], &[("summary = \"\"", edges)]);
        host.ok(&["save", name]);
    };
    part("phone");
    part("compass");
    host.fact("fact", "phone/dial");
    host.fact("idea", "phone/strap");
    host.ok(&["end", "compass", "retired", "Never built"]);

    let phone = host.row("phone", "phone  A part  part of lantern  (1 fact, 1 idea)");
    assert_eq!(host.ok(&["topics", "lantern"]), phone);
    let both = host.run(&["topics", "lantern", "--ended"]);
    assert_eq!((both.stdout.as_str(), both.code), ("", Some(2)));
    assert!(both.stderr.contains("--ended"), "{}", both.stderr);
    assert_eq!(host.ok(&["topics", "phone"]), "");
    let listed = host.ok(&["topics"]);
    assert!(listed.contains(&phone), "{listed}");
    assert!(
        listed.contains(&host.row("lantern", "lantern  A thing  (2 facts, 1 idea)")),
        "{listed}"
    );

    let parts = host.json(&["topics", "lantern"]);
    assert_eq!(labels(&parts), ["phone"]);
    let lantern = &host.json(&["show", "lantern"])["document"];
    assert_eq!(
        parts[0]["part_of"],
        serde_json::json!([{"document": lantern, "label": "lantern", "ended": null}])
    );
    assert_eq!(
        (
            &parts[0]["fact_count"],
            &parts[0]["idea_count"],
            &parts[0]["open_count"]
        ),
        (
            &serde_json::json!(1),
            &serde_json::json!(1),
            &serde_json::json!(0)
        )
    );

    assert_refused(
        &host.run(&["topics", "nowhere"]),
        1,
        "worklog: nowhere: names no document\n",
    );
    assert_refused(
        &host.run(&["topics", "lantern/relay"]),
        1,
        "worklog: lantern/relay: is not a topic\n",
    );
}

#[test]
fn a_listing_is_a_line_a_document_from_its_short_id() {
    let host = Host::stocked();
    let fact = |address: &str| {
        let rest = format!("{address}  Pin four");
        format!("2026-01-05  {}", host.row(address, &rest))
    };
    let (dimmer, fuse, relay) = (
        fact("lantern/dimmer"),
        fact("lantern/fuse"),
        fact("lantern/relay"),
    );

    assert_eq!(
        host.ok(&["topics"]),
        [
            host.row("atlas", "atlas  A thing"),
            host.row("desk", "desk  A machine"),
            host.row("lantern", "lantern  A thing  (2 facts, 1 idea)"),
        ]
        .concat()
    );
    assert_eq!(host.ok(&["facts"]), format!("{fuse}{relay}"));
    assert_eq!(host.ok(&["facts", "atlas"]), "");
    assert_eq!(host.ok(&["ideas", "lantern"]), dimmer);

    host.ok(&["end", "lantern/fuse", "false", "It was three amps"]);
    assert_eq!(host.ok(&["facts"]), relay);
    let ended = host.ok(&["facts", "--ended"]);
    let line = ended.lines().next().expect("the ended fact");
    assert!(
        line.ends_with("  lantern/fuse  Pin four  ended false"),
        "{ended}"
    );

    host.drafted(
        &["new", "entry", "wiring", "--date", "2026-10-09"],
        &[SUMMARY, ("topics = []", "topics = [\"lantern\"]")],
    );
    host.ok(&["save", "2026-10-09-wiring"]);
    let wired = format!(
        "2026-10-09  {}",
        host.row("2026-10-09-wiring", "2026-10-09-wiring  Pin four")
    );
    assert_eq!(host.ok(&["entries", "lantern"]), wired);
    assert_eq!(host.ok(&["entries", "atlas"]), "");

    let waiting = host.followup("Order a fuse");
    let open = host.followup("Check the driver");
    host.ok(&["trigger", &waiting, "touching", "lantern"]);
    let late = host.followup("Fit the lens");
    host.ok(&["trigger", &late, "2020-01-05", "The lens shipped"]);
    let later = host.followup("Paint the case");
    host.ok(&["trigger", &later, "2999-01-05", "The paint dries"]);
    assert_eq!(
        host.ok(&["followups"]),
        format!(
            "due 2020-01-05  {late}  Fit the lens\n\
             by 2999-01-05  {later}  Paint the case\n\
             touching lantern  {waiting}  Order a fuse\n\
             no trigger  {open}  Check the driver\n"
        )
    );

    let project = host.home().join("projects/lantern");
    let here = host.ok_in(&project, &["claim", "lantern"]);
    assert_stored(&here, "lantern at ~/projects/lantern");
    let anywhere = host.ok(&["claim", "atlas", "--anywhere"]);
    assert_stored(&anywhere, "atlas anywhere");
    assert_eq!(
        host.ok(&["where"]),
        format!(
            "atlas  (anywhere)  {}\nlantern  ~/projects/lantern  {}\n",
            host.claim_of("atlas"),
            host.claim_of("lantern")
        )
    );

    assert_eq!(
        host.ok(&["search", "relay"]),
        host.row("lantern/relay", "lantern/relay  Pin four")
    );
    let found = host.ok(&["search", "four", "--topic", "lantern"]);
    let hits: Vec<&str> = found.lines().collect();
    assert_eq!(hits.len(), 6, "{found}");
    assert!(hits[0].ends_with("  lantern/dimmer  Pin four"), "{found}");
    assert_eq!(hits[1], "    Pin four", "{found}");
    assert!(
        hits[4].ends_with("  2026-10-09-wiring  Pin four"),
        "{found}"
    );
    assert_eq!(host.ok(&["search", "no-such-word"]), "");
}

#[test]
fn a_document_is_read_as_its_text_its_versions_and_their_difference() {
    let host = Host::stocked();
    let live = host.ok(&["show", "lantern/relay"]);
    assert!(live.starts_with("+++\n"), "{live}");
    assert!(live.contains("summary = \"Pin four\""), "{live}");

    host.ok(&["rename", "lantern/relay", "relay-pin"]);
    let history = host.ok(&["history", "lantern/relay-pin"]);
    let versions: Vec<Vec<&str>> = history
        .lines()
        .map(|line| line.split("  ").collect())
        .collect();
    assert_eq!(versions.len(), 2, "{history}");
    assert_eq!(versions[0][2..], ["desk", "rename", "head"], "{history}");
    assert_eq!(versions[1][2..], ["desk", "new"], "{history}");
    let (renamed, saved) = (versions[0][0], versions[1][0]);
    assert!(is_hex(renamed, 12) && is_hex(saved, 12), "{history}");
    assert_eq!(versions[0][1].len(), "2026-10-09T18:22:41.118+01:00".len());

    let old = host.ok(&["show", saved]);
    let named = format!("==== {}\n", history.lines().nth(1).unwrap());
    assert!(old.starts_with(&(named + "+++\n")), "{old}");
    assert!(old.contains("name = \"relay\""), "{old}");

    let changed = host.ok(&["diff", renamed]);
    assert!(
        changed.starts_with(&format!("--- {saved}\n+++ lantern/relay-pin\n@@ ")),
        "{changed}"
    );
    assert!(
        changed.contains("\n-name = \"relay\"\n+name = \"relay-pin\"\n"),
        "{changed}"
    );
    assert_eq!(host.ok(&["diff", saved, renamed]), changed);

    let logged = host.ok(&["log"]);
    let of_relay: Vec<&str> = logged
        .lines()
        .filter(|line| line.contains("  lantern/relay-pin  "))
        .collect();
    assert_eq!(
        of_relay,
        [
            format!(
                "{}  desk  {renamed}  lantern/relay-pin  rename",
                versions[0][1]
            ),
            format!("{}  desk  {saved}  lantern/relay-pin  new", versions[1][1]),
        ],
        "{logged}"
    );
    assert_eq!(logged.lines().next(), Some(of_relay[0]), "{logged}");
    assert_eq!(host.ok(&["log", "1"]), format!("{}\n", of_relay[0]));
    let first = host.ok(&["diff", saved]);
    assert!(
        first.starts_with("--- nothing\n+++ lantern/relay-pin\n@@ -0,0 +1,"),
        "{first}"
    );

    host.drafted(&["checkout", "lantern/relay-pin"], &[]);
    assert_eq!(
        host.ok(&["diff", "lantern/relay-pin"]),
        format!("--- {renamed}\n+++ lantern/relay-pin\nno changes\n")
    );
    host.ok(&["discard", "lantern/relay-pin"]);

    host.ok(&["end", "lantern/relay-pin", "false", "It was pin five"]);
    let ended = host.ok(&["show", "lantern/relay-pin"]);
    let heading = ended.lines().next().expect("a line naming the version");
    assert!(heading.starts_with("==== "), "{ended}");
    assert!(heading.ends_with("  desk  ended false"), "{ended}");
    let last = host.ok(&["log", "1"]);
    assert!(
        last.ends_with("  lantern/relay-pin  ended false\n"),
        "{last}"
    );
}

#[test]
fn a_file_that_does_not_read_is_a_note_on_stderr_beside_the_document() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    host.ok(&["rename", "lantern", "lamp"]);
    let listed = host.ok(&["history", "lamp"]);
    let first = listed.lines().nth(1).expect("two versions")[..12].to_owned();
    fs::write(host.file_of(&first), "not a version\n").unwrap();

    let shown = host.run(&["show", "lamp"]);
    assert!(shown.stdout.starts_with("+++\n"), "{}", shown.stdout);
    let note = format!("worklog: lamp: version {first} does not read: ");
    assert!(shown.stderr.starts_with(&note), "{}", shown.stderr);
    assert_eq!(shown.stderr.lines().count(), 1, "{}", shown.stderr);
    assert_eq!(shown.code, Some(0));

    let history = host.run(&["history", "lamp"]);
    assert_eq!(history.stdout.lines().count(), 1, "{}", history.stdout);
    assert_eq!(history.stderr, shown.stderr);
    assert_eq!(history.code, Some(0));
    assert_eq!(
        host.ok(&["show", "lamp", "--json"]).lines().next(),
        Some("{")
    );
}

fn taken_since(host: &Host, before: &[PathBuf]) -> Vec<(PathBuf, Vec<u8>)> {
    (host.version_files().into_iter())
        .filter(|file| !before.contains(file))
        .map(|file| {
            let bytes = fs::read(&file).unwrap();
            fs::remove_file(&file).unwrap();
            (file, bytes)
        })
        .collect()
}

fn labels_in(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| labels_in(item, found)),
        Value::Object(fields) => {
            let reference = fields.len() == 3
                && ["document", "label", "ended"]
                    .iter()
                    .all(|key| fields.contains_key(*key));
            if reference {
                return;
            }
            for (key, held) in fields {
                match held.as_str() {
                    Some(label) if key == "label" => found.push(label.to_owned()),
                    _ => labels_in(held, found),
                }
            }
        }
        _ => {}
    }
}

fn labels(value: &Value) -> Vec<String> {
    let mut found = Vec::new();
    labels_in(value, &mut found);
    found
}

fn column(listed: &str, at: usize) -> Vec<String> {
    listed
        .lines()
        .filter(|line| !line.starts_with(' '))
        .map(|line| line.split("  ").nth(at).expect("a label").to_owned())
        .collect()
}

#[test]
fn a_new_host_is_stocked_and_its_session_opens_on_what_is_due_until_that_is_done() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    host.fact("fact", "lantern/relay");
    let project = host.home().join("projects/lantern");

    assert!(!host.ok_in(&project, &["context"]).contains("relay"));
    host.ok_in(&project, &["claim", "lantern"]);
    let stocked = host.ok_in(&project, &["context"]);
    assert!(stocked.contains("relay"), "{stocked}");
    assert!(!stocked.contains("due 2000-01-01"), "{stocked}");
    assert!(!host.ok(&["context"]).contains("relay"));

    let entry = host.entry("wiring");
    assert_eq!(column(&host.ok(&["entries", "lantern"]), 2), [&*entry]);
    let fuse = host.due("Order a fuse", &["--entry", &entry]);
    let line = format!("due 2000-01-01  {fuse}  Order a fuse\n");
    assert_eq!(host.ok(&["followups", &entry]), line);

    let due = host.ok_in(&project, &["context"]);
    assert!(due.contains(&line), "{due}");
    assert!(due.find(&line) < due.find("relay"), "{due}");

    assert_stored(&host.ok(&["done", &fuse, "It arrived"]), &fuse);
    let cleared = host.ok_in(&project, &["context"]);
    assert!(!cleared.contains("Order a fuse"), "{cleared}");
    assert!(!cleared.contains("due 2000-01-01"), "{cleared}");
    assert!(cleared.contains("relay"), "{cleared}");
    assert_eq!(host.ok(&["followups"]), "");
    let ended = host.ok(&["followups", "--ended"]);
    assert!(ended.contains("Order a fuse"), "{ended}");
}

#[test]
fn two_machines_fork_a_document_and_resolving_it_keeps_the_names_of_both_heads() {
    let desk = Host::set_up("desk");
    desk.topic("lantern");
    let phone = desk.sharing("phone");
    let id = desk.id("lantern");
    let synced = desk.version_files();

    assert_stored(&desk.ok(&["rename", "lantern", "lamp"]), "lamp");
    let late = taken_since(&desk, &synced);
    assert_eq!(late.len(), 1);
    assert_stored(&phone.ok(&["rename", "lantern", "torch"]), "torch");
    for (file, bytes) in &late {
        fs::write(file, bytes).unwrap();
    }

    let forked = phone.ok(&["forks"]);
    assert_eq!(desk.ok(&["forks"]), forked);
    let lines: Vec<&str> = forked.lines().collect();
    assert_eq!(lines.len(), 3, "{forked}");
    assert!(lines[0].starts_with(&format!("{id}  ")), "{forked}");
    assert!(lines[0].ends_with("  A thing  forked"), "{forked}");
    let mut writers: Vec<&str> = lines[1..]
        .iter()
        .map(|head| head.rsplit("  ").next().unwrap())
        .collect();
    writers.sort_unstable();
    assert_eq!(writers, ["desk", "phone"], "{forked}");
    for host in [&desk, &phone] {
        assert_eq!(host.ok(&["check"]), "0 problems, 0 notices, 1 fork\n");
    }

    phone.drafted(&["resolve", &id], &[]);
    let stored = phone.ok(&["save", &id]);
    let kept = halves(&stored).0;
    assert!(["lamp", "torch"].contains(&kept), "{stored}");

    for host in [&desk, &phone] {
        assert_eq!(host.ok(&["forks"]), "");
        assert_eq!(host.ok(&["check"]), "0 problems, 0 notices, 0 forks\n");
        for name in ["lantern", "lamp", "torch"] {
            assert_eq!(host.id(name), id, "{name}");
            let shown = host.ok(&["show", name]);
            assert!(shown.contains(&format!("name = \"{kept}\"")), "{shown}");
            assert!(!shown.contains("===="), "{shown}");
        }
        let mut topics = column(&host.ok(&["topics"]), 1);
        topics.sort_unstable();
        let mut expected = ["desk", kept, "phone"];
        expected.sort_unstable();
        assert_eq!(topics, expected);
    }
}

#[test]
fn a_renamed_fact_keeps_its_old_name_and_its_topic_ends_once_the_fact_is_filed_elsewhere() {
    let host = Host::set_up("desk");
    host.topic("lantern");
    host.topic("atlas");
    host.fact("fact", "lantern/relay");
    let id = host.id("lantern/relay");

    assert_stored(
        &host.ok(&["rename", "lantern/relay", "relay-pin"]),
        "lantern/relay-pin",
    );
    for name in ["lantern/relay", "lantern/relay-pin"] {
        assert_eq!(host.id(name), id, "{name}");
        let shown = host.ok(&["show", name]);
        assert!(shown.contains("name = \"relay-pin\""), "{shown}");
    }
    assert_eq!(
        column(&host.ok(&["facts", "lantern"]), 2),
        ["lantern/relay-pin"]
    );

    let ending = ["end", "lantern", "retired", "Folded into the atlas"];
    assert_refused(
        &host.run(&ending),
        1,
        "worklog: lantern: still has 1 fact\n",
    );
    assert_eq!(
        column(&host.ok(&["topics"]), 1),
        ["atlas", "desk", "lantern"]
    );

    assert_stored(
        &host.ok(&["move", "lantern", "atlas", "lantern/relay"]),
        "atlas/relay-pin",
    );
    assert_eq!(host.id("atlas/relay-pin"), id);
    assert_eq!(host.ok(&["facts", "lantern"]), "");
    assert_eq!(
        column(&host.ok(&["facts", "atlas"]), 2),
        ["atlas/relay-pin"]
    );

    assert_stored(&host.ok(&ending), "lantern");
    assert_eq!(column(&host.ok(&["topics"]), 1), ["atlas", "desk"]);
    let ended = host.ok(&["topics", "--ended"]);
    let retired = ended.lines().nth(2).expect("the ended topic");
    assert!(
        retired.ends_with("  lantern  A thing  ended retired"),
        "{ended}"
    );
    assert_eq!(column(&host.ok(&["facts"]), 2), ["atlas/relay-pin"]);
}

#[test]
fn a_damaged_file_is_named_by_check_and_the_rest_of_the_store_still_reads() {
    let host = Host::stocked();
    let topics = host.ok(&["topics"]);
    let relay = host.ok(&["show", "lantern/relay"]);
    let facts = host.ok(&["facts"]);
    assert_eq!(
        column(&facts, 2),
        ["lantern/fuse", "lantern/relay"],
        "{facts}"
    );
    let history = host.ok(&["history", "lantern/fuse"]);
    let version = &history[..12];
    assert!(is_hex(version, 12), "{history}");
    assert_eq!(host.ok(&["check"]), "0 problems, 0 notices, 0 forks\n");

    fs::write(host.file_of(version), "not a version\n").unwrap();

    let checked = host.run(&["check"]);
    assert_eq!((checked.stderr.as_str(), checked.code), ("", Some(1)));
    let lines: Vec<&str> = checked.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{}", checked.stdout);
    assert!(lines[0].contains(&format!(": {version}: ")), "{}", lines[0]);
    assert_eq!(lines[1], "1 problem, 0 notices, 0 forks");

    assert_eq!(
        host.ok(&["topics"]),
        topics.replace("(2 facts, 1 idea)", "(1 fact, 1 idea)")
    );
    assert_eq!(host.ok(&["show", "lantern/relay"]), relay);
    let left = host.ok(&["facts"]);
    assert_eq!(column(&left, 2), ["lantern/relay"], "{left}");
    assert_eq!(host.ok(&["facts", "lantern"]), left);
    assert_eq!(column(&host.ok(&["ideas"]), 2), ["lantern/dimmer"]);
}

#[test]
fn json_carries_the_labels_the_text_of_the_same_command_prints() {
    let text = Host::stocked();
    let json = Host::stocked();
    assert_eq!(text.id("lantern/relay"), json.id("lantern/relay"));

    for (args, at, count) in [
        (&["topics"][..], 1, 3),
        (&["facts", "lantern"], 2, 2),
        (&["search", "four"], 1, 3),
    ] {
        let printed = column(&text.ok(args), at);
        assert_eq!(printed.len(), count, "{args:?}");
        assert_eq!(labels(&json.json(args)), printed, "{args:?}");
        assert_eq!(labels(&text.json(args)), printed, "{args:?}");
    }

    for (args, count) in [
        (&["rename", "lantern/relay", "relay-pin"][..], 1),
        (
            &[
                "move",
                "lantern",
                "atlas",
                "lantern/relay-pin",
                "lantern/fuse",
            ],
            2,
        ),
    ] {
        let stored = text.ok(args);
        let printed: Vec<&str> = stored
            .split_inclusive('\n')
            .map(|line| halves(line).0)
            .collect();
        assert_eq!(printed.len(), count, "{args:?}");
        assert_eq!(labels(&json.json(args)), printed, "{args:?}");
    }
    assert_eq!(
        labels(&json.json(&["facts"])),
        column(&text.ok(&["facts"]), 2)
    );
}

#[test]
fn each_command_reads_what_the_one_before_it_wrote_on_either_machine() {
    let desk = Host::set_up("desk");
    desk.topic("lantern");
    let phone = desk.sharing("phone");

    desk.fact("fact", "lantern/relay");
    assert_stored(&phone.ok(&["verify", "lantern/relay"]), "lantern/relay");
    assert_stored(
        &desk.ok(&["rename", "lantern/relay", "relay-pin"]),
        "lantern/relay-pin",
    );
    phone.drafted(
        &["checkout", "lantern/relay-pin"],
        &[("Pin four", "Pin five")],
    );
    assert_stored(&phone.ok(&["save", "lantern/relay"]), "lantern/relay-pin");

    let history = desk.json(&["history", "lantern/relay-pin"]);
    assert_eq!(phone.json(&["history", "lantern/relay"]), history);
    let versions = history["versions"].as_array().expect("the versions");
    assert_eq!(versions.len(), 4, "{history}");
    let mut written = Vec::new();
    let mut parents: &[Value] = &[];
    for _ in versions {
        let next = versions
            .iter()
            .find(|version| version["parents"].as_array().map(Vec::as_slice) == Some(parents))
            .unwrap_or_else(|| panic!("a version after {parents:?}: {history}"));
        written.push(next["machine"]["label"].as_str().expect("a machine"));
        parents = std::slice::from_ref(&next["version"]);
        assert_eq!(next["head"], written.len() == versions.len(), "{history}");
    }
    assert_eq!(written, ["desk", "phone", "desk", "phone"], "{history}");

    for host in [&desk, &phone] {
        assert_eq!(host.ok(&["forks"]), "");
        assert_eq!(host.ok(&["check"]), "0 problems, 0 notices, 0 forks\n");
        let facts = host.ok(&["facts"]);
        assert!(
            facts.ends_with("  lantern/relay-pin  Pin five\n"),
            "{facts}"
        );
        assert_eq!(facts.lines().count(), 1, "{facts}");
    }
}

const PATIENCE: Duration = Duration::from_secs(30);

struct Spawned(Child);

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Spawned {
    fn stopped(&mut self) -> String {
        let _ = self.0.kill();
        let _ = self.0.wait();
        let mut stderr = String::new();
        if let Some(mut piped) = self.0.stderr.take() {
            piped.read_to_string(&mut stderr).expect("the stderr");
        }
        stderr
    }
}

struct Serving {
    spawned: Spawned,
    address: String,
}

impl Serving {
    fn asked(&self, host: &str, target: &str) -> (u16, String) {
        let to: SocketAddr = self.address.parse().expect("the address it printed");
        let mut stream = TcpStream::connect_timeout(&to, PATIENCE).expect("a connection");
        stream.set_read_timeout(Some(PATIENCE)).expect("a timeout");
        stream.set_write_timeout(Some(PATIENCE)).expect("a timeout");
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .expect("a request");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("an answer");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
        let status = head.split(' ').nth(1).and_then(|code| code.parse().ok());
        (status.expect("a status"), body.to_owned())
    }

    fn get(&self, target: &str) -> (u16, String) {
        self.asked(&self.address, target)
    }

    fn page(&self, target: &str) -> String {
        let (status, body) = self.get(target);
        assert_eq!(status, 200, "{target}: {body}");
        body
    }

    fn running(&mut self) {
        let ended = self.spawned.0.try_wait().expect("a child to ask");
        assert_eq!(ended, None, "serve ended");
    }
}

impl Host {
    fn spawned(&self, args: &[&str]) -> Spawned {
        self.spawned_on(None, Stdio::null(), args)
    }

    fn spawned_on(&self, path: Option<OsString>, stdin: Stdio, args: &[&str]) -> Spawned {
        let mut program = std::process::Command::new(env!("CARGO_BIN_EXE_worklog"));
        if let Some(path) = path {
            program.env("PATH", path);
        }
        for (_, variable) in SESSIONS {
            program.env_remove(variable);
        }
        let child = program
            .env(HOME_OF_THE_TOOL, self.root.join("next"))
            .env("HOME", self.home())
            .env(RELEASES, self.unpublished())
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_STATE_HOME")
            .current_dir(self.home())
            .args(args)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        Spawned(child)
    }

    fn ends(&self, args: &[&str]) -> Ran {
        Host::ended(self.spawned(args), args)
    }

    fn ended(mut spawned: Spawned, args: &[&str]) -> Ran {
        let deadline = Instant::now() + PATIENCE;
        let status = loop {
            if let Some(status) = spawned.0.try_wait().expect("a child to ask") {
                break status;
            }
            assert!(Instant::now() < deadline, "{args:?} is still running");
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = String::new();
        let mut piped = spawned.0.stdout.take().expect("a piped stdout");
        piped.read_to_string(&mut stdout).expect("the stdout");
        Ran {
            stdout,
            stderr: spawned.stopped(),
            code: status.code(),
        }
    }

    fn serve(&self, rest: &[&str]) -> Serving {
        self.serving(None, rest)
    }

    fn serving(&self, path: Option<OsString>, rest: &[&str]) -> Serving {
        let line = [&["serve", "--bind", "127.0.0.1:0"], rest].concat();
        let mut spawned = self.spawned_on(path, Stdio::null(), &line);
        let stdout = spawned.0.stdout.take().expect("a piped stdout");
        let (line, lines) = mpsc::channel();
        std::thread::spawn(move || {
            let mut first = String::new();
            let _ = BufReader::new(stdout).read_line(&mut first);
            let _ = line.send(first);
        });
        let printed = lines.recv_timeout(PATIENCE).unwrap_or_default();
        let address = (printed.strip_prefix("http://127.0.0.1:"))
            .and_then(|port| port.strip_suffix('\n'))
            .filter(|port| port.parse::<u16>().is_ok_and(|port| port != 0))
            .map(|port| format!("127.0.0.1:{port}"));
        let Some(address) = address else {
            panic!("serve printed {printed:?}: {}", spawned.stopped());
        };
        Serving { spawned, address }
    }
}

#[test]
fn serve_answers_the_home_a_topic_and_a_document_from_the_store() {
    let host = Host::stocked();
    let relay = host.json(&["show", "lantern/relay"]);
    let relay = relay["document"].as_str().expect("a document");
    let mut serving = host.serve(&[]);

    let home = serving.page("/");
    let lantern = ">lantern</a> <span class=\"what\">A thing</span> \
                   <span class=\"label\">2 facts</span> <span class=\"label\">1 idea</span>";
    assert!(home.contains(lantern), "{home}");
    assert!(home.contains(">desk</a>"), "{home}");

    let topic = serving.page("/doc/lantern");
    assert!(topic.contains("<h1>lantern</h1>"), "{topic}");
    let row =
        format!("href=\"/doc/{relay}\">lantern/relay</a> <span class=\"what\">Pin four</span>");
    assert!(topic.contains(&row), "{topic}");

    let document = serving.page("/doc/lantern/relay");
    assert!(document.contains("<h1>lantern/relay</h1>"), "{document}");
    assert!(
        document.contains("<dt>summary</dt><dd>Pin four</dd>"),
        "{document}"
    );
    assert_eq!(serving.page(&format!("/doc/{relay}")), document);

    serving.running();
    assert_eq!(serving.spawned.stopped(), "");
}

#[test]
fn serve_answers_each_request_from_the_store_as_it_then_stands() {
    let host = Host::stocked();
    let mut serving = host.serve(&[]);
    assert!(!serving.page("/facts").contains("lantern/wick"));
    assert!(
        serving
            .page("/")
            .contains("<span class=\"label\">2 facts</span>")
    );

    host.fact("fact", "lantern/wick");

    let facts = serving.page("/facts");
    assert!(facts.contains(">lantern/wick</a>"), "{facts}");
    let home = serving.page("/");
    assert!(
        home.contains("<span class=\"label\">3 facts</span>"),
        "{home}"
    );
    serving.running();
}

#[test]
fn serve_answers_500_while_the_store_or_the_config_is_gone_and_keeps_serving() {
    let host = Host::stocked();
    let mut serving = host.serve(&[]);
    assert!(serving.page("/").contains(">lantern</a>"));

    let config = host.root.join("next/config.toml");
    for (held, why) in [
        (host.store(), "store is not a directory"),
        (config, "this host is not set up"),
    ] {
        let away = host.root.join("away");
        fs::rename(&held, &away).unwrap();
        for target in ["/", "/facts", "/nowhere"] {
            let (status, body) = serving.get(target);
            assert_eq!(status, 500, "{target} without {}: {body}", held.display());
            assert!(body.contains("<h1>500</h1>"), "{body}");
            assert!(body.contains(why), "{body}");
            assert!(!body.contains(">lantern</a>"), "{body}");
        }
        serving.running();

        fs::rename(&away, &held).unwrap();
        assert!(serving.page("/").contains(">lantern</a>"));
    }
    serving.running();
    assert_eq!(serving.spawned.stopped(), "");
}

#[test]
fn serve_answers_only_a_request_that_names_it_by_an_address_or_a_host_it_was_given() {
    let host = Host::stocked();
    let mut serving = host.serve(&[]);
    for other in ["evil.example", "desk.local"] {
        let (status, body) = serving.asked(other, "/");
        assert_eq!(status, 421, "{other}: {body}");
        assert!(body.contains("<h1>421</h1>"), "{body}");
        assert!(!body.contains(">lantern</a>"), "{body}");
    }
    assert_eq!(serving.asked("localhost", "/").0, 200);
    serving.running();
    drop(serving);

    let mut named = host.serve(&["--host", "desk.local", "--host", "atlas"]);
    for own in ["desk.local", "atlas:8080"] {
        let (status, body) = named.asked(own, "/");
        assert_eq!(status, 200, "{own}: {body}");
        assert!(body.contains(">lantern</a>"), "{body}");
    }
    assert_eq!(named.asked("evil.example", "/").0, 421);
    named.running();
}

#[test]
fn serve_is_refused_before_it_listens_on_a_host_or_an_address_it_cannot_serve() {
    let bind = ["serve", "--bind", "127.0.0.1:0"];
    let new = Host::new();
    assert_refused(&new.ends(&bind), 1, NOT_SET_UP);

    let host = Host::set_up("desk");
    let away = host.root.join("away");
    fs::rename(host.store(), &away).unwrap();
    assert_refused(
        &host.ends(&bind),
        1,
        &format!(
            "worklog: {}: this host's store is not a directory\n",
            host.store().display()
        ),
    );
    fs::rename(&away, host.store()).unwrap();

    let taken = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = taken.local_addr().expect("its address").to_string();
    let busy = host.ends(&["serve", "--bind", &address]);
    assert_eq!((busy.stdout.as_str(), busy.code), ("", Some(1)));
    let named = format!("worklog: {address}: ");
    assert!(busy.stderr.starts_with(&named), "{}", busy.stderr);

    for line in [
        &["serve", "--bind", "desk"][..],
        &["serve", "--bind", "localhost:8080"],
        &["serve", "--bind", "127.0.0.1:0", "--json"],
        &["--json", "serve", "--bind", "127.0.0.1:0"],
    ] {
        let ran = host.ends(line);
        assert_eq!((ran.stdout.as_str(), ran.code), ("", Some(2)), "{line:?}");
        assert!(ran.stderr.starts_with("error: "), "{}", ran.stderr);
    }
}

fn binary() -> PathBuf {
    fs::canonicalize(
        Command::cargo_bin("worklog")
            .expect("the binary")
            .get_program(),
    )
    .expect("the binary's own path")
}

impl Host {
    fn with_agents(self, homes: &[&str]) -> Host {
        for home in homes {
            fs::create_dir_all(self.home().join(home)).unwrap();
        }
        self
    }

    fn skill_of(&self, home: &str) -> PathBuf {
        self.home().join(home).join("skills/worklog/SKILL.md")
    }

    fn hooks_of(&self, home: &str) -> PathBuf {
        let file = if home == ".claude" {
            "settings.json"
        } else {
            "hooks.json"
        };
        self.home().join(home).join(file)
    }

    fn rule_of_codex(&self) -> PathBuf {
        self.home().join(".codex/rules/worklog.rules")
    }

    fn hooks_json(&self, home: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(self.hooks_of(home)).unwrap()).unwrap()
    }

    fn placed(&self, homes: &[&str]) -> Vec<PathBuf> {
        (homes.iter())
            .flat_map(|home| {
                let rule = (*home == ".codex").then(|| self.rule_of_codex());
                [Some(self.skill_of(home)), rule, Some(self.hooks_of(home))]
            })
            .flatten()
            .collect()
    }
}

fn hook_command(home: &str) -> String {
    let word = if home == ".codex" { "codex" } else { "claude" };
    format!(
        "\"{}\" context --hook {word} 2>/dev/null || true",
        binary().display()
    )
}

fn lines(paths: &[PathBuf]) -> String {
    let mut text = String::new();
    for path in paths {
        text.push_str(&path.display().to_string());
        text.push('\n');
    }
    text
}

const INIT: [&str; 5] = ["init", "desk", "--store", "kept", "--summary"];

fn init_line(extra: &[&'static str]) -> Vec<&'static str> {
    [&INIT[..], &["A machine"], extra].concat()
}

fn tree(root: &Path, dir: &Path, found: &mut Vec<(PathBuf, Option<Vec<u8>>)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        if path.is_dir() {
            found.push((relative, None));
            tree(root, &path, found);
        } else {
            found.push((relative, Some(fs::read(&path).unwrap())));
        }
    }
}

fn snapshot(host: &Host) -> Vec<(PathBuf, Option<Vec<u8>>)> {
    let mut found = Vec::new();
    tree(&host.root, &host.root, &mut found);
    found
}

#[test]
fn init_with_agents_installs_for_every_agent_present_and_prints_the_paths() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    let printed = host.ok(&init_line(&["--agents"]));
    let paths = host.placed(&[".claude", ".codex"]);
    assert_eq!(printed, format!("desk created\n{}", lines(&paths)));
    for home in [".claude", ".codex"] {
        assert!(
            fs::read_to_string(host.skill_of(home))
                .unwrap()
                .contains("name: worklog")
        );
        let hooks = host.hooks_json(home);
        let command = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(command, hook_command(home));
    }
}

#[test]
fn init_json_carries_the_paths_it_wrote_and_none_without_the_flag() {
    let plain = Host::new().with_agents(&[".claude"]);
    let bound = plain.json(&init_line(&[]));
    assert_eq!(bound["written"], serde_json::json!([]), "{bound}");
    assert!(!plain.skill_of(".claude").exists());
    assert!(!plain.hooks_of(".claude").exists());

    let host = Host::new().with_agents(&[".claude", ".codex"]);
    let bound = host.json(&init_line(&["--agents"]));
    let paths: Vec<String> = (host.placed(&[".claude", ".codex"]).iter())
        .map(|path| path.display().to_string())
        .collect();
    assert_eq!(bound["written"], serde_json::json!(paths), "{bound}");
    assert_eq!(bound["label"], "desk", "{bound}");
}

#[test]
fn init_with_agents_and_no_agent_home_is_refused_before_anything_is_written() {
    let host = Host::new();
    let before = snapshot(&host);
    let ran = host.run(&init_line(&["--agents"]));
    assert_refused(
        &ran,
        1,
        "worklog: no Claude Code or Codex home on this host\n",
    );
    assert_eq!(snapshot(&host), before);
    assert!(!host.root.join("next").exists());
    assert!(!host.home().join("kept").exists());
    assert_refused(&host.run(&["topics"]), 1, NOT_SET_UP);
}

#[test]
fn agents_install_reaches_the_agents_present_and_notes_a_hook_already_there() {
    let host = Host::new().with_agents(&[".codex"]);
    assert_refused(&host.run(&["topics"]), 1, NOT_SET_UP);
    let first = host.ok(&["agents", "install"]);
    assert_eq!(first, lines(&host.placed(&[".codex"])));
    assert!(
        fs::read_to_string(host.rule_of_codex())
            .unwrap()
            .contains("pattern = [\"worklog\"]")
    );
    assert!(!host.home().join(".claude").exists());
    let hooks = fs::read(host.hooks_of(".codex")).unwrap();

    let again = host.run(&["agents", "install"]);
    assert_eq!(
        again.stdout,
        lines(&[host.skill_of(".codex"), host.rule_of_codex()])
    );
    assert_eq!(
        again.stderr,
        format!(
            "worklog: {}: a SessionStart hook already runs worklog context\n",
            host.hooks_of(".codex").display()
        )
    );
    assert_eq!(again.code, Some(0));
    assert_eq!(fs::read(host.hooks_of(".codex")).unwrap(), hooks);
    assert_eq!(host.ok(&["agents", "refresh"]), "");
}

#[test]
fn agents_install_with_no_agent_home_is_refused() {
    let host = Host::new();
    let ran = host.run(&["agents", "install"]);
    assert_refused(
        &ran,
        1,
        "worklog: no Claude Code or Codex home on this host\n",
    );
    assert!(!host.home().join(".claude").exists());
}

#[test]
fn agents_refresh_writes_nothing_where_there_is_nothing_to_bring_up() {
    let host = Host::new();
    let before = snapshot(&host);
    let ran = host.run(&["agents", "refresh"]);
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    assert_eq!(ran.code, Some(0));
    assert_eq!(snapshot(&host), before);
    assert!(!host.home().join(".claude").exists());
    assert!(!host.home().join(".codex").exists());

    let host = Host::new().with_agents(&[".claude", ".codex"]);
    let before = snapshot(&host);
    assert_eq!(host.ok(&["agents", "refresh"]), "");
    assert_eq!(snapshot(&host), before);
}

#[test]
fn agents_refresh_rewrites_a_stale_skill_rule_and_hook_and_nothing_else() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    fs::create_dir_all(host.skill_of(".claude").parent().unwrap()).unwrap();
    fs::write(host.skill_of(".claude"), "old").unwrap();
    let theirs = serde_json::json!({"hooks": [{"type": "command", "command": "date"}]});
    let stale = serde_json::json!(
        {"hooks": [{"type": "command", "command": "\"/gone/worklog\" context"}]}
    );
    let before = serde_json::json!({"hooks": {"SessionStart": [theirs, stale]}});
    fs::write(host.hooks_of(".codex"), before.to_string()).unwrap();

    let printed = host.ok(&["agents", "refresh"]);
    assert_eq!(
        printed,
        lines(&[
            host.skill_of(".claude"),
            host.rule_of_codex(),
            host.hooks_of(".codex")
        ])
    );
    assert_ne!(fs::read_to_string(host.skill_of(".claude")).unwrap(), "old");
    fs::write(host.rule_of_codex(), "old").unwrap();
    assert_eq!(
        host.ok(&["agents", "refresh"]),
        lines(&[host.rule_of_codex()])
    );
    assert_ne!(fs::read_to_string(host.rule_of_codex()).unwrap(), "old");
    assert!(!host.hooks_of(".claude").exists());
    assert!(!host.skill_of(".codex").exists());
    let after = host.hooks_json(".codex");
    let groups = after["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(groups[0], theirs);
    assert_eq!(groups[1]["hooks"][0]["command"], hook_command(".codex"));
    assert_eq!(host.ok(&["agents", "refresh"]), "");
}

#[test]
fn agents_uninstall_takes_back_only_what_install_placed() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    let other_skill = host.home().join(".claude/skills/other/SKILL.md");
    fs::create_dir_all(other_skill.parent().unwrap()).unwrap();
    fs::write(&other_skill, "desk").unwrap();
    let theirs = serde_json::json!({"hooks": [{"type": "command", "command": "date"}]});
    let before = serde_json::json!(
        {"model": "atlas", "hooks": {"SessionStart": [theirs], "Stop": []}}
    );
    fs::write(host.hooks_of(".claude"), before.to_string()).unwrap();
    host.ok(&["agents", "install"]);

    let printed = host.ok(&["agents", "uninstall"]);
    let skills = |home: &str| host.skill_of(home).parent().unwrap().to_path_buf();
    assert_eq!(
        printed,
        lines(&[
            skills(".claude"),
            host.hooks_of(".claude"),
            skills(".codex"),
            host.rule_of_codex(),
            host.hooks_of(".codex"),
        ])
    );
    assert!(!host.rule_of_codex().exists());
    assert_eq!(fs::read_to_string(&other_skill).unwrap(), "desk");
    assert!(!skills(".claude").exists());
    assert_eq!(host.hooks_json(".claude"), before);
    assert_eq!(host.ok(&["agents", "uninstall"]), "");
}

#[test]
fn agents_install_json_prints_the_written_object() {
    let host = Host::new().with_agents(&[".claude"]);
    let printed = host.json(&["agents", "install"]);
    let paths =
        [host.skill_of(".claude"), host.hooks_of(".claude")].map(|p| p.display().to_string());
    assert_eq!(printed, serde_json::json!({"written": paths}));
    assert_eq!(
        host.json(&["agents", "uninstall"])["written"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        host.json(&["agents", "uninstall"]),
        serde_json::json!({"written": []})
    );
}

#[test]
fn init_with_agents_over_a_hooks_file_it_cannot_edit_writes_nothing_at_all() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    fs::write(host.hooks_of(".codex"), "{ not json").unwrap();
    let before = snapshot(&host);
    let ran = host.run(&init_line(&["--agents"]));
    assert_eq!((ran.stdout.as_str(), ran.code), ("", Some(1)));
    assert!(
        ran.stderr.starts_with(&format!(
            "worklog: {}: not readable as JSON",
            host.hooks_of(".codex").display()
        )),
        "{}",
        ran.stderr
    );
    assert_eq!(snapshot(&host), before);
    assert!(!host.root.join("next").exists());
    assert!(!host.home().join("kept").exists());
    assert_refused(&host.run(&["topics"]), 1, NOT_SET_UP);
}

#[test]
fn agents_install_writes_nothing_for_the_first_agent_when_the_second_cannot_be_edited() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    fs::write(host.hooks_of(".codex"), "{\"hooks\": 1}").unwrap();
    let before = snapshot(&host);
    let refused = format!(
        "worklog: {}: `hooks` is not an object\n",
        host.hooks_of(".codex").display()
    );
    assert_refused(&host.run(&["agents", "install"]), 1, &refused);
    assert_eq!(snapshot(&host), before);
}

#[test]
fn agents_refresh_and_uninstall_over_a_hooks_file_that_is_not_json_change_nothing() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    fs::create_dir_all(host.skill_of(".claude").parent().unwrap()).unwrap();
    fs::write(host.skill_of(".claude"), "old").unwrap();
    fs::write(host.hooks_of(".codex"), "{ not json").unwrap();
    let before = snapshot(&host);
    for action in ["refresh", "uninstall"] {
        let ran = host.run(&["agents", action]);
        assert_eq!((ran.stdout.as_str(), ran.code), ("", Some(1)), "{action}");
        assert!(ran.stderr.contains("not readable as JSON"), "{action}");
        assert_eq!(snapshot(&host), before, "{action}");
    }
}

#[test]
fn agents_refresh_under_another_file_name_is_refused_only_over_a_hook() {
    let host = Host::new().with_agents(&[".claude", ".codex"]);
    let renamed = host.root.join("worklog-0.3.0");
    copy_of_the_binary(&renamed);
    let refresh = || host.run_as(Command::new(&renamed), &host.home(), &["agents", "refresh"]);
    let before = snapshot(&host);
    let ran = refresh();
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    assert_eq!(ran.code, Some(0));
    assert_eq!(snapshot(&host), before);

    host.ok(&["agents", "install"]);
    let before = snapshot(&host);
    assert_refused(
        &refresh(),
        1,
        &format!(
            "worklog: {}: the binary's file name must be worklog, or its hook is not \
             found again\n",
            renamed.display()
        ),
    );
    assert_eq!(snapshot(&host), before);
}

impl Host {
    fn fish(&self) -> PathBuf {
        let dir = self.home().join(".config/fish/completions");
        fs::create_dir_all(&dir).unwrap();
        dir.join("worklog.fish")
    }
}

fn fish_line() -> String {
    format!("COMPLETE=fish \"{}\" | source\n", binary().display())
}

#[test]
fn completions_prints_the_line_a_shell_takes_them_from_this_binary_with() {
    let host = Host::new();
    assert_eq!(host.ok(&["completions", "fish"]), fish_line());
    for shell in ["bash", "zsh"] {
        assert_eq!(
            host.ok(&["completions", shell]),
            format!("source <(COMPLETE={shell} \"{}\")\n", binary().display())
        );
    }
    for line in [
        &["completions", "fish", "--json"][..],
        &["completions", "elvish"],
        &["completions"],
    ] {
        let ran = host.run(line);
        assert_eq!((ran.stdout.as_str(), ran.code), ("", Some(2)), "{line:?}");
    }
    assert!(!host.root.join("next").exists());
}

#[test]
fn refresh_writes_the_completions_where_fish_reads_them_and_only_where_fish_is() {
    let host = Host::new().with_agents(&[".claude"]);
    assert_eq!(host.ok(&["refresh"]), "");
    assert!(!host.home().join(".config").exists());

    fs::create_dir_all(host.home().join(".config/fish")).unwrap();
    fs::write(host.home().join(".config/fish/completions"), "").unwrap();
    assert_eq!(host.ok(&["refresh"]), "");
    fs::remove_file(host.home().join(".config/fish/completions")).unwrap();

    let completions = host.fish();
    fs::create_dir_all(host.skill_of(".claude").parent().unwrap()).unwrap();
    fs::write(host.skill_of(".claude"), "old").unwrap();
    assert_eq!(
        host.ok(&["refresh"]),
        lines(&[host.skill_of(".claude"), completions.clone()])
    );
    assert_eq!(fs::read_to_string(&completions).unwrap(), fish_line());
    assert_eq!(
        host.ok(&["completions", "fish"]),
        fs::read_to_string(&completions).unwrap()
    );

    fs::write(&completions, "old").unwrap();
    let again = host.json(&["refresh"]);
    let path = completions.display().to_string();
    assert_eq!(again, serde_json::json!({"written": [path]}));
    assert_eq!(fs::read_to_string(&completions).unwrap(), fish_line());
    assert_eq!(host.ok(&["refresh"]), lines(&[completions]));
}

#[test]
fn agents_refresh_leaves_the_completions_as_they_are() {
    let host = Host::new().with_agents(&[".claude"]);
    let completions = host.fish();
    fs::create_dir_all(host.skill_of(".claude").parent().unwrap()).unwrap();
    fs::write(host.skill_of(".claude"), "old").unwrap();
    assert_eq!(
        host.ok(&["agents", "refresh"]),
        lines(&[host.skill_of(".claude")])
    );
    assert!(!completions.exists());
    fs::write(&completions, "old").unwrap();
    assert_eq!(host.ok(&["agents", "refresh"]), "");
    assert_eq!(fs::read_to_string(&completions).unwrap(), "old");
}

#[test]
fn refresh_follows_the_config_directory_the_environment_gives_fish() {
    let host = Host::new();
    let under_home = host.fish();
    let given = host.root.join("config");
    fs::create_dir_all(given.join("fish/completions")).unwrap();
    let mut program = Command::cargo_bin("worklog").expect("the binary");
    program.env("XDG_CONFIG_HOME", &given);
    let ran = host.run_as(program, &host.home(), &["refresh"]);
    let written = given.join("fish/completions/worklog.fish");
    assert_eq!(
        (ran.stdout, ran.stderr, ran.code),
        (
            lines(std::slice::from_ref(&written)),
            String::new(),
            Some(0)
        )
    );
    assert_eq!(fs::read_to_string(written).unwrap(), fish_line());
    assert!(!under_home.exists());

    fs::create_dir_all(host.home().join("config/fish/completions")).unwrap();
    let mut program = Command::cargo_bin("worklog").expect("the binary");
    program.env("XDG_CONFIG_HOME", "config");
    let ran = host.run_as(program, &host.home(), &["refresh"]);
    assert_eq!(
        (ran.stdout, ran.stderr, ran.code),
        (
            lines(std::slice::from_ref(&under_home)),
            String::new(),
            Some(0)
        )
    );
    let relative = host.home().join("config/fish/completions");
    assert_eq!(fs::read_dir(relative).unwrap().count(), 0);
}

fn fish_left_out(renamed: &Path) -> String {
    format!(
        "worklog: {}: fish's completions are not written, since the binary's file name is \
         not worklog\n",
        renamed.display()
    )
}

#[test]
fn refresh_under_another_file_name_writes_for_the_agents_and_no_completions() {
    let host = Host::new().with_agents(&[".claude"]);
    let completions = host.fish();
    let renamed = host.root.join("worklog-0.3.0");
    copy_of_the_binary(&renamed);
    let run = |line: &[&str]| host.run_as(Command::new(&renamed), &host.home(), line);
    let before = snapshot(&host);
    let ran = run(&["refresh"]);
    assert_eq!(
        (ran.stdout.as_str(), ran.stderr, ran.code),
        ("", fish_left_out(&renamed), Some(0))
    );
    assert_refused(
        &run(&["completions", "fish"]),
        1,
        &format!(
            "worklog: {}: the binary's file name must be worklog, or its \
             completions are registered for another command\n",
            renamed.display()
        ),
    );
    assert_eq!(snapshot(&host), before);

    fs::create_dir_all(host.skill_of(".claude").parent().unwrap()).unwrap();
    fs::write(host.skill_of(".claude"), "old").unwrap();
    let ran = run(&["refresh"]);
    assert_eq!(
        (ran.stdout, ran.stderr, ran.code),
        (
            lines(&[host.skill_of(".claude")]),
            fish_left_out(&renamed),
            Some(0)
        )
    );
    assert_ne!(fs::read_to_string(host.skill_of(".claude")).unwrap(), "old");
    let again = run(&["refresh", "--json"]);
    assert_eq!(
        one_value(&again.stdout),
        Some(serde_json::json!({"written": []}))
    );
    assert_eq!(
        (again.stderr, again.code),
        (fish_left_out(&renamed), Some(0))
    );
    assert!(!completions.exists());
}

#[test]
fn install_and_uninstall_leave_the_fish_completions_as_they_are() {
    let host = Host::new().with_agents(&[".claude"]);
    let completions = host.fish();
    let placed = host.placed(&[".claude"]);
    assert_eq!(host.ok(&["agents", "install"]), lines(&placed));
    assert!(!completions.exists());
    host.ok(&["agents", "uninstall"]);
    assert_eq!(
        host.ok(&init_line(&["--agents"])),
        format!("desk created\n{}", lines(&placed))
    );
    assert!(!completions.exists());

    assert_eq!(
        host.ok(&["refresh"]),
        lines(std::slice::from_ref(&completions))
    );
    let skills = host.skill_of(".claude").parent().unwrap().to_path_buf();
    assert_eq!(
        host.ok(&["agents", "uninstall"]),
        lines(&[skills, host.hooks_of(".claude")])
    );
    assert_eq!(fs::read_to_string(&completions).unwrap(), fish_line());
}

impl Host {
    fn asking(&self, mut program: Command, words: &[&str]) -> Ran {
        program.env("COMPLETE", "fish");
        self.run_as(program, &self.home(), words)
    }

    fn completing(&self, program: Command, words: &[&str]) -> Ran {
        self.asking(program, &[&["--", "worklog"], words].concat())
    }

    fn offered(&self, words: &[&str]) -> String {
        let ran = self.completing(found(), words);
        assert_eq!(ran.stderr, "", "{words:?}");
        assert_eq!(ran.code, Some(0), "{words:?}");
        ran.stdout
    }

    fn completed(&self, words: &[&str]) -> String {
        let offered = self.offered(words);
        let values = offered.split_inclusive('\n');
        values.filter(|line| !line.starts_with('-')).collect()
    }

    fn forked_by(&self, line: &[&str]) {
        let other = Host::new();
        copy_tree(&self.store(), &other.store());
        let store = other.store();
        other.ok(&["init", "desk", "--store", store.to_str().unwrap()]);
        self.ok(line);
        other.ok(line);
        copy_tree(&other.store(), &self.store());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Topics,
    Facts,
    Entries,
    Followups,
    Documents,
    Renamable,
    Movable,
    TopicsAndEntries,
    Forked,
    Ended,
    Drafts,
    Reasons,
    Triggers,
}

const SOURCED: [(&str, &[&str], Source); 36] = [
    (
        "new followup topics",
        &["new", "followup", "--topics", ""],
        Source::Topics,
    ),
    (
        "new followup touching",
        &["new", "followup", "--topics", "lantern", "--touching", ""],
        Source::Topics,
    ),
    (
        "new followup entry",
        &["new", "followup", "--topics", "lantern", "--entry", ""],
        Source::Entries,
    ),
    (
        "new followup about",
        &["new", "followup", "--topics", "lantern", "--about", ""],
        Source::Documents,
    ),
    ("checkout target", &["checkout", ""], Source::Documents),
    ("save target", &["save", ""], Source::Drafts),
    ("discard target", &["discard", ""], Source::Drafts),
    ("resolve target", &["resolve", ""], Source::Forked),
    ("rename target", &["rename", ""], Source::Renamable),
    ("move from", &["move", ""], Source::Topics),
    ("move to", &["move", "lantern", ""], Source::Topics),
    (
        "move targets",
        &["move", "lantern", "atlas", ""],
        Source::Movable,
    ),
    ("verify fact", &["verify", ""], Source::Facts),
    ("end target", &["end", ""], Source::Documents),
    ("end reason", &["end", "lantern", ""], Source::Reasons),
    (
        "end by",
        &["end", "lantern", "retired", "--by", ""],
        Source::Documents,
    ),
    ("reopen target", &["reopen", ""], Source::Ended),
    ("done followup", &["done", ""], Source::Followups),
    ("drop followup", &["drop", ""], Source::Followups),
    ("trigger followup", &["trigger", ""], Source::Followups),
    ("trigger when", &["trigger", "0a1b", ""], Source::Triggers),
    ("claim topic", &["claim", ""], Source::Topics),
    ("unclaim topic", &["unclaim", ""], Source::Topics),
    ("show target", &["show", ""], Source::Documents),
    ("history target", &["history", ""], Source::Documents),
    ("diff target", &["diff", ""], Source::Drafts),
    ("log machine", &["log", "--machine", ""], Source::Topics),
    (
        "search topic",
        &["search", "relay", "--topic", ""],
        Source::Topics,
    ),
    ("usage machine", &["usage", "--machine", ""], Source::Topics),
    ("topics topic", &["topics", ""], Source::Topics),
    ("facts topic", &["facts", ""], Source::Topics),
    ("ideas topic", &["ideas", ""], Source::Topics),
    ("entries topic", &["entries", ""], Source::Topics),
    (
        "followups about",
        &["followups", ""],
        Source::TopicsAndEntries,
    ),
    ("where topic", &["where", ""], Source::Topics),
    ("where machine", &["where", "--machine", ""], Source::Topics),
];

const REASONS: &str = "retired\ttopic\nmerged\ttopic, entry\nfalse\tfact\nmoved\tfact\n\
                       superseded\tfact\nbuilt\tidea\nabandoned\tidea\nremoved\tentry, claim\n\
                       done\tfollowup\ndropped\tfollowup\n";

#[test]
fn each_argument_is_completed_from_the_list_its_command_takes_and_a_tab_writes_nothing() {
    let host = Host::stocked();
    let open = host.followup("Order a fuse");
    let wiring = host.entry("wiring");
    host.ok(&["end", "lantern/fuse", "false", "It blew"]);
    host.forked_by(&["verify", "lantern/relay"]);
    host.ok(&["new", "topic", "phone"]);

    let topics = "atlas\tA thing\ndesk\tA machine\nlantern\tA thing\n";
    let facts = "lantern/relay\tPin four\nlantern/dimmer\tPin four\n";
    let entries = format!("{wiring}\tPin four\n");
    let followups = format!("{open}\tOrder a fuse\n");
    let listed = |source: Source| match source {
        Source::Topics => topics.to_owned(),
        Source::Facts => facts.to_owned(),
        Source::Entries => entries.clone(),
        Source::Followups => followups.clone(),
        Source::Documents => format!("{topics}{facts}{entries}{followups}"),
        Source::Renamable => format!("{topics}{facts}{entries}"),
        Source::Movable => format!("{facts}{entries}{followups}"),
        Source::TopicsAndEntries => format!("{topics}{entries}"),
        Source::Forked => "lantern/relay\tPin four\n".to_owned(),
        Source::Ended => "lantern/fuse\tPin four\n".to_owned(),
        Source::Drafts => "phone\ttopic\n".to_owned(),
        Source::Reasons => REASONS.to_owned(),
        Source::Triggers => "touching\tthen a topic\nnone\tno trigger\n".to_owned(),
    };
    let mut sources: Vec<Source> = SOURCED.iter().map(|(_, _, source)| *source).collect();
    sources.sort_unstable();
    sources.dedup();
    assert_eq!(sources.len(), 13);
    let mut lists: Vec<String> = sources.iter().map(|source| listed(*source)).collect();
    lists.sort();
    lists.dedup();
    assert_eq!(lists.len(), 13, "two lists read the same on this host");

    let before = snapshot(&host);
    for (argument, words, source) in SOURCED {
        assert_eq!(host.completed(words), listed(source), "{argument}");
    }
    assert_eq!(snapshot(&host), before);
    assert_eq!(host.usage_files(), [""; 0]);
}

#[test]
fn a_shell_completes_a_prefix_a_comma_list_and_directories_but_never_files() {
    let host = Host::stocked();
    fs::write(host.home().join("projects.txt"), "").unwrap();
    let before = snapshot(&host);

    assert_eq!(host.completed(&["facts", "la"]), "lantern\tA thing\n");
    assert_eq!(
        host.completed(&["new", "followup", "--topics", "atlas,la"]),
        "atlas,lantern\tA thing\n"
    );
    assert!(!host.completed(&["show", ""]).contains("projects"));
    assert_eq!(host.completed(&["context", "pro"]), "projects/\n");
    assert_eq!(host.completed(&["claim", "lantern", "pro"]), "projects/\n");
    for words in [
        &["search", "pro"][..],
        &["new", "topic", "pro"],
        &["rename", "lantern", "pro"],
        &["reopen", "lantern", "pro"],
    ] {
        assert_eq!(host.completed(words), "", "{words:?}");
    }
    assert_eq!(host.completed(&["completions", ""]), "fish\nbash\nzsh\n");

    let flags = |words: &[&str]| -> Vec<String> {
        let offered = host.offered(words);
        let flag = |line: &str| line.split('\t').next().unwrap_or_default().to_owned();
        offered.lines().map(flag).collect()
    };
    assert_eq!(flags(&["serve", "--"]), ["--bind", "--host", "--help"]);
    assert_eq!(flags(&["completions", "--"]), ["--help"]);
    assert_eq!(flags(&["forks", "--"]), ["--json", "--help"]);
    assert_eq!(snapshot(&host), before);
}

#[test]
fn a_forked_document_and_one_with_a_file_that_does_not_read_are_each_offered_once() {
    let host = Host::stocked();
    host.forked_by(&["verify", "lantern/relay"]);
    assert_eq!(host.ok(&["forks"]).lines().count(), 3);
    host.ok(&["rename", "lantern/fuse", "breaker"]);
    let listed = host.ok(&["history", "lantern/breaker"]);
    let first = listed.lines().nth(1).expect("two versions")[..12].to_owned();
    fs::write(host.file_of(&first), "not a version\n").unwrap();
    let whole = host.ok(&["history", "atlas"]);
    fs::write(host.file_of(&whole[..12]), "not a version\n").unwrap();

    let before = snapshot(&host);
    assert_eq!(
        host.completed(&["verify", ""]),
        "lantern/breaker\tPin four\nlantern/relay\tPin four\nlantern/dimmer\tPin four\n"
    );
    assert_eq!(
        host.completed(&["facts", ""]),
        "desk\tA machine\nlantern\tA thing\n"
    );
    assert_eq!(
        host.completed(&["resolve", ""]),
        "lantern/relay\tPin four\n"
    );
    assert_eq!(snapshot(&host), before);
}

#[test]
fn a_shell_asking_on_a_host_with_no_store_is_offered_nothing_and_told_nothing() {
    let host = Host::new();
    let before = snapshot(&host);
    assert_eq!(host.completed(&["facts", ""]), "");
    assert_eq!(host.completed(&["show", ""]), "");
    assert_eq!(host.completed(&["reopen", ""]), "");
    assert_eq!(host.completed(&["resolve", ""]), "");
    assert_eq!(snapshot(&host), before);
    let stocked = Host::stocked();
    fs::remove_dir_all(stocked.store()).unwrap();
    assert_eq!(stocked.completed(&["facts", ""]), "");
    fs::write(stocked.root.join("next/config.toml"), "store = 4\n").unwrap();
    let before = snapshot(&stocked);
    assert_eq!(stocked.completed(&["save", ""]), "");
    assert_eq!(snapshot(&stocked), before);
}

#[test]
fn a_shell_asking_for_candidates_is_no_run_the_usage_log_holds() {
    let host = Host::stocked();
    let asked = host.completing(found(), &["facts", "la"]);
    assert_eq!(
        (asked.stdout.as_str(), asked.stderr.as_str(), asked.code),
        ("lantern\tA thing\n", "", Some(0))
    );
    let script = host.asking(found(), &[]);
    assert_eq!((script.stderr.as_str(), script.code), ("", Some(0)));
    assert!(script.stdout.contains("worklog"), "{}", script.stdout);
    host.used_ok(&["completions", "fish"]);
    assert_eq!(host.usage_files(), [""; 0]);

    let listed = host.used_ok(&["topics"]);
    for unset in ["", "0"] {
        let mut program = found();
        program.env("COMPLETE", unset);
        let ran = host.run_as(program, &host.home(), &["topics"]);
        assert_eq!(
            (ran.stdout.as_str(), ran.stderr.as_str(), ran.code),
            (listed.as_str(), "", Some(0)),
            "{unset:?}"
        );
    }
    assert_eq!(commands(&host.logged()).len(), 3);
}

fn path_finding(binary: &Path) -> OsString {
    let ahead = binary.parent().map(Path::to_path_buf).into_iter();
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(ahead.chain(std::env::split_paths(&rest))).expect("a path")
}

fn path_finding_the_binary() -> OsString {
    path_finding(Path::new(env!("CARGO_BIN_EXE_worklog")))
}

fn found() -> Command {
    let mut program = Command::cargo_bin("worklog").expect("the binary");
    program.env("PATH", path_finding_the_binary());
    program
}

impl Host {
    fn used(&self, args: &[&str]) -> Ran {
        self.run_as(found(), &self.home(), args)
    }

    fn used_ok(&self, args: &[&str]) -> String {
        let ran = self.used(args);
        assert_eq!(ran.stderr, "", "{args:?}");
        assert_eq!(ran.code, Some(0), "{args:?}");
        ran.stdout
    }

    fn usage_files(&self) -> Vec<String> {
        let Ok(files) = fs::read_dir(self.store().join("usage")) else {
            return Vec::new();
        };
        let name = |file: std::io::Result<fs::DirEntry>| {
            file.unwrap().file_name().into_string().expect("a name")
        };
        let mut names: Vec<String> = files.map(name).collect();
        names.sort();
        names
    }

    fn logged(&self) -> Vec<Value> {
        let files = self.usage_files();
        let mut machines: Vec<&str> = files.iter().map(|file| &file[..32]).collect();
        machines.dedup();
        assert_eq!(machines.len(), 1, "one machine's files: {files:?}");
        let mut logged = Vec::new();
        for file in &files {
            let text = fs::read_to_string(self.store().join("usage").join(file)).unwrap();
            assert!(text.ends_with('\n'), "{text}");
            let lines = text.lines();
            logged.extend(lines.map(|line| serde_json::from_str(line).expect("a line that reads")));
        }
        logged
    }

    fn machine(&self, name: &str) -> String {
        let shown = self.json(&["show", name]);
        shown["document"].as_str().expect("a document").to_owned()
    }
}

fn commands(logged: &[Value]) -> Vec<(&str, i64, Vec<&str>)> {
    (logged.iter())
        .map(|line| {
            let arguments = line["arguments"].as_array().expect("the arguments");
            (
                line["command"].as_str().expect("a command"),
                line["exit"].as_i64().expect("an exit code"),
                (arguments.iter().filter_map(Value::as_str)).collect(),
            )
        })
        .collect()
}

#[test]
fn a_run_is_logged_only_into_a_store_that_holds_the_machine_s_topic() {
    let host = Host::set_up("desk");
    let kept = host.root.join("kept");
    fs::rename(host.store(), &kept).unwrap();
    fs::create_dir(host.store()).unwrap();
    assert_eq!(host.used_ok(&["topics"]), "");
    assert_eq!(host.used(&["facts", "--no-such-flag"]).code, Some(2));
    assert_refused(
        &host.used(&["show", "lantern"]),
        1,
        "worklog: lantern: names no document\n",
    );
    assert_eq!(fs::read_dir(host.store()).unwrap().count(), 0);

    fs::remove_dir(host.store()).unwrap();
    fs::rename(&kept, host.store()).unwrap();
    let listed = host.used_ok(&["topics"]);
    assert!(listed.ends_with("  desk  A machine\n"), "{listed}");
    assert_eq!(commands(&host.logged()), [("topics", 0, vec![])]);
}

#[test]
fn every_run_is_logged_under_the_machine_that_ran_it() {
    let host = Host::stocked();
    let desk = host.machine("desk");
    assert_eq!(host.usage_files(), [""; 0]);
    host.used_ok(&["facts", "lantern"]);
    host.used_ok(&["facts", "lantern"]);
    let refused = host.used(&["show", "lantern/nothing"]);
    assert_eq!((refused.stdout.as_str(), refused.code), ("", Some(1)));
    assert_eq!(
        host.used_ok(&["usage"]),
        "desk — 3 runs\n      2 facts\n      1 show\n"
    );

    let files = host.usage_files();
    let month = files[0].strip_prefix(&format!("{desk}-")).expect("the id");
    let month = month.strip_suffix(".jsonl").expect("a log's name");
    assert_eq!(month.len(), "2026-10".len(), "{files:?}");
    let logged = host.logged();
    let keys: Vec<&String> = logged[2].as_object().expect("an object").keys().collect();
    assert_eq!(
        keys,
        [
            "written",
            "machine",
            "version",
            "agent",
            "session",
            "command",
            "exit",
            "refusal",
            "hits",
            "directory",
            "arguments"
        ]
    );
    let refusal = logged[2]["refusal"].as_str().expect("what the run said");
    assert_eq!(refused.stderr, format!("worklog: {refusal}\n"));
    let written = logged[2]["written"].as_str().expect("a stamp");
    assert!(written.starts_with(month), "{written}");
    let expected = serde_json::json!({
        "written": written,
        "machine": desk,
        "version": env!("CARGO_PKG_VERSION"),
        "agent": null,
        "session": null,
        "command": "show",
        "exit": 1,
        "refusal": refusal,
        "hits": null,
        "directory": "~",
        "arguments": ["lantern/nothing"],
    });
    assert_eq!(logged[2], expected);
    assert_eq!(logged[0]["refusal"], Value::Null);

    host.used_ok(&["--json", "topics"]);
    host.used_ok(&["topics", "--json"]);
    host.run_as(found(), &host.home().join("projects/lantern"), &["topics"]);
    let logged = host.logged();
    assert_eq!(
        commands(&logged)[4..],
        [
            ("topics", 0, vec!["--json"]),
            ("topics", 0, vec!["--json"]),
            ("topics", 0, vec![])
        ]
    );
    assert_eq!(logged[6]["directory"], "~/projects/lantern");

    for line in [
        &["usage", "--machine", "atlas"][..],
        &["usage", "--since", "2099-01-01"],
    ] {
        let none = host.used(line);
        assert_eq!(none.stderr, "worklog: nothing logged\n", "{line:?}");
        assert_eq!((none.stdout.as_str(), none.code), ("", Some(0)), "{line:?}");
    }
    assert_eq!(
        host.used_ok(&["usage", "--since", "2099-01-01", "--json"]),
        "[]\n"
    );
    let unknown = host.used(&["usage", "--machine", "torch"]);
    assert_refused(&unknown, 1, "worklog: torch: names no document\n");
    let undated = host.used(&["usage", "--since", "soon"]);
    assert_refused(&undated, 2, "worklog: `soon` is not a date (YYYY-MM-DD)\n");
    let counted = one_value(&host.used_ok(&["usage", "--machine", "desk", "--json"]));
    let counted = counted.expect("one value");
    assert_eq!(
        counted[0]["machine"],
        serde_json::json!({"document": desk, "label": "desk", "ended": null})
    );
    assert_eq!(
        counted[0]["commands"][0],
        serde_json::json!({"command": "usage", "run_count": 6})
    );
}

#[test]
fn a_search_is_logged_with_how_many_documents_it_matched() {
    let host = Host::stocked();
    host.used_ok(&["search", "relay"]);
    host.used_ok(&["search", "relay", "--json"]);
    assert_eq!(host.used_ok(&["search", "no-such-word-anywhere"]), "");
    host.used_ok(&["facts", "lantern"]);
    let logged = host.logged();
    let hits: Vec<(&str, &Value)> = (logged.iter())
        .map(|line| (line["command"].as_str().expect("a command"), &line["hits"]))
        .collect();
    let (one, none) = (Value::from(1), Value::from(0));
    assert_eq!(
        hits,
        [
            ("search", &one),
            ("search", &one),
            ("search", &none),
            ("facts", &Value::Null)
        ]
    );
}

#[test]
fn a_command_line_the_parser_refuses_is_logged_under_the_command_it_named() {
    let host = Host::stocked();
    let lines = [
        &["facts", "--no-such-flag"][..],
        &["new", "entry"],
        &["no-such-command"],
        &["serve", "--json"],
        &["init", "--no-such-flag"],
        &["agents"],
        &["refresh"],
        &["--help"],
        &["--version"],
        &["help", "facts"],
    ];
    let ran: Vec<Ran> = lines.iter().map(|line| host.used(line)).collect();
    let logged = host.logged();
    assert_eq!(
        commands(&logged),
        [
            ("facts", 2, vec!["--no-such-flag"]),
            ("new entry", 2, vec![]),
            ("", 2, vec!["no-such-command"]),
            ("serve", 2, vec!["--json"]),
        ]
    );
    let refusal = logged[0]["refusal"].as_str().expect("what the parser said");
    assert!(refusal.contains("--no-such-flag"), "{refusal}");
    assert!(!refusal.starts_with("error"), "{refusal}");
    assert_eq!(
        ran[0].stderr.lines().next(),
        Some(&*format!("error: {refusal}"))
    );
    assert_eq!(
        host.used_ok(&["usage"]),
        "desk — 3 runs\n      1 facts\n      1 new entry\n      1 serve\n"
    );
}

fn within(agent: &str, id: &str) -> (Value, Value) {
    (Value::from(agent), Value::from(id))
}

impl Host {
    fn sessions(&self) -> Vec<(Value, Value)> {
        let session = |line: Value| (line["agent"].clone(), line["session"].clone());
        self.logged().into_iter().map(session).collect()
    }
}

#[test]
fn a_run_inside_an_agent_s_session_is_logged_with_its_id_and_the_first_agent_s_when_nested() {
    let host = Host::stocked();
    let home = host.home();
    for (variable, id) in [(SESSIONS[0].1, "s-0"), (SESSIONS[1].1, "s-1")] {
        let mut program = found();
        program.env(variable, id);
        assert_eq!(host.run_as(program, &home, &["facts"]).code, Some(0));
    }
    let mut nested = found();
    for (_, variable) in SESSIONS {
        nested.env(variable, "s-nested");
    }
    assert_eq!(host.run_as(nested, &home, &["facts"]).code, Some(0));
    host.used_ok(&["facts"]);
    let expected = [
        within(SESSIONS[0].0, "s-0"),
        within(SESSIONS[1].0, "s-1"),
        within(SESSIONS[0].0, "s-nested"),
        (Value::Null, Value::Null),
    ];
    assert_eq!(host.sessions(), expected);
}

#[test]
fn a_context_an_agent_s_hook_runs_is_logged_in_the_session_its_input_holds() {
    let host = Host::stocked();
    let home = host.home();
    let plain = host.used_ok(&["context"]);
    let hooked = |input: &str, args: &[&str], outer: bool| {
        let mut program = found();
        program.write_stdin(input);
        if outer {
            program.env(SESSIONS[0].1, "s-outer");
        }
        let ran = host.run_as(program, &home, args);
        assert_eq!((ran.stdout.as_str(), ran.code), (plain.as_str(), Some(0)));
    };
    let given = r#"{"session_id": "s-9", "cwd": "/home/desk", "source": "startup"}"#;
    let codex = ["context", "--hook", "codex"];
    hooked(given, &codex, false);
    hooked(given, &codex, true);
    hooked("not json", &codex, false);
    hooked(given, &["context", "--hook", "lantern"], false);
    hooked(given, &["context"], false);
    hooked(given, &["context"], true);

    let none = (Value::Null, Value::Null);
    let expected = [
        none.clone(),
        within(SESSIONS[1].0, "s-9"),
        within(SESSIONS[1].0, "s-9"),
        none.clone(),
        none.clone(),
        none,
        within(SESSIONS[0].0, "s-outer"),
    ];
    assert_eq!(host.sessions(), expected);
}

#[test]
fn a_hook_whose_input_is_never_closed_still_opens_its_session() {
    let host = Host::stocked();
    let plain = host.ok(&["context"]);
    let line = ["context", "--hook", "codex"];
    let mut held_open = host.spawned_on(None, Stdio::piped(), &line);
    let input = held_open.0.stdin.take().expect("a piped stdin");
    let ran = Host::ended(held_open, &line);
    drop(input);
    assert_eq!(
        (ran.stdout, ran.stderr, ran.code),
        (plain, String::new(), Some(0))
    );
}

#[test]
fn a_binary_the_path_does_not_find_logs_nothing() {
    let host = Host::stocked();
    let elsewhere = host.root.join("bench");
    fs::create_dir_all(&elsewhere).unwrap();
    let built = Path::new(env!("CARGO_BIN_EXE_worklog"));
    fs::write(elsewhere.join(built.file_name().expect("a name")), "").unwrap();
    let mut shadowed = found();
    let finding = path_finding_the_binary();
    let behind = std::iter::once(elsewhere.clone()).chain(std::env::split_paths(&finding));
    shadowed.env("PATH", std::env::join_paths(behind).unwrap());
    let mut unfound = Command::cargo_bin("worklog").expect("the binary");
    unfound.env("PATH", &elsewhere);
    for program in [shadowed, unfound] {
        let off = host.run_as(program, &host.home(), &["facts", "lantern"]);
        assert_eq!((off.stderr.as_str(), off.code), ("", Some(0)));
    }
    assert_eq!(host.usage_files(), [""; 0]);
    host.used_ok(&["facts", "lantern"]);
    assert_eq!(host.used_ok(&["usage"]), "desk — 1 run\n      1 facts\n");
}

#[test]
fn a_host_that_is_not_set_up_logs_nothing_and_neither_does_setting_one_up() {
    let host = Host::new();
    assert_refused(&host.used(&["topics"]), 1, NOT_SET_UP);
    let opened = host.used(&["context"]);
    assert_eq!((opened.stderr.as_str(), opened.code), (NOT_SET_UP, Some(0)));
    assert_eq!(host.used(&["facts", "--no-such-flag"]).code, Some(2));
    let logs = |host: &Host| {
        let held = snapshot(host);
        let logs = held
            .iter()
            .filter(|(path, _)| path.starts_with("store/usage"));
        logs.count()
    };
    assert_eq!(logs(&host), 0);

    let store = host.store();
    let line = ["init", "desk", "--store", store.to_str().unwrap()];
    let made = host.used_ok(&[&line[..], &["--summary", "A machine"]].concat());
    assert_eq!(made, "desk created\n");
    assert_ne!(host.used(&line).code, Some(0));
    host.used(&["refresh"]);
    assert_eq!(logs(&host), 0);
    host.used_ok(&["topics"]);
    assert_eq!(commands(&host.logged()), [("topics", 0, vec![])]);
}

#[test]
fn a_store_holding_its_usage_log_alone_is_one_a_host_can_be_bound_to() {
    let host = Host::stocked();
    host.used_ok(&["topics"]);
    let other = Host::new();
    copy_tree(&host.store().join("usage"), &other.store().join("usage"));
    assert_eq!(other.init("phone", &other.store()), "phone created\n");
    other.used_ok(&["topics"]);
    assert_eq!(other.usage_files().len(), 2);
    let counted = other.used_ok(&["usage"]);
    let unheld = host.machine("desk");
    let of_unheld = format!("{unheld} — 1 run\n      1 topics\n");
    assert_eq!(
        counted,
        format!("{of_unheld}\nphone — 1 run\n      1 topics\n")
    );
    assert_eq!(other.used_ok(&["usage", "--machine", &unheld]), of_unheld);
    let machines = other.json(&["usage"]);
    assert_eq!(
        machines[0]["machine"],
        serde_json::json!({"document": unheld, "label": unheld, "ended": null})
    );
    assert_eq!(
        [&machines[0]["topic"], &machines[1]["topic"]],
        [&Value::Null, &Value::from("phone")]
    );
    let logged_nowhere = unheld.replace(|digit: char| digit != '0', "0");
    assert_refused(
        &other.used(&["usage", "--machine", &logged_nowhere]),
        1,
        &format!("worklog: {logged_nowhere}: names no document\n"),
    );

    let usage = other.store().join("usage");
    for file in other.usage_files() {
        let conflict = file.replace(".jsonl", ".sync-conflict-20261012-101010-ABCDEFG.jsonl");
        fs::copy(usage.join(&file), usage.join(conflict)).unwrap();
        fs::copy(
            usage.join(&file),
            usage.join(format!("desk-{}", &file[33..])),
        )
        .unwrap();
    }
    let again = other.used_ok(&["usage"]);
    assert!(again.starts_with(&of_unheld), "{again}");
    let by_id = other.used_ok(&["usage", "--machine", &other.machine("phone")[..8]]);
    assert!(by_id.starts_with("phone — "), "{by_id}");
}

#[test]
fn a_log_that_cannot_be_written_or_read_leaves_every_command_as_it_was() {
    let host = Host::stocked();
    let listed = host.ok(&["topics"]);
    fs::write(host.store().join("usage"), "").unwrap();
    let ran = host.used(&["topics"]);
    assert_eq!(ran.stderr, "");
    assert_eq!((ran.stdout, ran.code), (listed, Some(0)));
    let unparsed = host.used(&["facts", "--no-such-flag"]);
    assert_eq!(unparsed.code, Some(2));
    assert!(
        unparsed.stderr.starts_with("error: "),
        "{}",
        unparsed.stderr
    );
    let none = host.used(&["usage"]);
    assert_eq!(none.stderr, "worklog: nothing logged\n");
    assert_eq!((none.stdout.as_str(), none.code), ("", Some(0)));
    assert_eq!(fs::read(host.store().join("usage")).unwrap(), b"");
}

#[test]
fn a_renamed_machine_topic_keeps_its_file_and_is_printed_by_its_new_name() {
    let host = Host::stocked();
    host.used_ok(&["facts"]);
    host.used_ok(&["rename", "desk", "bench"]);
    host.used_ok(&["facts"]);
    let files = host.usage_files();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].starts_with(&host.machine("bench")), "{files:?}");
    let counted = "bench — 3 runs\n      2 facts\n      1 rename\n";
    assert_eq!(host.used_ok(&["usage"]), counted);
    let by_the_old_name = host.used_ok(&["usage", "--machine", "desk"]);
    assert!(
        by_the_old_name.starts_with("bench — 4 runs\n"),
        "{by_the_old_name}"
    );

    let phone = host.sharing("phone");
    phone.used_ok(&["topics"]);
    assert_eq!(host.usage_files().len(), 2);
    assert_eq!(
        phone.used_ok(&["usage", "--machine", "phone"]),
        "phone — 1 run\n      1 topics\n"
    );
}

impl Host {
    fn serve_used(&self) -> Serving {
        self.serving(Some(path_finding_the_binary()), &[])
    }
}

#[test]
fn serve_is_logged_once_before_its_first_request() {
    let host = Host::stocked();
    let mut serving = host.serve_used();
    let bound = ("serve", 0, vec!["--bind", "127.0.0.1:0"]);
    assert_eq!(commands(&host.logged()), std::slice::from_ref(&bound));
    assert!(serving.page("/").contains(">lantern</a>"));
    assert!(serving.page("/facts").contains("lantern/relay"));
    serving.running();
    assert_eq!(commands(&host.logged()), std::slice::from_ref(&bound));

    let mut again = found();
    again.args(["serve", "--bind", &serving.address]);
    let busy = host.run_as(again, &host.home(), &[]);
    assert_eq!(busy.code, Some(1));
    let logged = host.logged();
    assert_eq!(
        commands(&logged),
        [bound, ("serve", 1, vec!["--bind", &serving.address])]
    );
    let refusal = logged[1]["refusal"]
        .as_str()
        .expect("why it did not listen");
    assert_eq!(busy.stderr, format!("worklog: {refusal}\n"));
}

const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn release_asset() -> String {
    worklog::domain::release::asset("worklog").expect("a release for this target")
}

impl Host {
    fn releases(&self) -> PathBuf {
        self.root.join("releases")
    }

    fn publish(&self, tag: &str, bytes: &[u8], checksum_of: &[u8]) {
        let (dir, asset) = (self.releases(), release_asset());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("latest"), format!("{tag}\n")).unwrap();
        fs::write(dir.join(&asset), bytes).unwrap();
        let digest = worklog::domain::release::sha256_hex(checksum_of);
        fs::write(
            dir.join(format!("{asset}.sha256")),
            format!("{digest}  {asset}\n"),
        )
        .unwrap();
    }

    fn install_as(&self, name: &str) -> PathBuf {
        let prefix = self.home().join(".local/bin");
        fs::create_dir_all(&prefix).unwrap();
        let installed = prefix.join(name);
        copy_of_the_binary(&installed);
        installed
    }

    fn install(&self) -> PathBuf {
        self.install_as("worklog")
    }

    fn upgrade(&self, installed: &Path, args: &[&str]) -> Ran {
        let mut program = Command::new(installed);
        program.env(RELEASES, self.releases());
        self.run_as(program, &self.home(), args)
    }
}

fn copy_of_the_binary(to: &Path) {
    fs::copy(binary(), to).unwrap();
    let unpublished = to.with_extension("unpublished");
    let version = || {
        let mut copy = std::process::Command::new(to);
        copy.arg("--version").env(RELEASES, &unpublished).output()
    };
    loop {
        match version() {
            Err(busy) if busy.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::yield_now();
            }
            ran => break assert!(ran.expect("the copy runs").status.success()),
        }
    }
}

fn beside(file: &Path) -> Vec<OsString> {
    let dir = file.parent().expect("a directory");
    let entries = fs::read_dir(dir).unwrap();
    entries.map(|entry| entry.unwrap().file_name()).collect()
}

struct Stale {
    host: Host,
    installed: PathBuf,
    skill: PathBuf,
    settings: PathBuf,
    completions: PathBuf,
}

impl Stale {
    fn new() -> Stale {
        let host = Host::new().with_agents(&[".claude"]);
        let (skill, settings) = (host.skill_of(".claude"), host.hooks_of(".claude"));
        fs::create_dir_all(skill.parent().unwrap()).unwrap();
        fs::write(&skill, "old").unwrap();
        let stale = serde_json::json!(
            {"hooks": [{"type": "command", "command": "\"/gone/worklog\" context"}]}
        );
        let hooks = serde_json::json!({"hooks": {"SessionStart": [stale]}});
        fs::write(&settings, hooks.to_string()).unwrap();
        Stale {
            installed: host.install(),
            completions: host.fish(),
            host,
            skill,
            settings,
        }
    }

    fn upgrade(&self, args: &[&str]) -> Ran {
        self.host.upgrade(&self.installed, args)
    }

    fn hook(&self) -> String {
        let hooks = self.host.hooks_json(".claude");
        let command = &hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"];
        command.as_str().expect("a command").to_owned()
    }

    fn fish_line(&self) -> String {
        fs::read_to_string(&self.completions).unwrap()
    }
}

#[test]
fn upgrade_has_the_release_it_put_in_place_refresh_the_host() {
    let stale = Stale::new();
    let released = stale.host.root.join("released/worklog");
    fs::create_dir_all(released.parent().unwrap()).unwrap();
    copy_of_the_binary(&released);
    let handing_on = format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", released.display());
    stale
        .host
        .publish("v9.9.9", handing_on.as_bytes(), handing_on.as_bytes());
    let checked = stale.upgrade(&["upgrade", "--check"]);
    assert_eq!(
        (checked.stdout, checked.stderr.as_str(), checked.code),
        (format!("current: {CURRENT}\nlatest: 9.9.9\n"), "", Some(0))
    );
    let checked = stale.upgrade(&["upgrade", "--check", "--json"]);
    assert_eq!((checked.stderr.as_str(), checked.code), ("", Some(0)));
    assert_eq!(
        one_value(&checked.stdout),
        Some(serde_json::json!({"current": CURRENT, "latest": "9.9.9", "newer": true}))
    );
    assert_eq!(fs::read_to_string(&stale.skill).unwrap(), "old");
    assert!(!stale.completions.exists());

    let upgraded = stale.upgrade(&["upgrade"]);
    assert_eq!(
        upgraded.stderr,
        format!("worklog: upgraded {CURRENT} to 9.9.9\n")
    );
    assert_eq!(
        upgraded.stdout,
        lines(&[
            stale.installed.clone(),
            stale.skill.clone(),
            stale.settings.clone(),
            stale.completions.clone()
        ])
    );
    assert_eq!(upgraded.code, Some(0));
    assert_eq!(beside(&stale.installed), ["worklog"]);
    assert_eq!(fs::read(&stale.installed).unwrap(), handing_on.as_bytes());
    assert_ne!(fs::read_to_string(&stale.skill).unwrap(), "old");
    assert_eq!(
        stale.hook(),
        format!(
            "\"{}\" context --hook claude 2>/dev/null || true",
            released.display()
        )
    );
    assert_eq!(
        stale.fish_line(),
        format!("COMPLETE=fish \"{}\" | source\n", released.display())
    );
}

#[test]
fn upgrade_replaces_the_binary_only_with_a_newer_release_whose_checksum_matches() {
    let stale = Stale::new();
    let bytes = fs::read(binary()).unwrap();
    let path = |file: &Path| file.display().to_string();
    stale.host.publish("v9.9.9", &bytes, &bytes);
    let upgraded = stale.upgrade(&["upgrade", "--json"]);
    assert_eq!((upgraded.stderr.as_str(), upgraded.code), ("", Some(0)));
    assert_eq!(
        one_value(&upgraded.stdout),
        Some(serde_json::json!({
            "current": CURRENT,
            "latest": "9.9.9",
            "outcome": "upgraded",
            "written": [
                path(&stale.installed),
                path(&stale.skill),
                path(&stale.settings),
                path(&stale.completions),
            ],
        }))
    );
    assert_eq!(fs::read(&stale.installed).unwrap(), bytes);
    assert_eq!(
        stale.hook(),
        "\"$HOME/.local/bin/worklog\" context --hook claude 2>/dev/null || true"
    );
    assert_eq!(
        stale.fish_line(),
        "COMPLETE=fish \"$HOME/.local/bin/worklog\" | source\n"
    );

    stale.host.publish(&format!("v{CURRENT}"), b"", b"");
    fs::write(&stale.skill, "old").unwrap();
    let stayed = stale.upgrade(&["upgrade"]);
    assert_eq!(stayed.stderr, format!("worklog: already at {CURRENT}\n"));
    assert_eq!(
        (stayed.stdout, stayed.code),
        (
            lines(&[stale.skill.clone(), stale.completions.clone()]),
            Some(0)
        )
    );
    let checked = stale.upgrade(&["upgrade", "--check"]);
    assert_eq!(
        (checked.stdout, checked.stderr.as_str(), checked.code),
        (
            format!("current: {CURRENT}\nlatest: {CURRENT}\n"),
            "",
            Some(0)
        )
    );

    stale.host.publish("v9.9.9", b"tampered", b"");
    assert_refused(
        &stale.upgrade(&["upgrade"]),
        1,
        &format!(
            "worklog: {}: the checksum does not match the release\n",
            release_asset()
        ),
    );
    assert_eq!(fs::read(&stale.installed).unwrap(), bytes);
    assert_eq!(beside(&stale.installed), ["worklog"]);
}

#[test]
fn an_upgrade_check_against_releases_that_do_not_read_is_refused_in_text_and_in_json() {
    let stale = Stale::new();
    let latest = stale.host.releases().join("latest");
    let asked = [
        &["upgrade", "--check"][..],
        &["upgrade", "--check", "--json"],
    ];
    let refused = |stale: &Stale| {
        for line in asked {
            let checked = stale.upgrade(line);
            assert_eq!((checked.stdout.as_str(), checked.code), ("", Some(1)));
            let said = checked
                .stderr
                .strip_prefix(&format!("worklog: {}: ", latest.display()));
            assert!(
                said.is_some_and(|why| why.ends_with('\n')),
                "{}",
                checked.stderr
            );
            assert_eq!(checked.stderr.lines().count(), 1);
        }
    };
    refused(&stale);
    fs::create_dir_all(stale.host.releases()).unwrap();
    refused(&stale);
    fs::write(&latest, "\n").unwrap();
    for line in asked {
        assert_refused(
            &stale.upgrade(line),
            1,
            "worklog: `` is not a version of the form X.Y.Z\n",
        );
    }
    assert_eq!(fs::read_to_string(&stale.skill).unwrap(), "old");
    assert_eq!(beside(&stale.installed), ["worklog"]);
}

#[test]
fn the_refresh_after_an_upgrade_is_run_by_the_file_at_the_binary_s_own_path() {
    let host = Host::new();
    let installed = host.install();
    let echoing = b"#!/bin/sh\nprintf '%s\\n' \"$0\" \"$*\"\n";
    host.publish("v9.9.9", echoing, echoing);
    let upgraded = host.upgrade(&installed, &["upgrade"]);
    assert_eq!(
        upgraded.stderr,
        format!("worklog: upgraded {CURRENT} to 9.9.9\n")
    );
    let path = installed.display();
    assert_eq!(
        (upgraded.stdout, upgraded.code),
        (format!("{path}\n{path}\nrefresh\n"), Some(0))
    );
    assert_eq!(fs::read(&installed).unwrap(), echoing);
    assert_eq!(beside(&installed), ["worklog"]);

    let installed = host.install();
    let failing = b"#!/bin/sh\necho /desk/skill\necho 'worklog: no agents here' >&2\nexit 1\n";
    host.publish("v9.9.9", failing, failing);
    let stopped = host.upgrade(&installed, &["upgrade"]);
    assert_eq!(
        stopped.stderr,
        format!(
            "worklog: upgraded {CURRENT} to 9.9.9\n\
             worklog: {path} refresh: no agents here\n"
        )
    );
    assert_eq!(
        (stopped.stdout, stopped.code),
        (format!("{path}\n/desk/skill\n"), Some(1))
    );
    assert_eq!(fs::read(&installed).unwrap(), failing);

    let installed = host.install();
    host.publish("v9.9.9", failing, failing);
    let stopped = host.upgrade(&installed, &["upgrade", "--json"]);
    assert_eq!(
        one_value(&stopped.stdout),
        Some(serde_json::json!({
            "current": CURRENT,
            "latest": "9.9.9",
            "outcome": "upgraded",
            "written": [path.to_string(), "/desk/skill"],
        }))
    );
    assert_eq!(
        (stopped.stderr, stopped.code),
        (
            format!("worklog: {path} refresh: no agents here\n"),
            Some(1)
        )
    );
}

#[test]
fn an_upgrade_whose_new_binary_cannot_write_for_fish_says_what_that_binary_wrote() {
    let stale = Stale::new();
    fs::create_dir(&stale.completions).unwrap();
    let bytes = fs::read(binary()).unwrap();
    stale.host.publish("v9.9.9", &bytes, &bytes);
    let stopped = stale.upgrade(&["upgrade"]);
    assert_eq!(
        (stopped.stdout, stopped.code),
        (
            lines(&[
                stale.installed.clone(),
                stale.skill.clone(),
                stale.settings.clone()
            ]),
            Some(1)
        )
    );
    let said: Vec<&str> = stopped.stderr.lines().collect();
    assert_eq!(said.len(), 2, "{}", stopped.stderr);
    assert_eq!(said[0], format!("worklog: upgraded {CURRENT} to 9.9.9"));
    let failed = format!(
        "worklog: {} refresh: {}: ",
        stale.installed.display(),
        stale.completions.display()
    );
    assert!(said[1].starts_with(&failed), "{}", stopped.stderr);
    assert_ne!(fs::read_to_string(&stale.skill).unwrap(), "old");
}

#[test]
fn a_binary_under_another_file_name_is_upgraded_on_a_host_with_fish() {
    let host = Host::new().with_agents(&[".claude"]);
    let completions = host.fish();
    let skill = host.skill_of(".claude");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "old").unwrap();
    let installed = host.install_as("worklog-0.3.0");
    let bytes = fs::read(binary()).unwrap();
    host.publish("v9.9.9", &bytes, &bytes);
    let upgraded = host.upgrade(&installed, &["upgrade"]);
    assert_eq!(
        upgraded.stderr,
        format!(
            "worklog: upgraded {CURRENT} to 9.9.9\n{}",
            fish_left_out(&installed)
        )
    );
    assert_eq!(
        (upgraded.stdout, upgraded.code),
        (lines(&[installed.clone(), skill.clone()]), Some(0))
    );
    assert_ne!(fs::read_to_string(&skill).unwrap(), "old");

    host.publish(&format!("v{CURRENT}"), b"", b"");
    fs::write(&skill, "old").unwrap();
    let stayed = host.upgrade(&installed, &["upgrade"]);
    assert_eq!(
        stayed.stderr,
        format!(
            "worklog: already at {CURRENT}\n{}",
            fish_left_out(&installed)
        )
    );
    assert_eq!(
        (stayed.stdout, stayed.code),
        (lines(std::slice::from_ref(&skill)), Some(0))
    );
    assert!(!completions.exists());
}

#[test]
fn a_misnamed_binary_whose_hook_is_installed_is_upgraded_and_its_refresh_refused_by_the_hook_s_rule()
 {
    let host = Host::new().with_agents(&[".claude"]);
    let completions = host.fish();
    host.ok(&["agents", "install"]);
    let installed = host.install_as("worklog-0.3.0");
    let refusal = format!(
        "{}: the binary's file name must be worklog, or its hook is not found again\n",
        installed.display()
    );
    let bytes = fs::read(binary()).unwrap();
    host.publish(&format!("v{CURRENT}"), b"", b"");
    let before = snapshot(&host);
    let stayed = host.upgrade(&installed, &["upgrade"]);
    assert_refused(
        &stayed,
        1,
        &format!("worklog: already at {CURRENT}\nworklog: {refusal}"),
    );
    assert_eq!(snapshot(&host), before);

    host.publish("v9.9.9", &bytes, &bytes);
    let upgraded = host.upgrade(&installed, &["upgrade"]);
    assert_eq!(
        upgraded.stderr,
        format!(
            "worklog: upgraded {CURRENT} to 9.9.9\n\
             worklog: {} refresh: {refusal}",
            installed.display()
        )
    );
    assert_eq!(
        (upgraded.stdout, upgraded.code),
        (lines(std::slice::from_ref(&installed)), Some(1))
    );
    assert!(!completions.exists());
    assert_eq!(host.hooks_json(".claude"), {
        let hooks = &before
            .iter()
            .find(|(path, _)| path.ends_with("settings.json"));
        let (_, bytes) = hooks.expect("the hooks file");
        serde_json::from_slice::<Value>(bytes.as_ref().unwrap()).unwrap()
    });
}

#[test]
fn an_upgrade_and_the_refresh_it_has_the_new_binary_run_are_no_runs_the_usage_log_holds() {
    let host = Host::set_up("desk").with_agents(&[".claude"]);
    let installed = host.install();
    let found = || {
        let mut program = Command::new("worklog");
        program.env("PATH", installed.parent().expect("a directory"));
        program.env(RELEASES, host.releases());
        program
    };
    let used = |args: &[&str]| host.run_as(found(), &host.home(), args);
    let logged = || -> Vec<String> {
        let logged = host.logged();
        let command = |line: &Value| line["command"].as_str().expect("a command").to_owned();
        logged.iter().map(command).collect()
    };
    host.publish(&format!("v{CURRENT}"), b"", b"");
    assert_eq!(used(&["topics"]).code, Some(0));
    assert_eq!(logged(), ["topics"]);

    assert_eq!(used(&["upgrade", "--check"]).code, Some(0));
    let stayed = used(&["upgrade"]);
    assert_eq!(
        (stayed.stderr, stayed.code),
        (format!("worklog: already at {CURRENT}\n"), Some(0))
    );
    let bytes = fs::read(binary()).unwrap();
    host.publish("v9.9.9", &bytes, &bytes);
    assert_eq!(used(&["upgrade", "--check"]).code, Some(0));
    let upgraded = used(&["upgrade"]);
    assert_eq!(
        (upgraded.stderr, upgraded.code),
        (format!("worklog: upgraded {CURRENT} to 9.9.9\n"), Some(0))
    );
    assert_eq!(upgraded.stdout, lines(std::slice::from_ref(&installed)));
    assert_eq!(logged(), ["topics"]);

    assert_eq!(used(&["facts"]).code, Some(0));
    assert_eq!(logged(), ["topics", "facts"]);
}

impl Host {
    fn run_under_home(&self, args: &[&str]) -> Ran {
        let mut program = Command::cargo_bin("worklog").expect("the binary");
        program.env_remove(HOME_OF_THE_TOOL);
        self.run_as(program, &self.home(), args)
    }

    fn ok_under_home(&self, args: &[&str]) -> String {
        let ran = self.run_under_home(args);
        assert_eq!(ran.stderr, "", "{args:?}");
        assert_eq!(ran.code, Some(0), "{args:?}");
        ran.stdout
    }

    fn init_under_home(&self) -> String {
        let store = self.store();
        let store = store.to_str().unwrap();
        self.ok_under_home(&["init", "desk", "--store", store, "--summary", "A machine"])
    }
}

#[test]
fn refresh_takes_the_hook_the_tool_before_the_rebuild_left_as_its_own() {
    for spelled_with_home in [true, false] {
        let host = Host::new().with_agents(&[".claude"]);
        let home = if spelled_with_home {
            "$HOME".to_owned()
        } else {
            host.home().display().to_string()
        };
        let command = format!("\"{home}/.local/bin/worklog\" context 2>/dev/null || true");
        let theirs = serde_json::json!({"hooks": [{"type": "command", "command": "date"}]});
        let left = serde_json::json!({"hooks": [{"type": "command", "command": command}]});
        let before = serde_json::json!({"hooks": {"SessionStart": [theirs, left]}});
        fs::write(host.hooks_of(".claude"), before.to_string()).unwrap();

        let printed = host.ok(&["refresh"]);
        assert_eq!(printed, lines(&[host.hooks_of(".claude")]), "{command}");
        let ours =
            serde_json::json!({"hooks": [{"type": "command", "command": hook_command(".claude")}]});
        assert_eq!(
            host.hooks_json(".claude")["hooks"]["SessionStart"],
            serde_json::json!([theirs, ours]),
            "{command}"
        );
        assert_eq!(host.ok(&["refresh"]), "", "{command}");
    }
}

#[test]
fn refresh_writes_its_skill_over_the_one_the_tool_before_the_rebuild_left() {
    let host = Host::new().with_agents(&[".codex"]);
    let left = host.home().join(".codex/skills/worklog/SKILL.md");
    fs::create_dir_all(left.parent().unwrap()).unwrap();
    fs::write(&left, "---\nname: worklog\ndescription: Lamp notes\n---\n").unwrap();
    assert_eq!(left, host.skill_of(".codex"));

    let printed = host.ok(&["refresh"]);
    assert_eq!(
        printed,
        lines(&[host.skill_of(".codex"), host.rule_of_codex()])
    );
    assert_eq!(
        fs::read_to_string(&left).unwrap(),
        include_str!("../skill/SKILL.md")
    );
    assert_eq!(host.ok(&["refresh"]), "");
}

#[test]
fn a_host_holding_the_config_of_the_tool_before_the_rebuild_is_not_set_up_until_init() {
    let host = Host::new();
    let left = host.home().join(".config/worklog/config");
    fs::create_dir_all(left.parent().unwrap()).unwrap();
    let text = "machine: desk\nstore: /home/desk/store\n";
    fs::write(&left, text).unwrap();

    for line in [
        &["topics"][..],
        &["new", "topic", "lantern"],
        &["serve", "--bind", "127.0.0.1:0"],
    ] {
        assert_refused(&host.run_under_home(line), 1, NOT_SET_UP);
    }
    let opened = host.run_under_home(&["context"]);
    assert_eq!(
        (opened.stdout.as_str(), opened.stderr.as_str(), opened.code),
        ("", NOT_SET_UP, Some(0))
    );

    assert_eq!(host.init_under_home(), "desk created\n");
    assert_eq!(fs::read_to_string(&left).unwrap(), text);
    assert!(left.with_file_name("config.toml").is_file());
    let listed = host.ok_under_home(&["topics"]);
    assert!(listed.ends_with("  desk  A machine\n"), "{listed}");
}

#[test]
fn the_drafts_the_tool_before_the_rebuild_left_are_passed_over_and_kept_beside_new_ones() {
    let host = Host::new();
    let drafts = host.home().join(".local/state/worklog/drafts");
    let left = [
        (
            "fact/lantern/relay.md",
            "---\nsummary: Pin four\n---\nThe relay is on pin four.\n",
        ),
        (
            "fact/lantern/relay.parents",
            "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9\n",
        ),
        (
            "entry/2026-09/2026-09-04-lamp.md",
            "---\nsummary: Rewired the lamp\ntags: [lantern]\n---\nDone.\n",
        ),
        ("entry/2026-09/2026-09-04-lamp.parents", ""),
    ];
    for (file, text) in left {
        fs::create_dir_all(drafts.join(file).parent().unwrap()).unwrap();
        fs::write(drafts.join(file), text).unwrap();
    }
    host.init_under_home();
    assert_eq!(host.ok_under_home(&["drafts"]), "");

    let printed = host.ok_under_home(&["new", "topic", "lantern"]);
    let opened = PathBuf::from(printed.strip_suffix('\n').expect("one line"));
    assert_eq!(opened.parent(), Some(drafts.as_path()), "{printed}");
    let text = fs::read_to_string(&opened).unwrap();
    fs::write(
        &opened,
        text.replacen("summary = \"\"", "summary = \"A thing\"", 1),
    )
    .unwrap();
    let listed = host.ok_under_home(&["drafts"]);
    assert_eq!(listed.lines().count(), 1, "{listed}");
    assert!(listed.contains("lantern"), "{listed}");
    assert_stored(&host.ok_under_home(&["save", "lantern"]), "lantern");
    assert_eq!(host.ok_under_home(&["drafts"]), "");
    for (file, text) in left {
        assert_eq!(fs::read_to_string(drafts.join(file)).unwrap(), text);
    }
}
