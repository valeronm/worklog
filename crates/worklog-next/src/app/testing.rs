use std::cell::Cell;

use crate::app::heads::read_or_skip;
use crate::app::lookup::{Lookup, Stored};
use crate::app::rows::Row;
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::document::State;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::ports::{Clock, Ids, Store, StoreError};
use crate::domain::schema::Record;
use crate::domain::schema::address::Found;
use crate::domain::testing::{
    FixedClock, FixedHost, MemoryDrafts, MemoryStore, SequenceIds, after, first_minted,
};
use crate::domain::version::{Envelope, Fields, Kind, Links, Stamp, Version};

pub struct World {
    pub store: MemoryStore,
    pub drafts: MemoryDrafts,
    pub ids: SequenceIds,
    pub clock: FixedClock,
    pub host: FixedHost,
}

impl World {
    pub fn new() -> World {
        World {
            store: MemoryStore::default(),
            drafts: MemoryDrafts::default(),
            ids: SequenceIds::default(),
            clock: FixedClock::at("2026-10-09T18:22:41.118204+01:00"),
            host: FixedHost::new(Some(first_minted()), None),
        }
    }

    pub fn deps(&self) -> Deps<'_> {
        Deps {
            store: Stored::new(&self.store),
            drafts: &self.drafts,
            ids: &self.ids,
            clock: &self.clock,
            host: &self.host,
        }
    }

    pub(in crate::app) fn lookup(&self) -> Lookup<'_> {
        Lookup::new(Stored::new(&self.store))
    }

    pub fn machine(&self) -> DocumentId {
        self.host.bound().expect("a host with a machine topic")
    }

    /// Stores a first version with these fields (TOML, references as ids) and returns its id.
    pub fn put(&self, kind: &str, fields: &str, body: &str) -> DocumentId {
        let id = self.ids.mint().expect("an id");
        let fields: Fields = fields.parse().expect("fields in TOML");
        let envelope = Envelope::first(
            id.clone(),
            Kind::parse(kind).expect("a kind"),
            self.clock.now(),
            self.machine(),
            "new",
        );
        let version = Version::compose(envelope, fields, Links::default(), body.to_owned())
            .expect("a version");
        self.store.put(&version).expect("a write");
        id
    }

    pub fn amend(&self, id: &DocumentId, fields: &str, body: &str) -> VersionId {
        let held = self.store.document(id).expect("a read");
        let envelope = Envelope::following(&held.heads(), self.clock.now(), self.machine(), "save")
            .expect("a head to follow");
        let fields: Fields = fields.parse().expect("fields in TOML");
        let version = Version::compose(envelope, fields, Links::default(), body.to_owned())
            .expect("a version");
        self.store.put(&version).expect("a write");
        version.id
    }
}

pub const TOPIC: &str = "created = 2026-09-04\nsummary = \"s\"\n";
pub const ENDED: &str = "ended = \"retired\"\nended_on = 2026-10-09\nnote = \"n\"\n";
pub const ENDED_FACT: &str = "ended = \"false\"\nended_on = 2026-10-09\nnote = \"n\"\n";

pub fn topic_fields(name: &str, rest: &str) -> String {
    format!("name = \"{name}\"\n{TOPIC}{rest}")
}

pub fn topic(world: &World, name: &str, rest: &str) -> DocumentId {
    world.put("topic", &topic_fields(name, rest), "\n")
}

pub fn list(key: &str, ids: &[&DocumentId]) -> String {
    let ids: Vec<String> = ids.iter().map(|id| format!("\"{id}\"")).collect();
    format!("{key} = [{}]\n", ids.join(", "))
}

pub fn part_of(parents: &[&DocumentId]) -> String {
    list("part_of", parents)
}

pub fn uses(used: &[&DocumentId]) -> String {
    list("uses", used)
}

pub fn world_of<const N: usize>(names: [&str; N]) -> (World, [DocumentId; N]) {
    let world = World::new();
    topic(&world, "desk", "");
    let ids = names.map(|name| topic(&world, name, ""));
    (world, ids)
}

pub fn world() -> (World, DocumentId) {
    let (world, [lantern]) = world_of(["lantern"]);
    (world, lantern)
}

/// `world`, with the topic `atlas` after `lantern`.
pub fn world_with_atlas() -> (World, DocumentId, DocumentId) {
    let (world, lantern) = world();
    let atlas = topic(&world, "atlas", "");
    (world, lantern, atlas)
}

pub fn fact_fields(topic: &DocumentId, name: &str, summary: &str, rest: &str) -> String {
    format!(
        "name = \"{name}\"\ntopic = \"{topic}\"\ncreated = 2026-09-04\n\
         confirmed = 2026-09-04\nsummary = \"{summary}\"\n{rest}"
    )
}

pub fn fact(world: &World, topic: &DocumentId, name: &str, rest: &str) -> DocumentId {
    world.put("fact", &fact_fields(topic, name, "s", rest), "The relay.\n")
}

pub fn entry(world: &World, date: &str, name: &str, topics: &[&DocumentId]) -> DocumentId {
    let topics = if topics.is_empty() {
        String::new()
    } else {
        list("topics", topics)
    };
    world.put(
        "entry",
        &format!(
            "name = \"{name}\"\ndate = {date}\nmachine = \"{}\"\n{topics}summary = \"s\"\n",
            world.machine()
        ),
        "\n",
    )
}

pub fn followup(world: &World, topics: &[&DocumentId], summary: &str, rest: &str) -> DocumentId {
    let topics = if topics.is_empty() {
        String::new()
    } else {
        list("topics", topics)
    };
    world.put(
        "followup",
        &format!("created = 2026-09-04\n{topics}summary = \"{summary}\"\n{rest}"),
        "\n",
    )
}

pub fn claim_fields(machine: &DocumentId, topic: &DocumentId, directory: &str) -> String {
    let directory = if directory.is_empty() {
        String::new()
    } else {
        format!("directory = \"{directory}\"\n")
    };
    format!("machine = \"{machine}\"\ntopic = \"{topic}\"\n{directory}")
}

/// Stores two versions of `fields` on the live head and returns the heads.
pub fn fork(world: &World, id: &DocumentId, fields: &str) -> Vec<VersionId> {
    let root = head(world, id);
    for body in ["left\n", "right\n"] {
        world
            .store
            .put(&after(&[&root], fields, body))
            .expect("a write");
    }
    let held = world.store.document(id).expect("a read");
    held.heads().iter().map(|head| head.id.clone()).collect()
}

pub fn written_at(
    world: &World,
    root: &Version,
    fields: &str,
    stamp: &str,
    body: &str,
    machine: Option<&DocumentId>,
) -> Version {
    let envelope = Envelope::following(
        &[root],
        Stamp::parse(stamp).unwrap(),
        machine.cloned().unwrap_or_else(|| world.machine()),
        "save",
    )
    .unwrap();
    let version = Version::compose(
        envelope,
        fields.parse().unwrap(),
        Links::default(),
        body.to_owned(),
    )
    .unwrap();
    world.store.put(&version).unwrap();
    version
}

pub fn labels<T>(items: &[T], row_of: impl Fn(&T) -> &Row) -> Vec<String> {
    items
        .iter()
        .map(|item| row_of(item).label.clone())
        .collect()
}

pub fn head(world: &World, id: &DocumentId) -> Version {
    match world.store.document(id).expect("a read").state() {
        State::Live(head) => head.clone(),
        other => panic!("expected a live document, got {other:?}"),
    }
}

pub fn record(world: &World, id: &DocumentId) -> Record {
    read_or_skip(&head(world, id)).expect("a readable head")
}

pub fn found(world: &World, address: &str) -> Found {
    world.lookup().find(address).expect("a read")
}

pub fn refused<T>(result: Result<T, Failure>) -> String {
    match result {
        Err(Failure::Refused(text)) => text,
        Err(other) => panic!("expected a refusal, got {other}"),
        Ok(_) => panic!("expected a refusal"),
    }
}

pub struct Counting {
    pub inner: MemoryStore,
    pub documents: Cell<usize>,
    pub holdings: Cell<usize>,
    pub kinds: Cell<usize>,
    pub kind_lists: Cell<usize>,
    pub prefixes: Cell<usize>,
    pub unreadables: Cell<usize>,
    pub forks: Cell<usize>,
}

impl Counting {
    pub fn over(inner: MemoryStore) -> Counting {
        Counting {
            inner,
            documents: Cell::new(0),
            holdings: Cell::new(0),
            kinds: Cell::new(0),
            kind_lists: Cell::new(0),
            prefixes: Cell::new(0),
            unreadables: Cell::new(0),
            forks: Cell::new(0),
        }
    }
}

impl Counting {
    pub fn questions(&self) -> usize {
        self.documents.get() + self.holdings.get() + self.kinds.get()
    }

    // A scan reads more than one document's directory.
    pub fn scans(&self) -> usize {
        self.holdings.get()
            + self.kinds.get()
            + self.kind_lists.get()
            + self.prefixes.get()
            + self.unreadables.get()
            + self.forks.get()
    }
}

impl Store for Counting {
    fn document(&self, id: &DocumentId) -> Result<Document, StoreError> {
        self.documents.set(self.documents.get() + 1);
        self.inner.document(id)
    }

    fn put(&self, version: &Version) -> Result<(), StoreError> {
        self.inner.put(version)
    }

    fn of_kind(&self, kind: &Kind) -> Result<Vec<Document>, StoreError> {
        self.kinds.set(self.kinds.get() + 1);
        self.inner.of_kind(kind)
    }

    fn holding(&self, key: &str, values: &[&str]) -> Result<Vec<Document>, StoreError> {
        self.holdings.set(self.holdings.get() + 1);
        self.inner.holding(key, values)
    }

    fn unreadable(&self) -> Result<Vec<Document>, StoreError> {
        self.unreadables.set(self.unreadables.get() + 1);
        self.inner.unreadable()
    }

    fn forked(&self) -> Result<Vec<Document>, StoreError> {
        self.forks.set(self.forks.get() + 1);
        self.inner.forked()
    }

    fn kinds(&self) -> Result<Vec<Kind>, StoreError> {
        self.kind_lists.set(self.kind_lists.get() + 1);
        self.inner.kinds()
    }

    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        self.prefixes.set(self.prefixes.get() + 1);
        self.inner.documents_under(prefix)
    }

    fn versions_under(&self, prefix: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError> {
        self.prefixes.set(self.prefixes.get() + 1);
        self.inner.versions_under(prefix)
    }
}
