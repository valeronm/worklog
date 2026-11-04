use super::{has, lacks, ok, page};
use worklog_app::testing::{ENDED_FACT, entry, fact, fact_fields, fork, topic, world_with_atlas};

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
fn stats_offer_topics_that_share_a_name_by_an_address_each_answers_to() {
    let (world, _, _) = world_with_atlas();
    let one = topic(&world, "phone", "");
    let other = topic(&world, "phone", "");
    entry(&world, "2026-10-08", "case", &[&one]);
    entry(&world, "2026-10-07", "screen", &[&other]);
    let all = ok(&world, "/stats");
    for (phone, on) in [(&one, "2026-10-08"), (&other, "2026-10-07")] {
        let link = format!("/stats?topic={}", phone.short());
        let count = "phone <span class=\"count\">1</span></a>";
        has(&all, &format!("<a href=\"{link}\">{count}"));
        let chosen = ok(&world, &link);
        has(
            &chosen,
            &format!("<a href=\"{link}\" aria-current=\"true\">{count}"),
        );
        let day = format!("/entries?topic={}&#38;on={on}", phone.short());
        has(&chosen, &format!("<a class=\"l4\" href=\"{day}\""));
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

#[test]
fn stats_count_what_the_store_holds_and_what_was_written_in_the_span() {
    let (world, lantern, atlas) = world_with_atlas();
    fact(&world, &lantern, "relay", "");
    let fuse = fact(&world, &atlas, "fuse", "");
    world.amend(
        &fuse,
        &fact_fields(&atlas, "fuse", "s", ENDED_FACT),
        "The relay.\n",
    );
    world.put("lamp", "x = 1\n", "\n");
    world.usage.size.set(4200);

    let html = ok(&world, "/stats");
    has(&html, "<h2>Holds</h2>");
    has(
        &html,
        "<tr><td>topics</td><td class=\"n\">3</td><td class=\"n\">0</td></tr>",
    );
    has(
        &html,
        "<tr><td>facts</td><td class=\"n\">1</td><td class=\"n\">1</td></tr>",
    );
    has(
        &html,
        "<tr><td>ideas</td><td class=\"n\">0</td><td class=\"n\">0</td></tr>",
    );
    has(&html, "<p class=\"muted\">5 documents · 6 versions · ");
    has(
        &html,
        "<p class=\"muted\">usage log 4.2 KB · 1 document of an unknown kind</p>",
    );
    has(
        &html,
        "<tr><td class=\"mono\">2026-10</td><td class=\"n\">5</td><td class=\"n\">6</td>",
    );
    has(&html, "<tr><td>desk</td><td class=\"n\">6</td>");

    let html = ok(&world, "/stats?topic=atlas");
    has(
        &html,
        "<tr><td>topics</td><td class=\"n\">1</td><td class=\"n\">0</td></tr>",
    );
    has(
        &html,
        "<tr><td>facts</td><td class=\"n\">0</td><td class=\"n\">1</td></tr>",
    );
    lacks(&html, "usage log");

    let html = ok(&world, "/stats?year=2025");
    has(
        &html,
        "<tr><td>facts</td><td class=\"n\">1</td><td class=\"n\">1</td></tr>",
    );
    lacks(&html, "<th>Month</th>");
    lacks(&html, ">2026-10<");
    lacks(&html, "<th>Machine</th>");
}

fn holdings_paragraphs(html: &str) -> usize {
    let (_, rest) = html
        .split_once("<section class=\"holdings\">")
        .expect("the holdings");
    let (section, _) = rest.split_once("</section>").expect("a closed section");
    section.matches("<p").count()
}

#[test]
fn the_totals_count_forked_documents_and_a_store_with_nothing_unfiled_has_no_second_line() {
    let (world, lantern, _) = world_with_atlas();
    let relay = fact(&world, &lantern, "relay", "");
    let html = ok(&world, "/stats");
    lacks(&html, " forked</p>");
    assert_eq!(holdings_paragraphs(&html), 1);

    fork(&world, &relay, &fact_fields(&lantern, "relay", "s", ""));
    let html = ok(&world, "/stats");
    has(&html, "<p class=\"muted\">4 documents · 6 versions · ");
    has(&html, " · 1 forked</p>");
    assert_eq!(holdings_paragraphs(&html), 1);
}
