use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::app::lookup::Lookup;
use crate::app::rows::{Reference, reading};
use crate::app::{Failure, ReadDeps, day_given};
use crate::domain::id::DocumentId;
use crate::domain::schema::address::Found;
use crate::domain::session::Session;
use crate::domain::usage::{Agent, Invocation, RunFilter};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub text: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommandCount {
    pub command: String,
    pub run_count: usize,
    /// Most often first, then by text.
    pub refusals: Vec<Refusal>,
}

impl CommandCount {
    #[must_use]
    pub fn refused_count(&self) -> usize {
        self.refusals.iter().map(|refusal| refusal.count).sum()
    }
}

/// `agent` is `None` for the runs made in no agent's session, and their `session_count` is 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgentUsage {
    pub agent: Option<String>,
    pub run_count: usize,
    pub session_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MachineUsage {
    /// Labeled by its whole id where no topic answers for it.
    pub machine: Reference,
    /// The name of the topic that answers for the machine.
    pub topic: Option<String>,
    /// Most runs first, then by name, the runs of no agent last among equals.
    pub agents: Vec<AgentUsage>,
    pub commands: Vec<CommandCount>,
}

impl MachineUsage {
    #[must_use]
    pub fn run_count(&self) -> usize {
        self.commands.iter().map(|count| count.run_count).sum()
    }

    #[must_use]
    pub fn refused_count(&self) -> usize {
        self.commands.iter().map(CommandCount::refused_count).sum()
    }
}

/// How often each command was run, by the machine that ran it: the machines by label, and a
/// machine's commands most run first, then by name, each with the refusals it was answered
/// and the machine's runs split by the agent whose session made them.
///
/// `machine` names a topic, by its name or its id as any document is named, and keeps the
/// runs of that machine; the whole id of a machine no topic answers for is taken where the
/// log holds a run of it, and any other address that is not one topic is refused. `since` is
/// `YYYY-MM-DD` and keeps the runs written on that day and after, a run's day being the date
/// its stamp is written with, at the offset of the host that ran it; anything else is a usage
/// failure. `agent` keeps the runs made in one agent's sessions, or in no session. A command
/// line that named no command is not counted.
pub fn usage(
    deps: &ReadDeps,
    machine: Option<&str>,
    since: Option<&str>,
    agent: Option<Agent>,
) -> Result<Vec<MachineUsage>, Failure> {
    let (lookup, topics, _) = reading(deps, None)?;
    let (machine, its_runs) = match machine {
        Some(address) => machine_named(deps, &lookup, address)?,
        None => (None, None),
    };
    let since = since.map(day_given).transpose()?.map(|day| day.to_string());
    let runs = match its_runs {
        Some(runs) if since.is_none() && agent.is_none() => runs,
        _ => deps.usage.read(&RunFilter {
            machine,
            since,
            agent,
        })?,
    };
    let mut counted: BTreeMap<DocumentId, Counted> = BTreeMap::new();
    for run in runs {
        if !run.command.is_empty() {
            (counted.entry(run.machine).or_default()).add(run.command, run.refusal, run.session);
        }
    }
    let mut machines: Vec<MachineUsage> = (counted.into_iter())
        .map(|(machine, Counted { commands, agents })| MachineUsage {
            machine: topics.machine(&machine),
            topic: topics.name(&machine).map(ToString::to_string),
            agents: agent_rows(agents),
            commands: command_rows(commands),
        })
        .collect();
    machines.sort_by(|a, b| {
        (&a.machine.label, &a.machine.document).cmp(&(&b.machine.label, &b.machine.document))
    });
    Ok(machines)
}

#[derive(Default)]
struct Counted {
    commands: BTreeMap<String, (usize, BTreeMap<String, usize>)>,
    agents: BTreeMap<Option<String>, (usize, BTreeSet<String>)>,
}

impl Counted {
    fn add(&mut self, command: String, refusal: Option<String>, session: Option<Session>) {
        let (runs, refusals) = self.commands.entry(command).or_default();
        *runs += 1;
        if let Some(refusal) = refusal {
            *refusals.entry(refusal).or_default() += 1;
        }
        let (agent, session) = match session {
            Some(session) => (Some(session.agent), Some(session.id)),
            None => (None, None),
        };
        let (runs, sessions) = self.agents.entry(agent).or_default();
        *runs += 1;
        sessions.extend(session);
    }
}

// The maps come in key order, which a stable sort by count alone keeps among equals.
fn command_rows(commands: BTreeMap<String, (usize, BTreeMap<String, usize>)>) -> Vec<CommandCount> {
    let mut commands: Vec<CommandCount> = (commands.into_iter())
        .map(|(command, (run_count, refusals))| {
            let mut refusals: Vec<Refusal> = (refusals.into_iter())
                .map(|(text, count)| Refusal { text, count })
                .collect();
            refusals.sort_by_key(|refusal| Reverse(refusal.count));
            CommandCount {
                command,
                run_count,
                refusals,
            }
        })
        .collect();
    commands.sort_by_key(|count| Reverse(count.run_count));
    commands
}

fn agent_rows(agents: BTreeMap<Option<String>, (usize, BTreeSet<String>)>) -> Vec<AgentUsage> {
    let mut agents: Vec<AgentUsage> = (agents.into_iter())
        .map(|(agent, (run_count, sessions))| AgentUsage {
            agent,
            run_count,
            session_count: sessions.len(),
        })
        .collect();
    agents.sort_by_key(|agent| (Reverse(agent.run_count), agent.agent.is_none()));
    agents
}

type Named = (Option<DocumentId>, Option<Vec<Invocation>>);

// A machine no topic answers for comes with every run of it, which is what showed it is one.
fn machine_named(deps: &ReadDeps, lookup: &Lookup, address: &str) -> Result<Named, Failure> {
    if let Ok(id) = DocumentId::parse(address)
        && lookup.find(address)? == Found::None
    {
        let its_runs = RunFilter {
            machine: Some(id.clone()),
            ..RunFilter::default()
        };
        let runs = deps.usage.read(&its_runs)?;
        if !runs.is_empty() {
            return Ok((Some(id), Some(runs)));
        }
    }
    Ok((Some(lookup.one_topic(address)?), None))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::app::amend::rename;
    use crate::app::testing::{
        ENDED, World, by_no_agent, counted, fact, refused, to, to_ended, topic, topic_fields,
        world_with_atlas,
    };
    use crate::domain::id::DocumentId;
    use crate::domain::ports::{StoreError, Usage, UsageRead};
    use crate::domain::testing::ran;
    use crate::domain::version::Stamp;

    fn logged(world: &World, phone: &DocumentId) {
        for (machine, command, day) in [
            (&world.machine(), "context", "2026-09-01"),
            (&world.machine(), "show", "2026-09-02"),
            (&world.machine(), "new entry", "2026-09-02"),
            (&world.machine(), "show", "2026-09-03"),
            (phone, "context", "2026-09-02"),
            (phone, "", "2026-09-02"),
        ] {
            world.usage.record(&ran(machine, command, day)).unwrap();
        }
    }

    #[test]
    fn each_command_is_counted_under_the_machine_that_ran_it_most_run_first() {
        let (world, _, _) = world_with_atlas();
        let phone = topic(&world, "phone", "");
        logged(&world, &phone);
        let all = usage(&world.deps().read(), None, None, None).unwrap();
        let desk_commands = counted(&[("show", 2), ("context", 1), ("new entry", 1)]);
        let desk = MachineUsage {
            machine: to(&world.machine(), "desk"),
            topic: Some("desk".to_owned()),
            agents: by_no_agent(&desk_commands),
            commands: desk_commands,
        };
        let phone_commands = counted(&[("context", 1)]);
        let phone = MachineUsage {
            machine: to(&phone, "phone"),
            topic: Some("phone".to_owned()),
            agents: by_no_agent(&phone_commands),
            commands: phone_commands,
        };
        assert_eq!(all, [desk, phone]);
    }

    #[test]
    fn the_counts_are_narrowed_to_a_machine_topic_and_to_a_day_and_after() {
        let (world, _, _) = world_with_atlas();
        let phone = topic(&world, "phone", "");
        logged(&world, &phone);
        let deps = world.deps();
        let of = |machine, since| usage(&deps.read(), machine, since, None);

        let only = of(Some("phone"), None).unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].machine, to(&phone, "phone"));
        world.amend(&phone, &topic_fields("phone", ENDED), "\n");
        let only = of(Some("phone"), None).unwrap();
        assert_eq!(only[0].machine, to_ended(&phone, "phone", "retired"));
        assert_eq!(of(Some("atlas"), None), Ok(Vec::new()));
        assert_eq!(refused(of(Some("torch"), None)), "torch: names no document");

        let recent = of(None, Some("2026-09-02")).unwrap();
        assert_eq!(
            recent[0].commands,
            counted(&[("show", 2), ("new entry", 1)])
        );
        assert_eq!(recent[1].commands, counted(&[("context", 1)]));
        let latest = of(Some("desk"), Some("2026-09-03")).unwrap();
        assert_eq!(latest[0].commands, counted(&[("show", 1)]));
        assert_eq!(of(None, Some("2099-01-01")), Ok(Vec::new()));
        assert_eq!(
            of(None, Some("soon")),
            Err(Failure::Usage(
                "`soon` is not a date (YYYY-MM-DD)".to_owned()
            ))
        );
    }

    #[test]
    fn runs_are_split_by_agent_with_sessions_counted_and_kept_for_one_agent() {
        let (world, _, _) = world_with_atlas();
        let in_session = |agent: &str, id: &str, command: &str| Invocation {
            session: Some(Session {
                agent: agent.to_owned(),
                id: id.to_owned(),
            }),
            ..ran(&world.machine(), command, "2026-09-02")
        };
        for run in [
            in_session("Lamp", "a", "show"),
            in_session("Lamp", "a", "save"),
            in_session("Lamp", "b", "show"),
            in_session("Torch", "c", "show"),
            ran(&world.machine(), "context", "2026-09-02"),
        ] {
            world.usage.record(&run).unwrap();
        }
        let deps = world.deps();
        let of = |machine, agent| usage(&deps.read(), machine, None, agent);
        let named = |name: &str| Some(Agent::Named(name.to_owned()));

        let all = of(None, None).unwrap();
        let by = |agent: Option<&str>, run_count, session_count| AgentUsage {
            agent: agent.map(str::to_owned),
            run_count,
            session_count,
        };
        assert_eq!(
            all[0].agents,
            [
                by(Some("Lamp"), 3, 2),
                by(Some("Torch"), 1, 1),
                by(None, 1, 0)
            ]
        );
        let lamp = of(None, named("lamp")).unwrap();
        assert_eq!(lamp[0].commands, counted(&[("show", 2), ("save", 1)]));
        assert_eq!(lamp[0].agents, [by(Some("Lamp"), 3, 2)]);
        let none = of(None, Some(Agent::None)).unwrap();
        assert_eq!(none[0].commands, counted(&[("context", 1)]));
        assert_eq!(of(None, named("candle")), Ok(Vec::new()));
        let narrowed = of(Some("desk"), named("Torch")).unwrap();
        assert_eq!(narrowed[0].commands, counted(&[("show", 1)]));
    }

    #[test]
    fn a_command_s_refusals_are_counted_by_their_text_most_often_first() {
        let (world, _, _) = world_with_atlas();
        let refused = |text: &str| Invocation {
            exit: 1,
            refusal: Some(text.to_owned()),
            ..ran(&world.machine(), "show", "2026-09-02")
        };
        for run in [
            refused("torch: names no document"),
            refused("lamp: names no document"),
            refused("torch: names no document"),
            ran(&world.machine(), "show", "2026-09-02"),
        ] {
            world.usage.record(&run).unwrap();
        }
        let all = usage(&world.deps().read(), None, None, None).unwrap();
        let show = &all[0].commands[0];
        let refusal = |text: &str, count| Refusal {
            text: text.to_owned(),
            count,
        };
        assert_eq!(show.run_count, 4);
        assert_eq!(
            show.refusals,
            [
                refusal("torch: names no document", 2),
                refusal("lamp: names no document", 1)
            ]
        );
        assert_eq!(show.refused_count(), 3);
        assert_eq!((all[0].run_count(), all[0].refused_count()), (4, 3));
    }

    #[test]
    fn a_day_is_the_one_of_the_stamp_as_written_whatever_day_it_was_elsewhere() {
        let (world, _, _) = world_with_atlas();
        for (command, written) in [
            ("early", "2026-09-02T00:30:00+02:00"),
            ("late", "2026-09-01T23:30:00-02:00"),
        ] {
            let run = Invocation {
                written: Stamp::parse(written).unwrap(),
                ..ran(&world.machine(), command, "2026-09-01")
            };
            world.usage.record(&run).unwrap();
        }
        let deps = world.deps();
        let since = |day| usage(&deps.read(), None, Some(day), None).unwrap();
        assert_eq!(
            since("2026-09-01")[0].commands,
            counted(&[("early", 1), ("late", 1)])
        );
        assert_eq!(since("2026-09-02")[0].commands, counted(&[("early", 1)]));
        assert_eq!(since("2026-09-03"), []);
    }

    #[test]
    fn a_machine_is_given_as_any_document_is_or_by_the_whole_id_of_one_the_log_holds() {
        let (world, _, atlas) = world_with_atlas();
        let unheld = DocumentId::from_bytes([0xde; 16]);
        for machine in [&world.machine(), &unheld] {
            world
                .usage
                .record(&ran(machine, "show", "2026-09-02"))
                .unwrap();
        }
        let deps = world.deps();
        let of = |machine: &str| usage(&deps.read(), Some(machine), None, None);
        let machines = |machine: &str| -> Vec<DocumentId> {
            let counts = of(machine).unwrap();
            counts
                .into_iter()
                .map(|counts| counts.machine.document)
                .collect()
        };

        assert_eq!(machines(world.machine().as_str()), [world.machine()]);
        assert_eq!(machines(world.machine().short()), [world.machine()]);
        assert_eq!(machines(unheld.as_str()), std::slice::from_ref(&unheld));
        assert_eq!(machines(atlas.as_str()), []);
        assert_eq!(refused(of(unheld.short())), "dededede: names no document");
        let other = DocumentId::from_bytes([0xab; 16]);
        assert_eq!(
            refused(of(other.as_str())),
            format!("{other}: names no document")
        );
        let relay = fact(&world, &atlas, "relay", "");
        assert_eq!(
            refused(of(relay.as_str())),
            format!("{relay}: is not a topic")
        );
    }

    #[test]
    fn a_machine_is_named_as_its_topic_is_now_and_by_its_whole_id_where_no_topic_answers() {
        let (world, _, _) = world_with_atlas();
        let unheld = DocumentId::from_bytes([0xde; 16]);
        world
            .usage
            .record(&ran(&world.machine(), "show", "2026-09-02"))
            .unwrap();
        world
            .usage
            .record(&ran(&unheld, "show", "2026-09-02"))
            .unwrap();
        rename(&world.deps(), "desk", "bench").unwrap();

        let all = usage(&world.deps().read(), None, None, None).unwrap();
        let machines: Vec<(&Reference, Option<&str>)> = (all.iter())
            .map(|counts| (&counts.machine, counts.topic.as_deref()))
            .collect();
        assert_eq!(
            machines,
            [
                (&to(&world.machine(), "bench"), Some("bench")),
                (&to(&unheld, unheld.as_str()), None)
            ]
        );
        let renamed = usage(&world.deps().read(), Some("desk"), None, None).unwrap();
        assert_eq!(renamed.len(), 1);
        assert_eq!(renamed[0].machine, to(&world.machine(), "bench"));
    }

    #[derive(Default)]
    struct Asked(RefCell<Vec<RunFilter>>);

    impl UsageRead for Asked {
        fn read(&self, filter: &RunFilter) -> Result<Vec<Invocation>, StoreError> {
            self.0.borrow_mut().push(filter.clone());
            Ok(Vec::new())
        }
    }

    #[test]
    fn the_log_is_asked_for_the_runs_of_the_machine_and_from_the_day_given() {
        let (world, _, _) = world_with_atlas();
        let deps = world.deps();
        let asked = |machine, since| {
            let log = Asked::default();
            let asking = ReadDeps {
                usage: &log,
                ..deps.read()
            };
            usage(&asking, machine, since, None).unwrap();
            log.0.into_inner()
        };
        assert_eq!(asked(None, None), [RunFilter::default()]);
        let narrowed = RunFilter {
            machine: Some(world.machine()),
            since: Some("2026-09-03".to_owned()),
            agent: None,
        };
        assert_eq!(asked(Some("desk"), Some("2026-09-03")), [narrowed]);
    }
}
