use super::{forked_into, has, lacks, main_of, ok, page, stamp};
use crate::app::testing::{
    ENDED, NOW, fact, fact_fields, fork, head, topic, topic_fields, world, world_with_atlas,
    written_at,
};
use crate::domain::id::VersionId;

#[test]
fn a_history_is_a_row_a_version_newest_first_each_linked() {
    let (world, lantern) = world();
    let root = head(&world, &lantern).id;
    let second = world.amend(&lantern, &topic_fields("lantern", ""), "second\n");
    let third = world.amend(&lantern, &topic_fields("lantern", ENDED), "third\n");

    let html = ok(&world, &format!("/history/{lantern}"));
    has(&html, "<title>History of lantern · worklog</title>");
    has(&html, "<h1>History</h1>");
    has(
        &html,
        &format!(
            "<p class=\"meta\"><span class=\"kind\">of</span>\
             <a class=\"ended\" href=\"/doc/{lantern}\" title=\"ended retired\">lantern</a></p>"
        ),
    );
    let row = |version: &VersionId, head: &str, change: &str| {
        format!(
            "<tr><td class=\"mono\"><a href=\"/doc/{version}\">{}</a>{head}</td>\
             <td class=\"mono\" title=\"{NOW}\">\
             2026-10-09T18:22:41.118+01:00</td>\
             <td class=\"mono\"><a href=\"/doc/{}\">desk</a></td>\
             <td class=\"mono\">{change}</td></tr>",
            version.short(),
            world.machine()
        )
    };
    let rows = [
        row(
            &third,
            " <span class=\"label\">head</span>",
            "ended retired",
        ),
        row(&second, "", "save"),
        row(&root, "", "new"),
    ];
    let places: Vec<usize> = (rows.iter())
        .map(|row| html.find(row).unwrap_or_else(|| panic!("{row}\n{html}")))
        .collect();
    assert!(places[0] < places[1] && places[1] < places[2], "{html}");
    assert_eq!(html.matches("<tr><td").count(), 3);
    assert_eq!(ok(&world, "/history/lantern"), html);
    assert_eq!(ok(&world, &format!("/history/{}/", lantern.short())), html);

    let forked = topic(&world, "phone", "");
    fork(&world, &forked, &topic_fields("phone", ""));
    let html = ok(&world, "/history/phone");
    assert_eq!(html.matches("<span class=\"label\">head</span>").count(), 2);
    assert_eq!(html.matches("<tr><td").count(), 3);
}

fn diff_of(html: &str) -> Vec<&str> {
    (html.split("<pre class=\"diff\">").skip(1))
        .map(|rest| rest.split_once("</pre>").expect("a closed diff").0)
        .collect()
}

#[test]
fn a_version_is_shown_with_its_parent_linked_and_what_it_changed() {
    let (world, lantern) = world();
    let root = head(&world, &lantern).id;
    let second = world.amend(&lantern, &topic_fields("lantern", ""), "second <b>\n");

    let html = ok(&world, &format!("/doc/{second}"));
    has(
        &html,
        &format!("<title>{} of lantern · worklog</title>", second.short()),
    );
    has(&html, "<h1>lantern</h1>");
    has(
        &html,
        &format!(
            "<p class=\"meta\"><span class=\"kind\">version</span>{} of \
             <a href=\"/doc/{lantern}\">lantern</a> · \
             <a href=\"/history/{lantern}\">history</a> <span class=\"label\">head</span></p>",
            second.short()
        ),
    );
    has(
        &html,
        &format!(
            "<p class=\"parents\">against <a href=\"/doc/{root}\">{}</a></p>",
            root.short()
        ),
    );
    assert_eq!(
        diff_of(&html),
        ["<span class=\"hunk\">@@ -3,4 +3,4 @@</span>\
          <span class=\"context\"> created = 2026-09-04</span>\
          <span class=\"context\"> summary = &#34;s&#34;</span>\
          <span class=\"context\"> +++</span>\
          <span class=\"removed\">-</span>\
          <span class=\"added\">+second &#60;b&#62;</span>"]
    );
    has(
        &html,
        &format!("<p class=\"stamp\">{}</p>", stamp(&world, &second, "save")),
    );
    has(&html, "<dt>name</dt><dd>lantern</dd>");
    has(&html, "<div class=\"body\"><p>second &lt;b&gt;</p>\n</div>");
    for target in [
        format!("/doc/{}", second.short()),
        format!("/doc/{}", &second.as_str()[3..14]),
        format!("/doc/{second}"),
        format!("/doc/{}", second.short()),
    ] {
        assert_eq!(ok(&world, &target), html, "{target}");
    }

    let old = ok(&world, &format!("/doc/{root}"));
    lacks(&old, "<span class=\"label\">head</span>");
    has(&old, "<p class=\"parents\">against nothing</p>");
    assert_eq!(
        diff_of(&old),
        ["<span class=\"hunk\">@@ -0,0 +1,6 @@</span>\
          <span class=\"added\">++++</span>\
          <span class=\"added\">+name = &#34;lantern&#34;</span>\
          <span class=\"added\">+created = 2026-09-04</span>\
          <span class=\"added\">+summary = &#34;s&#34;</span>\
          <span class=\"added\">++++</span>\
          <span class=\"added\">+</span>"]
    );
}

#[test]
fn a_version_with_two_parents_is_compared_with_each() {
    let (world, lantern) = world();
    let heads = fork(&world, &lantern, &topic_fields("lantern", ""));
    let joined = world.amend(&lantern, &topic_fields("lantern", ""), "left\n");

    let html = ok(&world, &format!("/doc/{joined}"));
    let against: Vec<usize> = (heads.iter())
        .map(|head| {
            let link = format!(
                "<p class=\"parents\">against <a href=\"/doc/{head}\">{}</a></p>",
                head.short()
            );
            html.find(&link).unwrap_or_else(|| panic!("{link}\n{html}"))
        })
        .collect();
    assert_ne!(against[0], against[1]);
    assert_eq!(html.matches("<p class=\"parents\">").count(), 2);
    assert_eq!(html.matches("<p class=\"muted\">no changes</p>").count(), 1);
    assert_eq!(
        diff_of(&html),
        ["<span class=\"hunk\">@@ -3,4 +3,4 @@</span>\
          <span class=\"context\"> created = 2026-09-04</span>\
          <span class=\"context\"> summary = &#34;s&#34;</span>\
          <span class=\"context\"> +++</span>\
          <span class=\"removed\">-right</span>\
          <span class=\"added\">+left</span>"]
    );
}

#[test]
fn a_prefix_several_versions_share_is_refused() {
    let (world, lantern) = world();
    let mut ids = Vec::new();
    for round in 0..24 {
        ids.push(world.amend(
            &lantern,
            &topic_fields("lantern", ""),
            &format!("{round}\n"),
        ));
    }
    let digit = |id: &VersionId| id.as_str()[3..4].to_owned();
    let shared = (ids.iter())
        .map(digit)
        .filter(|first| first != "0")
        .find(|first| ids.iter().filter(|id| digit(id) == *first).count() > 1)
        .expect("two versions share a first digit");
    let page = page(&world, &format!("/doc/{shared}"));
    assert_eq!(page.status, 404, "{}", page.html);
    has(&page.html, "is held by several versions");
}

#[test]
fn a_log_is_a_row_a_version_newest_first_with_its_document_linked_when_it_has_a_row() {
    let (world, lantern) = world();
    let desk = world.machine();
    let relay = fact(&world, &lantern, "relay-pin", "");
    let gadget = world.put("gadget", "size = 4\n", "\n");
    let root = head(&world, &lantern);
    let ended = written_at(
        &world,
        &root,
        &topic_fields("lantern", ENDED),
        "2026-10-10T09:00:00+01:00",
        "\n",
        None,
    );

    let html = ok(&world, "/log");
    has(&html, "<h1>Log</h1>");
    has(&html, "<p class=\"summary\">5 newest versions</p>");
    let row = |version: &VersionId, written: &str, change: &str, document: &str| {
        format!(
            "<tr><td class=\"mono\"><a href=\"/doc/{version}\">{}</a></td>\
             <td class=\"mono\" {written}</td>\
             <td class=\"mono\"><a href=\"/doc/{desk}\">desk</a></td>\
             <td class=\"mono\">{change}</td><td>{document}</td></tr>",
            version.short()
        )
    };
    let now = &format!("title=\"{NOW}\">2026-10-09T18:22:41.118+01:00");
    let newest = row(
        &ended.id,
        "title=\"2026-10-10T09:00:00+01:00\">2026-10-10T09:00:00+01:00",
        "ended retired",
        &format!("<a class=\"ended\" href=\"/doc/{lantern}\" title=\"ended retired\">lantern</a>"),
    );
    assert_eq!(html.find(&newest), html.find("<tr><td"), "{newest}\n{html}");
    has(
        &html,
        &row(
            &root.id,
            now,
            "new",
            &format!(
                "<a class=\"ended\" href=\"/doc/{lantern}\" title=\"ended retired\">lantern</a>"
            ),
        ),
    );
    has(
        &html,
        &row(
            &head(&world, &relay).id,
            now,
            "new",
            &format!("<a href=\"/doc/{relay}\">lantern/relay-pin</a>"),
        ),
    );
    has(
        &html,
        &row(
            &head(&world, &gadget).id,
            now,
            "new",
            &format!("<a href=\"/doc/{gadget}\">{}</a>", gadget.short()),
        ),
    );
    assert_eq!(html.matches("<tr><td").count(), 5);
}

#[test]
fn a_log_shows_a_hundred_versions_or_the_number_asked_for_up_to_five_hundred() {
    let (world, lantern) = world();
    for round in 0..104 {
        world.amend(
            &lantern,
            &topic_fields("lantern", ""),
            &format!("{round}\n"),
        );
    }
    let rows = |target: &str| ok(&world, target).matches("<tr><td").count();
    assert_eq!(rows("/log"), 100);
    has(
        &ok(&world, "/log"),
        "<p class=\"summary\">100 newest versions</p>",
    );
    assert_eq!(rows("/log?n=3"), 3);
    assert_eq!(rows("/log?n=1"), 1);
    has(
        &ok(&world, "/log?n=1"),
        "<p class=\"summary\">1 newest version</p>",
    );
    assert_eq!(rows("/log?n=500"), 106);
    assert_eq!(rows("/log/?n=105"), 105);
    for target in [
        "/log?n=0",
        "/log?n=501",
        "/log?n=many",
        "/log?n=",
        "/log?n=-1",
        "/log?n=1.5",
        "/log?n=99999999999999999999999999",
    ] {
        let page = page(&world, target);
        assert_eq!(page.status, 400, "{target}");
        has(&page.html, "n: a number from 1 to 500");
    }
}

#[test]
fn the_forks_are_each_forked_document_over_its_heads() {
    let (world, lantern, atlas) = world_with_atlas();
    has(
        &ok(&world, "/forks"),
        "<p class=\"muted\">No document has more than one head.</p>",
    );

    let relay = fact(&world, &lantern, "relay-pin", "");
    let heads = fork(&world, &relay, &fact_fields(&lantern, "relay-pin", "s", ""));
    let [open, closed] = forked_into(&world, &atlas, "atlas", ["", ENDED]);
    let html = ok(&world, "/forks");
    has(&html, "<h1>Forks</h1>");
    has(&html, "<p class=\"summary\">2 forked documents</p>");
    let line = |version: &VersionId, machine: &str, written: &str, ended: &str| {
        format!(
            "<span class=\"details\"><a href=\"/doc/{version}\">{}</a> on {machine} · \
             <span title=\"{written}\">{written}</span>{ended}</span>",
            version.short()
        )
    };
    let unheld = "<a href=\"/doc/dededededededededededededededede\">dededede</a>";
    let desk = format!("<a href=\"/doc/{}\">desk</a>", world.machine());
    has(
        &html,
        &format!(
            "<li><a class=\"title\" href=\"/doc/{relay}\">lantern/relay-pin</a> \
             <span class=\"what\">s</span>{}{}</li>",
            line(&heads[0], unheld, "2026-09-04T10:00:00+01:00", ""),
            line(&heads[1], unheld, "2026-09-04T10:00:00+01:00", ""),
        ),
    );
    has(
        &html,
        &format!(
            "<li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a> \
             <span class=\"what\">s</span>"
        ),
    );
    has(&html, &line(&open, &desk, "2026-10-09T10:00:00+01:00", ""));
    has(
        &html,
        &line(
            &closed,
            &desk,
            "2026-10-09T11:00:00+01:00",
            " · ended retired",
        ),
    );
    assert_eq!(main_of(&html).matches("<li>").count(), 2);
    lacks(&html, "class=\"mark\"");
}

#[test]
fn a_fork_that_is_ended_is_marked_and_one_no_head_of_which_reads_is_linked_as_a_document() {
    let (world, _, atlas) = world_with_atlas();
    forked_into(&world, &atlas, "atlas", [ENDED, ENDED]);
    let unread = world.put("topic", "size = 4\n", "\n");
    fork(&world, &unread, "size = 4\n");

    let html = ok(&world, "/forks");
    has(
        &html,
        &format!(
            "<li><a class=\"title ended\" href=\"/doc/{atlas}\" title=\"ended retired\">atlas</a> \
             <span class=\"what\">s</span> <span class=\"mark\">ended retired</span>\
             <span class=\"details\">"
        ),
    );
    assert_eq!(html.matches(" · ended retired</span>").count(), 2);
    has(
        &html,
        &format!(
            "<li><a class=\"title\" href=\"/doc/{unread}\">{}</a> \
             <span class=\"what\"></span><span class=\"details\">",
            unread.short()
        ),
    );
    assert_eq!(html.matches("class=\"mark\"").count(), 1);
}
