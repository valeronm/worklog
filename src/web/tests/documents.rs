use super::{forked_into, has, lacks, main_of, ok, stamp};
use crate::app::show;
use crate::app::show::Shown;
use crate::app::testing::{
    DONE, ENDED, fact, fact_fields, fork, topic, topic_fields, world, world_with_atlas,
};
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::version::ReadError;

#[test]
fn a_document_is_its_label_its_marks_and_its_head_with_fields_and_a_rendered_body() {
    let (world, lantern, atlas) = world_with_atlas();
    let relay = fact(&world, &lantern, "relay", "");
    let saved = world.amend_linking(
        &relay,
        &fact_fields(
            &lantern,
            "relay-pin",
            "Pin <four>",
            "former_names = [\"relay\", \"coil\"]\n",
        ),
        "## Why\n\nSee [[atlas]], [[nowhere]] and `[[atlas]]`.\n",
        &[("atlas", Some(&atlas)), ("nowhere", None)],
    );

    let html = ok(&world, &format!("/doc/{relay}"));
    has(&html, "<title>lantern/relay-pin · worklog</title>");
    has(&html, "<h1>lantern/relay-pin</h1>");
    has(
        &html,
        &format!(
            "<p class=\"meta\"><span class=\"kind\">fact</span>\
             <a href=\"/history/{relay}\">history</a></p>"
        ),
    );
    has(
        &html,
        "<p class=\"muted\">formerly lantern/relay, lantern/coil</p>",
    );
    has(
        &html,
        &format!("<p class=\"stamp\">{}</p>", stamp(&world, &saved, "save")),
    );
    has(
        &html,
        &format!(
            "<dl class=\"fields\">\n\
             <dt>name</dt><dd>relay-pin</dd>\n\
             <dt>topic</dt><dd><a href=\"/doc/{lantern}\">lantern</a></dd>\n\
             <dt>created</dt><dd>2026-09-04</dd>\n\
             <dt>confirmed</dt><dd>2026-09-04</dd>\n\
             <dt>summary</dt><dd>Pin &lt;four&gt;</dd>\n\
             <dt>former_names</dt><dd>relay, coil</dd>\n\
             </dl>"
        ),
    );
    has(
        &html,
        &format!(
            "<div class=\"body\"><h2>Why</h2>\n\
             <p>See <a href=\"/doc/{atlas}\">atlas</a>, \
             <span class=\"unresolved\">[[nowhere]]</span> and \
             <code>[[atlas]]</code>.</p>\n</div>"
        ),
    );
    lacks(&html, "class=\"mark\"");
    lacks(&html, "class=\"notice\"");
    assert_eq!(html.matches("<article class=\"head\">").count(), 1);
    assert_eq!(html.matches(" aria-current=").count(), 0);

    for target in [
        "/doc/lantern/relay-pin".to_owned(),
        "/doc/lantern/relay".to_owned(),
        "/doc/lantern%2Frelay-pin/".to_owned(),
        format!("/doc/{}", relay.short()),
    ] {
        assert_eq!(ok(&world, &target), html, "{target}");
    }
}

#[test]
fn a_field_that_refers_to_several_documents_links_each_by_id() {
    let (world, lantern, atlas) = world_with_atlas();
    let absent = DocumentId::from_bytes([0xb8; 16]);
    let desk = world.machine();
    world.amend(
        &atlas,
        &topic_fields(
            "atlas",
            &format!("part_of = [\"{lantern}\", \"{absent}\"]\nuses = [\"{desk}\"]\n"),
        ),
        "\n",
    );
    let html = ok(&world, "/doc/atlas");
    has(
        &html,
        &format!(
            "<dt>part_of</dt><dd><a href=\"/doc/{lantern}\">lantern</a>, \
             <a href=\"/doc/{absent}\">{}</a></dd>\n\
             <dt>uses</dt><dd><a href=\"/doc/{desk}\">desk</a></dd>",
            absent.short()
        ),
    );
    has(&html, "<span class=\"kind\">topic</span>");
    has(&html, "<div class=\"body\"></div>");
}

#[test]
fn a_forked_document_is_marked_and_shows_each_head_under_a_heading_naming_it() {
    let (world, lantern) = world();
    let heads = fork(&world, &lantern, &topic_fields("lantern", ""));
    let html = ok(&world, "/doc/lantern");
    has(
        &html,
        &format!(
            "<a href=\"/history/{lantern}\">history</a> <span class=\"mark\">forked</span></p>"
        ),
    );
    assert_eq!(html.matches("<article class=\"head\">").count(), 2);
    let headings: Vec<String> = (heads.iter())
        .map(|head| {
            format!(
                "<h2 class=\"stamp\"><a href=\"/doc/{head}\">{}</a> · save on \
                 <a href=\"/doc/dededededededededededededededede\">dededede</a> · \
                 <span title=\"2026-09-04T10:00:00+01:00\">2026-09-04T10:00:00+01:00</span></h2>",
                head.short()
            )
        })
        .collect();
    let places: Vec<usize> = (headings.iter())
        .map(|heading| {
            html.find(heading)
                .unwrap_or_else(|| panic!("{heading}\n{html}"))
        })
        .collect();
    assert!(places[0] < places[1], "{html}");
    let Shown::Document(shown) = show::show(&world.deps(), "lantern").expect("a document") else {
        panic!("expected a document");
    };
    let articles: Vec<&str> = html.split("<article class=\"head\">").skip(1).collect();
    for (article, head) in articles.iter().zip(&shown.heads) {
        has(article, &format!("<a href=\"/doc/{}\">", head.version));
        has(
            article,
            &format!("<div class=\"body\"><p>{}</p>\n</div>", head.body.trim()),
        );
    }
    assert_ne!(shown.heads[0].body, shown.heads[1].body);
    lacks(&html, "<p class=\"stamp\">");
    lacks(main_of(&html), "ended");
}

#[test]
fn an_ended_document_shows_its_reason_on_the_head_that_ended_it() {
    let (world, lantern) = world();
    let ended = world.amend(&lantern, &topic_fields("lantern", ENDED), "\n");
    let html = ok(&world, "/doc/lantern");
    has(
        &html,
        &format!(
            "<a href=\"/history/{lantern}\">history</a> \
             <span class=\"mark\">ended retired</span></p>"
        ),
    );
    let on_the_stamp = format!(
        "<p class=\"stamp\">{} <span class=\"mark\">ended retired</span></p>",
        stamp(&world, &ended, "save")
    );
    has(&html, &on_the_stamp);
    has(&html, "<dt>ended</dt><dd>retired</dd>");
    has(&html, "<dt>note</dt><dd>n</dd>");
    lacks(&html, "forked");

    let version = ok(&world, &format!("/doc/{ended}"));
    has(&version, &on_the_stamp);
    assert_eq!(version.matches("class=\"mark\"").count(), 1);
}

#[test]
fn a_fork_is_marked_ended_only_when_the_document_is() {
    let (world, lantern, atlas) = world_with_atlas();
    let [open, closed] = forked_into(&world, &lantern, "lantern", ["", ENDED]);
    let html = ok(&world, "/doc/lantern");
    has(
        &html,
        &format!(
            "<a href=\"/history/{lantern}\">history</a> <span class=\"mark\">forked</span></p>"
        ),
    );
    assert_eq!(html.matches("ended retired</span>").count(), 1);
    let articles: Vec<&str> = html.split("<article class=\"head\">").skip(1).collect();
    for article in articles {
        let ended = article.contains(&format!("<a href=\"/doc/{closed}\">"));
        assert_ne!(
            ended,
            article.contains(&format!("<a href=\"/doc/{open}\">"))
        );
        assert_eq!(
            article.contains("</span> <span class=\"mark\">ended retired</span></h2>"),
            ended,
            "{article}"
        );
    }

    forked_into(&world, &atlas, "atlas", [ENDED, ENDED]);
    let html = ok(&world, "/doc/atlas");
    has(
        &html,
        &format!(
            "<a href=\"/history/{atlas}\">history</a> <span class=\"mark\">forked</span> \
             <span class=\"mark\">ended retired</span></p>"
        ),
    );
    assert_eq!(html.matches("ended retired</span>").count(), 3);
}

#[test]
fn a_document_of_a_kind_this_worklog_does_not_know_is_shown_as_the_text_it_holds() {
    let (world, lantern) = world();
    let gadget = world.put(
        "gadget",
        &format!("size = 4\ntopic = \"{lantern}\"\nparts = [\"a\", \"b\"]\n"),
        "A *gadget*.\n",
    );
    let html = ok(&world, &format!("/doc/{gadget}"));
    has(&html, &format!("<h1>{}</h1>", gadget.short()));
    has(&html, "<span class=\"kind\">gadget</span>");
    has(
        &html,
        &format!(
            "<dt>size</dt><dd>4</dd>\n<dt>topic</dt><dd>{lantern}</dd>\n\
             <dt>parts</dt><dd>a, b</dd>\n</dl>"
        ),
    );
    has(
        &html,
        "<div class=\"body\"><p>A <em>gadget</em>.</p>\n</div>",
    );
    assert_eq!(
        ok(&world, &format!("/history/{gadget}"))
            .matches("<tr><td")
            .count(),
        1
    );
}

#[test]
fn a_file_that_does_not_read_is_a_note_on_the_document_and_its_history() {
    let (world, lantern) = world();
    let lost = VersionId::of(b"lost");
    world
        .store
        .plant_unreadable(&lantern, lost.clone(), ReadError::Corrupt);
    let note = format!(
        "<p class=\"notice\">version {} does not read: \
         the bytes do not hash to the file&#39;s name</p>",
        lost.short()
    );
    let html = ok(&world, "/doc/lantern");
    has(&html, &note);
    assert_eq!(html.matches("<article class=\"head\">").count(), 1);
    has(&ok(&world, "/history/lantern"), &note);

    let stray = DocumentId::from_bytes([0xc4; 16]);
    world
        .store
        .plant_unreadable(&stray, VersionId::of(b"x"), ReadError::Corrupt);
    let html = ok(&world, &format!("/doc/{stray}"));
    has(&html, &format!("<h1>{}</h1>", stray.short()));
    has(
        &html,
        &format!("<p class=\"meta\"><a href=\"/history/{stray}\">history</a></p>"),
    );
    lacks(&html, "<article");
    has(
        &ok(&world, &format!("/history/{stray}")),
        "<p class=\"muted\">No version reads.</p>",
    );
}

#[test]
fn what_a_body_holds_as_html_or_as_a_script_address_is_never_markup() {
    let (world, lantern) = world();
    world.amend(
        &lantern,
        &topic_fields("lantern", ""),
        "<script>alert(1)</script>\n\nA <b onclick=\"x()\">bold</b> \
         [link](javascript:alert(2)) ![img](data:text/html,x) \
         ![lamp](https://example.org/lamp.png)\n",
    );
    let html = ok(&world, "/doc/lantern");
    let (_, body) = html.split_once("<div class=\"body\">").expect("a body");
    let (body, _) = body.split_once("</div>").expect("a closed body");
    assert_eq!(
        body,
        "&lt;script&gt;alert(1)&lt;/script&gt;\n\
         <p>A &lt;b onclick=\"x()\"&gt;bold&lt;/b&gt; link img \
         <a href=\"https://example.org/lamp.png\">lamp</a></p>\n"
    );
    lacks(&html, "<script");
    lacks(&html, "<img");
    lacks(&html, "javascript:");
    lacks(&html, "data:");
}

#[test]
fn a_link_stored_as_unresolved_links_to_the_document_its_text_names_now() {
    let (world, lantern) = world();
    let phone = topic(&world, "phone", "");
    world.amend_linking(
        &phone,
        &topic_fields("phone", ""),
        "See [[lantern/relay-pin]] and [[atlas]].\n",
        &[("lantern/relay-pin", None), ("atlas", None)],
    );
    let html = ok(&world, &format!("/doc/{phone}"));
    has(
        &html,
        "<p>See <span class=\"unresolved\">[[lantern/relay-pin]]</span> and \
         <span class=\"unresolved\">[[atlas]]</span>.</p>",
    );

    let relay = fact(&world, &lantern, "relay-pin", "");
    let atlas = topic(&world, "atlas", "");
    let html = ok(&world, &format!("/doc/{phone}"));
    has(&html, &format!("See <a href=\"/doc/{relay}\">"));
    has(&html, &format!(" and <a href=\"/doc/{atlas}\">"));
    lacks(&html, "class=\"unresolved\"");
}

#[test]
fn a_link_to_an_ended_document_is_marked_with_the_reason_and_still_leads_to_it() {
    let (world, lantern, atlas) = world_with_atlas();
    let fuse = fact(&world, &lantern, "fuse", DONE);
    let phone = topic(&world, "phone", ENDED);
    world.amend_linking(
        &atlas,
        &topic_fields("atlas", ""),
        "See [[lantern/fuse]], [[lantern]], [[phone]] and [[nowhere]].\n",
        &[
            ("lantern/fuse", Some(&fuse)),
            ("lantern", Some(&lantern)),
            ("phone", None),
            ("nowhere", None),
        ],
    );
    let html = ok(&world, "/doc/atlas");
    has(
        &html,
        &format!(
            "<p>See <a class=\"ended\" href=\"/doc/{fuse}\" title=\"ended done\">\
             lantern/fuse</a>, <a href=\"/doc/{lantern}\">lantern</a>, \
             <a class=\"ended\" href=\"/doc/{phone}\" title=\"ended retired\">phone</a> and \
             <span class=\"unresolved\">[[nowhere]]</span>.</p>"
        ),
    );
    has(&html, "a.ended");
    ok(&world, &format!("/doc/{fuse}"));
}

#[test]
fn a_reason_this_worklog_does_not_know_is_escaped_in_the_title_of_a_link() {
    let (world, lantern, atlas) = world_with_atlas();
    let sold = "ended = \"sold & <gone> \\\"x\\\"\"\nended_on = 2026-10-09\nnote = \"n\"\n";
    world.amend(&atlas, &topic_fields("atlas", sold), "\n");
    world.amend_linking(
        &lantern,
        &topic_fields("lantern", ""),
        "See [[atlas]].\n",
        &[("atlas", Some(&atlas))],
    );
    let html = ok(&world, "/doc/lantern");
    has(
        &html,
        &format!(
            "<p>See <a class=\"ended\" href=\"/doc/{atlas}\" \
             title=\"ended sold &amp; &lt;gone&gt; &quot;x&quot;\">atlas</a>.</p>"
        ),
    );
}
