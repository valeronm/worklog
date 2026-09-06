# Working on worklog

Read README.md first for what the tool is, `docs/store.md` for the file
format and `docs/documents.md` for the kinds. This file covers the
constraints that are not visible from the code or those docs.

## Layers

`domain` holds the model and its rules and reaches nothing outside memory:
no file, clock, environment or process. `tests/layers.rs` reads the sources
and fails on an I/O import, because the boundary is what makes every rule
testable with the in-memory ports in `domain::testing`. `app` is one use
case per command over the ports and never prints. `fs` implements the ports
on a directory tree, and `net` the one port that reaches the network, the
releases `upgrade` reads; `WORKLOG_RELEASES=<dir>` swaps in the `fs`
implementation so no test touches it. `cli` parses arguments and renders.
`web` is a second renderer over the reads, one request being one use
case, and the layer test keeps it from reaching `fs` or `net`: a page
that needs data no read returns
gets a read use case, which `--json` then has too, never logic in a
handler. A use case that wants something new from outside adds it to a
port, not to a parameter.

## The store's invariants

`docs/store.md` is the format, and `fs::store` is where a file outside it
is refused. Extend the grammar only if the writer and the reader change
together, and only by adding, since that is what an older binary
tolerates; `docs/release.md` has the step for anything else.

`init` asks on a terminal when given no name; nothing reads a store
before it exists.

## Documents

`docs/documents.md` has the kinds, their slugs and their fields. An entry
sits under a month directory so no directory grows past a few dozen
children, and a topic needs a letter so it cannot be read as one. An
idea is flagged by `idea` rather than `kind` because `kind` names the
document.

## Retrieval

`domain::graph::resolve` is the whole rule, stated in `docs/documents.md`
under Topic. Since every matching claim loads, a topic that should not
reach every project must not be claimed for a wide directory.

`context` is printed at session start and a session is shown only its
first couple of kilobytes, so the text renderer keeps it to names and
counts, must-act-on items first. `tests/cli.rs` pins the shape.

## Output contract

stdout carries data only; refusals and notes go to stderr. Exit 1 is a
refusal or a store problem, 2 a usage error. A listing that reports a
problem a person may act on, a fork, a claim on a directory this host
lacks, still exits 0: the listing is the answer, and `check` is the
command whose exit code means something. `check` exits 1 on a problem, a
rule the store must keep; a notice is a judgement call, printed with a
`notice:` prefix and counted in the summary, and never moves the exit
code.

## The skill

`skill/SKILL.md` is compiled into the binary so the skill a session reads
is the one written for the commands it has, and `tests/skill.rs` holds the
two to each other. The session hook is merged into the agent's hooks file
rather than written over it, because that file is the user's and holds
other hooks. What the tool owns in it is any `SessionStart` group whose
command runs `worklog context`, whoever wrote it: that is the unit a
refresh replaces and an uninstall removes, and the rule a change to the
recognition must keep. The file is kept as data, so key order survives a
write and formatting does not. A scripted `init` touches neither the skill
nor the hooks file unless told to, since a script writing into an agent's
home unasked is the kind of surprise the store exists to avoid, and a
refresh writes only where the thing already is, a fish completions
directory included, which is what lets the installer and `upgrade` run it
on every host without a network. The skill is public and written
for a stranger: it holds how to use the store, including the follow-up
triage put to the user and the offer to log after work, and nothing about
any one person's other tools.

## What stays out of the repo

No store content reaches the repo, and nothing in it refers to whoever
keeps one. Fixtures, examples and test expectations are invented: no text,
slug, path, machine name or recheck lifted from a real store, no real
project, device or host names, and no "the author" or "my machine", since a
sentence like that is itself the disclosure. The invented vocabulary in the
tests, `lantern`, `atlas`, `phone`, `desk`, is the one to extend.

## Build and check

`cargo fmt --all --check`, `cargo clippy --all-targets --all-features --
-D warnings` and `cargo test` are what CI runs; pedantic clippy is on for
the crate and a suppression lives at its site with a `reason`. The
toolchain is pinned in `mise.toml` and CI reads the pin from there.
Releases are cut by tag; `docs/release.md` has the steps.
