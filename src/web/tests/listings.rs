use super::{has, lacks, ok};
use crate::app::testing::{
    ENDED_FACT, entry, fact, fact_fields, followup, fork, world, world_with_atlas,
};
use crate::app::{Deps, list, show};
use crate::domain::id::DocumentId;
use crate::web::respond;

#[test]
fn facts_are_listed_with_their_links_and_marks() {
    let (world, lantern, atlas) = world_with_atlas();
    let relay = world.put(
        "fact",
        &fact_fields(&lantern, "relay-pin", "Pin four", ""),
        "\n",
    );
    fact(&world, &atlas, "map", "");
    let wick = fact(&world, &lantern, "wick", ENDED_FACT);
    let fuse = fact(&world, &lantern, "fuse", "");
    fork(&world, &fuse, &fact_fields(&lantern, "fuse", "s", ""));
    fact(&world, &lantern, "dimmer", "idea = true\n");

    let html = ok(&world, "/facts");
    has(
        &html,
        "<p class=\"summary\">3 facts · <a href=\"/facts?ended=1\">ended too</a></p>",
    );
    has(
        &html,
        &format!(
            "<li><span class=\"label\">confirmed 2026-09-04</span> \
             <a class=\"title\" href=\"/doc/{relay}\">lantern/relay-pin</a> \
             <span class=\"chips\"><a href=\"/doc/{lantern}\">lantern</a></span>\
             <div class=\"what\">Pin four</div></li>"
        ),
    );
    has(
        &html,
        &format!(
            "<a class=\"title\" href=\"/doc/{fuse}\">lantern/fuse</a> \
             <span class=\"mark\">forked</span>"
        ),
    );
    has(&html, "atlas/map");
    lacks(&html, "lantern/wick");
    lacks(&html, "lantern/dimmer");

    let html = ok(&world, "/facts?ended=1");
    has(
        &html,
        "<p class=\"summary\">4 facts · <a href=\"/facts\">unended only</a></p>",
    );
    has(
        &html,
        &format!(
            "<a class=\"title ended\" href=\"/doc/{wick}\" title=\"ended false\">lantern/wick</a> \
             <span class=\"mark\">ended false</span>"
        ),
    );

    let html = ok(&world, "/facts?topic=atlas&colour=red");
    has(
        &html,
        &format!(
            "<p class=\"summary\">1 fact under <a href=\"/doc/{atlas}\">atlas</a> · \
             <a href=\"/facts?topic=atlas&#38;ended=1\">ended too</a></p>"
        ),
    );
    let by_id = ok(&world, &format!("/facts?topic={atlas}"));
    has(
        &by_id,
        &format!(
            "<p class=\"summary\">1 fact under <a href=\"/doc/{atlas}\">atlas</a> · \
             <a href=\"/facts?topic={atlas}&#38;ended=1\">ended too</a></p>"
        ),
    );
    has(&html, "atlas/map");
    lacks(&html, "lantern/relay-pin");

    let html = ok(&world, "/facts?topic=lantern&ended=1");
    has(&html, "<a href=\"/facts?topic=lantern\">unended only</a>");
    has(&html, "lantern/wick");
    lacks(&html, "atlas/map");
}

#[test]
fn ideas_are_the_facts_flagged_so() {
    let (world, lantern, atlas) = world_with_atlas();
    fact(&world, &lantern, "relay-pin", "");
    let dimmer = fact(&world, &lantern, "dimmer", "idea = true\n");
    fact(&world, &atlas, "legend", "idea = true\n");
    fact(
        &world,
        &atlas,
        "scale",
        &format!("idea = true\n{ENDED_FACT}"),
    );

    let html = ok(&world, "/ideas");
    has(
        &html,
        "<p class=\"summary\">2 ideas · <a href=\"/ideas?ended=1\">ended too</a></p>",
    );
    has(
        &html,
        &format!("<a class=\"title\" href=\"/doc/{dimmer}\">lantern/dimmer</a>"),
    );
    has(&html, "atlas/legend");
    lacks(&html, "lantern/relay-pin");
    lacks(&html, "atlas/scale");
    has(&ok(&world, "/ideas?ended=1"), "atlas/scale");
    let under = ok(&world, "/ideas?topic=atlas");
    has(&under, "atlas/legend");
    lacks(&under, "lantern/dimmer");
}

#[test]
fn entries_are_listed_newest_first_with_their_day_and_topics() {
    let (world, lantern, atlas) = world_with_atlas();
    let driver = entry(&world, "2026-10-08", "lamp-driver", &[&lantern, &atlas]);
    entry(&world, "2026-10-09", "wiring", &[&lantern]);
    let old = entry(&world, "2026-10-01", "old-case", &[&atlas]);
    world.amend(
        &old,
        &format!(
            "name = \"old-case\"\ndate = 2026-10-01\nmachine = \"{}\"\n\
             topics = [\"{atlas}\"]\nsummary = \"s\"\n\
             ended = \"retracted\"\nended_on = 2026-10-09\nnote = \"n\"\n",
            world.machine()
        ),
        "\n",
    );

    let html = ok(&world, "/entries");
    has(
        &html,
        "<p class=\"summary\">2 entries · <a href=\"/entries?ended=1\">ended too</a></p>",
    );
    has(
        &html,
        &format!(
            "<li><span class=\"label\">2026-10-08</span> \
             <a class=\"title\" href=\"/doc/{driver}\">2026-10-08-lamp-driver</a> \
             <span class=\"chips\"><a href=\"/doc/{lantern}\">lantern</a>\
             <a href=\"/doc/{atlas}\">atlas</a></span><div class=\"what\">s</div></li>"
        ),
    );
    let newest = html.find("2026-10-09-wiring").expect("the newest entry");
    let older = html
        .find("2026-10-08-lamp-driver")
        .expect("the older entry");
    assert!(newest < older, "{html}");
    lacks(&html, "old-case");

    let under = ok(&world, "/entries?topic=atlas&ended=1");
    has(&under, "2026-10-08-lamp-driver");
    has(&under, "<span class=\"mark\">ended retracted</span>");
    lacks(&under, "2026-10-09-wiring");
}

#[test]
fn followups_lead_with_the_state_of_their_trigger() {
    let (world, lantern, atlas) = world_with_atlas();
    let driver = entry(&world, "2026-10-08", "lamp-driver", &[&lantern]);
    let relay = fact(&world, &lantern, "relay-pin", "");
    let due = followup(
        &world,
        &[&lantern],
        "Order a fuse",
        "look_again = 2026-10-01\nwhy = \"the shop reopens\"\n",
    );
    followup(
        &world,
        &[&lantern],
        "Check the driver",
        "look_again = 2026-10-20\nwhy = \"w\"\n",
    );
    followup(
        &world,
        &[&atlas],
        "Fit the lens",
        &format!("touching = \"{atlas}\"\n"),
    );
    followup(
        &world,
        &[&lantern],
        "Paint the case",
        &format!("entry = \"{driver}\"\nabout = \"{relay}\"\n"),
    );
    let done = followup(&world, &[&lantern], "Solder it", "");
    world.amend(
        &done,
        &format!(
            "created = 2026-09-04\ntopics = [\"{lantern}\"]\nsummary = \"Solder it\"\n\
             ended = \"done\"\nended_on = 2026-10-09\nnote = \"n\"\n"
        ),
        "\n",
    );

    let html = ok(&world, "/followups");
    has(
        &html,
        "<p class=\"summary\">4 follow-ups · <a href=\"/followups?ended=1\">ended too</a></p>",
    );
    has(
        &html,
        &format!(
            "<li class=\"due\"><span class=\"label\">due 2026-10-01</span> \
             <a class=\"title\" href=\"/doc/{due}\">{}</a> \
             <span class=\"chips\"><a href=\"/doc/{lantern}\">lantern</a></span>\
             <div class=\"what\">Order a fuse</div>\
             <span class=\"details\">the shop reopens</span></li>",
            due.short()
        ),
    );
    has(&html, "<li><span class=\"label\">by 2026-10-20</span> ");
    has(
        &html,
        &format!("<li><span class=\"label\">touching <a href=\"/doc/{atlas}\">atlas</a></span> "),
    );
    has(&html, "<li><span class=\"label\">no trigger</span> ");
    has(
        &html,
        &format!(
            "<span class=\"details\">in <a href=\"/doc/{driver}\">2026-10-08-lamp-driver</a> · \
             about <a href=\"/doc/{relay}\">lantern/relay-pin</a></span>"
        ),
    );
    assert_eq!(html.matches("class=\"due\"").count(), 1);
    lacks(&html, "Solder it");

    let ended = ok(&world, "/followups?ended=1");
    has(&ended, "Solder it");
    has(&ended, "<span class=\"mark\">ended done</span>");

    let under = ok(&world, "/followups?topic=atlas");
    has(&under, "Fit the lens");
    lacks(&under, "Order a fuse");
    has(
        &under,
        &format!("<p class=\"summary\">1 follow-up under <a href=\"/doc/{atlas}\">atlas</a> · "),
    );

    let of_entry = ok(&world, "/followups?topic=2026-10-08-lamp-driver");
    has(
        &of_entry,
        &format!(
            "<p class=\"summary\">1 follow-up under \
             <a href=\"/doc/{driver}\">2026-10-08-lamp-driver</a> · \
             <a href=\"/followups?topic=2026-10-08-lamp-driver&#38;ended=1\">ended too</a></p>"
        ),
    );
    has(&of_entry, "Paint the case");
    lacks(&of_entry, "Order a fuse");
    lacks(&of_entry, "/doc/2026-10-08-lamp-driver");
}

#[test]
fn an_empty_listing_says_so() {
    let (world, _) = world();
    has(
        &ok(&world, "/entries"),
        "<p class=\"muted\">Nothing here.</p>",
    );
}

#[test]
fn a_query_flag_is_read_as_the_use_case_s_argument() {
    let (world, lantern) = world();
    fact(&world, &lantern, "kept", "");
    fact(&world, &lantern, "gone", ENDED_FACT);
    for (target, listed) in [
        ("/facts?ended=1", true),
        ("/facts?ended=0", false),
        ("/facts?ended", false),
        ("/facts?topic=&ended=1", true),
        ("/facts?topic=lantern&ended=1&ended=0", true),
        ("/facts?Ended=1", false),
    ] {
        let html = ok(&world, target);
        assert_eq!(html.contains("lantern/gone"), listed, "{target}");
        has(&html, "lantern/kept");
    }
}

fn scans_of(ask: &dyn Fn(&Deps, &DocumentId)) -> usize {
    let (world, lantern) = world();
    fact(&world, &lantern, "relay-pin", "");
    let world = world.counted();
    let deps = world.deps();
    ask(&deps, &lantern);
    world.store.scans()
}

fn page_scans(target: &str, narrowed: bool) -> usize {
    scans_of(&|deps, lantern| {
        let target = if narrowed {
            format!("{target}?topic={lantern}")
        } else {
            target.to_owned()
        };
        let page = respond(deps, "GET", &target);
        assert_eq!(page.status, 200, "{target}: {}", page.html);
        assert_eq!(page.html.contains(" under <a href="), narrowed, "{target}");
    })
}

#[test]
fn a_narrowed_listing_makes_one_more_read_to_name_what_it_is_under() {
    let listing = scans_of(&|deps, lantern| {
        list::facts(deps, Some(lantern.as_str()), false).expect("a listing");
    });
    let naming = scans_of(&|deps, lantern| {
        show::show(deps, lantern.as_str()).expect("a document");
    });
    assert_eq!((listing, naming), (3, 1));
    for target in ["/facts", "/ideas", "/entries", "/followups"] {
        assert_eq!(page_scans(target, false), 2, "{target}");
        assert_eq!(page_scans(target, true), listing + naming, "{target}");
    }
}
