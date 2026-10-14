use super::{has, lacks, ok, page};
use crate::app::testing::{ENDED_FACT, fact, fact_fields, topic, world, world_with_atlas};

const FORM: &str = "<form action=\"/search\" method=\"get\">\
                    <input type=\"search\" name=\"q\" value=\"";

fn form(term: &str, topic: &str, regex: bool, ended: bool) -> String {
    let checked = |on: bool| if on { " checked" } else { "" };
    format!(
        "{FORM}{term}\" autofocus> \
         <input class=\"narrow\" type=\"search\" name=\"topic\" placeholder=\"Topic\" \
         value=\"{topic}\"> \
         <label><input type=\"checkbox\" name=\"regex\" value=\"1\"{}> pattern</label> \
         <label><input type=\"checkbox\" name=\"ended\" value=\"1\"{}> ended too</label> \
         <button>Find</button></form>",
        checked(regex),
        checked(ended)
    )
}

#[test]
fn a_search_with_no_term_is_the_form_alone() {
    let (world, lantern) = world();
    fact(&world, &lantern, "relay-pin", "");
    for target in [
        "/search",
        "/search/",
        "/search?q=",
        "/search?topic=lantern&q=",
        "/search?q=+",
        "/search?q=%20%09+&regex=1",
    ] {
        let html = ok(&world, target);
        has(&html, "<h1>Search</h1>");
        has(&html, FORM);
        lacks(&html, "<ul class=\"items\">");
        lacks(&html, "match");
        assert_eq!(html.matches(" aria-current=").count(), 0);
    }
    has(&ok(&world, "/search"), &form("", "", false, false));
    has(
        &ok(&world, "/search?topic=lantern&q=&ended=1"),
        &form("", "lantern", false, true),
    );
}

#[test]
fn a_search_shows_each_hit_s_row_over_the_lines_that_matched() {
    let (world, lantern, atlas) = world_with_atlas();
    let relay = fact(&world, &lantern, "relay-pin", "");
    let coil = world.put(
        "fact",
        &fact_fields(&atlas, "coil", "Wound by hand", ""),
        "relay one\n  relay <two>\nrelay 3\nrelay 4\nrelay 5\nrelay 6\nrelay 7\nother\n",
    );
    fact(&world, &atlas, "old-relay", ENDED_FACT);
    let named = topic(&world, "relay", "");

    let html = ok(&world, "/search?q=RELAY");
    has(&html, &form("RELAY", "", false, false));
    has(
        &html,
        "<input type=\"search\" name=\"q\" aria-label=\"Search\" placeholder=\"Search\" \
         value=\"RELAY\">",
    );
    has(&html, "<p class=\"summary\">3 documents match</p>");
    has(
        &html,
        &format!(
            "<li><span class=\"label\">fact</span> \
             <a class=\"title\" href=\"/doc/{relay}\">lantern/relay-pin</a> \
             <span class=\"chips\"><a href=\"/doc/{lantern}\">lantern</a></span>\
             <span class=\"what\">s</span>\
             <span class=\"details\">The relay.</span></li>"
        ),
    );
    has(
        &html,
        &format!(
            "<a class=\"title\" href=\"/doc/{coil}\">atlas/coil</a> \
             <span class=\"chips\"><a href=\"/doc/{atlas}\">atlas</a></span>\
             <span class=\"what\">Wound by hand</span>\
             <span class=\"details\">relay one</span>\
             <span class=\"details\">relay &#60;two&#62;</span>\
             <span class=\"details\">relay 3</span>\
             <span class=\"details\">… 4 more</span></li>"
        ),
    );
    has(
        &html,
        &format!(
            "<li><span class=\"label\">topic</span> \
             <a class=\"title\" href=\"/doc/{named}\">relay</a>\
             <span class=\"what\">s</span></li>"
        ),
    );
    lacks(&html, "old-relay");

    let narrowed = ok(&world, "/search?q=relay&topic=atlas&ended=1");
    has(&narrowed, &form("relay", "atlas", false, true));
    has(&narrowed, "<p class=\"summary\">2 documents match</p>");
    has(
        &narrowed,
        "atlas/old-relay</a> \
         <span class=\"mark\">ended false</span><span class=\"what\">s</span>",
    );
    lacks(&narrowed, "lantern/relay-pin");
    has(&narrowed, &format!("href=\"/doc/{coil}\">atlas/coil</a>"));
    lacks(&narrowed, "<span class=\"chips\">");

    let pattern = ok(&world, "/search?q=%5Erelay+%5B36%5D%24&regex=1");
    has(&pattern, &form("^relay [36]$", "", true, false));
    let plain = ok(&world, "/search?q=%5Erelay+%5B36%5D%24&regex=on");
    has(&plain, &form("^relay [36]$", "", false, false));
    has(&plain, "<p class=\"summary\">0 documents match</p>");
    has(&pattern, "<p class=\"summary\">1 document matches</p>");
    has(
        &pattern,
        "<span class=\"details\">relay 3</span><span class=\"details\">relay 6</span></li>",
    );

    let none = ok(&world, "/search?q=%3Cb%3E%22");
    has(&none, &form("&#60;b&#62;&#34;", "", false, false));
    has(&none, "<p class=\"summary\">0 documents match</p>");
    lacks(&none, "<ul class=\"items\">");
    lacks(&none, "<b>");
}

#[test]
fn a_hit_s_summary_line_is_rendered_as_its_row_is_and_its_body_lines_are_text() {
    let (world, lantern) = world();
    let pin = world.put(
        "fact",
        &fact_fields(&lantern, "pin", "  The `relay` <b> [[lantern]]", ""),
        "a `relay` line\n",
    );
    let html = ok(&world, "/search?q=relay");
    has(
        &html,
        &format!(
            "<a class=\"title\" href=\"/doc/{pin}\">lantern/pin</a> \
             <span class=\"chips\"><a href=\"/doc/{lantern}\">lantern</a></span>\
             <span class=\"what\">The <code>relay</code> &lt;b&gt; [[lantern]]</span>\
             <span class=\"details\">The <code>relay</code> &lt;b&gt; [[lantern]]</span>\
             <span class=\"details\">a `relay` line</span></li>"
        ),
    );
}

#[test]
fn a_search_the_use_case_fails_keeps_its_form_under_the_failure_s_status() {
    let (world, lantern) = world();
    fact(&world, &lantern, "relay-pin", "");
    for (target, status, shown, text) in [
        (
            "/search?q=(&regex=1",
            400,
            form("(", "", true, false),
            "bad pattern: ",
        ),
        (
            "/search?q=%3Cb%3E(&regex=1",
            400,
            form("&#60;b&#62;(", "", true, false),
            "bad pattern: regex parse error:\n    &#60;b&#62;(\n",
        ),
        (
            "/search?q=[a-z]{60000}&regex=1",
            400,
            form("[a-z]{60000}", "", true, false),
            "bad pattern: ",
        ),
        (
            "/search?q=pin&topic=nowhere",
            404,
            form("pin", "nowhere", false, false),
            "nowhere: names no document",
        ),
        (
            "/search?q=pin&topic=lantern/relay-pin",
            404,
            form("pin", "lantern/relay-pin", false, false),
            "lantern/relay-pin: is not a topic",
        ),
    ] {
        let page = page(&world, target);
        assert_eq!(page.status, status, "{target}: {}", page.html);
        has(&page.html, "<h1>Search</h1>");
        has(&page.html, &shown);
        has(&page.html, &format!("<p class=\"problem\">{text}"));
        lacks(&page.html, "<ul class=\"items\">");
    }
    assert_eq!(ok(&world, "/search?q=(").matches("<li>").count(), 0);
}
