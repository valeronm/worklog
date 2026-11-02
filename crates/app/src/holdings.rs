//! What the store holds and what was written to it, over the whole store or one topic.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use chrono::Datelike as _;
use serde::Serialize;

use crate::heads::{ended_for, is_forked, row_head};
use crate::index::Topics;
use crate::lookup::Lookup;
use crate::rows::memberships;
use crate::{Failure, Reference};
use worklog_domain::document::Document;
use worklog_domain::id::DocumentId;
use worklog_schema::{Category, Date, KindOf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Holdings {
    /// Every kind, one holding nothing included.
    pub kinds: Vec<KindCount>,
    pub version_count: usize,
    /// The lengths of the versions' texts, added.
    pub size: usize,
    pub forked_count: usize,
    /// `None` under a topic.
    pub unfiled: Option<Unfiled>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct KindCount {
    pub kind: Category,
    pub live: usize,
    pub ended: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unfiled {
    pub usage_size: usize,
    /// Version files that do not read.
    pub unreadable_count: usize,
    /// Documents of a kind this binary does not know.
    pub unknown_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Writes {
    /// Every month from the first to the last holding a version written in the span, oldest
    /// first, a month between them with no version included; empty when none was written.
    pub months: Vec<MonthWrites>,
    /// The most versions first, then by label.
    pub machines: Vec<MachineWrites>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MonthWrites {
    /// `YYYY-MM`.
    pub month: String,
    pub created_count: usize,
    pub version_count: usize,
    pub size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MachineWrites {
    pub machine: Reference,
    pub version_count: usize,
    pub size: usize,
}

impl Holdings {
    #[must_use]
    pub fn document_count(&self) -> usize {
        self.kinds
            .iter()
            .map(|count| count.live + count.ended)
            .sum()
    }
}

pub(crate) struct Counted {
    category: Category,
    ended: bool,
    document: Rc<Document>,
}

/// Every document of a known kind whose head reads, under `topic` and its parts when one is
/// given.
pub(crate) fn under(
    lookup: &Lookup,
    topics: &Topics,
    topic: Option<&DocumentId>,
) -> Result<Vec<Counted>, Failure> {
    let covered = topic.map(|topic| topics.covered(topic));
    let mut found = Vec::new();
    for kind in KindOf::ALL {
        for document in lookup.of_kind(kind)? {
            let Some((_, record)) = row_head(&document) else {
                continue;
            };
            if let Some(covered) = &covered {
                let filed = if kind == KindOf::Topic {
                    covered.contains(document.id())
                } else {
                    (memberships(&document, kind, true).iter()).any(|filed| covered.contains(filed))
                };
                if !filed {
                    continue;
                }
            }
            found.push(Counted {
                category: Category::of(&record.content),
                ended: ended_for(&record).is_some(),
                document,
            });
        }
    }
    Ok(found)
}

pub(crate) fn unfiled(lookup: &Lookup, usage_size: usize) -> Result<Unfiled, Failure> {
    let unreadable_count = (lookup.holding_unreadable()?.iter())
        .map(|document| document.unreadable().len())
        .sum();
    let unknown_count = (lookup.unknown()?.iter())
        .map(|unknown| unknown.documents.len())
        .sum();
    Ok(Unfiled {
        usage_size,
        unreadable_count,
        unknown_count,
    })
}

pub(crate) fn holdings(counted: &[Counted], unfiled: Option<Unfiled>) -> Holdings {
    let mut kinds: Vec<KindCount> = (Category::all().into_iter())
        .map(|kind| KindCount {
            kind,
            live: 0,
            ended: 0,
        })
        .collect();
    let (mut version_count, mut size, mut forked_count) = (0, 0, 0);
    for one in counted {
        if let Some(count) = kinds.iter_mut().find(|count| count.kind == one.category) {
            if one.ended {
                count.ended += 1;
            } else {
                count.live += 1;
            }
        }
        let headers = one.document.headers();
        version_count += headers.len();
        size += headers.iter().map(|header| header.length).sum::<usize>();
        forked_count += usize::from(is_forked(&one.document));
    }
    Holdings {
        kinds,
        version_count,
        size,
        forked_count,
        unfiled,
    }
}

type Month = (i32, u32);

fn month_writes((year, month): Month) -> MonthWrites {
    MonthWrites {
        month: format!("{year:04}-{month:02}"),
        created_count: 0,
        version_count: 0,
        size: 0,
    }
}

fn months_between(first: Month, last: Month) -> Vec<Month> {
    let (mut year, mut month) = first;
    let mut months = Vec::new();
    while (year, month) <= last {
        months.push((year, month));
        (year, month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
    }
    months
}

pub(crate) fn writes(counted: &[Counted], topics: &Topics, from: Date, to: Date) -> Writes {
    let mut months: BTreeMap<Month, MonthWrites> = BTreeMap::new();
    let mut machines: BTreeMap<&DocumentId, MachineWrites> = BTreeMap::new();
    for header in counted.iter().flat_map(|one| one.document.headers()) {
        let Some(day) = Date::parse(&header.envelope.written.day())
            .ok()
            .filter(|day| (from..=to).contains(day))
        else {
            continue;
        };
        let day = day.on_calendar();
        let key = (day.year(), day.month());
        let month = months.entry(key).or_insert_with(|| month_writes(key));
        month.version_count += 1;
        month.size += header.length;
        month.created_count += usize::from(header.envelope.parents.is_empty());
        let id = &header.envelope.machine;
        let machine = machines.entry(id).or_insert_with(|| MachineWrites {
            machine: topics.machine(id),
            version_count: 0,
            size: 0,
        });
        machine.version_count += 1;
        machine.size += header.length;
    }
    let months = match (months.first_key_value(), months.last_key_value()) {
        (Some((first, _)), Some((last, _))) => months_between(*first, *last),
        _ => Vec::new(),
    }
    .into_iter()
    .map(|key| months.remove(&key).unwrap_or_else(|| month_writes(key)))
    .collect();
    let mut machines: Vec<MachineWrites> = machines.into_values().collect();
    machines.sort_by(|a, b| {
        (b.version_count, &a.machine.label).cmp(&(a.version_count, &b.machine.label))
    });
    Writes { months, machines }
}

#[cfg(test)]
mod tests {
    use alloc::borrow::ToOwned;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    use super::*;
    use crate::activity::stats;
    use crate::testing::{
        ENDED_FACT, claim_fields, entry, fact, fact_fields, followup, fork, head, part_of, stored,
        topic, world, written_at,
    };
    use worklog_domain::id::{DocumentId, VersionId};
    use worklog_domain::ports::StoreRead as _;
    use worklog_domain::version::ReadError;

    fn held(world: &crate::testing::World, topic: Option<&str>) -> Holdings {
        stats(&world.deps().read(), topic, None).unwrap().holdings
    }

    fn kinds(holdings: &Holdings) -> Vec<(&'static str, usize, usize)> {
        (holdings.kinds.iter())
            .map(|count| (count.kind.word(), count.live, count.ended))
            .collect()
    }

    #[test]
    fn documents_are_counted_by_kind_live_or_ended_under_a_topic_and_its_parts() {
        let (world, lantern) = world();
        let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
        let phone = topic(&world, "phone", "");
        fact(&world, &lantern, "relay", "");
        fact(&world, &atlas, "dimmer", "idea = true\n");
        let fuse = fact(&world, &atlas, "fuse", "");
        world.amend(
            &fuse,
            &fact_fields(&atlas, "fuse", "s", ENDED_FACT),
            "The relay.\n",
        );
        entry(&world, "2026-10-08", "wiring", &[&atlas]);
        followup(&world, &[&phone, &lantern], "Order a lens", "");
        fact(&world, &phone, "case", "");
        let claim = claim_fields(&world.machine(), &lantern, "~/projects/lantern");
        world.put("claim", &claim, "\n");

        assert_eq!(
            kinds(&held(&world, Some("lantern"))),
            [
                ("topic", 2, 0),
                ("fact", 1, 1),
                ("idea", 1, 0),
                ("entry", 1, 0),
                ("followup", 1, 0),
                ("claim", 1, 0),
            ]
        );
        assert_eq!(
            kinds(&held(&world, None)),
            [
                ("topic", 4, 0),
                ("fact", 2, 1),
                ("idea", 1, 0),
                ("entry", 1, 0),
                ("followup", 1, 0),
                ("claim", 1, 0),
            ]
        );
    }

    #[test]
    fn versions_their_size_and_the_forked_documents_are_counted_with_the_documents() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        world.amend(
            &relay,
            &fact_fields(&lantern, "relay", "s", ""),
            "The relay, wired.\n",
        );
        let fuse = fact(&world, &lantern, "fuse", "");
        fork(&world, &fuse, &fact_fields(&lantern, "fuse", "s", ""));
        let texts: usize = [&lantern, &relay, &fuse]
            .into_iter()
            .flat_map(|id| {
                let held = world.store.headers(id).unwrap();
                (held.headers().iter())
                    .map(|header| stored(&world, id, &header.id).text().len())
                    .collect::<Vec<_>>()
            })
            .sum();
        let counted = held(&world, Some("lantern"));
        assert_eq!(
            (
                counted.document_count(),
                counted.version_count,
                counted.size,
                counted.forked_count
            ),
            (3, 6, texts, 1)
        );
    }

    #[test]
    fn what_is_under_no_topic_is_counted_over_the_whole_store_alone() {
        let (world, lantern) = world();
        let relay = fact(&world, &lantern, "relay", "");
        world
            .store
            .plant_unreadable(&relay, VersionId::of(b"damaged"), ReadError::Corrupt);
        world.put("lamp", "x = 1\n", "\n");
        world.put("lamp", "x = 2\n", "\n");
        world.usage.size.set(4200);
        assert_eq!(
            held(&world, None).unfiled,
            Some(Unfiled {
                usage_size: 4200,
                unreadable_count: 1,
                unknown_count: 2,
            })
        );
        assert_eq!(held(&world, Some("lantern")).unfiled, None);
    }

    #[test]
    fn versions_written_in_the_span_are_counted_by_month_and_machine() {
        let (world, lantern) = world();
        let phone = topic(&world, "phone", "");
        let stray = DocumentId::parse("0123456789abcdef0123456789abcdef").unwrap();
        let relay = fact(&world, &lantern, "relay", "");
        let fields = fact_fields(&lantern, "relay", "s", "");
        let root = head(&world, &relay);
        let before = "2025-10-05T23:30:00+00:00";
        let early = written_at(&world, &root, &fields, before, "Before.\n", None);
        let first_day = "2025-10-06T00:30:00+02:00";
        let late = written_at(&world, &early, &fields, first_day, "In.\n", Some(&phone));
        let winter = "2026-01-15T10:00:00+00:00";
        let away = written_at(&world, &late, &fields, winter, "Away.\n", Some(&stray));
        let roots: usize = [&world.machine(), &lantern, &phone, &relay]
            .map(|id| head_of_root(&world, id))
            .iter()
            .sum();

        let writes = stats(&world.deps().read(), None, None).unwrap().writes;
        let months: Vec<(String, usize, usize, usize)> = (writes.months.iter())
            .map(|m| (m.month.clone(), m.created_count, m.version_count, m.size))
            .collect();
        assert_eq!(months.len(), 13);
        assert_eq!(months[0], ("2025-10".to_owned(), 0, 1, late.text().len()));
        assert_eq!(months[3], ("2026-01".to_owned(), 0, 1, away.text().len()));
        assert_eq!(months[12], ("2026-10".to_owned(), 4, 4, roots));
        assert_eq!(months.iter().filter(|m| m.2 == 0).count(), 10);
        let machines: Vec<(String, usize)> = (writes.machines.iter())
            .map(|m| (m.machine.label.clone(), m.version_count))
            .collect();
        assert_eq!(
            machines,
            [
                ("desk".to_owned(), 4),
                (stray.to_string(), 1),
                ("phone".to_owned(), 1),
            ]
        );

        let under = stats(&world.deps().read(), Some("lantern"), Some(2025))
            .unwrap()
            .writes;
        assert_eq!(under.months.len(), 1);
        let october = &under.months[0];
        assert_eq!(
            (
                october.month.as_str(),
                october.created_count,
                october.version_count
            ),
            ("2025-10", 0, 2)
        );
        let none = stats(&world.deps().read(), None, Some(2024))
            .unwrap()
            .writes;
        assert!(none.months.is_empty());
    }

    fn head_of_root(world: &crate::testing::World, id: &DocumentId) -> usize {
        let held = world.store.headers(id).unwrap();
        let root = (held.headers().iter())
            .find(|header| header.envelope.parents.is_empty())
            .unwrap();
        stored(world, id, &root.id).text().len()
    }
}
