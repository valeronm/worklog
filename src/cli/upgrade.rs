use super::agents::{self, Done};
use super::render::{Output, Render, Stopped, output};
use super::{Environment, refresh};
use crate::app::upgrade::{self, Checked, Installed, Outcome, Upgrade};

impl Render for Checked {
    fn text(&self) -> String {
        format!("current: {}\nlatest: {}\n", self.current, self.latest)
    }
}

impl Render for Upgrade {
    fn text(&self) -> String {
        agents::lines(&self.written)
    }

    fn notes(&self) -> Vec<String> {
        let (current, latest) = (self.current, self.latest);
        vec![match self.outcome {
            Outcome::Current => format!("already at {current}"),
            Outcome::Ahead => format!("{current} is newer than the latest release {latest}"),
            Outcome::Upgraded => format!("upgraded {current} to {latest}"),
        }]
    }
}

pub(super) fn run(check: bool, environment: &Environment, json: bool) -> Result<Output, Stopped> {
    let releases = (environment.releases)();
    if check {
        let (checked, _) = upgrade::check(releases.as_ref(), environment.version)?;
        return Ok(output(&checked, json));
    }
    let binary = (environment.binary_at)(environment.binary()?);
    let (checked, tag) = upgrade::check(releases.as_ref(), environment.version)?;
    let (done, ended) = if checked.newer {
        let Installed {
            written,
            notes,
            ended,
        } = upgrade::install(releases.as_ref(), binary.as_ref(), &tag)?;
        (Done { written, notes }, ended)
    } else {
        let mut done = Done::default();
        let ended = refresh::refresh_into(environment, &mut done);
        (done, ended)
    };
    done.ended(ended, json, |written| checked.upgrade(written))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use serde_json::{Value, json};

    use super::super::NAME;
    use super::super::skill::SKILL;
    use super::super::tests::Process;
    use super::*;
    use crate::domain::ports::{Binary, Releases, StoreError};
    use crate::domain::testing::{
        ASSET, BINARY, MemoryBinary, MemoryReleases, REFRESHED, build, published, version,
    };
    use crate::wiring::SKILL_DIRECTORY;

    #[test]
    fn a_check_prints_both_versions_and_exits_zero_whichever_is_newer() {
        let checked = |current: &str, latest: &str| Checked {
            current: version(current).into(),
            latest: version(latest),
            newer: version(latest) > version(current),
        };
        let newer = checked("0.2.1", "0.3.0");
        assert_eq!(newer.text(), "current: 0.2.1\nlatest: 0.3.0\n");
        assert_eq!((newer.exit(), newer.notes().len()), (0, 0));
        assert_eq!(checked("0.3.0", "0.3.0").exit(), 0);
        let ahead = checked("0.4.0", "0.3.0");
        assert_eq!(ahead.text(), "current: 0.4.0\nlatest: 0.3.0\n");
        assert_eq!(ahead.exit(), 0);
    }

    #[test]
    fn an_upgrade_prints_what_was_written_and_notes_what_became_of_the_binary() {
        let done = |outcome: Outcome, written: &[&str]| Upgrade {
            current: version("0.2.1").into(),
            latest: version("0.3.0"),
            outcome,
            written: written.iter().map(|path| (*path).to_owned()).collect(),
        };
        let upgraded = done(Outcome::Upgraded, &["/desk/bin/lantern", "/desk/skill"]);
        assert_eq!(upgraded.text(), "/desk/bin/lantern\n/desk/skill\n");
        assert_eq!(upgraded.notes(), ["upgraded 0.2.1 to 0.3.0"]);
        assert_eq!(upgraded.exit(), 0);
        let kept = Upgrade {
            current: build("0.3.0"),
            ..done(Outcome::Ahead, &[])
        };
        assert_eq!(
            kept.notes(),
            ["0.3.0+3f2a1c9 is newer than the latest release 0.3.0"]
        );
        let current = done(Outcome::Current, &[]);
        assert_eq!(current.text(), "");
        assert_eq!(current.notes(), ["already at 0.2.1"]);
        let ahead = done(Outcome::Ahead, &["/desk/skill"]);
        assert_eq!(ahead.text(), "/desk/skill\n");
        assert_eq!(
            ahead.notes(),
            ["0.2.1 is newer than the latest release 0.3.0"]
        );
    }

    struct Asked {
        held: MemoryReleases,
        for_the_latest: Rc<Cell<usize>>,
    }

    impl Releases for Asked {
        fn latest(&self) -> Result<String, StoreError> {
            self.for_the_latest.set(self.for_the_latest.get() + 1);
            self.held.latest()
        }

        fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, StoreError> {
            self.held.fetch(tag, asset)
        }
    }

    struct Installed {
        _kept: tempfile::TempDir,
        root: PathBuf,
        memory: Rc<MemoryBinary>,
        asked_for_the_latest: Rc<Cell<usize>>,
    }

    #[derive(Clone, Copy)]
    struct Given<'a> {
        running: &'a str,
        tag: &'a str,
        checksum_of: &'a [u8],
    }

    const NEWER: Given = Given {
        running: "0.2.1",
        tag: "v0.3.0",
        checksum_of: b"new",
    };
    const SAME: Given = Given {
        running: "0.3.0",
        ..NEWER
    };
    const OLDER: Given = Given {
        running: "0.4.0",
        ..NEWER
    };

    type Ran = (String, String, i32);

    impl Installed {
        fn with(memory: MemoryBinary) -> Installed {
            let kept = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(kept.path()).unwrap();
            let host = Installed {
                _kept: kept,
                root,
                memory: Rc::new(memory),
                asked_for_the_latest: Rc::default(),
            };
            let skill = host.skill();
            std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
            std::fs::write(skill, "old").unwrap();
            let store = host.root.join("store");
            let set_up = Process {
                home: Some(host.root.join("home")),
                ..Process::nowhere()
            };
            let made = set_up.run(&["init", "desk", "--store", store.to_str().unwrap()]);
            assert_eq!(made.code, 0, "{}", made.err);
            host
        }

        fn new() -> Installed {
            Installed::with(MemoryBinary::built_as(ASSET))
        }

        fn whose_refresh_stops() -> Installed {
            Installed::with(MemoryBinary {
                stops: Some(StoreError::io("lantern refresh", "no agents here")),
                ..MemoryBinary::built_as(ASSET)
            })
        }

        fn binary(&self) -> PathBuf {
            self.root.join("bin").join(NAME)
        }

        fn skill(&self) -> PathBuf {
            let skills = self.root.join("home/.claude/skills");
            skills.join(SKILL_DIRECTORY).join("SKILL.md")
        }

        fn hooks(&self) -> PathBuf {
            self.root.join("home/.claude/settings.json")
        }

        fn fish(&self) -> PathBuf {
            let dir = self.root.join("home/.config/fish/completions");
            std::fs::create_dir_all(&dir).unwrap();
            dir.join(format!("{NAME}.fish"))
        }

        fn run_as(&self, given: Given, binary: Option<&Path>, line: &[&str]) -> Ran {
            let source = || -> Box<dyn Releases> {
                Box::new(Asked {
                    held: published(given.tag, ASSET, b"new", given.checksum_of),
                    for_the_latest: Rc::clone(&self.asked_for_the_latest),
                })
            };
            let memory = |_: &Path| -> Box<dyn Binary> { Box::new(Rc::clone(&self.memory)) };
            let process = Process {
                home: Some(self.root.join("home")),
                binary: binary.map(Path::to_path_buf),
                version: version(given.running).into(),
                releases: &source,
                binary_at: &memory,
                ..Process::nowhere()
            };
            let ran = process.run(line);
            (ran.out, ran.err, ran.code)
        }

        fn run(&self, given: Given, line: &[&str]) -> Ran {
            self.run_as(given, Some(&self.binary()), line)
        }

        fn json(&self, given: Given, line: &[&str]) -> (Value, String, i32) {
            let (out, err, code) = self.run(given, line);
            (serde_json::from_str(&out).expect("one value"), err, code)
        }

        fn untouched(&self) -> bool {
            self.memory.calls.borrow().is_empty()
        }
    }

    fn said(why: &str) -> String {
        format!("{NAME}: {why}\n")
    }

    fn path(file: &Path) -> String {
        file.display().to_string()
    }

    #[test]
    fn upgrade_check_compares_with_the_release_published_and_replaces_nothing() {
        let host = Installed::new();
        let check = ["upgrade", "--check"];
        let both = "current: 0.2.1\nlatest: 0.3.0\n".to_owned();
        assert_eq!(host.run(NEWER, &check), (both, String::new(), 0));
        assert_eq!(host.run_as(NEWER, None, &check), host.run(NEWER, &check));
        let found = |given: Given, newer: bool| {
            let value = json!({"current": given.running, "latest": "0.3.0", "newer": newer});
            (value, String::new(), 0)
        };
        let asked = ["upgrade", "--check", "--json"];
        assert_eq!(host.json(NEWER, &asked), found(NEWER, true));
        assert_eq!(host.json(SAME, &asked), found(SAME, false));
        assert_eq!(host.json(OLDER, &asked), found(OLDER, false));
        let both = "current: 0.4.0\nlatest: 0.3.0\n".to_owned();
        assert_eq!(host.run(OLDER, &check), (both, String::new(), 0));

        let untagged = Given {
            tag: "nightly",
            ..NEWER
        };
        let refused = said("`nightly` is not a version of the form X.Y.Z");
        for line in [&check[..], &["upgrade"]] {
            assert_eq!(
                host.run(untagged, line),
                (String::new(), refused.clone(), 1)
            );
        }
        assert!(host.untouched());
        assert_eq!(std::fs::read_to_string(host.skill()).unwrap(), "old");
    }

    #[test]
    fn an_upgrade_to_a_newer_release_replaces_the_binary_and_has_it_refresh_the_host() {
        let host = Installed::new();
        let written = format!("{BINARY}\n{REFRESHED}\n");
        assert_eq!(
            host.run(NEWER, &["upgrade"]),
            (written, said("upgraded 0.2.1 to 0.3.0"), 0)
        );
        let replaced = host.memory.replaced_with.borrow();
        assert_eq!(replaced.as_deref(), Some(&b"new"[..]));
        assert_eq!(*host.memory.calls.borrow(), ["replace", "refresh"]);
        assert_eq!(std::fs::read_to_string(host.skill()).unwrap(), "old");

        let again = Installed::new();
        let expected = json!({
            "current": "0.2.1",
            "latest": "0.3.0",
            "outcome": "upgraded",
            "written": [BINARY, REFRESHED],
        });
        assert_eq!(
            again.json(NEWER, &["upgrade", "--json"]),
            (expected, String::new(), 0)
        );
    }

    #[test]
    fn an_upgraded_binary_whose_refresh_fails_is_said_to_be_upgraded_before_the_failure() {
        let failure = said("lantern refresh: no agents here");
        let host = Installed::whose_refresh_stops();
        let written = format!("{BINARY}\n{REFRESHED}\n");
        let noted = said("upgraded 0.2.1 to 0.3.0") + &failure;
        assert_eq!(host.run(NEWER, &["upgrade"]), (written, noted, 1));
        assert_eq!(*host.memory.calls.borrow(), ["replace", "refresh"]);

        let again = Installed::whose_refresh_stops();
        let known = json!({
            "current": "0.2.1",
            "latest": "0.3.0",
            "outcome": "upgraded",
            "written": [BINARY, REFRESHED],
        });
        assert_eq!(
            again.json(NEWER, &["upgrade", "--json"]),
            (known, failure, 1)
        );
    }

    #[test]
    fn the_latest_release_is_asked_for_once_whatever_becomes_of_the_binary() {
        for (given, line) in [
            (NEWER, &["upgrade"][..]),
            (SAME, &["upgrade"]),
            (OLDER, &["upgrade", "--json"]),
            (NEWER, &["upgrade", "--check"]),
        ] {
            let host = Installed::new();
            host.run(given, line);
            assert_eq!(host.asked_for_the_latest.get(), 1, "{line:?}");
        }
    }

    #[test]
    fn an_upgrade_to_a_release_that_is_not_newer_refreshes_the_host_from_this_binary() {
        for (given, note) in [
            (SAME, "already at 0.3.0"),
            (OLDER, "0.4.0 is newer than the latest release 0.3.0"),
        ] {
            let host = Installed::new();
            let written = format!("{}\n", host.skill().display());
            assert_eq!(host.run(given, &["upgrade"]), (written, said(note), 0));
            assert_eq!(std::fs::read_to_string(host.skill()).unwrap(), SKILL);
            assert!(host.untouched());

            std::fs::write(host.skill(), "old").unwrap();
            let outcome = if given.running == "0.3.0" {
                "current"
            } else {
                "ahead"
            };
            let expected = json!({
                "current": given.running,
                "latest": "0.3.0",
                "outcome": outcome,
                "written": [host.skill().display().to_string()],
            });
            assert_eq!(
                host.json(given, &["upgrade", "--json"]),
                (expected, String::new(), 0)
            );
            assert!(host.untouched());
        }
    }

    #[test]
    fn what_became_of_a_binary_that_stays_is_said_before_its_refresh_fails() {
        for (given, note) in [
            (SAME, "already at 0.3.0"),
            (OLDER, "0.4.0 is newer than the latest release 0.3.0"),
        ] {
            let host = Installed::new();
            std::fs::write(host.hooks(), "[]").unwrap();
            let (out, err, code) = host.run(given, &["upgrade"]);
            assert_eq!((out.as_str(), code), ("", 1));
            let lines: Vec<&str> = err.lines().collect();
            assert_eq!(lines.len(), 2, "{err}");
            assert_eq!(format!("{}\n", lines[0]), said(note));
            let named = format!("{NAME}: {}", host.hooks().display());
            assert!(lines[1].starts_with(&named), "{err}");

            let (out, err, code) = host.json(given, &["upgrade", "--json"]);
            let outcome = if given.running == "0.3.0" {
                "current"
            } else {
                "ahead"
            };
            let known = json!({
                "current": given.running,
                "latest": "0.3.0",
                "outcome": outcome,
                "written": [],
            });
            assert_eq!((out, code), (known, 1));
            assert_eq!(err.lines().collect::<Vec<_>>(), [lines[1]]);
            assert_eq!(std::fs::read_to_string(host.skill()).unwrap(), "old");
        }
    }

    #[test]
    fn a_refresh_that_fails_before_it_wrote_prints_under_json_a_value_with_nothing_written() {
        let host = Installed::new();
        std::fs::write(host.hooks(), "[]").unwrap();
        let named = format!("{NAME}: {}", host.hooks().display());
        let (out, err, code) = host.run(SAME, &["refresh"]);
        assert_eq!((out.as_str(), code), ("", 1));
        assert!(err.starts_with(&named) && err.lines().count() == 1, "{err}");
        let (out, said, code) = host.json(SAME, &["refresh", "--json"]);
        assert_eq!((out, said, code), (json!({"written": []}), err, 1));
    }

    #[test]
    fn what_a_refresh_noted_before_it_failed_is_noted_though_it_wrote_nothing() {
        let host = Installed::new();
        host.fish();
        std::fs::remove_file(host.skill()).unwrap();
        std::fs::create_dir(host.skill()).unwrap();
        let renamed = host.root.join("bin").join(format!("{NAME}-0.3.0"));
        let skipped = said(&format!(
            "{}: fish's completions are not written, since the binary's file name is not {NAME}",
            renamed.display()
        ));
        let failed = |err: &str, before: &[&str]| {
            let lines: Vec<String> = err.lines().map(|line| format!("{line}\n")).collect();
            let (last, first) = lines.split_last().expect("a failure");
            assert_eq!(first, before, "{err}");
            let named = format!("{NAME}: {}: ", host.skill().display());
            assert!(last.starts_with(&named), "{err}");
        };

        let (out, err, code) = host.run_as(SAME, Some(&renamed), &["refresh"]);
        assert_eq!((out.as_str(), code), ("", 1));
        failed(&err, &[&skipped]);
        let line = ["refresh", "--json"];
        let (out, err, code) = host.run_as(SAME, Some(&renamed), &line);
        assert_eq!((out.as_str(), code), ("{\n  \"written\": []\n}\n", 1));
        failed(&err, &[&skipped]);
        let (out, err, code) = host.run_as(SAME, Some(&renamed), &["upgrade"]);
        assert_eq!((out.as_str(), code), ("", 1));
        failed(&err, &[&said("already at 0.3.0"), &skipped]);
        let (out, err, code) = host.run_as(SAME, Some(&renamed), &["upgrade", "--json"]);
        let value: Value = serde_json::from_str(&out).expect("one value");
        assert_eq!((&value["written"], code), (&json!([]), 1));
        failed(&err, &[&skipped]);
    }

    #[test]
    fn what_the_new_binary_s_refresh_noted_is_noted_after_the_upgrade() {
        let noting = || {
            Installed::with(MemoryBinary {
                notes: vec!["fish left out".to_owned()],
                ..MemoryBinary::built_as(ASSET)
            })
        };
        let written = format!("{BINARY}\n{REFRESHED}\n");
        let noted = said("upgraded 0.2.1 to 0.3.0") + &said("fish left out");
        assert_eq!(noting().run(NEWER, &["upgrade"]), (written, noted, 0));
        let (out, err, code) = noting().json(NEWER, &["upgrade", "--json"]);
        assert_eq!(out["outcome"], "upgraded");
        assert_eq!((err, code), (said("fish left out"), 0));
    }

    #[test]
    fn a_refresh_that_fails_after_it_wrote_says_what_it_wrote_before_the_failure() {
        let stale = || {
            let host = Installed::new();
            std::fs::create_dir(host.fish()).unwrap();
            host
        };
        let skill = |host: &Installed| path(&host.skill());
        let failed = |host: &Installed, err: &str, before: &[String]| {
            let lines: Vec<String> = err.lines().map(|line| format!("{line}\n")).collect();
            let (last, first) = lines.split_last().expect("a failure");
            assert_eq!(first, before, "{err}");
            let named = format!("{NAME}: {}: ", host.fish().display());
            assert!(last.starts_with(&named), "{err}");
            assert_eq!(std::fs::read_to_string(host.skill()).unwrap(), SKILL);
        };

        let host = stale();
        let (out, err, code) = host.run(SAME, &["refresh"]);
        assert_eq!((out, code), (format!("{}\n", skill(&host)), 1));
        failed(&host, &err, &[]);

        let host = stale();
        let (out, err, code) = host.json(SAME, &["refresh", "--json"]);
        assert_eq!((out, code), (json!({"written": [skill(&host)]}), 1));
        failed(&host, &err, &[]);

        let host = stale();
        let (out, err, code) = host.run(SAME, &["upgrade"]);
        assert_eq!((out, code), (format!("{}\n", skill(&host)), 1));
        failed(&host, &err, &[said("already at 0.3.0")]);

        let host = stale();
        let (out, err, code) = host.json(SAME, &["upgrade", "--json"]);
        let known = json!({
            "current": "0.3.0",
            "latest": "0.3.0",
            "outcome": "current",
            "written": [skill(&host)],
        });
        assert_eq!((out, code), (known, 1));
        failed(&host, &err, &[]);
    }

    #[test]
    fn an_upgrade_that_is_refused_replaces_nothing_and_refreshes_nothing() {
        let host = Installed::new();
        let tampered = Given {
            checksum_of: b"other",
            ..NEWER
        };
        let mismatch = said(&format!("{ASSET}: the checksum does not match the release"));
        assert_eq!(
            host.run(tampered, &["upgrade"]),
            (String::new(), mismatch, 1)
        );

        let unknown = said("the path of this binary is unknown");
        for given in [NEWER, SAME] {
            assert_eq!(
                host.run_as(given, None, &["upgrade"]),
                (String::new(), unknown.clone(), 1)
            );
        }
        assert_eq!(host.asked_for_the_latest.get(), 1);
        assert!(host.untouched());
        assert_eq!(std::fs::read_to_string(host.skill()).unwrap(), "old");

        let unbuilt = Installed::with(MemoryBinary::default());
        let none = said("no release is built for this operating system and architecture");
        assert_eq!(unbuilt.run(NEWER, &["upgrade"]), (String::new(), none, 1));
        assert!(unbuilt.untouched());
    }
}
