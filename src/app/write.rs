//! The write use cases. Every one ends in a new version or a refusal;
//! nothing here edits a file the store already holds.

use crate::domain::draft::Draft;
use crate::domain::entry::Entry;
use crate::domain::fact::Fact;
use crate::domain::followup::{Followup, FollowupState, NotOpen};
use crate::domain::frontmatter::Fields;
use crate::domain::graph;
use crate::domain::kind_keys;
use crate::domain::recheck::Recheck;
use crate::domain::slug::{Kind, Slug};
use crate::domain::topic::{ClaimError, Topic};
use crate::domain::version::{
    Operation, State, Version, VersionBlock, VersionId, note_body, with_note,
};

use super::load;
use super::output::{DraftList, DraftRef, Written};
use super::{Deps, Failure, machine, slug_arg};

const ENTRY_BODY: &str = "\n## What\n\n## Why\n\n## Changes\n\n## Notes\n";
const FACT_BODY: &str = "\n\n**Why:** \n\n**How to apply:** \n";
const TOPIC_BODY: &str = "\n";

fn written(version: &Version) -> Written {
    Written {
        slug: version.slug.path().to_owned(),
        id: version.id.to_string(),
        tombstone: None,
    }
}

fn draft_ref(deps: &Deps, draft: &Draft) -> DraftRef {
    DraftRef {
        slug: draft.slug.path().to_owned(),
        kind: draft.slug.kind().dir().to_owned(),
        location: deps.drafts.location(&draft.slug),
        parents: draft.parents.iter().map(ToString::to_string).collect(),
    }
}

/// Stamps and stores a version; the caller has checked the parents.
#[allow(
    clippy::too_many_arguments,
    reason = "the parts of a version the caller decides; the rest are stamped here"
)]
pub fn store_version(
    deps: &Deps,
    slug: Slug,
    parents: Vec<VersionId>,
    operation: Operation,
    fields: Fields,
    body: String,
    superseded_by: Option<Slug>,
    renamed_from: Option<Slug>,
) -> Result<Version, Failure> {
    let block = VersionBlock {
        parents,
        written: deps.clock.now(),
        machine: machine(deps)?,
        operation,
        superseded_by,
        renamed_from,
        raw: None,
    };
    let version = Version::compose(slug, block, fields, body);
    deps.store.put(&version)?;
    Ok(version)
}

/// A document's first version.
pub fn first_version(
    deps: &Deps,
    slug: Slug,
    operation: Operation,
    fields: Fields,
    body: String,
) -> Result<Version, Failure> {
    store_version(deps, slug, vec![], operation, fields, body, None, None)
}

/// A version following `parent`, for the commands that change one thing.
fn amend(
    deps: &Deps,
    parent: &Version,
    operation: Operation,
    fields: Fields,
    body: String,
) -> Result<Written, Failure> {
    load::refuse_foreign(parent)?;
    let version = store_version(
        deps,
        parent.slug.clone(),
        vec![parent.id.clone()],
        operation,
        fields,
        body,
        None,
        None,
    )?;
    Ok(written(&version))
}

/// The fields a kind accepts, or the refusal naming what it does not.
fn validate(deps: &Deps, slug: &Slug, fields: &Fields) -> Result<(), Failure> {
    fields
        .reject_unknown(kind_keys(slug.kind()))
        .map_err(|e| Failure::at(slug, e))?;
    match slug.kind() {
        Kind::Entry => {
            let entry = Entry::from_fields(fields).map_err(|e| Failure::at(slug, e))?;
            if Some(entry.date.as_str()) != slug.date() {
                return Err(Failure::at(
                    slug,
                    format!("date {} is not the slug's", entry.date),
                ));
            }
        }
        Kind::Fact => {
            Fact::from_fields(fields).map_err(|e| Failure::at(slug, e))?;
            let topic = Slug::of_kind(Kind::Topic, slug.topic().unwrap_or_default())
                .map_err(|e| Failure::at(slug, e))?;
            load::live(deps.store, &topic)
                .map_err(|_| Failure::at(slug, format!("no topic: {topic}")))?;
        }
        Kind::Topic => {
            Topic::from_fields(fields).map_err(|e| Failure::at(slug, e))?;
        }
        Kind::Followup => {
            let followup = Followup::from_fields(fields).map_err(|e| Failure::at(slug, e))?;
            load::live(deps.store, &followup.entry).map_err(|_| {
                Failure::at(slug, format!("arose in no live entry: {}", followup.entry))
            })?;
        }
    }
    Ok(())
}

fn open_draft(deps: &Deps, draft: &Draft) -> Result<DraftRef, Failure> {
    if deps.drafts.read(&draft.slug)?.is_some() {
        return Err(Failure::Refused(format!(
            "a draft of {} already exists at {}",
            draft.slug,
            deps.drafts.location(&draft.slug)
        )));
    }
    deps.drafts.write(draft)?;
    Ok(draft_ref(deps, draft))
}

/// A first draft of a document no version exists for.
fn open_new(deps: &Deps, slug: Slug, fields: Fields, body: &str) -> Result<DraftRef, Failure> {
    refuse_existing(deps, &slug)?;
    machine(deps)?;
    open_draft(
        deps,
        &Draft {
            slug,
            parents: vec![],
            fields,
            body: body.to_owned(),
        },
    )
}

fn refuse_existing(deps: &Deps, slug: &Slug) -> Result<(), Failure> {
    let document = deps.store.document(slug)?;
    match document.state() {
        State::Absent => Ok(()),
        State::Live(_) | State::Forked(_) => Err(Failure::Refused(format!(
            "{slug} exists: `worklog checkout {slug}` to change it"
        ))),
        State::Tombstoned(_) => Err(Failure::Refused(format!(
            "{slug} was removed; a slug is never reused"
        ))),
    }
}

/// A draft for a new entry dated today, or on the date given.
pub fn new_entry(deps: &Deps, name: &str, date: Option<&str>) -> Result<DraftRef, Failure> {
    let date = date.map_or_else(|| deps.clock.today(), str::to_owned);
    let slug = Slug::entry(&date, name)?;
    let entry = Entry {
        date,
        machine: machine(deps)?,
        tags: vec![],
        files_touched: vec![],
        summary: String::new(),
    };
    open_new(deps, slug, entry.to_fields(), ENTRY_BODY)
}

pub fn new_fact(deps: &Deps, slug: &str, idea: bool) -> Result<DraftRef, Failure> {
    let slug = slug_arg(slug, Some(Kind::Fact))?;
    let fact = Fact {
        tags: vec![slug.topic().unwrap_or_default().to_owned()],
        idea,
        recheck: None,
        verified: None,
        summary: String::new(),
    };
    open_new(deps, slug, fact.to_fields(), FACT_BODY)
}

pub fn new_topic(deps: &Deps, name: &str) -> Result<DraftRef, Failure> {
    let slug = slug_arg(name, Some(Kind::Topic))?;
    open_new(deps, slug, Topic::default().to_fields(), TOPIC_BODY)
}

/// What a new followup carries besides its name and entry.
pub struct NewFollowup<'a> {
    pub summary: Option<&'a str>,
    pub recheck: Option<&'a str>,
    pub tags: Option<&'a [String]>,
}

/// How a new followup came out: stored at once, or opened for prose.
pub enum Made {
    Written(Written),
    Draft(DraftRef),
}

/// Written at once when a summary is given, otherwise opened as a draft.
pub fn new_followup(
    deps: &Deps,
    name: &str,
    entry: &str,
    what: &NewFollowup,
) -> Result<Made, Failure> {
    let entry = slug_arg(entry, Some(Kind::Entry))?;
    let entry_version = load::live(deps.store, &entry)?;
    let entry_data =
        Entry::from_fields(&entry_version.fields).map_err(|e| Failure::at(&entry, e))?;
    let slug = Slug::followup(&deps.clock.today(), name)?;
    refuse_existing(deps, &slug)?;
    let followup = Followup {
        entry,
        tags: what.tags.map_or(entry_data.tags, <[String]>::to_vec),
        recheck: what.recheck.map(Recheck::parse).transpose()?,
        state: FollowupState::Open,
        summary: what.summary.unwrap_or_default().to_owned(),
    };
    if what.summary.is_some() {
        let version = first_version(
            deps,
            slug,
            Operation::New,
            followup.to_fields(),
            "\n".to_owned(),
        )?;
        return Ok(Made::Written(written(&version)));
    }
    Ok(Made::Draft(open_draft(
        deps,
        &Draft {
            slug,
            parents: vec![],
            fields: followup.to_fields(),
            body: "\n".to_owned(),
        },
    )?))
}

/// The current version as a draft, for editing.
pub fn checkout(deps: &Deps, slug: &Slug) -> Result<DraftRef, Failure> {
    let version = load::live_to_amend(deps.store, slug)?;
    open_draft(
        deps,
        &Draft {
            slug: slug.clone(),
            parents: vec![version.id],
            fields: version.fields,
            body: version.body,
        },
    )
}

/// A draft holding every head of a fork, for a person to reconcile.
pub fn resolve(deps: &Deps, slug: &Slug) -> Result<DraftRef, Failure> {
    let document = deps.store.document(slug)?;
    let State::Forked(heads) = document.state() else {
        return Err(Failure::Refused(format!("{slug} is not forked")));
    };
    let heads: Vec<Version> = heads.into_iter().cloned().collect();
    heads.iter().try_for_each(load::refuse_foreign)?;
    open_draft(deps, &Draft::merging(&heads))
}

/// Validates, stamps, hashes and stores the draft, then deletes it.
pub fn save(deps: &Deps, slug: &Slug, dry_run: bool) -> Result<Written, Failure> {
    let draft = load::draft(deps, slug)?;
    validate(deps, slug, &draft.fields)?;
    if draft.has_conflict_markers() {
        return Err(Failure::at(slug, "conflict markers remain in the draft"));
    }
    let document = deps.store.document(slug)?;
    let heads: Vec<&Version> = match document.state() {
        State::Absent => vec![],
        State::Live(v) | State::Tombstoned(v) => vec![v],
        State::Forked(heads) => heads,
    };
    heads.iter().try_for_each(|h| load::refuse_foreign(h))?;
    let operation = match (draft.parents.len(), heads.len()) {
        (0, 0) => Operation::New,
        (0, _) => return Err(Failure::at(slug, "exists; the draft names no parent")),
        (1, 1) if heads[0].id == draft.parents[0] => Operation::Save,
        (n, m) if n == m && n > 1 && heads.iter().all(|h| draft.parents.contains(&h.id)) => {
            Operation::Resolve
        }
        _ => {
            let current: Vec<&str> = heads.iter().map(|h| h.id.short()).collect();
            return Err(Failure::at(
                slug,
                format!(
                    "moved on since the draft was checked out; current: {} — check out again and carry the edits over",
                    current.join(", ")
                ),
            ));
        }
    };
    if let [parent] = heads.as_slice()
        && operation == Operation::Save
        && parent.fields == draft.fields
        && parent.body == draft.body
    {
        return Err(Failure::at(
            slug,
            "the draft equals its parent; `worklog verify` records a check without a change",
        ));
    }
    if dry_run {
        return Ok(Written {
            slug: slug.path().to_owned(),
            id: String::new(),
            tombstone: None,
        });
    }
    let version = store_version(
        deps,
        slug.clone(),
        draft.parents,
        operation,
        draft.fields,
        draft.body,
        None,
        None,
    )?;
    deps.drafts.delete(slug)?;
    Ok(written(&version))
}

pub fn drafts(deps: &Deps) -> Result<DraftList, Failure> {
    Ok(DraftList {
        drafts: deps
            .drafts
            .list()?
            .iter()
            .map(|d| draft_ref(deps, d))
            .collect(),
    })
}

pub fn discard(deps: &Deps, slug: &Slug) -> Result<(), Failure> {
    load::draft(deps, slug)?;
    Ok(deps.drafts.delete(slug)?)
}

fn followup_of(deps: &Deps, slug: &Slug) -> Result<(Version, Followup), Failure> {
    if slug.kind() != Kind::Followup {
        return Err(Failure::Usage(format!("{slug} is not a followup")));
    }
    let version = load::live(deps.store, slug)?;
    let followup = Followup::from_fields(&version.fields).map_err(|e| Failure::at(slug, e))?;
    Ok((version, followup))
}

fn fact_of(deps: &Deps, slug: &Slug) -> Result<(Version, Fact), Failure> {
    if slug.kind() != Kind::Fact {
        return Err(Failure::Usage(format!("{slug} is not a fact")));
    }
    let version = load::live(deps.store, slug)?;
    let fact = Fact::from_fields(&version.fields).map_err(|e| Failure::at(slug, e))?;
    Ok((version, fact))
}

fn close(
    deps: &Deps,
    slug: &Slug,
    transition: fn(&Followup) -> Result<Followup, NotOpen>,
    operation: Operation,
    note: Option<&str>,
) -> Result<Written, Failure> {
    let (version, followup) = followup_of(deps, slug)?;
    let closed = transition(&followup).map_err(|e| Failure::at(slug, e))?;
    let body = with_note(version.body.clone(), note);
    amend(deps, &version, operation, closed.to_fields(), body)
}

pub fn done(deps: &Deps, slug: &Slug, note: Option<&str>) -> Result<Written, Failure> {
    close(deps, slug, Followup::done, Operation::Done, note)
}

pub fn drop_(deps: &Deps, slug: &Slug, note: Option<&str>) -> Result<Written, Failure> {
    close(deps, slug, Followup::dropped, Operation::Drop, note)
}

/// Moves the recheck of an open followup, or of a fact or idea.
pub fn recheck(deps: &Deps, slug: &Slug, recheck: &str) -> Result<Written, Failure> {
    let recheck = Recheck::parse(recheck)?;
    let (version, fields) = match slug.kind() {
        Kind::Followup => {
            let (version, followup) = followup_of(deps, slug)?;
            let moved = followup
                .rescheduled(recheck)
                .map_err(|e| Failure::at(slug, e))?;
            (version, moved.to_fields())
        }
        Kind::Fact => {
            let (version, fact) = fact_of(deps, slug)?;
            let moved = Fact {
                recheck: Some(recheck),
                ..fact
            };
            (version, moved.to_fields())
        }
        _ => return Err(Failure::Usage(format!("{slug} carries no recheck"))),
    };
    let body = version.body.clone();
    amend(deps, &version, Operation::Recheck, fields, body)
}

/// Records that a fact was confirmed today, changing nothing else.
pub fn verify(deps: &Deps, slug: &Slug) -> Result<Written, Failure> {
    let (version, fact) = fact_of(deps, slug)?;
    let fields = fact.verified_on(&deps.clock.today()).to_fields();
    let body = version.body.clone();
    amend(deps, &version, Operation::Verify, fields, body)
}

/// Removes a document, the note as the tombstone's body saying why. An
/// already removed document takes a note again; a renamed one does not.
pub fn tombstone(deps: &Deps, slug: &Slug, note: &str) -> Result<Written, Failure> {
    let body = note_body(note)
        .ok_or_else(|| Failure::Usage("a tombstone's note cannot be blank".into()))?;
    let document = deps.store.document(slug)?;
    let version = match document.state() {
        State::Live(v) => v.clone(),
        State::Tombstoned(v) if v.block.superseded_by.is_none() => v.clone(),
        _ => return Err(load::not_live(slug, &document)),
    };
    let fields = version.fields.clone();
    amend(deps, &version, Operation::Tombstone, fields, body)
}

/// A tombstone naming the new slug, and the new slug's first version with
/// the same content, parented on the tombstone so a walk back from the new
/// slug crosses the move.
pub fn rename(deps: &Deps, from: &Slug, to: &str) -> Result<Written, Failure> {
    let to = Slug::of_kind(from.kind(), to)?;
    let version = load::live_to_amend(deps.store, from)?;
    refuse_existing(deps, &to)?;
    let stone = store_version(
        deps,
        from.clone(),
        vec![version.id],
        Operation::Rename,
        version.fields.clone(),
        String::new(),
        Some(to.clone()),
        None,
    )?;
    let moved = store_version(
        deps,
        to,
        vec![stone.id.clone()],
        Operation::Rename,
        version.fields,
        version.body,
        None,
        Some(from.clone()),
    )?;
    Ok(Written {
        slug: moved.slug.path().to_owned(),
        id: moved.id.to_string(),
        tombstone: Some(stone.id.to_string()),
    })
}

/// A claim or its removal on this machine's topic. `path` is absolute;
/// `change` gets it spelled as the store spells claims.
fn reclaim(
    deps: &Deps,
    topic: &str,
    path: &str,
    change: impl FnOnce(&mut Topic, &str) -> Result<(), ClaimError>,
    operation: Operation,
) -> Result<Written, Failure> {
    let topics = load::topics(deps.store)?;
    if !topics.iter().any(|t| t.slug.path() == topic) {
        // The precise refusal: absent, removed, or forked.
        load::live(deps.store, &Slug::of_kind(Kind::Topic, topic)?)?;
    }
    let name = machine(deps)?;
    let mut doc = load::machine_topic(&topics, name.as_str())?.clone();
    change(&mut doc.data, &graph::contract(path, &deps.home))
        .map_err(|e| Failure::at(&doc.slug, e))?;
    let body = std::mem::take(&mut doc.version.body);
    amend(deps, &doc.version, operation, doc.data.to_fields(), body)
}

/// Claims a directory for a topic on this machine.
pub fn claim(deps: &Deps, topic: &str, path: &str) -> Result<Written, Failure> {
    reclaim(
        deps,
        topic,
        path,
        |t, p| t.claim(topic, p),
        Operation::Claim,
    )
}

pub fn unclaim(deps: &Deps, topic: &str, path: &str) -> Result<Written, Failure> {
    reclaim(
        deps,
        topic,
        path,
        |t, p| t.unclaim(topic, p),
        Operation::Unclaim,
    )
}

#[cfg(any(test, feature = "testing"))]
pub use seeding::*;

/// First versions written straight into a store, for tests.
#[cfg(any(test, feature = "testing"))]
mod seeding {
    use super::{
        Deps, Entry, Fact, Failure, Followup, FollowupState, Kind, Operation, Recheck, Slug, Topic,
        Version, first_version, machine,
    };
    use crate::domain::machine::MachineName;

    fn owned(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    pub fn put_topic(
        deps: &Deps,
        name: &str,
        summary: &str,
        includes: &[&str],
        body: Option<&str>,
    ) -> Result<Version, Failure> {
        let topic = Topic {
            summary: summary.into(),
            includes: owned(includes),
            ..Topic::default()
        };
        first_version(
            deps,
            Slug::of_kind(Kind::Topic, name)?,
            Operation::New,
            topic.to_fields(),
            body.unwrap_or("\n").to_owned(),
        )
    }

    pub fn put_machine_topic(
        deps: &Deps,
        name: &str,
        summary: &str,
        machine: &str,
        claims: &[(&str, &[&str])],
        unclaimed: &[&str],
    ) -> Result<Version, Failure> {
        let topic = Topic {
            summary: summary.into(),
            includes: vec![],
            machine: Some(MachineName::parse(machine)?),
            claims: claims
                .iter()
                .map(|(t, paths)| ((*t).to_owned(), owned(paths)))
                .collect(),
            unclaimed: owned(unclaimed),
        };
        first_version(
            deps,
            Slug::of_kind(Kind::Topic, name)?,
            Operation::New,
            topic.to_fields(),
            "\n".to_owned(),
        )
    }

    pub fn put_fact(
        deps: &Deps,
        slug: &str,
        summary: &str,
        tags: &[&str],
        idea: bool,
    ) -> Result<Version, Failure> {
        let fact = Fact {
            tags: owned(tags),
            idea,
            recheck: None,
            verified: None,
            summary: summary.into(),
        };
        first_version(
            deps,
            Slug::of_kind(Kind::Fact, slug)?,
            Operation::New,
            fact.to_fields(),
            "\n".to_owned(),
        )
    }

    pub fn put_entry(
        deps: &Deps,
        slug: &str,
        date: &str,
        summary: &str,
        tags: &[&str],
    ) -> Result<Version, Failure> {
        let entry = Entry {
            date: date.into(),
            machine: machine(deps)?,
            tags: owned(tags),
            files_touched: vec![],
            summary: summary.into(),
        };
        first_version(
            deps,
            Slug::of_kind(Kind::Entry, slug)?,
            Operation::New,
            entry.to_fields(),
            format!("\n## What\n{summary}\n"),
        )
    }

    pub fn put_followup(
        deps: &Deps,
        slug: &str,
        entry: &str,
        summary: &str,
        tags: &[&str],
        recheck: Option<&str>,
    ) -> Result<Version, Failure> {
        let followup = Followup {
            entry: Slug::of_kind(Kind::Entry, entry)?,
            tags: owned(tags),
            recheck: recheck.map(Recheck::parse).transpose()?,
            state: FollowupState::Open,
            summary: summary.into(),
        };
        first_version(
            deps,
            Slug::of_kind(Kind::Followup, slug)?,
            Operation::New,
            followup.to_fields(),
            "\n".to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::World;
    use crate::domain::frontmatter::Value;
    use crate::domain::machine::MachineName;
    use crate::domain::ports::Store;

    fn edit_draft(deps: &Deps, slug: &Slug, summary: &str, body: &str) {
        let mut draft = deps.drafts.read(slug).unwrap().unwrap();
        draft.fields.set("summary", Value::Scalar(summary.into()));
        draft.body = body.into();
        deps.drafts.write(&draft).unwrap();
    }

    fn current(w: &World, slug: &Slug) -> Version {
        w.store.document(slug).unwrap().current().unwrap().clone()
    }

    /// A fact as a newer worklog would have written it, by editing the
    /// text where that grammar differs; put as the document's only version.
    fn from_the_future(w: &World, edit: fn(&str) -> String) -> Version {
        let d = w.deps();
        put_topic(&d, "lantern", "A lamp", &[], None).unwrap();
        let scratch = World::new("m1");
        let sd = scratch.deps();
        put_topic(&sd, "lantern", "A lamp", &[], None).unwrap();
        put_fact(
            &sd,
            "lantern/relay",
            "The relay is fixed",
            &["lantern"],
            false,
        )
        .unwrap();
        let written = current(&scratch, &Slug::parse("lantern/relay").unwrap()).to_text();
        let foreign = Version::from_text(&edit(&written)).unwrap();
        w.store.put(&foreign).unwrap();
        foreign
    }

    #[test]
    fn a_version_from_a_newer_worklog_refuses_every_write() {
        let w = World::new("m1");
        let foreign = from_the_future(&w, |t| t.replace("operation: new", "operation: pin"));
        let d = w.deps();
        let slug = Slug::parse("lantern/relay").unwrap();
        assert!(load::live(&w.store, &slug).is_ok());
        let refused = |r: Result<Written, Failure>| matches!(r, Err(Failure::Refused(m)) if m.contains("upgrade to change it"));
        assert!(refused(verify(&d, &slug)));
        assert!(refused(tombstone(&d, &slug, "gone")));
        assert!(refused(rename(&d, &slug, "lantern/relay-pin")));
        assert!(matches!(
            checkout(&d, &slug),
            Err(Failure::Refused(m)) if m.contains("operation `pin`")
        ));
        d.drafts
            .write(&Draft {
                slug: slug.clone(),
                parents: vec![foreign.id.clone()],
                fields: foreign.fields.clone(),
                body: "\nedited\n".into(),
            })
            .unwrap();
        assert!(refused(save(&d, &slug, false)));
    }

    #[test]
    fn a_kind_field_from_a_newer_worklog_still_lists() {
        let w = World::new("m1");
        from_the_future(&w, |t| t.replace("summary:", "hue: 3\nsummary:"));
        let loaded = load::load(&w.store).unwrap();
        assert_eq!(loaded.facts.len(), 1);
        assert_eq!(loaded.facts[0].foreign.as_deref(), Some("field `hue`"));
        assert!(loaded.broken.is_empty());
        assert!(matches!(
            checkout(&w.deps(), &Slug::parse("lantern/relay").unwrap()),
            Err(Failure::Refused(m)) if m.contains("field `hue`")
        ));
    }

    #[test]
    fn new_topic_then_save_then_checkout_and_save_again() {
        let w = World::new("m1");
        let d = w.deps();
        let slug = Slug::parse("lantern").unwrap();
        let opened = new_topic(&d, "lantern").unwrap();
        assert_eq!(opened.parents, Vec::<String>::new());
        assert!(
            matches!(save(&d, &slug, false), Err(Failure::Refused(m)) if m.contains("summary"))
        );
        edit_draft(&d, &slug, "A Rust app", "\nbody\n");
        let first = save(&d, &slug, false).unwrap();
        assert_eq!(d.drafts.list().unwrap().len(), 0);
        assert!(matches!(new_topic(&d, "lantern"), Err(Failure::Refused(_))));
        let again = checkout(&d, &slug).unwrap();
        assert_eq!(again.parents, std::slice::from_ref(&first.id));
        assert!(
            matches!(save(&d, &slug, false), Err(Failure::Refused(m)) if m.contains("equals its parent"))
        );
        edit_draft(&d, &slug, "A Rust app", "\nmore\n");
        let second = save(&d, &slug, false).unwrap();
        assert_ne!(first.id, second.id);
        let head = current(&w, &slug);
        assert_eq!(head.id.to_string(), second.id);
        assert_eq!(head.block.operation, Operation::Save);
    }

    #[test]
    fn a_stale_draft_is_refused_and_two_machines_fork() {
        let w = World::new("m1");
        let d = w.deps();
        let slug = Slug::parse("lantern").unwrap();
        put_topic(&d, "lantern", "A Rust app", &[], None).unwrap();
        checkout(&d, &slug).unwrap();
        // A sync lands a version from the other machine meanwhile.
        let other = World::new("m2");
        let od = Deps {
            store: &w.store,
            ..other.deps()
        };
        checkout(&od, &slug).unwrap();
        edit_draft(&od, &slug, "A Rust app", "\nfrom m2\n");
        save(&od, &slug, false).unwrap();
        edit_draft(&d, &slug, "A Rust app", "\nfrom m1\n");
        assert!(
            matches!(save(&d, &slug, false), Err(Failure::Refused(m)) if m.contains("moved on"))
        );
        assert!(d.drafts.read(&slug).unwrap().is_some());
        // The same race across machines that only sync afterwards is a fork:
        // m2's write lands as it would after a sync, straight into the store.
        let base = current(&w, &slug);
        let mut draft = d.drafts.read(&slug).unwrap().unwrap();
        draft.parents = vec![base.id.clone()];
        d.drafts.write(&draft).unwrap();
        save(&d, &slug, false).unwrap();
        let mut fields = base.fields.clone();
        fields.set("summary", Value::Scalar("A Rust app".into()));
        store_version(
            &od,
            slug.clone(),
            vec![base.id.clone()],
            Operation::Save,
            fields,
            "\nm2 again\n".into(),
            None,
            None,
        )
        .unwrap();
        assert!(matches!(
            w.store.document(&slug).unwrap().state(),
            State::Forked(_)
        ));
        assert!(matches!(checkout(&d, &slug), Err(Failure::Refused(m)) if m.contains("forked")));
        let opened = resolve(&d, &slug).unwrap();
        assert_eq!(opened.parents.len(), 2);
        assert!(
            matches!(save(&d, &slug, false), Err(Failure::Refused(m)) if m.contains("conflict markers"))
        );
        edit_draft(&d, &slug, "A Rust app", "\nreconciled\n");
        let resolved = save(&d, &slug, false).unwrap();
        let head = current(&w, &slug);
        assert_eq!(head.id.to_string(), resolved.id);
        assert_eq!(head.block.operation, Operation::Resolve);
    }

    #[test]
    fn followup_lifecycle_and_fact_commands() {
        let w = World::new("m1");
        let d = w.deps();
        put_topic(&d, "lantern", "A Rust app", &[], None).unwrap();
        put_entry(
            &d,
            "2026-09/2026-09-01-first",
            "2026-09-01",
            "First",
            &["lantern"],
        )
        .unwrap();
        let Made::Written(made) = new_followup(
            &d,
            "port",
            "2026-09/2026-09-01-first",
            &NewFollowup {
                summary: Some("Port it"),
                recheck: Some("2026-10-01 why"),
                tags: None,
            },
        )
        .unwrap() else {
            panic!("a summary writes at once")
        };
        let slug = Slug::parse(&made.slug).unwrap();
        assert_eq!(slug.path(), "2026-09-04-port");
        recheck(&d, &slug, "touching lantern").unwrap();
        done(&d, &slug, Some("dissolved by [[2026-09/2026-09-01-first]]")).unwrap();
        assert!(
            matches!(done(&d, &slug, None), Err(Failure::Refused(m)) if m.contains("already done"))
        );
        let head = current(&w, &slug);
        assert!(head.body.contains("dissolved by"));
        assert_eq!(head.block.operation, Operation::Done);
        let fact = Slug::parse("lantern/relay").unwrap();
        put_fact(
            &d,
            "lantern/relay",
            "The relay is fixed",
            &["lantern"],
            false,
        )
        .unwrap();
        verify(&d, &fact).unwrap();
        assert_eq!(
            current(&w, &fact).fields.scalar("verified"),
            Some("2026-09-04")
        );
        let renamed = rename(&d, &fact, "lantern/relay-pin").unwrap();
        assert!(renamed.tombstone.is_some());
        assert!(
            matches!(load::live(&w.store, &fact), Err(Failure::Refused(m)) if m.contains("renamed"))
        );
        let moved = Slug::parse("lantern/relay-pin").unwrap();
        let head = current(&w, &moved);
        assert_eq!(head.block.renamed_from, Some(fact.clone()));
        assert_eq!(
            head.block
                .parents
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            [renamed.tombstone.clone().unwrap()]
        );
        assert!(matches!(tombstone(&d, &moved, " "), Err(Failure::Usage(_))));
        tombstone(&d, &moved, "superseded by the board revision").unwrap();
        assert!(
            matches!(new_fact(&d, "lantern/relay-pin", false), Err(Failure::Refused(m)) if m.contains("never reused"))
        );
        assert!(matches!(
            load::live(&w.store, &moved),
            Err(Failure::Refused(m)) if m.ends_with("was removed: superseded by the board revision")
        ));
        // A removed document takes its note again; a renamed one takes none.
        tombstone(&d, &moved, "and the README").unwrap();
        assert!(matches!(
            tombstone(&d, &fact, "gone"),
            Err(Failure::Refused(m)) if m.contains("renamed to")
        ));
    }

    #[test]
    fn claims_land_on_the_machine_topic() {
        let w = World::new("m1");
        let d = w.deps();
        put_topic(&d, "lantern", "A Rust app", &[], None).unwrap();
        let dir = "/home/u/projects/lantern";
        assert!(
            matches!(claim(&d, "lantern", dir), Err(Failure::Refused(m)) if m.contains("machine: m1"))
        );
        put_machine_topic(&d, "desk", "This machine", "m1", &[], &[]).unwrap();
        assert!(
            matches!(claim(&d, "nowhere", dir), Err(Failure::Refused(m)) if m.contains("no topic"))
        );
        claim(&d, "lantern", dir).unwrap();
        let desk = current(&w, &Slug::parse("desk").unwrap());
        assert_eq!(desk.block.operation, Operation::Claim);
        let topic = Topic::from_fields(&desk.fields).unwrap();
        assert_eq!(
            topic.claims[0].1,
            ["~/projects/lantern"],
            "spelled as a claim"
        );
        unclaim(&d, "lantern", dir).unwrap();
        assert!(
            matches!(unclaim(&d, "lantern", dir), Err(Failure::Refused(m)) if m.contains("does not claim"))
        );
    }

    #[test]
    fn writes_need_a_machine_name() {
        let w = World::unnamed();
        let d = w.deps();
        assert!(
            matches!(new_topic(&d, "x"), Err(Failure::Refused(m)) if m.contains("worklog init"))
        );
        *w.identity.0.borrow_mut() = Some(MachineName::parse("m1").unwrap());
        assert!(new_topic(&d, "x").is_ok());
    }
}
