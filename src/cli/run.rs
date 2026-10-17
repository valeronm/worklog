use std::path::Path;

use super::args::{ChangeCommand, Command, DraftCommand, ListCommand, NewWhat, ReadCommand};
use super::render::{Output, output};
use super::{Environment, agents};
use crate::app::followup::{NewFollowup, TriggerArg};
use crate::app::search::Query;
use crate::app::{
    Deps, Failure, ReadDeps, amend, bulk, check, claim, context, draft, followup, fork, list, save,
    search, setup, show, usage,
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

fn trigger<'a>(when: &'a str, what: Option<&'a str>) -> Result<TriggerArg<'a>, Failure> {
    let Some(what) = what else {
        return Err(Failure::Usage(
            "a trigger is `<date> <why>` or `touching <topic>`".to_owned(),
        ));
    };
    Ok(match when {
        "touching" => TriggerArg::Touching(what),
        on => TriggerArg::LookAgain { on, why: what },
    })
}

fn initialized(
    deps: &Deps,
    environment: &Environment,
    json: bool,
    name: &str,
    summary: Option<&str>,
    with_agents: bool,
) -> Result<Output, Failure> {
    let placing = with_agents
        .then(|| agents::Placing::on_this_host(environment))
        .transpose()?;
    agents::initialized(placing, json, || setup::init(deps, name, summary))
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
            body,
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
                body: body.as_deref(),
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
        } => followup::set_trigger(deps, followup, &trigger(when, what.as_deref())?)?,
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
    deps: &ReadDeps,
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
        ReadCommand::Context { dir, .. } => {
            let directory = directory(environment, dir.as_deref())?;
            output(&context::context(deps, &directory)?, json)
        }
        ReadCommand::Due { dir } => {
            let directory = directory(environment, dir.as_deref())?;
            output(&context::due(deps, &directory)?, json)
        }
        ReadCommand::Check => output(&check::check(deps)?, json),
        ReadCommand::Usage { machine, since } => output(
            &usage::usage(deps, machine.as_deref(), since.as_deref())?,
            json,
        ),
    })
}

fn listed(deps: &ReadDeps, json: bool, command: &ListCommand) -> Result<Output, Failure> {
    Ok(match command {
        ListCommand::Drafts => output(&draft::drafts(deps)?, json),
        ListCommand::Topics { topic, ended } => {
            output(&list::topics(deps, topic.as_deref(), *ended)?, json)
        }
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
        Command::Init {
            name: Some(name),
            summary,
            agents: with_agents,
            ..
        } => initialized(
            deps,
            environment,
            json,
            name,
            summary.as_deref(),
            *with_agents,
        ),
        Command::Init { name: None, .. } => {
            unreachable!("an `init` given no name is asked for one before a command is run")
        }
        Command::Draft(command) => drafted(deps, json, command),
        Command::Change(command) => changed(deps, environment, json, command),
        Command::Read(command) => read(&deps.read(), environment, json, command),
        Command::List(command) => listed(&deps.read(), json, command),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::app::testing::{
        World, claim_fields, entry, fact, followup, fork, topic, world_with_atlas,
    };
    use crate::cli::NAME;
    use crate::cli::args::{Cli, Invoked};
    use crate::cli::render::{CONTEXT_BYTES, assert_fits};
    use crate::cli::tests::environment;
    use crate::domain::id::DocumentId;
    use crate::domain::ports::Usage as _;
    use crate::domain::testing::ran;

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
            let cli = Cli::parsed([&[NAME], line].concat()).expect("a command");
            let Invoked::Run(command) = &cli.command else {
                panic!("{line:?} returns no result");
            };
            let environment = Environment {
                cwd: cwd.map(PathBuf::from),
                ..environment(&|_| None)
            };
            run(&self.world.deps(), &environment, cli.json, command)
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

        fn claim(&self, cwd: &str, line: &[&str]) -> DocumentId {
            let stored = self.text_in(cwd, &[line, &["--json"]].concat());
            let stored: serde_json::Value = serde_json::from_str(&stored).expect("one value");
            let id = stored["document"].as_str().expect("a claim's id");
            DocumentId::parse(id).expect("a whole id")
        }

        fn claimed(&self, cwd: &str, line: &[&str]) -> String {
            self.claim(cwd, line).short().to_owned()
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
    fn a_trigger_is_a_date_with_a_why_or_a_topic_after_touching() {
        let fixture = Fixture::new();
        let id = fixture.followup("");
        let shown = || fixture.text(&["show", &id]);

        fixture.stored(&["trigger", &id, "2026-11-01", "The part arrives"], &id);
        assert!(shown().contains("look_again = 2026-11-01"), "{}", shown());
        fixture.stored(&["trigger", &id, "touching", "lantern"], &id);
        assert!(shown().contains("touching = "), "{}", shown());
        assert!(!shown().contains("look_again"), "{}", shown());

        assert_eq!(
            usage(fixture.run(&["trigger", &id, "someday", "why"])),
            "`someday` is not a date (YYYY-MM-DD)"
        );
        for line in [
            &["trigger", &id, "2026-11-01"][..],
            &["trigger", &id, "touching"],
            &["trigger", &id, "none"],
        ] {
            assert_eq!(
                usage(fixture.run(line)),
                "a trigger is `<date> <why>` or `touching <topic>`",
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

        let opened = |first: &str, text: String| {
            let blocks = format!("{first}: s\n  (no facts)\n\ndesk — this machine: s\n");
            assert!(text.starts_with(&blocks), "{text}");
        };
        opened(
            "lantern — claimed at ~/projects/lantern",
            fixture.text(&["context"]),
        );
        assert_eq!(
            fixture.text_in("/home/desk", &["context", "projects/lantern/case"]),
            fixture.text(&["context"])
        );
        opened(
            "atlas — claimed anywhere",
            fixture.text_in("/home/desk", &["context"]),
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
    fn what_is_due_is_listed_for_the_working_directory_or_the_one_given() {
        let fixture = Fixture::new();
        fixture.claimed(PROJECT, &["claim", "lantern"]);
        assert_eq!(fixture.text(&["due"]), "");
        assert_eq!(fixture.text(&["due", "--json"]), "[]\n");
        let id = fixture.followup("look_again = 2026-01-05\nwhy = \"w\"\n");
        fixture.followup("");

        let line = format!("due 2026-01-05  {id}  Check the driver\n");
        assert_eq!(fixture.text(&["due"]), line);
        assert_eq!(fixture.text(&["followups"]).lines().count(), 2);
        assert_eq!(fixture.text_in("/home/desk", &["due"]), "");
        assert_eq!(
            fixture.text_in("/home/desk", &["due", "link/case/.."]),
            line
        );
        let opened = fixture.text(&["context"]);
        let heading = "due now — `worklog show <id>` for one, `worklog due`, run here, for all:\n";
        assert!(
            opened.starts_with(&format!("{heading}{line}\n")),
            "{opened}"
        );
        assert_eq!(
            refused(fixture.run_in(None, &["due"])),
            "no working directory"
        );
    }

    #[test]
    fn a_session_over_more_facts_than_fit_counts_the_machine_s_and_keeps_what_is_due() {
        let fixture = Fixture::new();
        let (world, lantern) = (&fixture.world, &fixture.lantern);
        fixture.claimed(PROJECT, &["claim", "lantern"]);
        for n in 0..3 {
            fact(world, lantern, &format!("relay-{n}"), "");
        }
        for n in 0..700 {
            fact(world, &world.machine(), &format!("socket-{n}-is-fused"), "");
        }
        let due: Vec<String> = (0..3)
            .map(|_| fixture.followup("look_again = 2026-01-05\nwhy = \"w\"\n"))
            .collect();

        let text = fixture.text(&["context"]);
        assert_fits(&text);
        assert!(text.starts_with("due now — "), "{text}");
        assert_eq!(
            text.lines().nth(1).map(|line| &line[..16]),
            Some("due 2026-01-05  ")
        );
        for id in &due {
            let line = format!("due 2026-01-05  {id}  Check the driver\n");
            assert!(text.contains(&line), "{text}");
        }
        assert!(
            text.contains(
                "lantern — claimed at ~/projects/lantern: s\n  relay-0, relay-1, relay-2\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("desk — this machine: s\n  (700 facts)\n"),
            "{text}"
        );

        let whole = fixture.text(&["context", "--json"]);
        assert!(whole.len() > CONTEXT_BYTES, "{}", whole.len());
        let whole: serde_json::Value = serde_json::from_str(&whole).expect("one value");
        assert_eq!(
            whole["topics"][1]["facts"].as_array().map(Vec::len),
            Some(700)
        );
    }

    #[test]
    fn a_claim_is_printed_by_its_label_and_addressed_by_its_short_id() {
        let fixture = Fixture::new();
        let id = fixture.claim(PROJECT, &["claim", "lantern"]);
        let claim = id.short().to_owned();
        let label = "lantern at ~/projects/lantern";
        let logged = fixture.text(&["log"]);
        assert!(logged.contains(&format!("  {label}  claim\n")), "{logged}");

        let fields = claim_fields(
            &fixture.world.machine(),
            &fixture.lantern,
            "~/projects/lantern",
        );
        fork(&fixture.world, &id, &fields);
        let forks = fixture.text(&["forks"]);
        let row = format!("{claim}  {label}  forked\n");
        assert!(forks.starts_with(&row), "{forks}");
        let opened = fixture.text(&["context"]);
        let forked = format!(
            "Forked — `worklog forks` lists each with its id, \
             `worklog resolve <id>` opens one: {label}\n\nlantern — "
        );
        assert!(opened.starts_with(&forked), "{opened}");
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
        let opened = fixture.text_in("/home/desk", &["context", "link/case/.."]);
        assert!(
            opened
                .starts_with("lantern — claimed at ~/projects/lantern: s\n  (no facts)\n\ndesk — "),
            "{opened}"
        );
        fixture.text_in("/home/desk/projects", &["unclaim", "lantern", "../link"]);
        assert_eq!(fixture.text(&["where"]), "");
    }

    #[test]
    fn usage_counts_the_runs_logged_for_a_machine_topic_and_since_a_day() {
        let fixture = Fixture::new();
        let world = &fixture.world;
        for (command, day) in [("show", "2026-09-01"), ("facts", "2026-09-02")] {
            let run = ran(&world.machine(), command, day);
            world.usage.record(&run).unwrap();
        }
        let counted = fixture.text(&["usage"]);
        assert_eq!(counted, "desk — 2 runs\n      1 facts\n      1 show\n");
        assert_eq!(fixture.text(&["usage", "--machine", "desk"]), counted);
        let none = fixture.run(&["usage", "--machine", "atlas"]).unwrap();
        assert_eq!((none.text.as_str(), none.exit), ("", 0));
        assert_eq!(none.notes, ["nothing logged"]);
        assert_eq!(
            fixture.text(&["usage", "--since", "2026-09-02"]),
            "desk — 1 run\n      1 facts\n"
        );
        let later = ["usage", "--since", "2026-09-03", "--json"];
        assert_eq!(fixture.text(&later), "[]\n");
        assert_eq!(
            usage(fixture.run(&["usage", "--since", "soon"])),
            "`soon` is not a date (YYYY-MM-DD)"
        );
        assert_eq!(
            refused(fixture.run(&["usage", "--machine", "torch"])),
            "torch: names no document"
        );
        assert_eq!(fixture.run(&["search", "lantern"]).unwrap().hits, Some(1));
        assert_eq!(fixture.run(&["topics"]).unwrap().hits, None);
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
