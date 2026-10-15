use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap_complete::{ArgValueCandidates, CompleteEnv, CompletionCandidate};

use super::args::{Cli, Shell};
use super::render::Output;
use super::{Environment, NAME, hook};
use crate::app::offer::{self, Offered, Wanted};
use crate::app::{Deps, Failure, draft};
use crate::domain::schema::{KindOf, Reason};
use crate::wiring;

type Listed = Result<Vec<CompletionCandidate>, Failure>;

/// Answers a shell's request for candidates or for its registration script on stdout, with
/// the exit code to end on; `None` when the process was run for a command.
pub(super) fn answered(args: &[OsString], cwd: Option<&Path>, err: &mut dyn Write) -> Option<i32> {
    match CompleteEnv::with_factory(Cli::asked_by_a_shell).try_complete(args, cwd) {
        Ok(true) => Some(0),
        Ok(false) => None,
        Err(error) => {
            let _ = writeln!(err, "{NAME}: {error}");
            Some(1)
        }
    }
}

/// The line a shell's startup file holds to take its completions from `binary`, spelled with
/// a leading `$HOME` for a binary under one of `homes`; refuses a binary whose file name is
/// not `NAME`, the command the completions are registered for, and a path the line cannot
/// quote.
pub(super) fn registration(
    shell: Shell,
    binary: &Path,
    homes: &[&Path],
) -> Result<String, Failure> {
    if !runs_as_the_command(binary) {
        let or_else = "its completions are registered for another command";
        return Err(hook::misnamed(binary, or_else));
    }
    let binary = hook::quoted(binary, homes)?;
    Ok(match shell {
        Shell::Fish => format!("COMPLETE=fish {binary} | source\n"),
        Shell::Bash => format!("source <(COMPLETE=bash {binary})\n"),
        Shell::Zsh => format!("source <(COMPLETE=zsh {binary})\n"),
    })
}

pub(super) fn runs_as_the_command(binary: &Path) -> bool {
    binary.file_name() == Some(OsStr::new(NAME))
}

pub(super) fn registered(shell: Shell, environment: &Environment) -> Result<Output, Failure> {
    let homes = wiring::homes(environment.variable)?;
    let homes: Vec<&Path> = homes.iter().map(PathBuf::as_path).collect();
    Ok(Output {
        text: registration(shell, environment.binary()?, &homes)?,
        notes: Vec::new(),
        exit: 0,
        hits: None,
    })
}

fn described(value: &str, help: &str) -> CompletionCandidate {
    CompletionCandidate::new(value).help(Some(help.to_owned().into()))
}

const TOPIC: KindOf = KindOf::Topic;
const FACT: KindOf = KindOf::Fact;
const ENTRY: KindOf = KindOf::Entry;
const FOLLOWUP: KindOf = KindOf::Followup;

type Source = (&'static [KindOf], Wanted);

const TOPICS: Source = (&[TOPIC], Wanted::Live);
const FACTS: Source = (&[FACT], Wanted::Live);
const ENTRIES: Source = (&[ENTRY], Wanted::Live);
const FOLLOWUPS: Source = (&[FOLLOWUP], Wanted::Live);
const DOCUMENTS: Source = (&[TOPIC, FACT, ENTRY, FOLLOWUP], Wanted::Live);
const RENAMABLE: Source = (&[TOPIC, FACT, ENTRY], Wanted::Live);
const MOVABLE: Source = (&[FACT, ENTRY, FOLLOWUP], Wanted::Live);
const TOPICS_AND_ENTRIES: Source = (&[TOPIC, ENTRY], Wanted::Live);
const FORKED: Source = (&KindOf::ALL, Wanted::Forked);
const ENDED: Source = (&[TOPIC, FACT, ENTRY, FOLLOWUP], Wanted::Ended);

fn addresses(deps: &Deps, (kinds, wanted): Source) -> Listed {
    let candidate = |offered: &Offered| described(&offered.address, &offered.summary);
    let offered = offer::offered(deps, kinds, wanted)?;
    Ok(offered.iter().map(candidate).collect())
}

fn draft_names(deps: &Deps) -> Listed {
    let candidate = |row: &draft::DraftRow| described(&row.label, &row.kind);
    Ok(draft::drafts(deps)?.iter().map(candidate).collect())
}

fn reason_words() -> Vec<CompletionCandidate> {
    let mut ended: Vec<(&Reason, Vec<&str>)> = Vec::new();
    for (word, reasons) in KindOf::endings_by_word() {
        for reason in reasons {
            match ended.iter_mut().find(|(known, _)| *known == reason) {
                Some((_, words)) => words.push(word),
                None => ended.push((reason, vec![word])),
            }
        }
    }
    let candidate =
        |(reason, words): &(&Reason, Vec<&str>)| described(reason.word(), &words.join(", "));
    ended.iter().map(candidate).collect()
}

fn trigger_words() -> Vec<CompletionCandidate> {
    vec![described("touching", "then a topic")]
}

// A shell that asks has nowhere to show a refusal.
fn offered(
    variable: &dyn Fn(&str) -> Option<OsString>,
    list: &dyn Fn(&Deps) -> Listed,
) -> Vec<CompletionCandidate> {
    let Ok(Some(adapters)) = wiring::wire(variable, None) else {
        return Vec::new();
    };
    list(&adapters.deps()).unwrap_or_default()
}

// clap asks a source with no arguments.
fn from_store(list: impl Fn(&Deps) -> Listed + Send + Sync + 'static) -> ArgValueCandidates {
    ArgValueCandidates::new(move || offered(&|name| std::env::var_os(name), &list))
}

fn stored(source: Source) -> ArgValueCandidates {
    from_store(move |deps| addresses(deps, source))
}

pub(super) fn topics() -> ArgValueCandidates {
    stored(TOPICS)
}

pub(super) fn facts() -> ArgValueCandidates {
    stored(FACTS)
}

pub(super) fn entries() -> ArgValueCandidates {
    stored(ENTRIES)
}

pub(super) fn followups() -> ArgValueCandidates {
    stored(FOLLOWUPS)
}

pub(super) fn documents() -> ArgValueCandidates {
    stored(DOCUMENTS)
}

pub(super) fn renamable() -> ArgValueCandidates {
    stored(RENAMABLE)
}

pub(super) fn movable() -> ArgValueCandidates {
    stored(MOVABLE)
}

pub(super) fn topics_and_entries() -> ArgValueCandidates {
    stored(TOPICS_AND_ENTRIES)
}

pub(super) fn forked() -> ArgValueCandidates {
    stored(FORKED)
}

pub(super) fn ended() -> ArgValueCandidates {
    stored(ENDED)
}

pub(super) fn drafts() -> ArgValueCandidates {
    from_store(draft_names)
}

pub(super) fn reasons() -> ArgValueCandidates {
    ArgValueCandidates::new(reason_words)
}

pub(super) fn triggers() -> ArgValueCandidates {
    ArgValueCandidates::new(trigger_words)
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory as _, ValueHint};

    use super::*;
    use crate::app::draft;
    use crate::app::testing::{
        DONE, ENDED, ENDED_FACT, REMOVED, World, claim_fields, entry, fact, followup, topic, world,
        world_with_atlas,
    };
    use crate::cli::args::Cli;
    use crate::cli::tests::Process;
    use crate::domain::id::DocumentId;

    fn pair(candidate: &CompletionCandidate) -> (String, String) {
        let help = candidate.get_help().map(ToString::to_string);
        (
            candidate.get_value().to_string_lossy().into_owned(),
            help.unwrap_or_default(),
        )
    }

    fn pairs(listed: Listed) -> Vec<(String, String)> {
        listed.expect("candidates").iter().map(pair).collect()
    }

    fn summarized(value: &str) -> (String, String) {
        (value.to_owned(), "s".to_owned())
    }

    #[test]
    fn the_line_a_shell_registers_runs_the_binary_under_its_own_name() {
        let homes = [Path::new("/home/desk")];
        let line = |shell, binary: &str| registration(shell, Path::new(binary), &homes);
        let made = |text: String| Ok(text);
        assert_eq!(
            line(Shell::Fish, &format!("/opt/bin/{NAME}")),
            made(format!("COMPLETE=fish \"/opt/bin/{NAME}\" | source\n"))
        );
        assert_eq!(
            line(Shell::Fish, &format!("/home/desk/.local/bin/{NAME}")),
            made(format!(
                "COMPLETE=fish \"$HOME/.local/bin/{NAME}\" | source\n"
            ))
        );
        assert_eq!(
            line(Shell::Bash, &format!("/opt/my tools/{NAME}")),
            made(format!(
                "source <(COMPLETE=bash \"/opt/my tools/{NAME}\")\n"
            ))
        );
        assert_eq!(
            line(Shell::Zsh, &format!("/home/desk/bin/{NAME}")),
            made(format!("source <(COMPLETE=zsh \"$HOME/bin/{NAME}\")\n"))
        );
        let refused = |binary: &str| match line(Shell::Fish, binary) {
            Err(Failure::Refused(why)) => why,
            other => panic!("{binary} must be refused, got {other:?}"),
        };
        let quoted = format!("/opt/a\"b/{NAME}");
        assert!(refused(&quoted).starts_with(&format!("{quoted}: holds '\"'")));
        for binary in [
            format!("/opt/bin/{NAME}-0.3.0"),
            format!("/opt/{NAME}/lamp"),
        ] {
            assert_eq!(
                refused(&binary),
                format!(
                    "{binary}: the binary's file name must be {NAME}, or its completions are \
                     registered for another command"
                )
            );
        }
    }

    fn ended_entry(world: &World, date: &str, name: &str) -> DocumentId {
        let machine = world.machine();
        let fields = format!(
            "name = \"{name}\"\ndate = {date}\nmachine = \"{machine}\"\nsummary = \"s\"\n{REMOVED}"
        );
        world.put("entry", &fields, "\n")
    }

    #[test]
    fn a_document_is_offered_by_what_addresses_it_with_its_summary_unless_it_is_ended() {
        let (world, lantern, _) = world_with_atlas();
        fact(&world, &lantern, "relay", "");
        fact(&world, &lantern, "dimmer", "idea = true\n");
        fact(&world, &lantern, "fuse", ENDED_FACT);
        fact(
            &world,
            &lantern,
            "shade",
            &format!("idea = true\n{ENDED_FACT}"),
        );
        topic(&world, "phone", ENDED);
        entry(&world, "2026-10-08", "wiring", &[&lantern]);
        ended_entry(&world, "2026-10-07", "casing");
        let open = followup(&world, &[&lantern], "Check the driver", "");
        followup(&world, &[&lantern], "Order a fuse", DONE);
        let machine = world.machine();
        world.put("claim", &claim_fields(&machine, &lantern, ""), "\n");
        let deps = world.deps();

        let topics = ["atlas", "desk", "lantern"].map(summarized);
        let facts = ["lantern/relay", "lantern/dimmer"].map(summarized);
        let entries = [summarized("2026-10-08-wiring")];
        let followups = [(open.short().to_owned(), "Check the driver".to_owned())];
        assert_eq!(pairs(addresses(&deps, TOPICS)), topics);
        assert_eq!(pairs(addresses(&deps, FACTS)), facts);
        assert_eq!(pairs(addresses(&deps, ENTRIES)), entries);
        assert_eq!(pairs(addresses(&deps, FOLLOWUPS)), followups);
        let of = |lists: &[&[(String, String)]]| lists.concat();
        assert_eq!(
            pairs(addresses(&deps, DOCUMENTS)),
            of(&[&topics, &facts, &entries, &followups])
        );
        assert_eq!(
            pairs(addresses(&deps, RENAMABLE)),
            of(&[&topics, &facts, &entries])
        );
        assert_eq!(
            pairs(addresses(&deps, MOVABLE)),
            of(&[&facts, &entries, &followups])
        );
        assert_eq!(
            pairs(addresses(&deps, TOPICS_AND_ENTRIES)),
            of(&[&topics, &entries])
        );
    }

    #[test]
    fn the_words_an_ending_and_a_trigger_take_are_offered_without_a_store() {
        let words = |listed: Vec<CompletionCandidate>| -> Vec<(String, String)> {
            listed.iter().map(pair).collect()
        };
        let worded = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            let owned = |(word, help): &(&str, &str)| ((*word).to_owned(), (*help).to_owned());
            pairs.iter().map(owned).collect()
        };
        assert_eq!(
            words(reason_words()),
            worded(&[
                ("retired", "topic"),
                ("merged", "topic, entry"),
                ("false", "fact"),
                ("moved", "fact"),
                ("superseded", "fact"),
                ("built", "idea"),
                ("abandoned", "idea"),
                ("removed", "entry, claim"),
                ("done", "followup"),
                ("dropped", "followup"),
            ])
        );
        assert_eq!(
            words(trigger_words()),
            worded(&[("touching", "then a topic")])
        );
    }

    #[test]
    fn a_draft_is_offered_by_its_label_with_its_kind() {
        let (world, _) = world();
        let deps = world.deps();
        assert_eq!(pairs(draft_names(&deps)), []);
        draft::checkout(&deps, "lantern").unwrap();
        let relay = draft::New::Fact {
            address: "lantern/relay",
            idea: false,
        };
        draft::new(&deps, &relay).unwrap();
        let mut offered = pairs(draft_names(&deps));
        offered.sort();
        assert_eq!(
            offered,
            [
                ("lantern".to_owned(), "topic".to_owned()),
                ("lantern/relay".to_owned(), "fact".to_owned())
            ]
        );
        assert_eq!(pairs(addresses(&deps, TOPICS)).len(), 2);
    }

    #[test]
    fn a_source_offers_what_this_host_s_store_holds_and_nothing_where_none_opens() {
        let kept = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(kept.path()).unwrap();
        let process = Process::under(&root);
        let values = |list: &dyn Fn(&Deps) -> Listed| -> Vec<String> {
            let offered = offered(&|name| process.variable(name), list);
            offered.iter().map(|candidate| pair(candidate).0).collect()
        };
        let topic_names = |deps: &Deps| addresses(deps, TOPICS);
        let document_names = |deps: &Deps| addresses(deps, DOCUMENTS);
        assert_eq!(values(&topic_names), [""; 0]);

        let init = ["init", "desk", "--store", "kept", "--summary", "s"];
        assert_eq!(process.run(&init).code, 0);
        assert_eq!(values(&topic_names), ["desk"]);
        assert_eq!(values(&document_names), ["desk"]);
        assert_eq!(
            values(&|_| Err(Failure::Refused("gone".to_owned()))),
            [""; 0]
        );

        std::fs::write(root.join("next/config.toml"), "store = 4\n").unwrap();
        assert_eq!(values(&topic_names), [""; 0]);
    }

    fn arguments(command: &clap::Command, path: &str, found: &mut Vec<(String, clap::Arg)>) {
        for argument in command.get_arguments() {
            let named = format!("{path} {}", argument.get_id());
            found.push((named.trim_start().to_owned(), argument.clone()));
        }
        for sub in command.get_subcommands() {
            let deeper = format!("{path} {}", sub.get_name());
            arguments(sub, deeper.trim_start(), found);
        }
    }

    fn every_argument() -> Vec<(String, clap::Arg)> {
        let mut found = Vec::new();
        arguments(&Cli::command(), "", &mut found);
        assert!(found.len() > 60, "{}", found.len());
        found
    }

    #[test]
    fn these_arguments_have_a_source_of_candidates_and_no_other_has() {
        let sourced: Vec<String> = (every_argument().into_iter())
            .filter(|(_, argument)| argument.get::<ArgValueCandidates>().is_some())
            .map(|(named, _)| named)
            .collect();
        assert_eq!(
            sourced,
            [
                "new followup topics",
                "new followup touching",
                "new followup entry",
                "new followup about",
                "checkout target",
                "save target",
                "discard target",
                "resolve target",
                "rename target",
                "move from",
                "move to",
                "move targets",
                "verify fact",
                "end target",
                "end reason",
                "end by",
                "reopen target",
                "done followup",
                "drop followup",
                "trigger followup",
                "trigger when",
                "claim topic",
                "unclaim topic",
                "show target",
                "history target",
                "diff target",
                "log machine",
                "search topic",
                "usage machine",
                "topics topic",
                "facts topic",
                "ideas topic",
                "entries topic",
                "followups about",
                "where topic",
                "where machine",
            ],
            "each is a row of `SOURCED` in tests/cli.rs, which holds the list it is given"
        );
    }

    #[test]
    fn a_directory_is_offered_where_one_is_taken_and_a_file_nowhere() {
        let mut directories = Vec::new();
        for (named, argument) in every_argument() {
            match argument.get_value_hint() {
                ValueHint::DirPath => directories.push(named),
                hint => assert_eq!(hint, ValueHint::Unknown, "{named}"),
            }
        }
        assert_eq!(
            directories,
            [
                "init store",
                "claim dir",
                "unclaim dir",
                "context dir",
                "due dir"
            ]
        );
    }
}
