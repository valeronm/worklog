use super::{forked_into, has, lacks, main_of, ok};
use crate::app::show;
use crate::app::testing::{
    DONE, ENDED, ENDED_FACT, REMOVED, World, entry, fact, fact_fields, followup, fork, head,
    topic_fields, world_with_atlas,
};
use crate::domain::id::{DocumentId, VersionId};
use crate::domain::version::ReadError;

struct Ended {
    world: World,
    lantern: DocumentId,
    atlas: DocumentId,
    relay: DocumentId,
    fuse: DocumentId,
}

fn anchor(class: &str, document: &DocumentId, reason: &str, label: &str) -> String {
    format!("<a class=\"{class}\" href=\"/doc/{document}\" title=\"ended {reason}\">{label}</a>")
}

fn ended() -> Ended {
    let (world, lantern, atlas) = world_with_atlas();
    let fuse = fact(&world, &atlas, "fuse", ENDED_FACT);
    let relay = fact(&world, &lantern, "relay-pin", "");
    world.amend_linking(
        &relay,
        &fact_fields(&lantern, "relay-pin", "Fed by [[atlas/fuse]] & <b>", ""),
        "## Under [[atlas/fuse]]\n",
        &[("atlas/fuse", Some(&fuse))],
    );
    let driver = entry(&world, "2026-10-08", "lamp-driver", &[&atlas]);
    followup(
        &world,
        &[&lantern, &atlas],
        "Paint the case",
        &format!("entry = \"{driver}\"\nabout = \"{fuse}\"\n"),
    );
    followup(
        &world,
        &[&lantern, &atlas],
        "Fit the lens",
        &format!("touching = \"{atlas}\"\n"),
    );
    world.amend(
        &lantern,
        &topic_fields("lantern", &format!("uses = [\"{atlas}\"]\n")),
        "\n",
    );
    world.amend(&atlas, &topic_fields("atlas", ENDED), "\n");
    Ended {
        world,
        lantern,
        atlas,
        relay,
        fuse,
    }
}

#[test]
fn a_listing_marks_each_link_to_a_document_that_ended() {
    let Ended {
        world,
        lantern,
        atlas,
        fuse,
        ..
    } = ended();
    let to_atlas = anchor("ended", &atlas, "retired", "atlas");
    let to_fuse = anchor("ended", &fuse, "false", "atlas/fuse");

    let facts = ok(&world, "/facts?topic=atlas&ended=1");
    has(&facts, &format!("1 fact under {to_atlas} · "));
    has(
        &facts,
        &format!(
            "{} <span class=\"what\">s</span> <span class=\"mark\">ended false</span> \
             <span class=\"chips\">{to_atlas}</span>",
            anchor("title ended", &fuse, "false", "atlas/fuse")
        ),
    );

    let followups = ok(&world, "/followups");
    has(
        &followups,
        &format!(
            "<span class=\"chips\"><a href=\"/doc/{lantern}\">lantern</a>{to_atlas}</span>\
             <span class=\"details\">in "
        ),
    );
    has(&followups, &format!(" · about {to_fuse}</span>"));
    has(
        &followups,
        &format!("<span class=\"label\">touching {to_atlas}</span>"),
    );

    let entries = ok(&world, "/entries");
    has(
        &entries,
        &format!("<span class=\"chips\">{to_atlas}</span>"),
    );

    let home = ok(&world, "/");
    has(
        &home,
        &format!("<span class=\"muted\">uses {to_atlas}</span>"),
    );

    let search = ok(&world, "/search?q=fuse&ended=1");
    has(
        &search,
        &anchor("title ended", &fuse, "false", "atlas/fuse"),
    );

    let log = ok(&world, "/log");
    has(&log, &format!("<td>{to_atlas}</td>"));
    has(&log, &format!("<td>{to_fuse}</td>"));
}

#[test]
fn a_link_in_a_summary_is_the_text_it_is_on_a_row_and_on_its_document_s_page() {
    let Ended { world, relay, .. } = ended();
    let summary = "Fed by [[atlas/fuse]] &#38; &#60;b&#62;";
    let facts = ok(&world, "/facts");
    has(&facts, &format!("<span class=\"what\">{summary}</span>"));
    let page = ok(&world, &format!("/doc/{relay}"));
    has(&page, &format!("<dt>summary</dt><dd>{summary}</dd>"));
}

#[test]
fn an_ended_link_in_a_heading_of_a_body_is_marked_and_the_rule_outranks_every_other() {
    let Ended {
        world, relay, fuse, ..
    } = ended();
    let page = ok(&world, &format!("/doc/{relay}"));
    has(
        &page,
        &format!(
            "<div class=\"body\"><h2>Under {}</h2>",
            anchor("ended", &fuse, "false", "atlas/fuse")
        ),
    );
    let rule = (page.lines())
        .find(|line| line.starts_with("a.ended {"))
        .expect("the rule");
    assert_eq!(rule.matches("!important").count(), 2, "{rule}");
    assert_eq!(page.matches("a.ended").count(), 1);
}

#[test]
fn a_document_s_page_marks_a_field_that_refers_to_a_document_that_ended() {
    let Ended {
        world, atlas, fuse, ..
    } = ended();
    let page = ok(&world, &format!("/doc/{fuse}"));
    has(
        &page,
        &format!(
            "<dt>topic</dt><dd>{}</dd>",
            anchor("ended", &atlas, "retired", "atlas")
        ),
    );
}

#[test]
fn a_version_s_page_and_a_check_mark_the_document_that_ended() {
    let Ended { world, atlas, .. } = ended();
    let to_atlas = anchor("ended", &atlas, "retired", "atlas");
    let version = head(&world, &atlas).id;
    let page = ok(&world, &format!("/doc/{version}"));
    has(
        &page,
        &format!(" of {to_atlas} · <a href=\"/history/{atlas}\">history</a>"),
    );

    let lost = VersionId::of(b"lost");
    world
        .store
        .plant_unreadable(&atlas, lost, ReadError::Corrupt);
    let check = ok(&world, "/check");
    has(
        &check,
        &format!(
            "<li>{} <span class=\"problem\">",
            anchor("title ended", &atlas, "retired", "atlas")
        ),
    );
}

#[test]
fn a_stamp_marks_a_machine_whose_topic_ended() {
    let (world, lantern, _) = world_with_atlas();
    let desk = world.machine();
    world.amend(&desk, &topic_fields("desk", ENDED), "\n");
    let to_desk = anchor("ended", &desk, "retired", "desk");
    has(
        &ok(&world, &format!("/doc/{lantern}")),
        &format!(" on {to_desk} · "),
    );
    has(
        &ok(&world, &format!("/history/{lantern}")),
        &format!("<td class=\"mono\">{to_desk}</td>"),
    );
}

#[test]
fn a_page_of_documents_none_of_which_ended_marks_no_link() {
    let (world, lantern, atlas) = world_with_atlas();
    let relay = fact(&world, &lantern, "relay-pin", "");
    world.amend(
        &relay,
        &fact_fields(&lantern, "relay-pin", "Pin <four> & \\\"five\\\" 'six'", ""),
        "\n",
    );
    let driver = entry(&world, "2026-10-08", "lamp-driver", &[&atlas]);
    followup(
        &world,
        &[&lantern],
        "Paint the case",
        &format!("entry = \"{driver}\"\nabout = \"{relay}\"\ntouching = \"{atlas}\"\n"),
    );
    let version = head(&world, &relay).id;
    world
        .store
        .plant_unreadable(&lantern, VersionId::of(b"lost"), ReadError::Corrupt);
    fork(&world, &atlas, &topic_fields("atlas", ""));
    has(
        &ok(&world, "/check"),
        &format!("<li><a class=\"title\" href=\"/doc/{lantern}\">lantern</a> "),
    );
    has(
        &ok(&world, "/forks"),
        &format!("<li><a class=\"title\" href=\"/doc/{atlas}\">atlas</a> "),
    );
    for target in [
        "/".to_owned(),
        "/facts".to_owned(),
        "/facts?topic=lantern".to_owned(),
        "/entries".to_owned(),
        "/followups".to_owned(),
        "/search?q=pin".to_owned(),
        "/log".to_owned(),
        "/check".to_owned(),
        "/forks".to_owned(),
        "/doc/lantern".to_owned(),
        format!("/doc/{relay}"),
        format!("/doc/{version}"),
        format!("/history/{relay}"),
    ] {
        let html = ok(&world, &target);
        let main = main_of(&html);
        for mark in [
            "class=\"ended\"",
            " ended\"",
            "title=\"ended",
            "unresolved",
            "&lt;",
            "&amp;",
        ] {
            lacks(main, mark);
        }
    }
    let (marked, plain) = document_links(&ok(&world, "/facts"));
    assert_eq!(marked, 0);
    assert!(!plain.is_empty());
    let summary = "Pin &#60;four&#62; &#38; &#34;five&#34; &#39;six&#39;";
    has(
        &ok(&world, "/facts"),
        &format!(
            "<a class=\"title\" href=\"/doc/{relay}\">lantern/relay-pin</a> \
             <span class=\"what\">{summary}</span>"
        ),
    );
    has(
        &ok(&world, &format!("/doc/{relay}")),
        &format!("<dt>summary</dt><dd>{summary}</dd>"),
    );
}

fn is_document_id(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

// The tabs of a topic's page lead to the page itself.
fn without_tabs(main: &str) -> String {
    match main.split_once("<p class=\"tabs\">") {
        Some((before, rest)) => {
            let (_, after) = rest.split_once("</p>").expect("closed tabs");
            format!("{before}{after}")
        }
        None => main.to_owned(),
    }
}

fn document_links(html: &str) -> (usize, Vec<String>) {
    let main = without_tabs(main_of(html));
    let (mut marked, mut plain) = (0, Vec::new());
    for rest in main.split("<a ").skip(1) {
        let tag = rest.split('>').next().unwrap_or_default();
        let Some(target) = tag.split("href=\"/doc/").nth(1) else {
            continue;
        };
        let id = target.split(['"', '?']).next().unwrap_or_default();
        if !is_document_id(id) {
            continue;
        }
        let classes = tag
            .split("class=\"")
            .nth(1)
            .and_then(|at| at.split('"').next());
        if classes.is_some_and(|classes| classes.split(' ').any(|class| class == "ended")) {
            marked += 1;
        } else {
            plain.push(tag.to_owned());
        }
    }
    (marked, plain)
}

#[test]
fn every_link_to_a_document_is_marked_where_every_document_has_ended() {
    let (world, lantern, atlas) = world_with_atlas();
    let desk = world.machine();
    let fuse = fact(&world, &atlas, "fuse", ENDED_FACT);
    let dimmer = fact(
        &world,
        &lantern,
        "dimmer",
        &format!("idea = true\n{ENDED_FACT}"),
    );
    let driver = world.put(
        "entry",
        &format!(
            "name = \"lamp-driver\"\ndate = 2026-10-08\nmachine = \"{desk}\"\n\
             topics = [\"{lantern}\", \"{atlas}\"]\nsummary = \"s\"\n{REMOVED}"
        ),
        "See [[atlas/fuse]].\n",
    );
    let painted = followup(
        &world,
        &[&lantern, &atlas],
        "s",
        &format!("entry = \"{driver}\"\nabout = \"{fuse}\"\n{DONE}"),
    );
    let fitted = followup(
        &world,
        &[&lantern],
        "s",
        &format!("touching = \"{lantern}\"\n{DONE}"),
    );
    let claim = world.put(
        "claim",
        &format!("machine = \"{desk}\"\ntopic = \"{lantern}\"\n{REMOVED}"),
        "\n",
    );
    world.amend_linking(
        &lantern,
        &topic_fields(
            "lantern",
            &format!("part_of = [\"{atlas}\"]\nuses = [\"{desk}\"]\n{ENDED}"),
        ),
        "# On [[atlas]]\n\nSee [[atlas/fuse]] and [[desk]].\n",
        &[
            ("atlas", Some(&atlas)),
            ("atlas/fuse", Some(&fuse)),
            ("desk", None),
        ],
    );
    world.amend(&desk, &topic_fields("desk", ENDED), "\n");
    forked_into(&world, &atlas, "atlas", [ENDED, ENDED]);
    world
        .store
        .plant_unreadable(&fuse, VersionId::of(b"lost"), ReadError::Corrupt);

    let documents = [
        &desk, &lantern, &atlas, &fuse, &dimmer, &driver, &painted, &fitted, &claim,
    ];
    let mut targets: Vec<String> = Vec::new();
    for route in ["/", "/facts", "/ideas", "/entries", "/followups"] {
        targets.push(route.to_owned());
        targets.push(format!("{route}?ended=1"));
        targets.push(format!("{route}?topic={lantern}&ended=1"));
    }
    targets.extend(["/search?q=s&ended=1", "/log", "/check", "/forks"].map(str::to_owned));
    for document in documents {
        targets.push(format!("/doc/{document}"));
        targets.push(format!("/history/{document}"));
        let history = show::history(&world.deps(), document.as_str()).unwrap();
        for version in history.versions {
            targets.push(format!("/doc/{}", version.version));
        }
    }
    for topic in [&desk, &lantern, &atlas] {
        for section in ["parts", "facts", "ideas", "followups", "entries"] {
            targets.push(format!("/doc/{topic}?show={section}"));
        }
    }

    let mut marked = 0;
    for target in &targets {
        let (found, plain) = document_links(&ok(&world, target));
        assert_eq!(plain, [""; 0], "{target}");
        marked += found;
    }
    assert!(marked > targets.len(), "{marked} marked links");
}
