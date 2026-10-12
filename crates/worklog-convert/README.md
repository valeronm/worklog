# worklog-convert

Converts a store of the `worklog` crate, with its whole history, to the store
`worklog-next` keeps. It is run once per store, from a clone: one release
carries its source and no release builds it. After that release it is removed,
and with it the workspace member in the root `Cargo.toml`,
`worklog_next::wiring::write_version` with its tests, the test in
`crates/worklog-next/tests/layers.rs` that keeps that function out of the rest
of `worklog-next`, and the passages of the root `CLAUDE.md` that name this
crate.

## Running it

```
cargo run -p worklog-convert -- <OLD_STORE> [--decisions <FILE>] [--into <NEW_STORE>]
```

Without `--into` a run writes nothing. It prints a report: what is blocked,
what is open with the lines of a decision file that answer it, which decisions
are unused, and what the conversion will write that no version of the old store
holds. With nothing open it also prints the counts a run with `--into` would
write, and exits 0; otherwise it exits 1.

With `--into` and nothing open, the new store is written beside the target, at
`<NEW_STORE>.partial`, read back, held to the new store's own `check`, and
moved to the target in one rename. The target is absent or a directory holding
nothing. A failure leaves the target as it was.

The old store is only read. A target that is, holds or is inside the old store,
or a kind's directory the old store keeps through a symbolic link, is refused,
and so is one whose `.partial` directory would be.

The same old store and the same decision file give the same bytes on any
machine. A converted store is bound to a host with
`worklog-next init <its machine topic> --store <NEW_STORE>`.

## The decision file

One TOML file answers what the old store leaves unsaid. A dry run prints a stub
for every open question a new line answers, with `"?"` where the answer goes.
A topic cycle and an answer that names nothing have no stub: each is mended by
changing a line the file already holds, which the row names.

A `"?"` answers nothing, so its question stays open and the file can be filled
in sittings. An answer no question asked for is listed as unused, and so is a
`"?"` no open question asks for; a run with either exits 1, so a typo cannot
hide. These are read with no question behind them and are not unused
once read: `[conversion]`, which the file may hold before an answer needs it
and which is never listed; the `topics` list of an entry or a followup; an
item of `part_of` or `uses` that adds an edge the store does not hold; and
`ended_by` beside an `ended`.

The file's tags and topic names are compared without regard to ASCII case. An
answer is one line that is not blank, except where `""` is said to mean
something.

```toml
[conversion]            # the stamp of the versions the conversion writes itself
written = 2026-10-20T09:30:00+01:00   # later than every write time of the store, not quoted
machine = "desk"        # a machine name of the store

[tags]                  # one line per tag an entry or a followup carries
lantern = "lantern"     # its documents are filed under this topic
phone = ""              # the tag is dropped

[topics.atlas]          # a name the store holds no topic under: the topic is created
summary = "A book of maps"
part_of = []
uses = ["lantern"]

[topics.lantern]        # a topic of the store: every item of its `includes` is placed
part_of = []            # the item is a topic this one is part of
uses = ["desk"]         # the item is a topic this one uses
drop = ["phone"]        # the item is no edge; beside its place in `part_of` or `uses`,
                        # an edge to an ended topic that one more version removes

[topics.desk]           # a machine topic
unclaim = ["phone"]     # a claimed name that is no topic, or an ended topic whose
                        # claims on this machine end

[machines]              # a machine name on a version that no machine topic holds,
phone = "desk"          # or several do: the name of its topic

[documents."entry/2026-09/2026-09-04-lantern"]   # a document, by kind and newest slug
tags = { lantern = "atlas" }    # this document's answer for a tag, over `[tags]`
topics = ["lantern"]    # the whole list, in place of what its tags give; a fact's one topic
name = "lantern"        # where the new store refuses the name, or another document holds it
former_names = { Lantern_Atlas = "" }   # an old name the new store refuses: another, or "" to leave it out
summary = "One line"    # where the old one is missing or not one line
ended = "merged"        # the reason of a removed document, a word its kind has
ended_by = "entry/2026-09/2026-09-05-atlas"   # the document that replaced it

[documents."fact/lantern/desk"]
recheck_summary = "Look again"   # the summary of the followup its recheck becomes

[documents."followup/2026-09-04-phone"]
entry = ""              # where the entry it names is not there: a slug, or "" for none
touching = "atlas"      # where its recheck touches no live topic: a name, or "" for no trigger
```

## What blocks a conversion

| in the report | resolved |
| --- | --- |
| Blocked: a forked, waiting or unreadable document, a path no command reads, a rename cut short | in the old store, with the released `worklog`, as the table's advice says |
| Blocked: two topics whose names differ only in case, under a name no key of the file tells apart | in the old store with `worklog rename` where one of them is live under that name; else on the line of the file that holds the name, or in the converter, as the table's advice says |
| Blocked: a stored form the converter does not place | in the converter: the advice says so, and no answer mends it |
| Open | in the decision file, under the key the stub gives or, with no stub, on the line the row names |
| Unused decisions | in the decision file: correct the line, answer the `"?"` or remove it |
| a decision file that does not read | in the decision file: each line is listed with its key or its line number |
| a target that cannot take a store, or a `.partial` directory in the way | at the target: the refusal names the path and what to do |
| a plan that does not compose, does not account for the store, or is not sound once written | in the converter: it is its defect, and nothing was left at the target |

## What stays out of the repository

A decision file and a report hold store content: slugs, tags, topic and machine
names, summaries. Neither is committed. The tests build invented stores from the
words `lantern`, `atlas`, `phone` and `desk`.
