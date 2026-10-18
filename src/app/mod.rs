use std::cell::RefCell;
use std::fmt;

use serde::Serialize;
use toml::Value;

use crate::domain::id::{DocumentId, VersionId};
use crate::domain::ports::{
    Clock, Drafts, DraftsRead, Host, HostRead, Ids, Store, StoreError, Usage, UsageRead,
};
use crate::domain::schema::{Date, Directory, KindOf, SchemaError};
use crate::domain::version::Kind;

pub mod amend;
pub mod bulk;
pub mod check;
pub mod claim;
pub mod context;
pub mod draft;
pub mod followup;
pub mod fork;
mod heads;
mod index;
pub mod list;
mod live;
mod lookup;
pub mod machines;
pub mod offer;
pub mod record;
mod rows;
mod rules;
pub mod save;
pub mod search;
pub mod setup;
pub mod show;
#[cfg(test)]
pub(crate) mod testing;
pub mod upgrade;
pub mod usage;

pub use lookup::Stored;
pub use rows::{FollowupRow, Reference, Row, TriggerShown};

/// Read access to stored documents, drafts, usage, host state, and the clock.
///
/// ```
/// use worklog::app::{Failure, ReadDeps, context, draft, list, show, usage};
/// fn read(deps: &ReadDeps) -> Result<(), Failure> {
///     list::facts(deps, None, false)?;
///     draft::drafts(deps)?;
///     context::context(deps, "/srv")?;
///     show::diff(deps, "lantern", None)?;
///     usage::usage(deps, None, None)?;
///     Ok(())
/// }
/// ```
///
/// ```
/// use worklog::{
///     app::{ReadDeps, save},
///     domain::{draft::Draft, id::DocumentId, usage::Invocation, version::Version},
/// };
/// fn inputs(_: &ReadDeps, _: &Version, _: &Draft, _: &DocumentId, _: &Invocation) {
///     let _ = save::save;
/// }
/// ```
///
/// ```compile_fail
/// use worklog::{app::ReadDeps, domain::version::Version};
/// fn write(deps: &ReadDeps, version: &Version) {
///     deps.store.put(version);
/// }
/// ```
///
/// ```compile_fail
/// use worklog::{app::ReadDeps, domain::draft::Draft};
/// fn write(deps: &ReadDeps, draft: &Draft) {
///     deps.drafts.write(draft);
/// }
/// ```
///
/// ```compile_fail
/// use worklog::{app::ReadDeps, domain::id::DocumentId};
/// fn delete(deps: &ReadDeps, id: &DocumentId) {
///     deps.drafts.delete(id);
/// }
/// ```
///
/// ```compile_fail
/// use worklog::{app::ReadDeps, domain::usage::Invocation};
/// fn record(deps: &ReadDeps, invocation: &Invocation) {
///     deps.usage.record(invocation);
/// }
/// ```
///
/// ```compile_fail
/// use worklog::{app::ReadDeps, domain::id::DocumentId};
/// fn bind(deps: &ReadDeps, id: &DocumentId) {
///     deps.host.bind(id);
/// }
/// ```
///
/// ```compile_fail
/// use worklog::app::ReadDeps;
/// fn mint(deps: &ReadDeps) {
///     deps.ids.mint();
/// }
/// ```
///
/// ```compile_fail
/// use worklog::app::{ReadDeps, save};
/// fn change(deps: &ReadDeps) {
///     save::save(deps, "lantern");
/// }
/// ```
#[derive(Clone, Copy)]
pub struct ReadDeps<'a> {
    pub store: Stored<'a>,
    pub drafts: &'a dyn DraftsRead,
    pub clock: &'a dyn Clock,
    pub host: &'a dyn HostRead,
    pub usage: &'a dyn UsageRead,
}

#[derive(Clone, Copy)]
pub struct Deps<'a> {
    writer: save::Writer<'a>,
    drafts: &'a dyn Drafts,
    ids: &'a dyn Ids,
    clock: &'a dyn Clock,
    host: &'a dyn Host,
    usage: &'a dyn Usage,
}

impl<'a> Deps<'a> {
    #[must_use]
    pub fn new(
        store: &'a dyn Store,
        drafts: &'a dyn Drafts,
        ids: &'a dyn Ids,
        clock: &'a dyn Clock,
        host: &'a dyn Host,
        usage: &'a dyn Usage,
    ) -> Deps<'a> {
        Deps {
            writer: save::Writer::new(store),
            drafts,
            ids,
            clock,
            host,
            usage,
        }
    }

    #[must_use]
    pub fn read(&self) -> ReadDeps<'a> {
        ReadDeps {
            store: self.writer.read(),
            drafts: self.drafts,
            clock: self.clock,
            host: self.host,
            usage: self.usage,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The store and the arguments are fine and the operation is not allowed on them.
    Refused(String),
    /// The arguments name no valid operation.
    Usage(String),
    Store(StoreError),
}

impl Failure {
    pub fn at(what: impl fmt::Display, why: impl fmt::Display) -> Failure {
        Failure::Refused(format!("{what}: {why}"))
    }

    #[must_use]
    pub fn not_set_up() -> Failure {
        Failure::Refused("this host is not set up; run `worklog init`".to_owned())
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Refused(text) | Failure::Usage(text) => f.write_str(text),
            Failure::Store(error) => error.fmt(f),
        }
    }
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Failure {
        Failure::Store(error)
    }
}

impl From<SchemaError> for Failure {
    fn from(error: SchemaError) -> Failure {
        Failure::Refused(error.to_string())
    }
}

fn kind_named(kind: KindOf) -> Kind {
    Kind::parse(kind.word()).expect("a kind word is a kind")
}

pub(super) fn text(value: impl Into<String>) -> Value {
    Value::String(value.into())
}

/// Keeps the first failure of a closure that cannot return one.
#[derive(Default)]
pub(super) struct Failed(RefCell<Option<Failure>>);

impl Failed {
    pub(super) fn keep(&self, failure: Failure) {
        self.0.borrow_mut().get_or_insert(failure);
    }

    pub(super) fn done(self) -> Result<(), Failure> {
        self.0.into_inner().map_or(Ok(()), Err)
    }
}

pub(crate) fn counted(count: usize, one: &str, several: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { several })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Written {
    pub document: DocumentId,
    /// The document's label as of the stored version.
    pub label: String,
    pub version: VersionId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftRef {
    pub document: DocumentId,
    #[serde(rename = "path")]
    pub location: String,
}

impl ReadDeps<'_> {
    pub fn today(&self) -> Result<Date, Failure> {
        Ok(Date::parse(&self.clock.now().day())?)
    }

    /// Refuses when the host is not set up.
    pub fn machine(&self) -> Result<DocumentId, Failure> {
        self.host.machine()?.ok_or_else(Failure::not_set_up)
    }

    // Only a path from the root is one the host's file system reads, and `..` after a link
    // means the directory the link is in only while the link is not yet followed.
    fn on_host(&self, path: &str) -> Result<String, StoreError> {
        if path.starts_with('/') {
            self.host.resolve(&Directory::folded(path))
        } else {
            Ok(path.to_owned())
        }
    }

    pub(super) fn home(&self) -> Result<Option<String>, Failure> {
        let home = self.host.home()?;
        Ok(home.map(|home| self.on_host(&home)).transpose()?)
    }

    pub(super) fn directory(&self, given: &str) -> Result<Directory, Failure> {
        Directory::on_host(&self.on_host(given)?, self.home()?.as_deref())
            .map_err(|error| Failure::Usage(format!("{given:?}: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::testing::{
        FixedClock, FixedHost, MemoryDrafts, MemoryStore, MemoryUsage, SequenceIds, atlas,
    };

    #[test]
    fn a_listing_reads_no_body() {
        use crate::app::testing::{DONE, World, entry, fact, followup, topic};
        type Listing = fn(&ReadDeps) -> Result<(), Failure>;
        let listings: [(&str, Listing); 12] = [
            ("topics", |deps| list::topics(deps, None, false).map(drop)),
            ("facts", |deps| list::facts(deps, None, false).map(drop)),
            ("ideas", |deps| list::ideas(deps, None, false).map(drop)),
            ("entries", |deps| {
                list::entries(deps, Some("lantern"), None, false).map(drop)
            }),
            ("followups", |deps| {
                list::followups(deps, None, true).map(drop)
            }),
            ("forks", |deps| list::forks(deps).map(drop)),
            ("log", |deps| search::log(deps, 10, None).map(drop)),
            ("history", |deps| show::history(deps, "lantern").map(drop)),
            ("context", |deps| context::context(deps, "/work").map(drop)),
            ("due", |deps| context::due(deps, "/work").map(drop)),
            ("machines", |deps| machines::machines(deps).map(drop)),
            ("check", |deps| check::check(deps).map(drop)),
        ];
        for (name, listing) in listings {
            let world = World::new();
            topic(&world, "desk", "");
            let lantern = topic(&world, "lantern", "");
            fact(&world, &lantern, "relay-pin", "");
            entry(&world, "2026-09-04", "lamp-driver", &[&lantern]);
            followup(&world, &[&lantern], "open", "");
            followup(&world, &[&lantern], "done", DONE);
            let world = world.counted();
            listing(&world.deps().read()).unwrap_or_else(|failure| panic!("{name}: {failure}"));
            assert_eq!(world.store.bodies(), 0, "{name}");
        }
    }

    #[test]
    fn a_failure_reads_as_its_text() {
        assert_eq!(
            Failure::at("lantern", "is ended").to_string(),
            "lantern: is ended"
        );
        assert_eq!(
            Failure::from(SchemaError::Ended),
            Failure::Refused(SchemaError::Ended.to_string())
        );
        let error = StoreError::io("desk", "gone");
        assert_eq!(Failure::from(error.clone()), Failure::Store(error.clone()));
        assert_eq!(Failure::Store(error.clone()).to_string(), error.to_string());
        assert_eq!(Failure::Usage("no".to_owned()).to_string(), "no");
    }

    #[test]
    fn a_count_takes_the_singular_for_one_alone() {
        assert_eq!(counted(1, "entry", "entries"), "1 entry");
        assert_eq!(counted(0, "entry", "entries"), "0 entries");
        assert_eq!(counted(2, "entry", "entries"), "2 entries");
    }

    #[test]
    fn a_directory_and_the_home_it_is_folded_under_are_read_as_the_host_resolves_them() {
        let store = MemoryStore::default();
        let drafts = MemoryDrafts::default();
        let ids = SequenceIds::default();
        let usage = MemoryUsage::default();
        let clock = FixedClock::at("2026-10-09T18:22:41.118204+01:00");
        let mut host = FixedHost::new(Some(atlas()), Some("/home/desk".to_owned()));
        host.link("/home/desk", "/atlas/desk");
        host.link("/srv/link", "/atlas/desk/projects/lantern");
        host.link("/srv/phone", "/atlas/phone");
        let deps = Deps::new(&store, &drafts, &ids, &clock, &host, &usage);
        for (given, held) in [
            ("/home/desk/projects/lantern", "~/projects/lantern"),
            ("/atlas/desk/projects/lantern", "~/projects/lantern"),
            ("/srv/link/case", "~/projects/lantern/case"),
            ("/home/desk", "~"),
            ("/srv/phone/case", "/atlas/phone/case"),
            ("/srv/lantern", "/srv/lantern"),
            ("~/projects/lantern", "~/projects/lantern"),
        ] {
            assert_eq!(
                deps.read().directory(given).unwrap().as_str(),
                held,
                "{given}"
            );
        }
        let Err(Failure::Usage(text)) = deps.read().directory("srv/link") else {
            panic!("a path that is not from the root is no directory");
        };
        assert!(text.starts_with("\"srv/link\": "), "{text}");
    }

    #[test]
    fn today_is_the_clock_s_day_and_the_machine_needs_a_host() {
        let store = MemoryStore::default();
        let drafts = MemoryDrafts::default();
        let ids = SequenceIds::default();
        let usage = MemoryUsage::default();
        let clock = FixedClock::at("2026-10-09T18:22:41.118204+01:00");
        let bare = FixedHost::new(None, None);
        let set_up = FixedHost::new(Some(atlas()), None);
        let deps = |host| Deps::new(&store, &drafts, &ids, &clock, host, &usage);

        assert_eq!(
            deps(&bare).read().today().unwrap(),
            Date::parse("2026-10-09").unwrap()
        );
        let Err(Failure::Refused(text)) = deps(&bare).read().machine() else {
            panic!("a host with no machine topic must be refused");
        };
        assert_eq!(text, "this host is not set up; run `worklog init`");
        assert_eq!(deps(&set_up).read().machine(), Ok(atlas()));
    }
}
