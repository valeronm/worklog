---
name: worklog
description: Record work done, durable facts, follow-ups and topics in the worklog store, and look them up. Use when asked to log work ("log this", "/worklog", "write an entry"), when a durable fact, a decision or an unbuilt idea is stated, when open work needs closing or rescheduling, or when a topic is raised without enough context to act on and the store may hold it. Every write goes through the `worklog` binary; nothing in the store is edited by hand.
---

# worklog

The store is a set of documents, each a chain of immutable versions, read
and written only through `worklog`. Four kinds:

- **entry** — one piece of work as it was done, dated; a day holds as many
  as there were tasks; `2026-09/2026-09-04-lamp-driver`.
- **fact** — what is true now, under a topic; `lantern/relay-pin-is-fixed`.
  A fact with `idea: true` is an idea: a settled design not yet built.
- **followup** — open work that arose in an entry; `2026-09-04-port`.
- **topic** — what a subject is and which topics a session about it also
  needs; `lantern`. A machine's topic says where topics live on that host.

A session opens with `worklog context`: the topics its directory and
machine reach, their facts by name, what is due, and what needs a hand.
Read a fact before relying on it: `worklog show <topic>/<name>`. Read a
topic's facts before starting work in it: `worklog facts <topic>`.

## When to write

- **Capture**: the user asks to log work, or a substantive piece of work has
  just finished. After such work, offer to log it; do not write an entry
  unprompted.
- **Record**: the user states something that outlives the conversation and
  no work happened: a fact, a decision, an unbuilt idea. Write or update the
  fact; no entry.
- **Recall**: "what did we do about X", "have we touched Y" — read and
  answer; write nothing.

Invoked as a command: a bare `/worklog` after work is capture, and
`/worklog <text>` is recall for the text unless it plainly asks to log or
record something.

## Writing anything

Prose goes through a draft; the store is never edited directly.

1. `worklog new entry <name>`, `worklog new fact <topic>/<name>`, `worklog
   new idea <topic>/<name>`, `worklog new topic <name>`, or `worklog
   checkout <slug>` for an existing document. Each prints the draft path.
2. Edit the file at that path. Change only the kind's own fields and the
   body; the `version` block is the tool's.
3. `worklog diff <slug>` shows the draft against the store. Show it to the
   user before saving anything they have not seen.
4. `worklog save <slug>` validates, stamps and stores it. A refusal names
   what is wrong; the draft stays for another edit. `worklog discard
   <slug>` drops it.

If `save` says the document moved on since checkout, another machine wrote
in between: `discard`, `checkout` again, carry the edits over, `save`.

State changes are commands and need no draft: `worklog done <followup>
[note]`, `worklog drop <followup> [note]`, `worklog recheck <slug> <date>
<why>` or `worklog recheck <slug> touching <topic>`, `worklog verify
<fact>`, `worklog tombstone <slug>`, `worklog rename <slug> <new>`.

`--json` on any command returns the same data structured. Refusals go to
stderr with exit 1; a usage error exits 2.

## Capture — an entry for work done

1. `worklog new entry <kebab-name>`; edit the draft. `date` and `machine`
   are filled. Set `tags` from the vocabulary `worklog tags` shows, reusing
   before inventing, and `summary` as one line, which is what every listing
   shows. Body sections: **What / Why / Changes / Notes**. Record what was
   non-obvious: decisions, gotchas, why not the other way. Never git state.
2. `worklog diff` then `worklog save`.
3. Open work from this session becomes followups, one each:
   `worklog new followup <name> --entry <entry-slug> --summary "…"
   --recheck "<date> <why>"` or `--recheck "touching <topic>"`. Put each
   doable item to the user first, one AskUserQuestion per item naming the
   item and what doing it involves, with three options: **do it now**,
   carried out in this session so it never becomes a followup; **record as
   followup**; **drop it**, and if the drop was a decision its why goes in
   the entry's Notes. Never withhold an item because it seemed not worth
   asking, and never ask the same item twice. A silently written followup
   is a delay decision the user never made.
4. Reconcile: `worklog followups <topic>` for the topics this work touched;
   `done` what it completed with a note naming what dissolved it, `recheck`
   what it moved, `drop` what it made moot.
5. Promote what outlives the entry into a fact, and link it from the entry.

## Open work

`worklog followups [topic]` lists open items oldest first with each
item's state: `due <date>`, `by <date>`, `touching <topic>`, or `no
recheck`; given an entry slug instead of a topic, it lists the items that
arose in that entry. Everything `context` shows as due is triaged in that session:
done, dropped, rescheduled, or acted on.

Triage an item, new or found in a backlog, in this order: can it be done
now, then it is not a followup, do it or put it to the user; is it already
answered, then `done` it with a note naming what dissolved it; does an idea
already hold the work, then `drop` it and let the entry's Notes link the
idea; is something waiting on it, then it is a followup with its recheck;
otherwise it is an idea.

A followup carries a recheck: a date and why, meaning when to look again,
not when the thing is expected; or `touching <topic>`, raised by every
session opening in that topic. An idea, listed apart under `worklog ideas
[topic]`, gains a recheck through `worklog recheck` when something starts
waiting on it.

## Facts and topics

A fact is one claim, under the topic it is about, with a `summary` that is
the fact itself, then a body: the fact as it holds now, **Why**, and **How
to apply**. It is rewritten (checkout, save) when it changes and
tombstoned when it stops being true. `worklog verify <fact>` records that
it was checked and found still true.

Which store holds a claim:

- A repo's own `CLAUDE.md` holds what a stranger cloning it needs. A fact
  that turns out to be the repo's business moves there and is tombstoned
  here, never copied: two stores holding one claim is how one goes stale.
- A fact holds what is true of its subject wherever a session runs and is
  not the repo's business: which checkout answers a question, a device
  quirk and the setting that works around it, a decision and the reasoning
  that would otherwise be re-argued, a correction that generalises.
- An entry holds what is worth having because it happened on a date. Work
  about nothing durable, a form filed, a machine reimaged, a question
  answered once, is an entry and stops there.

Writing one well:

- File a fact under the narrowest topic it is true of. A quirk of one app
  goes under that app; a fact that holds for every Android app goes under
  `android`, which the apps include, so each of them loads it and no app
  carries a copy. A topic is a subject, never an activity: `testing` or
  `investigation` says what the work was, and its facts belong to whatever
  it was done to.
- Verify before distilling. Turning an older record into a fact means
  checking every claim against the code first; a stale source can assert
  the opposite of the evidence it cites.
- Split a fact on lifespan, not on the document it came from. An intention
  dies when it is built; the alternatives it rejected stay true. Two claims
  that die at different times are two facts.
- Detail rots in a plan and is the fact on a machine. Strip implementation
  detail from an intention, since it names code that gets renamed; keep it
  exactly for a device, where the firmware file and the config symbol are
  the fact.
- Work that invalidates a fact fixes the fact in the same session; the
  entry recording the work is no substitute, because the next session reads
  the fact. A fact that has become a story about how something used to work
  is tombstoned and, if worth keeping, an entry.

A topic's document carries a `summary` other sessions see, `includes`, the
topics loaded along with it, and a free body for what to know before
touching the subject at all. A machine topic carries `machine: <name>`,
`claims` mapping topics to that host's directories, and `unclaimed` for
topics loaded only in unclaimed directories.
`worklog topics` lists them; `worklog where <topic>` shows a topic's
directories on this machine, and `worklog where` alone every claim here,
marking a directory this host lacks.

Write a topic's summary from the subject itself, its README or what the
device is, not from its facts: facts cluster in whatever corner needed
writing down, and the summary is all a session in another directory ever
sees of the topic, so it decides whether the topic is opened at all.

## Setting up a project

A checkout `context` reaches nothing in is unclaimed. Two commands make it
a project:

1. `worklog new topic <name>`; give the draft a `summary` written from the
   repo, and `includes` for the topics it shares; `worklog save <name>`.
2. `worklog claim <name>` from inside the checkout, or with the directory
   as a second argument. A claim covers the directory and everything
   under it, so a parent holding several checkouts of one topic is claimed
   once. `worklog unclaim <name> [<dir>]` takes it back.

## Search and recall

- `worklog search <term>` — every document holding the term, facts first;
  `--regex` for a pattern.
- `worklog tag <tag>`, `worklog recent [n]`, `worklog list --kind <kind>`.
- `worklog show <slug>` prints a document as it stands; `worklog history
  <slug>` its versions.

Answer a "what did we do about X" question by reading the matching
documents and reconstructing what changed, when and why, citing slugs. If
nothing matches, say so. A fact answers where an entry only explains; where
the two disagree, the fact is current and the entry is what was true that
day, and the disagreement is worth reporting, since one of them is stale.

## Links and rules the store keeps

Cross-references are `[[slug]]` and resolve to the current version:
`[[lantern/relay-pin-is-fixed]]`, `[[2026-09/2026-09-04-lamp-driver]]`. Link
to say where something was decided, not instead of saying it here; an
entry that rests on a fact links it, and facts link each other the same
way. A link inside a code span is quoted, not made. Never delete a link
because it failed to resolve: the target may be alive under another name
or on a machine that has not synced.

`worklog check` verifies every link, recheck and reference across the store
and exits 1 on a problem; run it after a rename or tombstone.

A document with two current versions is a fork, made by two machines
writing from the same parent before syncing. `worklog forks` lists them,
every read shows both heads, and `worklog resolve <slug>` opens a draft
holding both for a person to reconcile and save. Nothing merges on its own.

`worklog drafts` lists drafts left open on this machine; a stale one is
finished or discarded, never left to surprise the next session.
