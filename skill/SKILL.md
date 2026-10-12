---
name: worklog
description: Record work done, durable facts, ideas, follow-ups and topics in the worklog store, and look them up. Use when asked to log work ("log this", "write an entry"), when a durable fact, a decision or an unbuilt idea is stated, when open work needs closing or rescheduling, when new work starts in a project that has a topic, or when a topic is raised without enough context to act on and the store may hold it. Every write goes through the `worklog` binary; nothing in the store is edited by hand.
---

# worklog

The store is a set of documents, each a chain of immutable versions, read
and written only through `worklog`. Five kinds:

- **topic** — a project, a device, a tool or a subject, known by its
  name: `lantern`. A machine has a topic too.
- **fact** — one thing that is true now, filed under one topic and known
  as `<topic>/<name>`: `lantern/relay-pin-is-fixed`.
- **idea** — a settled design not yet built, filed and known as a fact
  is: `lantern/dimmer`.
- **entry** — one piece of work as it was done, dated; a day holds as many
  as there were tasks. Known as `<date>-<name>`, `2026-09-04-lamp-driver`,
  and filed under the topics the work touched.
- **followup** — open work, filed under one or more topics. It has no
  name and is known by its id.

A command takes a document by its name as above, by a name it had before
a rename, or by the start of its id, which every listing prints.

A session opens with `worklog context`: what is due now, any forked
document among what it loads, how many drafts and followups are open, then
each topic the session loads: the topics claimed for its directory, the
machine's topic, and the topics those are part of or use. A topic comes
with its summary, its facts and ideas by name or, where the names would
not fit, by count, and its parts as counts of what is filed on them. A
long opener leaves topics out and says so; `worklog context --json`
has them all.

Read a fact before relying on it: `worklog show <topic>/<name>`. A
session handing work to a subagent gives it the facts that work rests on,
since a subagent opens with none.

## When to write

- **Capture**: the user asks to log work, or a substantive piece of work has
  just finished.
- **Record**: the user states something that outlives the conversation and
  no work happened: a fact, a decision, an unbuilt idea. Write or update the
  fact or the idea; no entry.
- **Recall**: "what did we do about X", "have we touched Y" — read and
  answer; write nothing.

Invoked as a command: a bare `/worklog` after work is capture, and
`/worklog <text>` is recall for the text unless it plainly asks to log
or record something.

## Writing anything

Prose goes through a draft. The records are the agent's own: write them
without putting the text to the user for approval, and answer for their
content afterwards through `show` and `history`. What the user decides is
what happens next, the triage under Open work; never the text of a record.

1. `worklog new topic <name>`, `worklog new fact <topic>/<name>`,
   `worklog new idea <topic>/<name>`, `worklog new entry <name>`,
   or `worklog checkout <target>` for an existing document. Each
   prints the draft's path. A name is lowercase letters, digits and
   hyphens, with at least one letter and no date at its start.
2. Read the file at that path, then edit it: it lives outside the working
   directory, and an edit tool refuses a file it has not read. The draft
   is the document's fields between two `+++` lines, then its body. The
   fields are TOML: text in quotes, a date bare as `2026-11-01`, a list as
   `["lantern", "atlas"]`. A field that holds another document holds its
   name. `summary` is one line and never empty. A field the kind does not
   have is refused. A fact's draft holds `topic` and `confirmed`, and an
   idea's `idea = true` besides. A followup's holds `topics`, its trigger
   as `look_again` with `why` or as `touching`, and `entry` and `about`,
   which never change once stored.
3. `worklog diff <target>` shows the draft against what it started
   from; read it once to check the edit landed as meant.
4. `worklog save <target>` checks the draft and stores it as the
   document's next version. A refusal says what is wrong; the draft stays
   for another edit. `worklog discard <target>` deletes the draft.

A draft is named as its document is; a new entry's is `<date>-<name>`. If
`save` says the document moved on since the draft was checked out, another
session or machine wrote in between: read the draft, or `diff` it, for the
edits made, then `discard`, `checkout` again, make the edits again, `save`.

State changes are commands and need no draft. Each is refused while a
draft of its document is open.

- `worklog end <target> <reason> [note]` ends a document for a
  reason its kind has: a topic `retired` or `merged`; a fact `false`,
  `moved` or `superseded`; an idea `built` or `abandoned`; an entry
  `removed` or `merged`; a followup `done` or `dropped`. Only `done` goes
  without a note, which says why and where the matter went. `--by
  <document>` records what ended it: the entry that built an idea, the
  fact that superseded another. A topic does not end while a fact, an
  idea, a followup, a claim or another topic still names it.
  An ended document still reads with `show`, is left out of listings
  unless `--ended` is given, and takes no change until `worklog
  reopen <target> <why>` brings it back.
- `worklog done <followup> [note]` and `worklog drop <followup>
  <note>` end a followup; a `drop` note says why the work fell away.
  `worklog trigger`, under Open work, sets when one comes up again.
- `worklog verify <fact>` records that a fact was checked today and
  still holds. It is refused on a day the fact was already confirmed.
- `worklog rename <target> <name>` gives a topic, a fact, an idea or
  an entry a new name, written bare with no topic or date before it. The
  old name keeps working in commands and links.
- `worklog move <from> <to> <targets…>` files facts, ideas, entries
  and followups under the topic `<to>` where they were under `<from>`,
  all of them or none.

`--json` on any command returns the same data structured. Refusals go to
stderr with exit 1; a usage error exits 2.

## Capture — an entry for work done

1. `worklog new entry <name>`, with `--date <YYYY-MM-DD>` for work
   of another day; edit the draft. `date` and `machine` are filled. Set
   `topics` to the topics the work touched, `topics = ["lantern"]`, which
   a listing or a search by topic finds the entry by, and `summary`,
   which every listing shows. Body sections: **What / Why / Changes /
   Notes**. Record what was non-obvious: decisions, gotchas, why not the
   other way. Never git state.
2. `diff`, then `save`, each with the entry's `<date>-<name>`.
3. Put each doable item of open work from this session to the user, one
   question per item naming the item and what doing it involves, with
   three options: **do it now**, carried out in this session so it is never
   recorded; **record it**; **drop it**, and if the drop was a decision its
   why goes in the entry's Notes. Never withhold an item because it seemed
   not worth asking, and never ask the same item twice. A silently recorded
   item is a delay decision the user never made. A recorded item goes where
   the triage under Open work puts it: a followup, one each, `worklog
   new followup --topics <topic> --summary "<what is open>" --entry
   <date>-<name> --look-again <date> --why "<what to look for>"`, or
   `--touching <topic>` in place of the date and its why; or an idea,
   `worklog new idea <topic>/<name>`, which the entry links.
4. Reconcile: `worklog followups <topic>` for the topics this work
   touched; `done` what it completed with a note naming what dissolved it,
   `trigger` what it moved, `drop` what it made moot.
5. Promote what outlives the entry into a fact, and link it from the
   entry. A note or a link added to the entry after its save is
   `worklog checkout <date>-<name>` and a save again.

## Open work

`worklog followups [<topic>]` lists open items, due ones first, each
with its state, `due <date>`, `by <date>`, `touching <topic>` or `no
trigger`, then its id and summary. A topic covers its parts; an entry in
place of the topic lists the followups that arose in it.

`worklog due [<dir>]` lists what is due for a session in the working
directory, or in the one given: the lines `context` opens with. It covers
the followups filed on a topic claimed for the session's directory, on
anything part of one, and on what the claimed topics are part of or use.
Where no claimed directory covers the session, the topics claimed
`--anywhere` count as the claimed ones, and it covers those filed on the
machine's topics too. Everything due is triaged in that session: done,
dropped, rescheduled, or acted on.

Triage an item, new or found in a backlog, by the first question that
fits:

1. Can it be done now? Then do it, or put it to the user.
2. Is it already answered? Then `done` it, naming what dissolved it.
3. Does an idea already hold the work? Then `drop` it and let the entry's
   Notes link the idea.
4. Is something waiting on it? Then it is a followup with its trigger.
5. Otherwise it is an idea.

A followup's trigger says when it comes up again:

- `worklog trigger <followup> <date> <why>` — a day to look again,
  not the day the thing is expected, and what to look for then.
- `worklog trigger <followup> touching <topic>` — one of its own
  topics; it is due in a session whose directory claims that topic, or a
  topic that is part of it or uses it, through any number of steps. A
  session that only claims what that topic is part of does not bring it
  up. In a session outside every claimed directory, the machine topic and
  the topics claimed `--anywhere` count as claimed there.
- `worklog trigger <followup> none` clears the trigger. A followup
  with none is never due and is found only by listing.

`new followup` takes several `--topics` separated by commas, and `--about
<document>` for the document the followup is about. With `--summary` it
is stored at once and its id printed; without, a draft opens, saved by
the id `worklog drafts` shows.

## Facts and topics

A fact has a `summary` that is the fact itself, then a body: the fact as
it holds now, **Why**, and **How to apply**. It is rewritten (checkout,
save) when it changes and ended when it stops being true: `false` when it
was wrong, `moved` when another store now holds it, `superseded` when
another fact replaced it. `confirmed` is the last day it was checked:
`verify` sets it, and so does a save that changes the summary or the body.

Which store holds it:

- A repo's own docs, its README and its agent instructions, hold what a
  stranger cloning it needs. A fact that turns out to be the repo's
  business moves there and is ended here as `moved`, never copied: two
  stores holding one claim is how one goes stale.
- A fact holds what is true of its subject wherever a session runs and is
  not the repo's business: a device quirk and the setting that works
  around it, a decision and the reasoning that would otherwise be
  re-argued, and what the code and its history cannot give back, a
  measurement or what an experiment ruled out.
- An entry holds what is worth having because it happened on a date. Work
  about nothing durable, a form filed, a question answered once, is an
  entry and stops there.

Writing one well:

- File a fact under the narrowest topic it is true of. A topic is a
  subject, never an activity: `testing` says what the work was, and its
  facts belong to whatever it was done to.
- Verify before distilling. Turning an older record into a fact means
  checking every claim against the code first; a stale source can assert
  the opposite of the evidence it cites.
- Split a fact on lifespan, not on the document it came from. An intention
  ends when it is built; the alternatives it rejected stay true.
- Strip implementation detail from an idea, since it names code that gets
  renamed; keep it exactly for a device, where the firmware file and the
  config symbol are the fact.
- Work that invalidates a fact fixes the fact in the same session, because
  the next session reads the fact, not the entry. A fact that has become
  an account of how something used to work is ended and, if worth
  keeping, an entry.

A topic's document carries the lists `part_of` and `uses`, a free body for
what to know before touching the subject at all, and a `summary`, which is
all a session in another directory sees of the topic: write it from the
subject itself, its README or what the device is, not from its facts. A
new topic's draft shows neither list; add the line.

- `part_of = ["lantern"]` makes the topic a part of `lantern`. A read of
  `lantern` covers its parts: `worklog facts lantern` returns what is
  filed on `lantern` and on everything part of it, and `ideas`, `entries`,
  `followups` and `search --topic` do the same. A session in the part
  loads `lantern` too; a session in `lantern` sees the part as counts,
  and what is due under it by name.
- `uses = ["atlas"]` makes a session in the topic load `atlas` too, and
  nothing more: a read of `atlas` does not cover the topic. A fact that
  holds for everything built on `atlas` goes under `atlas`, so each topic
  that uses it loads the fact and none carries a copy.

`worklog topics` lists every topic with its summary, what it is part
of and uses, and how much is filed on it; `worklog topics <topic>`
lists the topics directly part of one.

## Machines and claims

A host is set up once, by the user: `worklog init <name> --store
<dir>` binds it to the machine topic of that name, and with `--summary
<text>` makes that topic when the store has none. Until then every command
that uses the store is refused, and `context` prints only a note saying so
and exits 0. The machine topic loads in every session on that host; facts
about the machine go under it.

A claim places a topic in a directory of this machine: `worklog claim
<topic>` from inside the checkout, or `worklog claim <topic> <dir>`.
A claim covers the directory and everything under it: claim the narrowest
directory the topic's facts hold for, and a parent once when it holds
several checkouts of one topic. Several topics may claim one directory,
and all of them load there. `worklog claim <topic> --anywhere` places
a topic in no directory: it loads in a session no claimed directory
covers.

A checkout is unclaimed when `context` shows no topic as `claimed at` a
directory: `worklog new topic <name>`, with a summary written from
the repo and `part_of` or `uses` for the topics it belongs with, then
`worklog claim <name>` inside it.

`worklog where` lists this machine's claims, each with its topic, its
directory and its id; `worklog where <topic>` shows one topic's, and
`--machine <name>` another machine's. `worklog unclaim <topic>
[<dir>]`, or with `--anywhere`, ends a claim; after a checkout moves,
unclaim the old directory by its path and claim the new one.

## Search and recall

Before new work in a topic, find what the work touches and what was
already decided about it:

1. When the work names what is about to change, a component, a file, an
   identifier, `worklog search <term> --topic <topic>` returns the
   facts and ideas, then the topics, entries and followups, of that topic
   and its parts whose name, summary or body holds the term. The term is
   matched as written, whatever its case, so try the other words the
   thing goes by before reading an empty result as nothing decided.
2. When the work is open-ended, `worklog facts <topic>` and
   `worklog ideas <topic>` list everything in the topic, and
   `worklog entries <topic>` its entries, newest first.
3. `show` the facts and entries whose summary or matching lines bear on
   the work, for the decision taken and what was rejected.
4. `worklog followups <topic>` for open work on what is about to
   change.

Tell the user what was found that bears on the work, citing names.

- `search`, `facts`, `ideas`, `entries`, `followups` and `topics` with no
  topic cover the whole store, each with `--ended` for the ended ones too;
  `search` takes `--regex` to read the term as a pattern, and `entries`
  `-n <count>` for at most that many.
- `worklog log [n]` — the newest versions written anywhere in the
  store, twenty unless told, so what other machines wrote since is one
  call; `--machine <name>` for one machine's writes.
- `worklog show <target>` prints a document as it stands and
  `worklog history <target>` its versions, each with an id. Given
  the start of such an id, `worklog show <id>` prints that version,
  `worklog diff <id>` what it changed, and `worklog diff <id>
  <id>` what changed between two.

Answer a "what did we do about X" question by reading the matching
documents and reconstructing what changed, when and why, citing names. If
nothing matches, say so. Where a fact and an entry disagree, the fact is
current and the entry is what was true that day; report the disagreement,
since one of them is stale.

## Links and what the store keeps

Cross-references are `[[name]]`: `[[lantern]]`,
`[[lantern/relay-pin-is-fixed]]`, `[[2026-09-04-lamp-driver]]`, or the
start of an id for a followup. A link is resolved when its document is
saved and follows its target through a rename; one written before its
target exists resolves once the target is stored. Link to say where
something was decided, not instead of saying it here. A link inside a code
span is quoted, not made. Never delete a link because it failed to
resolve: the target may be alive under another name or on a machine that
has not synced.

`worklog check` prints each problem, then each notice with `notice:`
before it, then the counts of problems, notices and forks, and exits 1 on
a problem. A problem is what a write would have refused: a field naming a
document that is missing or of another kind, a topic, a fact, a followup
or a claim naming an ended topic, a name two documents hold, a topic made
part of itself, a file that does not read. A notice never changes the exit
code: a link from a topic, a fact or a followup that leads nowhere or to
an ended document, which means the linking document has gone stale; a
version a newer binary wrote; documents of a kind this binary does not
know; a claim whose directory is under the home directory and not stored
that way. An entry's links are citations and are not checked.

A fork is a document with two or more current versions, written on
different machines before they synced. `worklog forks` lists each
with its versions, and `show` prints them all. `worklog resolve
<target>` opens a draft over every one of them, holding the text of one;
`worklog diff <target>` shows that draft against each in turn, with
`no changes` under one it equals. Edit the draft into what should stand
and `save`; where the versions disagree on substance, put the choice to
the user. A forked document takes no other change until resolved. Neither
does one holding a version this binary cannot read: report it and leave
it.

`worklog drafts` lists the drafts open on this host, this session's
and any other's: a draft is the session's that opened it. Save or discard
only a draft this session opened. Report the rest and leave them; one
that is plainly abandoned is put to the user, who says whether it is
finished or dropped.
