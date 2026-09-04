# Working on worklog

Read README.md first for what the tool is. This file covers the constraints
that are not visible from the code.

## Layers

`domain` holds the model and its rules and reaches nothing outside memory:
no file, clock, environment or process. `tests/layers.rs` reads the sources
and fails on an I/O import, because the boundary is what makes every rule
testable with the in-memory ports in `domain::testing`. `app` is one use
case per command over the ports and never prints. `fs` implements the ports
on a directory tree. `cli` parses arguments and renders. A use case that
wants something new from outside adds it to a port, not to a parameter.

## The store's invariants

- A version file is named by the BLAKE3 hash of its bytes, and `fs::store`
  refuses a file whose bytes are not what the writer would emit for their
  content, so the frontmatter grammar in `domain::frontmatter` is the only
  shape that exists. It is not YAML on purpose: a second writer with its
  own quoting or ordering would produce files that hash differently for the
  same content. Extend the grammar only if the writer and the reader change
  together.
- A file is written under a dotted name and renamed into place, so a file
  sync never ships a partial version.
- Nothing is edited in place. A command that needs to change a document
  writes a new version naming the old one as parent. The one file that is
  edited is a draft, and drafts live outside the store. A draft is the
  document's own text and nothing else; the versions it came from sit in
  a file beside it, so an editor cannot touch what `save` relies on.
- Forks are reported, never merged. `save` refuses a draft whose parent is
  no longer current, so a fork can arise only between machines that synced
  afterwards, and `resolve` hands both heads to a person.
- Slugs are never reused: a tombstone stays a head forever.

The config file, `~/.config/worklog/config`, uses the same grammar without
fences through `frontmatter::parse_fields`, so the tree has one syntax.
`init` writes it once per host, asking on a terminal when given no name;
nothing reads a store before it exists.

## Documents

Slug shapes decide the kind (`domain::slug`), so a bare slug names one
document across the store. A topic needs a letter so it cannot be confused
with an entry's month directory, `2026-09`, which holds the month's entries
so no directory grows past a few dozen children. An idea is a fact with `idea: true`, not a
kind of its own, so it becomes a fact by a field change and keeps its links.
The fact-level key is `idea` rather than `kind` because `kind` is taken by
the document header.

Follow-ups are documents naming their entry, not lines in it, so an entry
never changes when work is closed. `show` derives the Follow-ups section.

## Retrieval

`domain::graph::resolve` is the whole rule: a machine topic's claims match
the directory closest first, its `unclaimed` topics load only when nothing
matched, the machine topic always loads, and `includes` is walked
breadth-first from each root, each topic once. Every claim loads; there is
no shadowing of a wider claim by a narrower one, so a topic that should
not reach every project must not be claimed for a wide directory.

`context` is printed at session start and a session is shown only its
first couple of kilobytes, so the text renderer keeps it to names and
counts, must-act-on items first. `tests/cli.rs` pins the shape.

## Output contract

stdout carries data only; refusals and notes go to stderr. Exit 1 is a
refusal or a store problem, 2 a usage error. A listing that reports a
problem a person may act on, a fork, a claim on a directory this host
lacks, still exits 0: the listing is the answer, and `check` is the
command whose exit code means something.

## The skill

`skill/SKILL.md` is compiled into the binary so the skill a session reads
is the one written for the commands it has, and `tests/skill.rs` holds the
two to each other. The session hook is merged into the agent's settings
file rather than written over it, because that file is the user's and holds
other hooks; a scripted `init` touches neither the skill nor the settings
unless told to, since a script writing into `~/.claude` unasked is the
kind of surprise the store exists to avoid. The skill is public and written
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
