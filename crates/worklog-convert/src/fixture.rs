//! An invented store in the format being converted, written by that format's own writers,
//! and what the tests of several modules share.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use worklog::domain::id::DocumentId;
use worklog_old::domain::entry::Entry;
use worklog_old::domain::fact::Fact;
use worklog_old::domain::followup::{Followup, FollowupState};
use worklog_old::domain::frontmatter::{Fields, Value};
use worklog_old::domain::machine::MachineName;
use worklog_old::domain::ports::Store as _;
use worklog_old::domain::slug::{Kind, Slug};
use worklog_old::domain::topic::Topic;
use worklog_old::domain::version::{
    RENAME, TOMBSTONE, Version, VersionBlock, VersionId, note_body, with_note,
};
use worklog_old::fs::FsStore;

use crate::key::DocumentKey;
use crate::old;
use crate::plan::{Document, Planned};

#[derive(Clone, Copy)]
pub(crate) struct At<'a> {
    written: &'a str,
    machine: &'a str,
}

pub(crate) fn at<'a>(written: &'a str, machine: &'a str) -> At<'a> {
    At { written, machine }
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn block(at: At, operation: &str, parents: &[&Version]) -> VersionBlock {
    VersionBlock {
        parents: parents.iter().map(|parent| parent.id.clone()).collect(),
        written: at.written.to_owned(),
        machine: MachineName::parse(at.machine).unwrap(),
        operation: operation.to_owned(),
        superseded_by: None,
        renamed_from: None,
        raw: None,
    }
}

fn accepted(version: Version) -> Version {
    assert_eq!(Version::from_text(&version.to_text()), Ok(version.clone()));
    version
}

pub(crate) fn composed(
    slug: &str,
    at: At,
    operation: &str,
    parents: &[&Version],
    fields: Fields,
    body: &str,
) -> Version {
    accepted(Version::compose(
        Slug::parse(slug).unwrap(),
        block(at, operation, parents),
        fields,
        body.to_owned(),
    ))
}

fn as_written(kind: Kind, fields: &Fields) -> Fields {
    match kind {
        Kind::Entry => Entry::from_fields(fields).unwrap().to_fields(),
        Kind::Fact => Fact::from_fields(fields).unwrap().to_fields(),
        Kind::Topic => Topic::from_fields(fields).unwrap().to_fields(),
        Kind::Followup => Followup::from_fields(fields).unwrap().to_fields(),
    }
}

pub(crate) fn following(
    parent: &Version,
    at: At,
    operation: &str,
    changes: &[(&str, Value)],
    body: Option<&str>,
) -> Version {
    let mut fields = parent.fields.clone();
    for (key, value) in changes {
        fields.set(key, value.clone());
    }
    let fields = as_written(parent.slug.kind(), &fields);
    composed(
        parent.slug.path(),
        at,
        operation,
        &[parent],
        fields,
        body.unwrap_or(&parent.body),
    )
}

pub(crate) fn scalar(text: &str) -> Value {
    Value::Scalar(text.to_owned())
}

pub(crate) fn list(items: &[&str]) -> Value {
    Value::List(owned(items))
}

pub(crate) fn claims(of: &[(&str, &[&str])]) -> Value {
    let claimed = |(topic, paths): &(&str, &[&str])| ((*topic).to_owned(), list(paths));
    Value::Map(of.iter().map(claimed).collect())
}

pub(crate) struct OldStore {
    kept: TempDir,
    store: FsStore,
}

impl OldStore {
    pub(crate) fn new() -> OldStore {
        let kept = tempfile::tempdir().unwrap();
        let store = FsStore::new(kept.path().join("store"));
        OldStore { kept, store }
    }

    pub(crate) fn root(&self) -> &Path {
        self.store.root()
    }

    pub(crate) fn beside(&self, name: &str) -> PathBuf {
        self.kept.path().join(name)
    }

    pub(crate) fn write(&self, version: &Version) {
        self.store.put(version).unwrap();
    }

    fn written(&self, version: Version) -> Version {
        self.write(&version);
        version
    }

    fn first(&self, slug: &Slug, at: At, fields: Fields, body: &str) -> Version {
        self.written(composed(slug.path(), at, "new", &[], fields, body))
    }

    pub(crate) fn slugs(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for kind in Kind::ALL {
            for slug in self.store.slugs(kind).unwrap() {
                found.push((kind.dir().to_owned(), slug.path().to_owned()));
            }
        }
        found
    }

    pub(crate) fn file_of(&self, version: &Version) -> PathBuf {
        self.root()
            .join(version.slug.kind().dir())
            .join(version.slug.path())
            .join(format!("{}.md", version.id))
    }

    pub(crate) fn topic(&self, name: &str, summary: &str, includes: &[&str], at: At) -> Version {
        let topic = Topic {
            summary: summary.to_owned(),
            includes: owned(includes),
            ..Topic::default()
        };
        let slug = Slug::of_kind(Kind::Topic, name).unwrap();
        self.first(&slug, at, topic.to_fields(), "\n")
    }

    pub(crate) fn machine_topic(
        &self,
        name: &str,
        claims: &[(&str, &[&str])],
        unclaimed: &[&str],
        at: At,
    ) -> Version {
        let topic = Topic {
            summary: format!("The machine {}", at.machine),
            includes: Vec::new(),
            machine: Some(MachineName::parse(at.machine).unwrap()),
            claims: (claims.iter())
                .map(|(topic, paths)| ((*topic).to_owned(), owned(paths)))
                .collect(),
            unclaimed: owned(unclaimed),
        };
        let slug = Slug::of_kind(Kind::Topic, name).unwrap();
        self.first(&slug, at, topic.to_fields(), "\n")
    }

    pub(crate) fn fact(&self, slug: &str, summary: &str, tags: &[&str], at: At) -> Version {
        self.fact_or_idea(slug, summary, tags, false, at)
    }

    pub(crate) fn idea(&self, slug: &str, summary: &str, tags: &[&str], at: At) -> Version {
        self.fact_or_idea(slug, summary, tags, true, at)
    }

    fn fact_or_idea(
        &self,
        slug: &str,
        summary: &str,
        tags: &[&str],
        idea: bool,
        at: At,
    ) -> Version {
        let fact = Fact {
            tags: owned(tags),
            idea,
            recheck: None,
            verified: None,
            summary: summary.to_owned(),
        };
        let slug = Slug::of_kind(Kind::Fact, slug).unwrap();
        self.first(&slug, at, fact.to_fields(), "\nWhat holds.\n")
    }

    pub(crate) fn entry(&self, slug: &str, summary: &str, tags: &[&str], at: At) -> Version {
        let slug = Slug::of_kind(Kind::Entry, slug).unwrap();
        let entry = Entry {
            date: slug.date().unwrap().to_owned(),
            machine: MachineName::parse(at.machine).unwrap(),
            tags: owned(tags),
            files_touched: Vec::new(),
            summary: summary.to_owned(),
        };
        let body = format!("\n## What\n{summary}\n");
        self.first(&slug, at, entry.to_fields(), &body)
    }

    pub(crate) fn followup(
        &self,
        slug: &str,
        entry: &Version,
        summary: &str,
        tags: &[&str],
        at: At,
    ) -> Version {
        let followup = Followup {
            entry: entry.slug.clone(),
            tags: owned(tags),
            recheck: None,
            state: FollowupState::Open,
            summary: summary.to_owned(),
        };
        let slug = Slug::of_kind(Kind::Followup, slug).unwrap();
        self.first(&slug, at, followup.to_fields(), "\n")
    }

    pub(crate) fn changed(
        &self,
        parent: &Version,
        operation: &str,
        changes: &[(&str, Value)],
        at: At,
    ) -> Version {
        self.written(following(parent, at, operation, changes, None))
    }

    pub(crate) fn saved(&self, parent: &Version, summary: &str, body: &str, at: At) -> Version {
        let changes = [("summary", scalar(summary))];
        self.written(following(parent, at, "save", &changes, Some(body)))
    }

    pub(crate) fn done(&self, parent: &Version, note: &str, at: At) -> Version {
        let body = with_note(parent.body.clone(), Some(note));
        let changes = [("state", scalar("done"))];
        self.written(following(parent, at, "done", &changes, Some(&body)))
    }

    pub(crate) fn dropped(&self, parent: &Version, note: &str, at: At) -> Version {
        let body = with_note(parent.body.clone(), Some(note));
        let changes = [("state", scalar("dropped"))];
        self.written(following(parent, at, "drop", &changes, Some(&body)))
    }

    pub(crate) fn removed(&self, head: &Version, note: &str, at: At) -> Version {
        let body = note_body(note).unwrap();
        self.written(following(head, at, TOMBSTONE, &[], Some(&body)))
    }

    pub(crate) fn removed_with_no_note(&self, head: &Version, at: At) -> Version {
        self.written(following(head, at, TOMBSTONE, &[], Some("")))
    }

    pub(crate) fn rename_cut_short(&self, head: &Version, to: &str, at: At) -> Version {
        let mut block = block(at, RENAME, &[head]);
        block.superseded_by = Some(Slug::of_kind(head.slug.kind(), to).unwrap());
        let stone = Version::compose(head.slug.clone(), block, head.fields.clone(), String::new());
        self.written(accepted(stone))
    }

    pub(crate) fn renamed(&self, head: &Version, to: &str, at: At) -> (Version, Version) {
        let stone = self.rename_cut_short(head, to, at);
        let moved = Version::moved_by(&stone, head).unwrap();
        (stone, self.written(accepted(moved)))
    }

    pub(crate) fn renamed_with(
        &self,
        head: &Version,
        to: &str,
        at: At,
        change: impl FnOnce(&mut VersionBlock),
    ) -> (Version, Version) {
        let stone = self.rename_cut_short(head, to, at);
        let moved = Version::moved_by(&stone, head).unwrap();
        let mut block = moved.block;
        change(&mut block);
        let moved = Version::compose(moved.slug, block, moved.fields, moved.body);
        (stone, self.written(accepted(moved)))
    }

    pub(crate) fn renamed_apart(
        &self,
        head: &Version,
        to: &str,
        at: At,
        moved_at: At,
    ) -> (Version, Version) {
        self.renamed_with(head, to, at, |block| {
            moved_at.written.clone_into(&mut block.written);
            block.machine = MachineName::parse(moved_at.machine).unwrap();
        })
    }

    pub(crate) fn forked(&self, parent: &Version, one: At, other: At) -> (Version, Version) {
        (
            self.saved(parent, "As one machine has it", "\nOne.\n", one),
            self.saved(parent, "As the other has it", "\nOther.\n", other),
        )
    }

    pub(crate) fn withhold(&self, version: &Version) {
        let file = self.file_of(version);
        std::fs::remove_file(&file).unwrap();
        let directory = file.parent().unwrap();
        if std::fs::read_dir(directory).unwrap().next().is_none() {
            std::fs::remove_dir(directory).unwrap();
        }
    }

    pub(crate) fn damage(&self, version: &Version, bytes: &[u8]) {
        std::fs::write(self.file_of(version), bytes).unwrap();
    }

    pub(crate) fn planted(&self, directory: &str, text: &str) -> String {
        let path = format!("{directory}/{}.md", VersionId::of(text));
        self.stray_file(&path, text);
        path
    }

    pub(crate) fn stray_file(&self, path: &str, text: &str) {
        let path = self.root().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    pub(crate) fn link_out(&self, name: &str) {
        let outside = self.beside(&format!("linked-{name}"));
        std::fs::rename(self.root().join(name), &outside).unwrap();
        std::os::unix::fs::symlink(&outside, self.root().join(name)).unwrap();
    }

    pub(crate) fn stray_directory(&self, path: &str) {
        std::fs::create_dir_all(self.root().join(path)).unwrap();
    }
}

pub(crate) const DAY_1: &str = "2026-09-04T10:15:07.412093+01:00";
pub(crate) const DAY_2: &str = "2026-09-05T08:00:00+00:00";
pub(crate) const DAY_3: &str = "2026-09-06T23:30:00-07:00";
pub(crate) const DAY_4: &str = "2026-09-08T09:00:00.5+01:00";
pub(crate) const DAY_5: &str = "2026-09-10T09:00:00Z";
pub(crate) const DAY_6: &str = "2026-09-11T12:00:00+02:00";

pub(crate) const PIN: &str = "lantern/relay-pin";
pub(crate) const SCALE: &str = "atlas/scale";
pub(crate) const BENCH: &str = "2026-09/2026-09-06-bench";
pub(crate) const LAMP_DRIVER: &str = "2026-09/2026-09-04-lamp-driver";
pub(crate) const PORT: &str = "2026-09-01-port";

macro_rules! conversion {
    () => {
        "[conversion]\nwritten = 2026-10-20T09:30:00+01:00\nmachine = \"desk\"\n"
    };
}

pub(crate) const CONVERSION: &str = conversion!();
pub(crate) const ENDED_PHONE: &str = "[documents.\"topic/phone\"]\nended = \"retired\"\n";

pub(crate) const FILE: &str = concat!(
    conversion!(),
    "[topics.firmware]\nsummary = \"The firmware of [[lantern]]\"\npart_of = [\"lantern\"]\n\
     [topics.atlas]\nuses = [\"lantern\"]\n\
     [topics.relay]\npart_of = [\"lantern\"]\n\
     [tags]\nrelay = \"lantern\"\nmaps = \"atlas\"\nrust = \"\"\n\
     [documents.\"topic/phone\"]\nended = \"retired\"\n\
     [documents.\"fact/phone/battery\"]\ntopics = [\"atlas\"]\n\
     [documents.\"entry/2026-09/2026-09-05-wiring\"]\nended = \"merged\"\n\
     ended_by = \"entry/2026-09/2026-09-04-lamp\"\n\
     [documents.\"entry/2026-09/2026-09-06-bench-test\"]\nended = \"removed\"\n\
     [documents.\"followup/2026-09-03-solder\"]\nended = \"dropped\"\n\
     [documents.\"followup/2026-09-02-wire\"]\ntopics = [\"lantern\"]\n"
);

pub(crate) fn desk(written: &str) -> At<'_> {
    at(written, "desk")
}

pub(crate) fn with_desk() -> OldStore {
    let old = OldStore::new();
    old.machine_topic("desk", &[], &[], desk(DAY_1));
    old
}

pub(crate) fn with_topics() -> OldStore {
    let old = with_desk();
    old.topic("lantern", "A lamp", &[], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old
}

pub(crate) fn read(old: &OldStore) -> old::Store {
    old::read(old.root()).unwrap()
}

pub(crate) fn entry_key(slug: &str) -> DocumentKey {
    DocumentKey::new(old::Kind::Entry, slug)
}

pub(crate) fn fact_key(slug: &str) -> DocumentKey {
    DocumentKey::new(old::Kind::Fact, slug)
}

pub(crate) fn followup_key(slug: &str) -> DocumentKey {
    DocumentKey::new(old::Kind::Followup, slug)
}

pub(crate) fn planned_of<'p, 'a>(
    planned: &'p mut Planned<'a>,
    id: &DocumentId,
) -> &'p mut Document<'a> {
    (planned.documents.iter_mut())
        .find(|document| document.id == *id)
        .unwrap_or_else(|| panic!("no document {id} was planned"))
}

pub(crate) fn no_host(under: &Path) -> impl Fn(&str) -> Option<OsString> + use<> {
    let next = under.join("next").into_os_string();
    let home = under.join("home").into_os_string();
    move |name: &str| match name {
        "WORKLOG_HOME" => Some(next.clone()),
        "HOME" => Some(home.clone()),
        _ => None,
    }
}

fn topics(old: &OldStore) {
    let claimed = [("lantern", &["~/projects/lantern"][..])];
    let desk_topic = old.machine_topic("desk", &claimed, &["atlas"], desk(DAY_1));
    let fewer = [("unclaimed", list(&[]))];
    old.changed(&desk_topic, "unclaim", &fewer, desk(DAY_4));
    let lantern = old.topic("lantern", "A lamp", &[], desk(DAY_1));
    let brighter = old.saved(&lantern, "A brighter lamp", "\nBrighter.\n", desk(DAY_2));
    let warmer = old.saved(&brighter, "A warmer lamp", "\nWarmer.\n", desk(DAY_3));
    old.saved(&warmer, "A lamp on a relay", "\nOn a relay.\n", desk(DAY_4));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.topic("relay", "The relay of the lamp", &["lantern"], desk(DAY_1));
    let phone = old.topic("phone", "A phone", &[], desk(DAY_1));
    old.removed(&phone, "Sold", desk(DAY_3));
}

fn facts_of(old: &OldStore) {
    let pin = old.fact(PIN, "Pin 5", &[], desk(DAY_1));
    let body = "\nFound in [[2026-09/2026-09-04-lamp-driver]].\n";
    let saved = old.saved(&pin, "Pin 5 of [[lantern]]", body, desk(DAY_4));
    let again = [(
        "recheck",
        scalar("2026-11-01 See [[2026-09/2026-09-04-lamp-driver]]"),
    )];
    let rechecked = old.changed(&saved, "recheck", &again, desk(DAY_5));
    old.renamed(&rechecked, "lantern/relay-output", desk(DAY_6));
    old.fact("relay/coil", "Twelve volts", &[], desk(DAY_2));
    let scale = old.fact(SCALE, "One to a thousand", &[], desk(DAY_1));
    let (one, other) = old.forked(&scale, desk(DAY_2), desk(DAY_3));
    let fields = one.fields.clone();
    let joined = composed(
        SCALE,
        desk(DAY_5),
        "resolve",
        &[&one, &other],
        fields,
        &one.body,
    );
    old.write(&joined);
    let battery = old.fact("phone/battery", "Lasts a day", &[], desk(DAY_1));
    old.saved(&battery, "Lasts two days", "\nTwo.\n", desk(DAY_2));
}

fn entries_and_followups(old: &OldStore) {
    let tags = ["relay", "maps", "rust"];
    let first = old.entry(
        "2026-09/2026-09-04-lamp-driver",
        "Drove it",
        &tags,
        desk(DAY_1),
    );
    let body = "\nSee [[lantern/relay-pin]] and [[2026-09-01-port]].\n";
    let saved = old.saved(&first, "Drove the lamp", body, desk(DAY_2));
    let (_, lamp) = old.renamed(&saved, "2026-09/2026-09-04-lamp", desk(DAY_3));
    let wiring = old.entry("2026-09/2026-09-05-wiring", "Wired", &["maps"], desk(DAY_2));
    let folded = "Folded into [[2026-09/2026-09-04-lamp]]";
    old.removed(&wiring, folded, desk(DAY_4));
    let bench = old.entry(BENCH, "Benched", &["relay"], desk(DAY_3));
    let (_, tested) = old.renamed(&bench, "2026-09/2026-09-06-bench-test", desk(DAY_4));
    old.removed(&tested, "Never happened", desk(DAY_5));

    let port = old.followup("2026-09-01-port", &lamp, "Port it", &["relay"], desk(DAY_3));
    let dated = [("recheck", scalar("2026-10-01 The board ships"))];
    let dated = old.changed(&port, "recheck", &dated, desk(DAY_4));
    let note = "Ported, see [[2026-09/2026-09-04-lamp]]";
    old.done(&dated, note, desk(DAY_5));
    let wire = old.followup("2026-09-02-wire", &lamp, "Wire it", &[], desk(DAY_3));
    let touching = [("recheck", scalar("touching lantern"))];
    old.changed(&wire, "recheck", &touching, desk(DAY_4));
    let solder = old.followup("2026-09-03-solder", &lamp, "Solder", &["maps"], desk(DAY_3));
    let dropped = old.dropped(&solder, "Glued instead", desk(DAY_4));
    old.removed(&dropped, "Was never ours", desk(DAY_5));
}

pub(crate) fn rich() -> OldStore {
    let old = OldStore::new();
    topics(&old);
    facts_of(&old);
    entries_and_followups(&old);
    old
}

pub(crate) fn tree(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn walk(root: &Path, at: &Path, found: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            let kind = std::fs::symlink_metadata(&path).unwrap().file_type();
            if kind.is_dir() {
                found.insert(relative, None);
                walk(root, &path, found);
            } else if kind.is_symlink() {
                let to = std::fs::read_link(&path).unwrap();
                found.insert(relative, Some(to.into_os_string().into_encoded_bytes()));
            } else {
                found.insert(relative, Some(std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(root, root, &mut found);
    found
}

pub(crate) fn in_another_order(old: &OldStore) -> OldStore {
    let copy = OldStore::new();
    for (path, bytes) in tree(old.root()).into_iter().rev() {
        if let Some(bytes) = bytes {
            let to = copy.root().join(path);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::write(to, bytes).unwrap();
        }
    }
    copy
}
