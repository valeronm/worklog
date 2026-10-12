//! What a shell offers for an argument that takes a document.

use std::cmp::Reverse;
use std::rc::Rc;

use crate::app::heads::kind_of;
use crate::app::index::Topics;
use crate::app::lookup::{Lookup, Names};
use crate::app::rows::{Shown, row, urgency};
use crate::app::{Deps, Failure};
use crate::domain::document::Document;
use crate::domain::id::DocumentId;
use crate::domain::schema::{Content, Date, KindOf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wanted {
    Live,
    Ended,
    /// Ended or not, and with a head that reads or with none.
    Forked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub document: DocumentId,
    /// The label, or the short id where the label finds another document, several or none,
    /// or the whole id where the short id does too.
    pub address: String,
    /// Empty for a claim and for a document no head of which reads.
    pub summary: String,
}

/// The documents of `kinds` that are as `wanted`, each with an address its command resolves
/// to it and to no other document. The live ones come kind by kind in the order of `kinds`,
/// each kind as its listing orders it; the ended and the forked by label.
pub fn offered(deps: &Deps, kinds: &[KindOf], wanted: Wanted) -> Result<Vec<Offered>, Failure> {
    let lookup = Lookup::new(deps.store);
    let topics = Topics::load(&lookup)?;
    let names = Names::of(&lookup);
    let today = deps.today()?;
    let mut found = Vec::new();
    for (at, document) in gathered(&lookup, kinds, wanted)? {
        let shown = row(&topics, &document);
        let ended = shown.as_ref().map(|shown| shown.row.ended.is_some());
        let kept = match wanted {
            Wanted::Live => ended == Some(false),
            Wanted::Ended => ended == Some(true),
            Wanted::Forked => true,
        };
        if !kept {
            continue;
        }
        let id = document.id();
        let (label, summary) = match &shown {
            Some(shown) => (shown.row.label.as_str(), shown.row.summary.as_str()),
            None => (id.short(), ""),
        };
        let place = match (wanted, &shown) {
            (Wanted::Live, Some(shown)) => (at, listed(shown, today)),
            _ => (0, (0, None, None, label.to_owned())),
        };
        let offered = Offered {
            document: id.clone(),
            address: names.address(id, label)?,
            summary: summary.to_owned(),
        };
        found.push((place, offered));
    }
    found.sort_by(|(a, one), (b, other)| (a, &one.document).cmp(&(b, &other.document)));
    Ok(found.into_iter().map(|(_, offered)| offered).collect())
}

fn gathered(
    lookup: &Lookup,
    kinds: &[KindOf],
    wanted: Wanted,
) -> Result<Vec<(usize, Rc<Document>)>, Failure> {
    let place = |document: &Document| {
        let kind = kind_of(document)?;
        kinds.iter().position(|wanted| *wanted == kind)
    };
    let mut gathered = Vec::new();
    if wanted == Wanted::Forked {
        for document in lookup.forks()? {
            gathered.extend(place(&document).map(|at| (at, document)));
        }
    } else {
        for (at, kind) in kinds.iter().enumerate() {
            let documents = lookup.of_kind(*kind)?;
            gathered.extend(documents.into_iter().map(|document| (at, document)));
        }
    }
    Ok(gathered)
}

type Place = (u8, Option<Date>, Option<Reverse<Date>>, String);

fn listed(shown: &Shown, today: Date) -> Place {
    let label = shown.row.label.clone();
    match &shown.record.content {
        Content::Fact(fact) => (u8::from(fact.idea), None, None, label),
        Content::Entry(entry) => (0, None, Some(Reverse(entry.date)), label),
        Content::Followup(followup) => {
            let (rank, on) = urgency(followup.trigger.as_ref(), today);
            (rank, on, None, label)
        }
        Content::Topic(_) | Content::Claim(_) => (0, None, None, label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{
        DONE, ENDED, ENDED_FACT, REMOVED, World, claim_fields, entry, fact, fact_fields, followup,
        fork, found, topic, world,
    };
    use crate::domain::ports::Store;
    use crate::domain::schema::address::Found;
    use crate::domain::testing::first;

    const NAMED: [KindOf; 4] = [KindOf::Topic, KindOf::Fact, KindOf::Entry, KindOf::Followup];

    fn addressed(world: &World, kinds: &[KindOf], wanted: Wanted) -> Vec<(DocumentId, String)> {
        let offered = offered(&world.deps(), kinds, wanted).unwrap();
        let mut rows: Vec<(DocumentId, String)> = (offered.into_iter())
            .map(|offered| (offered.document, offered.address))
            .collect();
        for (id, address) in &rows {
            assert_eq!(found(world, address), Found::One(id.clone()), "{address}");
        }
        rows.sort();
        rows
    }

    fn sorted(mut rows: Vec<(DocumentId, String)>) -> Vec<(DocumentId, String)> {
        rows.sort();
        rows
    }

    fn by_name(id: &DocumentId, name: &str) -> (DocumentId, String) {
        (id.clone(), name.to_owned())
    }

    fn by_id(id: &DocumentId) -> (DocumentId, String) {
        (id.clone(), id.short().to_owned())
    }

    fn whole(id: &DocumentId) -> (DocumentId, String) {
        (id.clone(), id.to_string())
    }

    #[test]
    fn an_ended_document_is_offered_by_its_name_or_by_its_id_where_the_name_finds_another() {
        let (world, lantern) = world();
        let fuse = fact(&world, &lantern, "fuse", ENDED_FACT);
        fact(&world, &lantern, "relay", "");
        let phone = topic(&world, "phone", ENDED);
        let case = fact(&world, &phone, "case", ENDED_FACT);
        let ordered = followup(&world, &[&lantern], "Order a fuse", DONE);
        followup(&world, &[&lantern], "Check the driver", "");
        let machine = world.machine();
        let ended_claim = format!("{}{REMOVED}", claim_fields(&machine, &lantern, ""));
        world.put("claim", &ended_claim, "\n");
        let ended = |world: &World| addressed(world, &NAMED, Wanted::Ended);

        let named = vec![
            by_id(&ordered),
            by_name(&fuse, "lantern/fuse"),
            by_name(&phone, "phone"),
            by_name(&case, "phone/case"),
        ];
        assert_eq!(ended(&world), sorted(named));
        let listed = offered(&world.deps(), &NAMED, Wanted::Ended).unwrap();
        let addresses: Vec<&str> = (listed.iter().map(|ended| ended.address.as_str())).collect();
        assert_eq!(addresses[1..], ["lantern/fuse", "phone", "phone/case"]);
        assert_eq!(listed[0].summary, "Order a fuse");

        let taken = topic(&world, "phone", "");
        fact(&world, &lantern, "fuse", "");
        assert_eq!(found(&world, "phone"), Found::One(taken));
        let by_ids = vec![by_id(&ordered), by_id(&fuse), by_id(&phone), by_id(&case)];
        assert_eq!(ended(&world), sorted(by_ids));

        let twice = [topic(&world, "torch", ENDED), topic(&world, "torch", ENDED)];
        let torches: Vec<_> = (ended(&world).into_iter())
            .filter(|(id, _)| twice.contains(id))
            .collect();
        assert_eq!(torches, sorted(twice.iter().map(by_id).collect()));
    }

    #[test]
    fn a_live_document_is_offered_by_its_id_where_its_name_is_held_by_another_too() {
        let (world, lantern) = world();
        let torches = [topic(&world, "torch", ""), topic(&world, "torch", "")];
        let wired = [
            entry(&world, "2026-10-08", "wiring", &[&lantern]),
            entry(&world, "2026-10-08", "wiring", &[&lantern]),
        ];
        let casing = entry(&world, "2026-10-07", "wiring", &[&lantern]);
        let relay = fact(&world, &lantern, "relay", "");
        let lit = fact(&world, &torches[0], "wick", "");
        let renamed = topic(&world, "lamp", "former_names = [\"lantern\"]\n");
        fact(&world, &lantern, "fuse", ENDED_FACT);
        let fuse = fact(&world, &lantern, "fuse", "");

        let mut live = vec![
            by_name(&world.machine(), "desk"),
            by_name(&lantern, "lantern"),
            by_name(&renamed, "lamp"),
            by_name(&casing, "2026-10-07-wiring"),
            by_name(&relay, "lantern/relay"),
            by_name(&fuse, "lantern/fuse"),
            by_id(&lit),
        ];
        live.extend(torches.iter().chain(&wired).map(by_id));
        assert_eq!(addressed(&world, &NAMED, Wanted::Live), sorted(live));
    }

    #[test]
    fn a_document_whose_short_id_is_a_topic_s_name_or_starts_another_id_is_offered_whole() {
        let (world, lantern) = world();
        let stored = |bytes: [u8; 16]| {
            let id = DocumentId::from_bytes(bytes);
            let fields =
                format!("created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"s\"\n");
            world
                .store
                .put(&first(&id, "followup", &fields, "\n"))
                .unwrap();
            id
        };
        let named_alike = stored([0xab; 16]);
        let topic = topic(&world, "abababab", "");
        let mut twin = [0xcd; 16];
        let first_twin = stored(twin);
        twin[15] = 0x01;
        let second_twin = stored(twin);
        let alone = stored([0xef; 16]);

        let followups = addressed(&world, &[KindOf::Followup], Wanted::Live);
        let expected = vec![
            whole(&named_alike),
            whole(&first_twin),
            whole(&second_twin),
            by_id(&alone),
        ];
        assert_eq!(followups, sorted(expected));
        let topics = addressed(&world, &[KindOf::Topic], Wanted::Live);
        assert!(topics.contains(&by_name(&topic, "abababab")), "{topics:?}");
    }

    #[test]
    fn a_forked_document_is_offered_by_an_address_that_finds_it_whoever_shares_its_name() {
        let (world, lantern) = world();
        assert_eq!(addressed(&world, &KindOf::ALL, Wanted::Forked), []);
        let relay = fact(&world, &lantern, "relay", "");
        let fuse = fact(&world, &lantern, "fuse", "");
        for id in [&relay, &fuse] {
            let name = if *id == relay { "relay" } else { "fuse" };
            fork(&world, id, &fact_fields(&lantern, name, "Pin four", ""));
        }
        let shared = fact(&world, &lantern, "relay", "");
        let open = followup(&world, &[&lantern], "Check the driver", "");
        let checked = format!(
            "created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"Check the driver\"\n"
        );
        fork(&world, &open, &checked);
        let machine = world.machine();
        let placed = claim_fields(&machine, &lantern, "~/projects/lantern");
        let claim = world.put("claim", &placed, "\n");
        fork(&world, &claim, &placed);
        let unread = topic(&world, "atlas", "");
        fork(&world, &unread, "name = 4\n");

        let forked = vec![
            by_id(&relay),
            by_name(&fuse, "lantern/fuse"),
            by_id(&open),
            by_id(&claim),
            by_id(&unread),
        ];
        assert_eq!(
            addressed(&world, &KindOf::ALL, Wanted::Forked),
            sorted(forked)
        );
        let listed = offered(&world.deps(), &KindOf::ALL, Wanted::Forked).unwrap();
        let summary = |id: &DocumentId| {
            let of = listed.iter().find(|offered| offered.document == *id);
            of.expect("a forked document").summary.as_str()
        };
        assert_eq!(
            [
                summary(&fuse),
                summary(&open),
                summary(&claim),
                summary(&unread)
            ],
            ["Pin four", "Check the driver", "", ""]
        );
        let live = addressed(&world, &[KindOf::Fact], Wanted::Live);
        assert_eq!(
            live,
            sorted(vec![
                by_id(&relay),
                by_id(&shared),
                by_name(&fuse, "lantern/fuse")
            ])
        );
    }

    #[test]
    fn the_live_come_kind_by_kind_each_as_its_listing_orders_it() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay", "");
        fact(&world, &lantern, "dimmer", "idea = true\n");
        fact(&world, &lantern, "cord", "idea = true\n");
        fact(&world, &lantern, "fuse", "");
        entry(&world, "2026-10-07", "casing", &[&lantern]);
        entry(&world, "2026-10-08", "wiring", &[&lantern]);
        entry(&world, "2026-10-08", "soldering", &[&lantern]);
        let bare = followup(&world, &[&lantern], "s", "");
        let touching = followup(
            &world,
            &[&lantern],
            "s",
            &format!("touching = \"{lantern}\"\n"),
        );
        let later = followup(
            &world,
            &[&lantern],
            "s",
            "look_again = 2026-10-10\nwhy = \"w\"\n",
        );
        let due = followup(
            &world,
            &[&lantern],
            "s",
            "look_again = 2026-10-01\nwhy = \"w\"\n",
        );
        let kinds = [KindOf::Entry, KindOf::Followup, KindOf::Fact, KindOf::Topic];
        let listed = offered(&world.deps(), &kinds, Wanted::Live).unwrap();
        let addresses: Vec<&str> =
            (listed.iter().map(|offered| offered.address.as_str())).collect();
        assert_eq!(
            addresses,
            [
                "2026-10-08-soldering",
                "2026-10-08-wiring",
                "2026-10-07-casing",
                due.short(),
                later.short(),
                touching.short(),
                bare.short(),
                "lantern/fuse",
                "lantern/relay",
                "lantern/cord",
                "lantern/dimmer",
                "desk",
                "lantern",
            ]
        );
    }

    fn scans_for(ended: usize) -> usize {
        let (world, lantern) = world();
        for _ in 0..ended {
            followup(&world, &[&lantern], "s", DONE);
        }
        let world = world.counted();
        let offered = offered(&world.deps(), &NAMED, Wanted::Ended).unwrap();
        assert_eq!(offered.len(), ended);
        let store = &world.store;
        assert_eq!((store.holdings.get(), store.documents.get()), (0, 0));
        store.scans()
    }

    #[test]
    fn the_store_is_asked_only_for_the_kinds_the_addresses_offered_are_checked_against() {
        let (world, lantern) = world();
        fact(&world, &lantern, "relay", "");
        entry(&world, "2026-10-08", "wiring", &[&lantern]);
        followup(&world, &[&lantern], "s", "");
        let world = world.counted();
        let asked = |kinds: &[KindOf], wanted: Wanted| {
            let before = (world.store.kinds.get(), world.store.scans());
            offered(&world.deps(), kinds, wanted).unwrap();
            let store = &world.store;
            (store.kinds.get() - before.0, store.scans() - before.1)
        };
        assert_eq!(asked(&[KindOf::Topic], Wanted::Live), (1, 1));
        assert_eq!(asked(&[KindOf::Fact], Wanted::Live), (2, 2));
        assert_eq!(asked(&[KindOf::Entry], Wanted::Ended), (2, 2));
        assert_eq!(asked(&KindOf::ALL, Wanted::Forked), (1, 2));
        let before = world.store.prefixes.get();
        assert_eq!(asked(&[KindOf::Followup], Wanted::Live), (2, 18));
        assert_eq!(world.store.prefixes.get() - before, 16);
    }

    #[test]
    fn the_store_is_asked_the_same_whatever_the_number_of_documents_offered() {
        assert_eq!(scans_for(1), scans_for(500));
    }
}
