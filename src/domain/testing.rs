//! In-memory ports for tests of the layers above the domain, and the
//! contract every implementation of a port is held to.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::document::{Document, State, Unreadable};
use super::draft::{Draft, Listed};
use super::id::{DocumentId, VersionId};
use super::ports::{
    Binary, Clock, Drafts, DraftsRead, Host, HostRead, Ids, NotRead, Refreshed, Releases, Store,
    StoreError, StoreRead, Usage, UsageRead,
};
use super::release::{Built, ReleaseVersion, sha256_hex};
use super::session::Session;
use super::usage::{Invocation, Kept};
use super::version::{Envelope, Fields, Header, Kind, Links, ReadError, Stamp, Version};

pub struct FailingStore;

impl FailingStore {
    fn gone<T>() -> Result<T, StoreError> {
        Err(StoreError::io("memory:store", "gone"))
    }
}

impl StoreRead for FailingStore {
    fn headers(&self, _: &DocumentId) -> Result<Rc<Document>, StoreError> {
        FailingStore::gone()
    }

    fn version(
        &self,
        _: &DocumentId,
        _: &VersionId,
    ) -> Result<Result<Version, NotRead>, StoreError> {
        FailingStore::gone()
    }

    fn body(&self, _: &DocumentId, _: &VersionId) -> Result<Result<String, NotRead>, StoreError> {
        FailingStore::gone()
    }

    fn of_kind(&self, _: &Kind) -> Result<Vec<Rc<Document>>, StoreError> {
        FailingStore::gone()
    }

    fn holding(&self, _: &str, _: &[&str]) -> Result<Vec<Rc<Document>>, StoreError> {
        FailingStore::gone()
    }

    fn unreadable(&self) -> Result<Vec<Rc<Document>>, StoreError> {
        FailingStore::gone()
    }

    fn forked(&self) -> Result<Vec<Rc<Document>>, StoreError> {
        FailingStore::gone()
    }

    fn kinds(&self) -> Result<Vec<Kind>, StoreError> {
        FailingStore::gone()
    }

    fn documents_under(&self, _: &str) -> Result<Vec<DocumentId>, StoreError> {
        FailingStore::gone()
    }

    fn versions_under(&self, _: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError> {
        FailingStore::gone()
    }

    fn reread(&self) -> Result<(), StoreError> {
        FailingStore::gone()
    }
}

impl Store for FailingStore {
    fn put(&self, _: &Version) -> Result<(), StoreError> {
        FailingStore::gone()
    }
}

#[derive(Default)]
pub struct MemoryStore {
    versions: RefCell<BTreeMap<DocumentId, Vec<Version>>>,
    unreadable: RefCell<BTreeMap<DocumentId, Vec<Unreadable>>>,
}

impl MemoryStore {
    /// Makes the document hold an unreadable version beside its stored ones.
    pub fn plant_unreadable(&self, document: &DocumentId, version: VersionId, why: ReadError) {
        self.unreadable
            .borrow_mut()
            .entry(document.clone())
            .or_default()
            .push(Unreadable { version, why });
    }

    fn held(&self, id: &DocumentId) -> Document {
        let headers = (self.versions.borrow().get(id))
            .map(|versions| versions.iter().map(Version::header).cloned().collect())
            .unwrap_or_default();
        let unreadable = self
            .unreadable
            .borrow()
            .get(id)
            .cloned()
            .unwrap_or_default();
        Document::new(id.clone(), headers, unreadable)
    }

    // A version planted as unreadable reads as its damaged file would, stored or not.
    fn stored(&self, document: &DocumentId, version: &VersionId) -> Result<Version, NotRead> {
        let unreadable = self.unreadable.borrow();
        if let Some(file) =
            (unreadable.get(document).into_iter().flatten()).find(|file| file.version == *version)
        {
            return Err(NotRead::Unreadable(file.why.clone()));
        }
        let all = self.versions.borrow();
        (all.get(document).into_iter().flatten())
            .find(|held| held.id == *version)
            .cloned()
            .ok_or(NotRead::Missing)
    }

    fn matching(&self, wanted: impl Fn(&Document) -> bool) -> Vec<Rc<Document>> {
        let ids: BTreeSet<DocumentId> = self
            .versions
            .borrow()
            .keys()
            .chain(self.unreadable.borrow().keys())
            .cloned()
            .collect();
        ids.iter()
            .map(|id| self.held(id))
            .filter(|document| wanted(document))
            .map(Rc::new)
            .collect()
    }
}

impl StoreRead for MemoryStore {
    fn headers(&self, id: &DocumentId) -> Result<Rc<Document>, StoreError> {
        Ok(Rc::new(self.held(id)))
    }

    fn version(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<Version, NotRead>, StoreError> {
        Ok(self.stored(document, version))
    }

    fn body(
        &self,
        document: &DocumentId,
        version: &VersionId,
    ) -> Result<Result<String, NotRead>, StoreError> {
        Ok(self.stored(document, version).map(|version| version.body))
    }

    fn of_kind(&self, kind: &Kind) -> Result<Vec<Rc<Document>>, StoreError> {
        Ok(self.matching(|document| document.kind() == Some(kind)))
    }

    fn holding(&self, key: &str, values: &[&str]) -> Result<Vec<Rc<Document>>, StoreError> {
        if values.is_empty() {
            return Ok(Vec::new());
        }
        let asked: BTreeSet<&str> = values.iter().copied().collect();
        Ok(self.matching(|document| document.holds_any(key, &asked)))
    }

    fn unreadable(&self) -> Result<Vec<Rc<Document>>, StoreError> {
        Ok(self.matching(|document| !document.unreadable().is_empty()))
    }

    fn forked(&self) -> Result<Vec<Rc<Document>>, StoreError> {
        Ok(self.matching(|document| matches!(document.state(), State::Forked(_))))
    }

    fn kinds(&self) -> Result<Vec<Kind>, StoreError> {
        let held: BTreeSet<Kind> = self
            .matching(|_| true)
            .iter()
            .filter_map(|document| document.kind().cloned())
            .collect();
        Ok(held.into_iter().collect())
    }

    fn documents_under(&self, prefix: &str) -> Result<Vec<DocumentId>, StoreError> {
        let held: BTreeSet<DocumentId> = self
            .versions
            .borrow()
            .keys()
            .chain(self.unreadable.borrow().keys())
            .filter(|id| !prefix.is_empty() && id.as_str().starts_with(prefix))
            .cloned()
            .collect();
        Ok(held.into_iter().collect())
    }

    fn versions_under(&self, prefix: &str) -> Result<Vec<(DocumentId, VersionId)>, StoreError> {
        let mut found: Vec<(DocumentId, VersionId)> = self
            .versions
            .borrow()
            .iter()
            .flat_map(|(document, versions)| {
                versions
                    .iter()
                    .filter(|version| version.id.starts_with(prefix))
                    .map(|version| (document.clone(), version.id.clone()))
            })
            .chain(
                self.unreadable
                    .borrow()
                    .iter()
                    .flat_map(|(document, files)| {
                        files
                            .iter()
                            .filter(|file| file.version.starts_with(prefix))
                            .map(|file| (document.clone(), file.version.clone()))
                    }),
            )
            .collect();
        found.sort();
        Ok(found)
    }

    fn reread(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

impl Store for MemoryStore {
    fn put(&self, version: &Version) -> Result<(), StoreError> {
        let mut all = self.versions.borrow_mut();
        let versions = all.entry(version.envelope.document.clone()).or_default();
        if !versions.iter().any(|held| held.id == version.id) {
            versions.push(version.clone());
        }
        Ok(())
    }
}

/// The time `MemoryDrafts` gives each draft it writes, until `touch` sets another.
pub const DRAFTED: &str = "2026-10-09T18:10:00+01:00";

#[derive(Default)]
pub struct MemoryDrafts {
    drafts: RefCell<BTreeMap<DocumentId, Listed>>,
}

impl MemoryDrafts {
    /// # Panics
    ///
    /// When `document` has no draft or `at` is not a stamp.
    pub fn touch(&self, document: &DocumentId, at: &str) {
        let mut drafts = self.drafts.borrow_mut();
        let listed = drafts.get_mut(document).expect("a draft");
        listed.touched = Stamp::parse(at).expect("a stamp");
    }
}

impl DraftsRead for MemoryDrafts {
    fn read(&self, document: &DocumentId) -> Result<Option<Draft>, StoreError> {
        Ok(self
            .drafts
            .borrow()
            .get(document)
            .map(|listed| listed.draft.clone()))
    }

    fn list(&self) -> Result<Vec<Listed>, StoreError> {
        Ok(self.drafts.borrow().values().cloned().collect())
    }

    fn documents(&self) -> Result<BTreeSet<DocumentId>, StoreError> {
        Ok(self.drafts.borrow().keys().cloned().collect())
    }

    fn location(&self, document: &DocumentId) -> String {
        format!("memory:{document}")
    }
}

impl Drafts for MemoryDrafts {
    fn write(&self, draft: &Draft) -> Result<String, StoreError> {
        let listed = Listed {
            draft: draft.clone(),
            touched: Stamp::parse(DRAFTED).expect("a stamp"),
        };
        self.drafts
            .borrow_mut()
            .insert(draft.document.clone(), listed);
        Ok(self.location(&draft.document))
    }

    fn delete(&self, document: &DocumentId) -> Result<(), StoreError> {
        self.drafts.borrow_mut().remove(document);
        Ok(())
    }
}

/// Counts in the leading bytes, so the short ids differ.
#[derive(Default)]
pub struct SequenceIds {
    minted: Cell<u128>,
}

impl Ids for SequenceIds {
    fn mint(&self) -> Result<DocumentId, StoreError> {
        self.minted.set(self.minted.get() + 1);
        Ok(DocumentId::from_bytes(
            (self.minted.get() << 96).to_be_bytes(),
        ))
    }
}

#[must_use]
pub fn first_minted() -> DocumentId {
    DocumentId::from_bytes((1u128 << 96).to_be_bytes())
}

pub struct FixedClock(pub Stamp);

impl FixedClock {
    /// # Panics
    ///
    /// When `text` is not a stamp.
    #[must_use]
    pub fn at(text: &str) -> FixedClock {
        FixedClock(Stamp::parse(text).expect("a stamp"))
    }
}

impl Clock for FixedClock {
    fn now(&self) -> Stamp {
        self.0.clone()
    }
}

#[derive(Default)]
pub struct MemoryUsage {
    runs: RefCell<Vec<Invocation>>,
}

impl UsageRead for MemoryUsage {
    fn read(&self, kept: &Kept) -> Result<Vec<Invocation>, StoreError> {
        let runs = self.runs.borrow();
        Ok(runs.iter().filter(|run| kept.keeps(run)).cloned().collect())
    }
}

impl Usage for MemoryUsage {
    fn record(&self, invocation: &Invocation) -> Result<(), StoreError> {
        self.runs.borrow_mut().push(invocation.clone());
        Ok(())
    }
}

/// One published release, its assets by name, that remembers in `fetched` the tag an asset
/// was last asked for under.
#[derive(Default)]
pub struct MemoryReleases {
    pub latest: String,
    pub assets: BTreeMap<String, Vec<u8>>,
    pub fetched: Cell<Option<String>>,
}

impl Releases for MemoryReleases {
    fn latest(&self) -> Result<String, StoreError> {
        Ok(self.latest.clone())
    }

    fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, StoreError> {
        self.fetched.set(Some(tag.to_owned()));
        let held = self.assets.get(asset).cloned();
        held.ok_or_else(|| StoreError::io(asset, "no such asset"))
    }
}

pub const ASSET: &str = "lantern-x86_64-linux";

/// The `.sha256` file of an asset, `<hex>  <name>` as `shasum` writes it.
#[must_use]
pub fn checksum_file(asset: &str, bytes: &[u8]) -> String {
    format!("{}  {asset}\n", sha256_hex(bytes))
}

/// The release `tag` holding `asset` as `bytes` beside the checksum of `checksum_of`.
#[must_use]
pub fn published(tag: &str, asset: &str, bytes: &[u8], checksum_of: &[u8]) -> MemoryReleases {
    let checksum = checksum_file(asset, checksum_of).into_bytes();
    MemoryReleases {
        latest: tag.to_owned(),
        assets: BTreeMap::from([
            (asset.to_owned(), bytes.to_vec()),
            (format!("{asset}.sha256"), checksum),
        ]),
        fetched: Cell::new(None),
    }
}

pub const BINARY: &str = "/home/desk/bin/lantern";
pub const REFRESHED: &str = "/home/desk/.config/fish/completions/lantern.fish";

/// A binary at `BINARY` that remembers what replaced it and, in `calls`, each call made of
/// it in order; its refresh writes `REFRESHED`, notes `notes` and then stops with `stops`.
#[derive(Default)]
pub struct MemoryBinary {
    pub asset: Option<String>,
    pub notes: Vec<String>,
    pub replaced_with: RefCell<Option<Vec<u8>>>,
    pub calls: RefCell<Vec<&'static str>>,
    pub stops: Option<StoreError>,
}

impl MemoryBinary {
    #[must_use]
    pub fn built_as(asset: &str) -> MemoryBinary {
        MemoryBinary {
            asset: Some(asset.to_owned()),
            ..MemoryBinary::default()
        }
    }
}

impl Binary for MemoryBinary {
    fn asset(&self) -> Option<String> {
        self.asset.clone()
    }

    fn replace(&self, bytes: &[u8]) -> Result<String, StoreError> {
        *self.replaced_with.borrow_mut() = Some(bytes.to_vec());
        self.calls.borrow_mut().push("replace");
        Ok(BINARY.to_owned())
    }

    fn refresh(&self) -> Refreshed {
        self.calls.borrow_mut().push("refresh");
        Refreshed {
            written: vec![REFRESHED.to_owned()],
            notes: self.notes.clone(),
            ended: self.stops.clone().map_or(Ok(()), Err),
        }
    }
}

impl<B: Binary + ?Sized> Binary for Rc<B> {
    fn asset(&self) -> Option<String> {
        (**self).asset()
    }

    fn replace(&self, bytes: &[u8]) -> Result<String, StoreError> {
        (**self).replace(bytes)
    }

    fn refresh(&self) -> Refreshed {
        (**self).refresh()
    }
}

/// # Panics
///
/// When `text` is not a version.
#[must_use]
pub fn version(text: &str) -> ReleaseVersion {
    ReleaseVersion::parse(text).expect("a version")
}

pub const BUILD: &str = "3f2a1c9";

/// A build named `BUILD` made on top of the release `text`.
///
/// # Panics
///
/// When `text` is not a version.
#[must_use]
pub fn build(text: &str) -> Built {
    Built {
        release: version(text),
        build: Some(BUILD),
    }
}

#[must_use]
pub fn lamp_session() -> Session {
    Session {
        agent: "Lamp".to_owned(),
        id: "s-1".to_owned(),
    }
}

/// A run of `command` on `day` at ten, in no session, that succeeded.
///
/// # Panics
///
/// When `day` is not a date.
#[must_use]
pub fn ran(machine: &DocumentId, command: &str, day: &str) -> Invocation {
    Invocation {
        written: Stamp::parse(&format!("{day}T10:00:00+01:00")).expect("a day"),
        machine: machine.clone(),
        version: None,
        session: None,
        command: command.to_owned(),
        exit: 0,
        refusal: None,
        hits: None,
        directory: "~".to_owned(),
        arguments: Vec::new(),
    }
}

pub struct FixedHost {
    machine: RefCell<Option<DocumentId>>,
    home: Option<String>,
    binds: bool,
    links: Vec<(String, String)>,
    missing: Vec<String>,
    session: Option<Session>,
    name: Option<String>,
}

const FIXED_HOST: &str = "memory:host";

impl FixedHost {
    #[must_use]
    pub fn new(machine: Option<DocumentId>, home: Option<String>) -> FixedHost {
        FixedHost {
            machine: RefCell::new(machine),
            home,
            binds: true,
            links: Vec::new(),
            missing: Vec::new(),
            session: None,
            name: None,
        }
    }

    #[must_use]
    pub fn bound(&self) -> Option<DocumentId> {
        self.machine.borrow().clone()
    }

    pub fn set_machine(&mut self, machine: Option<DocumentId>) {
        self.machine.replace(machine);
    }

    pub fn set_home(&mut self, home: Option<String>) {
        self.home = home;
    }

    pub fn set_binds(&mut self, binds: bool) {
        self.binds = binds;
    }

    pub fn set_session(&mut self, session: Option<Session>) {
        self.session = session;
    }

    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    /// Makes `absolute` no directory; every other path is one.
    pub fn remove(&mut self, absolute: &str) {
        self.missing.push(absolute.to_owned());
    }

    /// Makes `link`, and every path under it, resolve to `target` and the same path under it.
    pub fn link(&mut self, link: &str, target: &str) {
        self.links.push((link.to_owned(), target.to_owned()));
    }
}

impl HostRead for FixedHost {
    fn machine(&self) -> Result<Option<DocumentId>, StoreError> {
        Ok(self.bound())
    }

    fn home(&self) -> Result<Option<String>, StoreError> {
        Ok(self.home.clone())
    }

    fn resolve(&self, absolute: &str) -> Result<String, StoreError> {
        let through = self.links.iter().find_map(|(link, target)| {
            let rest = absolute.strip_prefix(link.as_str())?;
            (rest.is_empty() || rest.starts_with('/')).then(|| format!("{target}{rest}"))
        });
        Ok(through.unwrap_or_else(|| absolute.to_owned()))
    }

    fn is_directory(&self, absolute: &str) -> Result<bool, StoreError> {
        Ok(!self.missing.iter().any(|missing| missing == absolute))
    }

    fn session(&self) -> Option<Session> {
        self.session.clone()
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }
}

impl Host for FixedHost {
    fn bind(&self, machine: &DocumentId) -> Result<(), StoreError> {
        if !self.binds {
            return Err(StoreError::io(FIXED_HOST, "does not bind"));
        }
        if self.bound().is_some() {
            return Err(StoreError::io(FIXED_HOST, "is already bound"));
        }
        self.machine.replace(Some(machine.clone()));
        Ok(())
    }
}

pub const LANTERN: &str = "7f3a91c05be2446d8a10c3f29b7e6d54";

#[must_use]
pub fn lantern() -> DocumentId {
    DocumentId::from_bytes(0x7f3a_91c0_5be2_446d_8a10_c3f2_9b7e_6d54_u128.to_be_bytes())
}

pub const ATLAS: &str = "a71a5000000000000000000000000001";

#[must_use]
pub fn atlas() -> DocumentId {
    DocumentId::from_bytes(0xa71a_5000_0000_0000_0000_0000_0000_0001_u128.to_be_bytes())
}

fn compose(envelope: Envelope, fields: &str, body: &str) -> Version {
    let fields: Fields = fields.parse().expect("fields in TOML");
    Version::compose(envelope, fields, Links::default(), body.to_owned()).expect("a version")
}

/// The moment every fixture is written at.
///
/// # Panics
///
/// Never.
#[must_use]
pub fn stamp() -> Stamp {
    Stamp::parse("2026-09-04T10:00:00+01:00").expect("a stamp")
}

/// The machine every fixture is written on.
#[must_use]
pub fn desk() -> DocumentId {
    DocumentId::from_bytes([0xde; 16])
}

/// The first version of `document`, its fields given as TOML.
///
/// # Panics
///
/// When the parts do not make a version.
#[must_use]
pub fn first(document: &DocumentId, kind: &str, fields: &str, body: &str) -> Version {
    let kind = Kind::parse(kind).expect("a kind");
    let envelope = Envelope::first(document.clone(), kind, stamp(), desk(), "new");
    compose(envelope, fields, body)
}

/// A version written on top of `heads`, its fields given as TOML.
///
/// # Panics
///
/// When there is no head to follow or the parts do not make a version.
#[must_use]
pub fn after(heads: &[&Version], fields: &str, body: &str) -> Version {
    let heads: Vec<&Header> = heads.iter().map(|head| head.header()).collect();
    let envelope = Envelope::following(&heads, stamp(), desk(), "save").expect("a head to follow");
    compose(envelope, fields, body)
}

/// What every `Store` answers, whatever it keeps its versions in.
///
/// # Panics
///
/// When the store answers otherwise.
pub fn store_contract(store: &dyn Store, ids: &dyn Ids, damage: &dyn Fn(&DocumentId, &VersionId)) {
    let lantern = ids.mint().expect("an id");
    let atlas = ids.mint().expect("an id");
    assert_ne!(lantern, atlas);
    assert_eq!(
        store.headers(&lantern).expect("a read").state(),
        State::Absent
    );
    assert!(store.kinds().expect("a read").is_empty());
    assert!(store.forked().expect("a read").is_empty());

    let topics = "topics = [\"lantern\", \"phone\"]";
    let relay = first(
        &lantern,
        "fact",
        &format!("name = \"relay\"\n{topics}"),
        "first\n",
    );
    store.put(&relay).expect("a write");
    store
        .put(&relay)
        .expect("a second write of the same version");
    assert_eq!(
        store.headers(&lantern).expect("a read").history(),
        [relay.header()]
    );

    let renamed = after(
        &[&relay],
        &format!("name = \"relay-pin\"\n{topics}"),
        "second\n",
    );
    store.put(&renamed).expect("a write");
    let held = store.headers(&lantern).expect("a read");
    assert_eq!(held.id(), &lantern);
    assert_eq!(held.state(), State::Live(renamed.header()));
    assert_eq!(held.history(), [renamed.header(), relay.header()]);
    assert_eq!(
        store
            .headers(&ids.mint().expect("an id"))
            .expect("a read")
            .state(),
        State::Absent
    );
    assert_eq!(
        store.version(&lantern, &relay.id).expect("a read"),
        Ok(relay.clone())
    );
    assert_eq!(
        store.body(&lantern, &relay.id).expect("a read").as_deref(),
        Ok("first\n")
    );
    let never = VersionId::of(b"never");
    assert_eq!(
        store.version(&lantern, &never).expect("a read"),
        Err(NotRead::Missing)
    );
    assert_eq!(
        store.body(&lantern, &never).expect("a read"),
        Err(NotRead::Missing)
    );

    store
        .put(&first(&atlas, "topic", "name = \"atlas\"", "\n"))
        .expect("a write");

    answers_with_headers(store, &lantern, &atlas);
    reports_what_does_not_read(store, ids, damage, &renamed);
    finds_by_prefix(store, &renamed);
    lists_the_kinds_held(store, ids);
    lists_what_is_forked(store, ids);
}

fn lists_what_is_forked(store: &dyn Store, ids: &dyn Ids) {
    let forked = || store.forked().expect("a read");
    assert!(forked().is_empty(), "no document held has two heads");
    let fork = |kind: &str| {
        let id = ids.mint().expect("an id");
        let root = first(&id, kind, "name = \"compass\"", "root\n");
        store.put(&root).expect("a write");
        for body in ["left\n", "right\n"] {
            store
                .put(&after(&[&root], "name = \"compass\"", body))
                .expect("a write");
        }
        id
    };
    let fact = fork("fact");
    assert_eq!(forked(), by_id(store, &[&fact]));
    let sketch = fork("sketch");
    assert_eq!(forked(), by_id(store, &[&fact, &sketch]));
}

fn lists_the_kinds_held(store: &dyn Store, ids: &dyn Ids) {
    let kinds = || -> Vec<String> {
        let held = store.kinds().expect("a read");
        held.iter().map(ToString::to_string).collect()
    };
    assert_eq!(kinds(), ["fact", "topic"], "a damaged file has no kind");
    for name in ["desk", "phone"] {
        let id = ids.mint().expect("an id");
        store
            .put(&first(&id, "sketch", &format!("name = \"{name}\""), "\n"))
            .expect("a write");
    }
    assert_eq!(kinds(), ["fact", "sketch", "topic"]);
    let found = of_kind(store, "sketch");
    assert_eq!(found.len(), 2);
}

fn by_id(store: &dyn Store, ids: &[&DocumentId]) -> Vec<Rc<Document>> {
    let mut documents: Vec<Rc<Document>> = ids
        .iter()
        .map(|id| store.headers(id).expect("a read"))
        .collect();
    documents.sort_by(|a, b| a.id().cmp(b.id()));
    documents
}

fn of_kind(store: &dyn Store, word: &str) -> Vec<Rc<Document>> {
    store
        .of_kind(&Kind::parse(word).expect("a kind"))
        .expect("a read")
}

fn answers_with_headers(store: &dyn Store, lantern: &DocumentId, atlas: &DocumentId) {
    let by_id = |ids: &[&DocumentId]| by_id(store, ids);
    let of_kind = |word: &str| of_kind(store, word);
    assert_eq!(of_kind("fact"), by_id(&[lantern]));
    assert_eq!(of_kind("topic"), by_id(&[atlas]));
    assert!(of_kind("entry").is_empty());

    let holding = |key: &str, values: &[&str]| store.holding(key, values).expect("a read");
    assert_eq!(holding("name", &["relay-pin"]), by_id(&[lantern]));
    assert_eq!(holding("topics", &["phone"]), by_id(&[lantern]));
    assert!(
        holding("name", &["relay"]).is_empty(),
        "only an older version has it"
    );
    assert!(holding("name", &["desk"]).is_empty());
    assert_eq!(
        holding("name", &["desk", "atlas", "relay-pin"]),
        by_id(&[lantern, atlas])
    );
    assert_eq!(
        holding("topics", &["lantern", "phone"]),
        by_id(&[lantern]),
        "a document holding two of the values is one answer"
    );
    assert!(holding("name", &[]).is_empty());
}

fn reports_what_does_not_read(
    store: &dyn Store,
    ids: &dyn Ids,
    damage: &dyn Fn(&DocumentId, &VersionId),
    head: &Version,
) {
    let lantern = &head.envelope.document;
    let whole = |id: &DocumentId| store.headers(id).expect("a read");
    let holding = |values: &[&str]| store.holding("name", values).expect("a read");
    assert!(store.unreadable().expect("a read").is_empty());
    let phone = ids.mint().expect("an id");
    let lost = first(&phone, "topic", "name = \"phone\"", "\n").id.clone();
    damage(&phone, &lost);
    let alone = whole(&phone);
    assert_eq!(alone.state(), State::Absent);
    assert_eq!(alone.kind(), None);
    assert_eq!(alone.unreadable().len(), 1);
    assert_eq!(store.unreadable().expect("a read"), [alone]);
    let under = store.documents_under(phone.as_str()).expect("a read");
    assert_eq!(under, std::slice::from_ref(&phone));
    let named = store.versions_under(lost.as_str()).expect("a read");
    assert_eq!(named, [(phone.clone(), lost.clone())]);
    for word in ["topic", "fact", "entry", "followup", "claim"] {
        let found = of_kind(store, word);
        assert!(
            found.iter().all(|document| document.id() != &phone),
            "{word}"
        );
    }
    assert!(holding(&["phone"]).is_empty());

    let torn = after(&[head], "name = \"relay\"", "third\n").id.clone();
    damage(lantern, &torn);
    let corrupt = NotRead::Unreadable(ReadError::Corrupt);
    assert_eq!(
        store.version(lantern, &torn).expect("a read"),
        Err(corrupt.clone())
    );
    assert_eq!(store.body(lantern, &torn).expect("a read"), Err(corrupt));
    let beside = whole(lantern);
    let named = store.versions_under(torn.short()).expect("a read");
    assert_eq!(named, [(lantern.clone(), torn.clone())]);
    assert_eq!(beside.state(), State::Live(head));
    assert_eq!(beside.unreadable().len(), 1);
    assert_eq!(
        store.unreadable().expect("a read"),
        by_id(store, &[lantern, &phone])
    );
    assert_eq!(of_kind(store, "fact"), std::slice::from_ref(&beside));
    assert_eq!(holding(&["relay-pin"]), [beside]);
}

fn finds_by_prefix(store: &dyn Store, head: &Version) {
    let lantern = &head.envelope.document;
    for length in [1, 2, 5] {
        let under = store
            .documents_under(&lantern.as_str()[..length])
            .expect("a read");
        assert!(under.contains(lantern), "{length}: {under:?}");
    }
    assert_eq!(
        store.documents_under(lantern.as_str()).expect("a read"),
        std::slice::from_ref(lantern)
    );
    assert!(store.documents_under("").expect("a read").is_empty());

    let found = [(lantern.clone(), head.id.clone())];
    for prefix in [head.id.short(), head.id.as_str()] {
        assert_eq!(store.versions_under(prefix).expect("a read"), found);
    }
    for all in ["", "b3-"] {
        assert!(
            store.versions_under(all).expect("a read").is_empty(),
            "{all}"
        );
    }
}

/// What every `Drafts` answers, wherever it keeps them.
///
/// # Panics
///
/// When the drafts answer otherwise.
pub fn drafts_contract(drafts: &dyn Drafts, ids: &dyn Ids) {
    let lantern = ids.mint().expect("an id");
    let atlas = ids.mint().expect("an id");
    assert_eq!(drafts.read(&lantern).expect("a read"), None);
    assert!(drafts.list().expect("a read").is_empty());
    drafts.delete(&lantern).expect("deleting no draft");

    let draft = Draft::of(&first(&lantern, "fact", "name = \"relay\"", "first\n"));
    assert_eq!(
        drafts.write(&draft).expect("a write"),
        drafts.location(&lantern)
    );
    assert_eq!(drafts.read(&lantern).expect("a read"), Some(draft.clone()));

    let opened = Draft {
        opened_in: Some(lamp_session()),
        ..draft.clone()
    };
    drafts.write(&opened).expect("a write");
    assert_eq!(drafts.read(&lantern).expect("a read"), Some(opened));

    let edited = draft
        .with_text("+++\nname = \"relay-pin\"\n+++\nsecond\n")
        .expect("a draft");
    drafts.write(&edited).expect("a write over the draft");
    assert_eq!(drafts.read(&lantern).expect("a read"), Some(edited.clone()));

    let topic = Draft::first(
        atlas.clone(),
        Kind::parse("topic").expect("a kind"),
        Fields::new(),
        String::new(),
    );
    drafts.write(&topic).expect("a write");
    let mut expected = vec![edited, topic];
    expected.sort_by(|a, b| a.document.cmp(&b.document));
    let listed: Vec<Draft> = (drafts.list().expect("a read").into_iter())
        .map(|listed| listed.draft)
        .collect();
    assert_eq!(listed, expected);
    let documents = BTreeSet::from([lantern.clone(), atlas.clone()]);
    assert_eq!(drafts.documents().expect("a read"), documents);

    drafts.delete(&lantern).expect("a delete");
    assert_eq!(drafts.read(&lantern).expect("a read"), None);
    assert_eq!(drafts.list().expect("a read").len(), 1);
    assert_eq!(drafts.documents().expect("a read"), BTreeSet::from([atlas]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_store_keeps_the_contract() {
        let store = MemoryStore::default();
        store_contract(&store, &SequenceIds::default(), &|document, version| {
            store.plant_unreadable(document, version.clone(), ReadError::Corrupt);
        });
    }

    #[test]
    fn the_memory_drafts_keep_the_contract() {
        drafts_contract(&MemoryDrafts::default(), &SequenceIds::default());
    }

    #[test]
    fn a_fixed_clock_and_host_answer_what_they_hold() {
        let clock = FixedClock::at("2026-10-09T18:22:41.118204+01:00");
        assert_eq!(clock.now(), clock.now());
        assert_eq!(clock.now().day(), "2026-10-09");
        let mut host = FixedHost::new(Some(atlas()), Some("/home/desk".to_owned()));
        assert_eq!(host.name(), None);
        host.set_name(Some("Desk.local".to_owned()));
        assert_eq!(host.name(), Some("Desk.local".to_owned()));
        assert_eq!(host.machine(), Ok(Some(atlas())));
        assert_eq!(host.home(), Ok(Some("/home/desk".to_owned())));
        assert_eq!(FixedHost::new(None, None).machine(), Ok(None));
        assert_eq!(FixedHost::new(None, None).home(), Ok(None));
    }

    #[test]
    fn a_fixed_host_is_in_the_session_it_is_given() {
        let mut host = FixedHost::new(None, None);
        assert_eq!(host.session(), None);
        let session = lamp_session();
        host.set_session(Some(session.clone()));
        assert_eq!(host.session(), Some(session));
    }

    #[test]
    fn the_memory_usage_gives_back_the_runs_kept_in_the_order_recorded() {
        let usage = MemoryUsage::default();
        assert_eq!(usage.read(&Kept::default()), Ok(Vec::new()));
        let runs = [
            ran(&lantern(), "show", "2026-09-04"),
            ran(&atlas(), "facts", "2026-09-05"),
        ];
        for run in &runs {
            usage.record(run).unwrap();
        }
        assert_eq!(usage.read(&Kept::default()), Ok(runs.to_vec()));
        let of_atlas = Kept {
            machine: Some(atlas()),
            ..Kept::default()
        };
        assert_eq!(usage.read(&of_atlas), Ok(runs[1..].to_vec()));
    }

    #[test]
    fn a_fixed_host_reads_a_path_through_the_links_it_is_given() {
        let mut host = FixedHost::new(None, None);
        assert_eq!(
            host.resolve("/home/desk/link"),
            Ok("/home/desk/link".to_owned())
        );
        host.link("/home/desk/link", "/home/desk/projects/lantern");
        for (given, read) in [
            ("/home/desk/link", "/home/desk/projects/lantern"),
            ("/home/desk/link/case", "/home/desk/projects/lantern/case"),
            ("/home/desk/linked", "/home/desk/linked"),
            ("/home/desk", "/home/desk"),
        ] {
            assert_eq!(host.resolve(given), Ok(read.to_owned()), "{given}");
        }
    }

    #[test]
    fn a_fixed_host_is_bound_once_and_only_when_it_binds() {
        let mut host = FixedHost::new(None, Some("/home/desk".to_owned()));
        host.set_binds(false);
        assert!(host.bind(&lantern()).is_err());
        assert_eq!(host.machine(), Ok(None));

        host.set_binds(true);
        host.bind(&lantern()).unwrap();
        assert_eq!(host.machine(), Ok(Some(lantern())));
        assert_eq!(host.bound(), Some(lantern()));
        assert_eq!(host.home(), Ok(Some("/home/desk".to_owned())));
        assert!(host.bind(&atlas()).is_err());
        assert_eq!(host.machine(), Ok(Some(lantern())));

        host.set_machine(None);
        host.set_home(None);
        assert_eq!(host.machine(), Ok(None));
        assert_eq!(host.home(), Ok(None));
    }

    #[test]
    fn a_planted_version_is_unreadable_beside_the_stored_ones() {
        let store = MemoryStore::default();
        let relay = first(&lantern(), "fact", "name = \"relay\"", "text\n");
        store.put(&relay).unwrap();
        let planted = first(&lantern(), "fact", "name = \"other\"", "text\n")
            .id
            .clone();
        store.plant_unreadable(&lantern(), planted, ReadError::Corrupt);
        let held = store.headers(&lantern()).unwrap();
        assert_eq!(held.unreadable().len(), 1);
        assert_eq!(held.state(), State::Live(&relay));

        let planted = relay.id.clone();
        store.plant_unreadable(&atlas(), planted, ReadError::Corrupt);
        let alone = store.headers(&atlas()).unwrap();
        assert_eq!(alone.unreadable().len(), 1);
        assert_eq!(alone.state(), State::Absent);
    }

    #[test]
    fn sequence_ids_count_up() {
        let ids = SequenceIds::default();
        assert_eq!(
            ids.mint().unwrap().as_str(),
            "00000001000000000000000000000000"
        );
        assert_eq!(
            ids.mint().unwrap().as_str(),
            "00000002000000000000000000000000"
        );
    }
}
