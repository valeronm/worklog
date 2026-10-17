use std::collections::BTreeMap;

use serde::Serialize;

use crate::app::lookup::Lookup;
use crate::app::rows::{Reference, reading};
use crate::app::{Failure, ReadDeps};
use crate::domain::id::DocumentId;
use crate::domain::schema::Date;
use crate::domain::schema::address::Found;
use crate::domain::usage::{Invocation, Kept};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommandCount {
    pub command: String,
    pub run_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MachineUsage {
    /// Labeled by its whole id where no topic answers for it.
    pub machine: Reference,
    /// The name of the topic that answers for the machine.
    pub topic: Option<String>,
    pub commands: Vec<CommandCount>,
}

/// How often each command was run, by the machine that ran it: the machines by label, and a
/// machine's commands most run first, then by name.
///
/// `machine` names a topic, by its name or its id as any document is named, and keeps the
/// runs of that machine; the whole id of a machine no topic answers for is taken where the
/// log holds a run of it, and any other address that is not one topic is refused. `since` is
/// `YYYY-MM-DD` and keeps the runs written on that day and after, a run's day being the date
/// its stamp is written with, at the offset of the host that ran it; anything else is a usage
/// failure. A command line that named no command is not counted.
pub fn usage(
    deps: &ReadDeps,
    machine: Option<&str>,
    since: Option<&str>,
) -> Result<Vec<MachineUsage>, Failure> {
    let (lookup, topics, _) = reading(deps, None)?;
    let (machine, its_runs) = match machine {
        Some(address) => machine_named(deps, &lookup, address)?,
        None => (None, None),
    };
    let since = since
        .map(|day| Date::parse(day).map_err(|error| Failure::Usage(error.to_string())))
        .transpose()?
        .map(|day| day.to_string());
    let runs = match its_runs {
        Some(runs) if since.is_none() => runs,
        _ => deps.usage.read(&Kept { machine, since })?,
    };
    let mut counted: BTreeMap<DocumentId, BTreeMap<String, usize>> = BTreeMap::new();
    for run in runs {
        if !run.command.is_empty() {
            *(counted.entry(run.machine).or_default())
                .entry(run.command)
                .or_default() += 1;
        }
    }
    let mut machines: Vec<MachineUsage> = (counted.into_iter())
        .map(|(machine, commands)| {
            let mut commands: Vec<CommandCount> = (commands.into_iter())
                .map(|(command, run_count)| CommandCount { command, run_count })
                .collect();
            commands.sort_by(|a, b| (b.run_count, &a.command).cmp(&(a.run_count, &b.command)));
            MachineUsage {
                machine: topics.machine(&machine),
                topic: topics.name(&machine).map(ToString::to_string),
                commands,
            }
        })
        .collect();
    machines.sort_by(|a, b| {
        (&a.machine.label, &a.machine.document).cmp(&(&b.machine.label, &b.machine.document))
    });
    Ok(machines)
}

type Named = (Option<DocumentId>, Option<Vec<Invocation>>);

// A machine no topic answers for comes with every run of it, which is what showed it is one.
fn machine_named(deps: &ReadDeps, lookup: &Lookup, address: &str) -> Result<Named, Failure> {
    if let Ok(id) = DocumentId::parse(address)
        && lookup.find(address)? == Found::None
    {
        let its_runs = Kept {
            machine: Some(id.clone()),
            since: None,
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
        ENDED, World, counted, fact, refused, to, to_ended, topic, topic_fields, world_with_atlas,
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
        let all = usage(&world.deps().read(), None, None).unwrap();
        let desk = MachineUsage {
            machine: to(&world.machine(), "desk"),
            topic: Some("desk".to_owned()),
            commands: counted(&[("show", 2), ("context", 1), ("new entry", 1)]),
        };
        let phone = MachineUsage {
            machine: to(&phone, "phone"),
            topic: Some("phone".to_owned()),
            commands: counted(&[("context", 1)]),
        };
        assert_eq!(all, [desk, phone]);
    }

    #[test]
    fn the_counts_are_narrowed_to_a_machine_topic_and_to_a_day_and_after() {
        let (world, _, _) = world_with_atlas();
        let phone = topic(&world, "phone", "");
        logged(&world, &phone);
        let deps = world.deps();
        let of = |machine, since| usage(&deps.read(), machine, since);

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
        let since = |day| usage(&deps.read(), None, Some(day)).unwrap();
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
        let of = |machine: &str| usage(&deps.read(), Some(machine), None);
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

        let all = usage(&world.deps().read(), None, None).unwrap();
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
        let renamed = usage(&world.deps().read(), Some("desk"), None).unwrap();
        assert_eq!(renamed.len(), 1);
        assert_eq!(renamed[0].machine, to(&world.machine(), "bench"));
    }

    #[derive(Default)]
    struct Asked(RefCell<Vec<Kept>>);

    impl UsageRead for Asked {
        fn read(&self, kept: &Kept) -> Result<Vec<Invocation>, StoreError> {
            self.0.borrow_mut().push(kept.clone());
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
            usage(&asking, machine, since).unwrap();
            log.0.into_inner()
        };
        assert_eq!(asked(None, None), [Kept::default()]);
        let narrowed = Kept {
            machine: Some(world.machine()),
            since: Some("2026-09-03".to_owned()),
        };
        assert_eq!(asked(Some("desk"), Some("2026-09-03")), [narrowed]);
    }
}
