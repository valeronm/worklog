# worklog

worklog gives your coding agents one memory, shared between your machines
and between the agents themselves. It records the work you and your agent
do, what the agent learns along the way, ideas you haven't built yet and
things to come back to.

Coding agents keep a memory of their own, but each keeps it for itself,
on the machine where it ran. What Claude Code learned on your desktop
isn't there when you open Codex, or when you open the same project on
your laptop. So you explain the same constraints again, or dig through
old chats to find why something was decided.

worklog works with Claude Code and Codex, on Linux and macOS. Your records
are files in a folder, your store, which you can sync between machines
with any file sync tool, such as Syncthing, Dropbox, Google Drive or
OneDrive. worklog needs no hosted service and no account of its own.

## What it keeps

worklog files records under topics. A topic is a project, a machine, a
tool or anything else you work on. The records are:

- **facts**: what's true now, such as a decision and its reasons, or what
  an experiment ruled out
- **ideas**: designs you've settled on but haven't built yet
- **entries**: work you've done, dated, with what was decided along the
  way
- **followups**: things to come back to, which can be set to come up
  again on a date or the next time you work on one of their topics

On each machine, a project folder is connected to its topic, so a session
in that folder starts with a list of the topic's facts and ideas and the
followups that are due. Each machine has a topic of its own too, whose
facts and ideas every session on that machine gets.
[docs/documents.md](docs/documents.md) describes each kind of record and
how worklog picks what a session gets.

worklog keeps one person's records, on as many machines as they use. It
isn't built for a team to share: information everyone working on a
project needs belongs in the project's own docs.

## Status

worklog is at 0.x: its commands and its store format can still change
between releases.

## Install

You can install worklog on Linux x86_64 or aarch64, or on macOS Apple
Silicon:

```sh
curl -fsSL https://raw.githubusercontent.com/valeronm/worklog/main/packaging/get.sh | sh
```

The installer picks the release for your machine, checks its checksum and
puts it in `~/.local/bin`, or in `$PREFIX` if you set it. Make sure that
folder is on your shell's `PATH`, then try `worklog --version`. You only
need Rust if you want to [build from source](#contribute).

To update later, run `worklog upgrade`. Upgrade every machine that shares
a store at the same time, since an older release refuses to change a
record written in a newer format.

## Set up this machine

```sh
worklog init
```

It asks where to keep your records and what to call this machine. To
have the same records on other machines, choose a new folder inside one
your sync tool shares. If the folder already holds a store, setup joins
it; otherwise it asks before creating one. It won't use a folder that
holds other files.

Setup then offers to configure the coding agents it finds. Say yes to let
them use worklog; [What setup changes](#what-setup-changes) lists what
that writes to their settings. If you skip this step, you can run
`worklog agents install` later.

## Work with your agent

Start a new Claude Code or Codex session in your project folder and ask:

```text
Set up worklog for this project.
```

The agent creates a topic for the project, connects it to this folder and
fills in a description of your machine if it still needs one. From then
on, each session there starts with that list, and your agent reads a
record in full when it needs it.

worklog's [skill](skill/SKILL.md) teaches your agent to record the work
you do together and what it learns. When work is left unfinished, your
agent asks whether to do it now, save it for later or drop it. You can
ask for a record or a reminder yourself, too. For example, after looking
into a flaky login test:

```text
You: Log what we tried. Next time we work on this project, remind me
     to run the login test with a fixed clock.

Agent: Saved what we found, and a reminder for the fixed-clock run
       next time we work on this project.
```

The next day, in Codex on your laptop:

```text
You: What's next on this project?

Agent: The fixed-clock run for the login test is due. Last time, we
       ruled out leftover test data and found that the session expired
       before the assertion.
```

## Share between machines

On another machine, once your sync tool has copied the store folder
there, install worklog, run `worklog init`, choose that folder and give
that machine a name of its own. Then ask your agent to connect the
project to its existing topic. Your project can live in a different
folder on each machine.

Each change adds a new file and keeps the old ones, so file sync only has
to copy files. If two machines change the same record before syncing,
worklog keeps both versions as a fork, and your agent combines them,
asking you where they disagree.
[docs/store.md](docs/store.md) describes the files and their versions.

## Browse your records

```sh
worklog serve
```

Open the address it prints in your browser to search your records, read
them and see each one's history. The pages are read-only; to add or
change something, ask your agent. Press Ctrl-C to stop the server.

Each page reads the store when it loads, so records synced from other
machines show up without a restart. By default, only your machine can
reach the server. If you use `--bind` to listen on another address,
anyone who can reach that address can read the store without logging in.
To open it by a hostname, also pass that name with `--host`.

## What setup changes

Setup gives each agent worklog's skill and a hook that runs
`worklog context` at the start of each session. The hook is added to the
agent's hooks file beside your other hooks:

- Claude Code: the skill in `~/.claude/skills/worklog/` and the hook in
  `~/.claude/settings.json`
- Codex: the skill in `~/.codex/skills/worklog/` and the hook in
  `~/.codex/hooks.json`

Codex runs commands in a sandbox that keeps the store read-only. Setup
also gives Codex a rule, `~/.codex/rules/worklog.rules`, that lets
`worklog` commands run outside the sandbox without asking. It adds
worklog's drafts folder, where your agent writes a record before saving
it, to `sandbox_workspace_write.writable_roots` in `~/.codex/config.toml`,
so Codex's edit tool can write there.

The skill and the hook ship inside the binary. `worklog refresh` brings
the ones you have up to date with the binary you're running; run it after
moving the binary, since the hook holds its path. The installer and
`worklog upgrade` run it for you. If fish is set up, it also writes
worklog's completions for fish.

## Your data

worklog logs each command run against the store, with its arguments, the
folder it ran in and the agent session it ran under. The log is kept in
the store's `usage` folder, so it syncs along with your records, and
`worklog usage` summarizes it. [docs/store.md](docs/store.md#usage)
lists everything a log line holds.

Only the installer and `worklog upgrade` reach the network, to check for
or download a release from GitHub. Your records and the usage log go
wherever your sync tool copies the folder, and what your agent reads from
the store goes to its model provider like the rest of the conversation.

## Uninstall

```sh
worklog uninstall
```

It removes what setup added to your agents' settings and leaves the rest
of each file as it was. It also removes worklog's fish completions and the
config, drafts and cache worklog keeps for this machine, and it refuses
while a draft is still open. Then delete the binary at the path it prints.

Your store is a folder of its own. Keep it or delete it as you choose; if
your sync tool shares it, deleting it here deletes it on your other
machines too.

## Contribute

For bugs and ideas, open an
[issue](https://github.com/valeronm/worklog/issues) or send a
[pull request](https://github.com/valeronm/worklog/pulls). Before
changing code, read [CLAUDE.md](CLAUDE.md) for the design and testing
rules. Use made-up content in examples and fixtures; keep real worklog
records out of the repo.

Use the Rust toolchain pinned in [mise.toml](mise.toml). From a checkout:

```sh
cargo build --locked
cargo test --workspace --locked
```

The tests use temporary stores. To try the checkout without touching your
own records, set `WORKLOG_HOME` to an unused folder on every run; it gives
the checkout its own config, drafts and cache. Give `init` a `--store` of
its own too:

```sh
env WORKLOG_HOME=/tmp/worklog-dev cargo run --locked -- init desk \
    --store /tmp/worklog-dev/store --summary "A development machine"
env WORKLOG_HOME=/tmp/worklog-dev cargo run --locked -- context
```

`WORKLOG_HOME` doesn't cover your agents' settings or fish completions.
A build run from the checkout can't set them up or refresh them: only the
`worklog` your `PATH` finds can.

The [CI workflow](.github/workflows/build.yml) has the full set of checks.
If you're changing the store format, read the
[release guide](docs/release.md) first.

To install your build, run `packaging/install-build.sh`. It replaces the
installed binary and runs `worklog refresh`. `worklog upgrade` leaves that
build in place until a release with a higher version number is out.

worklog uses the [MIT license](LICENSE).
