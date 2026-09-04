//! The binary over a scratch store: exit codes, stdout shapes and the
//! write path end to end.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;

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

    fn run(&self, args: &[&str]) -> Output {
        Command::cargo_bin("worklog")
            .expect("the binary")
            .env("WORKLOG_HOME", self.root.path())
            .env("HOME", self.home())
            .current_dir(self.home())
            .args(args)
            .output()
            .expect("the binary runs")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
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
    let written = elsewhere.ok(&["init", "m3", "--store", "sync/worklog", "--skill", "--hook"]);
    let config = fs::read_to_string(elsewhere.root.path().join("config")).unwrap();
    assert!(config.ends_with("/home/sync/worklog\n"), "{config}");
    let agent = elsewhere.home().join(".claude");
    let skill = fs::read_to_string(agent.join("skills/worklog/SKILL.md")).unwrap();
    assert_eq!(skill, worklog::cli::SKILL);
    assert_eq!(elsewhere.ok(&["skill", "show"]), skill);
    let completions = elsewhere.ok(&["completions", "fish"]);
    assert!(completions.contains("complete -c worklog"), "{completions}");
    assert!(completions.contains("unclaim"), "{completions}");
    let settings = fs::read_to_string(agent.join("settings.json")).unwrap();
    assert!(settings.contains("\"SessionStart\""), "{settings}");
    assert!(written.contains("settings.json"), "{written}");
    // A scripted init without a choice touches nothing under ~/.claude.
    assert!(!s.home().join(".claude").exists());
    // No terminal here, so a nameless init cannot ask and is a usage error.
    assert_eq!(Scratch::new().run(&["init"]).status.code(), Some(2));
    assert_eq!(s.run(&["bogus"]).status.code(), Some(2));
    assert_eq!(s.run(&["show"]).status.code(), Some(2));
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
        "check: 10 documents, 1 links, 0 problems, 0 forks\n"
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
    // Everything here is stamped within a second, so only membership holds.
    let log = s.ok(&["log", "2"]);
    assert_eq!(log.lines().count(), 2, "{log}");
    let log = s.ok(&["log", "100"]);
    assert!(
        log.lines().any(|l| l.ends_with("  m1  unclaim  host")),
        "{log}"
    );
    assert!(s.ok(&["log", "--machine", "nobody"]).is_empty());
}

#[test]
fn rename_and_tombstone() {
    let s = seeded();
    let out = s.ok(&["rename", "lantern/relay-pin-is-fixed", "lantern/relay-pin"]);
    assert!(
        out.contains("moved to lantern/relay-pin; the old slug's tombstone is "),
        "{out}"
    );
    let err = s.refused(&["show", "lantern/relay-pin-is-fixed"]);
    assert!(err.contains("renamed to lantern/relay-pin"), "{err}");
    s.ok(&["show", "lantern/relay-pin"]);
    s.ok(&["tombstone", "lantern/relay-pin"]);
    let err = s.refused(&["new", "fact", "lantern/relay-pin"]);
    assert!(err.contains("never reused"), "{err}");
    let facts = s.run(&["facts", "lantern"]);
    assert!(facts.stdout.is_empty());
    assert!(String::from_utf8_lossy(&facts.stderr).contains("no facts for: lantern"));
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
