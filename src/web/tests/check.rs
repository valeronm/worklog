use super::{has, lacks, ok};
use crate::app::testing::{fork, topic, topic_fields, world};
use crate::domain::id::VersionId;
use crate::domain::version::ReadError;

#[test]
fn a_check_is_a_page_whether_or_not_the_store_has_a_problem() {
    let (world, lantern) = world();
    let clean = ok(&world, "/check");
    has(&clean, "<h1>Check</h1>");
    has(
        &clean,
        "<p class=\"summary\">0 problems, 0 notices, <a href=\"/forks\">0 forks</a></p>",
    );
    has(&clean, "<p>Every rule the store has to keep holds.</p>");
    lacks(&clean, "<h2>");

    let lost = VersionId::of(b"lost");
    world
        .store
        .plant_unreadable(&lantern, lost.clone(), ReadError::Corrupt);
    world.put("gadget", "size = 4\n", "\n");
    let atlas = topic(&world, "atlas", "");
    fork(&world, &atlas, &topic_fields("atlas", ""));
    let html = ok(&world, "/check");
    has(
        &html,
        "<p class=\"summary\">1 problem, 1 notice, <a href=\"/forks\">1 fork</a></p>",
    );
    has(
        &html,
        &format!(
            "<h2>Problems</h2>\n<ul class=\"items\">\n\
             <li><a class=\"title\" href=\"/doc/{lantern}\">lantern</a> \
             <span class=\"problem\">{}: \
             the bytes do not hash to the file&#39;s name</span></li>\n</ul>",
            lost.short()
        ),
    );
    has(
        &html,
        "<h2>Notices</h2>\n<ul class=\"items\">\n\
         <li><span class=\"title\">gadget</span> <span class=\"notice\">\
         1 document of a kind this worklog does not know</span></li>\n</ul>",
    );
    lacks(&html, "Every rule");
}
