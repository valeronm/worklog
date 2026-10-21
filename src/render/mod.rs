//! What the command line and the pages show the same way.

pub mod calendar;
pub mod diff;

use crate::app::check::Check;
use crate::app::search::Matched;
use crate::app::show::{Side, Unread};
use crate::app::{FollowupRow, Reference, TriggerShown, counted};
use crate::domain::id::VersionId;
use crate::domain::version::Stamp;

const HIT_LINES: usize = 3;

#[must_use]
pub fn ended(reason: &str) -> String {
    format!("ended {reason}")
}

#[must_use]
pub fn change_or_ending(change: &str, reason: Option<&str>) -> String {
    reason.map_or_else(|| change.to_owned(), ended)
}

pub struct TriggerState<'f> {
    pub words: String,
    /// The topic a followup waits on a touch of, which `words` leaves out.
    pub touching: Option<&'f Reference>,
}

#[must_use]
pub fn trigger_state(followup: &FollowupRow) -> TriggerState<'_> {
    let (words, touching) = match &followup.trigger {
        Some(TriggerShown::LookAgain { on, .. }) if followup.due => (format!("due {on}"), None),
        Some(TriggerShown::LookAgain { on, .. }) => (format!("by {on}"), None),
        Some(TriggerShown::Touching { topic }) => ("touching".to_owned(), Some(topic)),
        None => ("no trigger".to_owned(), None),
    };
    TriggerState { words, touching }
}

/// What is filed on a topic as one text for each count, a count of zero left out.
#[must_use]
pub fn filed_counts(fact_count: usize, idea_count: usize, open_count: usize) -> Vec<String> {
    [
        (fact_count, "fact", "facts"),
        (idea_count, "idea", "ideas"),
        (open_count, "open", "open"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, one, several)| counted(count, one, several))
    .collect()
}

/// The first of a hit's matching lines, and a line counting the rest when there are more.
#[must_use]
pub fn hit_lines(lines: &[Matched]) -> (&[Matched], Option<String>) {
    let shown = &lines[..lines.len().min(HIT_LINES)];
    let more = (lines.len() > HIT_LINES).then(|| format!("… {} more", lines.len() - HIT_LINES));
    (shown, more)
}

#[must_use]
pub fn unread_notes(files: &[Unread]) -> Vec<String> {
    files.iter().map(Unread::note).collect()
}

pub struct Against<'s> {
    pub words: String,
    /// The version compared against when it is held.
    pub held: Option<&'s VersionId>,
}

#[must_use]
pub fn against(side: &Side) -> Against<'_> {
    match &side.against {
        None => Against {
            words: "nothing".to_owned(),
            held: None,
        },
        Some(version) if side.held => Against {
            words: version.short().to_owned(),
            held: Some(version),
        },
        Some(version) => Against {
            words: format!("{} (not held)", version.short()),
            held: None,
        },
    }
}

pub struct CheckSummary {
    pub problems: String,
    pub notices: String,
    pub forks: String,
}

#[must_use]
pub fn check_summary(check: &Check) -> CheckSummary {
    CheckSummary {
        problems: counted(check.problems.len(), "problem", "problems"),
        notices: counted(check.notices.len(), "notice", "notices"),
        forks: counted(check.fork_count, "fork", "forks"),
    }
}

/// A time cut to the millisecond, its offset kept.
#[must_use]
pub fn written(stamp: &Stamp) -> String {
    let stamp = stamp.to_string();
    let Some(dot) = stamp.find('.') else {
        return stamp;
    };
    let fraction = dot + 1;
    let digits = stamp[fraction..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    let shown = fraction + digits.min(3);
    format!("{}{}", &stamp[..shown], &stamp[fraction + digits..])
}
