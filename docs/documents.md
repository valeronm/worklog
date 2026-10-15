# The documents

The kinds a store holds, what each is for, how a document is named, the
fields it carries, and which topics a session reaches. `store.md` covers
the file format they share.

## Kinds

| kind | what it is |
| --- | --- |
| `topic` | a project, a device, a tool, a machine or a subject |
| `fact` | one thing that is true now, filed under one topic; an idea is a fact flagged as one |
| `entry` | one piece of work as it was done, dated |
| `followup` | open work, filed under one or more topics |
| `claim` | a topic placed in a directory of one machine |

A version's `kind` is one of these words. A store may hold documents of a
kind this binary does not know; `store.md` has what becomes of them.

## Names and addresses

A document is its id, and its name is a field. A command line, a link and
a draft's reference take a document by an address:

| address | document | example |
| --- | --- | --- |
| `<name>` | a topic | `lantern` |
| `<topic>/<name>` | a fact or an idea | `lantern/relay-pin-is-fixed` |
| `<YYYY-MM-DD>-<name>` | an entry | `2026-09-04-lamp-driver` |
| the start of an id | any document | `7f3a91c0` |

A name is lowercase ASCII letters, digits and hyphens, with at least one
letter and no hyphen at either end. It does not open with a date and is
not shaped as a document id, so an address reads one way.

A followup and a claim have no name and are taken by id.

A name is unique among the documents that are not ended in its scope: the
store for a topic, the topic for a fact, the date for an entry. A renamed
document keeps its former names, and each still addresses it unless
another document holds that name now: a current name is taken before the
name of an ended document, and that before a former name. An address that
several documents hold equally is refused, and one that no document holds
as a name is tried as the start of an id.

## Fields

Each kind's table lists the fields of its own in the order a version
writes them. A field holding another document holds its id; a draft shows
it as that document's address where it has one, and a save turns it back.
A field `if set` is left out when it is empty, false or absent. A field
the kind does not have is refused.

Two fields are left out of the tables. Every kind but a claim has a
`summary`, one line and not empty, which every listing shows. A summary
that is new or changed is refused past 100 characters or with a link in
it; one a document already holds is kept as it is. A kind with
a name has `former_names`, the names the document had, left out while it
has none. A date is a TOML date, written bare as `2026-09-04`.

The tool writes some fields itself, and a draft that carries one is
refused: `created`, `former_names`, and the ending below. A field marked
*once* is not changed after the document's first version.

## Topic

What a subject is, and which other topics a session about it also needs.
Facts, entries, followups and claims are filed under topics.

| field | presence | meaning |
| --- | --- | --- |
| `name` | always | the topic's name |
| `created` | always, once | the day it was made |
| `part_of` | if set | the topics it is a part of |
| `uses` | if set | the topics a session in it also loads |

`part_of` runs both ways: a session in the part loads the whole, and a
read of the whole covers its parts, so `facts lantern` lists what is filed
on `lantern` and on every topic that is part of it, through any number of
steps. `uses` runs one way: a session in the topic loads what it uses, and
a read of the used topic does not cover the topic. A change that would let
a topic reach itself through either list is refused.

A machine's topic is an ordinary topic that a host was bound to by `init`.
Every version that host writes carries its id, so renaming it rewrites
nothing.

## Fact

What is true now: a constraint, a decision, a finding a later session
should not rediscover. A fact is rewritten when it changes and ended when
it stops being true.

| field | presence | meaning |
| --- | --- | --- |
| `name` | always | the fact's name under its topic |
| `topic` | always | the topic it is filed under |
| `created` | always, once | the day it was made |
| `confirmed` | always | the last day it was checked and held |
| `idea` | if set | `true` for an idea |

`verify` sets `confirmed` to the day it runs. A save that changes the
summary or the body does the same, unless the draft changed the date
itself.

An idea is a fact with `idea = true`: a settled design not yet built, kept
with its reasoning. It is listed apart from the facts and ends for reasons
of its own. Clearing `idea` in a draft makes it a fact, with the same id.

## Entry

One piece of work as it was done. It records what was true on its date and
is not maintained afterward.

| field | presence | meaning |
| --- | --- | --- |
| `name` | always | the entry's name on its date |
| `date` | always, once | the day the work was done |
| `machine` | always, once | the machine topic of the host the work happened on |
| `topics` | if set | the topics the work touched |

## Followup

| field | presence | meaning |
| --- | --- | --- |
| `created` | always, once | the day it was opened |
| `topics` | always | the topics it is filed under, at least one |
| `entry` | if set, once | the entry it arose in |
| `about` | if set, once | the document it is about, of any kind |
| `look_again`, `why` | if set | a day to look at it again, and what to look for then |
| `touching` | if set | one of its own topics, to raise it when a session touches that topic |

An open followup carries one trigger: `look_again` with `why`, or
`touching`. One whose document already has a head with neither may keep
none; it is never due and is found by listing.

## Claim

`claim` makes one and `unclaim` ends it; a claim is not edited and not
reopened.

| field | presence | meaning |
| --- | --- | --- |
| `machine` | always, once | the machine topic of the host the claim is on |
| `topic` | always, once | the topic placed |
| `directory` | if set, once | where: `~`, a path under `~/`, or an absolute path |

A directory at or under the host's home is stored in the `~` form, after
the links in it are followed, so one store serves hosts whose homes are in
different places. A claim with no directory places its topic in every
session that no claimed directory covers.

## Ending

A document ends by a version that adds the ending to its fields, and is
brought back by one that takes it away. An ended document still reads, is
left out of listings unless asked for, and takes no other change until it
is reopened.

| field | presence | meaning |
| --- | --- | --- |
| `ended` | always | the reason, a word |
| `ended_on` | always | the day |
| `ended_by` | if set | the document that ended it |
| `note` | if set | why; only `done` goes without one |

| kind | reasons |
| --- | --- |
| topic | `retired`, `merged` |
| fact | `false`, `moved`, `superseded` |
| idea | `built`, `abandoned` |
| entry | `removed`, `merged` |
| followup | `done`, `dropped` |
| claim | `removed` |

A topic does not end while a fact, a followup, a claim or another topic
that is not ended holds it. A reference made by a write has to lead to a
document of the right kind that is not ended, except a followup's `entry`
and `about`, which may lead to an ended one.

## Links

`[[address]]` in a body or a summary is a link. It is resolved when the
version is saved and stored with it as the target's id, so it follows its
target through a rename. One that resolved to nothing is stored as such
and tried again by its text whenever it is read. A later version keeps
the id a link has in the version it follows and resolves only a link that
has none there, so a link holds after its text stops naming the target,
as it does when a fact is moved to another topic. Where the heads of a
fork hold one text with different ids, the version that closes the fork
resolves it again. A link inside a code span or a fenced block is quoted,
not made.

`check` reads the links of each topic, fact and followup that is not
ended, and gives a notice for one that leads nowhere or to an ended
document. An entry's links are citations of what was true on its date and
are not checked. A page shows every link to a document that has ended
struck through, in an entry too.

## What a session reaches

`context` resolves a directory to topics. The directory is compared in the
form a claim stores, and a claim covers its directory and everything under
it by whole path components.

1. The claims are those of the host's machine topic that are not ended and
   whose topic is not ended.
2. Where a claim with a directory covers the session's directory, the
   claimed topics are those of every such claim, the deepest directory
   first. Where none does, they are the topics of the claims with no
   directory.
3. From the claimed topics, `part_of` and `uses` are followed breadth
   first, each topic loaded once and an ended one not at all.
4. The machine topic loads in every session, with what it reaches the same
   way.

A loaded topic comes with its summary and the facts and ideas filed on it.
A part of it that is not loaded is shown as counts: what is filed on the
part and on everything that is part of it.

A session is shown as due the followups filed on a topic loaded from a
claim or on anything that is part of a claimed topic. Where no claimed
directory covers the session, what the machine topic reaches counts as
loaded from a claim too; in a claimed directory the machine topic and what
only it reaches bring their facts and not their open work. Among those, a
followup with a date is due from that day on, and one touching a topic is
due where that topic was loaded from a claim, which is a session in the
topic, in a part of it or in a topic that uses it.
