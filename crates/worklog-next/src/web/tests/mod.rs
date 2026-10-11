mod check;
mod documents;
mod home;
mod listings;
mod requests;
mod search;
mod topics;
mod versions;

use crate::app::testing::{NOW, World, head, topic_fields, written_at};
use crate::domain::id::{DocumentId, VersionId};
use crate::web::{Page, respond};

pub fn page(world: &World, target: &str) -> Page {
    respond(&world.deps(), "GET", target)
}

pub fn ok(world: &World, target: &str) -> String {
    let page = page(world, target);
    assert_eq!(page.status, 200, "{target}: {}", page.html);
    page.html
}

pub fn has(html: &str, fragment: &str) {
    assert!(html.contains(fragment), "no `{fragment}` in:\n{html}");
}

pub fn lacks(html: &str, fragment: &str) {
    assert!(!html.contains(fragment), "`{fragment}` in:\n{html}");
}

pub fn main_of(html: &str) -> &str {
    let (_, rest) = html.split_once("<main>").expect("a main element");
    rest.split_once("</main>").expect("a closed main element").0
}

pub fn stamp(world: &World, version: &VersionId, change: &str) -> String {
    format!(
        "<a href=\"/doc/{version}\">{}</a> · {change} on \
         <a href=\"/doc/{}\">desk</a> · \
         <span title=\"{NOW}\">2026-10-09T18:22:41.118+01:00</span>",
        version.short(),
        world.machine()
    )
}

pub fn forked_into(world: &World, id: &DocumentId, name: &str, rests: [&str; 2]) -> [VersionId; 2] {
    let root = head(world, id);
    let mut hour = 9;
    rests.map(|rest| {
        hour += 1;
        let stamp = format!("2026-10-09T{hour}:00:00+01:00");
        written_at(world, &root, &topic_fields(name, rest), &stamp, "\n", None).id
    })
}
