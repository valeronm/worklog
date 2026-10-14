use super::{has, lacks, ok, page};
use crate::app::testing::{fact, fact_fields, head, topic_fields, world};
use crate::app::{Deps, Failure, Stored};
use crate::domain::ports::StoreError;
use crate::domain::testing::FailingStore;
use crate::web::{failed, respond};

#[test]
fn what_a_page_prints_from_the_store_is_escaped() {
    let (world, lantern) = world();
    world.put(
        "fact",
        &fact_fields(&lantern, "sharp", "<script>alert(1)</script> & 'more'", ""),
        "\n",
    );
    world.amend(
        &lantern,
        "name = \"lantern\"\ncreated = 2026-09-04\nsummary = \"<b>A lamp</b>\"\n",
        "\n",
    );
    let html = ok(&world, "/facts");
    lacks(&html, "<script>");
    has(&html, "&lt;script&gt;alert(1)&lt;/script&gt; &amp; 'more'");
    let home = ok(&world, "/");
    lacks(&home, "<b>");
    has(&home, "&lt;b&gt;A lamp&lt;/b&gt;");
}

#[test]
fn a_failure_is_a_page_with_its_text_under_the_status_it_maps_to() {
    let (world, lantern) = world();
    fact(&world, &lantern, "relay-pin", "");

    let refused = page(&world, "/facts?topic=nowhere");
    assert_eq!(refused.status, 404);
    has(&refused.html, "<h1>404</h1>");
    has(&refused.html, "names no document");
    has(&refused.html, "<nav");
    let not_a_topic = page(&world, "/entries?topic=lantern/relay-pin");
    assert_eq!(not_a_topic.status, 404);
    has(&not_a_topic.html, "is not a topic");
    let neither = page(&world, "/followups?topic=lantern/relay-pin");
    assert_eq!(neither.status, 404);
    has(&neither.html, "neither a topic nor an entry");

    let usage = page(&world, "/facts?topic=not+an+address!");
    assert_eq!(usage.status, 400, "{}", usage.html);
    has(&usage.html, "<h1>400</h1>");

    let unknown = page(&world, "/nowhere/<b>");
    assert_eq!(unknown.status, 404);
    has(&unknown.html, "no page at /nowhere/&#60;b&#62;");
}

#[test]
fn an_address_that_does_not_decode_is_a_bad_request() {
    let (world, _) = world();
    for target in [
        "/facts?topic=%zz",
        "/facts?topic=%4",
        "/facts?%",
        "/%zz",
        "/facts%",
        "/facts?topic=%ff",
        "/%c3%28",
    ] {
        let page = page(&world, target);
        assert_eq!(page.status, 400, "{target}");
        has(&page.html, "<h1>400</h1>");
    }
}

#[test]
fn no_address_makes_a_request_fail_to_be_answered() {
    let (world, _) = world();
    let long = format!("/{}", "a".repeat(100_000));
    let long_query = format!("/facts?topic={}", "a".repeat(100_000));
    for (target, status) in [
        (long.as_str(), 400),
        (long_query.as_str(), 400),
        ("", 400),
        ("*", 400),
        ("facts", 400),
        ("http://desk/facts", 400),
        ("/../../etc/passwd", 404),
        ("/facts/..", 404),
        ("/%2e%2e/facts", 404),
        ("/facts%00", 404),
        ("/\0", 404),
        ("/facts?topic=%00", 400),
        ("/facts?topic=..", 400),
        ("/é", 404),
        ("/?", 200),
        ("//", 200),
        ("/facts/", 200),
        ("/facts?", 200),
        ("/facts?&&=&topic", 200),
    ] {
        let shown = &target[..target.len().min(40)];
        assert_eq!(page(&world, target).status, status, "{shown:?}");
    }
}

#[test]
fn only_a_get_and_a_head_are_answered() {
    let (world, lantern) = world();
    let deps = world.deps();
    for target in [
        "/".to_owned(),
        format!("/doc/{lantern}"),
        "/nowhere".to_owned(),
    ] {
        let got = respond(&deps, "GET", &target);
        let head = respond(&deps, "HEAD", &target);
        assert_eq!((head.status, head.html), (got.status, got.html), "{target}");
        for method in ["POST", "PUT", "DELETE", "get", "<b>"] {
            let page = respond(&deps, method, &target);
            assert_eq!(page.status, 405, "{method} {target}");
            has(&page.html, "<h1>405</h1>");
            lacks(&page.html, "<b>");
        }
    }
}

#[test]
fn a_target_that_names_nothing_or_the_wrong_thing_is_the_use_case_s_failure() {
    let (world, lantern) = world();
    let second = world.amend(&lantern, &topic_fields("lantern", ""), "second\n");
    for (target, status, text) in [
        ("/doc/nowhere".to_owned(), 404, "nowhere: names no document"),
        ("/doc/ffff".to_owned(), 404, "ffff: names no document"),
        ("/history/nowhere".to_owned(), 404, "names no document"),
        (
            format!("/doc/b3-{}", "f".repeat(64)),
            404,
            "names no document",
        ),
        (
            "/doc/Not%20An%20Address!".to_owned(),
            400,
            "Not An Address!",
        ),
        ("/doc/..".to_owned(), 400, ".."),
        ("/doc/%00".to_owned(), 400, "`\0` names no document"),
        ("/history/a//b".to_owned(), 400, "a//b"),
        ("/doc/Not+A+Version".to_owned(), 400, "Not+A+Version"),
        (
            "/topic/lantern".to_owned(),
            404,
            "no page at /topic/lantern",
        ),
        (format!("/topic/{lantern}"), 404, "no page at /topic/"),
        (format!("/version/{second}"), 404, "no page at /version/"),
        (
            format!("/history/{second}"),
            400,
            "names a version; a history is of a document",
        ),
        ("/doc".to_owned(), 404, "no page at /doc"),
        ("/doc/".to_owned(), 404, "no page at /doc"),
        ("/version/".to_owned(), 404, "no page at /version"),
        ("/history".to_owned(), 404, "no page at /history"),
        ("/docs/lantern".to_owned(), 404, "no page at /docs/lantern"),
    ] {
        let page = page(&world, &target);
        assert_eq!(page.status, status, "{target}: {}", page.html);
        has(&page.html, &format!("<h1>{status}</h1>"));
        has(&page.html, text);
    }
}

#[test]
fn a_store_that_fails_is_a_500_page_with_the_failure_s_text() {
    let (world, lantern) = world();
    let deps = Deps {
        store: Stored::new(&FailingStore),
        ..world.deps()
    };
    for target in [
        "/".to_owned(),
        "/facts".to_owned(),
        "/ideas".to_owned(),
        "/entries".to_owned(),
        "/followups".to_owned(),
        format!("/doc/{lantern}"),
        format!("/doc/{}", head(&world, &lantern).id),
        "/history/lantern".to_owned(),
        "/search?q=pin".to_owned(),
        "/log".to_owned(),
        "/check".to_owned(),
        "/forks".to_owned(),
    ] {
        let page = respond(&deps, "GET", &target);
        assert_eq!(page.status, 500, "{target}");
        has(&page.html, "<h1>500</h1>");
        has(&page.html, "<p class=\"problem\">memory:store: gone</p>");
    }
}

#[test]
fn each_page_has_one_heading_and_marks_where_it_is_in_the_navigation() {
    let (world, _) = world();
    for (target, heading, in_the_navigation) in [
        ("/", "Topics", true),
        ("/facts", "Facts", true),
        ("/ideas", "Ideas", true),
        ("/entries", "Entries", true),
        ("/followups", "Follow-ups", true),
        ("/log", "Log", true),
        ("/check", "Check", true),
        ("/forks", "Forks", true),
        ("/nowhere", "404", false),
    ] {
        let html = page(&world, target).html;
        assert_eq!(html.matches("<h1>").count(), 1, "{target}");
        has(&html, &format!("<h1>{heading}</h1>"));
        assert_eq!(
            html.matches(" aria-current=\"page\"").count(),
            usize::from(in_the_navigation),
            "{target}"
        );
        if in_the_navigation {
            has(
                &html,
                &format!("<a href=\"{target}\" aria-current=\"page\">{heading}</a>"),
            );
        }
        has(
            &html,
            "<input type=\"search\" name=\"q\" aria-label=\"Search\"",
        );
        has(&html, "<form action=\"/search\" method=\"get\">");
    }
    assert_eq!(ok(&world, "/search?q=pin").matches("<h1>").count(), 1);
    assert_eq!(ok(&world, "/doc/lantern").matches("<h1>").count(), 1);
}

#[test]
fn a_failure_met_before_any_page_is_a_page_too() {
    let page = failed(Failure::Refused("the <store> is gone".to_owned()));
    assert_eq!(page.status, 404);
    has(&page.html, "the &#60;store&#62; is gone");
    let page = failed(Failure::Store(StoreError::io("desk", "gone")));
    assert_eq!(page.status, 500);
    has(&page.html, "desk: gone");
}
