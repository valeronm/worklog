use std::rc::Rc;

use serde::Serialize;

use crate::app::draft::{draft_for, draft_label, opened, shown_as};
use crate::app::heads::{ended_of, is_forked, machine_label};
use crate::app::lookup::{Lookup, names_no_document};
use crate::app::{Deps, Failure};
use crate::domain::document::{Document, Unreadable};
use crate::domain::draft::Draft;
use crate::domain::id::{DocumentId, VersionId, is_id_prefix};
use crate::domain::schema::KindOf;
use crate::domain::schema::address::Found;
use crate::domain::version::{ReadError, Stamp, Version};

/// A file of the document that does not read as a version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unread {
    pub version: VersionId,
    pub why: Why,
}

/// `text` is the reason as a person reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Why {
    Corrupt { text: String },
    Newer { format: u32, text: String },
    Malformed { text: String },
}

impl Why {
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Why::Corrupt { text } | Why::Newer { text, .. } | Why::Malformed { text } => text,
        }
    }
}

impl From<&Unreadable> for Unread {
    fn from(file: &Unreadable) -> Unread {
        let text = file.why.to_string();
        Unread {
            version: file.version.clone(),
            why: match file.why {
                ReadError::Corrupt => Why::Corrupt { text },
                ReadError::Newer { format } => Why::Newer { format, text },
                ReadError::Malformed(_) => Why::Malformed { text },
            },
        }
    }
}

fn unread(document: &Document) -> Vec<Unread> {
    document.unreadable().iter().map(Unread::from).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shown {
    Document(ShownDocument),
    Version(ShownVersion),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShownDocument {
    pub document: DocumentId,
    /// As stored, also for a kind this worklog does not know; `None` when no version reads.
    pub kind: Option<String>,
    pub label: String,
    pub former: Vec<String>,
    pub forked: bool,
    /// One when live, several when forked, in head order.
    pub heads: Vec<ShownVersion>,
    pub unreadable: Vec<Unread>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShownVersion {
    pub document: DocumentId,
    pub label: String,
    pub version: VersionId,
    pub written: Stamp,
    pub machine: String,
    pub change: String,
    pub parents: Vec<VersionId>,
    pub head: bool,
    /// The reason; `None` too for a version that does not read as a record.
    pub ended: Option<String>,
    /// Every field the version holds, tool-owned ones included, references as labels.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct History {
    pub document: DocumentId,
    pub label: String,
    /// The one with the most behind it first.
    pub versions: Vec<ShownVersion>,
    pub unreadable: Vec<Unread>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub label: String,
    pub sides: Vec<Side>,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Side {
    /// The version compared against; `None` when there is nothing before `after`.
    pub against: Option<VersionId>,
    /// False only when `against` is a version that is not held; true when there is nothing to
    /// compare against.
    pub held: bool,
    pub before: String,
}

enum Resolved {
    Document(DocumentId),
    Version {
        document: DocumentId,
        version: VersionId,
    },
}

fn resolved(lookup: &Lookup, address: &str) -> Result<Option<Resolved>, Failure> {
    match lookup.find(address)? {
        Found::One(id) => Ok(Some(Resolved::Document(id))),
        Found::Collision(_) => lookup.one(address).map(|id| Some(Resolved::Document(id))),
        Found::None if is_id_prefix(address) => {
            let mut held = lookup.versions_under(address)?;
            match held.len() {
                0 => Ok(None),
                1 => {
                    let (document, version) = held.remove(0);
                    if let Some(broken) = lookup
                        .document(&document)?
                        .unreadable()
                        .iter()
                        .find(|broken| broken.version == version)
                    {
                        return Err(Failure::at(
                            address,
                            format!("version {} does not read: {}", version.short(), broken.why),
                        ));
                    }
                    Ok(Some(Resolved::Version { document, version }))
                }
                _ => {
                    let shorts: Vec<&str> =
                        held.iter().map(|(_, version)| version.short()).collect();
                    Err(Failure::at(
                        address,
                        format!("is held by several versions: {}", shorts.join(", ")),
                    ))
                }
            }
        }
        Found::None => Ok(None),
    }
}

fn resolve(lookup: &Lookup, address: &str) -> Result<Resolved, Failure> {
    resolved(lookup, address)?.ok_or_else(|| names_no_document(address))
}

fn render(draft: &Draft, what: &str) -> Result<String, Failure> {
    draft.text().map_err(|error| Failure::at(what, error))
}

fn shown_text(lookup: &Lookup, version: &Version) -> Result<String, Failure> {
    let mut draft = Draft::of(version);
    if let Ok(kind) = KindOf::of(&version.envelope.kind) {
        draft.fields = shown_as(&version.fields, kind, |id| lookup.label(id).map(Some))?;
    }
    render(&draft, version.envelope.document.short())
}

fn shown_version(
    lookup: &Lookup,
    label: &str,
    version: &Version,
    head: bool,
) -> Result<ShownVersion, Failure> {
    Ok(ShownVersion {
        document: version.envelope.document.clone(),
        label: label.to_owned(),
        version: version.id.clone(),
        written: version.envelope.written.clone(),
        machine: machine_label(
            version,
            lookup.topic_name(&version.envelope.machine)?.as_ref(),
        ),
        change: version.envelope.change.clone(),
        parents: version.envelope.parents.clone(),
        head,
        ended: ended_of(version),
        text: shown_text(lookup, version)?,
    })
}

fn is_head(document: &Document, version: &Version) -> bool {
    document.heads().iter().any(|head| head.id == version.id)
}

fn held<'a>(
    document: &'a Document,
    version: &VersionId,
    address: &str,
) -> Result<&'a Version, Failure> {
    document
        .get(version)
        .ok_or_else(|| names_no_document(address))
}

/// Resolves a document by name, id prefix or former name, and a version by the start of its
/// hash with or without `b3-`; a name wins over a prefix. Refuses an address that names
/// nothing or several documents or versions. An ended or unreadable document is shown too.
pub fn show(deps: &Deps, address: &str) -> Result<Shown, Failure> {
    let lookup = Lookup::new(deps.store);
    let resolved = resolve(&lookup, address)?;
    match resolved {
        Resolved::Document(id) => {
            let document = lookup.document(&id)?;
            let label = lookup.label(&id)?;
            let heads = document
                .heads()
                .into_iter()
                .map(|head| shown_version(&lookup, &label, head, true))
                .collect::<Result<_, _>>()?;
            Ok(Shown::Document(ShownDocument {
                kind: document.kind().map(ToString::to_string),
                forked: is_forked(&document),
                former: lookup.former_addresses(&id)?,
                unreadable: unread(&document),
                document: id,
                label,
                heads,
            }))
        }
        Resolved::Version { document, version } => {
            let held_document = lookup.document(&document)?;
            let label = lookup.label(&document)?;
            let version = held(&held_document, &version, address)?;
            let head = is_head(&held_document, version);
            Ok(Shown::Version(shown_version(
                &lookup, &label, version, head,
            )?))
        }
    }
}

/// Takes a document only; a version's hash is a usage failure.
pub fn history(deps: &Deps, address: &str) -> Result<History, Failure> {
    let lookup = Lookup::new(deps.store);
    let id = match resolve(&lookup, address)? {
        Resolved::Document(id) => id,
        Resolved::Version { .. } => {
            return Err(Failure::Usage(format!(
                "{address}: names a version; a history is of a document"
            )));
        }
    };
    let document = lookup.document(&id)?;
    let label = lookup.label(&id)?;
    let versions = document
        .history()
        .into_iter()
        .map(|version| shown_version(&lookup, &label, version, is_head(&document, version)))
        .collect::<Result<_, _>>()?;
    Ok(History {
        unreadable: unread(&document),
        document: id,
        label,
        versions,
    })
}

fn sides(
    document: &Document,
    parents: &[VersionId],
    before: impl Fn(&Version) -> Result<String, Failure>,
) -> Result<Vec<Side>, Failure> {
    if parents.is_empty() {
        return Ok(vec![Side {
            against: None,
            held: true,
            before: String::new(),
        }]);
    }
    parents
        .iter()
        .map(|parent| match document.get(parent) {
            Some(version) => Ok(Side {
                against: Some(parent.clone()),
                held: true,
                before: before(version)?,
            }),
            None => Ok(Side {
                against: Some(parent.clone()),
                held: false,
                before: String::new(),
            }),
        })
        .collect()
}

fn draft_text(lookup: &Lookup, version: &Version) -> Result<String, Failure> {
    let kind = KindOf::of(&version.envelope.kind)?;
    render(
        &opened(lookup, version, kind)?,
        version.envelope.document.short(),
    )
}

fn diff_draft(deps: &Deps, lookup: &Lookup, address: &str, named: bool) -> Result<Diff, Failure> {
    let draft = match draft_for(deps, lookup, address) {
        Ok(draft) => draft,
        Err(Failure::Refused(_)) if !named => return Err(names_no_document(address)),
        Err(other) => return Err(other),
    };
    let document = lookup.document(&draft.document)?;
    let label = draft_label(&draft);
    Ok(Diff {
        sides: sides(&document, &draft.parents, |version| {
            draft_text(lookup, version)
        })?,
        after: render(&draft, &label)?,
        label,
    })
}

fn is_earlier(a: &Version, b: &Version) -> bool {
    if b.envelope.ancestors.contains(&a.id) {
        return true;
    }
    if a.envelope.ancestors.contains(&b.id) {
        return false;
    }
    (&a.envelope.written, &a.id) <= (&b.envelope.written, &b.id)
}

fn version_argument(
    lookup: &Lookup,
    address: &str,
    other: &str,
) -> Result<(Rc<Document>, VersionId), Failure> {
    match resolve(lookup, address)? {
        Resolved::Version { document, version } => Ok((lookup.document(&document)?, version)),
        Resolved::Document(_) => Err(Failure::Usage(format!(
            "{address}: names a document; comparing with {other} takes versions"
        ))),
    }
}

/// With one address, a document's draft against the versions it started from, or a version
/// against its parents; a draft is also found by the start of its document's id and by the
/// address its fields spell, which need not be a valid one. With two, two versions, the
/// earlier on the left whichever came first.
/// A document as either of two arguments is a usage failure. Across two documents the label
/// is the later side's.
pub fn diff(deps: &Deps, first: &str, second: Option<&str>) -> Result<Diff, Failure> {
    let lookup = Lookup::new(deps.store);
    let Some(second) = second else {
        let found = match resolved(&lookup, first) {
            Err(Failure::Usage(_)) => None,
            found => found?,
        };
        let Some(Resolved::Version { document, version }) = found else {
            return diff_draft(deps, &lookup, first, found.is_some());
        };
        let document = lookup.document(&document)?;
        let version = held(&document, &version, first)?;
        return Ok(Diff {
            label: lookup.label(&version.envelope.document)?,
            sides: sides(&document, &version.envelope.parents, |parent| {
                shown_text(&lookup, parent)
            })?,
            after: shown_text(&lookup, version)?,
        });
    };
    let (a_document, a) = version_argument(&lookup, first, second)?;
    let (b_document, b) = version_argument(&lookup, second, first)?;
    let (a, b) = (
        held(&a_document, &a, first)?,
        held(&b_document, &b, second)?,
    );
    let (earlier, later) = if is_earlier(a, b) { (a, b) } else { (b, a) };
    Ok(Diff {
        label: lookup.label(&later.envelope.document)?,
        sides: vec![Side {
            against: Some(earlier.id.clone()),
            held: true,
            before: shown_text(&lookup, earlier)?,
        }],
        after: shown_text(&lookup, later)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::draft::checkout;
    use crate::app::testing::{
        ENDED, TOPIC, World, fact, fork, head, refused, topic, topic_fields, world, written_at,
    };
    use crate::domain::draft::Draft;
    use crate::domain::id::VersionId;
    use crate::domain::ports::{Drafts, Store};
    use crate::domain::testing::{after, first, lantern};
    use crate::domain::version::ReadError;

    fn shown_document(world: &World, address: &str) -> ShownDocument {
        match show(&world.deps(), address).unwrap() {
            Shown::Document(document) => document,
            Shown::Version(_) => panic!("expected a document"),
        }
    }

    fn shown_version(world: &World, address: &str) -> ShownVersion {
        match show(&world.deps(), address).unwrap() {
            Shown::Version(version) => version,
            Shown::Document(_) => panic!("expected a version"),
        }
    }

    #[test]
    fn a_document_is_shown_by_name_id_prefix_or_former_name() {
        let world = World::new();
        topic(&world, "desk", "");
        let lantern = lantern();
        world
            .store
            .put(&first(
                &lantern,
                "topic",
                &topic_fields("lantern", ""),
                "\n",
            ))
            .unwrap();
        world.amend(
            &lantern,
            &topic_fields("lantern", "former_names = [\"lamp\"]\n"),
            "\n",
        );
        for address in ["lantern", "lamp", "7f3a91"] {
            let shown = shown_document(&world, address);
            assert_eq!(shown.document, lantern);
            assert_eq!(shown.label, "lantern");
            assert_eq!(shown.kind.as_deref(), Some("topic"));
            assert_eq!(shown.former, vec!["lamp".to_owned()]);
            assert_eq!((shown.heads.len(), shown.forked), (1, false));
            let head = &shown.heads[0];
            assert!(head.head && head.ended.is_none());
            assert_eq!(head.label, "lantern");
            assert_eq!(head.machine, "desk");
            assert_eq!(head.change, "save");
            assert!(
                head.text.starts_with("+++\nname = \"lantern\"\n"),
                "{}",
                head.text
            );
            assert!(
                head.text.contains("former_names = [\"lamp\"]"),
                "{}",
                head.text
            );
            assert!(head.text.contains("created = 2026-09-04"), "{}", head.text);
        }
        let text = refused(show(&world.deps(), "unknown"));
        assert!(text.contains("unknown: names no document"), "{text}");
    }

    #[test]
    fn a_machine_that_is_no_topic_is_shown_as_its_short_id() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &lantern);
        let written = written_at(
            &world,
            &root,
            &topic_fields("lantern", ""),
            "2026-10-09T10:00:00+01:00",
            "\n",
            Some(&relay),
        );
        let absent = DocumentId::from_bytes([0xb8; 16]);
        let newest = written_at(
            &world,
            &written,
            &topic_fields("lantern", ""),
            "2026-10-09T11:00:00+01:00",
            "\n",
            Some(&absent),
        );
        assert_eq!(
            shown_version(&world, written.id.as_str()).machine,
            relay.short()
        );
        assert_eq!(
            shown_document(&world, "lantern").heads[0].machine,
            absent.short()
        );
        let history = history(&world.deps(), "lantern").unwrap();
        let machines: Vec<&str> = history
            .versions
            .iter()
            .map(|version| version.machine.as_str())
            .collect();
        assert_eq!(machines, [absent.short(), relay.short(), "desk"]);
        assert_eq!(history.versions[0].version, newest.id);
    }

    #[test]
    fn an_ended_document_is_shown() {
        let (world, lantern) = world();
        world.amend(&lantern, &topic_fields("lantern", ENDED), "\n");
        let shown = shown_document(&world, "lantern");
        assert_eq!(shown.heads[0].ended.as_deref(), Some("retired"));
        assert!(shown.heads[0].text.contains("ended = \"retired\""));
    }

    #[test]
    fn a_fork_shows_both_heads_and_a_collision_is_refused() {
        let (world, lantern) = world();
        let heads = fork(&world, &lantern, &topic_fields("lantern", ""));
        let shown = shown_document(&world, "lantern");
        let shown_heads: Vec<&VersionId> = shown.heads.iter().map(|head| &head.version).collect();
        assert_eq!(shown_heads, heads.iter().collect::<Vec<_>>());
        assert!(shown.heads.iter().all(|head| head.head));
        assert!(shown.forked);

        topic(&world, "phone", "");
        topic(&world, "phone", "");
        let text = refused(show(&world.deps(), "phone"));
        assert!(text.contains("is held by several documents"), "{text}");
    }

    #[test]
    fn a_version_is_shown_by_prefix_with_or_without_the_algorithm() {
        let (world, lantern) = world();
        let second = world.amend(&lantern, &topic_fields("lantern", ""), "second\n");
        for prefix in [
            &second.as_str()[3..11],
            &second.as_str()[..11],
            second.as_str(),
        ] {
            let shown = shown_version(&world, prefix);
            assert_eq!(shown.version, second);
            assert_eq!(shown.document, lantern);
            assert_eq!(shown.label, "lantern");
            assert!(shown.head);
            assert_eq!(shown.parents.len(), 1);
            assert!(shown.text.ends_with("+++\nsecond\n"), "{}", shown.text);
        }
        let root = &shown_version(
            &world,
            &shown_version(&world, second.as_str()).parents[0].to_string(),
        );
        assert!(!root.head);
    }

    #[test]
    fn an_ambiguous_version_prefix_is_refused_and_a_name_wins_over_a_prefix() {
        let (world, lantern) = world();
        let mut ids = Vec::new();
        for round in 0..24 {
            ids.push(world.amend(
                &lantern,
                &topic_fields("lantern", ""),
                &format!("{round}\n"),
            ));
        }
        let shared = ids
            .iter()
            .map(|id| id.as_str().as_bytes()[3] as char)
            .filter(|c| *c != '0')
            .find(|c| {
                ids.iter()
                    .filter(|id| id.as_str().as_bytes()[3] as char == *c)
                    .count()
                    > 1
            })
            .expect("two versions share a first digit");
        let shared = shared.to_string();
        assert!(world.store.documents_under(&shared).unwrap().is_empty());
        let text = refused(show(&world.deps(), &shared));
        assert!(text.contains("is held by several versions"), "{text}");

        let named = ids
            .iter()
            .map(|id| id.as_str()[3..6].to_owned())
            .find(|prefix| {
                prefix.bytes().any(|b| b.is_ascii_lowercase()) && !prefix.starts_with('0')
            })
            .expect("a prefix that could be a name");
        assert!(world.store.documents_under(&named).unwrap().is_empty());
        let version = ids
            .iter()
            .find(|id| id.as_str()[3..].starts_with(&named))
            .unwrap();
        assert_eq!(shown_version(&world, &named).version, *version);
        let wins = topic(&world, &named, "");
        assert_eq!(shown_document(&world, &named).document, wins);
    }

    #[test]
    fn references_are_shown_as_labels_and_an_absent_target_as_a_short_id() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let shown = shown_document(&world, "lantern/relay-pin");
        assert_eq!(shown.document, relay);
        assert_eq!(shown.label, "lantern/relay-pin");
        assert!(shown.heads[0].text.contains("topic = \"lantern\""));

        let absent = DocumentId::from_bytes([0xb8; 16]);
        let atlas = topic(&world, "atlas", "");
        world.amend(
            &atlas,
            &format!(
                "name = \"atlas\"\n{TOPIC}{ENDED}ended_by = \"{relay}\"\npart_of = [\"{absent}\", \"{lantern}\"]\n"
            ),
            "\n",
        );
        let text = &shown_document(&world, "atlas").heads[0].text;
        assert!(text.contains("ended_by = \"lantern/relay-pin\""), "{text}");
        assert!(
            text.contains(&format!("part_of = [\"{}\", \"lantern\"]", absent.short())),
            "{text}"
        );
    }

    #[test]
    fn a_document_with_no_readable_head_is_shown_with_what_is_wrong() {
        let (world, _) = world();
        let stray = DocumentId::from_bytes([0xc4; 16]);
        let version = VersionId::of(b"x");
        world
            .store
            .plant_unreadable(&stray, version.clone(), ReadError::Corrupt);
        let shown = shown_document(&world, stray.as_str());
        assert_eq!(shown.kind, None);
        assert_eq!(shown.label, stray.short());
        assert!(shown.heads.is_empty());
        assert_eq!(shown.unreadable.len(), 1);
        assert_eq!(shown.unreadable[0].version, version);
    }

    #[test]
    fn a_history_is_newest_first_and_marks_the_head() {
        let (world, lantern) = world();
        let second = world.amend(&lantern, &topic_fields("lantern", ""), "second\n");
        let third = world.amend(&lantern, &topic_fields("lantern", ""), "third\n");
        let history = history(&world.deps(), "lantern").unwrap();
        assert_eq!(history.document, lantern);
        assert_eq!(history.label, "lantern");
        let ids: Vec<&VersionId> = history.versions.iter().map(|v| &v.version).collect();
        assert_eq!(ids[..2], [&third, &second]);
        assert_eq!(ids.len(), 3);
        let heads: Vec<bool> = history.versions.iter().map(|v| v.head).collect();
        assert_eq!(heads, [true, false, false]);

        let text = refused(super::history(&world.deps(), "nobody"));
        assert!(text.contains("names no document"), "{text}");
        let by_version = super::history(&world.deps(), &third.as_str()[3..12]);
        assert!(
            matches!(by_version, Err(Failure::Usage(_))),
            "{by_version:?}"
        );
    }

    fn only_side(diff: &Diff) -> &Side {
        assert_eq!(diff.sides.len(), 1);
        &diff.sides[0]
    }

    #[test]
    fn a_draft_checked_out_and_left_alone_equals_what_it_started_from() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        checkout(&world.deps(), "lantern").unwrap();
        let diff = diff(&world.deps(), "lantern", None).unwrap();
        assert_eq!(diff.label, "lantern");
        let side = only_side(&diff);
        assert_eq!((side.against.as_ref(), side.held), (Some(&root.id), true));
        assert_eq!(side.before, diff.after);

        let draft = world.drafts.read(&lantern).unwrap().unwrap();
        let edited = draft
            .with_text(
                &draft
                    .text()
                    .unwrap()
                    .replace("\n+++\n\n", "\n+++\nedited\n"),
            )
            .unwrap();
        world.drafts.write(&edited).unwrap();
        let diff = super::diff(&world.deps(), "lantern", None).unwrap();
        assert_ne!(only_side(&diff).before, diff.after);
        assert!(diff.after.ends_with("edited\n"), "{}", diff.after);
    }

    #[test]
    fn a_draft_with_references_compares_equal_to_its_stored_side() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "");
        checkout(&world.deps(), "lantern/relay-pin").unwrap();
        let diff = diff(&world.deps(), "lantern/relay-pin", None).unwrap();
        let side = only_side(&diff);
        assert_eq!(side.before, diff.after);
        assert!(diff.after.contains("topic = \"lantern\""));
    }

    #[test]
    fn a_first_draft_has_nothing_to_compare_to_and_no_draft_is_refused() {
        let (world, _) = world();
        let text = refused(diff(&world.deps(), "lantern", None));
        assert!(text.contains("lantern: no draft"), "{text}");
        let text = refused(diff(&world.deps(), "nobody", None));
        assert!(text.contains("names no document"), "{text}");

        crate::app::draft::new(
            &world.deps(),
            &crate::app::draft::New::Topic { name: "phone" },
        )
        .unwrap();
        let diff = diff(&world.deps(), "phone", None).unwrap();
        assert_eq!(diff.label, "phone");
        let side = only_side(&diff);
        assert_eq!((&side.against, side.held), (&None, true));
        assert_eq!(side.before, "");
        assert!(diff.after.contains("name = \"phone\""), "{}", diff.after);
    }

    #[test]
    fn a_draft_is_compared_by_the_address_its_listing_shows() {
        let (world, lantern) = world();
        let deps = world.deps();
        checkout(&deps, "lantern").unwrap();
        let mut draft = world.drafts.read(&lantern).unwrap().unwrap();
        draft
            .fields
            .insert("name".to_owned(), toml::Value::from("Relay Pin"));
        world.drafts.write(&draft).unwrap();
        let listed = crate::app::draft::drafts(&deps).unwrap();
        assert_eq!(listed[0].label, "Relay Pin");

        let diff = diff(&deps, &listed[0].label, None).unwrap();
        assert_eq!(diff.label, "Relay Pin");
        assert!(
            only_side(&diff).before.contains("name = \"lantern\""),
            "{}",
            only_side(&diff).before
        );
        assert!(
            diff.after.contains("name = \"Relay Pin\""),
            "{}",
            diff.after
        );
        let text = refused(super::diff(&deps, "Lamp Pin", None));
        assert!(text.contains("Lamp Pin: names no document"), "{text}");
    }

    #[test]
    fn a_draft_spelling_no_address_is_labeled_as_its_listing_labels_it() {
        let (world, lantern) = world();
        let deps = world.deps();
        checkout(&deps, "lantern").unwrap();
        let mut draft = world.drafts.read(&lantern).unwrap().unwrap();
        draft.fields.remove("name");
        world.drafts.write(&draft).unwrap();
        let listed = crate::app::draft::drafts(&deps).unwrap();
        assert_eq!(listed[0].label, lantern.short());
        assert_eq!(diff(&deps, "lantern", None).unwrap().label, lantern.short());
    }

    #[test]
    fn a_version_is_compared_with_each_of_its_parents() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        let one = fork(&world, &lantern, &topic_fields("lantern", ""));
        let held = world.store.document(&lantern).unwrap();
        let joined = after(
            &[held.get(&one[0]).unwrap(), held.get(&one[1]).unwrap()],
            &topic_fields("lantern", ""),
            "joined\n",
        );
        world.store.put(&joined).unwrap();

        let diff = diff(&world.deps(), &joined.id.as_str()[3..13], None).unwrap();
        assert_eq!(diff.label, "lantern");
        assert_eq!(diff.sides.len(), 2);
        let mut against: Vec<&VersionId> = diff.sides.iter().flat_map(|s| &s.against).collect();
        against.sort_unstable();
        let mut parents: Vec<&VersionId> = one.iter().collect();
        parents.sort_unstable();
        assert_eq!(against, parents);
        assert!(diff.sides.iter().all(|s| s.held));
        assert!(diff.after.ends_with("joined\n"));
        assert!(diff.sides.iter().any(|s| s.before.ends_with("left\n")));

        let first_version = super::diff(&world.deps(), &root.id.as_str()[3..13], None).unwrap();
        let side = only_side(&first_version);
        assert_eq!(side.before, "");
        assert_eq!((&side.against, side.held), (&None, true));
    }

    #[test]
    fn a_parent_not_held_is_a_side_with_nothing_before_it() {
        let (world, _) = world();
        let stray = lantern();
        let root = first(&stray, "topic", &topic_fields("lantern", ""), "root\n");
        let child = after(&[&root], &topic_fields("lantern", ""), "child\n");
        world.store.put(&child).unwrap();
        let diff = diff(&world.deps(), &child.id.as_str()[3..13], None).unwrap();
        let side = only_side(&diff);
        assert_eq!(side.before, "");
        assert_eq!((side.against.as_ref(), side.held), (Some(&root.id), false));
    }

    #[test]
    fn two_versions_are_compared_the_earlier_on_the_left() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        let second = world.amend(&lantern, &topic_fields("lantern", ""), "second\n");
        let (a, b) = (&root.id.as_str()[3..13], &second.as_str()[3..13]);
        for (typed_first, typed_second) in [(a, b), (b, a)] {
            let diff = diff(&world.deps(), typed_first, Some(typed_second)).unwrap();
            let side = only_side(&diff);
            assert_eq!((side.against.as_ref(), side.held), (Some(&root.id), true));
            assert!(side.before.ends_with("+++\n\n"), "{}", side.before);
            assert!(diff.after.ends_with("second\n"), "{}", diff.after);
        }

        let other = topic(&world, "atlas", "");
        let other = head(&world, &other);
        let diff = diff(&world.deps(), &other.id.as_str()[3..13], Some(a)).unwrap();
        assert_eq!(only_side(&diff).against.as_ref(), Some(&root.id));

        let usage = super::diff(&world.deps(), "lantern", Some(a));
        assert!(matches!(usage, Err(Failure::Usage(_))), "{usage:?}");
        let usage = super::diff(&world.deps(), a, Some("lantern"));
        assert!(matches!(usage, Err(Failure::Usage(_))), "{usage:?}");
    }

    #[test]
    fn a_version_that_does_not_read_is_refused_with_why() {
        let (world, lantern) = world();
        let broken = VersionId::of(b"broken");
        world
            .store
            .plant_unreadable(&lantern, broken.clone(), ReadError::Corrupt);
        for address in [broken.as_str(), &broken.as_str()[3..12]] {
            let text = refused(show(&world.deps(), address));
            assert!(
                text.contains(&format!("version {} does not read", broken.short())),
                "{text}"
            );
            assert!(text.contains(&ReadError::Corrupt.to_string()), "{text}");
        }
        let text = refused(diff(&world.deps(), &broken.as_str()[3..12], None));
        assert!(text.contains("does not read"), "{text}");
    }

    #[test]
    fn the_history_of_a_fork_and_of_an_unreadable_document() {
        let (world, lantern) = world();
        let heads = fork(&world, &lantern, &topic_fields("lantern", ""));
        let shown = history(&world.deps(), "lantern").unwrap();
        assert_eq!(shown.versions.len(), 3);
        let flagged: Vec<&VersionId> = shown
            .versions
            .iter()
            .filter(|version| version.head)
            .map(|version| &version.version)
            .collect();
        assert_eq!(flagged.len(), 2);
        assert!(heads.iter().all(|head| flagged.contains(&head)));

        let stray = DocumentId::from_bytes([0xc4; 16]);
        let version = VersionId::of(b"x");
        world
            .store
            .plant_unreadable(&stray, version.clone(), ReadError::Corrupt);
        let shown = history(&world.deps(), stray.as_str()).unwrap();
        assert_eq!(shown.label, stray.short());
        assert!(shown.versions.is_empty());
        assert_eq!(shown.unreadable[0].version, version);
    }

    #[test]
    fn a_draft_started_from_two_heads_is_compared_with_each() {
        let (world, lantern) = world();
        let heads = fork(&world, &lantern, &topic_fields("lantern", ""));
        let held = world.store.document(&lantern).unwrap();
        let mut draft = Draft::of(held.get(&heads[0]).unwrap());
        draft.parents = heads.clone();
        draft.fields = topic_fields("lantern", "").parse().unwrap();
        draft.body = "resolved\n".to_owned();
        world.drafts.write(&draft).unwrap();

        let diff = diff(&world.deps(), "lantern", None).unwrap();
        assert_eq!(diff.sides.len(), 2);
        for (side, head) in diff.sides.iter().zip(&heads) {
            assert_eq!((side.against.as_ref(), side.held), (Some(head), true));
        }
        assert!(
            diff.sides[0].before.ends_with("left\n") || diff.sides[0].before.ends_with("right\n")
        );
        assert_ne!(diff.sides[0].before, diff.sides[1].before);
        assert!(diff.after.ends_with("resolved\n"), "{}", diff.after);
    }
}
