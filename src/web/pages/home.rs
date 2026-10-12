//! The topics, each over its parts.

use std::collections::{BTreeMap, BTreeSet};

use askama::Template;

use super::links::{Line, Link, Nav, NavLink, Place, doc_link, doc_links};
use crate::app::list::TopicRow;
use crate::domain::id::DocumentId;
use crate::render::filed_counts;

pub struct TopicLine {
    pub line: Line,
    pub uses: Vec<Link>,
}

impl From<&TopicRow> for TopicLine {
    fn from(topic: &TopicRow) -> TopicLine {
        TopicLine {
            line: Line {
                counts: filed_counts(topic.fact_count, topic.idea_count, topic.open_count),
                ..Line::from(&topic.row)
            },
            uses: doc_links(&topic.uses),
        }
    }
}

pub enum Step {
    Open,
    Topic(TopicLine),
    // A topic whose line and parts are already on the page.
    Again(Link),
    End,
    Close,
}

struct Walk<'t> {
    rows: BTreeMap<&'t DocumentId, &'t TopicRow>,
    shown: BTreeSet<&'t DocumentId>,
    steps: Vec<Step>,
}

impl<'t> Walk<'t> {
    fn line(&mut self, topic: &'t TopicRow) -> bool {
        self.shown.insert(&topic.row.document);
        self.steps.push(Step::Topic(TopicLine::from(topic)));
        if topic.parts.is_empty() {
            self.steps.push(Step::End);
            return false;
        }
        self.steps.push(Step::Open);
        true
    }

    // A recursive call for each part would overflow the stack on a deep enough chain of parts.
    fn place(&mut self, top: &'t TopicRow) {
        let mut open = Vec::new();
        if self.line(top) {
            open.push(top.parts.iter());
        }
        while let Some(parts) = open.last_mut() {
            let Some(part) = parts.next() else {
                self.steps.push(Step::Close);
                self.steps.push(Step::End);
                open.pop();
                continue;
            };
            if self.shown.contains(&part.document) {
                self.steps.push(Step::Again(doc_link(part)));
            } else if let Some(row) = self.rows.get(&part.document).copied()
                && self.line(row)
            {
                open.push(row.parts.iter());
            }
        }
    }
}

fn nested(topics: &[TopicRow]) -> Vec<Step> {
    if topics.is_empty() {
        return Vec::new();
    }
    let mut walk = Walk {
        rows: (topics.iter())
            .map(|topic| (&topic.row.document, topic))
            .collect(),
        shown: BTreeSet::new(),
        steps: vec![Step::Open],
    };
    for topic in topics {
        if topic.top && !walk.shown.contains(&topic.row.document) {
            walk.place(topic);
        }
    }
    walk.steps.push(Step::Close);
    walk.steps
}

const UNDER_THE_TOPICS: [Place; 8] = [
    Place::Facts,
    Place::Ideas,
    Place::Entries,
    Place::Followups,
    Place::Search,
    Place::Log,
    Place::Check,
    Place::Forks,
];

#[derive(Template)]
#[template(path = "home.html")]
pub struct Home {
    pub nav: Nav,
    pub steps: Vec<Step>,
    pub more: Vec<NavLink>,
}

impl Home {
    pub fn new(topics: &[TopicRow]) -> Home {
        Home {
            nav: Nav::at(Some(Place::Topics)),
            steps: nested(topics),
            more: Place::links(&UNDER_THE_TOPICS, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::row;
    use crate::web::pages::testing::to;

    fn topic(byte: u8, label: &str, part_of: &[&str]) -> TopicRow {
        TopicRow {
            row: row(&DocumentId::from_bytes([byte; 16]), label, "A thing"),
            part_of: part_of.iter().map(|whole| to(0, whole)).collect(),
            uses: Vec::new(),
            parts: Vec::new(),
            top: true,
            fact_count: 0,
            idea_count: 0,
            open_count: 0,
        }
    }

    fn under(mut row: TopicRow, parts: &[(u8, &str)]) -> TopicRow {
        row.parts = parts.iter().map(|(byte, label)| to(*byte, label)).collect();
        row
    }

    fn part(mut row: TopicRow) -> TopicRow {
        row.top = false;
        row
    }

    fn outline(topics: &[TopicRow]) -> String {
        let steps = Home::new(topics).steps;
        let words: Vec<String> = (steps.iter())
            .map(|step| match step {
                Step::Open => "(".to_owned(),
                Step::Topic(topic) => topic.line.label.clone(),
                Step::Again(link) => format!("^{} {}", link.text, link.href),
                Step::End => ";".to_owned(),
                Step::Close => ")".to_owned(),
            })
            .collect();
        words.join(" ")
    }

    #[test]
    fn a_topic_under_two_topics_has_its_line_under_the_first_and_a_link_under_the_other() {
        let topics = [
            under(part(topic(1, "atlas", &["lantern"])), &[(5, "relay")]),
            topic(2, "desk", &[]),
            under(topic(3, "lantern", &[]), &[(1, "atlas"), (4, "phone")]),
            under(part(topic(4, "phone", &["lantern"])), &[(5, "relay")]),
            under(part(topic(5, "relay", &["atlas", "phone"])), &[(6, "coil")]),
            part(topic(6, "coil", &["relay"])),
        ];
        assert_eq!(
            outline(&topics),
            "( desk ; lantern ( atlas ( relay ( coil ; ) ; ) ; \
             phone ( ^relay /doc/05050505050505050505050505050505 ) ; ) ; )"
        );
        assert_eq!(outline(&[]), "");
        assert_eq!(outline(&topics[1..2]), "( desk ; )");
    }

    #[test]
    fn a_ring_shows_each_member_once_and_ends_at_the_one_it_started_from() {
        let ring = [
            under(topic(1, "atlas", &["phone"]), &[(3, "lantern")]),
            topic(2, "desk", &[]),
            under(part(topic(3, "lantern", &["atlas"])), &[(4, "phone")]),
            under(part(topic(4, "phone", &["lantern"])), &[(1, "atlas")]),
            under(topic(5, "relay", &["relay"]), &[(5, "relay")]),
        ];
        assert_eq!(
            outline(&ring),
            "( atlas ( lantern ( phone ( ^atlas /doc/01010101010101010101010101010101 ) ; ) ; ) ; \
             desk ; relay ( ^relay /doc/05050505050505050505050505050505 ) ; )"
        );
    }

    #[test]
    fn topics_sharing_a_name_are_walked_by_their_ids() {
        let topics = [
            under(topic(1, "atlas", &[]), &[(3, "phone")]),
            under(topic(2, "atlas", &[]), &[(4, "relay")]),
            part(topic(3, "phone", &["atlas"])),
            part(topic(4, "relay", &["atlas"])),
        ];
        assert_eq!(
            outline(&topics),
            "( atlas ( phone ; ) ; atlas ( relay ; ) ; )"
        );
    }

    #[test]
    fn a_topic_not_marked_top_and_under_none_is_not_placed_by_the_page() {
        let topics = [topic(1, "atlas", &[]), part(topic(2, "desk", &[]))];
        assert_eq!(outline(&topics), "( atlas ; )");
    }

    #[test]
    fn a_topic_line_links_by_id_and_shows_the_counts_that_are_not_zero() {
        let mut row = topic(7, "lantern", &[]);
        row.fact_count = 1;
        row.open_count = 2;
        row.uses = vec![to(9, "desk lamp")];
        let topic_line = TopicLine::from(&row);
        assert_eq!(topic_line.line.href, format!("/doc/{}", row.row.document));
        assert_eq!(topic_line.line.counts, ["1 fact", "2 open"]);
        assert_eq!(
            topic_line.uses[0].href,
            "/doc/09090909090909090909090909090909"
        );
        assert_eq!(topic_line.uses[0].text, "desk lamp");
        let bare = TopicLine::from(&topic(8, "atlas", &[]));
        assert!(bare.line.counts.is_empty());
    }

    #[test]
    fn a_chain_of_parts_of_any_depth_is_walked() {
        let deep = 20_000_u128;
        let id = |at: u128| DocumentId::from_bytes((at + 1).to_be_bytes());
        let topics: Vec<TopicRow> = (0..deep)
            .map(|at| {
                let mut row = topic(0, "link", &[]);
                row.row.document = id(at);
                row.top = at == 0;
                if at + 1 < deep {
                    row.parts = vec![crate::app::testing::to(&id(at + 1), "link")];
                }
                row
            })
            .collect();
        let home = Home::new(&topics);
        let lines = (home.steps.iter())
            .filter(|step| matches!(step, Step::Topic(_)))
            .count();
        assert_eq!(lines, 20_000);
        assert_eq!(home.steps.len(), 4 * 20_000);
        assert!(matches!(home.steps[home.steps.len() - 2], Step::End));
        let html = home.render().expect("a page");
        assert_eq!(html.matches("<ul class=\"topics\">").count(), 20_000);
    }
}
