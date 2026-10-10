use crate::app::Row;
use crate::domain::id::DocumentId;
use crate::domain::schema::KindOf;
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
        summary: summary.to_owned(),
        topics: Vec::new(),
        forked: false,
        ended: None,
    }
}
