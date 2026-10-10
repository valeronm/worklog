use toml::Value;

use crate::app::live::{Open, open};
use crate::app::lookup::Lookup;
use crate::app::save::Admitted;
use crate::app::{Deps, Failure, Written, text};
use crate::domain::id::DocumentId;
use crate::domain::schema::{Content, KindOf};
use crate::domain::version::Fields;

/// Writes nothing unless every document passes its checks.
pub fn move_to(
    deps: &Deps,
    documents: &[&str],
    from: &str,
    to: &str,
) -> Result<Vec<Written>, Failure> {
    let lookup = Lookup::new(deps.store);
    let from_id = lookup.referencable(from, Some(KindOf::Topic))?;
    let to_id = lookup.referencable(to, Some(KindOf::Topic))?;
    if from_id == to_id {
        return Err(Failure::Usage(format!("{from} and {to} are one topic")));
    }
    let mut moves: Vec<(DocumentId, Admitted)> = Vec::new();
    for address in documents {
        let target = open(deps, &lookup, address)?;
        if moves.iter().any(|(listed, _)| *listed == target.id) {
            return Err(Failure::at(&target.label, "is listed twice"));
        }
        target.unended()?;
        let fields = moved(&target, &from_id, &to_id)
            .ok_or_else(|| Failure::at(&target.label, format!("is not under {from}")))?;
        let admitted = target.checked(deps, &lookup, &fields, "move")?;
        moves.push((target.id, admitted));
    }
    moves
        .into_iter()
        .map(|(_, admitted)| admitted.store(deps))
        .collect()
}

fn moved(target: &Open, from: &DocumentId, to: &DocumentId) -> Option<Fields> {
    let mut fields = target.shown();
    let holds = |value: Option<&Value>| value.and_then(Value::as_str) == Some(from.as_str());
    let to_text = || text(to.as_str());
    match &target.record.content {
        Content::Fact(_) if holds(fields.get("topic")) => {
            fields.insert("topic".to_owned(), to_text());
        }
        Content::Entry(_) | Content::Followup(_) => {
            let topics = fields.get("topics")?.as_array()?;
            if !topics.iter().any(|topic| holds(Some(topic))) {
                return None;
            }
            let mut replaced: Vec<Value> = Vec::new();
            for topic in topics {
                let topic = if holds(Some(topic)) {
                    to_text()
                } else {
                    topic.clone()
                };
                if !replaced.contains(&topic) {
                    replaced.push(topic);
                }
            }
            fields.insert("topics".to_owned(), Value::Array(replaced));
            if holds(fields.get("touching")) {
                fields.insert("touching".to_owned(), to_text());
            }
        }
        _ => return None,
    }
    Some(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{World, fact, head, refused, world_with_atlas};
    use crate::domain::id::DocumentId;
    use crate::domain::ports::Store;

    fn followup(
        world: &World,
        topics: &[&DocumentId],
        touching: Option<&DocumentId>,
    ) -> DocumentId {
        let list: Vec<String> = topics.iter().map(|id| format!("\"{id}\"")).collect();
        let touching = touching.map_or_else(String::new, |id| format!("touching = \"{id}\"\n"));
        world.put(
            "followup",
            &format!(
                "created = 2026-09-04\ntopics = [{}]\n{touching}summary = \"s\"\n",
                list.join(", ")
            ),
            "\n",
        )
    }

    #[test]
    fn facts_and_a_followup_move_to_the_other_topic() {
        let (world, lantern, atlas) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let both = followup(&world, &[&lantern, &atlas], Some(&lantern));
        let only = followup(&world, &[&lantern], None);
        let addresses = [
            "lantern/relay-pin",
            "lantern/lamp-pin",
            both.as_str(),
            only.as_str(),
        ];
        let written = move_to(&world.deps(), &addresses, "lantern", "atlas").unwrap();
        assert_eq!(written.len(), 4);

        for (id, written) in [&relay, &lamp].into_iter().zip(&written) {
            assert_eq!(&written.document, id);
            let moved = head(&world, id);
            assert_eq!(moved.id, written.version);
            assert_eq!(moved.envelope.change, "move");
            assert_eq!(moved.fields["topic"].as_str(), Some(atlas.as_str()));
            assert_eq!(moved.fields["confirmed"].to_string(), "2026-09-04");
        }
        let moved = head(&world, &both);
        let topics = moved.fields["topics"].as_array().unwrap();
        assert_eq!(topics.len(), 1);
        assert_eq!(topics[0].as_str(), Some(atlas.as_str()));
        assert_eq!(moved.fields["touching"].as_str(), Some(atlas.as_str()));
        let moved = head(&world, &only);
        assert_eq!(
            moved.fields["topics"].as_array().unwrap()[0].as_str(),
            Some(atlas.as_str())
        );
    }

    #[test]
    fn a_refusal_for_any_document_writes_nothing() {
        let (world, lantern, atlas) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let map = fact(&world, &atlas, "map", "");
        let before = (head(&world, &relay).id, head(&world, &lamp).id);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/relay-pin", "lantern/lamp-pin", "atlas/map"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("is not under lantern"), "{text}");
        assert_eq!(head(&world, &relay).id, before.0);
        assert_eq!(head(&world, &lamp).id, before.1);
        assert_eq!(head(&world, &map).envelope.change, "new");
    }

    #[test]
    fn an_ended_document_is_refused() {
        let (world, lantern, _) = world_with_atlas();
        let old = world.put(
            "fact",
            &format!(
                "name = \"old-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\nended = \"false\"\n\
                 ended_on = 2026-10-09\nnote = \"n\"\n"
            ),
            "\n",
        );
        let text = refused(move_to(&world.deps(), &[old.as_str()], "lantern", "atlas"));
        assert!(text.contains("is ended"), "{text}");
    }

    #[test]
    fn a_topic_moved_to_itself_is_usage() {
        let (world, lantern, _) = world_with_atlas();
        let relay = fact(&world, &lantern, "relay-pin", "");
        let result = move_to(&world.deps(), &[relay.as_str()], "lantern", "lantern");
        assert!(matches!(result, Err(Failure::Usage(_))), "{result:?}");
    }

    fn names_of(world: &World, ids: &[&DocumentId]) -> Vec<crate::domain::id::VersionId> {
        ids.iter().map(|id| head(world, id).id).collect()
    }

    #[test]
    fn a_fact_name_taken_under_the_target_topic_writes_nothing() {
        let (world, lantern, atlas) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        fact(&world, &atlas, "relay-pin", "");
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(
            text.contains("lantern/relay-pin: name: relay-pin is taken by atlas/relay-pin"),
            "{text}"
        );
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_document_listed_twice_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin", "lantern/lamp-pin"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("lantern/lamp-pin: is listed twice"), "{text}");
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_document_with_a_draft_open_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        crate::app::draft::checkout(&world.deps(), "lantern/relay-pin").unwrap();
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(
            text.contains("a draft of lantern/relay-pin is open"),
            "{text}"
        );
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_topic_in_the_list_is_not_under_the_topic() {
        let (world, lantern, atlas) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let before = names_of(&world, &[&lamp, &atlas]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "atlas"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("atlas: is not under lantern"), "{text}");
        assert_eq!(names_of(&world, &[&lamp, &atlas]), before);
    }

    #[test]
    fn a_document_holding_an_unreadable_version_writes_nothing() {
        let (world, lantern, _) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let planted = crate::domain::testing::first(&relay, "fact", "name = \"x\"", "\n").id;
        world
            .store
            .plant_unreadable(&relay, planted, crate::domain::version::ReadError::Corrupt);
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(
            text.contains("lantern/relay-pin: holds a version this worklog cannot read"),
            "{text}"
        );
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }

    #[test]
    fn a_fact_name_a_forked_fact_holds_under_the_target_topic_writes_nothing() {
        let (world, lantern, atlas) = world_with_atlas();
        let lamp = fact(&world, &lantern, "lamp-pin", "");
        let relay = fact(&world, &lantern, "relay-pin", "");
        let forked = fact(&world, &atlas, "relay-pin", "");
        let root = head(&world, &forked);
        for body in ["left\n", "right\n"] {
            let fields = format!(
                "name = \"relay-pin\"\ntopic = \"{atlas}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n"
            );
            let version = crate::domain::testing::after(&[&root], &fields, body);
            world.store.put(&version).unwrap();
        }
        let before = names_of(&world, &[&lamp, &relay]);
        let text = refused(move_to(
            &world.deps(),
            &["lantern/lamp-pin", "lantern/relay-pin"],
            "lantern",
            "atlas",
        ));
        assert!(text.contains("name: relay-pin is taken by"), "{text}");
        assert_eq!(names_of(&world, &[&lamp, &relay]), before);
    }
}
