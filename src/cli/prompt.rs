//! The questions `init` asks when it is given no argument.

use std::io::{BufRead, ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::agents::{self, Placing, initialized};
use super::render::Output;
use super::{Environment, NAME};
use crate::app::machines::{self, HostName};
use crate::app::{Deps, Failure, ReadDeps, setup};
use crate::wiring;

const DEFAULT_STORE: &str = "worklog";

pub(super) fn no_terminal() -> Failure {
    Failure::Usage(format!(
        "`init` alone asks its questions on a terminal. A host is set up by its user, who gives \
         the directory of the store; without a terminal that is \
         `{NAME} init <NAME> --store <DIR>`"
    ))
}

pub(super) struct Terminal<'a> {
    answers: &'a mut dyn BufRead,
    shown: &'a mut dyn Write,
}

impl<'a> Terminal<'a> {
    pub(super) fn new(answers: &'a mut dyn BufRead, shown: &'a mut dyn Write) -> Terminal<'a> {
        Terminal { answers, shown }
    }

    fn say(&mut self, line: &str) {
        let _ = writeln!(self.shown, "{line}");
    }

    fn answer(&mut self, question: &str, hint: Option<&str>) -> Result<String, Failure> {
        loop {
            let _ = match hint {
                Some(hint) => write!(self.shown, "{question} [{hint}]: "),
                None => write!(self.shown, "{question}: "),
            };
            let _ = self.shown.flush();
            let mut line = String::new();
            match self.answers.read_line(&mut line) {
                Ok(0) => {
                    // The cursor is still on the question's line.
                    self.say("");
                    return Err(Failure::Refused(
                        "the answers ended before the questions; nothing was written".to_owned(),
                    ));
                }
                Ok(_) => return Ok(line.trim().to_owned()),
                Err(error) if error.kind() == ErrorKind::InvalidData => {
                    self.say("The answer is not UTF-8.");
                }
                Err(error) => return Err(Failure::at("the answer", error)),
            }
        }
    }

    fn ask(&mut self, question: &str, default: Option<&str>) -> Result<Option<String>, Failure> {
        let answer = self.answer(question, default)?;
        Ok(if answer.is_empty() {
            default.map(str::to_owned)
        } else {
            Some(answer)
        })
    }

    fn consent(&mut self, question: &str, default: bool) -> Result<bool, Failure> {
        let hint = if default { "Y/n" } else { "y/N" };
        loop {
            let answer = self.answer(question, Some(hint))?.to_ascii_lowercase();
            match answer.as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.say("Answer y or n."),
            }
        }
    }
}

fn expanded(answer: &str, home: Option<&Path>) -> Result<PathBuf, Failure> {
    let under_home = match answer.strip_prefix('~') {
        Some("") => Some(""),
        Some(rest) => rest.strip_prefix('/'),
        None => None,
    };
    match (under_home, home) {
        (None, _) if answer.starts_with('~') => Err(Failure::Usage(
            "Only `~` and `~/` are read as the home directory.".to_owned(),
        )),
        (None, _) => Ok(PathBuf::from(answer)),
        (Some(""), Some(home)) => Ok(home.to_path_buf()),
        (Some(rest), Some(home)) => Ok(home.join(rest)),
        (Some(_), None) => Err(Failure::at("$HOME", "not set")),
    }
}

/// The directory typed, as an absolute path, with what `open` makes of it; an answer that names
/// no directory or that `open` fails on is asked for again.
pub(super) fn store<T>(
    terminal: &mut Terminal,
    environment: &Environment,
    open: impl Fn(&Path) -> Result<T, Failure>,
) -> Result<(PathBuf, T), Failure> {
    let home = wiring::homes(environment.variable)?.into_iter().next();
    let default = home
        .as_ref()
        .map(|home| home.join(DEFAULT_STORE).display().to_string());
    loop {
        let Some(answer) = terminal.ask("Directory of the store", default.as_deref())? else {
            terminal.say("Name the directory.");
            continue;
        };
        let opened = expanded(&answer, home.as_deref())
            .and_then(|given| environment.placed(&given))
            .and_then(|placed| open(&placed).map(|opened| (placed, opened)));
        match opened {
            Ok(opened) => return Ok(opened),
            Err(failure) => terminal.say(&failure.to_string()),
        }
    }
}

fn machine(deps: &ReadDeps, terminal: &mut Terminal) -> Result<(String, bool), Failure> {
    let offered = match machines::host_name(deps)? {
        HostName::Held(name) => {
            terminal.say(&format!("This machine is {name}, found in the store."));
            Some(name)
        }
        named => {
            let mut listed = machines::machines(deps)?;
            if let HostName::Free(name) = named {
                listed.push(format!("{name} (new)"));
            }
            if !listed.is_empty() {
                terminal.say("Machine topics:");
            }
            for line in listed {
                terminal.say(&format!("  {line}"));
            }
            None
        }
    };
    loop {
        let Some(name) = terminal.ask("Machine topic", offered.as_deref())? else {
            terminal.say("Name the machine topic.");
            continue;
        };
        match machines::held(deps, &name) {
            Ok(held) => return Ok((name, held)),
            Err(Failure::Usage(why) | Failure::Refused(why)) => terminal.say(&why),
            Err(failure) => return Err(failure),
        }
    }
}

fn joined(names: &[&str]) -> String {
    match names {
        [first @ .., last] if !first.is_empty() => format!("{} and {last}", first.join(", ")),
        _ => names.concat(),
    }
}

fn placing(environment: &Environment, terminal: &mut Terminal) -> Result<Option<Placing>, Failure> {
    let present = agents::present_names(environment)?;
    if present.is_empty() {
        return Ok(None);
    }
    let question = format!("Configure {} to use {NAME}?", joined(&present));
    if terminal.consent(&question, true)? {
        Placing::on_this_host(environment).map(Some)
    } else {
        Ok(None)
    }
}

/// Writes nothing before the last question is answered yes; a topic it makes carries
/// `machines::PLACEHOLDER`.
pub(super) fn init(
    deps: &Deps,
    environment: &Environment,
    json: bool,
    store: &Path,
    terminal: &mut Terminal,
) -> Result<Output, Failure> {
    setup::refuse_bound(deps)?;
    let starting = machines::holds_nothing(&deps.read())?;
    if starting {
        terminal.say(&format!(
            "{} holds no documents. A store shared between machines may not have synced to \
             this one yet.",
            store.display()
        ));
        if !terminal.consent("Start a new store there?", false)? {
            return Err(Failure::at(store.display(), "no store was started"));
        }
    }
    let (name, held) = machine(&deps.read(), terminal)?;
    let placing = placing(environment, terminal)?;
    terminal.say(&format!("The store: {}", store.display()));
    let how = if held {
        "the topic in the store"
    } else {
        "a new topic"
    };
    terminal.say(&format!("The machine: {name}, {how}"));
    let unsynced = !held && !starting;
    if unsynced {
        terminal.say(&format!(
            "The store holds no topic named {name}. A store shared between machines may not \
             have synced to this one yet."
        ));
    }
    if !terminal.consent("Set this host up?", !unsynced)? {
        return Err(Failure::Refused("nothing was written".to_owned()));
    }
    initialized(placing, json, || setup::init_awaiting_summary(deps, &name))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;
    use crate::app::list;
    use crate::app::testing::{World, head, topic, world, written_at};
    use crate::cli::tests::environment;
    use crate::domain::id::DocumentId;
    use crate::domain::ports::HostRead as _;

    const STORE: &str = "/home/desk/worklog";
    const CONFIRM: &str = "The store: /home/desk/worklog\nThe machine: ";

    fn unsynced(name: &str) -> String {
        format!(
            "{name}, a new topic\nThe store holds no topic named {name}. A store shared between \
             machines may not have synced to this one yet.\nSet this host up? [y/N]: "
        )
    }

    struct Asked {
        result: Result<Output, Failure>,
        shown: String,
    }

    fn asked_in(world: &World, environment: &Environment, typed: &[u8]) -> Asked {
        let mut answers = typed;
        let mut shown = Vec::new();
        let result = {
            let mut terminal = Terminal::new(&mut answers, &mut shown);
            let store = Path::new(STORE);
            init(&world.deps(), environment, false, store, &mut terminal)
        };
        Asked {
            result,
            shown: String::from_utf8(shown).unwrap(),
        }
    }

    fn asked(world: &World, typed: &str) -> Asked {
        asked_in(world, &environment(&|_| None), typed.as_bytes())
    }

    fn unbound(name: Option<&str>) -> (World, DocumentId) {
        let (mut world, lantern) = world();
        world.host.set_machine(None);
        world.host.set_name(name.map(str::to_owned));
        (world, lantern)
    }

    fn printed(asked: &Asked) -> &str {
        match &asked.result {
            Ok(output) => &output.text,
            Err(failure) => panic!("{failure}: {}", asked.shown),
        }
    }

    fn refused(asked: &Asked) -> &str {
        match &asked.result {
            Err(Failure::Refused(text)) => text,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    fn topics(world: &World) -> Vec<String> {
        let listed = list::topics(&world.deps().read(), None, true).unwrap();
        let mut names: Vec<String> = listed.into_iter().map(|topic| topic.row.label).collect();
        names.sort();
        names
    }

    fn summary_of(world: &World) -> String {
        let machine = world.host.machine().unwrap().expect("a bound host");
        head(world, &machine).fields["summary"]
            .as_str()
            .expect("a summary")
            .to_owned()
    }

    #[test]
    fn a_host_named_like_a_topic_is_offered_it_and_an_empty_answer_takes_it() {
        let (world, lantern) = unbound(Some("Lantern.local"));
        let asked = asked(&world, "\n\n");
        assert_eq!(
            asked.shown,
            "This machine is lantern, found in the store.\n\
             Machine topic [lantern]: The store: /home/desk/worklog\n\
             The machine: lantern, the topic in the store\n\
             Set this host up? [Y/n]: "
        );
        assert_eq!(printed(&asked), "lantern bound\n");
        assert_eq!(world.host.machine(), Ok(Some(lantern)));
        assert_eq!(summary_of(&world), "s");
    }

    #[test]
    fn a_name_typed_is_taken_over_the_one_offered() {
        let (world, _) = unbound(Some("lantern"));
        let asked = asked(&world, "  atlas \ny\n");
        assert!(
            asked
                .shown
                .ends_with(&format!("{CONFIRM}{}", unsynced("atlas"))),
            "{}",
            asked.shown
        );
        assert_eq!(printed(&asked), "atlas created\n");
        assert_eq!(topics(&world), ["atlas", "desk", "lantern"]);
    }

    #[test]
    fn a_name_new_to_a_store_that_holds_documents_is_made_only_on_a_typed_yes() {
        let (world, _) = unbound(Some("lantern"));
        let asked = asked(&world, "atlas\n\n");
        assert!(asked.shown.ends_with(&unsynced("atlas")), "{}", asked.shown);
        assert_eq!(refused(&asked), "nothing was written");
        assert_eq!(world.host.machine(), Ok(None));
        assert_eq!(topics(&world), ["desk", "lantern"]);
    }

    #[test]
    fn a_host_no_topic_is_named_like_is_shown_the_machines_and_its_own_name_as_new() {
        let (mut world, lantern) = world();
        let phone = topic(&world, "phone", "");
        let root = head(&world, &lantern);
        let fields = toml::to_string(&root.fields).unwrap();
        let stamp = "2026-10-09T19:00:00+01:00";
        written_at(&world, &root, &fields, stamp, "\n", Some(&phone));
        world.host.set_machine(None);
        world.host.set_name(Some("Atlas".to_owned()));

        let listed = "Machine topics:\n  desk\n  phone\n  atlas (new)\nMachine topic: ";
        let from_the_list = asked(&world, "phone\n\n");
        assert_eq!(
            from_the_list.shown,
            format!("{listed}{CONFIRM}phone, the topic in the store\nSet this host up? [Y/n]: ")
        );
        assert_eq!(printed(&from_the_list), "phone bound\n");
        assert_eq!(world.host.machine(), Ok(Some(phone)));
    }

    #[test]
    fn the_new_name_or_any_other_makes_the_machine_topic_with_the_placeholder_summary() {
        for typed in ["atlas", "case"] {
            let (world, _) = unbound(Some("Atlas"));
            let asked = asked(&world, &format!("{typed}\ny\n"));
            assert_eq!(
                asked.shown,
                format!(
                    "Machine topics:\n  desk\n  atlas (new)\nMachine topic: {CONFIRM}{}",
                    unsynced(typed)
                )
            );
            assert_eq!(printed(&asked), format!("{typed} created\n"));
            assert_eq!(summary_of(&world), machines::PLACEHOLDER);
        }
    }

    #[test]
    fn a_host_with_no_name_to_offer_is_shown_the_machines_alone() {
        for name in [None, Some("the_desk")] {
            let (world, _) = unbound(name);
            let asked = asked(&world, "desk\n\n");
            assert!(
                asked
                    .shown
                    .starts_with("Machine topics:\n  desk\nMachine topic: The store: "),
                "{}",
                asked.shown
            );
            assert_eq!(printed(&asked), "desk bound\n");
        }
    }

    #[test]
    fn a_machine_question_left_empty_with_nothing_offered_is_asked_again() {
        let (world, _) = unbound(Some("Atlas"));
        let asked = asked(&world, "\n\natlas\ny\n");
        let again = "Machine topic: Name the machine topic.\n";
        assert!(
            asked
                .shown
                .contains(&format!("{again}{again}Machine topic: {CONFIRM}atlas")),
            "{}",
            asked.shown
        );
        assert_eq!(printed(&asked), "atlas created\n");
    }

    #[test]
    fn a_name_that_is_none_or_that_several_topics_hold_is_said_and_asked_for_again() {
        let (mut world, _) = world();
        topic(&world, "lantern", "");
        world.host.set_machine(None);
        let asked = asked(&world, "Not_A_Name\nlantern\natlas\ny\n");
        let malformed = "Machine topic: `Not_A_Name` is not a name: ";
        assert!(asked.shown.contains(malformed), "{}", asked.shown);
        let several = "Machine topic: lantern: is held by several documents: ";
        assert!(asked.shown.contains(several), "{}", asked.shown);
        assert!(
            asked
                .shown
                .ends_with(&format!("Machine topic: {CONFIRM}{}", unsynced("atlas"))),
            "{}",
            asked.shown
        );
        assert_eq!(printed(&asked), "atlas created\n");
        assert_eq!(topics(&world), ["atlas", "desk", "lantern", "lantern"]);
    }

    #[test]
    fn a_bad_name_followed_by_the_end_of_the_answers_writes_nothing() {
        let (world, _) = unbound(None);
        let asked = asked(&world, "Not_A_Name\n");
        assert_eq!(
            refused(&asked),
            "the answers ended before the questions; nothing was written"
        );
        assert_eq!(world.host.machine(), Ok(None));
        assert_eq!(topics(&world), ["desk", "lantern"]);
    }

    fn empty(name: Option<&str>) -> World {
        let mut world = World::new();
        world.host.set_machine(None);
        world.host.set_name(name.map(str::to_owned));
        world
    }

    const NOTHING_HELD: &str = "/home/desk/worklog holds no documents. A store shared between \
                                machines may not have synced to this one yet.\n\
                                Start a new store there? [y/N]: ";

    #[test]
    fn a_store_holding_nothing_is_started_only_on_a_yes() {
        for typed in ["\n", "n\n", "NO\n"] {
            let world = empty(Some("desk"));
            let asked = asked(&world, typed);
            assert_eq!(asked.shown, NOTHING_HELD, "{typed:?}");
            assert_eq!(refused(&asked), "/home/desk/worklog: no store was started");
            assert_eq!(world.host.machine(), Ok(None));
            assert_eq!(topics(&world), [""; 0]);
        }
        for yes in ["y", "Y", "yes", "YES"] {
            let world = empty(Some("desk"));
            let asked = asked(&world, &format!("{yes}\ndesk\n\n"));
            assert_eq!(
                asked.shown,
                format!(
                    "{NOTHING_HELD}Machine topics:\n  desk (new)\nMachine topic: \
                     {CONFIRM}desk, a new topic\nSet this host up? [Y/n]: "
                )
            );
            assert_eq!(printed(&asked), "desk created\n");
            assert_eq!(summary_of(&world), machines::PLACEHOLDER);
        }
    }

    #[test]
    fn a_new_store_with_no_host_name_lists_nothing_before_its_question() {
        let world = empty(None);
        let asked = asked(&world, "y\ndesk\n\n");
        assert_eq!(
            asked.shown,
            format!(
                "{NOTHING_HELD}Machine topic: {CONFIRM}desk, a new topic\n\
                 Set this host up? [Y/n]: "
            )
        );
        assert_eq!(printed(&asked), "desk created\n");
    }

    #[test]
    fn a_no_at_the_confirmation_writes_nothing() {
        for typed in ["atlas\nn\n", "atlas\nNo\n", "desk\nn\n"] {
            let (world, _) = unbound(None);
            let asked = asked(&world, typed);
            assert!(asked.shown.ends_with("]: "), "{}", asked.shown);
            assert_eq!(refused(&asked), "nothing was written");
            assert_eq!(world.host.machine(), Ok(None));
            assert_eq!(topics(&world), ["desk", "lantern"]);
        }
    }

    const YES_OR_NO: &str = "Answer y or n.\n";

    #[test]
    fn an_answer_that_is_neither_yes_nor_no_is_asked_for_again() {
        let world = empty(Some("desk"));
        let started = asked(&world, "later\nsure\ny\ndesk\nmaybe\n\n");
        let start = "Start a new store there? [y/N]: ";
        let both = format!("{start}{YES_OR_NO}{start}{YES_OR_NO}{start}Machine topics:");
        assert!(started.shown.contains(&both), "{}", started.shown);
        let confirm = "Set this host up? [Y/n]: ";
        assert!(
            started
                .shown
                .ends_with(&format!("{confirm}{YES_OR_NO}{confirm}")),
            "{}",
            started.shown
        );
        assert_eq!(printed(&started), "desk created\n");

        let world = empty(Some("desk"));
        let declined = asked(&world, "later\nn\n");
        assert_eq!(declined.shown, format!("{NOTHING_HELD}{YES_OR_NO}{start}"));
        assert_eq!(
            refused(&declined),
            "/home/desk/worklog: no store was started"
        );
        assert_eq!(topics(&world), [""; 0]);

        let home = Home::with(&[".codex"]);
        let (world, _) = unbound(None);
        let asked = home.asked(&world, "atlas\nperhaps\nn\ny\n");
        let one = "Configure Codex to use worklog? [Y/n]: ";
        let again = format!("{one}{YES_OR_NO}{one}{CONFIRM}");
        assert!(asked.shown.contains(&again), "{}", asked.shown);
        assert_eq!(printed(&asked), "atlas created\n");
        assert!(!home.skill(".codex").exists());
    }

    #[test]
    fn answers_that_end_before_the_questions_are_a_refusal_that_writes_nothing() {
        for typed in ["", "atlas\n"] {
            let (world, _) = unbound(None);
            let asked = asked(&world, typed);
            assert_eq!(
                refused(&asked),
                "the answers ended before the questions; nothing was written"
            );
            assert!(asked.shown.ends_with(": \n"), "{}", asked.shown);
            assert_eq!(world.host.machine(), Ok(None));
            assert_eq!(topics(&world), ["desk", "lantern"]);
        }
    }

    #[test]
    fn a_host_that_is_bound_is_refused_before_it_is_asked_anything() {
        let (world, _) = world();
        let asked = asked(&world, "atlas\n\n");
        assert_eq!(refused(&asked), "this host is already set up as desk");
        assert_eq!(asked.shown, "");
    }

    struct Home {
        _kept: tempfile::TempDir,
        root: PathBuf,
    }

    impl Home {
        fn with(agents: &[&str]) -> Home {
            let kept = tempfile::tempdir().unwrap();
            let root = std::fs::canonicalize(kept.path()).unwrap();
            for agent in agents {
                std::fs::create_dir_all(root.join("home").join(agent)).unwrap();
            }
            Home { _kept: kept, root }
        }

        fn asked(&self, world: &World, typed: &str) -> Asked {
            let home = self.root.join("home");
            let variable = |name: &str| (name == "HOME").then(|| OsString::from(&home));
            let environment = Environment {
                binary: Some(self.root.join("bin").join(NAME)),
                ..environment(&variable)
            };
            asked_in(world, &environment, typed.as_bytes())
        }

        fn skill(&self, agent: &str) -> PathBuf {
            let skills = self.root.join("home").join(agent).join("skills");
            skills.join(NAME).join("SKILL.md")
        }
    }

    const BOTH: &str = "Configure Claude Code and Codex to use worklog? [Y/n]: ";

    #[test]
    fn the_agents_present_are_installed_for_unless_the_answer_is_no() {
        let home = Home::with(&[".claude", ".codex"]);
        let (world, _) = unbound(None);
        let asked = home.asked(&world, "atlas\n\ny\n");
        assert!(
            asked
                .shown
                .contains(&format!("Machine topic: {BOTH}{CONFIRM}")),
            "{}",
            asked.shown
        );
        let hooks = home.root.join("home/.claude/settings.json");
        assert_eq!(
            printed(&asked),
            format!(
                "atlas created\n{}\n{}\n{}\n{}\n{}\n",
                home.skill(".claude").display(),
                hooks.display(),
                home.skill(".codex").display(),
                home.root.join("home/.codex/rules/worklog.rules").display(),
                home.root.join("home/.codex/hooks.json").display()
            )
        );
        assert!(home.skill(".claude").is_file() && hooks.is_file());

        let declining = Home::with(&[".codex"]);
        let (world, _) = unbound(None);
        let asked = declining.asked(&world, "atlas\nn\ny\n");
        let one = "Configure Codex to use worklog? [Y/n]: ";
        assert!(asked.shown.contains(one), "{}", asked.shown);
        assert_eq!(printed(&asked), "atlas created\n");
        assert!(!declining.skill(".codex").exists());
    }

    #[test]
    fn a_host_with_no_agent_is_not_asked_about_them() {
        let home = Home::with(&[]);
        let (world, _) = unbound(None);
        let asked = home.asked(&world, "atlas\ny\n");
        assert!(!asked.shown.contains("Install"), "{}", asked.shown);
        assert_eq!(printed(&asked), "atlas created\n");
    }

    #[test]
    fn a_yes_for_the_agents_writes_none_of_their_files_before_the_confirmation() {
        let home = Home::with(&[".claude"]);
        let (world, _) = unbound(None);
        let asked = home.asked(&world, "atlas\ny\nn\n");
        assert_eq!(refused(&asked), "nothing was written");
        assert!(!home.skill(".claude").exists());
        assert!(!home.root.join("home/.claude/settings.json").exists());
        assert_eq!(world.host.machine(), Ok(None));
    }

    type Opened = (Result<PathBuf, Failure>, String);

    fn stored_by(
        home: Option<&str>,
        cwd: Option<&str>,
        typed: &str,
        open: &dyn Fn(&Path) -> Result<(), Failure>,
    ) -> Opened {
        let variable = |name: &str| match name {
            "HOME" => home.map(OsString::from),
            _ => None,
        };
        let environment = Environment {
            cwd: cwd.map(PathBuf::from),
            ..environment(&variable)
        };
        let mut answers = typed.as_bytes();
        let mut shown = Vec::new();
        let mut terminal = Terminal::new(&mut answers, &mut shown);
        let result = store(&mut terminal, &environment, open).map(|(path, ())| path);
        (result, String::from_utf8(shown).unwrap())
    }

    fn stored(home: Option<&str>, cwd: Option<&str>, typed: &str) -> Opened {
        stored_by(home, cwd, typed, &|_| Ok(()))
    }

    #[test]
    fn the_store_is_worklog_under_the_home_directory_unless_another_is_typed() {
        let desk = |typed: &str| stored(Some("/home/desk"), Some("/home/desk/projects"), typed);
        let question = "Directory of the store [/home/desk/worklog]: ";
        assert_eq!(
            desk("\n"),
            (Ok(PathBuf::from("/home/desk/worklog")), question.to_owned())
        );
        for (typed, held) in [
            ("/atlas/store\n", "/atlas/store"),
            ("~/sync/store\n", "/home/desk/sync/store"),
            ("~\n", "/home/desk"),
            ("kept\n", "/home/desk/projects/kept"),
            ("../kept\n", "/home/desk/kept"),
        ] {
            assert_eq!(desk(typed).0, Ok(PathBuf::from(held)), "{typed:?}");
        }
    }

    #[test]
    fn a_tilde_before_anything_but_a_slash_is_not_expanded_and_is_asked_for_again() {
        let question = "Directory of the store [/home/desk/worklog]: ";
        let typed = "~atlas/notes\n~atlas\n~/notes\n";
        let (result, shown) = stored(Some("/home/desk"), Some("/home/desk/projects"), typed);
        assert_eq!(result, Ok(PathBuf::from("/home/desk/notes")));
        let said = "Only `~` and `~/` are read as the home directory.\n";
        assert_eq!(shown, format!("{question}{said}{question}{said}{question}"));
    }

    #[test]
    fn an_answer_that_is_not_utf_8_is_asked_for_again() {
        let (world, _) = unbound(None);
        let asked = asked_in(&world, &environment(&|_| None), b"\xff\ndesk\n\n");
        let again = "Machine topic: The answer is not UTF-8.\nMachine topic: ";
        assert!(asked.shown.contains(again), "{}", asked.shown);
        assert_eq!(printed(&asked), "desk bound\n");
    }

    #[test]
    fn the_agents_are_named_with_one_and_before_the_last() {
        assert_eq!(joined(&["Codex"]), "Codex");
        assert_eq!(joined(&["Claude Code", "Codex"]), "Claude Code and Codex");
        assert_eq!(
            joined(&["lantern", "atlas", "phone"]),
            "lantern, atlas and phone"
        );
    }

    #[test]
    fn a_store_answer_that_names_no_directory_on_this_host_is_asked_for_again() {
        let question = "Directory of the store: ";
        let (result, shown) = stored(None, None, "\n~/store\nstore\n/atlas/store\n");
        assert_eq!(result, Ok(PathBuf::from("/atlas/store")));
        assert_eq!(
            shown,
            format!(
                "{question}Name the directory.\n{question}$HOME: not set\n\
                 {question}no working directory\n{question}"
            )
        );
        let ended = stored(None, None, "store\n").0;
        assert_eq!(
            ended,
            Err(Failure::Refused(
                "the answers ended before the questions; nothing was written".to_owned()
            ))
        );
    }

    #[test]
    fn a_store_that_does_not_open_is_said_and_asked_for_again() {
        let open = |path: &Path| match path.to_str() {
            Some("/atlas/store") => Ok(()),
            _ => Err(Failure::at(
                path.display(),
                "is not empty and holds no store",
            )),
        };
        let question = "Directory of the store [/home/desk/worklog]: ";
        let (result, shown) = stored_by(Some("/home/desk"), None, "\n/atlas/store\n", &open);
        assert_eq!(result, Ok(PathBuf::from("/atlas/store")));
        assert_eq!(
            shown,
            format!("{question}/home/desk/worklog: is not empty and holds no store\n{question}")
        );
    }
}
