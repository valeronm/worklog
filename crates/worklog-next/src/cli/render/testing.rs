use super::CONTEXT_BYTES;
use crate::app::{FollowupRow, Row, TriggerShown};
use crate::domain::id::DocumentId;
use crate::domain::schema::{Date, KindOf};
use crate::domain::testing::lantern;
use crate::domain::version::Stamp;

pub(super) const WRITTEN: &str = "2026-10-09T18:22:41.118204+01:00";

pub(super) fn stamp(text: &str) -> Stamp {
    Stamp::parse(text).expect("a stamp")
}

pub(super) fn row(id: &DocumentId, label: &str, summary: &str) -> Row {
    Row {
        document: id.clone(),
        kind: KindOf::Topic,
        label: label.to_owned(),
        name: None,
        summary: summary.to_owned(),
        topics: Vec::new(),
        forked: false,
        ended: None,
    }
}

pub(super) fn look_again(day: &str) -> TriggerShown {
    TriggerShown::LookAgain {
        on: Date::parse(day).expect("a date"),
        why: "The part arrives".to_owned(),
    }
}

pub(super) fn followup(summary: &str, trigger: Option<TriggerShown>, due: bool) -> FollowupRow {
    FollowupRow {
        row: row(&lantern(), "7f3a91c0", summary),
        trigger,
        due,
        entry: Some("2026-10-08-wiring".to_owned()),
        about: None,
    }
}

#[track_caller]
pub(in crate::cli) fn assert_fits(text: &str) {
    assert_case_fits("the text", text);
}

#[track_caller]
pub(super) fn assert_case_fits(case: &str, text: &str) {
    assert!(text.len() <= CONTEXT_BYTES, "{case}: {} bytes", text.len());
}
