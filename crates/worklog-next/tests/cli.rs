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
const DUE: &str =
    "due now — `worklog-next show <id>` for one, `worklog-next due`, run here, for all:\n";
const CLOSING: &str = "Durable facts and ideas, by name where they fit and counted where not —\n\
     `worklog-next facts <topic>` for a topic's facts,\n\
     `worklog-next ideas <topic>` for a topic's ideas,\n\
     `worklog-next show <topic>/<name>` for one,\n\
     `worklog-next followups <topic>` for open work.\n";
const NOT_SET_UP: &str = "worklog-next: this host is not set up; run `worklog-next init`\n";

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
        Host { _kept: kept, root }
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
        let output = Command::cargo_bin("worklog-next")
            .expect("the binary")
            .env("WORKLOG_NEXT_HOME", self.root.join("next"))
            .env("HOME", self.home())
            .env_remove("XDG_CONFIG_HOME")
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
            let mut files = Vec::new();
            read_tree(&host.root, &host.root, &mut files);
            Stocked {
                store: host.store().to_str().unwrap().to_owned(),
                files,
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

fn read_tree(root: &Path, dir: &Path, found: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            read_tree(root, &path, found);
        } else {
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            found.push((relative, fs::read(&path).unwrap()));
        }
    }
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
    assert_refused(
        &again,
        1,
        "worklog-next: this host is already set up as desk\n",
    );
    assert!(!host.home().join("elsewhere").exists());
}

#[test]
fn init_refuses_a_directory_of_other_files_and_writes_nothing() {
    let host = Host::new();
    let project = host.home().join("projects/lantern");
    fs::write(project.join("notes.md"), "a person's own").unwrap();
    let mut before = Vec::new();
    read_tree(&host.root, &host.root, &mut before);

    let line = ["init", "desk", "--store", "projects/lantern"];
    let refused = host.run(&[&line[..], &["--summary", "A machine"]].concat());
    assert_refused(
        &refused,
        1,
        &format!(
            "worklog-next: {}: is not empty and holds no store\n",
            project.display()
        ),
    );
    assert!(!host.root.join("next").exists());
    let mut after = Vec::new();
    read_tree(&host.root, &host.root, &mut after);
    assert_eq!(after, before);
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
            "worklog-next: {}: is not empty and holds no store\n",
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
            "1 draft left open on this machine — `worklog-next drafts`\n",
            "1 open followup not due — `worklog-next followups <topic>`\n",
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
        "worklog-next: {}: this host's store is not a directory\n",
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
        format!("worklog-next: {shown}: forked: resolve it\n")
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
        "worklog-next: lantern: no draft\n",
    );
    assert_refused(
        &host.run(&["show", "lantern"]),
        1,
        "worklog-next: lantern: names no document\n",
    );
}

#[test]
fn a_usage_error_exits_two() {
    let host = Host::set_up("desk");
    let named = host.run(&["new", "topic", "Not_A_Name"]);
    assert_eq!(named.stdout, "");
    assert!(
        named.stderr.starts_with("worklog-next: "),
        "{}",
        named.stderr
    );
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
    assert_refused(
        &ran,
        2,
        "worklog-next: `someday` is not a date (YYYY-MM-DD)\n",
    );
}

#[test]
fn help_and_the_version_go_to_stdout_with_exit_zero() {
    let host = Host::new();
    let help = host.ok(&["--help"]);
    assert!(help.contains("Usage: worklog-next"), "{help}");
    for command in ["init", "new", "checkout", "trigger", "context", "check"] {
        assert!(
            help.contains(&format!("\n  {command} ")),
            "{command}: {help}"
        );
    }
    assert!(host.ok(&["--version"]).starts_with("worklog-next "));
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
    host.followup("Check the driver");
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
    fs::write(&host.version_files()[0], "not a version\n").unwrap();
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
        "worklog-next: lantern: no draft\n",
    );
    assert_refused(
        &host.run(&["show", "lantern", "--json"]),
        1,
        "worklog-next: lantern: names no document\n",
    );
    assert_refused(
        &host.run(&["new", "entry", "wiring", "--date", "soon", "--json"]),
        2,
        "worklog-next: `soon` is not a date (YYYY-MM-DD)\n",
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
        serde_json::json!([{"document": lantern, "label": "lantern"}])
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
        "worklog-next: nowhere: names no document\n",
    );
    assert_refused(
        &host.run(&["topics", "lantern/relay"]),
        1,
        "worklog-next: lantern/relay: is not a topic\n",
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
    let note = format!("worklog-next: lamp: version {first} does not read: ");
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
            let reference = fields.len() == 2
                && fields.contains_key("document")
                && fields.contains_key("label");
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
        "worklog-next: lantern: still has 1 fact\n",
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
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_worklog-next"))
            .env("WORKLOG_NEXT_HOME", self.root.join("next"))
            .env("HOME", self.home())
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_STATE_HOME")
            .current_dir(self.home())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the binary runs");
        Spawned(child)
    }

    fn ends(&self, args: &[&str]) -> Ran {
        let mut spawned = self.spawned(args);
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
        let mut spawned = self.spawned(&[&["serve", "--bind", "127.0.0.1:0"], rest].concat());
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
            "worklog-next: {}: this host's store is not a directory\n",
            host.store().display()
        ),
    );
    fs::rename(&away, host.store()).unwrap();

    let taken = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let address = taken.local_addr().expect("its address").to_string();
    let busy = host.ends(&["serve", "--bind", &address]);
    assert_eq!((busy.stdout.as_str(), busy.code), ("", Some(1)));
    let named = format!("worklog-next: {address}: ");
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
