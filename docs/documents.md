# The documents

The four kinds a store holds, what each is for, the shape of its slug,
and the fields it carries. `store.md` covers the file format they share.

## Slugs

A slug is a document's path under its kind directory, and its shape says
the kind, so a bare slug names one document across the store:

| shape | kind | example |
| --- | --- | --- |
| `<YYYY-MM>/<YYYY-MM-DD>-<name>` | entry | `2026-09/2026-09-04-lamp-driver` |
| `<YYYY-MM-DD>-<name>` | followup | `2026-09-04-port` |
| `<topic>/<name>` | fact | `lantern/relay-pin-is-fixed` |
| `<name>` | topic | `lantern` |

A segment holds ASCII letters, digits, `.`, `_` and `-`, and does not
open with a dot. An entry's month directory is the date's own prefix. A
topic name needs a letter, so it cannot be read as a month or a date. A
date is checked by shape alone. A slug is never reused; `store.md` has
the rule.

## Fields

Each kind's table lists its fields in the order a version writes them.
The presence column says whether a field is written when it has nothing
to say: an `always` list is written as `[]` when empty, an `if set`
field is left out. A field a kind does not know is refused on `save`.

`summary` is one line, and every listing shows it. `tags` name topics,
compared without case, and are how an entry or a followup reaches a
topic's page. `recheck` on a fact or a followup says when to look at it
again, in one of two forms:

- `<YYYY-MM-DD> <why>`: due on that date, with the reason.
- `touching <topic>`: due in the next session about that topic, which
  has to exist; `check` reports one that does not.

## Entry

One piece of work as it was done, dated. It says what was true on its
date and is not maintained afterwards: its links are citations, and open
work that arose in it is a followup of its own. `show` derives the
Follow-ups section from the followups naming it.

| field | presence | meaning |
| --- | --- | --- |
| `date` | always | the day the work was done; equals the slug's |
| `machine` | always | where the work happened, which is not where the version was written |
| `tags` | always | the topics it belongs to |
| `files_touched` | always | the paths the work changed |
| `summary` | always | one line |

The body is prose.

## Fact

What is true now: a constraint, a decision, a finding a later session
should not rediscover. A fact is live and maintained; it is tombstoned
when it stops being true, and `verify` records a check that found it
still holding.

An idea is a fact with `idea: true`: a settled design not yet built, kept
with its reasoning so the build starts from the decision. Clearing
`idea` makes it a fact, with its slug and links intact.

| field | presence | meaning |
| --- | --- | --- |
| `tags` | always | the topics it belongs to |
| `idea` | if set | `true` for an idea |
| `recheck` | if set | when to look at it again |
| `verified` | if set | the day it was last confirmed, as opposed to last written |
| `summary` | always | one line |

The body is the fact and its reasoning. A fact sits under a topic that
has to exist; `check` reports one under no topic.

## Followup

Open work that arose in an entry and is closed on its own. It is a
document rather than a line in the entry so that closing it writes
nothing into the entry. After it is written it changes only by state:
`done` or `drop` close it, each with an optional note saying what
dissolved or ended it, and `recheck` reschedules it. A closed followup
takes no further state change.

| field | presence | meaning |
| --- | --- | --- |
| `entry` | always | the entry slug it arose in; live when the followup is written, and present for `check` |
| `tags` | always | the topics it belongs to |
| `recheck` | if set | when to look at it again |
| `state` | always | `open`, `done` or `dropped` |
| `summary` | always | one line |

The body starts as one blank line; `done` and `drop` append their note
as its last line. A followup's slug carries the date it was opened, not
the entry's.

## Topic

What a project, device, machine or subject is, and which other topics a
session about it also needs. A topic is what facts, entries and
followups hang from; `context` resolves a directory to topics and loads
their facts.

| field | presence | meaning |
| --- | --- | --- |
| `summary` | always | one line |
| `includes` | if set | topics loaded whenever this one is |
| `machine` | if set | the host whose sessions always load this topic |
| `claims` | if set | topic to the directories claimed for it on this host |
| `unclaimed` | if set | topics loaded only when no claim matches the directory |

A topic with `machine` is the machine's own topic. It says where the
topics live on that host, so one store serves machines with different
layouts:

```
summary: The desk machine
machine: desk
claims:
  lantern: [~/projects/lantern, ~/projects/firmware]
  atlas: [~/projects/Android/atlas]
unclaimed: [personal]
```

A claim covers a directory and everything under it, spelled with `~/`
for the home directory. Every claim matching the directory loads,
closest first, with no shadowing of a wider claim by a narrower one;
`unclaimed` loads only when none matches; the machine topic always
loads. From each topic reached, `includes` is walked breadth-first,
each topic once, so a cycle ends.

A topic named in `includes`, `claims` or `unclaimed` has to exist, and a
machine has one topic; `check` reports both.

The body is prose about the subject.
