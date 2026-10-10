use std::collections::BTreeSet;

use toml::Value;

use crate::domain::document::{Document, State};
use crate::domain::id::DocumentId;
use crate::domain::schema::graph::Edges;
use crate::domain::schema::{Content, KindOf, Name, Record, SchemaError};
use crate::domain::version::{Fields, Version};

pub(super) fn read_or_skip(head: &Version) -> Option<Record> {
    Record::read(&head.envelope.kind, &head.fields).ok()
}

pub(super) fn is_forked(document: &Document) -> bool {
    matches!(document.state(), State::Forked(_))
}

pub(super) fn unread_heads(document: &Document) -> Vec<(&Version, SchemaError)> {
    document
        .heads()
        .into_iter()
        .filter_map(|head| {
            Record::read(&head.envelope.kind, &head.fields)
                .err()
                .map(|error| (head, error))
        })
        .collect()
}

pub(super) type Read<'d> = (&'d Version, Record);

pub(super) fn readable_heads(document: &Document) -> Vec<Read<'_>> {
    document
        .heads()
        .into_iter()
        .filter_map(|head| read_or_skip(head).map(|record| (head, record)))
        .collect()
}

pub(super) fn edges_of(heads: &[Read]) -> Edges {
    let mut edges = Edges::default();
    for (_, record) in heads {
        if let Content::Topic(topic) = &record.content
            && record.ending.is_none()
        {
            for (into, ids) in [
                (&mut edges.part_of, &topic.part_of),
                (&mut edges.uses, &topic.uses),
            ] {
                for id in ids {
                    if !into.contains(id) {
                        into.push(id.clone());
                    }
                }
            }
        }
    }
    edges
}

pub(super) fn topic_edges(document: &Document) -> Edges {
    edges_of(&readable_heads(document))
}

pub(super) fn kind_of(document: &Document) -> Option<KindOf> {
    document.kind().and_then(|kind| KindOf::of(kind).ok())
}

pub(super) fn unended_heads(document: &Document) -> Vec<Read<'_>> {
    let mut heads = readable_heads(document);
    heads.retain(|(_, record)| record.ending.is_none());
    heads
}

pub(super) fn heads_not_ended(document: &Document) -> Vec<&Version> {
    let mut heads = document.heads();
    heads.retain(|head| !version_is_ended(head));
    heads
}

pub(super) fn all_ended(heads: &[Read]) -> bool {
    heads.iter().all(|(_, record)| record.ending.is_some())
}

// A version that does not read as a record counts as not ended.
pub(super) fn version_is_ended(version: &Version) -> bool {
    read_or_skip(version).is_some_and(|record| record.ending.is_some())
}

pub(super) fn ended_for(record: &Record) -> Option<String> {
    let ending = record.ending.as_ref()?;
    Some(ending.reason.word().to_owned())
}

fn row_head_at(heads: &[Read]) -> Option<usize> {
    let ended = all_ended(heads);
    (0..heads.len())
        .filter(|at| ended || heads[*at].1.ending.is_none())
        .max_by_key(|at| {
            let (head, _) = &heads[*at];
            (head.envelope.written.instant(), &head.id)
        })
}

pub(super) fn row_head_of<'h, 'd>(heads: &'h [Read<'d>]) -> Option<&'h Read<'d>> {
    row_head_at(heads).map(|at| &heads[at])
}

pub(super) fn row_head(document: &Document) -> Option<Read<'_>> {
    let mut heads = readable_heads(document);
    let at = row_head_at(&heads)?;
    Some(heads.swap_remove(at))
}

pub(super) fn label_or_short(label: Option<String>, id: &DocumentId) -> String {
    label.unwrap_or_else(|| id.short().to_owned())
}

pub(super) fn machine_label(version: &Version, topic_name: Option<&Name>) -> String {
    label_or_short(
        topic_name.map(ToString::to_string),
        &version.envelope.machine,
    )
}

pub(super) fn held_under<'d>(
    document: &'d Document,
    kind: KindOf,
    key: &str,
    ended: bool,
) -> Vec<&'d str> {
    let heads = readable_heads(document);
    let holding: Vec<&Read<'d>> = if !all_ended(&heads) {
        (heads.iter())
            .filter(|(_, record)| record.ending.is_none())
            .collect()
    } else if ended {
        row_head_of(&heads).into_iter().collect()
    } else {
        Vec::new()
    };
    (holding.into_iter())
        .filter(|(_, record)| record.content.kind() == kind)
        .flat_map(|(head, _)| texts(&head.fields, key))
        .collect()
}

pub(super) fn is_holder(
    document: &Document,
    kind: KindOf,
    key: &str,
    values: &BTreeSet<&str>,
    ended: bool,
) -> bool {
    held_under(document, kind, key, ended)
        .iter()
        .any(|text| values.contains(text))
}

pub(super) fn texts<'f>(fields: &'f Fields, key: &str) -> Vec<&'f str> {
    match fields.get(key) {
        Some(Value::String(text)) => vec![text],
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}
