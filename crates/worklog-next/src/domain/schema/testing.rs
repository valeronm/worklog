use super::kind::Record;
use crate::domain::version::{Fields, Kind};

pub(super) fn fields(toml: &str) -> Fields {
    toml.parse().expect("fields in TOML")
}

pub(super) fn record(word: &str, toml: &str) -> Record {
    Record::read(&Kind::parse(word).expect("a kind"), &fields(toml)).expect("a record")
}
