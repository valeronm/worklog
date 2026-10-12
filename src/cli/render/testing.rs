use super::CONTEXT_BYTES;
pub(super) use crate::app::testing::{NOW as WRITTEN, row};
use crate::app::{FollowupRow, Reference, TriggerShown};
use crate::domain::schema::Date;
use crate::domain::testing::{atlas, lantern};
use crate::domain::version::Stamp;

pub(super) fn stamp(text: &str) -> Stamp {
    Stamp::parse(text).expect("a stamp")
}

pub(super) fn to(label: &str) -> Reference {
    crate::app::testing::to(&atlas(), label)
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
        entry: Some(to("2026-10-08-wiring")),
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
