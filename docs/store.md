# The store format

What a store holds on disk: the tree, the shape of a version file, how a
document's state follows from its files, what a reader refuses, and the
files a host keeps beside the store. `documents.md` covers the kinds and
their fields.

A store is shared between machines by a file sync, so the shape of a file
is a contract. Nothing in it is edited in place, merged or repaired.

## Layout

```
<store>/
  <bucket>/<document id>/<version id>.md
  usage/<machine>-<YYYY-MM>.jsonl
  .stignore                               in a Syncthing folder only
```

A document is a directory named by its id, under a bucket named by the
id's first two characters. Every version of the document is one file in
that directory, named by the version's id with `.md`.

A reader takes a directory as a bucket when its name is two lowercase hex
characters, a directory in it as a document when its name is a document id
that starts with the bucket's name, and a file in that as a version when
its name is a version id followed by `.md`. Everything else in the tree is
passed over, so a sync tool may keep files of its own beside the documents.

A version is written under the name `.tmp-<version id>` in its document's
directory and renamed into place, so a sync does not carry a partial file.
A version whose file is already there is not written again. In a Syncthing
folder, recognized by its `.stfolder` marker, a store with no `.stignore`
is given one when a version is written, covering `.DS_Store` and the
`.tmp-` prefix; one already there is left alone.

## Ids

A document id is 32 lowercase hex characters. It is given when the
document is created, from 16 random bytes for one this binary makes, and
never changes.

A version id is `b3-` followed by the BLAKE3 hash of the file's bytes as 64
lowercase hex characters. A reader refuses a file whose bytes do not hash
to its name.

## A version

A version is one immutable UTF-8 file: a TOML header between two lines
that hold only `+++`, then the body. Lines end in LF, and the file opens
with the fence, no byte order mark before it.

```
+++
format = 1
document = "7f3a91c05be2446d8a10c3f29b7e6d54"
kind = "fact"
parents = ["b3-5c1e…"]
ancestors = ["b3-5c1e…", "b3-e759…"]
written = 2026-09-04T10:15:07.412093+01:00
machine = "03be552100000000000000000000000a"
change = "save"

[fields]
name = "relay-pin-is-fixed"
topic = "2be0417f00000000000000000000000b"
created = 2026-09-01
confirmed = 2026-09-04
summary = "The relay pin is fixed on the board"

[links]
"lantern/boot-straps" = "41c2d09a00000000000000000000000c"
"lantern/unwritten" = ""
+++
The relay is wired to GPIO 5.
```

The opening fence is the file's first line, and the header ends at the
first later line that is only `+++`. The body is every byte after that
line. A writer refuses a header in which a value holds such a line.

The header's keys:

| key | value | meaning |
| --- | --- | --- |
| `format` | an integer above zero | the layout the file is written in; `1` |
| `document` | a document id | the document this is a version of |
| `kind` | lowercase ASCII letters | the document's kind |
| `parents` | a list of version ids | the versions this one follows: none for a first version, one for a change, every head for a version that closes a fork |
| `ancestors` | a list of version ids | every version behind this one, the parents included |
| `written` | a TOML offset date-time, unquoted | when it was written, by the clock and at the offset of the writing host |
| `machine` | a document id | the machine topic of the host that wrote it |
| `change` | text | the command that wrote it, as one word; a command that gives a reason writes `: ` and the reason after the word |
| `fields` | a table | the document's own fields, which `documents.md` lists by kind |
| `links` | a table | each `[[target]]` of the body and the summary, with the id of the document it resolved to when the version was saved, or `""` for one that resolved to none |

A writer emits the keys in this order, with `parents` and `ancestors`
sorted and no `links` table for a version with no link. This binary's
clock gives `written` to the microsecond, and a reader takes any
precision. A reader takes the keys in any order and reads a missing
`fields` or `links` as an empty table.

A reader refuses a file that is not UTF-8, has no opening or closing
fence, whose header is not TOML, that lacks one of the keys from `format`
to `change` or holds one with a value of another type, whose `fields` or
`links` is not a table, that holds a link whose value is neither `""` nor
a document id, or that holds a key this table does not list. A file whose
`format` is greater than the reader's own is set apart as written by a
newer binary, and its other keys are not interpreted. A writer refuses to
make a version that would not read back as the parts it was made from.

Hosts stamp their own clocks, so versions are ordered across machines by
the instant `written` gives and not by its text.

## A document's state

A document is every version file in its directory. Its state is derived
from the files each time it is read; nothing stores it. Every version of a
document carries the same `kind`. A reader does not check it and takes the
kind of the version with the lowest id.

1. A file that does not read as a version, or whose `document` is not the
   directory it sits in, is kept apart as unreadable and takes no part in
   the rest.
2. The heads are the versions that no version in the document lists among
   its `ancestors`.
3. No version: the document does not exist.
4. One head: the document is live, and that head is its current version.
5. Two or more heads: the document is forked, the heads in the order of
   their ids.

Since a version lists everything behind it, a version that arrives before
its parents is a head on its own account, and the parents that arrive
later are not.

A fork arises when two machines write from the same parent before they
sync. It is reported, not merged: a read shows every head, and the
only write a forked document takes is a version whose `parents` are all of
its heads. A document holding an unreadable file takes no write at all,
and `show` and `history` note the file beside what they print.

A document's history is its versions with the one that has the most
ancestors first, the id deciding between two that have as many.

Whether a document is ended is a matter of its fields, not of the file
format: `documents.md` has the ending.

## What a newer binary wrote

A reader that meets a `format` above its own reports the file as written
by a newer binary: `check` lists it as a notice, where any other file that
does not read is a problem, and the document takes no write on that host.

A kind the reader does not know is kept: the documents read as versions,
`check` gives one notice per kind, with the count of its documents, and
nothing interprets their fields. A `change` is free text, and the reason
a document ended for is carried as a word, so a reader passes a value it
has not seen.

A header key outside the table above, and a field its kind does not have,
are not tolerated: the first makes the file unreadable and the second the
head, and `check` reports either as a problem. `release.md` has what that
asks of the machines sharing a store.

## Usage

The usage log is not a document, because a count is not immutable. A host
appends one line per command run to its own file for the month,
`usage/<machine>-<YYYY-MM>.jsonl`, where `<machine>` is the id of its
machine topic and the month is that of the line's `written`. A reader
takes every file of that name shape under `usage/`, so a sync carries the
log with no two machines writing one file.

A line is one JSON object ending in a newline, with every key present and
`null` under one the run has nothing for:

```
{"written":"2026-09-04T10:00:00.123456+01:00","machine":"03be552100000000000000000000000a","version":"1.2.3","agent":"Codex","session":"s-1","command":"search","exit":0,"refusal":null,"hits":2,"directory":"~/projects/lantern","arguments":["lantern"]}
```

| key | meaning |
| --- | --- |
| `written` | when the command ran, as a version's `written` is given |
| `machine` | the id of the machine topic of the host that ran it |
| `version` | the version of the binary that ran, `X.Y.Z` for a release and `X.Y.Z+<name>` for a build made on top of one |
| `agent`, `session` | the coding agent the command ran under and the id of its session; a line with one of them alone reads as having neither |
| `command` | the command's path, `new entry` for a command with a subcommand, and empty where the words reach no command |
| `exit` | the exit code |
| `refusal` | the message of a run that failed; for a command line the parser refused, its first line |
| `hits` | how many documents a `search` matched |
| `directory` | the working directory, with the home directory as `~` |
| `arguments` | the words typed, without those of the command's path |

A reader passes over a key it does not know and over a line that does not
read, which is what a sync delivers of a line it cut short.

A line is appended only by a binary that `PATH` finds under the name it
was run by, since the log counts use and a build under test is not use. It
is appended for a command that reads or writes the store and for `serve`;
binding a host, the commands for the agents, `refresh`, the completions
and `upgrade` write none, and neither does a shell asking for completions. A
command line the parser refuses is logged with its exit code under the
last command its words reach. No line is written into a
directory that holds nothing of the host's machine topic, so a store that
has not synced or mounted is not started a second time under its path.

## The host's config

`init` writes the config once per host, as TOML:

```
store = "/home/desk/sync/worklog"
machine = "03be552100000000000000000000000a"
```

`store` is the store's directory as an absolute path, and `machine` the id
of the host's machine topic, which every version and usage line written on
the host carries. A file without both keys, with a `store` that is not
absolute or a `machine` that is not a document id, is refused. A host with
no config file is not set up. A config whose `store` is not a directory is
refused by every command that uses the store, so a store that is gone is
not read as an empty one.

The file is `$XDG_CONFIG_HOME/worklog/config.toml`, or under `~/.config`
where that variable is not set. `WORKLOG_HOME=<dir>` moves it to
`<dir>/config.toml` and the drafts to `<dir>/drafts`.

## The host's cache

A host keeps the headers it parsed of the store's tree in one file per
store, `$XDG_CACHE_HOME/worklog/<hash of the store's path>.store`, or
under `~/.cache` where that variable is not set; `WORKLOG_HOME=<dir>`
moves it to `<dir>/cache`. The file holds, for every document the last
walk read, the header of each of its versions and the modification time
its directory had, and nothing else: no body is cached, the version files
stay what the store is, and the file is rebuilt from them whenever it is
missing, does not read, or was written by another release of the binary.

A listing, a search by topic and the session opener answer from the
headers alone. A command that shows, compares or searches a body reads
the file of each version it shows, so a search over the whole store opens
the current version of every document it searches. A version whose file
no longer reads as the cache recorded it is refused by name, and a search
matches that document on its label and summary and says why its body did
not read.

A walk takes a document as the cache holds it when its directory's
modification time is the one recorded, and reads the directory's files
otherwise. A version lands in its directory by rename, which is what moves
the time. A file rewritten in place under a directory whose time did not
move is read as the cache holds it; `check` reads every file whatever the
cache holds. A shell completion reads through the cache and leaves it as
it was, since a key press is no place for the file to be rewritten.
`serve` keeps the reading its last request made in the process and starts
the next request's walk from it in place of the file, so a page reads and
decodes the file once per process and every walk still reads each directory
whose time moved. A directory that changed within two seconds before the walk is
not recorded and is read again by the next, since two changes inside the
granularity of its time can look like one. A document holding a file that
does not read is not kept, so a damaged file is read on every walk. A
cache that cannot be written is left as it is and nothing is refused.

## Drafts

Prose reaches the store through a draft, an editable file outside it. A
host's drafts are its own and are not synced. They sit in one directory,
`$XDG_STATE_HOME/worklog/drafts`, or under `~/.local/state` where that
variable is not set:

```
<drafts>/
  <document id>.md
  <document id>.origin.toml
```

The `.md` file is what a person or an agent edits: the document's fields
as TOML between two `+++` lines, then the body. It carries neither the
header's other keys nor the fields only the tool writes, and it shows a
field that holds another document as that document's address where it has
one. `documents.md` has which fields those are.

The `.origin.toml` file is what the draft started from:

```
kind = "fact"
parents = ["b3-5c1e…"]
agent = "Claude Code"
session = "7f3a91c0-…"
```

`agent` and `session` name the agent session the draft was opened in, and
are both absent for a draft opened outside one. `drafts` compares them
with the session it runs in to say whose a draft is, and only reports it.

`parents` is empty for a document that does not exist yet, one head for a
document opened as it stands, and every head for a fork being resolved. A
`save` compares it with the document's heads: a draft with no parent makes
a first version of a document that has none, a draft whose parents are the
heads makes the next version, and any other draft is refused as stale.

The origin is written before the text. A text without its origin is
refused, an origin without a text is no draft, and a file in the directory
whose name is not a document id with `.md` is passed over. Storing or
discarding a draft removes both files.
