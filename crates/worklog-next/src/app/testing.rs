use crate::app::lookup::{Lookup, read_or_skip};
use crate::app::{Deps, Failure};
use crate::domain::document::State;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::ports::{Clock, Ids, Store};
use crate::domain::schema::Record;
use crate::domain::schema::address::Found;
use crate::domain::testing::{
    FixedClock, FixedHost, MemoryDrafts, MemoryStore, SequenceIds, after,
};
use crate::domain::version::{Envelope, Fields, Kind, Links, Version};

pub struct World {
    pub store: MemoryStore,
    pub drafts: MemoryDrafts,
    pub ids: SequenceIds,
    pub clock: FixedClock,
    pub host: FixedHost,
}

impl World {
    /// A world on 2026-10-09 whose host's machine topic is the first id `ids` mints.
    pub fn new() -> World {
        World {
            store: MemoryStore::default(),
            drafts: MemoryDrafts::default(),
            ids: SequenceIds::default(),
            clock: FixedClock::at("2026-10-09T18:22:41.118204+01:00"),
            host: FixedHost(Some(DocumentId::from_bytes(1u128.to_be_bytes()))),
        }
    }

    pub fn deps(&self) -> Deps<'_> {
        Deps {
            store: &self.store,
            drafts: &self.drafts,
            ids: &self.ids,
            clock: &self.clock,
            host: &self.host,
        }
    }

    fn machine(&self) -> DocumentId {
        self.host.0.clone().expect("a host with a machine topic")
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

pub fn topic(world: &World, name: &str, rest: &str) -> DocumentId {
    world.put("topic", &format!("name = \"{name}\"\n{TOPIC}{rest}"), "\n")
}

/// A world holding the machine topic `desk` and the topic `lantern`.
pub fn world() -> (World, DocumentId) {
    let world = World::new();
    topic(&world, "desk", "");
    let lantern = topic(&world, "lantern", "");
    (world, lantern)
}

/// `world`, with the topic `atlas` after `lantern`.
pub fn world_with_atlas() -> (World, DocumentId, DocumentId) {
    let (world, lantern) = world();
    let atlas = topic(&world, "atlas", "");
    (world, lantern, atlas)
}

pub fn fact(world: &World, topic: &DocumentId, name: &str, rest: &str) -> DocumentId {
    world.put(
        "fact",
        &format!(
            "name = \"{name}\"\ntopic = \"{topic}\"\ncreated = 2026-09-04\n\
             confirmed = 2026-09-04\nsummary = \"s\"\n{rest}"
        ),
        "The relay.\n",
    )
}

/// The entry `2026-10-08-lamp-driver`, made on the host's machine.
pub fn entry(world: &World, topics: &[&DocumentId]) -> DocumentId {
    let topics: Vec<String> = topics.iter().map(|id| format!("\"{id}\"")).collect();
    world.put(
        "entry",
        &format!(
            "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{}\"\n\
             topics = [{}]\nsummary = \"s\"\n",
            world.machine(),
            topics.join(", ")
        ),
        "\n",
    )
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
    Lookup::new(&world.store).find(address).expect("a read")
}

pub fn refused<T>(result: Result<T, Failure>) -> String {
    match result {
        Err(Failure::Refused(text)) => text,
        Err(other) => panic!("expected a refusal, got {other}"),
        Ok(_) => panic!("expected a refusal"),
    }
}
