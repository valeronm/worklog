# worklog

A store of work done, durable facts, follow-ups and topics, kept as
append-only versioned documents and read and written only through this
binary. It is the memory a coding agent opens a session with: which facts
hold for the directory it is in, what is due, and what happened last time.

## The store

One directory, meant to be shared between machines by a file sync such as
Syncthing. Nothing in it is ever edited or deleted; a write is a new file.
A folder Syncthing manages is given an ignore file for Finder's metadata
and the tool's own staging files on the first write from a host, if it
has none.

```
~/worklog/
  entry/2026-09/2026-09-04-lamp-driver/<hash>.md   one piece of work, dated
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
worklog new topic lantern                  # what the project is; then save
worklog claim lantern                      # this checkout is lantern, on this host
worklog context                            # the index a session opens with
worklog new entry lamp-driver              # prints a draft path to edit
worklog save 2026-09/2026-09-04-lamp-driver
worklog new followup port --entry 2026-09/2026-09-04-lamp-driver \
    --summary "Add the second relay" --recheck "2026-10-01 the board is back"
worklog followups lantern                  # open work with each item's state
worklog done 2026-09-04-port "dissolved by [[2026-09/2026-09-10-relay-landed]]"
worklog check                              # every rule the store has to keep
worklog usage                              # how often each command ran, per machine
```

Prose goes through a draft: `new` or `checkout` prints a file to edit,
`diff` shows it against the store, `save` validates it and writes the
version. State changes such as `done`, `recheck`, `verify`, `tombstone` and
`rename` write a version directly; `done` takes a note saying what
dissolved the followup, and `tombstone` requires one saying what ended the
document. `--json` on any command gives the same data structured.
`worklog --help` lists everything.

`worklog serve` answers on `127.0.0.1:8080`, or `--bind` elsewhere, with
the store as read-only pages: topics with their facts, entries and open
work, every document with its history and each version's diff against its
parent, listings, search and `check`. Nothing is cached; a page is what
the command would print at that moment, so a machine holding a synced copy
of the store can serve it.

## Install

```
curl -fsSL https://raw.githubusercontent.com/valeronm/worklog/main/packaging/get.sh | sh
```

Downloads the release for this machine's architecture into `~/.local/bin`,
verifying the checksum published beside it. Or build from source with a
current stable Rust: `cargo install --path .`.

The binary carries the agent skill that teaches a coding agent the store
and its commands, and the SessionStart hook that opens every session with
`worklog context`. An interactive `worklog init` offers both; `worklog init
<name> --skill --hook` takes them outright, and `--no-skill` or `--no-hook`
declines without a question. On their own, `worklog skill install` writes
`~/.claude/skills/worklog/SKILL.md` and `worklog hook install` merges one
entry into `~/.claude/settings.json`, once, keeping the rest of the file as
it is; `show` on either prints what would be written. The installer above
refreshes the skill on a host that has one, since the skill describes the
commands of the binary it shipped with.

`worklog completions <shell>` prints completions for the commands of the
binary it runs from. The installer writes the fish ones into the
completions directory on a host that has one, and rewrites them with every
install, so they never drift from the binary; another shell takes its
output in the place it reads.

## Develop

`cargo test` runs the unit tests, the layer test and the command-line
scenarios over a scratch store. `init` writes `~/.config/worklog/config`,
the machine name and the store directory, and nothing reads a store before
it exists. `WORKLOG_HOME=<dir>` points a run at `<dir>/config`,
`<dir>/drafts` and a default store of `<dir>/store`, which is how the tests
and a migration dry run keep clear of the real ones. CLAUDE.md holds the
constraints worth knowing before changing anything.
