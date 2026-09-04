# worklog

A store of work done, durable facts, follow-ups and topics, kept as
append-only versioned documents and read and written only through this
binary. It is the memory a coding agent opens a session with: which facts
hold for the directory it is in, what is due, and what happened last time.

## The store

One directory, meant to be shared between machines by a file sync such as
Syncthing. Nothing in it is ever edited or deleted; a write is a new file.

```
~/worklog/
  entry/2026/2026-09-04-lamp-driver/<hash>.md   what happened on a day
  fact/lantern/relay-pin-is-fixed/<hash>.md     what is true now
  followup/2026-09-04-port/<hash>.md            open work from an entry
  topic/lantern/<hash>.md                       what a subject is
```

Every file is one version of one document, named by the BLAKE3 hash of its
bytes. Its frontmatter names the document, the versions it follows, when
and on which machine it was written. The current version is the one no
other names as a parent. Two machines writing from the same parent make a
fork, which every command reports and `worklog resolve` lets a person
settle; nothing merges on its own.

A topic says what a subject is and which other topics a session about it
also needs. A machine's own topic says where the topics live on that host,
so the same store serves machines with different layouts.

## Use

```
worklog init desk --store ~/sync/worklog   # once per host
worklog context                            # the index a session opens with
worklog new entry lamp-driver              # prints a draft path to edit
worklog save 2026/2026-09-04-lamp-driver
worklog new followup port --entry 2026/2026-09-04-lamp-driver \
    --summary "Add the second relay" --recheck "2026-10-01 the board is back"
worklog followups lantern                  # open work with each item's state
worklog done 2026-09-04-port "dissolved by [[2026/2026-09-10-relay-landed]]"
worklog check                              # every rule the store has to keep
```

Prose goes through a draft: `new` or `checkout` prints a file to edit,
`diff` shows it against the store, `save` validates it and writes the
version. State changes such as `done`, `recheck`, `verify`, `tombstone` and
`rename` write a version directly. `--json` on any command gives the same
data structured. `worklog --help` lists everything.

## Install

```
curl -fsSL https://raw.githubusercontent.com/valeronm/worklog/main/packaging/get.sh | sh
```

Downloads the release for this machine's architecture into `~/.local/bin`,
verifying the checksum published beside it. Or build from source with a
current stable Rust: `cargo install --path .`.

## Develop

`cargo test` runs the unit tests, the layer test and the command-line
scenarios over a scratch store. `init` writes `~/.config/worklog/config`,
the machine name and the store directory, and nothing reads a store before
it exists. `WORKLOG_HOME=<dir>` points a run at `<dir>/config`,
`<dir>/drafts` and a default store of `<dir>/store`, which is how the tests
and a migration dry run keep clear of the real ones. CLAUDE.md holds the
constraints worth knowing before changing anything.
