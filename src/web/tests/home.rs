use super::{has, lacks, main_of, ok};
use crate::app::testing::{
    ENDED, fact, followup, fork, part_of, topic, topic_fields, world, world_with_atlas,
};
use crate::domain::id::DocumentId;

#[test]
fn home_nests_the_unended_topics_under_what_they_are_part_of_with_their_counts() {
    let (world, lantern) = world();
    let atlas = topic(&world, "atlas", &part_of(&[&lantern]));
    let relay = topic(&world, "relay", &part_of(&[&lantern, &atlas]));
    topic(
        &world,
        "globe",
        &format!("{}{}", part_of(&[&lantern]), ENDED),
    );
    world.amend(
        &lantern,
        "name = \"lantern\"\ncreated = 2026-09-04\nsummary = \"A lamp that dims\"\n",
        "\n",
    );
    fact(&world, &lantern, "pin", "");
    fact(&world, &lantern, "bulb", "");
    fact(&world, &lantern, "dimmer", "idea = true\n");
    followup(&world, &[&lantern], "open", "");
    fact(&world, &relay, "coil", "");

    let html = ok(&world, "/");
    let main = main_of(&html);
    has(
        main,
        &format!(
            "<li><a class=\"title\" href=\"/doc/{lantern}\">lantern</a> \
             <span class=\"label\">2 facts</span> <span class=\"label\">1 idea</span> \
             <span class=\"label\">1 open</span>\
             <div class=\"what\">A lamp that dims</div>\
             <ul class=\"topics\">\
             <li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a>\
             <div class=\"what\">s</div>\
             <ul class=\"topics\">\
             <li><a class=\"title\" href=\"/doc/{relay}\">relay</a> \
             <span class=\"label\">1 fact</span><div class=\"what\">s</div></li>\
             </ul>\
             </li>\
             <li><a href=\"/doc/{relay}\">relay</a> \
             <span class=\"muted\">shown above</span></li>\
             </ul>\
             </li>"
        ),
    );
    assert_eq!(main.matches(">relay</a>").count(), 2);
    assert_eq!(main.matches(">atlas</a>").count(), 1);
    assert_eq!(main.matches(">desk</a>").count(), 1);
    lacks(main, "globe");
    for href in [
        "/facts",
        "/ideas",
        "/entries",
        "/followups",
        "/search",
        "/log",
        "/check",
        "/forks",
    ] {
        has(main, &format!("<a href=\"{href}\">"));
    }
}

#[test]
fn home_shows_the_ended_topics_too_when_asked() {
    let (world, lantern) = world();
    let globe = topic(
        &world,
        "globe",
        &format!("{}{}", part_of(&[&lantern]), ENDED),
    );
    let live = main_of(&ok(&world, "/")).to_owned();
    lacks(&live, "globe");
    has(
        &live,
        "<p class=\"summary\"><a href=\"/?ended=1\">ended too</a></p>",
    );

    let html = ok(&world, "/?ended=1");
    let main = main_of(&html);
    has(
        main,
        &format!(
            "<a class=\"title ended\" href=\"/doc/{globe}\" title=\"ended retired\">globe</a>"
        ),
    );
    has(
        main,
        "<p class=\"summary\"><a href=\"/\">unended only</a></p>",
    );
}

#[test]
fn home_has_one_full_line_for_a_topic_however_many_topics_it_is_part_of() {
    let (world, _) = world();
    let mut above: Vec<DocumentId> = Vec::new();
    for layer in ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"] {
        let wholes: Vec<&DocumentId> = above.iter().collect();
        let edges = if wholes.is_empty() {
            String::new()
        } else {
            part_of(&wholes)
        };
        above = ["left", "right"]
            .map(|side| topic(&world, &format!("tier-{layer}-{side}"), &edges))
            .into();
    }

    let html = ok(&world, "/");
    let main = main_of(&html);
    assert_eq!(main.matches("<li><a class=\"title\" ").count(), 26);
    assert_eq!(
        main.matches("<span class=\"muted\">shown above</span>")
            .count(),
        22
    );
    assert_eq!(main.matches("<li>").count(), 48);
    for layer in ["a", "g", "l"] {
        for side in ["left", "right"] {
            let line = format!(">tier-{layer}-{side}</a><div class=\"what\">");
            assert_eq!(main.matches(&line).count(), 1, "{line}");
        }
    }
}

#[test]
fn home_shows_a_ring_of_topics_with_each_member_once() {
    let (world, _) = world();
    let phone = topic(&world, "phone", "");
    let atlas = topic(&world, "atlas", &part_of(&[&phone]));
    world.amend(&phone, &topic_fields("phone", &part_of(&[&atlas])), "\n");

    let html = ok(&world, "/");
    has(
        &html,
        &format!(
            "<li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a>\
             <div class=\"what\">s</div>\
             <ul class=\"topics\">\
             <li><a class=\"title\" href=\"/doc/{phone}\">phone</a>\
             <div class=\"what\">s</div>\
             <ul class=\"topics\">\
             <li><a href=\"/doc/{atlas}\">atlas</a> \
             <span class=\"muted\">shown above</span></li>\
             </ul></li></ul></li>"
        ),
    );
    assert_eq!(html.matches("<li><a class=\"title\" ").count(), 4);
}

#[test]
fn home_marks_a_forked_topic_and_shows_what_a_topic_uses() {
    let (world, lantern, atlas) = world_with_atlas();
    let fields = topic_fields("atlas", &format!("uses = [\"{lantern}\"]\n"));
    world.amend(&atlas, &fields, "\n");
    fork(&world, &atlas, &fields);

    let html = ok(&world, "/");
    has(
        &html,
        &format!(
            "<li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a> \
             <span class=\"mark\">forked</span> \
             <span class=\"muted\">uses <a href=\"/doc/{lantern}\">lantern</a></span>\
             <div class=\"what\">s</div></li>"
        ),
    );
}
