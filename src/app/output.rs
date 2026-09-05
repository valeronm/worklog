//! What use cases return. Each is rendered as text or JSON by the CLI and
//! carries only data.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub slug: String,
    pub kind: String,
    /// An entry's date, a followup's date, or the day a fact or topic was written.
    pub date: String,
    pub summary: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Listing {
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FactListing {
    pub facts: Vec<Row>,
    pub ideas: Vec<Row>,
}

/// Who wrote a version, when, and as what; the same four fields wherever
/// a version is listed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stamp {
    pub id: String,
    pub written: String,
    pub machine: String,
    pub operation: String,
}

impl Stamp {
    /// Enough of the id to tell versions of one document apart.
    #[must_use]
    pub fn short(&self) -> &str {
        short(&self.id)
    }

    /// The stamp to the minute, in the offset it was written with.
    #[must_use]
    pub fn written_to_minute(&self) -> String {
        chrono::DateTime::parse_from_rfc3339(&self.written).map_or_else(
            |_| self.written.clone(),
            |t| t.format("%Y-%m-%d %H:%M").to_string(),
        )
    }

    /// A stamp carries microseconds so two versions of one command can be
    /// ordered, which is finer than a line needs to show.
    #[must_use]
    pub fn written_to_millis(&self) -> String {
        let stamp = &self.written;
        let Some(dot) = stamp.find('.') else {
            return stamp.to_owned();
        };
        let first_digit = dot + 1;
        let digits = stamp[first_digit..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        format!(
            "{}{}",
            &stamp[..first_digit + digits.min(3)],
            &stamp[first_digit + digits..]
        )
    }
}

/// Enough of a version id to tell versions of one document apart.
#[must_use]
pub fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Head {
    #[serde(flatten)]
    pub stamp: Stamp,
    pub parents: Vec<String>,
    pub text: String,
}

/// A document as it stands: one head, or every head of a fork.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Shown {
    pub slug: String,
    pub kind: String,
    pub forked: bool,
    pub heads: Vec<Head>,
    /// For an entry, the followups naming it.
    pub followups: Vec<FollowupItem>,
    /// A note that a head was written by a newer worklog.
    pub foreign: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryRow {
    #[serde(flatten)]
    pub stamp: Stamp,
    /// The slug this version was written under.
    pub slug: String,
    pub parents: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct History {
    pub slug: String,
    pub versions: Vec<HistoryRow>,
    /// A note that a version was written by a newer worklog.
    pub foreign: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LogRow {
    #[serde(flatten)]
    pub stamp: Stamp,
    pub slug: String,
}

/// Versions across the whole store, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Log {
    pub versions: Vec<LogRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hit {
    pub row: Row,
    pub lines: Vec<(usize, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Search {
    pub term: String,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Tags {
    /// Most used first.
    pub tags: Vec<Count>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Count {
    pub name: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FollowupItem {
    pub slug: String,
    /// `followup`, `fact` or `idea`.
    pub source: String,
    pub entry: Option<String>,
    /// A followup's state; a fact or idea with a recheck has none.
    pub state: Option<String>,
    pub summary: String,
    pub recheck: Option<String>,
    /// `due <date>`, `by <date>`, `touching <topic>`, or `no recheck`.
    pub label: String,
    pub due: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Followups {
    pub items: Vec<FollowupItem>,
    pub open: usize,
    pub entries: usize,
    pub due: usize,
    pub without_recheck: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TopicRow {
    pub slug: String,
    pub summary: String,
    pub machine: Option<String>,
    pub includes: Vec<String>,
    pub facts: usize,
    pub ideas: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Topics {
    pub topics: Vec<TopicRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Where {
    pub machine: String,
    pub claims: Vec<Claimed>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Claimed {
    pub topic: String,
    pub dir: String,
    /// Whether the directory is on this host; unknown for another
    /// machine's layout.
    pub exists: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Group {
    pub topic: String,
    pub summary: String,
    pub distance: usize,
    pub via: String,
    pub facts: Vec<String>,
    pub ideas: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Context {
    pub machine: Option<String>,
    pub directory: String,
    pub groups: Vec<Group>,
    /// Open followups tagged with a directly claimed topic.
    pub open: usize,
    pub open_entries: usize,
    pub without_recheck: usize,
    pub due: Vec<FollowupItem>,
    pub forks: Vec<String>,
    pub drafts: Vec<String>,
    /// By name, since the count is what a topic holds and not its rank.
    pub unreached: Vec<Count>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fork {
    pub slug: String,
    pub heads: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Forks {
    pub forks: Vec<Fork>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub slug: String,
    pub message: String,
}

impl Problem {
    #[must_use]
    pub fn at(slug: &crate::domain::slug::Slug, message: String) -> Problem {
        Problem {
            slug: slug.path().to_owned(),
            message,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Check {
    pub problems: Vec<Problem>,
    /// Worth a look and no problem: the exit code ignores them.
    pub notices: Vec<Problem>,
    pub forks: Vec<String>,
    pub documents: usize,
    pub links: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub machines: Vec<MachineUsage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MachineUsage {
    pub machine: String,
    /// Most used first.
    pub commands: Vec<Count>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Written {
    pub slug: String,
    pub id: String,
    /// The tombstone a rename leaves behind.
    pub tombstone: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftRef {
    pub slug: String,
    pub kind: String,
    pub location: String,
    pub parents: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DraftList {
    pub drafts: Vec<DraftRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
/// The two texts a diff is made of; the diff itself is a rendering.
pub struct Diff {
    pub slug: String,
    pub before: Side,
    pub after: Side,
    /// The move, when the version was written by a rename.
    pub renamed: Option<Renamed>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Renamed {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Side {
    pub name: String,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::Stamp;

    fn stamped(written: &str) -> Stamp {
        Stamp {
            id: "a".repeat(64),
            written: written.to_owned(),
            machine: "desk".to_owned(),
            operation: "new".to_owned(),
        }
    }

    #[test]
    fn a_stamp_shows_three_fraction_digits_at_most() {
        assert_eq!(
            stamped("2026-09-04T10:00:00.123456+01:00").written_to_millis(),
            "2026-09-04T10:00:00.123+01:00"
        );
        assert_eq!(
            stamped("2026-09-04T10:00:00.12+01:00").written_to_millis(),
            "2026-09-04T10:00:00.12+01:00"
        );
        assert_eq!(
            stamped("2026-09-04T10:00:00+01:00").written_to_millis(),
            "2026-09-04T10:00:00+01:00"
        );
        assert_eq!(stamped("").short(), "aaaaaaaaaaaa");
    }
}
