use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::domain::ports::{StoreError, Usage};
use crate::domain::usage::Invocation;

use super::io_error;

/// `<root>/usage/<machine>-<month>.tsv`, outside the kind directories the
/// store walks. A machine appends only to the file its own name opens, so
/// a sync never has two writers to reconcile.
pub struct FsUsage {
    dir: PathBuf,
}

impl FsUsage {
    #[must_use]
    pub fn new(root: &Path) -> FsUsage {
        FsUsage {
            dir: root.join("usage"),
        }
    }
}

impl Usage for FsUsage {
    fn record(&self, invocation: &Invocation) -> Result<(), StoreError> {
        fs::create_dir_all(&self.dir).map_err(|e| io_error(&self.dir, &e))?;
        let path = self
            .dir
            .join(format!("{}-{}.tsv", invocation.machine, invocation.month()));
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| io_error(&path, &e))?;
        // Two processes can hold the file at once, and only a line written
        // in one call cannot land inside another.
        file.write_all(invocation.to_line().as_bytes())
            .map_err(|e| io_error(&path, &e))
    }

    fn all(&self) -> Result<Vec<Invocation>, StoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(&self.dir, &e)),
        };
        let mut found = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| io_error(&self.dir, &e))?.path();
            if path.extension().is_none_or(|e| e != "tsv") {
                continue;
            }
            let text = fs::read_to_string(&path).map_err(|e| io_error(&path, &e))?;
            found.extend(text.lines().filter_map(Invocation::parse_line));
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::machine::MachineName;

    fn ran(machine: &str, command: &str, day: &str) -> Invocation {
        Invocation {
            written: format!("{day}T10:00:00.000001+01:00"),
            machine: MachineName::parse(machine).unwrap(),
            command: command.into(),
            exit: 0,
            directory: "~/projects/lantern".into(),
            arguments: vec![],
        }
    }

    #[test]
    fn a_machine_appends_to_its_own_month_and_reads_every_machine_back() {
        let dir = tempfile::tempdir().unwrap();
        let usage = FsUsage::new(dir.path());
        assert_eq!(usage.all().unwrap(), []);
        usage.record(&ran("desk", "context", "2026-09-04")).unwrap();
        usage.record(&ran("desk", "show", "2026-09-04")).unwrap();
        usage.record(&ran("desk", "show", "2026-10-01")).unwrap();
        usage
            .record(&ran("phone", "context", "2026-09-04"))
            .unwrap();
        let mut named: Vec<String> = fs::read_dir(dir.path().join("usage"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        named.sort();
        assert_eq!(
            named,
            ["desk-2026-09.tsv", "desk-2026-10.tsv", "phone-2026-09.tsv"]
        );
        assert_eq!(usage.all().unwrap().len(), 4);
    }

    #[test]
    fn a_half_written_line_is_skipped_and_the_rest_still_reads() {
        let dir = tempfile::tempdir().unwrap();
        let usage = FsUsage::new(dir.path());
        usage.record(&ran("desk", "context", "2026-09-04")).unwrap();
        let path = dir.path().join("usage/desk-2026-09.tsv");
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"2026-09-04T10:00:00+01:00\tdesk\ths")
            .unwrap();
        let all = usage.all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].command, "context");
    }
}
