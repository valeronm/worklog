use std::path::PathBuf;
use std::process::Command;

use worklog_domain::ports::{Binary, Refreshed, Staged, StoreError};
use worklog_domain::release;

use super::{Staging, io_error, stage, target_of};

const REFRESH: [&str; 1] = ["refresh"];

pub struct FsBinary {
    exe: PathBuf,
    command: String,
}

impl FsBinary {
    pub fn new(exe: PathBuf, command: &str) -> FsBinary {
        FsBinary {
            exe,
            command: command.to_owned(),
        }
    }
}

struct FsStaged(Staging);

impl Staged for FsStaged {
    fn runs(&self) -> Result<(), StoreError> {
        let staged = &self.0.path;
        let output = Command::new(staged)
            .arg("--version")
            .output()
            .map_err(|e| io_error(staged, &e))?;
        if output.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&output.stderr);
        let why = match said.trim() {
            "" => output.status.to_string(),
            said => format!("{}: {said}", output.status),
        };
        Err(StoreError::io(staged.display(), why))
    }

    fn put(self: Box<Self>) -> Result<String, StoreError> {
        let FsStaged(staging) = *self;
        let target = staging.target.display().to_string();
        staging.put(true)?;
        Ok(target)
    }
}

impl Binary for FsBinary {
    fn asset(&self) -> Option<String> {
        release::asset(&self.command)
    }

    fn stage(&self, bytes: &[u8]) -> Result<Box<dyn Staged + '_>, StoreError> {
        let target = target_of(&self.exe)?;
        let kept = std::fs::metadata(&target)
            .map_err(|e| io_error(&target, &e))?
            .permissions();
        let staging = stage(&target, bytes, Some(&kept), true)?;
        Ok(Box::new(FsStaged(staging)))
    }

    fn refresh(&self) -> Refreshed {
        let output = match Command::new(&self.exe).args(REFRESH).output() {
            Ok(output) => output,
            Err(e) => {
                return Refreshed {
                    written: Vec::new(),
                    notes: Vec::new(),
                    ended: Err(io_error(&self.exe, &e)),
                };
            }
        };
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let written = text(&output.stdout).lines().map(str::to_owned).collect();
        let said = text(&output.stderr);
        let own = format!("{}: ", self.command);
        let unnamed = |line: &str| line.strip_prefix(&own).unwrap_or(line).to_owned();
        if output.status.success() {
            return Refreshed {
                written,
                notes: said.lines().map(unnamed).collect(),
                ended: Ok(()),
            };
        }
        let ran = format!("{} {}", self.exe.display(), REFRESH.join(" "));
        Refreshed {
            written,
            notes: Vec::new(),
            ended: Err(StoreError::io(ran, unnamed(said.trim()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn binary(exe: &Path) -> FsBinary {
        FsBinary::new(exe.to_path_buf(), "lantern")
    }

    fn replaced(binary: &FsBinary, bytes: &[u8]) -> Result<String, StoreError> {
        binary.stage(bytes)?.put()
    }

    #[test]
    fn a_binary_is_replaced_whole_with_its_mode_kept_and_nothing_left_beside_it() {
        let kept = tempfile::tempdir().unwrap();
        let exe = kept.path().join("lantern");
        fs::write(&exe, "old").unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).unwrap();

        assert_eq!(
            replaced(&binary(&exe), &[0, 159, 10]),
            Ok(exe.display().to_string())
        );
        assert_eq!(fs::read(&exe).unwrap(), [0, 159, 10]);
        let mode = fs::metadata(&exe).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o750);
        assert_eq!(names(kept.path()), ["lantern"]);
    }

    #[test]
    fn a_binary_run_through_a_link_has_the_file_it_links_to_replaced_and_the_link_kept() {
        let kept = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(kept.path()).unwrap();
        fs::create_dir_all(root.join("desk/bin")).unwrap();
        fs::create_dir_all(root.join("atlas")).unwrap();
        let real = root.join("atlas/lantern");
        fs::write(&real, "old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o750)).unwrap();
        let link = root.join("desk/bin/lantern");
        std::os::unix::fs::symlink("../../atlas/lantern", &link).unwrap();

        assert_eq!(
            replaced(&binary(&link), b"new"),
            Ok(real.display().to_string())
        );
        assert_eq!(fs::read(&real).unwrap(), b"new");
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        let mode = fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o750);
        assert_eq!(names(&root.join("atlas")), ["lantern"]);
    }

    #[test]
    fn a_binary_that_is_gone_is_not_replaced_and_nothing_is_staged_for_it() {
        let kept = tempfile::tempdir().unwrap();
        let gone = binary(&kept.path().join("lantern"));
        let refused = replaced(&gone, b"new").unwrap_err().to_string();
        assert!(refused.contains("lantern"), "{refused}");
        assert_eq!(names(kept.path()), [""; 0]);
    }

    #[test]
    fn a_replacement_that_fails_once_staged_leaves_the_binary_and_no_staged_file() {
        let kept = tempfile::tempdir().unwrap();
        let exe = kept.path().join("lantern");
        fs::create_dir(&exe).unwrap();
        fs::write(exe.join("relay"), "old").unwrap();
        assert!(replaced(&binary(&exe), b"new").is_err());
        assert_eq!(names(kept.path()), ["lantern"]);
        assert_eq!(fs::read_to_string(exe.join("relay")).unwrap(), "old");
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let exe = dir.join(name);
        fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        exe
    }

    // A file just written can be held open by a child another test thread forked meanwhile.
    fn busy(error: &StoreError) -> bool {
        error.to_string().contains("ext file busy")
    }

    fn refreshed(binary: &FsBinary) -> Refreshed {
        loop {
            match binary.refresh() {
                Refreshed {
                    ended: Err(error), ..
                } if busy(&error) => std::thread::yield_now(),
                ended => return ended,
            }
        }
    }

    fn ran(staged: &dyn Staged) -> Result<(), StoreError> {
        loop {
            match staged.runs() {
                Err(error) if busy(&error) => std::thread::yield_now(),
                ran => return ran,
            }
        }
    }

    #[test]
    fn a_staged_binary_is_run_with_version_and_gone_again_unless_it_is_put() {
        let kept = tempfile::tempdir().unwrap();
        let exe = script(kept.path(), "lantern", "exit 0");
        let lantern = binary(&exe);

        let answering = b"#!/bin/sh\n[ \"$1\" = --version ] || exit 9\n";
        let staged = lantern.stage(answering).unwrap();
        assert_eq!(ran(staged.as_ref()), Ok(()));
        drop(staged);
        assert_eq!(names(kept.path()), ["lantern"]);

        let failing = b"#!/bin/sh\necho 'no room' >&2\nexit 3\n";
        let staged = lantern.stage(failing).unwrap();
        let refused = ran(staged.as_ref()).unwrap_err().to_string();
        assert!(refused.ends_with(": exit status: 3: no room"), "{refused}");
        drop(staged);
        let staged = lantern.stage(b"#!/bin/sh\nexit 4\n").unwrap();
        let refused = ran(staged.as_ref()).unwrap_err().to_string();
        assert!(refused.ends_with(": exit status: 4"), "{refused}");
        drop(staged);
        let staged = lantern.stage(&[0, 159, 10]).unwrap();
        assert!(ran(staged.as_ref()).is_err());
        drop(staged);
        assert_eq!(names(kept.path()), ["lantern"]);

        let staged = lantern.stage(answering).unwrap();
        ran(staged.as_ref()).unwrap();
        assert_eq!(staged.put(), Ok(exe.display().to_string()));
        assert_eq!(fs::read(&exe).unwrap(), answering);
        assert_eq!(names(kept.path()), ["lantern"]);
    }

    fn wrote(lines: &[&str]) -> Refreshed {
        Refreshed {
            written: lines.iter().map(|line| (*line).to_owned()).collect(),
            notes: Vec::new(),
            ended: Ok(()),
        }
    }

    #[test]
    fn a_refresh_runs_the_binary_s_own_refresh_and_gives_each_line_it_printed() {
        let kept = tempfile::tempdir().unwrap();
        let echoing = script(kept.path(), "desk", "printf '%s\\n' \"$0\" \"$*\"");
        assert_eq!(
            refreshed(&binary(&echoing)),
            wrote(&[&echoing.display().to_string(), "refresh"])
        );
        let silent = script(kept.path(), "phone", "exit 0");
        assert_eq!(refreshed(&binary(&silent)), wrote(&[]));

        let gone = binary(Path::new("/gone/lantern")).refresh();
        let refused = gone.ended.unwrap_err().to_string();
        assert!(refused.starts_with("/gone/lantern: "), "{refused}");
        assert_eq!((gone.written, gone.notes), (Vec::new(), Vec::new()));
    }

    #[test]
    fn what_a_refresh_that_succeeds_said_is_noted_without_the_command_s_own_name() {
        let kept = tempfile::tempdir().unwrap();
        let body = "echo /home/desk/skill\necho 'lantern: fish left out' >&2\necho 'desk: odd' >&2";
        let noting = script(kept.path(), "lantern", body);
        assert_eq!(
            refreshed(&binary(&noting)),
            Refreshed {
                notes: vec!["fish left out".to_owned(), "desk: odd".to_owned()],
                ..wrote(&["/home/desk/skill"])
            }
        );
    }

    #[test]
    fn a_refresh_that_fails_says_what_the_binary_said_without_the_command_s_own_name() {
        let kept = tempfile::tempdir().unwrap();
        let said = |name: &str, line: &str| {
            let body = format!("echo '{line}' >&2\nexit 1");
            let exe = script(kept.path(), name, &body);
            let stopped = refreshed(&binary(&exe));
            assert_eq!((stopped.written, stopped.notes), (Vec::new(), Vec::new()));
            let refused = stopped.ended.unwrap_err().to_string();
            let ran = format!("{} refresh: ", exe.display());
            refused.strip_prefix(&ran).map(str::to_owned)
        };
        let why = Some("no agents here".to_owned());
        assert_eq!(said("lantern", "lantern: no agents here"), why);
        assert_eq!(said("atlas", "lantern: no agents here"), why);
        assert_eq!(
            said("phone", "phone: no agents here").as_deref(),
            Some("phone: no agents here")
        );
    }

    #[test]
    fn a_refresh_that_fails_after_it_printed_gives_the_lines_with_the_failure() {
        let kept = tempfile::tempdir().unwrap();
        let body = "echo /home/desk/skill\necho 'lantern: no room' >&2\nexit 1";
        let exe = script(kept.path(), "lantern", body);
        let ran = format!("{} refresh", exe.display());
        assert_eq!(
            refreshed(&binary(&exe)),
            Refreshed {
                ended: Err(StoreError::io(ran, "no room")),
                ..wrote(&["/home/desk/skill"])
            }
        );
    }

    #[test]
    fn the_asset_of_a_binary_is_the_one_built_for_this_target_under_its_command_s_name() {
        let named = binary(Path::new("/home/desk/bin/atlas")).asset();
        assert_eq!(named, release::asset("lantern"));
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(named.as_deref(), Some("lantern-x86_64-linux"));
        }
    }
}
