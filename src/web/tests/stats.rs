use super::{has, lacks, ok, page};
use crate::app::testing::{entry, world_with_atlas};

#[test]
fn stats_draw_the_days_of_a_topic_and_link_each_day_holding_an_entry_to_its_entries() {
    let (world, lantern, atlas) = world_with_atlas();
    entry(&world, "2026-10-08", "wiring", &[&lantern]);
    entry(&world, "2026-10-08", "relay", &[&lantern]);
    entry(&world, "2026-10-07", "map", &[&atlas]);
    entry(&world, "2025-03-02", "early", &[&lantern]);

    let html = ok(&world, "/stats");
    has(&html, "<a href=\"/stats\" aria-current=\"page\">Stats</a>");
    has(
        &html,
        "<p class=\"chips topics-chosen\"><a href=\"/stats\" aria-current=\"true\">All</a>\
         <a href=\"/stats?topic=lantern\">lantern <span class=\"count\">2</span></a>\
         <a href=\"/stats?topic=atlas\">atlas <span class=\"count\">1</span></a>\
         <a href=\"/stats?topic=desk\">desk <span class=\"count\">0</span></a></p>",
    );
    has(
        &html,
        "<a class=\"l4\" href=\"/entries?on=2026-10-08\" title=\"2026-10-08: 2 entries\"",
    );
    has(
        &html,
        "<a class=\"l2\" href=\"/entries?on=2026-10-07\" title=\"2026-10-07: 1 entry\"",
    );
    has(
        &html,
        "<span class=\"l0\" title=\"2026-10-09: 0 entries\"></span>",
    );
    has(
        &html,
        "3 entries in the last year · <strong>last year</strong> \
         <a href=\"/stats?year=2026\">2026</a> <a href=\"/stats?year=2025\">2025</a></p>",
    );

    let html = ok(&world, "/stats?topic=lantern&year=2025");
    has(&html, "<a href=\"/stats?year=2025\">All</a>");
    has(
        &html,
        "<a href=\"/stats?topic=lantern&#38;year=2025\" aria-current=\"true\">lantern \
         <span class=\"count\">1</span></a>",
    );
    has(
        &html,
        "<a class=\"l4\" href=\"/entries?topic=lantern&#38;on=2025-03-02\"",
    );
    has(
        &html,
        &format!(
            "1 entry in 2025 under <a href=\"/doc/{lantern}\">lantern</a> · \
             <a href=\"/stats?topic=lantern\">last year</a> \
             <a href=\"/stats?topic=lantern&#38;year=2026\">2026</a> <strong>2025</strong></p>"
        ),
    );
    lacks(&html, "2026-10-08");

    let listed = ok(&world, "/entries?on=2026-10-08&topic=lantern");
    has(
        &listed,
        "<p class=\"summary\">2 entries on 2026-10-08 under ",
    );
    has(
        &listed,
        "<a href=\"/entries?topic=lantern&#38;on=2026-10-08&#38;ended=1\">ended too</a>",
    );

    for (target, status) in [
        ("/stats?year=then", 400),
        ("/stats?year=10000", 400),
        ("/stats?topic=torch", 404),
        ("/entries?on=yesterday", 400),
    ] {
        assert_eq!(page(&world, target).status, status, "{target}");
    }
}

#[test]
fn a_topic_s_page_links_its_stats() {
    let (world, lantern, _) = world_with_atlas();
    has(
        &ok(&world, &format!("/doc/{lantern}")),
        &format!("history</a> <a href=\"/stats?topic={lantern}\">stats</a>"),
    );
}
