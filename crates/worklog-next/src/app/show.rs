use std::rc::Rc;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use toml::Value;

use crate::app::draft::{draft_for, draft_label, opened, shown_as};
use crate::app::heads::{ended_for, ended_of, is_forked, kind_of, row_head};
use crate::app::lookup::{Lookup, names_no_document};
use crate::app::rows::Reference;
use crate::app::{Deps, Failure};
use crate::domain::document::{Document, Unreadable};
use crate::domain::draft::Draft;
use crate::domain::id::{DocumentId, VersionId, is_id_prefix};
use crate::domain::schema::KindOf;
use crate::domain::schema::address::Found;
use crate::domain::schema::translate::referred;
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

impl Unread {
    #[must_use]
    pub fn note(&self) -> String {
        format!(
            "version {} does not read: {}",
            self.version.short(),
            self.why.text()
        )
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
    Version(Compared),
}

/// A version beside what it changed. Its JSON is `document` and `label`, the version's own
/// keys and `sides` after them.
#[derive(Clone, Debug, PartialEq)]
pub struct Compared {
    pub document: DocumentId,
    pub label: String,
    pub version: ShownVersion,
    /// One for each parent in the order the version lists them, or one against nothing for a
    /// first version; `before` is the parent's text as `version.text` is the version's.
    pub sides: Vec<Side>,
}

impl Serialize for Compared {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let ShownVersion {
            version,
            written,
            machine,
            change,
            parents,
            head,
            ended,
            text,
            fields,
            body,
            links,
        } = &self.version;
        let mut keyed = serializer.serialize_struct("Compared", 14)?;
        keyed.serialize_field("document", &self.document)?;
        keyed.serialize_field("label", &self.label)?;
        keyed.serialize_field("version", version)?;
        keyed.serialize_field("written", written)?;
        keyed.serialize_field("machine", machine)?;
        keyed.serialize_field("change", change)?;
        keyed.serialize_field("parents", parents)?;
        keyed.serialize_field("head", head)?;
        keyed.serialize_field("ended", ended)?;
        keyed.serialize_field("text", text)?;
        keyed.serialize_field("fields", fields)?;
        keyed.serialize_field("body", body)?;
        keyed.serialize_field("links", links)?;
        keyed.serialize_field("sides", &self.sides)?;
        keyed.end()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShownDocument {
    pub document: DocumentId,
    /// As stored, also for a kind this worklog does not know; `None` when no version reads.
    pub kind: Option<String>,
    /// The kind the document's row head reads as; `None` when no head reads as a record,
    /// which none of an unknown kind does, and when that kind is not the one `kind` holds.
    pub reads_as: Option<KindOf>,
    pub label: String,
    pub former: Vec<String>,
    pub forked: bool,
    /// The reason when the document is ended, which a fork with a head that is not ended is
    /// not.
    pub ended: Option<String>,
    /// One when live, several when forked, in head order.
    pub heads: Vec<ShownVersion>,
    pub unreadable: Vec<Unread>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ShownVersion {
    pub version: VersionId,
    pub written: Stamp,
    pub machine: Reference,
    pub change: String,
    pub parents: Vec<VersionId>,
    pub head: bool,
    /// The reason; `None` too for a version that does not read as a record.
    pub ended: Option<String>,
    /// Every field the version holds, tool-owned ones included, references as labels.
    pub text: String,
    /// The fields `text` shows, in its order.
    pub fields: Vec<ShownField>,
    /// What follows the fields in `text`.
    pub body: String,
    /// The `[[text]]` links of its summary and body, by text.
    pub links: Vec<Linked>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ShownField {
    pub name: String,
    /// One for a field that holds one value, and one for each item of a list.
    pub values: Vec<FieldItem>,
}

/// An id held where a kind this worklog knows holds another document's id is a reference.
/// Anything else is the text it holds, a value that is no text as TOML writes it, and so is
/// every field of an unknown kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldItem {
    Text { text: String },
    Reference { reference: Reference },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Linked {
    /// The target between the brackets with the spaces around it trimmed and its case kept.
    pub text: String,
    /// What the link resolved to when the version was saved, or what its text names now when
    /// that was nothing; `None` when it names nothing or several documents.
    pub target: Option<Reference>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct History {
    pub document: DocumentId,
    pub label: String,
    /// The one with the most behind it first.
    pub versions: Vec<HistoryRow>,
    pub unreadable: Vec<Unread>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryRow {
    pub version: VersionId,
    pub written: Stamp,
    pub machine: Reference,
    pub change: String,
    pub parents: Vec<VersionId>,
    pub head: bool,
    /// The reason; `None` too for a version that does not read as a record.
    pub ended: Option<String>,
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
                        return Err(Failure::at(address, Unread::from(broken).note()));
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

fn written(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn item(lookup: &Lookup, value: &Value, refers: bool) -> Result<FieldItem, Failure> {
    let referred = if refers {
        value.as_str().and_then(referred)
    } else {
        None
    };
    Ok(match referred {
        Some(id) => FieldItem::Reference {
            reference: lookup.reference(&id)?,
        },
        None => FieldItem::Text {
            text: written(value),
        },
    })
}

fn shown_fields(lookup: &Lookup, version: &Version) -> Result<Vec<ShownField>, Failure> {
    let references: Vec<&str> = match KindOf::of(&version.envelope.kind) {
        Ok(kind) => kind.reference_keys().collect(),
        Err(_) => Vec::new(),
    };
    (version.fields.iter())
        .map(|(name, value)| {
            let refers = references.contains(&name.as_str());
            let items = match value {
                Value::Array(items) => items.as_slice(),
                one => std::slice::from_ref(one),
            };
            Ok(ShownField {
                name: name.clone(),
                values: (items.iter())
                    .map(|value| item(lookup, value, refers))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect()
}

fn links(lookup: &Lookup, version: &Version) -> Result<Vec<Linked>, Failure> {
    (version.links.0.iter())
        .map(|(text, stored)| {
            let target = lookup.linked_now(text, stored.as_ref())?;
            Ok(Linked {
                text: text.clone(),
                target: (target.as_ref())
                    .map(|id| lookup.reference(id))
                    .transpose()?,
            })
        })
        .collect()
}

fn shown_version(lookup: &Lookup, version: &Version, head: bool) -> Result<ShownVersion, Failure> {
    Ok(ShownVersion {
        version: version.id.clone(),
        written: version.envelope.written.clone(),
        machine: lookup.topic_reference(&version.envelope.machine)?,
        change: version.envelope.change.clone(),
        parents: version.envelope.parents.clone(),
        head,
        ended: ended_of(version),
        text: shown_text(lookup, version)?,
        fields: shown_fields(lookup, version)?,
        body: version.body.clone(),
        links: links(lookup, version)?,
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
/// nothing or several documents or versions, and a version whose file does not read. An ended
/// or unreadable document is shown too.
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
                .map(|head| shown_version(&lookup, head, true))
                .collect::<Result<_, _>>()?;
            let speaking = row_head(&document).map(|(_, record)| record);
            Ok(Shown::Document(ShownDocument {
                kind: document.kind().map(ToString::to_string),
                reads_as: (speaking.as_ref())
                    .map(|record| record.content.kind())
                    .filter(|kind| kind_of(&document) == Some(*kind)),
                forked: is_forked(&document),
                ended: speaking.as_ref().and_then(ended_for),
                former: lookup.former_addresses(&id)?,
                unreadable: unread(&document),
                document: id,
                label,
                heads,
            }))
        }
        Resolved::Version { document, version } => {
            let held_document = lookup.document(&document)?;
            let version = held(&held_document, &version, address)?;
            let head = is_head(&held_document, version);
            Ok(Shown::Version(Compared {
                label: lookup.label(&document)?,
                sides: against_parents(&lookup, &held_document, version)?,
                version: shown_version(&lookup, version, head)?,
                document,
            }))
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
        .map(|version| {
            Ok(HistoryRow {
                version: version.id.clone(),
                written: version.envelope.written.clone(),
                machine: lookup.topic_reference(&version.envelope.machine)?,
                change: version.envelope.change.clone(),
                parents: version.envelope.parents.clone(),
                head: is_head(&document, version),
                ended: ended_of(version),
            })
        })
        .collect::<Result<_, Failure>>()?;
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

fn against_parents(
    lookup: &Lookup,
    document: &Document,
    version: &Version,
) -> Result<Vec<Side>, Failure> {
    sides(document, &version.envelope.parents, |parent| {
        shown_text(lookup, parent)
    })
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
            sides: against_parents(&lookup, &document, version)?,
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
        ENDED, TOPIC, World, fact, fact_fields, fork, head, refused, to, topic, topic_fields,
        world, world_with_atlas, written_at,
    };
    use crate::domain::draft::Draft;
    use crate::domain::id::VersionId;
    use crate::domain::ports::{Clock, Drafts, Store};
    use crate::domain::testing::{after, first, lantern};
    use crate::domain::version::ReadError;

    fn shown_document(world: &World, address: &str) -> ShownDocument {
        match show(&world.deps(), address).unwrap() {
            Shown::Document(document) => document,
            Shown::Version(_) => panic!("expected a document"),
        }
    }

    #[test]
    fn a_document_whose_speaking_head_is_of_another_kind_than_it_is_stored_as_reads_as_none() {
        let (world, _) = world();
        let mixed = DocumentId::from_bytes([0x5a; 16]);
        let as_topic = first(&mixed, "topic", &topic_fields("compass", ""), "\n");
        let as_fact = (0..64)
            .map(|round| {
                first(
                    &mixed,
                    "fact",
                    "name = \"compass\"\n",
                    &format!("{round}\n"),
                )
            })
            .find(|version| version.id < as_topic.id)
            .expect("a version that sorts before the topic's");
        world.store.put(&as_topic).unwrap();
        world.store.put(&as_fact).unwrap();

        let shown = shown_document(&world, mixed.as_str());
        assert_eq!(shown.kind.as_deref(), Some("fact"));
        assert_eq!(shown.label, "compass");
        assert_eq!(shown.reads_as, None);
    }

    #[test]
    fn a_shown_document_reads_as_the_kind_its_listing_row_has_and_as_none_when_no_head_reads() {
        let (world, lantern, atlas) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        world.amend(&atlas, &topic_fields("atlas", ENDED), "\n");
        let gadget = world.put("gadget", "size = 4\n", "\n");
        let deps = world.deps();

        let listed = crate::app::list::topics(&deps, None, true).unwrap();
        for (address, id) in [("lantern", &lantern), ("atlas", &atlas)] {
            let row = (listed.iter().map(|topic| &topic.row)).find(|row| row.document == *id);
            let shown = shown_document(&world, address);
            assert_eq!(shown.reads_as, row.map(|row| row.kind), "{address}");
            assert_eq!(shown.reads_as, Some(KindOf::Topic), "{address}");
        }

        let facts = crate::app::list::facts(&deps, None, false).unwrap();
        assert_eq!(facts[0].row.document, relay);
        assert_eq!(
            shown_document(&world, "lantern/relay-pin").reads_as,
            Some(facts[0].row.kind)
        );

        let unknown = shown_document(&world, gadget.short());
        assert_eq!(
            (unknown.kind.as_deref(), unknown.reads_as),
            (Some("gadget"), None)
        );
        let unread = world.put("topic", "name = 4\n", "\n");
        let unread = shown_document(&world, unread.as_str());
        assert_eq!(
            (unread.kind.as_deref(), unread.reads_as),
            (Some("topic"), None)
        );
    }

    fn walks_and_lookups(address: &dyn Fn(&DocumentId) -> String) -> (usize, usize) {
        let (world, lantern) = world();
        let world = world.counted();
        let deps = world.deps();
        show(&deps, &address(&lantern)).unwrap();
        let lookups = world.store.prefixes.get();
        (world.store.scans() - lookups, lookups)
    }

    #[test]
    fn a_document_shown_by_its_whole_id_is_found_by_one_lookup_and_no_walk_of_the_store() {
        assert_eq!(walks_and_lookups(&ToString::to_string), (0, 1));
        let (walks, _) = walks_and_lookups(&|_| "lantern".to_owned());
        assert!(walks > 0);
    }

    fn compared(world: &World, address: &str) -> Compared {
        match show(&world.deps(), address).unwrap() {
            Shown::Version(compared) => compared,
            Shown::Document(_) => panic!("expected a version"),
        }
    }

    fn shown_version(world: &World, address: &str) -> ShownVersion {
        compared(world, address).version
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
            assert_eq!(head.machine, to(&world.machine(), "desk"));
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
            to(&relay, relay.short())
        );
        assert_eq!(
            shown_document(&world, "lantern").heads[0].machine,
            to(&absent, absent.short())
        );
        let history = history(&world.deps(), "lantern").unwrap();
        let machines: Vec<&str> = history
            .versions
            .iter()
            .map(|version| version.machine.label.as_str())
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
            let compared = compared(&world, prefix);
            assert_eq!(compared.document, lantern);
            assert_eq!(compared.label, "lantern");
            let shown = compared.version;
            assert_eq!(shown.version, second);
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

    fn text(text: &str) -> FieldItem {
        FieldItem::Text {
            text: text.to_owned(),
        }
    }

    fn reference(document: &DocumentId, label: &str) -> FieldItem {
        FieldItem::Reference {
            reference: to(document, label),
        }
    }

    fn field(name: &str, value: FieldItem) -> ShownField {
        items(name, vec![value])
    }

    fn items(name: &str, values: Vec<FieldItem>) -> ShownField {
        ShownField {
            name: name.to_owned(),
            values,
        }
    }

    #[test]
    fn a_version_carries_its_fields_in_stored_order_and_its_body_apart_from_them() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay-pin", "idea = true\n");
        let shown = shown_document(&world, "lantern/relay-pin");
        let head = &shown.heads[0];
        assert_eq!(head.body, "The relay.\n");
        assert_eq!(
            head.fields,
            [
                field("name", text("relay-pin")),
                field("topic", reference(&lantern, "lantern")),
                field("created", text("2026-09-04")),
                field("confirmed", text("2026-09-04")),
                field("summary", text("s")),
                field("idea", text("true")),
            ]
        );
        assert!(head.text.ends_with("idea = true\n+++\nThe relay.\n"));
    }

    #[test]
    fn a_field_that_refers_to_documents_carries_each_by_id_with_the_label_the_text_shows() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let absent = DocumentId::from_bytes([0xb8; 16]);
        let atlas = topic(&world, "atlas", "");
        let saved = world.amend(
            &atlas,
            &format!(
                "name = \"atlas\"\nformer_names = [\"map\", \"chart\"]\n{TOPIC}{ENDED}\
                 ended_by = \"{relay}\"\npart_of = [\"{absent}\", \"{lantern}\"]\nuses = []\n"
            ),
            "\n",
        );
        let shown = shown_version(&world, saved.as_str());
        assert_eq!(
            shown.fields,
            [
                field("name", text("atlas")),
                items("former_names", vec![text("map"), text("chart")]),
                field("created", text("2026-09-04")),
                field("summary", text("s")),
                field("ended", text("retired")),
                field("ended_on", text("2026-10-09")),
                field("note", text("n")),
                field("ended_by", reference(&relay, "lantern/relay-pin")),
                items(
                    "part_of",
                    vec![
                        reference(&absent, absent.short()),
                        reference(&lantern, "lantern")
                    ]
                ),
                items("uses", Vec::new()),
            ]
        );
        assert!(
            shown.text.contains("ended_by = \"lantern/relay-pin\""),
            "{}",
            shown.text
        );
        assert_eq!(shown.body, "\n");
    }

    #[test]
    fn an_item_is_a_reference_only_where_the_kind_holds_ids_and_the_item_is_an_id() {
        let (world, lantern) = world();
        let gadget = world.put(
            "gadget",
            &format!("size = 4\ntopic = \"{lantern}\"\n"),
            "\n",
        );
        assert_eq!(
            shown_document(&world, gadget.as_str()).heads[0].fields,
            [
                field("size", text("4")),
                field("topic", text(lantern.as_str()))
            ]
        );

        let loose = world.put(
            "topic",
            &format!(
                "name = \"loose\"\n{TOPIC}part_of = [\"{lantern}\", \"loose\", 4]\n\
                 uses = \"lantern\"\nended_by = 4\n"
            ),
            "\n",
        );
        let fields = shown_document(&world, loose.as_str()).heads[0]
            .fields
            .clone();
        assert_eq!(
            fields[3..],
            [
                items(
                    "part_of",
                    vec![reference(&lantern, "lantern"), text("loose"), text("4")]
                ),
                field("uses", text("lantern")),
                field("ended_by", text("4")),
            ]
        );
        let shown = &shown_document(&world, loose.as_str()).heads[0];
        assert!(
            shown.text.contains("part_of = [\"lantern\", \"loose\", 4]"),
            "{}",
            shown.text
        );
    }

    fn linked(text: &str, target: Option<(&DocumentId, &str)>) -> Linked {
        Linked {
            text: text.to_owned(),
            target: target.map(|(document, label)| to(document, label)),
        }
    }

    #[test]
    fn a_version_carries_its_links_by_text_with_the_label_each_document_has_now() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let atlas = topic(&world, "atlas", "");
        let unheld = DocumentId::from_bytes([0x42; 16]);
        let root = head(&world, &atlas).id;
        let saved = world.amend_linking(
            &atlas,
            &topic_fields("atlas", ""),
            "See [[lantern/relay-pin]], [[unknown]], [[Not A Name]] and [[phone]].\n",
            &[
                ("unknown", None),
                ("phone", Some(&unheld)),
                ("lantern/relay-pin", Some(&relay)),
                ("Not A Name", None),
            ],
        );
        let stored = [
            linked("Not A Name", None),
            linked("lantern/relay-pin", Some((&relay, "lantern/relay-pin"))),
            linked("phone", Some((&unheld, unheld.short()))),
            linked("unknown", None),
        ];
        assert_eq!(shown_document(&world, "atlas").heads[0].links, stored);
        assert_eq!(shown_version(&world, saved.as_str()).links, stored);
        assert_eq!(shown_version(&world, root.as_str()).links, []);

        world.amend(
            &relay,
            &fact_fields(
                &lantern,
                "relay-coil",
                "s",
                "former_names = [\"relay-pin\"]\n",
            ),
            "\n",
        );
        let renamed = linked("lantern/relay-pin", Some((&relay, "lantern/relay-coil")));
        assert_eq!(shown_document(&world, "atlas").heads[0].links[1], renamed);
    }

    #[test]
    fn the_text_of_a_link_is_its_target_without_the_spaces_around_it() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", "");
        checkout(&world.deps(), "lantern").unwrap();
        let mut draft = world.drafts.read(&lantern).unwrap().unwrap();
        "See [[ atlas ]] and [[  Not A Name ]].\n".clone_into(&mut draft.body);
        world.drafts.write(&draft).unwrap();
        crate::app::save::save(&world.deps(), "lantern").unwrap();
        assert_eq!(
            shown_document(&world, "lantern").heads[0].links,
            [
                linked("Not A Name", None),
                linked("atlas", Some((&atlas, "atlas")))
            ]
        );
    }

    #[test]
    fn a_link_to_a_document_with_no_version_that_reads_is_labeled_by_its_short_id() {
        let (world, lantern) = world();
        let stray = DocumentId::from_bytes([0xc4; 16]);
        world
            .store
            .plant_unreadable(&stray, VersionId::of(b"x"), ReadError::Corrupt);
        world.amend_linking(
            &lantern,
            &topic_fields("lantern", ""),
            "[[phone]]\n",
            &[("phone", Some(&stray))],
        );
        assert_eq!(
            shown_document(&world, "lantern").heads[0].links,
            [linked("phone", Some((&stray, stray.short()))),]
        );
    }

    fn questions_showing(links: &[(&str, bool)]) -> (usize, usize) {
        let (world, lantern, atlas) = world_with_atlas();
        let links: Vec<(&str, Option<&DocumentId>)> = (links.iter())
            .map(|(text, stored)| (*text, stored.then_some(&atlas)))
            .collect();
        world.amend_linking(&lantern, &topic_fields("lantern", ""), "\n", &links);
        let world = world.counted();
        let deps = world.deps();
        show(&deps, lantern.as_str()).unwrap();
        (world.store.scans(), world.store.questions())
    }

    #[test]
    fn a_shown_document_asks_the_store_about_a_link_only_where_none_was_stored() {
        let plain = questions_showing(&[]);
        assert_eq!(
            questions_showing(&[("atlas", true)]),
            (plain.0, plain.1 + 1)
        );
        let (scans, questions) = questions_showing(&[("atlas", false), ("unknown", true)]);
        assert_eq!((scans, questions), (plain.0 + 1, plain.1 + 1));
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

    #[test]
    fn a_document_is_ended_when_the_head_that_speaks_for_it_is() {
        let (world, lantern) = world();
        assert_eq!(shown_document(&world, "lantern").ended, None);
        let root = head(&world, &lantern);
        let at = |hour: &str, rest: &str| {
            written_at(
                &world,
                &root,
                &topic_fields("lantern", rest),
                &format!("2026-10-09T{hour}:00:00+01:00"),
                "\n",
                None,
            )
        };
        at("10", "");
        at("11", ENDED);
        let forked = shown_document(&world, "lantern");
        assert!(forked.forked);
        assert_eq!(forked.ended, None);
        let ended: Vec<Option<&str>> = (forked.heads.iter())
            .map(|head| head.ended.as_deref())
            .collect();
        assert!(ended.contains(&Some("retired")) && ended.contains(&None));

        let atlas = topic(&world, "atlas", "");
        let root = head(&world, &atlas);
        for (hour, reason) in [("10", "retired"), ("11", "merged")] {
            written_at(
                &world,
                &root,
                &topic_fields(
                    "atlas",
                    &format!("ended = \"{reason}\"\nended_on = 2026-10-09\nnote = \"n\"\n"),
                ),
                &format!("2026-10-09T{hour}:00:00+01:00"),
                "\n",
                None,
            );
        }
        let closed = shown_document(&world, "atlas");
        assert!(closed.forked);
        assert_eq!(closed.ended.as_deref(), Some("merged"));

        let phone = topic(&world, "phone", "");
        world.amend(&phone, &topic_fields("phone", ENDED), "\n");
        assert_eq!(
            shown_document(&world, "phone").ended.as_deref(),
            Some("retired")
        );
    }

    #[test]
    fn a_version_shown_carries_what_a_diff_of_it_gives() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        let heads = fork(&world, &lantern, &topic_fields("lantern", ""));
        let joined = world.amend(&lantern, &topic_fields("lantern", ""), "joined\n");
        for version in [&root.id, &heads[0], &joined] {
            let Shown::Version(compared) = show(&world.deps(), version.as_str()).unwrap() else {
                panic!("expected a version");
            };
            let diff = diff(&world.deps(), version.as_str(), None).unwrap();
            assert_eq!(compared.sides, diff.sides);
            assert_eq!(compared.version.text, diff.after);
        }
        let Shown::Version(compared) = show(&world.deps(), joined.as_str()).unwrap() else {
            panic!("expected a version");
        };
        let against: Vec<Option<&VersionId>> = (compared.sides.iter())
            .map(|side| side.against.as_ref())
            .collect();
        assert_eq!(against, [Some(&heads[0]), Some(&heads[1])]);
        assert_eq!(compared.version.parents, heads);
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

    fn history_questions(versions: usize) -> usize {
        let (world, lantern, atlas) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        for round in 1..versions {
            world.amend_linking(
                &relay,
                &fact_fields(&lantern, "relay-pin", "s", ""),
                &format!("{round} [[atlas]]\n"),
                &[("atlas", Some(&atlas))],
            );
        }
        let world = world.counted();
        let deps = world.deps();
        let history = history(&deps, relay.as_str()).unwrap();
        assert_eq!(history.versions.len(), versions);
        assert_eq!(history.label, "lantern/relay-pin");
        assert_eq!(history.versions[0].machine.label, "desk");
        assert_eq!(world.store.scans(), 1);
        world.store.questions()
    }

    #[test]
    fn a_history_asks_the_store_no_more_for_a_document_of_many_versions() {
        assert_eq!(history_questions(1), 3);
        assert_eq!(history_questions(50), 3);
    }

    #[test]
    fn a_history_row_is_what_names_a_version_and_nothing_it_holds() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        let ended = world.amend(&lantern, &topic_fields("lantern", ENDED), "gone\n");
        let history = history(&world.deps(), "lantern").unwrap();
        assert_eq!(
            history.versions,
            [
                HistoryRow {
                    version: ended,
                    written: world.clock.now(),
                    machine: to(&world.machine(), "desk"),
                    change: "save".to_owned(),
                    parents: vec![root.id.clone()],
                    head: true,
                    ended: Some("retired".to_owned()),
                },
                HistoryRow {
                    version: root.id.clone(),
                    written: root.envelope.written.clone(),
                    machine: to(&world.machine(), "desk"),
                    change: "new".to_owned(),
                    parents: Vec::new(),
                    head: false,
                    ended: None,
                },
            ]
        );
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
