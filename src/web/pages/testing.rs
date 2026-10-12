use crate::app::Reference;
use crate::app::show::{FieldItem, ShownField, ShownVersion};
use crate::app::testing::NOW;
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::version::Stamp;

pub fn to(byte: u8, label: &str) -> Reference {
    crate::app::testing::to(&DocumentId::from_bytes([byte; 16]), label)
}

pub fn version(seed: &[u8]) -> ShownVersion {
    ShownVersion {
        version: VersionId::of(seed),
        written: Stamp::parse(NOW).expect("a stamp"),
        machine: to(9, "desk"),
        change: "save".to_owned(),
        parents: Vec::new(),
        head: false,
        ended: None,
        text: "a\nb".to_owned(),
        fields: vec![
            ShownField {
                name: "name".to_owned(),
                values: vec![FieldItem::Text {
                    text: "lantern".to_owned(),
                }],
            },
            ShownField {
                name: "part_of".to_owned(),
                values: [to(4, "atlas"), to(5, "phone")]
                    .map(|reference| FieldItem::Reference { reference })
                    .into(),
            },
        ],
        body: "Lit.\n".to_owned(),
        links: Vec::new(),
    }
}
