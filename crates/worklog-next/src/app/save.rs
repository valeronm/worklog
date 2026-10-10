use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use toml::Value;

use crate::app::draft::{draft_for, names, spelled};
use crate::app::heads::{heads_not_ended, read_or_skip, row_head, texts, topic_edges};
use crate::app::lookup::{Lookup, forked, no_head_reads, unreadable, with_article};
use crate::app::rules::{closing_edge, fits, holds_topic_open, name_is_free_as};
use crate::app::{Deps, Failed, Failure, Written, counted, kind_named};
use crate::domain::document::{Document, State};
use crate::domain::draft::Draft;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::schema::address::Found;
use crate::domain::schema::graph::Edges;
use crate::domain::schema::kind::Reference;
use crate::domain::schema::{Content, KindOf, Record, links, step, translate};
use crate::domain::version::{Envelope, Fields, Kind, Links, Version};

const NEW: &str = "new";
const SAVE: &str = "save";
const RESOLVE: &str = "resolve";

/// A refusal leaves the draft in place; a stored version deletes it.
pub fn save(deps: &Deps, address: &str) -> Result<Written, Failure> {
    let lookup = Lookup::new(deps.store);
    let draft = draft_for(deps, &lookup, address)?;
    let (id, body) = (&draft.document, draft.body.as_str());
    let target = Target::of(&lookup, id, &draft.kind, &draft.fields, Origin::Draft)?;
    let label = target.change(&draft)?;
    let fork = if label == RESOLVE {
        Fork::Resolved
    } else {
        Fork::Refused
    };
    target.admits(fork)?;
    let fields = target.ids(&draft)?;
    let next = target.next(deps, &fields, body)?;
    let written = target
        .checked(deps, &next, body, label, fork)?
        .store(deps)?;
    deps.drafts.delete(id)?;
    Ok(written)
}

/// Stores `next` as the document's next version, or its first, once it passes the checks
/// its step calls for: the references no head that is not ended held, the name in its scope
/// and a topic's cycle, or for an ended topic what still depends on it. Refuses a forked or
/// unreadable document, one of another kind, and a version equal to the only head.
pub(super) fn admit(
    deps: &Deps,
    lookup: &Lookup,
    id: &DocumentId,
    next: &Record,
    body: &str,
    label: &str,
) -> Result<Written, Failure> {
    let kind = kind_named(next.content.kind());
    Target::of(lookup, id, &kind, &next.fields(), Origin::Direct)?
        .checked(deps, next, body, label, Fork::Refused)?
        .store(deps)
}

// `fields` are shaped as a draft holds them, with references already ids.
pub(super) fn stepped(
    deps: &Deps,
    lookup: &Lookup,
    id: &DocumentId,
    kind: &Kind,
    fields: &Fields,
    body: &str,
    label: &str,
) -> Result<Admitted, Failure> {
    let target = Target::of(lookup, id, kind, fields, Origin::Direct)?;
    target.admits(Fork::Refused)?;
    let next = target.next(deps, fields, body)?;
    target.checked(deps, &next, body, label, Fork::Refused)
}

/// A version that passed every check against the store as it was, not stored yet.
pub(super) struct Admitted(Version);

impl Admitted {
    pub(super) fn store(self, deps: &Deps) -> Result<Written, Failure> {
        deps.store.put(&self.0)?;
        Ok(Written {
            document: self.0.envelope.document,
            version: self.0.id,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fork {
    Refused,
    Resolved,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Origin {
    Draft,
    Direct,
}

struct Target<'a, 'b> {
    lookup: &'a Lookup<'b>,
    document: Rc<Document>,
    kind: Kind,
    what: String,
    origin: Origin,
}

impl<'a, 'b> Target<'a, 'b> {
    fn of(
        lookup: &'a Lookup<'b>,
        id: &DocumentId,
        kind: &Kind,
        fields: &Fields,
        origin: Origin,
    ) -> Result<Target<'a, 'b>, Failure> {
        let document = lookup.document(id)?;
        let what = match lookup.address(id)? {
            Some(address) => address,
            None => match spelled_by(lookup, kind, fields)? {
                Some(spelled) => spelled,
                None if origin == Origin::Direct && document.heads().is_empty() => {
                    format!("new {kind}")
                }
                None => id.short().to_owned(),
            },
        };
        Ok(Target {
            lookup,
            document,
            kind: kind.clone(),
            what,
            origin,
        })
    }

    fn refusal(&self, why: impl fmt::Display) -> Failure {
        Failure::at(&self.what, why)
    }

    fn change(&self, draft: &Draft) -> Result<&'static str, Failure> {
        let parents: BTreeSet<&VersionId> = draft.parents.iter().collect();
        let heads: BTreeSet<&VersionId> =
            self.document.heads().iter().map(|head| &head.id).collect();
        match self.document.state() {
            State::Absent if parents.is_empty() => Ok(NEW),
            State::Live(_) if parents.is_empty() => {
                Err(self.refusal("exists; the draft names no parent"))
            }
            State::Live(_) if parents == heads => Ok(SAVE),
            State::Forked(_) if parents == heads => Ok(RESOLVE),
            _ => Err(self.refusal("moved on since the draft was checked out")),
        }
    }

    fn admits(&self, fork: Fork) -> Result<(), Failure> {
        if !self.document.unreadable().is_empty() {
            return Err(unreadable(&self.what));
        }
        if let Some(stored) = self.document.kind()
            && *stored != self.kind
        {
            return Err(self.refusal(format!(
                "is {}, not {}",
                with_article(stored.as_str()),
                with_article(self.kind.as_str())
            )));
        }
        match self.document.state() {
            State::Forked(_) if fork == Fork::Refused => Err(forked(&self.what)),
            _ => Ok(()),
        }
    }

    // A name resolved afresh can mean another document than the one a held reference shows.
    fn keeping_held(&self, fields: &Fields, kind: KindOf) -> Result<Fields, Failure> {
        let mut kept = fields.clone();
        let heads = self.document.heads();
        for reference in kind.references() {
            let key = reference.key;
            let mut shown = Vec::new();
            for text in held(&heads, key) {
                if let Ok(id) = DocumentId::parse(text)
                    && let Some(address) = self.lookup.address(&id)?
                {
                    shown.push((address, self.lookup.former_addresses(&id)?, text));
                }
            }
            let mut keep = |text: &mut String| {
                let at = shown
                    .iter()
                    .position(|(address, _, _)| address == text)
                    .or_else(|| {
                        shown
                            .iter()
                            .position(|(_, former, _)| former.contains(text))
                    });
                if let Some(at) = at {
                    shown.remove(at).2.clone_into(text);
                }
            };
            match kept.get_mut(key) {
                Some(Value::String(text)) => keep(text),
                Some(Value::Array(items)) => {
                    for item in items {
                        if let Value::String(text) = item {
                            keep(text);
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(kept)
    }

    fn ids(&self, draft: &Draft) -> Result<Fields, Failure> {
        let kind = KindOf::of(&draft.kind).map_err(|error| self.refusal(error))?;
        let kept = self.keeping_held(&draft.fields, kind)?;
        let failed = Failed::default();
        let turned = translate::to_ids(&kept, kind, |name| match self.lookup.find(name) {
            Ok(found) => found,
            Err(Failure::Usage(_)) => Found::None,
            Err(failure) => {
                failed.keep(failure);
                Found::None
            }
        });
        failed.done()?;
        turned.map_err(|error| self.refusal(error))
    }

    fn next(&self, deps: &Deps, fields: &Fields, body: &str) -> Result<Record, Failure> {
        let stepped = match self.document.state() {
            State::Absent => step::next(&self.kind, fields, None, true, deps.today()?),
            State::Live(head) => step::next(
                &self.kind,
                fields,
                Some(&head.fields),
                head.body != body,
                deps.today()?,
            ),
            State::Forked(heads) => {
                let Some((speaking, _)) = row_head(&self.document) else {
                    return Err(no_head_reads(&self.what));
                };
                let others: Vec<Record> = (heads.into_iter())
                    .filter(|head| head.id != speaking.id)
                    .filter_map(read_or_skip)
                    .collect();
                step::joined(
                    &self.kind,
                    fields,
                    &speaking.fields,
                    &others,
                    speaking.body != body,
                    deps.today()?,
                )
            }
        };
        stepped.map_err(|error| self.refusal(error))
    }

    fn checked(
        &self,
        deps: &Deps,
        next: &Record,
        body: &str,
        label: &str,
        fork: Fork,
    ) -> Result<Admitted, Failure> {
        self.admits(fork)?;
        let fields = next.fields();
        if next.ending.is_some() {
            if next.content.kind() == KindOf::Topic {
                self.dependents()?;
            }
        } else {
            self.references(next.content.kind(), &fields)?;
            name_is_free_as(self.lookup, self.document.id(), &self.what, &next.content)?;
            self.closes_no_cycle(&next.content)?;
        }
        if fork == Fork::Refused
            && let [head] = self.document.heads().as_slice()
            && head.fields == fields
            && head.body == body
        {
            return Err(match self.origin {
                Origin::Draft => self.refusal("the draft equals its parent"),
                Origin::Direct => self.refusal("nothing would change"),
            });
        }
        self.composed(deps, fields, body, label).map(Admitted)
    }

    fn dependents(&self) -> Result<(), Failure> {
        let mut named = Vec::new();
        let others = KindOf::ALL
            .into_iter()
            .filter(|kind| *kind != KindOf::Topic);
        for kind in others.chain([KindOf::Topic]) {
            let mut holders = BTreeSet::new();
            for reference in kind.references() {
                if holds_topic_open(reference) {
                    let id = self.document.id();
                    let held = self.lookup.holders(kind, reference.key, &[id], false)?;
                    holders.extend(held.iter().map(|holder| holder.id().clone()));
                }
            }
            if !holders.is_empty() {
                named.push(counted(holders.len(), kind.word(), kind.plural()));
            }
        }
        if named.is_empty() {
            return Ok(());
        }
        Err(self.refusal(format!("still has {}", named.join(", "))))
    }

    fn references(&self, kind: KindOf, fields: &Fields) -> Result<(), Failure> {
        let before = heads_not_ended(&self.document);
        for reference in kind.references() {
            let key = reference.key;
            let held = held(&before, key);
            for text in texts(fields, key) {
                if !held.contains(&text) {
                    self.reference(reference, text)
                        .map_err(|failure| match failure {
                            Failure::Refused(why) => self.refusal(format!("{key}: {why}")),
                            other => other,
                        })?;
                }
            }
        }
        Ok(())
    }

    fn reference(&self, reference: &Reference, text: &str) -> Result<(), Failure> {
        let id = DocumentId::parse(text).map_err(|error| Failure::Refused(error.to_string()))?;
        let shown = self.lookup.label(&id)?;
        let misfit = fits(
            self.lookup,
            &id,
            reference.target,
            reference.unended_when_made,
        )?;
        match misfit {
            Some(misfit) => Err(Failure::Refused(format!("{shown} {misfit}"))),
            None => Ok(()),
        }
    }

    fn closes_no_cycle(&self, content: &Content) -> Result<(), Failure> {
        let Content::Topic(topic) = content else {
            return Ok(());
        };
        let failed = Failed::default();
        let edges_of = |id: &DocumentId| match self.lookup.document(id) {
            Ok(document) => Some(topic_edges(&document)),
            Err(failure) => {
                failed.keep(failure);
                None
            }
        };
        let proposed = Edges {
            part_of: topic.part_of.clone(),
            uses: topic.uses.clone(),
        };
        let closing = closing_edge(self.document.id(), &proposed, edges_of);
        failed.done()?;
        match closing {
            Some(key) => {
                Err(self.refusal(format!("{key}: would make {} part of itself", topic.name)))
            }
            None => Ok(()),
        }
    }

    fn linked(&self, fields: &Fields, body: &str) -> Result<Links, Failure> {
        let summary = fields
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut resolved = BTreeMap::new();
        for target in links::links(summary).into_iter().chain(links::links(body)) {
            let found = match self.lookup.find(&target) {
                Ok(Found::One(id)) => Some(id),
                Ok(_) | Err(Failure::Usage(_)) => None,
                Err(failure) => return Err(failure),
            };
            resolved.insert(target, found);
        }
        Ok(Links(resolved))
    }

    fn composed(
        &self,
        deps: &Deps,
        fields: Fields,
        body: &str,
        label: &str,
    ) -> Result<Version, Failure> {
        let links = self.linked(&fields, body)?;
        let (written, machine) = (deps.clock.now(), deps.machine()?);
        let heads = self.document.heads();
        let envelope = Envelope::following(&heads, written.clone(), machine.clone(), label)
            .unwrap_or_else(|| {
                let id = self.document.id().clone();
                Envelope::first(id, self.kind.clone(), written, machine, label)
            });
        Version::compose(envelope, fields, links, body.to_owned())
            .map_err(|error| self.refusal(error))
    }
}

fn spelled_by(lookup: &Lookup, kind: &Kind, fields: &Fields) -> Result<Option<String>, Failure> {
    let named = match KindOf::of(kind) {
        Ok(kind) => names(lookup, fields, kind)?,
        Err(_) => fields.clone(),
    };
    Ok(spelled(kind, &named))
}

fn held<'v>(heads: &[&'v Version], key: &str) -> Vec<&'v str> {
    let mut held = Vec::new();
    for head in heads {
        for text in texts(&head.fields, key) {
            if !held.contains(&text) {
                held.push(text);
            }
        }
    }
    held
}

#[cfg(test)]
mod tests {
    use toml::Value;

    use super::*;
    use crate::app::draft::{self, New};
    use crate::app::testing::{
        ENDED, ENDED_FACT, TOPIC, World, entry, fact, fork, head, record, refused, topic, world,
    };
    use crate::domain::draft::Draft;
    use crate::domain::ports::{Drafts, Ids, Store};
    use crate::domain::schema::address::Found;
    use crate::domain::schema::{Date, Ending, Reason, step};
    use crate::domain::testing::{after, first};
    use crate::domain::version::ReadError;

    fn end(world: &World, id: &DocumentId, name: &str) {
        world.amend(id, &format!("name = \"{name}\"\n{TOPIC}{ENDED}"), "\n");
    }

    fn edit(world: &World, id: &DocumentId, change: impl FnOnce(&mut Draft)) {
        let mut draft = world.drafts.read(id).unwrap().expect("a draft");
        change(&mut draft);
        world.drafts.write(&draft).unwrap();
    }

    fn set(world: &World, id: &DocumentId, key: &str, value: impl Into<Value>) {
        edit(world, id, |draft| {
            draft.fields.insert(key.to_owned(), value.into());
        });
    }

    fn list(names: &[&str]) -> Value {
        Value::Array(names.iter().map(|name| Value::from(*name)).collect())
    }

    fn day(text: &str) -> Value {
        Date::parse(text).unwrap().value()
    }

    fn open(world: &World, id: &DocumentId) -> bool {
        world.drafts.read(id).unwrap().is_some()
    }

    fn new_topic(world: &World, name: &str) -> DocumentId {
        let opened = draft::new(&world.deps(), &New::Topic { name }).unwrap();
        set(world, &opened.document, "summary", "A summary");
        opened.document
    }

    fn new_fact(world: &World, address: &str) -> DocumentId {
        let what = New::Fact {
            address,
            idea: false,
        };
        let opened = draft::new(&world.deps(), &what).unwrap();
        set(world, &opened.document, "summary", "A summary");
        opened.document
    }

    fn kind(word: &str) -> Kind {
        Kind::parse(word).unwrap()
    }

    fn store(
        world: &World,
        id: &DocumentId,
        kind: &Kind,
        fields: &Fields,
        body: &str,
        label: &str,
    ) -> Result<Written, Failure> {
        let deps = world.deps();
        let lookup = world.lookup();
        stepped(&deps, &lookup, id, kind, fields, body, label)?.store(&deps)
    }

    #[test]
    fn a_new_topic_is_stored_as_a_first_version_and_its_draft_is_gone() {
        let (world, _) = world();
        let phone = new_topic(&world, "phone");
        let written = save(&world.deps(), "phone").unwrap();
        assert_eq!(written.document, phone);
        let stored = head(&world, &phone);
        assert_eq!(stored.id, written.version);
        assert_eq!(
            toml::to_string(&stored.fields).unwrap(),
            "name = \"phone\"\ncreated = 2026-10-09\nsummary = \"A summary\"\n"
        );
        assert_eq!(stored.envelope.change, "new");
        assert!(stored.envelope.parents.is_empty());
        assert_eq!(Some(&stored.envelope.machine), world.host.0.as_ref());
        assert_eq!(stored.envelope.written, world.clock.0);
        assert!(!open(&world, &phone));
    }

    #[test]
    fn a_fact_stores_its_topic_as_an_id_and_an_edit_follows_the_first_version() {
        let (world, lantern) = world();
        let deps = world.deps();
        let relay = new_fact(&world, "lantern/relay-pin");
        set(&world, &relay, "confirmed", day("2026-09-04"));
        let created = save(&deps, "lantern/relay-pin").unwrap();
        let stored = head(&world, &relay);
        assert_eq!(
            stored.fields.get("topic").and_then(Value::as_str),
            Some(lantern.as_str())
        );
        assert_eq!(stored.fields.get("confirmed"), Some(&day("2026-09-04")));

        draft::checkout(&deps, "lantern/relay-pin").unwrap();
        set(&world, &relay, "summary", "The relay pin is fixed");
        let edited = save(&deps, "lantern/relay-pin").unwrap();
        let stored = head(&world, &relay);
        assert_eq!(stored.id, edited.version);
        assert_eq!(stored.envelope.change, "save");
        assert_eq!(stored.envelope.parents, vec![created.version]);
        assert_eq!(stored.fields.get("confirmed"), Some(&day("2026-10-09")));
        assert_eq!(stored.fields.get("created"), Some(&day("2026-10-09")));
        assert!(!open(&world, &relay));
    }

    #[test]
    fn a_rename_through_a_draft_keeps_the_old_name() {
        let (world, lantern) = world();
        let deps = world.deps();
        draft::checkout(&deps, "lantern").unwrap();
        set(&world, &lantern, "name", "lamp");
        save(&deps, "lantern").unwrap();
        let stored = head(&world, &lantern);
        assert_eq!(stored.fields.get("name"), Some(&Value::from("lamp")));
        assert_eq!(stored.fields.get("former_names"), Some(&list(&["lantern"])));
    }

    #[test]
    fn a_draft_with_no_parent_of_a_stored_document_is_refused() {
        let (world, _) = world();
        let phone = new_topic(&world, "phone");
        let stored = first(&phone, "topic", &format!("name = \"phone\"\n{TOPIC}"), "\n");
        world.store.put(&stored).unwrap();
        let text = refused(save(&world.deps(), "phone"));
        assert!(
            text.contains("phone: exists; the draft names no parent"),
            "{text}"
        );
        assert!(open(&world, &phone));
    }

    #[test]
    fn a_draft_whose_parent_is_no_longer_the_head_is_refused() {
        let (world, lantern) = world();
        let deps = world.deps();
        draft::checkout(&deps, "lantern").unwrap();
        set(&world, &lantern, "summary", "A lamp controller");
        world.amend(&lantern, &format!("name = \"lantern\"\n{TOPIC}"), "moved\n");
        let text = refused(save(&deps, "lantern"));
        assert!(
            text.contains("lantern: moved on since the draft was checked out"),
            "{text}"
        );
        assert!(open(&world, &lantern));
    }

    #[test]
    fn a_document_holding_an_unreadable_version_is_refused() {
        let (world, lantern) = world();
        let deps = world.deps();
        draft::checkout(&deps, "lantern").unwrap();
        set(&world, &lantern, "summary", "A lamp controller");
        let planted = first(&lantern, "topic", "name = \"x\"", "\n").id;
        world
            .store
            .plant_unreadable(&lantern, planted, ReadError::Corrupt);
        let text = refused(save(&deps, "lantern"));
        assert!(
            text.contains("lantern: holds a version this worklog cannot read"),
            "{text}"
        );
        assert!(open(&world, &lantern));
    }

    #[test]
    fn a_draft_equal_to_its_parent_is_refused() {
        let (world, lantern) = world();
        let deps = world.deps();
        draft::checkout(&deps, "lantern").unwrap();
        let text = refused(save(&deps, "lantern"));
        assert!(
            text.contains("lantern: the draft equals its parent"),
            "{text}"
        );
        assert!(open(&world, &lantern));
    }

    #[test]
    fn a_reference_names_a_document_of_its_kind() {
        let (world, lantern) = world();
        let deps = world.deps();
        entry(&world, "2026-10-08", "lamp-driver", &[&lantern]);
        let relay = new_fact(&world, "lantern/relay-pin");
        let address = relay.as_str();

        set(&world, &relay, "topic", "unknown");
        let text = refused(save(&deps, address));
        assert!(text.contains("`topic` names `unknown`"), "{text}");

        set(&world, &relay, "topic", "2026-10-08-lamp-driver");
        let text = refused(save(&deps, address));
        assert!(
            text.ends_with("/relay-pin: topic: 2026-10-08-lamp-driver is not a topic"),
            "{text}"
        );

        let nowhere = DocumentId::from_bytes([0xab; 16]);
        set(&world, &relay, "topic", nowhere.as_str());
        let text = refused(save(&deps, address));
        assert!(
            text.ends_with("/relay-pin: topic: abababab is no document"),
            "{text}"
        );
        assert!(open(&world, &relay));
    }

    #[test]
    fn an_ended_document_is_refused_only_as_a_new_reference() {
        let (world, _) = world();
        let deps = world.deps();
        let atlas = topic(&world, "atlas", "");
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&atlas]);
        end(&world, &atlas, "atlas");

        let relay = new_fact(&world, "lantern/relay-pin");
        set(&world, &relay, "topic", "atlas");
        let text = refused(save(&deps, relay.as_str()));
        assert!(
            text.contains("atlas/relay-pin: topic: atlas is ended"),
            "{text}"
        );

        draft::checkout(&deps, "2026-10-08-lamp-driver").unwrap();
        set(&world, &driver, "summary", "Wired the lamp driver");
        save(&deps, "2026-10-08-lamp-driver").unwrap();
        let stored = head(&world, &driver);
        assert_eq!(stored.fields.get("topics"), Some(&list(&[atlas.as_str()])));
    }

    #[test]
    fn a_name_is_taken_only_by_a_live_document_in_its_scope() {
        let (world, lantern) = world();
        let deps = world.deps();
        let atlas = topic(&world, "atlas", "");
        fact(&world, &lantern, "relay-pin", "");

        let phone = new_topic(&world, "phone");
        set(&world, &phone, "name", "lantern");
        let text = refused(save(&deps, phone.as_str()));
        assert!(
            text.contains("lantern: name: lantern is taken by lantern"),
            "{text}"
        );
        assert!(open(&world, &phone));

        let spare = new_fact(&world, "lantern/spare");
        set(&world, &spare, "name", "relay-pin");
        let text = refused(save(&deps, spare.as_str()));
        assert!(
            text.contains("lantern/relay-pin: name: relay-pin is taken by lantern/relay-pin"),
            "{text}"
        );

        let other = new_fact(&world, "atlas/relay-pin");
        save(&deps, "atlas/relay-pin").unwrap();
        assert_eq!(
            head(&world, &other).fields.get("topic"),
            Some(&Value::from(atlas.as_str()))
        );

        end(&world, &lantern, "lantern");
        save(&deps, phone.as_str()).unwrap();
        assert_eq!(world.lookup().find("lantern").unwrap(), Found::One(phone));
    }

    #[test]
    fn a_topic_is_never_made_part_of_itself() {
        let (world, lantern) = world();
        let deps = world.deps();
        let atlas = topic(&world, "atlas", &format!("part_of = [\"{lantern}\"]\n"));
        let phone = topic(&world, "phone", &format!("uses = [\"{atlas}\"]\n"));

        draft::checkout(&deps, "lantern").unwrap();
        set(&world, &lantern, "part_of", list(&["atlas"]));
        let text = refused(save(&deps, "lantern"));
        assert!(
            text.contains("lantern: part_of: would make lantern part of itself"),
            "{text}"
        );

        edit(&world, &lantern, |draft| {
            draft.fields.remove("part_of");
            draft.fields.insert("uses".to_owned(), list(&["phone"]));
        });
        let text = refused(save(&deps, "lantern"));
        assert!(
            text.contains("lantern: uses: would make lantern part of itself"),
            "{text}"
        );
        assert!(open(&world, &lantern));

        let desk = world.machine();
        draft::checkout(&deps, "phone").unwrap();
        set(&world, &phone, "part_of", list(&["lantern"]));
        set(&world, &phone, "uses", list(&["atlas", "desk"]));
        save(&deps, "phone").unwrap();
        let stored = head(&world, &phone);
        assert_eq!(
            stored.fields.get("part_of"),
            Some(&list(&[lantern.as_str()]))
        );
        assert_eq!(
            stored.fields.get("uses"),
            Some(&list(&[atlas.as_str(), desk.as_str()]))
        );
    }

    #[test]
    fn the_links_of_the_summary_and_the_body_are_stored_resolved_or_not() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let phone = new_topic(&world, "phone");
        set(&world, &phone, "summary", "Follows [[lantern/relay-pin]]");
        edit(&world, &phone, |draft| {
            "Again [[lantern/relay-pin]], and [[unknown]] and [[Not A Name]].\n"
                .clone_into(&mut draft.body);
        });
        save(&world.deps(), "phone").unwrap();
        let links = head(&world, &phone).links.0;
        assert_eq!(
            links.into_iter().collect::<Vec<_>>(),
            vec![
                ("Not A Name".to_owned(), None),
                ("lantern/relay-pin".to_owned(), Some(relay)),
                ("unknown".to_owned(), None),
            ]
        );
    }

    #[test]
    fn a_draft_without_a_summary_or_with_a_field_of_the_tools_is_refused() {
        let (world, _) = world();
        let deps = world.deps();
        let phone = new_topic(&world, "phone");
        set(&world, &phone, "summary", "");
        let text = refused(save(&deps, "phone"));
        assert!(text.contains("phone: `summary` is empty"), "{text}");

        set(&world, &phone, "summary", "A phone");
        set(&world, &phone, "created", day("2026-09-04"));
        let text = refused(save(&deps, "phone"));
        assert!(
            text.contains("phone: `created` is not a draft's to set"),
            "{text}"
        );
        assert!(open(&world, &phone));
    }

    #[test]
    fn a_fork_is_resolved_by_a_draft_of_every_head() {
        let (world, lantern) = world();
        let deps = world.deps();
        let heads = fork(&world, &lantern, &format!("name = \"lantern\"\n{TOPIC}"));
        let held = world.store.document(&lantern).unwrap();
        let mut draft = Draft::of(held.heads()[1]);
        draft.fields = step::shown(&draft.fields);
        "both\n".clone_into(&mut draft.body);
        world.drafts.write(&draft).unwrap();
        let text = refused(save(&deps, "lantern"));
        assert!(text.contains("lantern: moved on"), "{text}");

        edit(&world, &lantern, |draft| {
            draft.parents = heads.iter().rev().cloned().collect();
        });
        let written = save(&deps, "lantern").unwrap();
        let merged = head(&world, &lantern);
        assert_eq!(merged.id, written.version);
        assert_eq!(merged.envelope.change, "resolve");
        assert_eq!(merged.envelope.parents, heads);
        assert_eq!(merged.body, "both\n");
        assert!(!open(&world, &lantern));
    }

    #[test]
    fn storing_takes_a_first_version_and_refuses_one_equal_to_its_parent() {
        let (world, lantern) = world();
        let phone = world.ids.mint().unwrap();
        let topic = kind("topic");
        let blank: Fields = "name = \"phone\"\nsummary = \"\"\n".parse().unwrap();
        let text = refused(store(&world, &phone, &topic, &blank, "\n", "new"));
        assert!(text.contains("phone: `summary` is empty"), "{text}");

        let fields: Fields = "name = \"phone\"\nsummary = \"A phone\"\n".parse().unwrap();
        let written = store(&world, &phone, &topic, &fields, "\n", "new").unwrap();
        let stored = head(&world, &phone);
        assert_eq!(stored.id, written.version);
        assert_eq!(stored.fields.get("created"), Some(&day("2026-10-09")));
        assert_eq!(stored.envelope.change, "new");

        let same: Fields = "name = \"lantern\"\nsummary = \"s\"\n".parse().unwrap();
        let text = refused(store(&world, &lantern, &topic, &same, "\n", "rename"));
        assert!(text.contains("lantern: nothing would change"), "{text}");
        store(&world, &lantern, &topic, &same, "reworded\n", "rename").unwrap();
        assert_eq!(head(&world, &lantern).envelope.change, "rename");
    }

    #[test]
    fn stepping_and_admitting_refuse_a_fork() {
        let (world, lantern) = world();
        let deps = world.deps();
        let ended = record(&world, &lantern).end(ending()).unwrap();
        fork(&world, &lantern, &format!("name = \"lantern\"\n{TOPIC}"));
        let draft: Fields = "name = \"lantern\"\nsummary = \"both\"\n".parse().unwrap();
        let text = refused(store(
            &world,
            &lantern,
            &kind("topic"),
            &draft,
            "\n",
            "rename",
        ));
        assert!(
            text.contains("lantern: is forked; resolve it first"),
            "{text}"
        );
        let lookup = world.lookup();
        let text = refused(admit(&deps, &lookup, &lantern, &ended, "\n", "end"));
        assert!(
            text.contains("lantern: is forked; resolve it first"),
            "{text}"
        );
    }

    #[test]
    fn a_reference_the_parent_held_is_not_checked_again() {
        let (world, _) = world();
        let deps = world.deps();
        let nowhere = DocumentId::from_bytes([0xab; 16]);
        let other = DocumentId::from_bytes([0xcd; 16]);
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&nowhere]);
        draft::checkout(&deps, "2026-10-08-lamp-driver").unwrap();
        set(&world, &driver, "summary", "Wired the lamp driver");
        set(
            &world,
            &driver,
            "topics",
            list(&[nowhere.as_str(), other.as_str()]),
        );
        let text = refused(save(&deps, "2026-10-08-lamp-driver"));
        assert!(
            text.contains("2026-10-08-lamp-driver: topics: cdcdcdcd is no document"),
            "{text}"
        );

        set(&world, &driver, "topics", list(&[nowhere.as_str()]));
        save(&deps, "2026-10-08-lamp-driver").unwrap();
        let stored = head(&world, &driver);
        assert_eq!(
            stored.fields.get("topics"),
            Some(&list(&[nowhere.as_str()]))
        );
        assert_eq!(
            stored.fields.get("summary"),
            Some(&Value::from("Wired the lamp driver"))
        );
    }

    #[test]
    fn a_held_reference_keeps_its_document_when_another_takes_the_name() {
        let (world, _) = world();
        let deps = world.deps();
        let ended = topic(&world, "atlas", "");
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&ended]);
        end(&world, &ended, "atlas");
        topic(&world, "atlas", "");
        draft::checkout(&deps, "2026-10-08-lamp-driver").unwrap();
        let shown = world.drafts.read(&driver).unwrap().unwrap();
        assert_eq!(shown.fields.get("topics"), Some(&list(&["atlas"])));
        set(&world, &driver, "summary", "Wired the lamp driver");
        save(&deps, "2026-10-08-lamp-driver").unwrap();
        assert_eq!(
            head(&world, &driver).fields.get("topics"),
            Some(&list(&[ended.as_str()]))
        );
    }

    #[test]
    fn a_fork_with_an_ended_head_resolves_to_a_version_that_is_not_ended() {
        let (world, lantern) = world();
        let root = head(&world, &lantern);
        let live = after(&[&root], &format!("name = \"lantern\"\n{TOPIC}"), "live\n");
        let ended = (0..64)
            .map(|round| {
                after(
                    &[&root],
                    &format!("name = \"lantern\"\n{TOPIC}{ENDED}"),
                    &format!("ended {round}\n"),
                )
            })
            .find(|version| version.id < live.id)
            .unwrap();
        world.store.put(&live).unwrap();
        world.store.put(&ended).unwrap();
        let mut draft = Draft::of(&live);
        draft.parents = vec![ended.id.clone(), live.id.clone()];
        draft.fields = step::shown(&draft.fields);
        "both\n".clone_into(&mut draft.body);
        world.drafts.write(&draft).unwrap();
        save(&world.deps(), "lantern").unwrap();
        let merged = head(&world, &lantern);
        assert_eq!(merged.envelope.change, "resolve");
        assert_eq!(merged.envelope.parents, vec![ended.id, live.id]);
        assert_eq!(merged.fields.get("ended"), None);
        assert_eq!(merged.fields.get("created"), Some(&day("2026-09-04")));
    }

    #[test]
    fn a_version_of_another_kind_than_the_document_is_refused() {
        let (world, lantern) = world();
        let deps = world.deps();
        draft::checkout(&deps, "lantern").unwrap();
        edit(&world, &lantern, |draft| draft.kind = kind("fact"));
        let text = refused(save(&deps, "lantern"));
        assert!(text.contains("lantern: is a topic, not a fact"), "{text}");
        assert!(open(&world, &lantern));

        let fields: Fields = "name = \"lantern\"\nsummary = \"A lamp\"\n"
            .parse()
            .unwrap();
        let entry: Fields = format!(
            "name = \"lantern\"\ndate = 2026-10-09\nmachine = \"{}\"\nsummary = \"A lamp\"\n",
            world.machine()
        )
        .parse()
        .unwrap();
        let entry = Record::read(&kind("entry"), &entry).unwrap();
        let lookup = world.lookup();
        let unlike = [
            store(&world, &lantern, &kind("entry"), &fields, "\n", "save"),
            admit(&deps, &lookup, &lantern, &entry, "\n", "end"),
        ];
        for result in unlike {
            let text = refused(result);
            assert!(text.contains("lantern: is a topic, not an entry"), "{text}");
        }
    }

    #[test]
    fn a_name_a_forked_document_holds_is_taken() {
        let (world, lantern) = world();
        fork(&world, &lantern, &format!("name = \"lantern\"\n{TOPIC}"));
        let phone = new_topic(&world, "phone");
        set(&world, &phone, "name", "lantern");
        let text = refused(save(&world.deps(), phone.as_str()));
        assert!(
            text.contains("lantern: name: lantern is taken by lantern"),
            "{text}"
        );
    }

    #[test]
    fn a_referenced_document_that_does_not_read_is_refused_by_the_field() {
        let (world, _) = world();
        let broken = world.put("topic", "name = \"atlas\"\n", "\n");
        let relay = new_fact(&world, "lantern/relay-pin");
        set(&world, &relay, "topic", broken.as_str());
        let text = refused(save(&world.deps(), relay.as_str()));
        assert!(
            text.ends_with(&format!(
                "/relay-pin: topic: {} does not read",
                broken.short()
            )),
            "{text}"
        );
    }

    #[test]
    fn a_reference_to_a_document_with_a_head_that_reads_beside_one_that_does_not_is_stored() {
        let (world, _) = world();
        let atlas = topic(&world, "atlas", "");
        let root = head(&world, &atlas);
        let fields = format!("name = \"atlas\"\n{TOPIC}");
        world
            .store
            .put(&after(&[&root], &fields, "live\n"))
            .unwrap();
        world
            .store
            .put(&after(&[&root], "name = \"atlas\"\n", "unread\n"))
            .unwrap();
        let relay = new_fact(&world, "lantern/relay-pin");
        set(&world, &relay, "topic", atlas.as_str());
        save(&world.deps(), relay.as_str()).unwrap();
        assert_eq!(
            head(&world, &relay).fields.get("topic"),
            Some(&Value::from(atlas.as_str()))
        );
    }

    #[test]
    fn an_entry_name_is_taken_only_on_its_date() {
        let (world, _) = world();
        let deps = world.deps();
        let entry = |name: &str, date: &str| {
            let what = New::Entry {
                name,
                date: Some(date),
            };
            let opened = draft::new(&deps, &what).unwrap();
            set(&world, &opened.document, "summary", "A summary");
            opened.document
        };
        entry("lamp-driver", "2026-10-01");
        save(&deps, "2026-10-01-lamp-driver").unwrap();
        entry("lamp-driver", "2026-10-02");
        save(&deps, "2026-10-02-lamp-driver").unwrap();
        let second = entry("relay-board", "2026-10-02");
        set(&world, &second, "name", "lamp-driver");
        let text = refused(save(&deps, second.as_str()));
        assert!(
            text.contains(
                "2026-10-02-lamp-driver: name: lamp-driver is taken by 2026-10-02-lamp-driver"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_draft_with_a_parent_of_a_document_not_stored_is_refused() {
        let (world, lantern) = world();
        let phone = new_topic(&world, "phone");
        let elsewhere = head(&world, &lantern).id;
        edit(&world, &phone, |draft| draft.parents = vec![elsewhere]);
        let text = refused(save(&world.deps(), "phone"));
        assert!(
            text.contains("phone: moved on since the draft was checked out"),
            "{text}"
        );
        assert!(open(&world, &phone));
    }

    #[test]
    fn an_ended_record_is_stored_whole_with_its_links_resolved() {
        let (world, lantern) = world();
        let deps = world.deps();
        let root = head(&world, &lantern);
        let ended: Fields = format!("name = \"lantern\"\n{TOPIC}{ENDED}")
            .parse()
            .unwrap();
        let next = Record::read(&kind("topic"), &ended).unwrap();
        let lookup = world.lookup();
        let written = admit(&deps, &lookup, &lantern, &next, "See [[desk]].\n", "end").unwrap();
        let stored = head(&world, &lantern);
        assert_eq!(stored.id, written.version);
        assert_eq!(stored.fields, ended);
        assert_eq!(stored.envelope.change, "end");
        assert_eq!(stored.envelope.parents, vec![root.id]);
        assert_eq!(stored.links.0.get("desk"), Some(&world.host.0));

        let planted = first(&lantern, "topic", "name = \"x\"", "\n").id;
        world
            .store
            .plant_unreadable(&lantern, planted, ReadError::Corrupt);
        let lookup = world.lookup();
        let text = refused(admit(&deps, &lookup, &lantern, &next, "\n", "reopen"));
        assert!(
            text.contains("lantern: holds a version this worklog cannot read"),
            "{text}"
        );
    }

    #[test]
    fn only_a_reference_to_a_topic_must_be_live() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        world.amend(
            &relay,
            &format!(
                "name = \"relay\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\nended = \"false\"\n\
                 ended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "The relay.\n",
        );
        let atlas = topic(&world, "atlas", "");
        end(&world, &atlas, "atlas");
        let fields = |topics: &DocumentId, about: Option<&DocumentId>| -> Fields {
            let about = about.map_or(String::new(), |id| format!("about = \"{id}\"\n"));
            format!("topics = [\"{topics}\"]\n{about}summary = \"s\"\n")
                .parse()
                .unwrap()
        };
        let id = world.ids.mint().unwrap();
        let kind = kind("followup");
        let both = fields(&lantern, Some(&relay));
        store(&world, &id, &kind, &both, "\n", "new").unwrap();
        let id = world.ids.mint().unwrap();
        let text = refused(store(
            &world,
            &id,
            &kind,
            &fields(&atlas, None),
            "\n",
            "new",
        ));
        assert!(
            text.contains("new followup: topics: atlas is ended"),
            "{text}"
        );
    }

    #[test]
    fn a_fork_whose_heads_are_all_ended_resolves_only_as_a_reopening_would() {
        let (world, lantern) = world();
        let deps = world.deps();
        let relay = fact(&world, &lantern, "relay-pin", "");
        fork(
            &world,
            &relay,
            &format!(
                "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\nended = \"false\"\n\
                 ended_on = 2026-10-09\nnote = \"n\"\n"
            ),
        );
        end(&world, &lantern, "lantern");
        crate::app::fork::resolve(&deps, relay.as_str()).unwrap();
        let text = refused(save(&deps, relay.as_str()));
        assert!(
            text.contains("lantern/relay-pin: topic: lantern is ended"),
            "{text}"
        );
        assert!(open(&world, &relay));

        world.amend(&lantern, &format!("name = \"lantern\"\n{TOPIC}"), "\n");
        save(&deps, relay.as_str()).unwrap();
        let merged = head(&world, &relay);
        assert_eq!(merged.envelope.change, "resolve");
        assert_eq!(merged.fields.get("ended"), None);
    }

    #[test]
    fn a_new_entry_on_a_host_whose_machine_topic_is_ended_is_refused() {
        let (world, _) = world();
        let deps = world.deps();
        let desk = world.machine();
        end(&world, &desk, "desk");
        let what = New::Entry {
            name: "lamp-driver",
            date: None,
        };
        let opened = draft::new(&deps, &what).unwrap();
        set(&world, &opened.document, "summary", "Wired the lamp driver");
        let text = refused(save(&deps, "2026-10-09-lamp-driver"));
        assert!(
            text.contains("2026-10-09-lamp-driver: machine: desk is ended"),
            "{text}"
        );
        assert!(open(&world, &opened.document));
    }

    #[test]
    fn a_held_reference_keeps_its_document_under_a_former_name() {
        let (world, _) = world();
        let deps = world.deps();
        let renamed = topic(&world, "atlas", "");
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&renamed]);
        draft::checkout(&deps, "2026-10-08-lamp-driver").unwrap();
        world.amend(
            &renamed,
            &format!("name = \"phone\"\nformer_names = [\"atlas\"]\n{TOPIC}"),
            "\n",
        );
        topic(&world, "atlas", "");
        set(&world, &driver, "summary", "Wired the lamp driver");
        save(&deps, "2026-10-08-lamp-driver").unwrap();
        assert_eq!(
            head(&world, &driver).fields.get("topics"),
            Some(&list(&[renamed.as_str()]))
        );
    }

    fn ending() -> Ending {
        Ending {
            reason: Reason::Retired,
            on: Date::parse("2026-10-09").unwrap(),
            by: None,
            note: "n".to_owned(),
        }
    }

    #[test]
    fn an_ended_record_of_a_topic_with_a_dependent_is_not_admitted() {
        let (world, lantern) = world();
        let deps = world.deps();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let ended = record(&world, &lantern).end(ending()).unwrap();
        let lookup = world.lookup();
        let text = refused(admit(&deps, &lookup, &lantern, &ended, "\n", "retired"));
        assert_eq!(text, "lantern: still has 1 fact");

        let gone = Ending {
            reason: Reason::False,
            ..ending()
        };
        let gone = record(&world, &relay).end(gone).unwrap();
        admit(&deps, &lookup, &relay, &gone, "The relay.\n", "false").unwrap();
        let lookup = world.lookup();
        admit(&deps, &lookup, &lantern, &ended, "\n", "retired").unwrap();
        assert_eq!(head(&world, &lantern).envelope.change, "retired");
    }

    #[test]
    fn a_resolve_holds_only_the_references_of_the_heads_that_are_not_ended() {
        let (world, lantern) = world();
        let deps = world.deps();
        let atlas = topic(&world, "atlas", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        let fields = |topic: &DocumentId, rest: &str| {
            format!(
                "name = \"relay-pin\"\ntopic = \"{topic}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n{rest}"
            )
        };
        let gone = ENDED_FACT;
        let ended = after(&[&root], &fields(&lantern, gone), "ended\n");
        let live = after(&[&root], &fields(&atlas, ""), "live\n");
        world.store.put(&ended).unwrap();
        world.store.put(&live).unwrap();
        crate::app::amend::end(&deps, "lantern", "retired", "n", None).unwrap();
        crate::app::fork::resolve(&deps, relay.as_str()).unwrap();

        set(&world, &relay, "topic", "lantern");
        let text = refused(save(&deps, relay.as_str()));
        assert!(
            text.contains("/relay-pin: topic: lantern is ended"),
            "{text}"
        );
        assert!(open(&world, &relay));

        set(&world, &relay, "topic", "atlas");
        save(&deps, relay.as_str()).unwrap();
        let merged = head(&world, &relay);
        assert_eq!(merged.envelope.change, "resolve");
        assert_eq!(
            merged.fields.get("topic"),
            Some(&Value::from(atlas.as_str()))
        );
        assert_eq!(merged.fields.get("ended"), None);
    }
}
