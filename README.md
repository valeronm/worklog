# worklog

A store of work done, durable facts, ideas, follow-ups and topics, read and
written only through this binary. It is the memory a coding agent opens a
session with: which facts hold for the directory it is in, what is due, and
what happened last time.

## The store

One directory, meant to be shared between machines by a file sync such as
Syncthing. Nothing in it is edited or deleted; a write is a new file.

A document is a directory named by an id it is given when it is made, and
every file in it is one immutable version, named by the hash of its bytes
and listing the versions it follows. A document's name is a field of its
versions and one document refers to another by id, so a rename moves no
file and rewrites no other document, and two machines never write the same
path with different content.

Two machines that change one document before they sync leave it with two
current versions. That is a fork: reads show every one, `check` counts it,
and a person settles it by saving a version that follows them all. Nothing
merges on its own, so the store needs no merge tool and no server, only a
sync that carries files.

A topic is a project, a device, a tool or a subject, and facts, ideas,
entries and followups are filed under topics. Each machine has a topic of
its own, and a claim places a topic in a directory of one machine, so the
same store serves machines with different layouts. `docs/documents.md` has
the kinds and the rule for what a session loads, and `docs/store.md` the
file format.

## Install

```
curl -fsSL https://raw.githubusercontent.com/valeronm/worklog/main/packaging/get.sh | sh
```

Downloads the release for this machine's operating system and architecture
into `~/.local/bin`, or into `$PREFIX`, after verifying the checksum
published beside it. Or build from source with the Rust toolchain
`mise.toml` pins: `cargo install --path .`.

From then on `worklog upgrade` does the same from the binary itself when a
newer release exists, verified the same way, and `worklog upgrade --check`
only reports the two versions.

## Set up a host

A host is bound once to its machine topic and to the directory of the
store. `init` on a terminal asks for both:

```
worklog init
```

Where the store holds the topic named, `init` binds the host to it; where
it holds none, `init` makes the topic, and `context` reports that it needs
a summary until one is written. A second host joins a store another
machine made with its own machine topic. A directory holding no document
is started as a store only on a yes, since a shared store may not have
synced yet; a directory of other files is not taken.

Without a terminal, `init` takes both as arguments, with no default for
either. `--summary` makes the topic where the store holds none, and
starts a missing or empty directory without asking:

```
worklog init desk --store ~/sync/worklog --summary "The desk machine"
```

Until a host is set up, every command that uses the store is refused, and
`context` prints a note and exits 0 so a session still opens.

A project is then given a topic and its checkout a claim:

```
worklog new topic lantern      # prints the path of a draft to edit
worklog save lantern           # stores the edited draft
worklog claim lantern          # run inside the checkout
worklog context                # what a session in this directory opens with
```

Prose goes through a draft: a command prints the path of a file to edit,
and `save` checks it and stores it as the document's next version. State
changes are commands that write a version directly. `--json` gives a
command's result structured, and `worklog --help` lists the commands.

`worklog serve` answers with the store as read-only pages, on the loopback
address unless `--bind` gives another. On any other address the store is
served without a login to whatever reaches it. Each request reads the
store as it then stands, so a machine holding a synced copy can serve it.

## Agents

The binary carries the skill that teaches a coding agent the store and its
commands, and a SessionStart hook that opens every session with `worklog
context`. `worklog agents install` places both for each agent it knows
whose home directory exists: the skill under the agent's skills, and one
hook merged into the agent's hooks file, the rest of which is kept. `init`
offers it, and given arguments does it with `--agents` and otherwise
writes neither.

`worklog agents uninstall` takes both away and leaves whatever else the
files hold. `worklog agents refresh` brings a skill and a hook up to the
binary that runs it, only where each already is: the skill describes the
commands of the binary it shipped with, and the hook holds the binary's
path, which an install to a new place leaves stale. `worklog refresh` does
the same for the agents and for fish's completions, and the installer and
`upgrade` run it.

Codex runs a command in a sandbox that holds everything outside the
project read-only, the store with it, so a `worklog` it ran would read the
store, write nothing to it and leave no usage line. For Codex an install
therefore also writes `rules/worklog.rules`, a rule that runs every
`worklog` command outside the sandbox without asking. The file is the
tool's own: a refresh writes it wherever the skill or the hook is, and an
uninstall removes it.

## Completions

Completions are answered by the binary, so a shell is offered what the
store holds as it stands: topics where a topic is expected, documents
where a document is, open drafts for `save`. `worklog completions <shell>`
prints the line a shell's startup file holds to take them; for fish,
`refresh` writes that line where fish reads completions, once that
directory exists.

## Develop

`cargo test --workspace` runs the unit tests, the layer test and the
command-line scenarios, each over a scratch store. `WORKLOG_HOME=<dir>`
moves the host's config and drafts under `<dir>`, which is how the
scenarios keep clear of a real store.
`CLAUDE.md` holds the constraints worth knowing before changing anything.
