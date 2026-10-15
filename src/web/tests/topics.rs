use super::{has, lacks, main_of, ok, page, stamp};
use crate::app::testing::{
    ENDED, ENDED_FACT, REMOVED, World, claim_fields, entry, fact, followup, fork, head, part_of,
    topic, topic_fields, uses, world,
};
use crate::domain::id::{DocumentId, VersionId};
use crate::web::respond;

fn tabs(topic: &DocumentId, current: &str, counts: [usize; 5]) -> String {
    let tabs = [
        ("?show=parts", "Parts"),
        ("", "Facts"),
        ("?show=ideas", "Ideas"),
        ("?show=followups", "Follow-ups"),
        ("?show=entries", "Entries"),
    ];
    let links: Vec<String> = (tabs.iter().zip(counts))
        .map(|((show, name), count)| {
            let here = if *name == current {
                " aria-current=\"page\""
            } else {
                ""
            };
            format!(
                "<a href=\"/doc/{topic}{show}\"{here}>{name} \
                 <span class=\"label\">{count}</span></a>"
            )
        })
        .collect();
    format!("<p class=\"tabs\">{}</p>", links.concat())
}

struct Lit {
    world: World,
    lantern: DocumentId,
    atlas: DocumentId,
    saved: VersionId,
}

fn lit() -> Lit {
    let (world, lantern) = world();
    let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
    topic(
        &world,
        "globe",
        &format!("{}{}", part_of(&[&lantern]), ENDED),
    );
    let saved = world.amend_linking(
        &lantern,
        &topic_fields("lantern", &uses(&[&world.machine()])),
        "A *lamp*, see [[atlas]].\n",
        &[("atlas", Some(&atlas))],
    );
    Lit {
        world,
        lantern,
        atlas,
        saved,
    }
}

const FILED: [usize; 5] = [1, 2, 1, 1, 1];

fn filed(world: &World, lantern: &DocumentId, atlas: &DocumentId) -> [DocumentId; 5] {
    let relay = fact(world, lantern, "relay-pin", "");
    let map = fact(world, atlas, "map", "");
    fact(world, lantern, "wick", ENDED_FACT);
    let dimmer = fact(world, atlas, "dimmer", "idea = true\n");
    let open = followup(world, &[lantern, atlas], "Order a fuse", "");
    let driver = entry(world, "2026-10-08", "lamp-driver", &[atlas]);
    fact(world, &world.machine(), "chair", "");
    [relay, map, dimmer, open, driver]
}

#[test]
fn a_topic_is_its_document_over_one_of_its_sections_with_a_count_for_each() {
    let Lit {
        world,
        lantern,
        atlas,
        saved,
    } = lit();
    let desk = world.machine();
    let [relay, map, ..] = filed(&world, &lantern, &atlas);

    let html = ok(&world, &format!("/doc/{lantern}"));
    has(&html, "<title>lantern · worklog</title>");
    has(&html, "<h1>lantern</h1>");
    has(
        &html,
        &format!(
            "<p class=\"meta\"><span class=\"kind\">topic</span>\
             <a href=\"/history/{lantern}\">history</a></p>"
        ),
    );
    has(
        &html,
        &format!("<p class=\"stamp\">{}</p>", stamp(&world, &saved, "save")),
    );
    has(
        &html,
        &format!("<dt>uses</dt><dd><a href=\"/doc/{desk}\">desk</a></dd>"),
    );
    has(
        &html,
        &format!(
            "<div class=\"body\"><p>A <em>lamp</em>, see \
             <a href=\"/doc/{atlas}\">atlas</a>.</p>\n</div>"
        ),
    );
    has(&html, &tabs(&lantern, "Facts", FILED));
    has(
        &html,
        &format!("<h2><a href=\"/facts?topic={lantern}\">Facts</a></h2>"),
    );
    let listed = ok(&world, &format!("/facts?topic={lantern}"));
    assert_eq!(main_of(&listed).matches("<li>").count(), 2);
    has(&listed, "lantern/relay-pin");
    has(&listed, "atlas/map");
    has(
        &html,
        &format!(
            "<li><span class=\"label\">confirmed 2026-09-04</span> \
             <a class=\"title\" href=\"/doc/{relay}\">lantern/relay-pin</a>\
             <div class=\"what\">s</div></li>"
        ),
    );
    let of_part = format!(
        "<a class=\"title\" href=\"/doc/{map}\">atlas/map</a> \
         <span class=\"chips\"><a href=\"/doc/{atlas}\">atlas</a></span><div class=\"what\">s</div></li>"
    );
    has(&html, &of_part);
    has(&listed, &of_part);
    lacks(
        &listed,
        &format!("<a href=\"/doc/{lantern}\">lantern</a></span>"),
    );
    for absent in ["lantern/wick", "atlas/dimmer", "desk/chair", "globe"] {
        lacks(&html, absent);
    }
    assert_eq!(html.matches("<article class=\"head\">").count(), 1);
    assert_eq!(html.matches(" aria-current=\"page\"").count(), 2);
    has(&html, "<a href=\"/\" aria-current=\"page\">Topics</a>");
    assert_eq!(main_of(&html).matches("<li>").count(), 2);
    for target in [
        "/doc/lantern".to_owned(),
        "/doc/lantern/".to_owned(),
        format!("/doc/{}", lantern.short()),
        format!("/doc/{lantern}?show=facts"),
    ] {
        assert_eq!(ok(&world, &target), html, "{target}");
    }
}

#[test]
fn a_topic_shows_the_section_asked_for_under_the_same_counts() {
    let Lit {
        world,
        lantern,
        atlas,
        ..
    } = lit();
    let [_, _, dimmer, open, driver] = filed(&world, &lantern, &atlas);
    let counts = FILED;

    let parts = ok(&world, "/doc/lantern?show=parts");
    has(&parts, &tabs(&lantern, "Parts", counts));
    has(
        &parts,
        &format!(
            "<h2>Parts</h2>\n<ul class=\"topics\">\
             <li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a> \
             <span class=\"label\">1 fact</span> \
             <span class=\"label\">1 idea</span> <span class=\"label\">1 open</span>\
             <div class=\"what\">s</div></li>\
             </ul>"
        ),
    );
    lacks(&parts, "lantern/relay-pin");

    let ideas = ok(&world, "/doc/lantern?show=ideas");
    has(&ideas, &tabs(&lantern, "Ideas", counts));
    has(
        &ideas,
        &format!("<h2><a href=\"/ideas?topic={lantern}\">Ideas</a></h2>"),
    );
    has(
        &ideas,
        &format!("<a class=\"title\" href=\"/doc/{dimmer}\">atlas/dimmer</a>"),
    );
    assert_eq!(main_of(&ideas).matches("<li>").count(), 1);

    let followups = ok(&world, "/doc/lantern?show=followups");
    has(&followups, &tabs(&lantern, "Follow-ups", counts));
    has(
        &followups,
        &format!("<h2><a href=\"/followups?topic={lantern}\">Follow-ups</a></h2>"),
    );
    let listed = ok(&world, &format!("/followups?topic={lantern}"));
    assert_eq!(main_of(&listed).matches("<li>").count(), 1);
    has(
        &followups,
        &format!(
            "<li><span class=\"label\">no trigger</span> \
             <a class=\"title\" href=\"/doc/{open}\">{}</a> \
             <span class=\"chips\"><a href=\"/doc/{atlas}\">atlas</a></span>\
             <div class=\"what\">Order a fuse</div></li>",
            open.short()
        ),
    );
    has(
        &listed,
        &format!(
            "<span class=\"chips\"><a href=\"/doc/{atlas}\">atlas</a></span>\
             <div class=\"what\">Order a fuse</div></li>"
        ),
    );

    let entries = ok(&world, "/doc/lantern?show=entries");
    has(&entries, &tabs(&lantern, "Entries", counts));
    has(
        &entries,
        &format!("<h2><a href=\"/entries?topic={lantern}\">Entries</a></h2>"),
    );
    has(
        &entries,
        &format!(
            "<li><span class=\"label\">2026-10-08</span> \
             <a class=\"title\" href=\"/doc/{driver}\">2026-10-08-lamp-driver</a>"
        ),
    );

    let of_part = ok(&world, &format!("/doc/{atlas}?show=parts"));
    has(&of_part, &tabs(&atlas, "Parts", [0, 1, 1, 1, 1]));
    has(&of_part, "<h2>Parts</h2>\n<p class=\"muted\">None.</p>");
    has(
        &of_part,
        &format!("<dt>part_of</dt><dd><a href=\"/doc/{lantern}\">lantern</a></dd>"),
    );
}

#[test]
fn a_topic_that_is_forked_or_ended_is_marked_as_its_document_is() {
    let Lit {
        world,
        lantern,
        atlas,
        ..
    } = lit();
    fact(&world, &atlas, "map", "");
    world.amend(&atlas, &topic_fields("atlas", ENDED), "\n");
    let ended = ok(&world, "/doc/atlas");
    has(
        &ended,
        &format!(
            "<a href=\"/history/{atlas}\">history</a> \
             <span class=\"mark\">ended retired</span></p>"
        ),
    );
    has(&ended, "atlas/map");

    fork(&world, &lantern, &topic_fields("lantern", ""));
    let forked = ok(&world, "/doc/lantern");
    has(
        &forked,
        &format!(
            "<a href=\"/history/{lantern}\">history</a> <span class=\"mark\">forked</span></p>"
        ),
    );
    assert_eq!(forked.matches("<article class=\"head\">").count(), 2);
    assert_eq!(forked.matches("<h2 class=\"stamp\">").count(), 2);
    has(&forked, &tabs(&lantern, "Facts", [0, 0, 0, 0, 0]));
    has(
        &forked,
        &format!(
            "<h2><a href=\"/facts?topic={lantern}\">Facts</a></h2>\n\
             <p class=\"muted\">None.</p>"
        ),
    );
}

#[test]
fn a_topic_s_document_page_is_its_topic_page() {
    let Lit {
        world,
        lantern,
        atlas,
        ..
    } = lit();
    let [relay, ..] = filed(&world, &lantern, &atlas);
    for show in ["", "?show=parts", "?show=entries"] {
        let page = ok(&world, &format!("/doc/{lantern}{show}"));
        has(&page, "<p class=\"tabs\">");
        assert_eq!(ok(&world, &format!("/doc/lantern{show}")), page, "{show}");
    }
    let bad = page(&world, &format!("/doc/{lantern}?show=tags"));
    assert_eq!(bad.status, 400, "{}", bad.html);

    let fact = ok(&world, &format!("/doc/{relay}?show=tags"));
    has(&fact, "<h1>lantern/relay-pin</h1>");
    lacks(&fact, "class=\"tabs\"");
    assert_eq!(fact, ok(&world, &format!("/doc/{relay}")));

    let part = ok(&world, &format!("/doc/{atlas}"));
    let to_whole = format!("/doc/{lantern}");
    has(
        &part,
        &format!("<dt>part_of</dt><dd><a href=\"{to_whole}\">lantern</a></dd>"),
    );
    let whole = ok(&world, &to_whole);
    has(&whole, "<h1>lantern</h1>");
    has(&whole, &tabs(&lantern, "Facts", FILED));

    let version = head(&world, &lantern).id;
    let of_version = ok(&world, &format!("/doc/{version}"));
    has(&of_version, "<span class=\"kind\">version</span>");
    lacks(&of_version, "class=\"tabs\"");
}

#[test]
fn a_tab_s_count_is_the_length_of_the_listing_its_section_links_to() {
    let (world, lantern) = world();
    let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
    let phone = topic(&world, "phone", &part_of(&[&atlas]));
    fact(&world, &phone, "dial", "");
    fact(&world, &phone, "case", "");
    fact(&world, &phone, "strap", "idea = true\n");
    fact(&world, &phone, "gone", ENDED_FACT);
    followup(&world, &[&phone, &atlas], "open", "");
    entry(&world, "2026-10-08", "wiring", &[&phone]);
    entry(&world, "2026-10-09", "case", &[&phone]);
    entry(&world, "2026-10-09", "chair", &[&world.machine()]);

    let rows = |target: String| main_of(&ok(&world, &target)).matches("<li>").count();
    for (topic, counts) in [
        (&lantern, [1, 2, 1, 1, 2]),
        (&atlas, [1, 2, 1, 1, 2]),
        (&phone, [0, 2, 1, 1, 2]),
    ] {
        has(
            &ok(&world, &format!("/doc/{topic}")),
            &tabs(topic, "Facts", counts),
        );
        let listed = [
            rows(format!("/doc/{topic}?show=parts")),
            rows(format!("/facts?topic={topic}")),
            rows(format!("/ideas?topic={topic}")),
            rows(format!("/followups?topic={topic}")),
            rows(format!("/entries?topic={topic}")),
        ];
        assert_eq!(listed, counts, "{topic}");
    }
}

#[test]
fn a_machine_topic_lists_its_claims_by_topic_under_a_tab_of_its_own() {
    let (world, lantern) = world();
    let desk = world.machine();
    let atlas = topic(&world, "atlas", "");
    let anywhere = world.put("claim", &claim_fields(&desk, &atlas, ""), "\n");
    let here = world.put("claim", &claim_fields(&desk, &lantern, "~/lantern"), "\n");
    let lamp = world.put("claim", &claim_fields(&desk, &lantern, "~/lamp"), "\n");
    let removed = format!("{}{REMOVED}", claim_fields(&desk, &lantern, "~/old"));
    world.put("claim", &removed, "\n");

    let claims = ok(&world, &format!("/doc/{desk}?show=claims"));
    has(
        &claims,
        &format!(
            "<a href=\"/doc/{desk}?show=claims\" aria-current=\"page\">Claims \
             <span class=\"label\">2</span></a></p>"
        ),
    );
    has(
        &claims,
        &format!(
            "<h2>Claims</h2>\n<ul class=\"items\">\
             <li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a>\
             <span class=\"details\"><a href=\"/doc/{anywhere}\">anywhere</a></span></li>\
             <li><a class=\"title\" href=\"/doc/{lantern}\">lantern</a>\
             <span class=\"details\"><a href=\"/doc/{lamp}\">~/lamp</a></span>\
             <span class=\"details\"><a href=\"/doc/{here}\">~/lantern</a></span></li></ul>"
        ),
    );
    has(&ok(&world, &format!("/doc/{desk}")), "Claims <span");

    lacks(&ok(&world, &format!("/doc/{lantern}")), "Claims");
    has(
        &ok(&world, &format!("/doc/{lantern}?show=claims")),
        "<h2>Claims</h2>\n<p class=\"muted\">None.</p>",
    );
}

#[test]
fn a_part_that_is_forked_is_marked_in_the_parts_of_its_topic() {
    let Lit {
        world,
        lantern,
        atlas,
        ..
    } = lit();
    let fields = topic_fields("atlas", &part_of(&[&lantern]));
    fork(&world, &atlas, &fields);
    has(
        &ok(&world, "/doc/lantern?show=parts"),
        &format!(
            "<li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a> \
             <span class=\"mark\">forked</span><div class=\"what\">s</div></li>"
        ),
    );
}

#[test]
fn a_topic_page_of_no_document_or_no_section_is_the_failure_it_meets() {
    let (world, lantern) = world();
    fact(&world, &lantern, "relay-pin", "");
    for (target, status, text) in [
        ("/doc/nowhere".to_owned(), 404, "nowhere: names no document"),
        (
            "/doc/lantern?show=tags".to_owned(),
            400,
            "show: tags is no section of a topic",
        ),
        (
            "/doc/lantern?show=".to_owned(),
            400,
            "show:  is no section of a topic",
        ),
        (
            "/doc/nowhere?show=tags".to_owned(),
            404,
            "nowhere: names no document",
        ),
        ("/doc/Not%20A%20Topic!".to_owned(), 400, "Not A Topic!"),
        ("/topic".to_owned(), 404, "no page at /topic"),
        ("/topic/".to_owned(), 404, "no page at /topic"),
    ] {
        let page = page(&world, &target);
        assert_eq!(page.status, status, "{target}: {}", page.html);
        has(&page.html, &format!("<h1>{status}</h1>"));
        has(&page.html, text);
    }
}

fn topic_page_scans(size: usize) -> usize {
    let (world, lantern) = world();
    for index in 0..size {
        let part = topic(&world, &format!("part-{index}"), &part_of(&[&lantern]));
        fact(&world, &lantern, &format!("pin-{index}"), "");
        fact(&world, &part, "cord", "");
        fact(&world, &part, "dimmer", "idea = true\n");
        followup(&world, &[&part, &lantern], "open", "");
        entry(&world, "2026-10-08", &format!("day-{index}"), &[&part]);
    }
    let world = world.counted();
    let deps = world.deps();
    for (show, current, rows) in [
        ("", "Facts", 2 * size),
        ("?show=parts", "Parts", size),
        ("?show=ideas", "Ideas", size),
        ("?show=followups", "Follow-ups", size),
        ("?show=entries", "Entries", size),
    ] {
        let page = respond(&deps, "GET", &format!("/doc/{lantern}{show}"));
        assert_eq!(page.status, 200, "{show}: {}", page.html);
        let counts = [size, 2 * size, size, size, size];
        has(&page.html, &tabs(&lantern, current, counts));
        assert_eq!(main_of(&page.html).matches("<li>").count(), rows, "{show}");
    }
    world.store.scans()
}

#[test]
fn a_topic_page_scans_the_store_the_same_whatever_the_topic_holds() {
    let a_request = 18;
    assert_eq!(topic_page_scans(1), 5 * a_request);
    assert_eq!(topic_page_scans(7), 5 * a_request);
}
