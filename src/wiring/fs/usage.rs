use std::fs;
use std::io::{ErrorKind, Write as _};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::id::DocumentId;
use crate::domain::ports::{StoreError, Usage, UsageRead};
use crate::domain::usage::{Invocation, Kept, Session};
use crate::domain::version::Stamp;

use super::{files, io_error};

pub const USAGE_DIRECTORY: &str = "usage";

const EXTENSION: &str = ".jsonl";

/// One JSON object to a line with every key present, `null` under a key the run has nothing
/// for; a reader passes over a key it does not know.
#[derive(Serialize, Deserialize)]
struct Line {
    written: String,
    machine: String,
    version: Option<String>,
    agent: Option<String>,
    session: Option<String>,
    command: String,
    exit: i32,
    refusal: Option<String>,
    hits: Option<usize>,
    directory: String,
    arguments: Vec<String>,
}

fn line(invocation: &Invocation) -> String {
    let session = invocation.session.as_ref();
    let line = Line {
        written: invocation.written.to_string(),
        machine: invocation.machine.to_string(),
        version: invocation.version.clone(),
        agent: session.map(|session| session.agent.clone()),
        session: session.map(|session| session.id.clone()),
        command: invocation.command.clone(),
        exit: invocation.exit,
        refusal: invocation.refusal.clone(),
        hits: invocation.hits,
        directory: invocation.directory.clone(),
        arguments: invocation.arguments.clone(),
    };
    serde_json::to_string(&line).expect("a line with text for keys")
}

fn read(line: &str) -> Option<Invocation> {
    let line: Line = serde_json::from_str(line).ok()?;
    Some(Invocation {
        written: Stamp::parse(&line.written).ok()?,
        machine: DocumentId::parse(&line.machine).ok()?,
        version: line.version,
        session: (line.agent.zip(line.session)).map(|(agent, id)| Session { agent, id }),
        command: line.command,
        exit: line.exit,
        refusal: line.refusal,
        hits: line.hits,
        directory: line.directory,
        arguments: line.arguments,
    })
}

// A sync's conflict copy of a file ends as the file does.
fn filed(name: &str) -> Option<(DocumentId, &str)> {
    let (machine, month) = name.strip_suffix(EXTENSION)?.split_once('-')?;
    let digits = |part: &str, count: usize| {
        part.len() == count && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    let (year, of_year) = month.split_once('-')?;
    let shaped = digits(year, 4) && digits(of_year, 2);
    shaped.then_some((DocumentId::parse(machine).ok()?, month))
}

pub struct FsUsage {
    dir: PathBuf,
}

impl FsUsage {
    #[must_use]
    pub fn new(dir: PathBuf) -> FsUsage {
        FsUsage { dir }
    }
}

impl UsageRead for FsUsage {
    fn read(&self, kept: &Kept) -> Result<Vec<Invocation>, StoreError> {
        let names = match files(&self.dir) {
            Ok(names) => names,
            // Nothing can be appended under a file.
            Err(_) if self.dir.is_file() => Vec::new(),
            Err(error) => return Err(error),
        };
        let mut found = Vec::new();
        let reached = |name: &&String| {
            filed(name).is_some_and(|(machine, month)| kept.reaches(&machine, month))
        };
        for name in names.iter().filter(reached) {
            // One entry a sync left unreadable does not hide the runs in the others.
            let Ok(bytes) = fs::read(self.dir.join(name)) else {
                continue;
            };
            // A sync can deliver a line cut inside a character.
            let text = String::from_utf8_lossy(&bytes);
            let runs = text.lines().filter_map(read);
            found.extend(runs.filter(|run| kept.keeps(run)));
        }
        Ok(found)
    }
}

impl Usage for FsUsage {
    fn record(&self, invocation: &Invocation) -> Result<(), StoreError> {
        // A sync cannot reconcile two writers of one file.
        let name = format!("{}-{}{EXTENSION}", invocation.machine, invocation.month());
        let path = self.dir.join(name);
        // The store the log is in is never made by a line.
        match fs::create_dir(&self.dir) {
            Err(e) if e.kind() != ErrorKind::AlreadyExists => return Err(io_error(&self.dir, &e)),
            _ => {}
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| io_error(&path, &e))?;
        // Two processes can hold the file at once, and only a line written in one call cannot
        // land inside another.
        file.write_all(format!("{}\n", line(invocation)).as_bytes())
            .map_err(|e| io_error(&path, &e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{ATLAS, LANTERN, atlas, lamp_session, lantern, ran};

    fn run() -> Invocation {
        Invocation {
            written: Stamp::parse("2026-09-04T10:00:00.123456+01:00").unwrap(),
            version: Some("1.2.3".to_owned()),
            session: Some(lamp_session()),
            exit: 1,
            refusal: Some("lantern/relay: names no document".to_owned()),
            hits: Some(0),
            directory: "~/projects/lantern".to_owned(),
            arguments: vec!["lantern/relay".to_owned(), "--json".to_owned()],
            ..ran(&lantern(), "show", "2026-09-04")
        }
    }

    #[test]
    fn a_line_holds_every_key_and_reads_back_as_the_run() {
        let line = line(&run());
        assert_eq!(
            line,
            [
                r#"{"written":"2026-09-04T10:00:00.123456+01:00","#,
                &format!(r#""machine":"{LANTERN}","#),
                r#""version":"1.2.3","agent":"Lamp","session":"s-1","command":"show","exit":1,"#,
                r#""refusal":"lantern/relay: names no document","hits":0,"#,
                r#""directory":"~/projects/lantern","arguments":["lantern/relay","--json"]}"#,
            ]
            .concat()
        );
        assert_eq!(read(&line), Some(run()));
    }

    #[test]
    fn what_a_run_has_nothing_for_is_null_under_its_key() {
        let bare = ran(&lantern(), "topics", "2026-09-04");
        let line = line(&bare);
        assert_eq!(
            line,
            format!(
                r#"{{"written":"2026-09-04T10:00:00+01:00","machine":"{LANTERN}","version":null,"agent":null,"session":null,"command":"topics","exit":0,"refusal":null,"hits":null,"directory":"~","arguments":[]}}"#
            )
        );
        assert_eq!(read(&line), Some(bare));
    }

    #[test]
    fn a_line_break_in_an_argument_stays_on_the_line() {
        let broken = Invocation {
            arguments: vec!["one\ntwo\r".to_owned()],
            ..run()
        };
        let line = line(&broken);
        assert!(!line.contains(['\n', '\r']), "{line}");
        assert_eq!(read(&line), Some(broken));
    }

    #[test]
    fn a_key_left_out_reads_as_null_and_one_not_known_is_passed_over() {
        let line = line(&run());
        let later = line.replacen('{', r#"{"elapsed":12,"#, 1);
        assert_eq!(read(&later), Some(run()));
        let earlier = line.replacen(r#""hits":0,"#, "", 1);
        let read = read(&earlier).expect("a line");
        assert_eq!(
            read,
            Invocation {
                hits: None,
                ..run()
            }
        );
    }

    #[test]
    fn an_agent_or_a_session_alone_reads_as_neither() {
        let line = line(&run());
        for (from, to) in [
            (r#""agent":"Lamp""#, r#""agent":null"#),
            (r#""session":"s-1""#, r#""session":null"#),
        ] {
            assert!(line.contains(from), "{line}");
            let read = read(&line.replacen(from, to, 1));
            assert_eq!(read.expect("a line").session, None, "{from}");
        }
    }

    #[test]
    fn a_torn_line_and_one_of_another_shape_read_as_nothing() {
        let line = line(&run());
        assert_eq!(read(""), None);
        assert_eq!(read("[]"), None);
        assert_eq!(read(&line[..line.len() / 2]), None);
        for (from, to) in [
            (r#""exit":1"#, r#""exit":"later""#),
            (LANTERN, "desk"),
            ("2026-09-04T10:00:00.123456+01:00", "2026-09-04"),
            (r#""command":"show","#, ""),
            (
                r#""arguments":["lantern/relay","--json"]"#,
                r#""arguments":[4]"#,
            ),
        ] {
            assert!(line.contains(from), "{line}");
            assert_eq!(read(&line.replacen(from, to, 1)), None, "{from}");
        }
    }

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let kept = tempfile::tempdir().unwrap();
        let store = kept.path().join("store");
        fs::create_dir(&store).unwrap();
        (kept, store.join(USAGE_DIRECTORY))
    }

    fn all(usage: &FsUsage) -> Result<Vec<Invocation>, StoreError> {
        usage.read(&Kept::default())
    }

    fn written(machine: &DocumentId, command: &str, day: &str) -> String {
        format!("{}\n", line(&ran(machine, command, day)))
    }

    #[test]
    fn a_machine_appends_to_the_file_of_its_id_and_month_and_every_machine_is_read_back() {
        let (_kept, dir) = scratch();
        let usage = FsUsage::new(dir.clone());
        assert_eq!(all(&usage), Ok(Vec::new()));
        assert!(!dir.exists());
        let runs = [
            ran(&lantern(), "context", "2026-09-04"),
            ran(&lantern(), "new entry", "2026-09-30"),
            ran(&lantern(), "show", "2026-10-01"),
            ran(&atlas(), "context", "2026-09-04"),
        ];
        for run in &runs {
            usage.record(run).unwrap();
        }
        assert_eq!(
            files(&dir).unwrap(),
            [
                format!("{LANTERN}-2026-09.jsonl"),
                format!("{LANTERN}-2026-10.jsonl"),
                format!("{ATLAS}-2026-09.jsonl"),
            ]
        );
        assert_eq!(
            fs::read_to_string(dir.join(format!("{LANTERN}-2026-09.jsonl"))).unwrap(),
            format!("{}\n{}\n", line(&runs[0]), line(&runs[1]))
        );
        let all = all(&usage).unwrap();
        assert_eq!(
            all,
            [&runs[0], &runs[1], &runs[2], &runs[3]].map(Clone::clone)
        );
    }

    #[test]
    fn a_line_that_does_not_read_is_skipped_and_only_a_jsonl_file_is_read() {
        let (_kept, dir) = scratch();
        let usage = FsUsage::new(dir.clone());
        let run = ran(&lantern(), "context", "2026-09-04");
        usage.record(&run).unwrap();
        let path = dir.join(format!("{LANTERN}-2026-09.jsonl"));
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        let unheld = written(&lantern(), "show", "2026-09-04").replace(LANTERN, "7f3a");
        let torn = &written(&lantern(), "show", "2026-09-04")[..40];
        file.write_all(format!("{unheld}\u{fffd} torn\n{torn}").as_bytes())
            .unwrap();
        file.write_all(b"\n\xff\xfe torn\n2026-09-0").unwrap();
        let line = written(&atlas(), "show", "2026-09-04");
        fs::write(dir.join("desk-2026-09.tsv"), &line).unwrap();
        fs::write(dir.join("notes.txt"), &line).unwrap();
        fs::create_dir(dir.join(format!("{ATLAS}-2026-10.jsonl"))).unwrap();
        assert_eq!(all(&usage), Ok(vec![run]));

        for other in [
            "desk-2026-09.jsonl".to_owned(),
            format!("{ATLAS}-2026-09.sync-conflict-20261001-101010-ABCDEFG.jsonl"),
            format!("{ATLAS}-2026-9.jsonl"),
            format!("{ATLAS}-2026-09-01.jsonl"),
            format!("{}-2026-09.jsonl", &ATLAS[..31]),
            format!("{}-2026-09.jsonl", ATLAS.to_uppercase()),
            format!("{ATLAS}_2026-09.jsonl"),
            format!("{ATLAS}-2026-ab.jsonl"),
        ] {
            fs::write(dir.join(&other), &line).unwrap();
            assert_eq!(all(&usage).unwrap().len(), 1, "{other}");
        }
        fs::write(dir.join(format!("{ATLAS}-2026-09.jsonl")), &line).unwrap();
        assert_eq!(all(&usage).unwrap().len(), 2);
    }

    #[test]
    fn a_reading_for_a_machine_or_from_a_day_opens_only_the_files_whose_names_it_reaches() {
        let (_kept, dir) = scratch();
        let usage = FsUsage::new(dir.clone());
        fs::create_dir(&dir).unwrap();
        let probes: String = [lantern(), atlas()]
            .map(|filed| written(&filed, "probe", "2099-01-01"))
            .concat();
        for machine in [lantern(), atlas()] {
            for month in ["2026-08", "2026-09", "2026-10"] {
                let own = written(&machine, "show", &format!("{month}-20"));
                fs::write(dir.join(format!("{machine}-{month}.jsonl")), own + &probes).unwrap();
            }
        }
        let read = |machine: Option<DocumentId>, since: Option<&str>| {
            let kept = Kept {
                machine,
                since: since.map(str::to_owned),
            };
            usage.read(&kept).unwrap()
        };
        let opened = |machine: Option<DocumentId>, since: Option<&str>| {
            let in_each = if machine.is_some() { 1 } else { 2 };
            let read = read(machine, since);
            let probes = read.iter().filter(|run| run.command == "probe").count();
            assert_eq!(probes % in_each, 0);
            probes / in_each
        };
        assert_eq!(opened(None, None), 6);
        assert_eq!(opened(Some(lantern()), None), 3);
        assert_eq!(opened(None, Some("2026-09-21")), 4);
        assert_eq!(opened(None, Some("2026-10-01")), 2);
        assert_eq!(opened(Some(atlas()), Some("2026-10-01")), 1);
        assert_eq!(opened(Some(atlas()), Some("2026-11-01")), 0);
        let kept: Vec<String> = (read(Some(atlas()), Some("2026-09-21")).iter())
            .map(|run| {
                format!(
                    "{} {} {}",
                    run.written.day(),
                    run.machine.short(),
                    run.command
                )
            })
            .collect();
        assert_eq!(
            kept,
            [
                "2099-01-01 a71a5000 probe",
                "2026-10-20 a71a5000 show",
                "2099-01-01 a71a5000 probe"
            ]
        );
    }

    #[test]
    fn a_run_is_filed_and_kept_by_the_month_and_day_its_stamp_is_written_with() {
        let (_kept, dir) = scratch();
        let usage = FsUsage::new(dir.clone());
        let at = |stamp: &str, command: &str| Invocation {
            written: Stamp::parse(stamp).unwrap(),
            ..ran(&lantern(), command, "2026-09-04")
        };
        let early = at("2026-10-01T00:30:00+03:00", "show");
        let late = at("2026-09-30T23:30:00-03:00", "topics");
        assert!(early.written.instant() < late.written.instant());
        usage.record(&early).unwrap();
        usage.record(&late).unwrap();
        let file = |month: &str| dir.join(format!("{LANTERN}-{month}.jsonl"));
        let held = |month: &str| fs::read_to_string(file(month)).unwrap();
        assert_eq!(held("2026-10"), format!("{}\n", line(&early)));
        assert_eq!(held("2026-09"), format!("{}\n", line(&late)));

        let from = |day: &str| {
            let kept = Kept {
                machine: None,
                since: Some(day.to_owned()),
            };
            usage.read(&kept).unwrap()
        };
        assert_eq!(from("2026-10-01"), std::slice::from_ref(&early));
        assert_eq!(from("2026-09-30"), [late, early]);
        assert_eq!(from("2026-10-02"), []);
    }

    #[test]
    fn a_log_whose_store_is_gone_is_not_written_and_the_store_is_not_made() {
        let (kept, dir) = scratch();
        let store = kept.path().join("store");
        fs::remove_dir(&store).unwrap();
        let usage = FsUsage::new(dir);
        let refused = usage.record(&ran(&lantern(), "context", "2026-09-04"));
        assert!(refused.is_err());
        assert!(!store.exists());
    }

    #[test]
    fn an_entry_that_cannot_be_read_is_passed_over_and_a_log_that_is_a_file_holds_nothing() {
        let (kept, dir) = scratch();
        let usage = FsUsage::new(dir.clone());
        let run = ran(&lantern(), "context", "2026-09-04");
        usage.record(&run).unwrap();
        let named = |month: &str| dir.join(format!("{ATLAS}-{month}.jsonl"));
        std::os::unix::fs::symlink(dir.join("gone"), named("2026-09")).unwrap();
        fs::create_dir(dir.join("held")).unwrap();
        std::os::unix::fs::symlink(dir.join("held"), named("2026-10")).unwrap();
        assert_eq!(all(&usage), Ok(vec![run]));

        let file = kept.path().join("desk");
        fs::write(&file, "").unwrap();
        assert_eq!(all(&FsUsage::new(file)), Ok(Vec::new()));
    }

    #[test]
    fn a_log_under_what_is_not_a_directory_fails_with_its_place() {
        let (kept, dir) = scratch();
        fs::remove_dir(kept.path().join("store")).unwrap();
        fs::write(kept.path().join("store"), "").unwrap();
        let usage = FsUsage::new(dir.clone());
        let refused = usage.record(&ran(&lantern(), "context", "2026-09-04"));
        let refused = refused.unwrap_err().to_string();
        assert!(refused.starts_with(&dir.display().to_string()), "{refused}");
    }
}
