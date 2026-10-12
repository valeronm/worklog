//! One line per command run, in the log each machine keeps in the store.

use super::id::DocumentId;
use super::version::Stamp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub agent: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub written: Stamp,
    pub machine: DocumentId,
    pub version: Option<String>,
    pub session: Option<Session>,
    /// The command path, `new entry` for a command taking a subcommand.
    pub command: String,
    pub exit: i32,
    pub refusal: Option<String>,
    /// How many documents a `search` matched.
    pub hits: Option<usize>,
    /// The working directory, spelled as a claim spells one.
    pub directory: String,
    pub arguments: Vec<String>,
}

impl Invocation {
    /// `YYYY-MM`.
    #[must_use]
    pub fn month(&self) -> String {
        self.written.day().chars().take(7).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    pub machine: Option<DocumentId>,
    /// `YYYY-MM-DD`, compared with the day a run's stamp is written with.
    pub since: Option<String>,
}

impl Kept {
    #[must_use]
    pub fn keeps(&self, run: &Invocation) -> bool {
        self.machine
            .as_ref()
            .is_none_or(|only| *only == run.machine)
            && (self.since.as_ref()).is_none_or(|day| run.written.day() >= *day)
    }

    /// Whether a run of `machine` written in `month`, `YYYY-MM`, can be one kept.
    #[must_use]
    pub fn reaches(&self, machine: &DocumentId, month: &str) -> bool {
        self.machine.as_ref().is_none_or(|only| only == machine)
            && (self.since.as_deref())
                .is_none_or(|day| day.get(..7).is_none_or(|from| month >= from))
    }
}

#[cfg(test)]
mod tests {
    use super::{DocumentId, Kept};
    use crate::domain::testing::{atlas, lantern, ran};

    #[test]
    fn the_month_a_line_was_written_in_names_its_file() {
        assert_eq!(ran(&lantern(), "show", "2026-09-04").month(), "2026-09");
        assert_eq!(ran(&lantern(), "show", "2027-01-31").month(), "2027-01");
    }

    fn kept(machine: Option<DocumentId>, since: Option<&str>) -> Kept {
        Kept {
            machine,
            since: since.map(str::to_owned),
        }
    }

    #[test]
    fn a_reading_keeps_the_runs_of_its_machine_from_its_day_on() {
        let run = ran(&lantern(), "show", "2026-09-04");
        assert!(Kept::default().keeps(&run));
        assert!(kept(Some(lantern()), Some("2026-09-04")).keeps(&run));
        assert!(!kept(Some(atlas()), None).keeps(&run));
        assert!(!kept(None, Some("2026-09-05")).keeps(&run));
        assert!(kept(None, Some("2026-08-31")).keeps(&run));
    }

    #[test]
    fn a_month_of_a_machine_is_reached_unless_it_ends_before_the_day_or_is_another_s() {
        assert!(Kept::default().reaches(&lantern(), "2026-09"));
        assert!(kept(Some(lantern()), None).reaches(&lantern(), "2026-09"));
        assert!(!kept(Some(atlas()), None).reaches(&lantern(), "2026-09"));
        let from = kept(None, Some("2026-09-15"));
        assert!(from.reaches(&lantern(), "2026-09"));
        assert!(from.reaches(&lantern(), "2027-01"));
        assert!(!from.reaches(&lantern(), "2026-08"));
        assert!(!kept(Some(atlas()), Some("2026-09-15")).reaches(&lantern(), "2026-10"));
    }
}
