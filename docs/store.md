# The store format

What a store holds on disk, byte for byte: the grammar every file uses,
the shape of a version, how a document's state follows from its files,
and what each operation writes. `documents.md` covers the four kinds and
their fields.

A store is shared between machines by a file sync, so the shape of a
file is a contract. Nothing in it is edited in place, merged, or
repaired.

## Layout

```
<store>/
  entry/<YYYY-MM>/<YYYY-MM-DD>-<name>/<id>.md
  fact/<topic>/<name>/<id>.md
  followup/<YYYY-MM-DD>-<name>/<id>.md
  topic/<name>/<id>.md
  usage/<machine>-<YYYY-MM>.tsv
  .stignore                                   in a Syncthing folder only
```

The four kind directories hold documents; a document is a directory
named by its slug, and every file in it is one version, named by the
version's id with `.md`. The reader takes a file as a version only when
its name parses as an id, and skips a directory whose name opens with a
dot. A version file that does not read, or whose `slug` field does not
name the directory it sits in, fails the whole document rather than
being skipped, so a damaged file never leaves a stale head in its place.

A version is written under a `.tmp-` name and renamed into place, so a
sync never ships a partial file. Writing a version whose file already
exists does nothing. In a Syncthing folder, recognised by its
`.stfolder` marker, `.stignore` covers the `.tmp-` prefix and
`.DS_Store`; an existing one is left alone.

## The frontmatter grammar

Every version, every draft and the config file use one grammar. It looks
like a subset of YAML and is not YAML: there is exactly one way to write
any content.

```
---
key: scalar
key: [item, item]
key:
  sub: scalar
  sub: [item]
---
body
```

- A file is UTF-8 with LF line endings and no byte order mark. The
  opening fence is the first line.
- A key matches `[A-Za-z_][A-Za-z0-9_-]*`, ASCII only, and appears once.
- A scalar is the rest of the line after `: `, trimmed. It holds no
  newline and no leading or trailing space. An empty scalar is written
  `key:` with nothing after the colon.
- A list is `[a, b]`. Items are trimmed, never empty, and hold no comma;
  `[]` is the empty list, and a list never spans lines.
- A map is a key with nothing after the colon followed by lines indented
  by exactly two spaces, each a scalar or a list. Nesting stops there.
- Fields keep the order they were written in. Order is part of the bytes.
- The body is everything after the closing fence, copied byte for byte.

There is no escaping. A scalar that opens with `[` and closes with `]`
reads as a list, and a comma inside a list item splits it, so a value in
either shape is refused when a draft is saved. A reader skips a blank
line anywhere in the fields and a `#` line at the top level; that
tolerance is for the files a person edits, and a version with either is
refused.

## A version

A version is one immutable file: the document header, the kind's own
fields, and a `version` block last.

```
---
slug: lantern/relay-pin-is-fixed
kind: fact
tags: [lantern]
verified: 2026-09-04
summary: The relay pin is fixed on the board
version:
  parents: [e759ec9a…]
  written: 2026-09-04T10:15:07.412093+01:00
  machine: desk
  operation: save
---
**Why:** …
```

`slug` and `kind` name the document; `kind` is the kind directory and the
slug's shape has to agree with it. The kind's own fields follow, in the
order and with the presence rules the kind's table in `documents.md`
gives. The block's keys, in this order:

| key | presence | meaning |
| --- | --- | --- |
| `parents` | always | the ids this version follows: none for a first version, one for a change, every head of a fork for a `resolve` |
| `written` | always | RFC 3339 with microseconds and the writing machine's own offset |
| `machine` | always | the configured name of the writing machine, never a hostname: ASCII letters, digits, `.`, `_` and `-` |
| `operation` | always | which command wrote it |
| `superseded_by` | if set | the new slug, on the tombstone a rename leaves |
| `renamed_from` | if set | the old slug, on the first version a rename writes at the new slug |

Machines stamp their own clocks, so the instant `written` names, not
the string, is what orders versions across machines.

### The id

A version's id is the BLAKE3 hash of the file's bytes: 64 characters of
lowercase hex. The file is named by it, and a reader refuses a file
whose bytes do not hash to its name. The reader also refuses a file that
parses but is not byte for byte what the writer would emit for its
content.

## A document's state

A document is every version under its slug. Its state is derived from
the files each time it is read; nothing stores it.

1. The heads are the versions no other version in the document names as
   a parent.
2. One head that is a tombstone, either `operation: tombstone` or
   `operation: rename` with `superseded_by`, means the document is
   tombstoned.
3. One other head means the document is live, and that head is its
   current version.
4. Two or more heads mean a fork. The heads are ordered by id. A fork
   with a tombstone among its heads is still a fork.
5. No version at all: the document does not exist.

A slug is never reused: outside a fork, nothing is written after a
tombstone, so it stays a head. The one exception is a removed document
tombstoned again to replace its note, whose new tombstone follows the
old. A renamed document may not be, since its tombstone points
elsewhere.

Forks are reported, never merged. Every read shows every head, `check`
counts them, and only `resolve` writes a version naming them all, a
tombstone among them included.

`history` orders a document newest first, each version after every
version naming it as a parent. Versions on a fork share no order, so the
id decides between them.

## Operations

Each operation names the command that writes the version, so `history`
and a fork report can say what happened.

| operation | parents | what changes |
| --- | --- | --- |
| `new` | none | a first version, with the fields and body of a draft or of `new followup` |
| `save` | the one head | the fields and body of a draft |
| `resolve` | every head | the fields and body of a draft, closing a fork |
| `done`, `drop` | the head | `state` on a followup; a note, if given, is appended to the body |
| `recheck` | the head | `recheck` on a fact or followup |
| `verify` | the head | `verified` on a fact |
| `tombstone` | the head | nothing in the fields; the body is the note alone |
| `rename` | see below | two versions |
| `claim`, `unclaim` | the head | `claims` on a machine topic |
| `migrate` | none | a first version imported from the old store |

A `save` whose draft names a parent that is no longer the head is
refused, so a fork can arise only between two machines that wrote from
the same parent before syncing.

A note is what a tombstone says about why, or what closed a followup.
`done` and `drop` append it as the body's last line; a tombstone's body
is the note alone, trimmed and never blank, and is what a read of the
removed document shows. A note is where the link to what ended or
dissolved the document goes.

### Rename

A rename writes two versions. Under the old slug, a tombstone with
`operation: rename` and `superseded_by` naming the new slug, parented on
the old head, with an empty body. Under the new slug, a first version
with the same fields and body, `operation: rename`, `renamed_from`
naming the old slug, and the tombstone as its parent. That parent link
is the one place a chain crosses documents.

A read of the old slug follows `superseded_by` to wherever the document
is now; a write to it is refused and names the new slug.

## What a newer binary wrote

The grammar grows only by adding: a key in the block, an operation, a
field on a kind. A binary meeting one it does not know still reads the
version, keeping the block as written so the bytes still hash; `show`,
`history` and `check` note it, and a write that would follow it is
refused, so the machine that cannot read the grammar never writes over
it. An unknown operation reads as a live head, since the reader cannot
tell whether it ended the document.

Anything else, a new envelope key, a new kind, a new slug shape, needs
every machine sharing the store upgraded before the first write.
`release.md` has the step.

## Links

`[[slug]]` anywhere in a body or a summary is a link to that document, and
resolves to its current version. The target is a bare slug, whose shape
says the kind. A `[[…]]` inside an inline code span is quoted, not made.

`check` reads every link in every live document and in every removed
document's note; a forked document is reported as a fork and its heads
are not read. A problem is a rule the store has to keep; a notice is a
judgement call for a person.

| link | from | `check` says |
| --- | --- | --- |
| naming no document shape, or no document | anything | a problem |
| to a removed document | an entry | nothing |
| to a removed document | any other kind | a notice |
| to a removed document with no note | anything | a notice on the removed document |

An entry cites what was true on its date; any other kind rests on what
it links, and has gone stale when that is removed.

## Usage

`usage` is not a document, because a count is not immutable. A machine
appends a line to its own file for the month on every command run
against a store, and reads every machine's files, so a sync carries
them without a conflict to report.

A line is tab-separated fields ending in a newline: the `written` stamp,
the machine, the command path, the exit code, the working directory
with `~` for home, then one field per argument. Inside a field, a
backslash, a tab, a newline and a carriage return are written `\\`,
`\t`, `\n` and `\r`. A line a sync delivers half-written reads as
nothing.

## Drafts

Prose reaches the store through a draft, an editable file outside it,
under the host's own state directory and never synced.

```
<drafts>/
  entry/<YYYY-MM>/<YYYY-MM-DD>-<name>.md
  entry/<YYYY-MM>/<YYYY-MM-DD>-<name>.parents
  fact/<topic>/<name>.md
  fact/<topic>/<name>.parents
  …
```

A draft is the document's own fields and body, the same text `show`
prints, with no header and no block, so nothing in it belongs to the
tool. The ids it was checked out from sit in the `.parents` file beside
it, one per line, and empty for a document that does not exist yet.
That file is what `save` relies on and an editor never opens: its
contents decide whether the version written is a `new`, a `save` or a
`resolve`, and whether the draft is stale.

The sidecar is written before the draft, so a draft on disk always has
its parents beside it; a draft found without one is refused, naming the
missing file. `save` and `discard` remove both files. A `.parents` file
left on its own is not a draft and is ignored.

## The config

`init` writes the config once per host, under the platform's config
directory as `worklog/config`, in the grammar above without fences, so
the tree has one syntax:

```
machine: desk
store: /home/u/worklog
```

`machine` is what every version written on the host is stamped with, and
what names its usage file.
