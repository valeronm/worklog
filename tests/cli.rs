//! The binary over a scratch store: exit codes, stdout shapes and the
//! write path end to end.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use worklog::domain::usage::Invocation;

struct Scratch {
    root: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        let root = tempfile::tempdir().expect("a temp dir");
        fs::create_dir_all(root.path().join("home/projects/lantern")).unwrap();
        fs::create_dir_all(root.path().join("home/projects/Android/atlas")).unwrap();
        fs::create_dir_all(root.path().join("home/Documents")).unwrap();
        Scratch { root }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    /// An agent's home directory, created so the agent counts as present.
    fn agent_home(&self, dir: &str) -> PathBuf {
        let home = self.home().join(dir);
        fs::create_dir_all(&home).unwrap();
        home
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_binary(Command::cargo_bin("worklog").expect("the binary"), args)
    }

    fn run_binary(&self, mut command: Command, args: &[&str]) -> Output {
        command
            .env("WORKLOG_HOME", self.root.path())
            .env("HOME", self.home())
            .env_remove("XDG_CONFIG_HOME")
            .current_dir(self.home())
            .args(args)
            .output()
            .expect("the binary runs")
    }

    fn ok(&self, args: &[&str]) -> String {
        self.ok_binary(Command::cargo_bin("worklog").expect("the binary"), args)
    }

    fn ok_binary(&self, command: Command, args: &[&str]) -> String {
        let out = self.run_binary(command, args);
        assert!(
            out.status.success(),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8 stdout")
    }

    fn refused(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{args:?} did not refuse: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(out.stdout.is_empty(), "a refusal leaves stdout empty");
        String::from_utf8(out.stderr).expect("utf-8 stderr")
    }

    /// Another machine holding a copy of this store, as after a sync.
    fn peer(&self, machine: &str) -> Scratch {
        let other = Scratch::new();
        other.ok(&["init", machine]);
        self.sync_to(&other);
        other
    }

    /// What Syncthing would do: every version file here appears there.
    fn sync_to(&self, other: &Scratch) {
        copy_dir(
            &self.root.path().join("store"),
            &other.root.path().join("store"),
        );
    }

    /// Opens a draft with the command, rewrites it with `edit`, saves it.
    fn write(&self, open: &[&str], edit: impl Fn(&str) -> String) -> String {
        let path = self.ok(open);
        let path = Path::new(path.trim());
        let text = fs::read_to_string(path).expect("the draft");
        assert!(!text.contains("\nslug: "), "no header in a draft: {text}");
        fs::write(path, edit(&text)).expect("the draft is writable");
        // `<root>/drafts/<kind>/<slug>.md` names the document.
        let slug = path
            .strip_prefix(self.root.path().join("drafts"))
            .expect("under the drafts directory")
            .with_extension("")
            .components()
            .skip(1)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        self.ok(&["save", &slug]).trim().to_owned()
    }
}

/// A field of a `history` or `log` line, which are two spaces apart.
fn column(line: &str, n: usize) -> &str {
    line.split("  ").nth(n).expect("the column")
}

fn set_summary(text: &str, summary: &str) -> String {
    text.replace("summary:\n", &format!("summary: {summary}\n"))
}

fn seeded() -> Scratch {
    let s = Scratch::new();
    s.ok(&["init", "m1"]);
    s.write(&["new", "topic", "lantern"], |t| {
        set_summary(t, "A Rust app that dims a lamp") + "What to know first.\n"
    });
    s.write(&["new", "topic", "android"], |t| {
        set_summary(t, "The Android toolchain").replace("summary:", "includes: [phone]\nsummary:")
    });
    s.write(&["new", "topic", "phone"], |t| {
        set_summary(t, "A test phone")
    });
    s.write(&["new", "topic", "atlas"], |t| {
        set_summary(t, "An Android app").replace("summary:", "includes: [android]\nsummary:")
    });
    s.write(&["new", "topic", "personal"], |t| {
        set_summary(t, "Life admin")
    });
    s.write(&["new", "topic", "host"], |t| {
        set_summary(t, "This machine").replace(
            "summary:",
            "machine: m1\nclaims:\n  lantern: [~/projects/lantern]\n  atlas: [~/projects/Android/atlas]\nunclaimed: [personal]\nsummary:",
        )
    });
    s.write(&["new", "fact", "lantern/relay-pin-is-fixed"], |t| {
        set_summary(t, "The relay pin is fixed on the board")
    });
    s.write(&["new", "idea", "phone/beta-builds"], |t| {
        set_summary(t, "Move the phone to beta builds")
    });
    s.write(
        &["new", "entry", "lamp-driver", "--date", "2026-09-01"],
        |t| set_summary(t, "Wired the lamp driver").replace("tags: []", "tags: [lantern, rust]"),
    );
    s.ok(&[
        "new",
        "followup",
        "port",
        "--entry",
        "2026-09/2026-09-01-lamp-driver",
        "--summary",
        "Add the second relay",
        "--recheck",
        "2026-01-01 long overdue",
    ]);
    s
}

#[test]
fn init_is_once_and_writes_need_it() {
    let s = Scratch::new();
    let err = s.refused(&["new", "topic", "x"]);
    assert!(err.contains("worklog init"), "{err}");
    let notice = s.ok(&["context"]);
    assert!(notice.contains("No store on this machine"), "{notice}");
    s.ok(&["init", "m1"]);
    let config = fs::read_to_string(s.root.path().join("config")).unwrap();
    assert_eq!(
        config,
        format!(
            "machine: m1\nstore: {}\n",
            s.root.path().join("store").display()
        )
    );
    let err = s.refused(&["init", "m2"]);
    assert!(err.contains("already named m1"), "{err}");
    let elsewhere = Scratch::new();
    let claude = elsewhere.agent_home(".claude");
    let codex = elsewhere.agent_home(".codex");
    let written = elsewhere.ok(&["init", "m3", "--store", "sync/worklog", "--agents"]);
    let config = fs::read_to_string(elsewhere.root.path().join("config")).unwrap();
    assert!(config.ends_with("/home/sync/worklog\n"), "{config}");
    for placed in [
        claude.join("skills/worklog/SKILL.md"),
        claude.join("settings.json"),
        codex.join("skills/worklog/SKILL.md"),
        codex.join("hooks.json"),
    ] {
        assert!(placed.is_file(), "{}", placed.display());
        assert!(written.contains(&placed.display().to_string()), "{written}");
    }
    let completions = elsewhere.ok(&["completions", "fish"]);
    assert!(completions.starts_with("COMPLETE=fish \""), "{completions}");
    assert!(completions.ends_with("\" | source\n"), "{completions}");
    // A scripted init without a choice touches nothing under ~/.claude.
    assert!(!s.home().join(".claude").exists());
    // No terminal here, so a nameless init cannot ask and is a usage error.
    assert_eq!(Scratch::new().run(&["init"]).status.code(), Some(2));
    assert_eq!(s.run(&["bogus"]).status.code(), Some(2));
    assert_eq!(s.run(&["show"]).status.code(), Some(2));
}

#[test]
fn agents_install_reaches_the_agents_that_are_present() {
    let s = Scratch::new();
    let codex = s.agent_home(".codex");
    let skill = codex.join("skills/worklog/SKILL.md");
    let hooks = codex.join("hooks.json");
    let written = s.ok(&["agents", "install"]);
    assert_eq!(
        written,
        format!("{}\n{}\n", skill.display(), hooks.display())
    );
    assert_eq!(fs::read_to_string(&skill).unwrap(), worklog::cli::SKILL);
    let text = fs::read_to_string(&hooks).unwrap();
    assert!(text.contains("\"SessionStart\""), "{text}");
    assert!(!s.home().join(".claude").exists());
    let again = s.ok(&["agents", "install"]);
    assert!(again.contains("already runs worklog context"), "{again}");
    assert_eq!(fs::read_to_string(&hooks).unwrap(), text);
}

#[test]
fn agents_refresh_brings_up_only_what_is_there() {
    let s = Scratch::new();
    let claude = s.agent_home(".claude");
    let codex = s.agent_home(".codex");
    assert_eq!(s.ok(&["agents", "refresh"]), "");
    assert!(!claude.join("skills").exists());
    let stale = claude.join("skills/worklog/SKILL.md");
    fs::create_dir_all(stale.parent().unwrap()).unwrap();
    fs::write(&stale, "an older skill\n").unwrap();
    let hooks = codex.join("hooks.json");
    fs::write(
        &hooks,
        r#"{"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "/gone/worklog context"}]}, {"hooks": [{"type": "command", "command": "date"}]}]}}"#,
    )
    .unwrap();
    let written = s.ok(&["agents", "refresh"]);
    assert_eq!(
        written,
        format!("{}\n{}\n", stale.display(), hooks.display())
    );
    assert_eq!(fs::read_to_string(&stale).unwrap(), worklog::cli::SKILL);
    let text = fs::read_to_string(&hooks).unwrap();
    assert!(!text.contains("/gone/"), "{text}");
    assert!(!codex.join("skills").exists());
    assert!(!claude.join("settings.json").exists());
    assert_eq!(s.ok(&["agents", "refresh"]), "");
    // Completions go where fish reads them, and only where fish is.
    let fish = s.home().join(".config/fish/completions");
    fs::create_dir_all(&fish).unwrap();
    let completions = fish.join("worklog.fish");
    assert_eq!(
        s.ok(&["agents", "refresh"]),
        format!("{}\n", completions.display())
    );
    assert!(
        fs::read_to_string(&completions)
            .unwrap()
            .starts_with("COMPLETE=fish ")
    );
}

#[test]
fn agents_uninstall_takes_back_only_what_install_placed() {
    let s = Scratch::new();
    let claude = s.agent_home(".claude");
    let codex = s.agent_home(".codex");
    assert_eq!(s.ok(&["agents", "uninstall"]), "");
    let settings = claude.join("settings.json");
    fs::write(&settings, "{\"model\": \"x\", \"hooks\": {\"Stop\": []}}\n").unwrap();
    let other_skill = claude.join("skills/other/SKILL.md");
    fs::create_dir_all(other_skill.parent().unwrap()).unwrap();
    fs::write(&other_skill, "theirs\n").unwrap();
    s.ok(&["agents", "install"]);
    let removed = s.ok(&["agents", "uninstall"]);
    assert_eq!(
        removed,
        format!(
            "{}\n{}\n{}\n{}\n",
            claude.join("skills/worklog").display(),
            settings.display(),
            codex.join("skills/worklog").display(),
            codex.join("hooks.json").display()
        )
    );
    assert!(!claude.join("skills/worklog").exists());
    assert_eq!(fs::read_to_string(&other_skill).unwrap(), "theirs\n");
    let text = fs::read_to_string(&settings).unwrap();
    assert!(!text.contains("worklog"), "{text}");
    assert!(text.contains("\"model\": \"x\""), "{text}");
    assert_eq!(
        fs::read_to_string(codex.join("hooks.json")).unwrap(),
        "{}\n"
    );
    assert_eq!(s.ok(&["agents", "uninstall"]), "");
    fs::write(&settings, "not json").unwrap();
    let err = s.refused(&["agents", "install"]);
    assert!(err.contains("not readable as JSON"), "{err}");
}

#[test]
fn installs_refuse_a_host_without_an_agent() {
    let s = Scratch::new();
    let err = s.refused(&["agents", "install"]);
    assert!(err.contains("no Claude Code or Codex home"), "{err}");
    let err = s.refused(&["init", "m1", "--agents"]);
    assert!(err.contains("no Claude Code or Codex home"), "{err}");
    assert!(!s.root.path().join("config").exists());
    assert!(!s.home().join(".claude").exists());
    assert!(!s.home().join(".codex").exists());
}

/// A release directory with one asset for this target under the tag.
fn published(dir: &Path, tag: &str, bytes: &[u8], checksum_of: &[u8]) {
    let asset = worklog::domain::release::asset().expect("a release is built for this target");
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join("latest"), format!("{tag}\n")).unwrap();
    fs::write(dir.join(asset), bytes).unwrap();
    fs::write(
        dir.join(format!("{asset}.sha256")),
        worklog::domain::release::checksum_file(asset, checksum_of),
    )
    .unwrap();
}

#[test]
fn upgrade_replaces_the_binary_only_with_a_newer_release() {
    let s = Scratch::new();
    let releases = s.root.path().join("releases");
    let built = assert_cmd::cargo::cargo_bin("worklog");
    let bytes = fs::read(&built).unwrap();
    let prefix = s.home().join(".local/bin");
    fs::create_dir_all(&prefix).unwrap();
    let installed = prefix.join("worklog");
    fs::copy(&built, &installed).unwrap();
    let fish = s.home().join(".config/fish/completions");
    fs::create_dir_all(&fish).unwrap();
    let current = worklog::domain::release::current().to_string();
    let upgrade = |args: &[&str]| {
        let mut command = Command::new(&installed);
        command.env("WORKLOG_RELEASES", &releases);
        s.run_binary(command, args)
    };

    // The one release that is fetched is the real binary, so the finish
    // runs a freshly replaced executable; the rest stop at the compare.
    published(&releases, "v9.9.9", &bytes, &bytes);
    let out = upgrade(&["upgrade", "--check"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("current: {current}\nlatest: 9.9.9\n")
    );
    let out = upgrade(&["upgrade"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!(
            "{}\n{}\n",
            installed.display(),
            fish.join("worklog.fish").display()
        )
    );
    let note = String::from_utf8_lossy(&out.stderr);
    assert!(
        note.contains(&format!("upgraded {current} to 9.9.9")),
        "{note}"
    );
    assert!(!prefix.join("worklog.new").exists());

    published(&releases, &format!("v{current}"), b"", b"");
    let out = upgrade(&["upgrade"]);
    assert!(out.status.success());
    let note = String::from_utf8_lossy(&out.stderr);
    assert!(note.contains("already at"), "{note}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}\n", fish.join("worklog.fish").display())
    );
    assert_eq!(upgrade(&["upgrade", "--check"]).status.code(), Some(0));

    published(&releases, "v9.9.9", b"tampered", b"");
    let out = upgrade(&["upgrade"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("checksum"), "{err}");
    assert_eq!(fs::read(&installed).unwrap(), bytes);
}

#[test]
fn every_run_is_logged_under_the_machine_that_ran_it() {
    let s = seeded();
    s.ok(&["facts", "lantern"]);
    s.ok(&["facts", "lantern"]);
    s.refused(&["show", "lantern/nothing"]);
    let counted = s.ok(&["usage"]);
    assert!(counted.starts_with("m1 — "), "{counted}");
    assert!(counted.contains("      2 facts\n"), "{counted}");
    let logs: Vec<PathBuf> = fs::read_dir(s.root.path().join("store/usage"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(logs.len(), 1, "one file per machine and month: {logs:?}");
    let lines = fs::read_to_string(&logs[0]).unwrap();
    let logged: Vec<Invocation> = lines.lines().filter_map(Invocation::parse_line).collect();
    let refused = logged
        .iter()
        .find(|i| i.command == "show")
        .expect("the refused run is logged");
    assert_eq!(refused.machine.as_str(), "m1");
    assert_eq!(refused.exit, 1);
    assert_eq!(refused.arguments, ["lantern/nothing"]);
    // A global flag belongs to the binary wherever it was typed.
    s.ok(&["--json", "topics"]);
    s.ok(&["topics", "--json"]);
    let lines = fs::read_to_string(&logs[0]).unwrap();
    let flagged: Vec<Vec<String>> = lines
        .lines()
        .filter_map(Invocation::parse_line)
        .filter(|i| i.command == "topics")
        .map(|i| i.arguments)
        .collect();
    assert_eq!(flagged, [["--json"], ["--json"]]);
    assert!(s.ok(&["usage", "--machine", "m2"]).is_empty());
    assert!(s.ok(&["usage", "--since", "2099-01-01"]).is_empty());
}

#[test]
fn a_synced_store_gets_an_ignore_file_on_the_first_write() {
    let s = seeded();
    let store = s.root.path().join("store");
    fs::create_dir(store.join(".stfolder")).unwrap();
    s.ok(&["topics"]);
    assert!(
        !store.join(".stignore").exists(),
        "a read stores no version"
    );
    s.ok(&["verify", "lantern/relay-pin-is-fixed"]);
    assert_eq!(
        fs::read_to_string(store.join(".stignore")).unwrap(),
        ".DS_Store\n.tmp-*\n"
    );
    fs::write(store.join(".stignore"), "mine\n").unwrap();
    s.ok(&["claim", "phone", "Documents"]);
    assert_eq!(
        fs::read_to_string(store.join(".stignore")).unwrap(),
        "mine\n"
    );
}

#[test]
fn store_layout_and_history() {
    let s = seeded();
    let dir = s.root.path().join("store/topic/lantern");
    let files: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].len(), 67);
    let history = s.ok(&["history", "lantern"]);
    assert!(history.contains("  m1  new  parents: none"), "{history}");
    let shown = s.ok(&["show", "lantern"]);
    assert_eq!(
        shown,
        "---\nsummary: A Rust app that dims a lamp\n---\n\nWhat to know first.\n"
    );
    assert_eq!(s.ok(&["drafts"]), "");
}

#[test]
fn context_indexes_the_directory_and_stays_small() {
    let s = seeded();
    let ctx = s.ok(&["context", "projects/Android/atlas"]);
    let expected = [
        "Durable facts and ideas, by name — `worklog facts <topic>` for what each",
        "claims, `worklog show <topic>/<name>` for one whole.",
        "",
        "atlas — this directory:",
        "  (no facts)",
        "",
        "android — via atlas:",
        "  (no facts)",
        "",
        "phone — via android:",
        "Ideas — unbuilt, kept with their settled design; opened like a fact:",
        "  beta-builds",
        "",
        "host — this machine:",
        "  (no facts)",
        "",
        "Not reached here, with fact counts — `worklog topics` says what each is:",
        "  lantern (1), personal (0)",
        "",
    ]
    .join("\n");
    assert_eq!(ctx, expected);
    assert!(ctx.len() < 2048);
    let ctx = s.ok(&["context", "projects/lantern"]);
    assert!(
        ctx.contains("lantern — this directory:\n  relay-pin-is-fixed\n"),
        "{ctx}"
    );
    assert!(
        ctx.contains("1 open follow-ups in 1 entries here, 0 without recheck"),
        "{ctx}"
    );
    assert!(
        ctx.contains("due now:\n- (due 2026-01-01) Add the second relay\n"),
        "{ctx}"
    );
    let ctx = s.ok(&["context", "Documents"]);
    assert!(ctx.contains("host — this machine:"), "{ctx}");
    assert!(ctx.contains("personal — unclaimed directory:"), "{ctx}");
    assert!(!ctx.contains("lantern — this directory"), "{ctx}");
}

#[test]
fn listings_keep_the_shapes_completions_parse() {
    let s = seeded();
    assert_eq!(
        s.ok(&["tags"]),
        "      2 lantern\n      1 phone\n      1 rust\n"
    );
    let topics = s.ok(&["topics"]);
    assert!(
        topics.starts_with("android — The Android toolchain\n"),
        "{topics}"
    );
    assert_eq!(s.ok(&["where", "lantern"]), "~/projects/lantern\n");
    let recent = s.ok(&["recent", "5"]);
    assert_eq!(
        recent,
        "● 2026-09-01  Wired the lamp driver\n  2026-09/2026-09-01-lamp-driver\n"
    );
    let facts = s.ok(&["facts", "atlas", "--deep"]);
    assert!(facts.starts_with("Ideas — unbuilt"), "{facts}");
    let json: serde_json::Value =
        serde_json::from_str(&s.ok(&["followups", "--json"])).expect("json");
    assert_eq!(json["open"], 1);
    assert_eq!(json["items"][0]["label"], "due 2026-01-01");
}

#[test]
fn a_shell_completes_slugs_and_topics_but_never_files() {
    let s = seeded();
    let complete = |args: &[&str]| {
        let mut command = Command::cargo_bin("worklog").expect("the binary");
        command.env("COMPLETE", "fish").arg("--").arg("worklog");
        s.ok_binary(command, args)
    };
    let topics = complete(&["facts", ""]);
    assert!(
        topics.contains("android\tThe Android toolchain\n"),
        "{topics}"
    );
    assert!(!topics.contains("Documents"), "{topics}");
    let slugs = complete(&["show", ""]);
    assert!(
        slugs.contains("2026-09/2026-09-01-lamp-driver\t"),
        "{slugs}"
    );
    assert!(slugs.contains("android\t"), "{slugs}");
    assert!(!complete(&["save", ""]).contains("android\t"));
    s.ok(&["checkout", "android"]);
    assert!(complete(&["save", ""]).contains("android\ttopic\n"));
    let followups = complete(&["done", ""]);
    assert!(followups.contains('\t'), "{followups}");
    assert!(!followups.contains("android\t"), "{followups}");
    let tags = complete(&["tag", "la"]);
    assert_eq!(tags, "lantern\t2\n");
    let term = complete(&["search", ""]);
    assert!(!term.contains("Documents"), "{term}");
    let dir = complete(&["context", "Doc"]);
    assert_eq!(dir, "Documents/\n");
}

#[test]
fn followup_lifecycle_and_check() {
    let s = seeded();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let slug = format!("{today}-port");
    s.ok(&["recheck", &slug, "touching", "lantern"]);
    let list = s.ok(&["followups", "lantern"]);
    assert!(
        list.contains("- (touching lantern) Add the second relay"),
        "{list}"
    );
    s.ok(&[
        "done",
        &slug,
        "dissolved by [[2026-09/2026-09-01-lamp-driver]]",
    ]);
    let err = s.refused(&["done", &slug]);
    assert!(err.contains("already done"), "{err}");
    let shown = s.ok(&["show", "2026-09/2026-09-01-lamp-driver"]);
    assert!(
        shown.ends_with(&format!(
            "## Follow-ups\n- [x] Add the second relay (touching lantern) — {slug}\n"
        )),
        "{shown}"
    );
    let run = s.run(&["followups"]);
    assert!(run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("no open follow-ups"));
    assert_eq!(
        s.ok(&["check"]),
        "check: 10 documents, 1 links, 0 problems, 0 forks, 0 notices\n"
    );
    s.ok(&["verify", "lantern/relay-pin-is-fixed"]);
    let shown = s.ok(&["show", "lantern/relay-pin-is-fixed"]);
    assert!(shown.contains(&format!("verified: {today}\n")), "{shown}");
}

#[test]
fn save_refuses_stale_unchanged_and_broken_drafts() {
    let s = seeded();
    let path = s.ok(&["checkout", "lantern"]);
    let path = Path::new(path.trim());
    assert!(path.ends_with("drafts/topic/lantern.md"));
    let err = s.refused(&["checkout", "lantern"]);
    assert!(err.contains("already exists"), "{err}");
    let err = s.refused(&["save", "lantern"]);
    assert!(err.contains("equals its parent"), "{err}");
    let text = fs::read_to_string(path).unwrap();
    fs::write(
        path,
        text.replace("summary: A Rust", "summary: A Rust\nscope: machine"),
    )
    .unwrap();
    let err = s.refused(&["save", "lantern"]);
    assert!(err.contains("scope"), "{err}");
    fs::write(
        path,
        text.replace("What to know first.", "What to know now."),
    )
    .unwrap();
    let diff = s.ok(&["diff", "lantern"]);
    assert!(
        diff.contains("-What to know first.\n+What to know now.\n"),
        "{diff}"
    );
    // Another machine's version lands before the save.
    let other = s.peer("m2");
    other.write(&["checkout", "lantern"], |t| {
        t.replace("What to know first.", "Written on m2.")
    });
    other.sync_to(&s);
    let err = s.refused(&["save", "lantern"]);
    assert!(err.contains("moved on"), "{err}");
    assert!(path.exists(), "the draft survives a refusal");
    s.ok(&["discard", "lantern"]);
    assert!(!path.exists());
    let shown = s.ok(&["show", "lantern"]);
    assert!(shown.contains("Written on m2."), "{shown}");
    // An id, or a prefix of one, names a stored version on its own.
    let history = s.ok(&["history", "lantern"]);
    let ids: Vec<&str> = history.lines().map(|l| column(l, 0)).collect();
    assert_eq!(ids.len(), 2);
    let first = s.ok(&["show", ids[1]]);
    assert!(first.contains("What to know first."), "{first}");
    let change = s.ok(&["diff", ids[0]]);
    assert!(
        change.contains("-What to know first.\n+Written on m2.\n"),
        "{change}"
    );
    let between = s.ok(&["diff", ids[0], ids[1]]);
    assert_eq!(
        between, change,
        "earlier on the left whichever is named first"
    );
    // A draft for a document the store does not hold yet diffs against nothing.
    s.ok(&["new", "fact", "lantern/fresh"]);
    let fresh = s.ok(&["diff", "lantern/fresh"]);
    assert!(
        fresh.starts_with("--- lantern/fresh (store)\n+++ lantern/fresh (draft)\n"),
        "{fresh}"
    );
    let err = s.refused(&["show", "lantern/fresh"]);
    assert!(err.contains("no fact: lantern/fresh"), "{err}");
    let log = s.ok(&["log", "100"]);
    assert!(
        log.lines().any(|l| l.starts_with(ids[0])),
        "log carries the id: {log}"
    );
    let err = s.run(&["diff", "lantern", ids[0]]);
    assert_eq!(err.status.code(), Some(2));
}

#[test]
fn a_fork_is_reported_everywhere_and_resolved_by_hand() {
    let s = seeded();
    let other = s.peer("m2");
    s.write(&["checkout", "lantern"], |t| {
        t.replace("first.", "first, says m1.")
    });
    other.write(&["checkout", "lantern"], |t| {
        t.replace("first.", "first, says m2.")
    });
    other.sync_to(&s);
    let forks = s.ok(&["forks"]);
    assert!(forks.starts_with("lantern: "), "{forks}");
    let err = s.refused(&["checkout", "lantern"]);
    assert!(err.contains("worklog resolve lantern"), "{err}");
    let shown = s.ok(&["show", "lantern"]);
    assert_eq!(shown.matches("==== head ").count(), 2);
    let ctx = s.ok(&["context", "projects/lantern"]);
    assert!(
        ctx.contains("Forked, needing `worklog resolve`: lantern"),
        "{ctx}"
    );
    let run = s.run(&["check"]);
    assert_eq!(run.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&run.stdout).contains("lantern: forked"));
    let path = s.ok(&["resolve", "lantern"]);
    let text = fs::read_to_string(path.trim()).unwrap();
    assert!(text.contains("<<<<<<< ") && text.contains("======= ") && text.contains(">>>>>>>"));
    let err = s.refused(&["save", "lantern"]);
    assert!(err.contains("conflict markers"), "{err}");
    let body_start = text[4..].find("---\n").unwrap() + 8;
    let resolved = format!("{}What to know first, say both.\n", &text[..body_start]);
    fs::write(path.trim(), resolved).unwrap();
    s.ok(&["save", "lantern"]);
    assert_eq!(s.ok(&["forks"]), "");
    let history = s.ok(&["history", "lantern"]);
    assert!(
        history
            .lines()
            .next()
            .unwrap()
            .contains("resolve  parents:"),
        "{history}"
    );
    assert_eq!(history.lines().count(), 4);
}

#[test]
fn claims_are_commands_on_the_machine_topic() {
    let s = seeded();
    let ctx = s.ok(&["context", "Documents"]);
    assert!(!ctx.contains("phone — this directory"), "{ctx}");
    s.ok(&["claim", "phone", "Documents"]);
    assert_eq!(s.ok(&["where", "phone"]), "~/Documents\n");
    let ctx = s.ok(&["context", "Documents"]);
    assert!(ctx.contains("phone — this directory:"), "{ctx}");
    let err = s.refused(&["claim", "phone", "Documents"]);
    assert!(err.contains("already claims"), "{err}");
    fs::remove_dir(s.home().join("Documents")).unwrap();
    assert_eq!(s.ok(&["where", "phone"]), "~/Documents (missing)\n");
    let all = s.ok(&["where"]);
    assert!(all.contains("phone    ~/Documents (missing)\n"), "{all}");
    assert!(all.contains("lantern  ~/projects/lantern\n"), "{all}");
    let elsewhere = s.ok(&["where", "--machine", "m1"]);
    assert!(!elsewhere.contains("missing"), "{elsewhere}");
    s.ok(&["unclaim", "phone", "Documents"]);
    let err = s.refused(&["unclaim", "phone", "Documents"]);
    assert!(err.contains("does not claim"), "{err}");
    let history = s.ok(&["history", "host"]);
    assert!(
        history.lines().next().unwrap().contains("unclaim"),
        "{history}"
    );
    let log = s.ok(&["log", "2"]);
    assert_eq!(log.lines().count(), 2, "{log}");
    assert!(
        log.lines().next().unwrap().ends_with("  m1  unclaim  host"),
        "{log}"
    );
    assert!(s.ok(&["log", "--machine", "nobody"]).is_empty());
}

#[test]
fn rename_and_tombstone() {
    let s = seeded();
    s.write(
        &["new", "entry", "relay-notes", "--date", "2026-09-02"],
        |t| set_summary(t, "Read up on the relay") + "See [[lantern/relay-pin-is-fixed]].\n",
    );
    s.ok(&["verify", "lantern/relay-pin-is-fixed"]);
    let out = s.ok(&["rename", "lantern/relay-pin-is-fixed", "lantern/relay-pin"]);
    assert!(
        out.contains("moved to lantern/relay-pin; the old slug's tombstone is "),
        "{out}"
    );
    // The old name reads as the new document, and a link to it still lands.
    assert_eq!(
        s.ok(&["show", "lantern/relay-pin-is-fixed"]),
        s.ok(&["show", "lantern/relay-pin"])
    );
    assert!(s.ok(&["check"]).contains("0 problems"));
    // Either name's history is the whole chain, with the rows written
    // under the other name marked.
    let history = s.ok(&["history", "lantern/relay-pin"]);
    let lines: Vec<&str> = history.lines().collect();
    let operations: Vec<&str> = lines.iter().map(|l| column(l, 3)).collect();
    assert_eq!(
        operations,
        ["rename", "rename", "verify", "new"],
        "{history}"
    );
    assert!(!lines[0].contains("  as "), "{history}");
    assert!(
        lines[1..]
            .iter()
            .all(|l| l.ends_with("  as lantern/relay-pin-is-fixed")),
        "{history}"
    );
    let history = s.ok(&["history", "lantern/relay-pin-is-fixed"]);
    let lines: Vec<&str> = history.lines().collect();
    assert!(lines[0].ends_with("  as lantern/relay-pin"), "{history}");
    assert!(lines[1..].iter().all(|l| !l.contains("  as ")), "{history}");
    // Either half of the rename diffs as the move, not as the body leaving.
    for line in &lines[..2] {
        let diff = s.ok(&["diff", column(line, 0)]);
        assert!(
            diff.ends_with("\nrenamed from lantern/relay-pin-is-fixed to lantern/relay-pin\n"),
            "{diff}"
        );
        assert_eq!(diff.lines().count(), 3, "{diff}");
    }
    let err = s.refused(&["checkout", "lantern/relay-pin-is-fixed"]);
    assert!(err.contains("renamed to lantern/relay-pin"), "{err}");
    // A tombstone says why. A link to a removed document lands on it: an
    // entry's link is a citation, a fact's is a notice.
    assert_eq!(
        s.run(&["tombstone", "lantern/relay-pin", " "])
            .status
            .code(),
        Some(2)
    );
    s.ok(&[
        "tombstone",
        "lantern/relay-pin",
        "moved into the board's README",
    ]);
    assert_eq!(
        s.ok(&["show", "lantern/relay-pin-is-fixed"]),
        "lantern/relay-pin was removed: moved into the board's README\n"
    );
    let json = s.ok(&["show", "--json", "lantern/relay-pin"]);
    assert!(
        json.contains("\"removed\": \"moved into the board's README\""),
        "{json}"
    );
    s.write(&["new", "fact", "lantern/relay-timing"], |t| {
        set_summary(t, "The relay settles in a millisecond") + "After [[lantern/relay-pin]].\n"
    });
    let check = s.ok(&["check"]);
    assert!(
        check.contains(
            "notice: lantern/relay-timing: links a removed document: [[lantern/relay-pin]]\n"
        ),
        "{check}"
    );
    assert!(
        check.ends_with("0 problems, 0 forks, 1 notices\n"),
        "{check}"
    );
    let err = s.refused(&["new", "fact", "lantern/relay-pin"]);
    assert!(err.contains("never reused"), "{err}");
    let facts = s.ok(&["facts", "lantern"]);
    assert!(
        facts.contains("relay-timing") && !facts.contains("relay-pin\n"),
        "{facts}"
    );
}

#[test]
fn a_version_from_a_newer_worklog_reads_and_refuses_edits() {
    use worklog::domain::version::Version;
    let s = seeded();
    // The stored fact, as a newer worklog would have written a sibling.
    let dir = s.root.path().join("store/fact/lantern/relay-pin-is-fixed");
    let stored = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    let text = fs::read_to_string(stored)
        .unwrap()
        .replace("relay-pin-is-fixed", "from-the-future")
        .replace("  operation: new\n", "  hue: 3\n  operation: new\n");
    let foreign = Version::from_text(&text).unwrap();
    let dir = s.root.path().join("store/fact/lantern/from-the-future");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{}.md", foreign.id)), text).unwrap();
    // Reads work and say so; the document lists like any other.
    let run = s.run(&["show", "lantern/from-the-future"]);
    assert!(run.status.success());
    let err = String::from_utf8_lossy(&run.stderr);
    assert!(err.contains("version field `hue`"), "{err}");
    assert!(s.ok(&["facts", "lantern"]).contains("from-the-future"));
    let check = s.ok(&["check"]);
    assert!(
        check.contains("notice: lantern/from-the-future:"),
        "{check}"
    );
    // A write on it is refused; one elsewhere is not.
    let err = s.refused(&["checkout", "lantern/from-the-future"]);
    assert!(err.contains("upgrade to change it"), "{err}");
    s.ok(&["verify", "lantern/relay-pin-is-fixed"]);
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else if !target.exists() {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn help_lists_every_command_under_its_heading() {
    let scratch = Scratch::new();
    let text = scratch.ok(&["--help"]);
    let section = |heading: &str| {
        let start = text
            .find(&format!("\n{heading}:\n"))
            .unwrap_or_else(|| panic!("{heading} in {text}"));
        let body = &text[start + heading.len() + 2..];
        body[..body.find("\n\n").unwrap_or(body.len())].to_string()
    };
    assert!(section("Setup").contains("\n  init "));
    assert!(section("Reads").contains("\n  show "));
    assert!(section("Writes").contains("\n  new "));
    assert!(section("Other").contains("\n  serve "));
    assert_eq!(text.matches("\n  show ").count(), 1);
}
