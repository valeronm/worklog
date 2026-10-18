//! The adapters a front end runs its use cases over, built from this host's environment.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

mod fs;
mod net;

use crate::app::{Deps, Failure};
use crate::domain::ports::{Binary, Releases};
use crate::domain::usage::Session;
use fs::{
    Config, DirReleases, FsAgent, FsBinary, FsDrafts, FsHost, FsStore, FsUsage, Paths, RandomIds,
    SystemClock,
};
use net::GitHubReleases;

/// The last reading of the store a process made through its adapters, which adapters built
/// `continuing` it start from.
#[derive(Default)]
pub struct Reading(fs::Reading);

/// The ports of one command, since the store among them is read once.
pub struct Adapters {
    store: FsStore,
    drafts: FsDrafts,
    ids: RandomIds,
    clock: SystemClock,
    host: FsHost,
    usage: FsUsage,
}

impl Adapters {
    #[must_use]
    pub fn deps(&self) -> Deps<'_> {
        Deps::new(
            &self.store,
            &self.drafts,
            &self.ids,
            &self.clock,
            &self.host,
            &self.usage,
        )
    }

    /// Every later header read of these adapters' store comes from one reading of its
    /// tree, made here unless one was made.
    pub fn settle(&self) -> Result<(), Failure> {
        Ok(self.store.settle()?)
    }

    /// Adapters whose store reads through this host's cache and leaves it as it was.
    #[must_use]
    pub fn recalling(mut self) -> Adapters {
        self.store = self.store.recalling();
        self
    }

    /// Adapters whose store starts its reading from `reading` and leaves its reading there.
    #[must_use]
    pub fn continuing(mut self, reading: &Reading) -> Adapters {
        self.store = self.store.continuing(&reading.0);
        self
    }

    /// Puts these adapters in the session of the agent whose hook's command holds `word`, under
    /// the id `given` returns for the key that agent's hook input holds it under. That session
    /// replaces the one a variable gave. `given` is not called for a word no agent's hook
    /// holds, and an id that is missing or empty changes nothing.
    #[must_use]
    pub fn run_by_hook(mut self, word: &str, given: &dyn Fn(&str) -> Option<String>) -> Adapters {
        if let Some(session) = hook_session(word, given) {
            self.host = self.host.in_session(session);
        }
        self
    }
}

fn hook_session(word: &str, given: &dyn Fn(&str) -> Option<String>) -> Option<Session> {
    let known = AGENTS.iter().find(|known| known.hook.word == word)?;
    let id = given(known.hook.session)?;
    (!id.is_empty()).then(|| Session {
        agent: known.name.to_owned(),
        id,
    })
}

/// The adapters over the store this host's config holds, or over `chosen`, the store a host
/// with no config is about to be bound to; `None` for a host with neither, one that is not
/// set up. Refuses a configured store that is not a directory unless a store is being chosen,
/// and a `chosen` one that is a file, or a directory holding no store and something that is
/// neither dot-named nor its usage log.
pub fn wire(
    variable: &dyn Fn(&str) -> Option<OsString>,
    chosen: Option<PathBuf>,
) -> Result<Option<Adapters>, Failure> {
    let paths = Paths::resolved(variable)?;
    let session = session(variable);
    let (root, store, host) = match (Config::read(&paths.config)?, chosen) {
        (Some(held), chosen) => {
            let root = held.store.clone();
            let store = match chosen {
                None => FsStore::open(root.clone())?,
                Some(_) => FsStore::new(root.clone()),
            }
            .caching(&paths.cache);
            let host = FsHost::bound(paths.config, paths.home, held, session);
            (root, store, host)
        }
        (None, Some(chosen)) => (
            chosen.clone(),
            FsStore::chosen(chosen.clone())?.caching(&paths.cache),
            FsHost::unbound(paths.config, paths.home, chosen, session),
        ),
        (None, None) => return Ok(None),
    };
    Ok(Some(Adapters {
        store,
        drafts: FsDrafts::new(paths.drafts),
        ids: RandomIds,
        clock: SystemClock,
        host,
        usage: FsUsage::new(root.join(USAGE_DIRECTORY)),
    }))
}

// The first agent whose variable is set is the one a command run by an agent inside another
// is logged under.
fn session(variable: &dyn Fn(&str) -> Option<OsString>) -> Option<Session> {
    AGENTS.iter().find_map(|known| {
        let id = fs::set(variable, known.session)?.into_string().ok()?;
        Some(Session {
            agent: known.name.to_owned(),
            id,
        })
    })
}

/// Whether `binary`, the file that is running, is the one `path`, the value of `PATH`, finds
/// for `name`, the file name it was run by.
#[must_use]
pub(crate) fn found_on_path(binary: &Path, name: &OsStr, path: &OsStr) -> bool {
    // A link on the path is found by the file it leads to.
    let Ok(running) = std::fs::canonicalize(binary) else {
        return false;
    };
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
        .is_some_and(|found| found == running)
}

pub const SKILL_DIRECTORY: &str = fs::SKILL_DIRECTORY;

/// The directory of a store that holds its usage log and no document.
pub const USAGE_DIRECTORY: &str = fs::USAGE_DIRECTORY;

#[derive(Debug, PartialEq, Eq)]
struct Known {
    name: &'static str,
    /// Under the user's home.
    home: &'static str,
    hooks: &'static str,
    /// The variable the agent sets to its session's id for a command it runs.
    session: &'static str,
    hook: Hook,
    rule: Option<Rule>,
}

#[derive(Debug, PartialEq, Eq)]
struct Hook {
    /// Names the agent in its hook's command.
    word: &'static str,
    /// The key the session's id is under in what the agent gives a hook on its input.
    session: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
struct Rule {
    /// Under the agent's home.
    file: &'static str,
    text: &'static str,
}

// Codex holds everything outside the workspace read-only for a command it runs, unless an
// `allow` rule matches the command.
const CODEX_RULE: Rule = Rule {
    file: "rules/worklog.rules",
    text: r#"prefix_rule(
    pattern = ["worklog"],
    decision = "allow",
    justification = "worklog writes its store, which is outside the workspace",
)
"#,
};

static AGENTS: [Known; 2] = [
    Known {
        name: "Claude Code",
        home: ".claude",
        hooks: "settings.json",
        session: "CLAUDE_CODE_SESSION_ID",
        hook: Hook {
            word: "claude",
            session: "session_id",
        },
        rule: None,
    },
    Known {
        name: "Codex",
        home: ".codex",
        hooks: "hooks.json",
        session: "CODEX_THREAD_ID",
        hook: Hook {
            word: "codex",
            session: "session_id",
        },
        rule: Some(CODEX_RULE),
    },
];

/// A coding agent on this host and what this tool places in its home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    files: FsAgent,
    homes: Vec<PathBuf>,
    known: &'static Known,
}

impl Agent {
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.files.name()
    }

    #[must_use]
    pub fn hooks(&self) -> &Path {
        self.files.hooks()
    }

    #[must_use]
    pub fn hook_word(&self) -> &'static str {
        self.known.hook.word
    }

    #[must_use]
    pub(crate) fn homes(&self) -> Vec<&Path> {
        self.homes.iter().map(PathBuf::as_path).collect()
    }

    /// Whether the agent's home directory is there; nothing here ever creates one.
    #[must_use]
    pub fn is_present(&self) -> bool {
        self.files.is_present()
    }

    pub fn read_skill(&self) -> Result<Option<String>, Failure> {
        Ok(self.files.read_skill()?)
    }

    /// Returns the file written.
    pub fn write_skill(&self, text: &str) -> Result<PathBuf, Failure> {
        Ok(self.files.write_skill(text)?)
    }

    /// Removes the skill's own directory and returns it, or `None` when there was none.
    pub fn remove_skill(&self) -> Result<Option<PathBuf>, Failure> {
        Ok(self.files.remove_skill()?)
    }

    /// Writes the rule that lets the agent run this tool outside its sandbox and returns the
    /// file, or `None` for an agent that takes no rule.
    pub fn write_rule(&self) -> Result<Option<PathBuf>, Failure> {
        let Some(rule) = &self.known.rule else {
            return Ok(None);
        };
        Ok(self.files.write_rule(rule.text)?)
    }

    /// Writes the rule where the one there is not the one this binary writes, and with
    /// `missing_too` where none is there, and returns the file then.
    pub fn refresh_rule(&self, missing_too: bool) -> Result<Option<PathBuf>, Failure> {
        let Some(rule) = &self.known.rule else {
            return Ok(None);
        };
        match self.files.read_rule()? {
            Some(held) if held == rule.text => Ok(None),
            None if !missing_too => Ok(None),
            _ => self.write_rule(),
        }
    }

    /// Removes the rule's file and returns it, or `None` when there was none.
    pub fn remove_rule(&self) -> Result<Option<PathBuf>, Failure> {
        Ok(self.files.remove_rule()?)
    }

    pub fn read_hooks(&self) -> Result<Option<String>, Failure> {
        Ok(self.files.read_hooks()?)
    }

    pub fn write_hooks(&self, text: &str) -> Result<(), Failure> {
        Ok(self.files.write_hooks(text)?)
    }
}

/// The agents this tool knows, present on this host or not, under `HOME` and not under the
/// directory `WORKLOG_HOME` moves the rest to.
pub fn agents(variable: &dyn Fn(&str) -> Option<OsString>) -> Result<Vec<Agent>, Failure> {
    let homes = homes(variable)?;
    let Some(home) = homes.first() else {
        return Err(Failure::at("$HOME", "not set"));
    };
    let agent = |known: &'static Known| Agent {
        files: FsAgent::new(
            known.name,
            home.join(known.home),
            known.hooks,
            known.rule.as_ref().map(|rule| rule.file),
        ),
        homes: homes.clone(),
        known,
    };
    Ok(AGENTS.iter().map(agent).collect())
}

/// The user's home directory as given and, where a link leads elsewhere, as it resolves; empty
/// where `HOME` is not set.
pub(crate) fn homes(variable: &dyn Fn(&str) -> Option<OsString>) -> Result<Vec<PathBuf>, Failure> {
    let Some(home) = Paths::home(variable)?.map(PathBuf::from) else {
        return Ok(Vec::new());
    };
    let mut homes = vec![home];
    if let Ok(resolved) = std::fs::canonicalize(&homes[0])
        && resolved != homes[0]
    {
        homes.push(resolved);
    }
    Ok(homes)
}

/// The file fish reads the completions of `command` from, under `XDG_CONFIG_HOME` or the home
/// directory's `.config` and not under the directory `WORKLOG_HOME` moves the rest to;
/// `None` where neither is set or fish's completions directory is not there.
pub(crate) fn fish_completions(
    variable: &dyn Fn(&str) -> Option<OsString>,
    command: &str,
) -> Result<Option<PathBuf>, Failure> {
    let config_home = Paths::resolved(variable)?.config_home;
    let dir = config_home.map(|config_home| config_home.join("fish").join("completions"));
    let dir = dir.filter(|dir| dir.is_dir());
    Ok(dir.map(|dir| dir.join(format!("{command}.fish"))))
}

/// Refuses a `file` whose directory is not there; nothing here creates it.
pub(crate) fn write_completions(file: &Path, text: &str) -> Result<(), Failure> {
    let dir = file.parent().unwrap_or(file);
    Ok(fs::replace_under(dir, file, text)?)
}

const RELEASES: &str = "WORKLOG_RELEASES";

fn release_directory(variable: &dyn Fn(&str) -> Option<OsString>) -> Option<DirReleases> {
    let dir = fs::set(variable, RELEASES)?;
    Some(DirReleases::new(PathBuf::from(dir)))
}

/// Where releases of this binary are published: the directory `WORKLOG_RELEASES` holds,
/// and the network without one.
#[must_use]
pub(crate) fn releases(variable: &dyn Fn(&str) -> Option<OsString>) -> Box<dyn Releases> {
    match release_directory(variable) {
        Some(directory) => Box::new(directory),
        None => Box::new(GitHubReleases::new()),
    }
}

/// The executable at `path`, the one this process runs from, whose own messages start with
/// `command`.
#[must_use]
pub(crate) fn binary(path: &Path, command: &str) -> Box<dyn Binary> {
    Box::new(FsBinary::new(path.to_path_buf(), command))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::domain::testing::{LANTERN, lantern, ran};
    use crate::domain::usage::Kept;

    fn scratch() -> (tempfile::TempDir, PathBuf) {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        (kept, root)
    }

    fn wired(root: &Path, chosen: Option<PathBuf>) -> Result<Option<Adapters>, Failure> {
        let variable = |name: &str| match name {
            "WORKLOG_HOME" => Some(root.join("next").into_os_string()),
            _ => None,
        };
        wire(&variable, chosen)
    }

    #[test]
    fn a_host_with_no_config_has_adapters_only_over_a_store_it_is_given() {
        let (_kept, root) = scratch();
        assert!(wired(&root, None).unwrap().is_none());
        assert!(wired(&root, Some(root.join("store"))).unwrap().is_some());
        assert!(!root.join("store").exists());
        assert!(!root.join("next").exists());
    }

    #[test]
    fn a_configured_store_that_is_gone_is_refused_except_to_one_choosing_a_store() {
        let (_kept, root) = scratch();
        let store = root.join("store");
        let config = Config {
            store: store.clone(),
            machine: lantern(),
        };
        config.write(&root.join("next/config.toml")).unwrap();

        let Err(Failure::Store(refused)) = wired(&root, None) else {
            panic!("a store that is not there must be refused");
        };
        assert_eq!(
            refused.to_string(),
            format!("{}: this host's store is not a directory", store.display())
        );
        assert!(
            wired(&root, Some(root.join("elsewhere")))
                .unwrap()
                .is_some()
        );

        std::fs::create_dir_all(&store).unwrap();
        assert!(wired(&root, None).unwrap().is_some());
    }

    #[test]
    fn a_damaged_config_is_a_failure_and_not_a_host_that_is_not_set_up() {
        let (_kept, root) = scratch();
        std::fs::create_dir_all(root.join("next")).unwrap();
        std::fs::write(root.join("next/config.toml"), "store = 4\n").unwrap();
        assert!(matches!(wired(&root, None), Err(Failure::Store(_))));
        let chosen = Some(root.join("store"));
        assert!(matches!(wired(&root, chosen), Err(Failure::Store(_))));
    }

    fn bound(root: &Path) -> PathBuf {
        let store = root.join("store");
        let config = Config {
            store: store.clone(),
            machine: lantern(),
        };
        config.write(&root.join("next/config.toml")).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        store
    }

    #[test]
    fn the_usage_log_of_the_adapters_is_under_the_store_they_are_over() {
        use crate::domain::ports::Usage as _;

        let (_kept, root) = scratch();
        let store = bound(&root);
        let adapters = wired(&root, None).unwrap().unwrap();
        let run = ran(&lantern(), "show", "2026-09-04");
        adapters.usage.record(&run).unwrap();
        let file = store.join(format!("usage/{LANTERN}-2026-09.jsonl"));
        assert!(file.is_file());
        let all = Kept::default();
        assert_eq!(adapters.deps().read().usage.read(&all), Ok(vec![run]));

        let chosen = wired(&root, Some(root.join("elsewhere"))).unwrap().unwrap();
        assert_eq!(chosen.deps().read().usage.read(&all).unwrap().len(), 1);
    }

    #[test]
    fn a_hook_s_session_is_its_agent_s_and_comes_before_a_variable() {
        let (_kept, root) = scratch();
        bound(&root);
        let variable = |name: &str| match name {
            "WORKLOG_HOME" => Some(root.join("next").into_os_string()),
            "CLAUDE_CODE_SESSION_ID" => Some(OsString::from("s-1")),
            _ => None,
        };
        let run_by = |word: &str, given: &dyn Fn(&str) -> Option<String>| {
            let adapters = wire(&variable, None).unwrap().unwrap();
            let session = adapters
                .run_by_hook(word, given)
                .deps()
                .read()
                .host
                .session();
            session.map(|session| (session.agent, session.id)).unwrap()
        };
        let session = |agent: &str, id: &str| (agent.to_owned(), id.to_owned());
        let under = |key: &str| (key == "session_id").then(|| "s-9".to_owned());
        assert_eq!(run_by("codex", &under), session("Codex", "s-9"));
        for found in [None, Some(String::new())] {
            let outer = run_by("codex", &|_| found.clone());
            assert_eq!(outer, session("Claude Code", "s-1"), "{found:?}");
        }
        for word in ["lantern", "Codex"] {
            let unasked = run_by(word, &|_| panic!("{word}: its input was asked for"));
            assert_eq!(unasked, session("Claude Code", "s-1"));
        }
    }

    #[test]
    fn a_host_is_in_the_session_of_the_first_agent_whose_variable_is_set() {
        let (_kept, root) = scratch();
        bound(&root);
        let within = |set: &[(&str, &str)]| {
            let variable = |name: &str| match name {
                "WORKLOG_HOME" => Some(root.join("next").into_os_string()),
                _ => (set.iter().find(|(key, _)| *key == name)).map(|(_, id)| OsString::from(id)),
            };
            let adapters = wire(&variable, None).unwrap().unwrap();
            let session = adapters.deps().read().host.session();
            session.map(|session| (session.agent, session.id))
        };
        let session = |agent: &str, id: &str| Some((agent.to_owned(), id.to_owned()));
        assert_eq!(within(&[]), None);
        assert_eq!(
            within(&[("CLAUDE_CODE_SESSION_ID", "s-1")]),
            session("Claude Code", "s-1")
        );
        assert_eq!(
            within(&[("CODEX_THREAD_ID", "s-2")]),
            session("Codex", "s-2")
        );
        assert_eq!(
            within(&[
                ("CODEX_THREAD_ID", "s-2"),
                ("CLAUDE_CODE_SESSION_ID", "s-1")
            ]),
            session("Claude Code", "s-1")
        );
        assert_eq!(
            within(&[("CLAUDE_CODE_SESSION_ID", ""), ("CODEX_THREAD_ID", "s-2")]),
            session("Codex", "s-2")
        );
        assert_eq!(within(&[("CODEX_THREAD_ID", "")]), None);
    }

    #[test]
    fn a_binary_is_found_on_the_path_only_as_the_first_file_of_its_name_there() {
        let (_kept, root) = scratch();
        let name = OsStr::new("lantern");
        for dir in ["bin", "bench", "empty", "linked", "holding/lantern"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let binary = root.join("bin/lantern");
        std::fs::write(&binary, "").unwrap();
        std::fs::write(root.join("bench/lantern"), "").unwrap();
        std::os::unix::fs::symlink(&binary, root.join("linked/lantern")).unwrap();
        let found = |binary: &Path, dirs: &[&str]| {
            let path = std::env::join_paths(dirs.iter().map(|dir| root.join(dir))).unwrap();
            found_on_path(binary, name, &path)
        };

        assert!(found(&binary, &["bin"]));
        assert!(found(&binary, &["empty", "holding", "bin", "bench"]));
        assert!(found(&binary, &["linked"]));
        assert!(found(&root.join("linked/lantern"), &["bin"]));
        assert!(!found(&binary, &["bench", "bin"]));
        assert!(!found(&binary, &["empty"]));
        assert!(!found(&binary, &[]));
        assert!(!found(&root.join("gone/lantern"), &["bin"]));
        assert!(!found_on_path(
            &binary,
            OsStr::new("atlas"),
            root.join("bin").as_os_str()
        ));
    }

    fn with_home(home: Option<&Path>, next: Option<&Path>) -> impl Fn(&str) -> Option<OsString> {
        let home = home.map(|home| home.as_os_str().to_owned());
        let next = next.map(|next| next.as_os_str().to_owned());
        move |name: &str| match name {
            "HOME" => home.clone(),
            "WORKLOG_HOME" => next.clone(),
            _ => None,
        }
    }

    #[test]
    fn the_agents_are_under_home_whatever_the_rest_is_moved_to() {
        let (_kept, root) = scratch();
        for next in [None, Some(root.join("next"))] {
            let listed = agents(&with_home(Some(&root), next.as_deref())).unwrap();
            let seen: Vec<_> = listed
                .iter()
                .map(|agent| (agent.name(), agent.hooks().to_path_buf()))
                .collect();
            assert_eq!(
                seen,
                [
                    ("Claude Code", root.join(".claude/settings.json")),
                    ("Codex", root.join(".codex/hooks.json")),
                ]
            );
        }
    }

    #[test]
    fn an_agent_is_present_by_its_home_and_its_files_are_reached_through_it() {
        let (_kept, root) = scratch();
        std::fs::create_dir(root.join(".codex")).unwrap();
        let listed = agents(&with_home(Some(&root), None)).unwrap();
        let present: Vec<_> = listed.iter().filter(|a| a.is_present()).collect();
        assert_eq!(present.len(), 1);
        let codex = present[0];
        assert_eq!(codex.name(), "Codex");
        assert_eq!(codex.read_hooks().unwrap(), None);
        codex.write_hooks("{}").unwrap();
        assert_eq!(codex.read_hooks().unwrap(), Some("{}".to_owned()));
        let skill = codex.write_skill("lantern").unwrap();
        assert_eq!(skill, root.join(".codex/skills/worklog/SKILL.md"));
        assert_eq!(codex.read_skill().unwrap(), Some("lantern".to_owned()));
        assert_eq!(
            codex.remove_skill().unwrap(),
            Some(root.join(".codex/skills/worklog"))
        );
        let rule = root.join(".codex/rules/worklog.rules");
        assert_eq!(codex.refresh_rule(false).unwrap(), None);
        assert_eq!(codex.refresh_rule(true).unwrap(), Some(rule.clone()));
        assert_eq!(codex.refresh_rule(true).unwrap(), None);
        std::fs::write(&rule, "lantern").unwrap();
        assert_eq!(codex.refresh_rule(false).unwrap(), Some(rule.clone()));
        assert_ne!(std::fs::read_to_string(&rule).unwrap(), "lantern");
        assert_eq!(codex.write_rule().unwrap(), Some(rule.clone()));
        assert_eq!(codex.remove_rule().unwrap(), Some(rule));
        assert!(!root.join(".claude").exists());
        assert_eq!(codex.homes(), [root.as_path()]);
    }

    #[test]
    fn fish_reads_its_completions_under_the_config_directory_and_only_where_that_is_there() {
        let (_kept, root) = scratch();
        let home = root.join("home");
        let next = root.join("next");
        let under_home = with_home(Some(&home), Some(&next));
        assert_eq!(fish_completions(&under_home, "lantern"), Ok(None));
        std::fs::create_dir_all(home.join(".config/fish")).unwrap();
        std::fs::write(home.join(".config/fish/completions"), "").unwrap();
        assert_eq!(fish_completions(&under_home, "lantern"), Ok(None));
        std::fs::remove_file(home.join(".config/fish/completions")).unwrap();

        let dir = home.join(".config/fish/completions");
        let file = dir.join("lantern.fish");
        assert!(write_completions(&file, "atlas").is_err());
        assert!(!dir.exists());
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            fish_completions(&under_home, "lantern"),
            Ok(Some(file.clone()))
        );
        write_completions(&file, "atlas\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "atlas\n");
        write_completions(&file, "desk\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "desk\n");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        assert!(!next.exists());

        let elsewhere = root.join("config/fish/completions");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let given = |name: &str| match name {
            "HOME" => Some(home.clone().into_os_string()),
            "XDG_CONFIG_HOME" => Some(root.join("config").into_os_string()),
            _ => None,
        };
        assert_eq!(
            fish_completions(&given, "lantern"),
            Ok(Some(elsewhere.join("lantern.fish")))
        );
        assert_eq!(
            fish_completions(&with_home(None, Some(&next)), "lantern"),
            Ok(None)
        );
    }

    #[test]
    fn releases_are_read_from_the_directory_the_environment_gives() {
        let (_kept, root) = scratch();
        std::fs::write(root.join("latest"), "v0.3.0\n").unwrap();
        std::fs::write(root.join("lantern"), "atlas").unwrap();
        let given = |name: &str| match name {
            "WORKLOG_RELEASES" => Some(root.clone().into_os_string()),
            _ => None,
        };
        let held = release_directory(&given).expect("a directory");
        assert_eq!(held.latest(), Ok("v0.3.0".to_owned()));
        assert_eq!(held.fetch("v0.3.0", "lantern"), Ok(b"atlas".to_vec()));
        let unset = |name: &str| (name != "WORKLOG_RELEASES").then(OsString::new);
        assert!(release_directory(&unset).is_none());
        let empty = |_: &str| Some(OsString::new());
        assert!(release_directory(&empty).is_none());
    }

    #[test]
    fn the_homes_are_the_one_given_and_the_one_it_resolves_to_and_none_without_a_home() {
        let (_kept, root) = scratch();
        std::fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        assert_eq!(
            homes(&with_home(Some(&root.join("link")), None)).unwrap(),
            [root.join("link"), root.join("real")]
        );
        assert_eq!(
            homes(&with_home(Some(&root.join("real")), None)).unwrap(),
            [root.join("real")]
        );
        assert_eq!(
            homes(&with_home(Some(&root.join("gone")), None)).unwrap(),
            [root.join("gone")]
        );
        assert_eq!(
            homes(&with_home(None, Some(&root))).unwrap(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            homes(&with_home(None, None)).unwrap(),
            Vec::<PathBuf>::new()
        );
        let odd = |name: &str| {
            use std::os::unix::ffi::OsStringExt as _;
            (name == "HOME").then(|| OsString::from_vec(vec![b'/', 0xff]))
        };
        assert!(homes(&odd).is_err());
    }

    #[test]
    fn no_home_directory_is_a_refusal_naming_it() {
        let (_kept, root) = scratch();
        for next in [None, Some(root.as_path())] {
            let refused = agents(&with_home(None, next)).unwrap_err().to_string();
            assert!(refused.contains("$HOME"), "{refused}");
        }
    }

    #[test]
    fn a_home_reached_through_a_link_is_given_as_it_is_and_as_it_resolves() {
        let (_kept, root) = scratch();
        std::fs::create_dir(root.join("real")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        let listed = agents(&with_home(Some(&root.join("link")), None)).unwrap();
        assert_eq!(
            listed[0].homes(),
            [root.join("link").as_path(), root.join("real").as_path()]
        );
    }
}
