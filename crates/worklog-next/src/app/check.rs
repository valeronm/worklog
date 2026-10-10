use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::app::heads::{kind_of, texts, unended_heads, unread_heads};
use crate::app::index::Topics;
use crate::app::lookup::Lookup;
use crate::app::rules::{Misfit, closing_edge, fits, holds_topic_open, other_holders};
use crate::app::{Deps, Failure, counted};
use crate::domain::document::Document;
use crate::domain::id::DocumentId;
use crate::domain::schema::{Content, Directory, KindOf};
use crate::domain::version::{ReadError, Version};

/// What a store holds that a write would have refused to make, and what a person may want
/// to look at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Check {
    pub problems: Vec<Finding>,
    pub notices: Vec<Finding>,
    /// The documents with more than one head, of the kinds this worklog knows.
    pub forks: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The label of the document the finding is about, or the kind's word for a kind this
    /// worklog does not know.
    pub document: String,
    pub what: String,
}

type Reported = BTreeSet<(DocumentId, String)>;

// Many references in one store share a target.
#[derive(Default)]
struct Fits(BTreeMap<(DocumentId, Option<&'static str>, bool), Option<Misfit>>);

impl Fits {
    fn of(
        &mut self,
        lookup: &Lookup,
        id: &DocumentId,
        kind: Option<KindOf>,
        unended: bool,
    ) -> Result<Option<Misfit>, Failure> {
        let question = (id.clone(), kind.map(KindOf::word), unended);
        if let Some(answer) = self.0.get(&question) {
            return Ok(*answer);
        }
        let answer = fits(lookup, id, kind, unended)?;
        self.0.insert(question, answer);
        Ok(answer)
    }
}

/// Problems and notices each come by document label, then text.
///
/// A document is checked in each of its heads that reads as a record and is not ended, and a
/// head that does not read as a record is a problem; a file that does not read as a version
/// is reported whatever its document. A kind this worklog does not know is one notice under
/// the kind's word, counting its documents, whose heads are not checked. A claim of this
/// host's machine whose directory this host's home would fold is a notice; a host with no
/// machine topic or no home has none.
pub fn check(deps: &Deps) -> Result<Check, Failure> {
    let lookup = Lookup::new(deps.store);
    let here = deps.host.machine()?.zip(deps.host.home()?);
    let (problems, notices) = found(&lookup, here.as_ref())?;
    let mut notices = labeled(&lookup, notices)?;
    for unknown in lookup.unknown()? {
        let documents = counted(unknown.documents.len(), "document", "documents");
        notices.push(Finding {
            document: unknown.kind.to_string(),
            what: format!("{documents} of a kind this worklog does not know"),
        });
    }
    Ok(Check {
        problems: sorted(labeled(&lookup, problems)?),
        notices: sorted(notices),
        forks: lookup.forks()?.len(),
    })
}

type Here = (DocumentId, String);

fn found(lookup: &Lookup, here: Option<&Here>) -> Result<(Reported, Reported), Failure> {
    let topics = Topics::load(lookup)?;
    let mut seen: BTreeMap<DocumentId, Rc<Document>> = BTreeMap::new();
    for document in lookup.everything()? {
        seen.insert(document.id().clone(), document);
    }
    for document in lookup.holding_unreadable()? {
        seen.insert(document.id().clone(), document);
    }
    let named = named(&seen);
    let (mut problems, mut notices) = (Reported::new(), Reported::new());
    let mut fits = Fits::default();
    for (id, document) in &seen {
        for file in document.unreadable() {
            let what = format!("{}: {}", file.id.short(), file.why);
            match file.why {
                ReadError::Newer { .. } => notices.insert((id.clone(), what)),
                _ => problems.insert((id.clone(), what)),
            };
        }
        if kind_of(document).is_none() {
            continue;
        }
        for (head, error) in unread_heads(document) {
            problems.insert((id.clone(), format!("{}: {error}", head.id.short())));
        }
        for (head, record) in unended_heads(document) {
            let mut wrong = references(lookup, &mut fits, head, record.content.kind())?;
            wrong.extend(name_held_twice(lookup, id, &record.content, &named)?);
            problems.extend(wrong.into_iter().map(|what| (id.clone(), what)));
            if let Some(what) = here.and_then(|here| unfolded(&record.content, here)) {
                notices.insert((id.clone(), what));
            }
            if links_are_checked(record.content.kind()) {
                let stale = links(lookup, &mut fits, head)?;
                notices.extend(stale.into_iter().map(|what| (id.clone(), what)));
            }
        }
        if let Some(what) = cycle(&topics, id) {
            problems.insert((id.clone(), what));
        }
    }
    Ok((problems, notices))
}

fn sorted(mut findings: Vec<Finding>) -> Vec<Finding> {
    findings.sort_by(|a, b| (&a.document, &a.what).cmp(&(&b.document, &b.what)));
    findings
}

fn unfolded(content: &Content, (machine, home): &Here) -> Option<String> {
    let Content::Claim(claim) = content else {
        return None;
    };
    let stored = claim.directory.as_ref()?;
    let folded = Directory::on_host(stored.as_str(), Some(home)).ok()?;
    (claim.machine == *machine && folded != *stored).then(|| {
        format!("directory: {stored} is under this host's home; end this claim and claim {folded}")
    })
}

fn links_are_checked(kind: KindOf) -> bool {
    match kind {
        KindOf::Topic | KindOf::Fact | KindOf::Followup => true,
        KindOf::Entry | KindOf::Claim => false,
    }
}

fn labeled(lookup: &Lookup, found: Reported) -> Result<Vec<Finding>, Failure> {
    let mut findings = Vec::new();
    for (id, what) in found {
        findings.push(Finding {
            document: lookup.label(&id)?,
            what,
        });
    }
    Ok(findings)
}

fn named(seen: &BTreeMap<DocumentId, Rc<Document>>) -> BTreeMap<String, BTreeSet<DocumentId>> {
    let mut named: BTreeMap<String, BTreeSet<DocumentId>> = BTreeMap::new();
    for (id, document) in seen {
        for (_, record) in unended_heads(document) {
            if let Some((name, _)) = record.content.naming() {
                named
                    .entry(name.to_string())
                    .or_default()
                    .insert(id.clone());
            }
        }
    }
    named
}

fn unfit(lookup: &Lookup, id: &DocumentId, misfit: Misfit) -> Result<String, Failure> {
    Ok(match misfit {
        Misfit::Missing => format!("{} is missing", id.short()),
        other => format!("{} {other}", lookup.label(id)?),
    })
}

fn references(
    lookup: &Lookup,
    fits: &mut Fits,
    head: &Version,
    kind: KindOf,
) -> Result<Vec<String>, Failure> {
    let mut wrong = Vec::new();
    for reference in kind.references() {
        let key = reference.key;
        for text in texts(&head.fields, key) {
            let Ok(id) = DocumentId::parse(text) else {
                continue;
            };
            let unended = holds_topic_open(reference);
            if let Some(misfit) = fits.of(lookup, &id, reference.target, unended)? {
                wrong.push(format!("{key}: {}", unfit(lookup, &id, misfit)?));
            }
        }
    }
    Ok(wrong)
}

fn name_held_twice(
    lookup: &Lookup,
    id: &DocumentId,
    content: &Content,
    named: &BTreeMap<String, BTreeSet<DocumentId>>,
) -> Result<Vec<String>, Failure> {
    let Some((name, _)) = content.naming() else {
        return Ok(Vec::new());
    };
    let candidates = named.get(name.as_str()).into_iter().flatten();
    Ok(other_holders(lookup, id, content, candidates)?
        .iter()
        .map(|other| format!("name: {name} is also held by {}", other.short()))
        .collect())
}

fn links(lookup: &Lookup, fits: &mut Fits, head: &Version) -> Result<Vec<String>, Failure> {
    let mut stale = Vec::new();
    for (name, target) in &head.links.0 {
        let nothing = format!("link {name}: resolves to nothing");
        let Some(id) = target else {
            stale.push(nothing);
            continue;
        };
        match fits.of(lookup, id, None, true)? {
            Some(Misfit::Missing) => stale.push(nothing),
            Some(misfit) => stale.push(format!("link {name}: {} {misfit}", lookup.label(id)?)),
            None => {}
        }
    }
    Ok(stale)
}

fn cycle(topics: &Topics, id: &DocumentId) -> Option<String> {
    let name = topics.name(id)?;
    let key = closing_edge(id, topics.edges(id)?, |topic| topics.edges(topic))?;
    Some(format!("{key}: makes {name} part of itself"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value;

    use super::*;
    use crate::app::lookup::Stored;
    use crate::app::save::save;
    use crate::app::testing::{
        Counting, ENDED, ENDED_FACT, World, claim_fields, entry, fact, fact_fields, followup, fork,
        head, list, refused, topic, topic_fields, world, world_with_atlas,
    };
    use crate::app::{amend, draft, fork as forks};
    use crate::domain::id::{DocumentId, VersionId};
    use crate::domain::ports::{Clock, Drafts, Ids, Store};
    use crate::domain::schema::{KindOf, Record};
    use crate::domain::testing::{after, first};
    use crate::domain::version::{Envelope, Kind, Links, ReadError, Version};

    fn checked(world: &World) -> Check {
        check(&world.deps()).unwrap()
    }

    fn finding(document: &str, what: &str) -> Finding {
        Finding {
            document: document.to_owned(),
            what: what.to_owned(),
        }
    }

    fn linking(
        world: &World,
        kind: &str,
        fields: &str,
        links: &[(&str, Option<&DocumentId>)],
    ) -> DocumentId {
        let id = world.ids.mint().unwrap();
        let envelope = Envelope::first(
            id.clone(),
            Kind::parse(kind).unwrap(),
            world.clock.now(),
            world.machine(),
            "new",
        );
        let links: BTreeMap<String, Option<DocumentId>> = links
            .iter()
            .map(|(name, target)| ((*name).to_owned(), target.cloned()))
            .collect();
        let names: Vec<&str> = links.keys().map(String::as_str).collect();
        let body = format!("[[{}]]\n", names.join("]]\n[["));
        let version =
            Version::compose(envelope, fields.parse().unwrap(), Links(links), body).unwrap();
        world.store.put(&version).unwrap();
        id
    }

    fn version_id(seed: &str) -> VersionId {
        VersionId::of(seed.as_bytes())
    }

    #[test]
    fn a_clean_store_gives_no_findings() {
        let (world, lantern, atlas) = world_with_atlas();
        let desk = world.machine();
        topic(
            &world,
            "phone",
            &format!(
                "{}{}",
                list("part_of", &[&lantern]),
                list("uses", &[&atlas, &desk])
            ),
        );
        let relay = fact(&world, &lantern, "relay-pin", "");
        let driver = entry(&world, "2026-10-08", "lamp-driver", &[&lantern, &atlas]);
        followup(
            &world,
            &[],
            "s",
            &format!(
                "{}entry = \"{driver}\"\nabout = \"{relay}\"\n",
                list("topics", &[&lantern])
            ),
        );
        world.put(
            "claim",
            &claim_fields(&world.machine(), &lantern, "/work/lantern"),
            "\n",
        );
        linking(
            &world,
            "topic",
            &topic_fields("map", ""),
            &[("lantern/relay-pin", Some(&relay))],
        );
        assert_eq!(checked(&world), Check::default());
    }

    #[test]
    fn a_reference_to_a_document_the_store_lacks_is_a_problem() {
        let (world, _) = world();
        let nowhere = DocumentId::from_bytes([0xab; 16]);
        let orphan = fact(&world, &nowhere, "relay-pin", "");
        fact(&world, &nowhere, "old-pin", ENDED_FACT);
        let found = checked(&world);
        assert_eq!(
            found.problems,
            [finding(orphan.short(), "topic: abababab is missing")]
        );
        assert!(found.notices.is_empty());
    }

    #[test]
    fn a_reference_to_a_document_of_another_kind_is_a_problem() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let wrong = followup(
            &world,
            &[],
            "s",
            &format!("{}entry = \"{relay}\"\n", list("topics", &[&lantern])),
        );
        followup(
            &world,
            &[],
            "s",
            &format!("{}about = \"{relay}\"\n", list("topics", &[&lantern])),
        );
        let misfiled = fact(&world, &relay, "spare", "");
        let (problems, _) = found(&world.lookup(), None).unwrap();
        let on: Vec<&DocumentId> = problems.iter().map(|(id, _)| id).collect();
        assert_eq!(on, [&wrong, &misfiled]);
        assert_eq!(
            checked(&world).problems,
            [
                finding(wrong.short(), "entry: lantern/relay-pin is not an entry"),
                finding(misfiled.short(), "topic: lantern/relay-pin is not a topic"),
            ]
        );
    }

    #[test]
    fn an_ended_target_is_a_problem_only_for_a_reference_that_holds_it_open() {
        let (world, _) = world();
        let atlas = topic(&world, "atlas", ENDED);
        fact(&world, &atlas, "relay-pin", "");
        fact(&world, &atlas, "old-pin", ENDED_FACT);
        entry(&world, "2026-10-08", "lamp-driver", &[&atlas]);
        let held = followup(
            &world,
            &[],
            "s",
            &format!("{}touching = \"{atlas}\"\n", list("topics", &[&atlas])),
        );
        let found = checked(&world);
        assert_eq!(
            found.problems,
            [
                finding(held.short(), "topics: atlas is ended"),
                finding(held.short(), "touching: atlas is ended"),
                finding("atlas/relay-pin", "topic: atlas is ended"),
            ]
        );
        assert!(found.notices.is_empty());
    }

    #[test]
    fn a_fork_s_target_is_ended_only_when_every_head_of_it_is() {
        let (world, _, atlas) = world_with_atlas();
        fact(&world, &atlas, "relay-pin", "");
        let root = head(&world, &atlas);
        let ended = after(
            &[&root],
            &format!("{}{ENDED}", topic_fields("atlas", "")),
            "\n",
        );
        let live = after(&[&root], &topic_fields("atlas", ""), "live\n");
        world.store.put(&ended).unwrap();
        world.store.put(&live).unwrap();
        let found = checked(&world);
        assert_eq!(found.problems, []);
        assert_eq!(found.forks, 1);

        let last = after(
            &[&live],
            &format!("{}{ENDED}", topic_fields("atlas", "")),
            "\n",
        );
        world.store.put(&last).unwrap();
        let found = checked(&world);
        assert_eq!(
            found.problems,
            [finding("atlas/relay-pin", "topic: atlas is ended")]
        );
        assert_eq!(found.forks, 1);
    }

    #[test]
    fn a_head_that_does_not_read_as_a_record_is_a_problem_and_does_not_end_its_document() {
        let (world, _, atlas) = world_with_atlas();
        fact(&world, &atlas, "relay-pin", "");
        let root = head(&world, &atlas);
        let unread = after(&[&root], "name = \"atlas\"\n", "\n");
        let live = after(&[&root], &topic_fields("atlas", ""), "live\n");
        world.store.put(&unread).unwrap();
        world.store.put(&live).unwrap();
        let why = Record::read(&unread.envelope.kind, &unread.fields).unwrap_err();
        let unread = finding("atlas", &format!("{}: {why}", unread.id.short()));
        let found = checked(&world);
        assert_eq!(found.problems, std::slice::from_ref(&unread));
        assert_eq!(found.forks, 1);

        let ended = after(
            &[&live],
            &format!("{}{ENDED}", topic_fields("atlas", "")),
            "\n",
        );
        world.store.put(&ended).unwrap();
        assert_eq!(
            checked(&world).problems,
            [unread, finding("atlas/relay-pin", "topic: atlas is ended")]
        );
    }

    #[test]
    fn a_target_no_head_of_which_reads_as_a_record_does_not_read() {
        let (world, _) = world();
        let phone = DocumentId::from_bytes([0xab; 16]);
        let broken = first(&phone, "topic", "name = \"phone\"\n", "\n");
        world.store.put(&broken).unwrap();
        let why = Record::read(&broken.envelope.kind, &broken.fields).unwrap_err();
        let orphan = fact(&world, &phone, "relay-pin", "");
        linking(
            &world,
            "topic",
            &topic_fields("atlas", ""),
            &[("phone", Some(&phone))],
        );
        let found = checked(&world);
        assert_eq!(
            found.problems,
            [
                finding(orphan.short(), "topic: abababab does not read"),
                finding("abababab", &format!("{}: {why}", broken.id.short())),
            ]
        );
        assert_eq!(
            found.notices,
            [finding("atlas", "link phone: abababab does not read")]
        );
    }

    #[test]
    fn a_write_refuses_a_target_that_does_not_read_in_the_words_of_the_check() {
        let (world, _) = world();
        let deps = world.deps();
        let phone = DocumentId::from_bytes([0xab; 16]);
        let broken = first(&phone, "topic", "name = \"phone\"\n", "\n");
        world.store.put(&broken).unwrap();
        let orphan = fact(&world, &phone, "relay-pin", "");
        let what = "topic: abababab does not read";
        assert_eq!(checked(&world).problems[0], finding(orphan.short(), what));

        let new = draft::New::Fact {
            address: "lantern/spare",
            idea: false,
        };
        let spare = draft::new(&deps, &new).unwrap().document;
        let mut draft = world.drafts.read(&spare).unwrap().unwrap();
        draft.fields.insert("summary".to_owned(), Value::from("s"));
        draft
            .fields
            .insert("topic".to_owned(), Value::from(phone.as_str()));
        world.drafts.write(&draft).unwrap();
        let text = refused(save(&deps, spare.as_str()));
        assert!(text.ends_with(&format!(": {what}")), "{text}");
    }

    #[test]
    fn an_edge_only_an_ended_head_holds_closes_no_cycle() {
        let (world, lantern, atlas) = world_with_atlas();
        let deps = world.deps();
        let root = head(&world, &atlas);
        let ended = format!(
            "{}{}{ENDED}",
            topic_fields("atlas", ""),
            list("part_of", &[&lantern])
        );
        world.store.put(&after(&[&root], &ended, "\n")).unwrap();
        world
            .store
            .put(&after(&[&root], &topic_fields("atlas", ""), "live\n"))
            .unwrap();

        draft::checkout(&deps, "lantern").unwrap();
        let mut draft = world.drafts.read(&lantern).unwrap().unwrap();
        let part_of = Value::Array(vec![Value::from("atlas")]);
        draft.fields.insert("part_of".to_owned(), part_of);
        world.drafts.write(&draft).unwrap();
        save(&deps, "lantern").unwrap();

        let found = checked(&world);
        assert_eq!(found.problems, []);
        assert_eq!(found.forks, 1);
    }

    #[test]
    fn each_unended_head_of_a_fork_is_checked_on_its_own() {
        let (world, lantern) = world();
        let nowhere = DocumentId::from_bytes([0xab; 16]);
        let gone = DocumentId::from_bytes([0xcd; 16]);
        let relay = fact(&world, &lantern, "relay-pin", "");
        let root = head(&world, &relay);
        for (topic, rest) in [(&lantern, ""), (&nowhere, ""), (&gone, ENDED_FACT)] {
            let fields = format!("{}{rest}", fact_fields(topic, "relay-pin", "s", ""));
            world
                .store
                .put(&after(&[&root], &fields, topic.as_str()))
                .unwrap();
        }
        let found = checked(&world);
        assert_eq!(found.forks, 1);
        let whats: Vec<&str> = found.problems.iter().map(|f| f.what.as_str()).collect();
        assert_eq!(whats, ["topic: abababab is missing"]);
    }

    #[test]
    fn a_current_name_held_twice_in_one_scope_is_a_problem_on_each_holder() {
        let world = World::new();
        topic(&world, "desk", "");
        let (lantern, twin) = (
            DocumentId::from_bytes([0xab; 16]),
            DocumentId::from_bytes([0xcd; 16]),
        );
        for id in [&lantern, &twin] {
            let version = first(id, "topic", &topic_fields("lantern", ""), "\n");
            world.store.put(&version).unwrap();
        }
        let atlas = topic(&world, "atlas", "");
        topic(&world, "lantern", ENDED);
        topic(&world, "phone", "former_names = [\"atlas\"]\n");
        let stored = |byte: u8, kind: &str, fields: &str| {
            let id = DocumentId::from_bytes([byte; 16]);
            world.store.put(&first(&id, kind, fields, "\n")).unwrap();
            id
        };
        let relay = stored(0x11, "fact", &fact_fields(&lantern, "relay-pin", "s", ""));
        let spare = stored(0x22, "fact", &fact_fields(&lantern, "relay-pin", "s", ""));
        fact(&world, &lantern, "relay-pin", ENDED_FACT);
        fact(&world, &atlas, "relay-pin", "");
        fact(&world, &twin, "relay-pin", "");
        let desk = world.machine();
        let entry_fields = |date: &str| {
            format!(
                "name = \"lamp-driver\"\ndate = {date}\nmachine = \"{desk}\"\nsummary = \"s\"\n"
            )
        };
        let driver = stored(0x33, "entry", &entry_fields("2026-10-08"));
        let again = stored(0x44, "entry", &entry_fields("2026-10-08"));
        world.put("entry", &entry_fields("2026-10-07"), "\n");
        let held_by = |name: &str, other: &DocumentId| {
            format!("name: {name} is also held by {}", other.short())
        };

        let (problems, _) = found(&world.lookup(), None).unwrap();
        let expected: Reported = [
            (lantern.clone(), held_by("lantern", &twin)),
            (twin.clone(), held_by("lantern", &lantern)),
            (relay.clone(), held_by("relay-pin", &spare)),
            (spare.clone(), held_by("relay-pin", &relay)),
            (driver.clone(), held_by("lamp-driver", &again)),
            (again.clone(), held_by("lamp-driver", &driver)),
        ]
        .into();
        assert_eq!(problems, expected);
        assert_eq!(
            checked(&world).problems,
            [
                finding("2026-10-08-lamp-driver", &held_by("lamp-driver", &driver)),
                finding("2026-10-08-lamp-driver", &held_by("lamp-driver", &again)),
                finding("lantern", &held_by("lantern", &lantern)),
                finding("lantern", &held_by("lantern", &twin)),
                finding("lantern/relay-pin", &held_by("relay-pin", &relay)),
                finding("lantern/relay-pin", &held_by("relay-pin", &spare)),
            ]
        );
    }

    #[test]
    fn unended_topics_in_a_cycle_are_each_a_problem() {
        let (world, lantern, atlas) = world_with_atlas();
        let phone = topic(&world, "phone", &list("uses", &[&atlas]));
        topic(&world, "map", &list("part_of", &[&phone]));
        world.amend(
            &atlas,
            &format!(
                "{}{}",
                topic_fields("atlas", ""),
                list("part_of", &[&lantern])
            ),
            "\n",
        );
        assert_eq!(checked(&world).problems, []);

        world.amend(
            &lantern,
            &format!(
                "{}{}",
                topic_fields("lantern", ""),
                list("part_of", &[&phone])
            ),
            "\n",
        );
        assert_eq!(
            checked(&world).problems,
            [
                finding("atlas", "part_of: makes atlas part of itself"),
                finding("lantern", "part_of: makes lantern part of itself"),
                finding("phone", "uses: makes phone part of itself"),
            ]
        );
    }

    #[test]
    fn a_file_that_does_not_read_is_a_problem_and_a_newer_one_a_notice() {
        let (world, lantern) = world();
        let stray = DocumentId::from_bytes([0xab; 16]);
        let (damaged, torn, newer) = (version_id("damaged"), version_id("torn"), version_id("new"));
        world
            .store
            .plant_unreadable(&lantern, damaged.clone(), ReadError::Corrupt);
        world.store.plant_unreadable(
            &stray,
            torn.clone(),
            ReadError::Malformed("no header".to_owned()),
        );
        world
            .store
            .plant_unreadable(&lantern, newer.clone(), ReadError::Newer { format: 9 });
        let found = checked(&world);
        assert_eq!(
            found.problems,
            [
                finding("abababab", &format!("{}: no header", torn.short())),
                finding(
                    "lantern",
                    &format!("{}: {}", damaged.short(), ReadError::Corrupt)
                ),
            ]
        );
        assert_eq!(
            found.notices,
            [finding(
                "lantern",
                &format!("{}: written in format 9, a newer one", newer.short())
            )]
        );
    }

    #[test]
    fn a_link_to_an_ended_document_or_to_nothing_is_a_notice_except_from_an_entry() {
        let (world, lantern) = world();
        let nowhere = DocumentId::from_bytes([0xab; 16]);
        let old = fact(&world, &lantern, "old-pin", ENDED_FACT);
        let relay = fact(&world, &lantern, "relay-pin", "");
        let links = [
            ("lantern/old-pin", Some(&old)),
            ("lantern/relay-pin", Some(&relay)),
            ("lantern/spare", Some(&nowhere)),
            ("unknown", None),
        ];
        linking(&world, "topic", &topic_fields("atlas", ""), &links);
        linking(
            &world,
            "entry",
            &format!(
                "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\nsummary = \"s\"\n",
                world.machine()
            ),
            &links,
        );
        linking(
            &world,
            "topic",
            &format!("{}{ENDED}", topic_fields("phone", "")),
            &links,
        );
        let found = checked(&world);
        assert_eq!(found.problems, []);
        assert_eq!(
            found.notices,
            [
                finding("atlas", "link lantern/old-pin: lantern/old-pin is ended"),
                finding("atlas", "link lantern/spare: resolves to nothing"),
                finding("atlas", "link unknown: resolves to nothing"),
            ]
        );

        let world = World::new();
        let desk = topic(&world, "desk", "");
        let lantern = topic(&world, "lantern", ENDED);
        let noted = linking(
            &world,
            "followup",
            &format!(
                "created = 2026-09-04\n{}summary = \"s\"\n",
                list("topics", &[&desk])
            ),
            &[("lantern", Some(&lantern))],
        );
        linking(
            &world,
            "fact",
            &fact_fields(&lantern, "relay-pin", "s", ""),
            &[("lantern", Some(&lantern))],
        );
        let found = checked(&world);
        assert_eq!(
            found.notices,
            [
                finding(noted.short(), "link lantern: lantern is ended"),
                finding("lantern/relay-pin", "link lantern: lantern is ended"),
            ]
        );
        assert_eq!(
            found.problems,
            [finding("lantern/relay-pin", "topic: lantern is ended")]
        );
    }

    #[test]
    fn findings_come_by_label_then_text() {
        let (world, _) = world();
        let nowhere = DocumentId::from_bytes([0xab; 16]);
        let gone = DocumentId::from_bytes([0xcd; 16]);
        topic(&world, "phone", &list("uses", &[&nowhere]));
        topic(
            &world,
            "atlas",
            &format!(
                "{}{}",
                list("part_of", &[&gone]),
                list("uses", &[&nowhere, &gone])
            ),
        );
        assert_eq!(
            checked(&world).problems,
            [
                finding("atlas", "part_of: cdcdcdcd is missing"),
                finding("atlas", "uses: abababab is missing"),
                finding("atlas", "uses: cdcdcdcd is missing"),
                finding("phone", "uses: abababab is missing"),
            ]
        );
    }

    #[test]
    fn the_store_is_scanned_once_per_kind_and_once_for_what_does_not_read() {
        let (world, lantern, atlas) = world_with_atlas();
        let nowhere = DocumentId::from_bytes([0xab; 16]);
        let gone = DocumentId::from_bytes([0xcd; 16]);
        for name in ["relay-pin", "lamp-pin", "spare"] {
            fact(&world, &lantern, name, "");
            fact(&world, &atlas, name, "");
        }
        fact(&world, &nowhere, "relay-pin", "");
        fact(&world, &nowhere, "lamp-pin", "");
        entry(&world, "2026-10-08", "lamp-driver", &[&lantern, &gone]);
        followup(&world, &[], "s", &list("topics", &[&atlas, &nowhere]));
        world.put(
            "claim",
            &claim_fields(&world.machine(), &lantern, "/work/lantern"),
            "\n",
        );
        linking(
            &world,
            "topic",
            &topic_fields("phone", ""),
            &[("lantern", Some(&lantern)), ("gone", Some(&gone))],
        );
        world
            .store
            .plant_unreadable(&lantern, version_id("damaged"), ReadError::Corrupt);
        let World {
            store,
            drafts,
            ids,
            clock,
            host,
        } = world;
        let counting = Counting::over(store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &drafts,
            ids: &ids,
            clock: &clock,
            host: &host,
        };
        let found = check(&deps).unwrap();
        assert_eq!(found.problems.len(), 5, "{:?}", found.problems);
        assert_eq!(found.notices.len(), 1, "{:?}", found.notices);
        assert_eq!(counting.kinds.get(), KindOf::ALL.len());
        assert_eq!(counting.kind_lists.get(), 1);
        assert_eq!(counting.holdings.get(), 0);
        assert_eq!(counting.unreadables.get(), 1);
        assert_eq!(counting.forks.get(), 1);
        assert_eq!(counting.scans(), KindOf::ALL.len() + 3);
        assert_eq!(counting.documents.get(), 2);
    }

    #[test]
    fn a_claim_of_this_machine_stored_in_a_form_this_host_would_fold_is_a_notice() {
        let (mut world, lantern) = world();
        let desk = world.machine();
        let phone = topic(&world, "phone", "");
        let claim = |machine: &DocumentId, directory: &str, rest: &str| {
            let fields = format!(
                "machine = \"{machine}\"\ntopic = \"{lantern}\"\ndirectory = \"{directory}\"\n{rest}"
            );
            world.put("claim", &fields, "\n")
        };
        let stored = claim(&desk, "/home/desk/lantern", "");
        claim(&phone, "/home/desk/lantern", "");
        claim(
            &desk,
            "/home/desk/atlas",
            "ended = \"removed\"\nended_on = 2026-10-09\nnote = \"n\"\n",
        );
        claim(&desk, "~/lantern", "");
        claim(&desk, "/srv/lantern", "");
        claim(&desk, "/home/desktop/lantern", "");
        assert_eq!(checked(&world), Check::default());

        world.host.1 = Some("/home/desk".to_owned());
        let found = checked(&world);
        assert_eq!(found.problems, []);
        assert_eq!(
            found.notices,
            [finding(
                stored.short(),
                "directory: /home/desk/lantern is under this host's home; \
                 end this claim and claim ~/lantern"
            )]
        );

        world.host.0 = None;
        assert_eq!(checked(&world), Check::default());
    }

    #[test]
    fn a_kind_this_worklog_does_not_know_is_a_notice_counting_its_documents() {
        let (world, _) = world();
        world.put("sketch", "name = \"one\"\n", "\n");
        world.put("sketch", "name = \"two\"\n", "\n");
        world.put("blueprint", "name = \"one\"\n", "\n");
        let unknown = |counted: &str| format!("{counted} of a kind this worklog does not know");
        let World {
            store,
            drafts,
            ids,
            clock,
            host,
        } = world;
        let counting = Counting::over(store);
        let deps = Deps {
            store: Stored::new(&counting),
            drafts: &drafts,
            ids: &ids,
            clock: &clock,
            host: &host,
        };
        let found = check(&deps).unwrap();
        assert_eq!(found.problems, []);
        assert_eq!(
            found.notices,
            [
                finding("blueprint", &unknown("1 document")),
                finding("sketch", &unknown("2 documents")),
            ]
        );
        assert_eq!(found.forks, 0);
        assert_eq!(counting.kind_lists.get(), 1);
        assert_eq!(counting.kinds.get(), KindOf::ALL.len() + 2);
    }

    #[test]
    fn a_document_of_a_kind_this_worklog_does_not_know_is_checked_in_its_files_alone() {
        let (world, _) = world();
        let sketch = world.put("sketch", "name = \"one\"\n", "\n");
        let damaged = version_id("damaged");
        world
            .store
            .plant_unreadable(&sketch, damaged.clone(), ReadError::Corrupt);
        let found = checked(&world);
        assert_eq!(
            found.problems,
            [finding(
                sketch.short(),
                &format!("{}: {}", damaged.short(), ReadError::Corrupt)
            )]
        );
        assert_eq!(
            found.notices,
            [finding(
                "sketch",
                "1 document of a kind this worklog does not know"
            )]
        );
    }

    #[test]
    fn resolving_a_forked_claim_stores_one_head_and_the_fork_is_no_longer_counted() {
        let (world, lantern) = world();
        let deps = world.deps();
        let fields = claim_fields(&world.machine(), &lantern, "/work/lantern");
        let claim = world.put("claim", &fields, "\n");
        fork(&world, &claim, &fields);
        let before = checked(&world);
        assert_eq!(before.forks, 1);
        assert_eq!(before.problems, []);

        forks::resolve(&deps, claim.as_str()).unwrap();
        let written = save(&deps, claim.as_str()).unwrap();
        let document = world.store.document(&claim).unwrap();
        assert_eq!(document.heads().len(), 1);
        assert_eq!(document.heads()[0].id, written.version);
        assert_eq!(checked(&world), Check::default());
    }

    #[test]
    fn a_cycle_two_machines_made_is_reported_and_a_member_of_it_is_not_reopened() {
        let (world, lantern) = world();
        let deps = world.deps();
        let atlas = topic(&world, "atlas", &list("part_of", &[&lantern]));
        world.amend(
            &lantern,
            &format!(
                "{}{}",
                topic_fields("lantern", ""),
                list("part_of", &[&atlas])
            ),
            "\n",
        );
        assert_eq!(
            checked(&world).problems,
            [
                finding("atlas", "part_of: makes atlas part of itself"),
                finding("lantern", "part_of: makes lantern part of itself"),
            ]
        );

        world.amend(
            &atlas,
            &format!(
                "{}{}{ENDED}",
                topic_fields("atlas", ""),
                list("part_of", &[&lantern])
            ),
            "\n",
        );
        assert_eq!(
            checked(&world).problems,
            [finding("lantern", "part_of: atlas is ended")]
        );
        let text = refused(amend::reopen(&deps, "atlas", "needed again"));
        assert!(
            text.contains("atlas: part_of: would make atlas part of itself"),
            "{text}"
        );
    }
}
