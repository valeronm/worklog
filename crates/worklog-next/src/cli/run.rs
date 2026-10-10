use std::path::Path;

use super::Environment;
use super::args::{ChangeCommand, Command, DraftCommand, ListCommand, NewWhat, ReadCommand};
use super::render::{Output, output};
use crate::app::followup::{NewFollowup, TriggerArg};
use crate::app::search::Query;
use crate::app::{
    Deps, Failure, amend, bulk, check, claim, context, draft, followup, fork, list, save, search,
    setup, show,
};
use crate::domain::schema::Reason;

fn directory(environment: &Environment, given: Option<&Path>) -> Result<String, Failure> {
    let path = match given {
        Some(given) => environment.joined(given)?,
        None => environment.working()?.to_path_buf(),
    };
    path.into_os_string()
        .into_string()
        .map_err(|path| Failure::Usage(format!("{}: is not UTF-8", Path::new(&path).display())))
}

fn claimed(
    environment: &Environment,
    dir: Option<&Path>,
    anywhere: bool,
) -> Result<Option<String>, Failure> {
    if anywhere {
        return Ok(None);
    }
    directory(environment, dir).map(Some)
}

fn trigger<'a>(when: &'a str, what: Option<&'a str>) -> Result<Option<TriggerArg<'a>>, Failure> {
    match (when, what) {
        ("none", None) => Ok(None),
        ("touching", Some(topic)) => Ok(Some(TriggerArg::Touching(topic))),
        (on, Some(why)) if on != "none" => Ok(Some(TriggerArg::LookAgain { on, why })),
        _ => Err(Failure::Usage(
            "a trigger is `<date> <why>`, `touching <topic>` or `none`".to_owned(),
        )),
    }
}

fn opened(deps: &Deps, json: bool, new: &draft::New) -> Result<Output, Failure> {
    Ok(output(&draft::new(deps, new)?, json))
}

fn new(deps: &Deps, json: bool, what: &NewWhat) -> Result<Output, Failure> {
    match what {
        NewWhat::Topic { name } => opened(deps, json, &draft::New::Topic { name }),
        NewWhat::Fact { address } => opened(
            deps,
            json,
            &draft::New::Fact {
                address,
                idea: false,
            },
        ),
        NewWhat::Idea { address } => opened(
            deps,
            json,
            &draft::New::Fact {
                address,
                idea: true,
            },
        ),
        NewWhat::Entry { name, date } => opened(
            deps,
            json,
            &draft::New::Entry {
                name,
                date: date.as_deref(),
            },
        ),
        NewWhat::Followup {
            topics,
            summary,
            look_again,
            why,
            touching,
            entry,
            about,
        } => {
            let topics: Vec<&str> = topics.iter().map(String::as_str).collect();
            let trigger = match (look_again, why, touching) {
                (Some(on), Some(why), _) => Some(TriggerArg::LookAgain { on, why }),
                (_, _, Some(topic)) => Some(TriggerArg::Touching(topic)),
                _ => None,
            };
            let what = NewFollowup {
                topics: &topics,
                summary: summary.as_deref(),
                entry: entry.as_deref(),
                about: about.as_deref(),
                trigger,
            };
            Ok(output(&followup::new_followup(deps, &what)?, json))
        }
    }
}

fn drafted(deps: &Deps, json: bool, command: &DraftCommand) -> Result<Output, Failure> {
    Ok(match command {
        DraftCommand::New { what } => new(deps, json, what)?,
        DraftCommand::Checkout { target } => output(&draft::checkout(deps, target)?, json),
        DraftCommand::Save { target } => output(&save::save(deps, target)?, json),
        DraftCommand::Discard { target } => output(&draft::discard(deps, target)?, json),
        DraftCommand::Resolve { target } => output(&fork::resolve(deps, target)?, json),
    })
}

fn changed(
    deps: &Deps,
    environment: &Environment,
    json: bool,
    command: &ChangeCommand,
) -> Result<Output, Failure> {
    let note = |note: &Option<String>| note.clone().unwrap_or_default();
    let written = match command {
        ChangeCommand::Rename { target, name } => amend::rename(deps, target, name)?,
        ChangeCommand::Move { from, to, targets } => {
            let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
            return Ok(output(&bulk::move_to(deps, &targets, from, to)?, json));
        }
        ChangeCommand::Verify { fact } => amend::verify(deps, fact)?,
        ChangeCommand::End {
            target,
            reason,
            note: said,
            by,
        } => amend::end(deps, target, reason, &note(said), by.as_deref())?,
        ChangeCommand::Reopen { target, why } => amend::reopen(deps, target, why)?,
        ChangeCommand::Done {
            followup,
            note: said,
        } => amend::end(deps, followup, Reason::Done.word(), &note(said), None)?,
        ChangeCommand::Drop {
            followup,
            note: said,
        } => amend::end(deps, followup, Reason::Dropped.word(), &note(said), None)?,
        ChangeCommand::Trigger {
            followup,
            when,
            what,
        } => followup::set_trigger(deps, followup, trigger(when, what.as_deref())?.as_ref())?,
        ChangeCommand::Claim {
            topic,
            dir,
            anywhere,
        } => {
            let directory = claimed(environment, dir.as_deref(), *anywhere)?;
            claim::claim(deps, topic, directory.as_deref())?
        }
        ChangeCommand::Unclaim {
            topic,
            dir,
            anywhere,
        } => {
            let directory = claimed(environment, dir.as_deref(), *anywhere)?;
            claim::unclaim(deps, topic, directory.as_deref())?
        }
    };
    Ok(output(&written, json))
}

fn read(
    deps: &Deps,
    environment: &Environment,
    json: bool,
    command: &ReadCommand,
) -> Result<Output, Failure> {
    Ok(match command {
        ReadCommand::Show { target } => output(&show::show(deps, target)?, json),
        ReadCommand::History { target } => output(&show::history(deps, target)?, json),
        ReadCommand::Diff { target, other } => {
            output(&show::diff(deps, target, other.as_deref())?, json)
        }
        ReadCommand::Log { n, machine } => {
            output(&search::log(deps, *n, machine.as_deref())?, json)
        }
        ReadCommand::Search {
            term,
            regex,
            topic,
            ended,
        } => {
            let query = Query {
                term,
                regex: *regex,
                topic: topic.as_deref(),
                ended: *ended,
            };
            output(&search::search(deps, &query)?, json)
        }
        ReadCommand::Context { dir } => {
            let directory = directory(environment, dir.as_deref())?;
            output(&context::context(deps, &directory)?, json)
        }
        ReadCommand::Check => output(&check::check(deps)?, json),
    })
}

fn listed(deps: &Deps, json: bool, command: &ListCommand) -> Result<Output, Failure> {
    Ok(match command {
        ListCommand::Drafts => output(&draft::drafts(deps)?, json),
        ListCommand::Topics { ended } => output(&list::topics(deps, *ended)?, json),
        ListCommand::Facts { topic, ended } => {
            output(&list::facts(deps, topic.as_deref(), *ended)?, json)
        }
        ListCommand::Ideas { topic, ended } => {
            output(&list::ideas(deps, topic.as_deref(), *ended)?, json)
        }
        ListCommand::Entries { topic, n, ended } => {
            output(&list::entries(deps, topic.as_deref(), *n, *ended)?, json)
        }
        ListCommand::Followups { about, ended } => {
            output(&list::followups(deps, about.as_deref(), *ended)?, json)
        }
        ListCommand::Where { topic, machine } => output(
            &list::where_(deps, topic.as_deref(), machine.as_deref())?,
            json,
        ),
        ListCommand::Forks => output(&list::forks(deps)?, json),
    })
}

pub(super) fn run(
    deps: &Deps,
    environment: &Environment,
    json: bool,
    command: &Command,
) -> Result<Output, Failure> {
    match command {
        Command::Init { name, summary, .. } => {
            Ok(output(&setup::init(deps, name, summary.as_deref())?, json))
        }
        Command::Draft(command) => drafted(deps, json, command),
        Command::Change(command) => changed(deps, environment, json, command),
        Command::Read(command) => read(deps, environment, json, command),
        Command::List(command) => listed(deps, json, command),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser as _;

    use super::*;
    use crate::app::testing::{
        World, claim_fields, entry, fact, followup, fork, topic, world_with_atlas,
    };
    use crate::cli::args::Cli;
    use crate::domain::id::DocumentId;
    use crate::domain::ports::Store as _;

    const PROJECT: &str = "/home/desk/projects/lantern";

    struct Fixture {
        world: World,
        lantern: DocumentId,
    }

    impl Fixture {
        fn new() -> Fixture {
            let (mut world, lantern, _) = world_with_atlas();
            world.host.set_home(Some("/home/desk".to_owned()));
            world.host.link("/home/desk/link", PROJECT);
            Fixture { world, lantern }
        }

        fn run_in(&self, cwd: Option<&str>, line: &[&str]) -> Result<Output, Failure> {
            let cli = Cli::try_parse_from([&["worklog-next"], line].concat()).expect("a command");
            let environment = Environment {
                cwd: cwd.map(PathBuf::from),
                variable: &|_| None,
            };
            run(&self.world.deps(), &environment, cli.json, &cli.command)
        }

        fn run(&self, line: &[&str]) -> Result<Output, Failure> {
            self.run_in(Some(PROJECT), line)
        }

        fn text_in(&self, cwd: &str, line: &[&str]) -> String {
            let output = self.run_in(Some(cwd), line).unwrap_or_else(|failure| {
                panic!("{line:?} failed: {failure}");
            });
            assert_eq!((output.exit, output.notes.len()), (0, 0), "{line:?}");
            output.text
        }

        fn text(&self, line: &[&str]) -> String {
            self.text_in(PROJECT, line)
        }

        fn stored(&self, line: &[&str], label: &str) {
            let text = self.text(line);
            let version = text
                .strip_prefix(&format!("{label} "))
                .and_then(|rest| rest.strip_suffix('\n'))
                .unwrap_or_else(|| panic!("{line:?} printed {text:?}"));
            assert_eq!(version.len(), 12, "{text}");
        }

        fn claimed(&self, cwd: &str, line: &[&str]) -> String {
            let stored = self.text_in(cwd, &[line, &["--json"]].concat());
            let stored: serde_json::Value = serde_json::from_str(&stored).expect("one value");
            stored["document"].as_str().expect("a claim's id")[..8].to_owned()
        }

        fn followup(&self, rest: &str) -> String {
            let id = followup(&self.world, &[&self.lantern], "Check the driver", rest);
            id.short().to_owned()
        }
    }

    fn usage(result: Result<Output, Failure>) -> String {
        match result {
            Err(Failure::Usage(text)) => text,
            other => panic!("expected a usage failure, got {other:?}"),
        }
    }

    fn refused(result: Result<Output, Failure>) -> String {
        match result {
            Err(Failure::Refused(text)) => text,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_trigger_is_a_date_with_a_why_a_topic_after_touching_or_none() {
        let fixture = Fixture::new();
        let id = fixture.followup("");
        let shown = || fixture.text(&["show", &id]);

        fixture.stored(&["trigger", &id, "2026-11-01", "The part arrives"], &id);
        assert!(shown().contains("look_again = 2026-11-01"), "{}", shown());
        fixture.stored(&["trigger", &id, "touching", "lantern"], &id);
        assert!(shown().contains("touching = "), "{}", shown());
        fixture.stored(&["trigger", &id, "none"], &id);
        assert!(!shown().contains("touching"), "{}", shown());
        assert!(!shown().contains("look_again"), "{}", shown());

        assert_eq!(
            usage(fixture.run(&["trigger", &id, "someday", "why"])),
            "`someday` is not a date (YYYY-MM-DD)"
        );
        for line in [
            &["trigger", &id, "2026-11-01"][..],
            &["trigger", &id, "touching"],
            &["trigger", &id, "none", "lantern"],
        ] {
            assert_eq!(
                usage(fixture.run(line)),
                "a trigger is `<date> <why>`, `touching <topic>` or `none`",
                "{line:?}"
            );
        }
        assert_eq!(
            refused(fixture.run(&["trigger", &id, "touching", "atlas"])),
            "atlas: is not one of the followup's topics"
        );
    }

    #[test]
    fn a_new_followup_takes_its_date_its_entry_and_what_it_is_about() {
        let fixture = Fixture::new();
        entry(&fixture.world, "2026-10-08", "wiring", &[&fixture.lantern]);
        fact(&fixture.world, &fixture.lantern, "relay", "");
        let stored = fixture.text(&[
            "new",
            "followup",
            "--topics",
            "lantern",
            "--summary",
            "Order a fuse",
            "--entry",
            "2026-10-08-wiring",
            "--about",
            "lantern/relay",
            "--look-again",
            "2026-11-01",
            "--why",
            "The part arrives",
        ]);
        let id = stored.split(' ').next().expect("an id");
        assert_eq!(
            fixture.text(&["followups", "2026-10-08-wiring"]),
            format!("by 2026-11-01  {id}  Order a fuse\n")
        );
        assert_eq!(fixture.text(&["followups", "atlas"]), "");
        assert!(
            fixture
                .text(&["show", id])
                .contains("look_again = 2026-11-01")
        );

        let bad_date = [
            "new",
            "followup",
            "--topics",
            "lantern",
            "--summary",
            "s",
            "--look-again",
            "soon",
            "--why",
            "w",
        ];
        assert_eq!(
            usage(fixture.run(&bad_date)),
            "`soon` is not a date (YYYY-MM-DD)"
        );
        assert_eq!(
            usage(fixture.run(&["new", "entry", "soldering", "--date", "soon"])),
            "`soon` is not a date (YYYY-MM-DD)"
        );
    }

    #[test]
    fn an_ending_without_a_note_is_refused_unless_the_reason_is_done() {
        let fixture = Fixture::new();
        let relay = fact(&fixture.world, &fixture.lantern, "relay", "");
        let (done, dropped) = (fixture.followup(""), fixture.followup(""));
        assert_eq!(
            refused(fixture.run(&["end", "lantern/relay", "false"])),
            "lantern/relay: an ending says why in its note"
        );
        assert_eq!(
            refused(fixture.run(&["drop", &dropped])),
            format!("{dropped}: an ending says why in its note")
        );
        assert_eq!(
            refused(fixture.run(&["end", "lantern/relay", "done", "n"])),
            "lantern/relay: a fact does not end as done"
        );
        fixture.stored(&["done", &done], &done);
        fixture.stored(&["end", &dropped, "dropped", "No longer wanted"], &dropped);
        assert_eq!(
            fixture.text(&["facts"]),
            format!("2026-09-04  {}  lantern/relay  s\n", relay.short())
        );
        assert_eq!(fixture.text(&["followups"]), "");
    }

    #[test]
    fn a_directory_left_out_is_the_working_directory_and_anywhere_is_none() {
        let fixture = Fixture::new();
        let here = fixture.claimed(PROJECT, &["claim", "lantern"]);
        let anywhere = fixture.claimed(PROJECT, &["claim", "atlas", "--anywhere"]);
        let placed = format!("lantern  ~/projects/lantern  {here}\n");
        assert_eq!(
            fixture.text(&["where"]),
            format!("atlas  (anywhere)  {anywhere}\n{placed}")
        );
        assert_eq!(fixture.text(&["where", "lantern"]), placed);
        assert_eq!(
            fixture.text(&["where", "--machine", "desk"]),
            fixture.text(&["where"])
        );
        assert_eq!(fixture.text(&["where", "--machine", "atlas"]), "");

        assert_eq!(fixture.text(&["context"]), "lantern  s\ndesk  s\n");
        assert_eq!(
            fixture.text_in("/home/desk", &["context", "projects/lantern/case"]),
            fixture.text(&["context"])
        );
        assert_eq!(
            fixture.text_in("/home/desk", &["context"]),
            "atlas  s\ndesk  s\n"
        );

        fixture.stored(&["unclaim", "lantern"], "lantern at ~/projects/lantern");
        assert_eq!(
            fixture.text(&["where"]),
            format!("atlas  (anywhere)  {anywhere}\n")
        );
        fixture.stored(&["unclaim", "atlas", "--anywhere"], "atlas anywhere");
        assert_eq!(fixture.text(&["where"]), "");

        for line in [
            &["claim", "lantern"][..],
            &["unclaim", "lantern"],
            &["claim", "lantern", "projects"],
            &["context"],
        ] {
            assert_eq!(
                refused(fixture.run_in(None, line)),
                "no working directory",
                "{line:?}"
            );
        }
        assert!(
            fixture
                .run_in(None, &["claim", "lantern", "--anywhere"])
                .is_ok()
        );
    }

    #[test]
    fn a_claim_is_printed_by_its_label_and_addressed_by_its_short_id() {
        let fixture = Fixture::new();
        let claim = fixture.claimed(PROJECT, &["claim", "lantern"]);
        let label = "lantern at ~/projects/lantern";
        let logged = fixture.text(&["log"]);
        assert!(logged.contains(&format!("  {label}  claim\n")), "{logged}");

        let id = fixture
            .world
            .store
            .documents_under(&claim)
            .unwrap()
            .remove(0);
        let fields = claim_fields(
            &fixture.world.machine(),
            &fixture.lantern,
            "~/projects/lantern",
        );
        fork(&fixture.world, &id, &fields);
        let forks = fixture.text(&["forks"]);
        let row = format!("{claim}  {label}  forked\n");
        assert!(forks.starts_with(&row), "{forks}");
        let shown = fixture.run(&["show", &claim]).unwrap();
        assert_eq!(shown.notes, [format!("{label}: forked: resolve it")]);
        assert_eq!(
            refused(fixture.run(&["unclaim", "lantern"])),
            format!("{label}: is forked; resolve it first")
        );
        usage(fixture.run(&["show", label]));
    }

    #[test]
    fn a_directory_given_is_read_as_the_host_resolves_it_by_every_command() {
        let fixture = Fixture::new();
        let claim = fixture.claimed("/home/desk", &["claim", "lantern", "link"]);
        assert_eq!(
            fixture.text(&["where"]),
            format!("lantern  ~/projects/lantern  {claim}\n")
        );
        assert_eq!(
            fixture.text_in("/home/desk", &["context", "link/case/.."]),
            "lantern  s\ndesk  s\n"
        );
        fixture.text_in("/home/desk/projects", &["unclaim", "lantern", "../link"]);
        assert_eq!(fixture.text(&["where"]), "");
    }

    #[test]
    fn a_listing_is_narrowed_by_its_topic_and_widened_by_ended() {
        let fixture = Fixture::new();
        let (world, lantern) = (&fixture.world, &fixture.lantern);
        let phone = topic(world, "phone", "");
        let relay = fact(world, lantern, "relay", "");
        let dimmer = fact(world, lantern, "dimmer", "idea = true\n");
        let case = fact(world, &phone, "case", "idea = true\n");
        let fuse = fact(
            world,
            lantern,
            "fuse",
            "ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n",
        );
        let wiring = entry(world, "2026-10-08", "wiring", &[lantern]);
        entry(world, "2026-10-07", "casing", &[&phone]);
        let confirmed =
            |id: &DocumentId, rest: &str| format!("2026-09-04  {}  {rest}\n", id.short());

        let live = confirmed(&relay, "lantern/relay  s");
        assert_eq!(fixture.text(&["facts"]), live);
        assert_eq!(
            fixture.text(&["facts", "--ended"]),
            confirmed(&fuse, "lantern/fuse  s  ended false") + &live
        );
        assert_eq!(
            fixture.text(&["ideas", "lantern"]),
            confirmed(&dimmer, "lantern/dimmer  s")
        );
        assert_eq!(
            fixture.text(&["ideas", "phone"]),
            confirmed(&case, "phone/case  s")
        );
        let wired = format!("2026-10-08  {}  2026-10-08-wiring  s\n", wiring.short());
        assert_eq!(fixture.text(&["entries", "lantern"]), wired);
        assert_eq!(fixture.text(&["entries", "-n", "1"]), wired);
        assert_eq!(
            refused(fixture.run(&["facts", "torch"])),
            "torch: names no document"
        );
    }

    #[test]
    fn the_reads_of_one_document_and_of_the_whole_store_each_print_their_lines() {
        let fixture = Fixture::new();
        let relay = fact(&fixture.world, &fixture.lantern, "relay", "");
        let hit = format!("{}  lantern/relay  s\n    The relay.\n", relay.short());
        assert_eq!(fixture.text(&["search", "relay"]), hit);
        assert_eq!(
            fixture.text(&["search", "^the", "--regex", "--topic", "lantern"]),
            hit
        );
        assert_eq!(fixture.text(&["search", "relay", "--topic", "atlas"]), "");
        assert_eq!(fixture.text(&["log"]).lines().count(), 4);
        assert_eq!(fixture.text(&["log", "1"]).lines().count(), 1);
        assert_eq!(fixture.text(&["history", "lantern"]).lines().count(), 1);

        let listed = fixture.text(&["history", "lantern"]);
        let version = &listed[..12];
        fixture.text(&["checkout", "lantern"]);
        assert_eq!(
            fixture.text(&["diff", "lantern"]),
            format!("--- {version}\n+++ lantern\nno changes\n")
        );
        assert_eq!(
            usage(fixture.run(&["history", version])),
            format!("{version}: names a version; a history is of a document")
        );
    }
}
