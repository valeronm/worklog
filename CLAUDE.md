# Working on worklog

Read README.md first for what the tool is, `docs/store.md` for the file
format and `docs/documents.md` for the kinds. This file covers the
constraints that are not visible from the code or those docs.

## Layers

`tests/layers.rs` checks source spellings for the boundaries below; it does
not prove the absence of every forbidden dependency. The compiler holds
capability and visibility restrictions. Neither checks that `app` prints
nothing.

`domain` holds the model and its rules and reaches nothing outside memory:
no file, clock, environment, process or random source. The boundary is
what makes every rule testable with the in-memory ports in
`domain::testing`. Inside it, `schema` alone knows the kinds: the version,
the document and the directory store do not refer to it, which is what
lets a store keep documents of a kind this binary does not know.

`app` is one use case per command over the ports. It prints nothing, uses
no renderer and reaches the host only through a port, so a use case that
wants something new from outside adds it to a port, not to a parameter.
The layer test rejects the write dependency type in read modules; the
compiler rejects mutations through their read capabilities. A write builds
its read view from its handles, since retaining both views would let them
disagree. The raw version writer is private to a child module whose write
method accepts only an admitted version; validation belongs to its parent.
One module appends to the usage log. A module of `app` decides no ending
unless the layer test lists it as a write or as shared.

`wiring` builds the adapters and keeps them to itself: `fs` implements the
ports on a directory tree and `net` the one port that reaches the network,
the releases `upgrade` reads. `WORKLOG_RELEASES=<dir>` swaps in a
directory, and the layer test holds every helper of `tests/cli.rs` that
runs the binary to set it, so no test touches the network.

`cli` parses arguments and renders, and `main.rs` only calls it. It reads
the process only where the process starts and where a shell asks for
completions, and it alone gives a result its JSON form, under the
convention at the top of `cli/json.rs`. A result type with no JSON
snapshot there does not compile in a test build.

`web` is a second renderer over the read use cases: it is handed its
ports, calls no write use case and decides no ending, so a page that needs
data no read returns gets a read use case, not logic in a handler. One
request is one reading of the store, since a page composes several reads.
No template turns escaping off, and `Markup` in `web/markdown.rs` is the
one type printed unescaped. Neither `cli` nor `web` refers to the store
port, in its tests either. `render` holds what the two show the same way
and can run no use case.

## The store's invariants

`docs/store.md` is the format, with what a reader refuses, and
`docs/release.md` has what a change to it asks of the machines sharing a
store. A version is composed only if its bytes read back to the same
parts, which is what holds the writer and the reader to each other.

The rules of the store are applied by the one module that stores a
version.

The check that keeps a usage line out of a directory holding nothing of
the machine's topic runs on every logged run and reads that one document,
not the tree.

A walk takes a document's headers from the host's cache on its
directory's modification time alone, so a file rewritten in place is read
as it was. `check` is the one command that reads every file whatever the
cache holds, which is what keeps that hole a finding and not a silent
loss. The cache holds no body, so a body is read from its version's file
as it stands. A document's headers come from the tree once it is walked
and from the document's own directory before, which is what keeps the
usage-line check off the tree.

## Documents

`docs/documents.md` has the kinds and their fields. The references of a
kind in `schema::kind` are the one list of the fields that refer to
another document, `ended_by` apart. A save, `check`, a draft's addresses
and the ending of a topic all read it, so a field carrying an id that is
not on it is seen by none of them.

## Retrieval

`app::context` is the whole rule, stated in `docs/documents.md` under
What a session reaches. Since every claim covering the directory loads, a
topic that should not reach every project must not be claimed for a wide
directory.

`context` is printed at session start, and an agent keeps only the start
of a hook's output past a size limit, so the text renderer holds it under
that limit, what is due first. The tests beside the renderer pin the
limit.

## Output contract

stdout carries data only; refusals and notes go to stderr. Exit 1 is a
refusal or a store problem, 2 a usage error. A listing that reports
something a person may act on, a fork among them, still exits 0: the
listing is the answer. `check` is the command whose exit code means
something: 1 on a problem, a rule the store must keep, while a notice is a
judgment call, printed with a `notice:` prefix and counted in the
summary, and does not move the exit code. `upgrade --check` exits 0
whether or not a newer release exists, and 1 only when the check itself
failed.

`refresh` and `agents refresh` print the paths they wrote on stdout and
everything else on stderr. The installer and the binary that ran
`upgrade` both read that stdout as the list of files written. A release
before `refresh` existed runs `agents refresh` on the binary it installed,
so that command keeps its name and its stdout.

## The skill

`skill/SKILL.md` is compiled into the binary so the skill a session reads
is the one written for the commands it has, and the tests beside
`cli/skill.rs` hold the two to each other. The skill is public and written
for a stranger: it holds how to use the store, including the follow-up
triage put to the user, and nothing about any one person's other tools. It
holds what the tool gives and how to use it, not how the tool works: a rule
restated there is a copy those tests cannot hold to the code.

The session hook is merged into the agent's hooks file, because that file
is the user's and holds other hooks. What the tool owns in it is any
`SessionStart` group holding a hook whose command runs a file named
`worklog` with `context` as its next word, whoever wrote it. An install
adds none where one is, a refresh leaves one such group with one such hook
running this binary, and an uninstall removes every such group; a change
to the recognition must keep all three. A binary under another file name is
refused a hook, since it would not find it again. The file is kept as
data, so key order survives a write and formatting does not.

An agent whose sandbox keeps the store read-only gets a rule that runs
the tool outside it, a file of the tool's own and so written whole. Codex
calls its rules experimental and nothing checks the form or the directory
against it: a change on its side drops the usage lines and the writes of
its sessions without failing a test.

What the tool owns in Codex's `config.toml` is the one writable root equal
to the host's drafts directory, along with an array or table its removal
leaves empty. The rest of that file is the user's and keeps its comments
and layout. Nothing checks that Codex still reads that key, so a rename on
its side leaves drafts unwritable to its edit tool without failing a test.

`init` and `join` touch neither the skill nor the hooks file unless given
`--agents`, or, for `init` on a terminal, told yes to the question it
asks. A refresh writes only where the thing already is, fish's
completions directory included, which is what lets the installer and
`upgrade` run it on every host. The rule is the exception: a refresh
writes one that is missing for an agent holding the skill or the hook,
since a host set up before the rule existed is upgraded and not installed
again.

A usage line gets its session from the environment variable an agent sets
for a command it runs, and for a session hook from the hook's input, which
the hook's command asks `context` to read by naming the agent. Nothing
checks the names the agent table holds against the agent, so a rename on
the agent's side leaves the session out without failing a test. The first
agent in the table whose variable is set is the one logged when one agent
runs inside another, and a hook whose input holds a session is logged
under its own agent whatever it runs inside.

## What stays out of the repo

No store content reaches the repo, and nothing in it refers to whoever
keeps one. Fixtures, examples and test expectations are invented: no text,
name, id, path, machine name or trigger lifted from a real store, no real
project, device or host names, and no "the author" or "my machine", since a
sentence like that is itself the disclosure. The invented vocabulary in the
tests, `lantern`, `atlas`, `phone`, `desk`, is the one to extend.

## Build and check

`docs/release.md` has the checks CI runs. Pedantic clippy is on and a
suppression carries a `reason`. The toolchain is pinned in `mise.toml` and
CI reads the pin from there. Releases are cut by tag.

A benchmark or a loop never runs the installed `worklog`, since the binary
`PATH` finds logs every run into the store's usage: time a build run by its
path, or set `WORKLOG_HOME` to a directory whose config carries the path of
a copy of the store.
