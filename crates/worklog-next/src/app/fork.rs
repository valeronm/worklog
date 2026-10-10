use crate::app::draft::{names, refuse_open};
use crate::app::heads::row_head;
use crate::app::lookup::{Lookup, no_head_reads, unreadable};
use crate::app::{Deps, DraftRef, Failure};
use crate::domain::document::State;
use crate::domain::draft::Draft;
use crate::domain::schema::{KindOf, step};

/// Opens a draft with every head as a parent, holding the fields and body of the head the
/// document is shown from. Refuses a document that is not forked, one with a draft open, one
/// holding an unreadable version, and a fork no head of which reads as a record.
pub fn resolve(deps: &Deps, address: &str) -> Result<DraftRef, Failure> {
    let lookup = Lookup::new(deps.store);
    let id = lookup.one(address)?;
    let label = lookup.label(&id)?;
    let document = lookup.document(&id)?;
    let State::Forked(heads) = document.state() else {
        return Err(Failure::at(&label, "is not forked"));
    };
    refuse_open(deps, &id, &label)?;
    if !document.unreadable().is_empty() {
        return Err(unreadable(&label));
    }
    let Some((shown, _)) = row_head(&document) else {
        return Err(no_head_reads(&label));
    };
    let kind = KindOf::of(&shown.envelope.kind)?;
    let mut draft = Draft::of(shown);
    draft.parents = heads.iter().map(|head| head.id.clone()).collect();
    draft.fields = names(&lookup, &step::shown(&shown.fields), kind)?;
    let location = deps.drafts.write(&draft)?;
    Ok(DraftRef {
        document: id,
        location,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::draft::drafts;
    use crate::app::save::save;
    use crate::app::show::{Shown, show};
    use crate::app::testing::{ENDED, TOPIC, World, fork, found, head, refused, topic, world};
    use crate::domain::id::{DocumentId, VersionId};
    use crate::domain::ports::{Drafts, Store};
    use crate::domain::schema::address::Found;
    use crate::domain::testing::after;
    use crate::domain::version::ReadError;

    #[test]
    fn resolving_opens_a_draft_of_every_head_and_saving_it_makes_the_document_live() {
        let (world, lantern) = world();
        let heads = fork(&world, &lantern, &format!("name = \"lantern\"\n{TOPIC}"));
        let deps = world.deps();
        let opened = resolve(&deps, "lantern").unwrap();
        assert_eq!(opened.document, lantern);
        assert_eq!(opened.location, world.drafts.location(&lantern));
        let draft = world.drafts.read(&lantern).unwrap().expect("a draft");
        assert_eq!(draft.parents, heads);
        let document = world.store.document(&lantern).unwrap();
        let (shown, _) = row_head(&document).unwrap();
        assert_eq!(shown.id, heads[1]);
        assert_eq!(draft.body, shown.body);
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            "name = \"lantern\"\nsummary = \"s\"\n"
        );

        let text = refused(resolve(&deps, "lantern"));
        assert!(text.contains("a draft of lantern is open"), "{text}");

        let written = save(&deps, "lantern").unwrap();
        let document = world.store.document(&lantern).unwrap();
        assert_eq!(document.heads().len(), 1);
        assert_eq!(document.heads()[0].id, written.version);
        assert_eq!(document.heads()[0].envelope.change, "resolve");
    }

    #[test]
    fn a_live_document_is_not_resolved() {
        let (world, _) = world();
        let text = refused(resolve(&world.deps(), "lantern"));
        assert!(text.contains("is not forked"), "{text}");
        assert!(world.drafts.list().unwrap().is_empty());
    }

    #[test]
    fn a_fork_holding_an_unreadable_version_is_not_resolved() {
        let (world, lantern) = world();
        fork(&world, &lantern, &format!("name = \"lantern\"\n{TOPIC}"));
        let planted = after(
            &[&world.store.document(&lantern).unwrap().heads()[0].clone()],
            "x = 1",
            "\n",
        )
        .id;
        world
            .store
            .plant_unreadable(&lantern, planted, ReadError::Corrupt);
        let text = refused(resolve(&world.deps(), "lantern"));
        assert!(
            text.contains("holds a version this worklog cannot read"),
            "{text}"
        );
    }

    #[test]
    fn the_draft_carries_the_shown_heads_fields_with_references_as_names() {
        let (world, lantern) = world();
        let relay = world.put(
            "fact",
            &format!(
                "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"s\"\n"
            ),
            "\n",
        );
        let root = world.store.document(&relay).unwrap().heads()[0].clone();
        for summary in ["left", "right"] {
            let fields = format!(
                "name = \"relay-pin\"\ntopic = \"{lantern}\"\ncreated = 2026-09-04\n\
                 confirmed = 2026-09-04\nsummary = \"{summary}\"\n"
            );
            world.store.put(&after(&[&root], &fields, summary)).unwrap();
        }
        let held = world.store.document(&relay).unwrap();
        let (shown, _) = row_head(&held).unwrap();
        assert_eq!(shown.id, held.heads()[1].id);
        let summary = shown.fields["summary"].as_str().unwrap().to_owned();
        resolve(&world.deps(), "lantern/relay-pin").unwrap();
        let draft = world.drafts.read(&relay).unwrap().expect("a draft");
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            format!(
                "name = \"relay-pin\"\ntopic = \"lantern\"\nconfirmed = 2026-09-04\nsummary = \"{summary}\"\n"
            )
        );
        assert_eq!(draft.body, summary);
    }

    #[test]
    fn the_draft_is_filled_from_the_unended_head_and_listed_under_the_name_shown() {
        let (world, lantern) = world();
        let deps = world.deps();
        let root = head(&world, &lantern);
        let live = after(&[&root], &format!("name = \"lamp\"\n{TOPIC}"), "live\n");
        let ended = (0..64)
            .map(|round| {
                let fields = format!("name = \"lantern\"\n{TOPIC}{ENDED}");
                after(&[&root], &fields, &format!("ended {round}\n"))
            })
            .find(|ended| ended.id < live.id)
            .unwrap();
        world.store.put(&live).unwrap();
        world.store.put(&ended).unwrap();
        let heads: Vec<VersionId> = (world.store.document(&lantern).unwrap().heads())
            .iter()
            .map(|head| head.id.clone())
            .collect();
        assert_eq!(heads, [ended.id.clone(), live.id.clone()]);

        resolve(&deps, lantern.as_str()).unwrap();
        let draft = world.drafts.read(&lantern).unwrap().expect("a draft");
        assert_eq!(draft.parents, heads);
        assert_eq!(draft.body, "live\n");
        assert_eq!(
            toml::to_string(&draft.fields).unwrap(),
            "name = \"lamp\"\nsummary = \"s\"\n"
        );
        let Shown::Document(shown) = show(&deps, lantern.as_str()).unwrap() else {
            panic!("expected a document");
        };
        assert_eq!(shown.label, "lamp");
        let listed = drafts(&deps).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, shown.label);
    }

    #[test]
    fn a_fork_whose_first_head_does_not_read_as_a_record_resolves_and_saves() {
        let (world, lantern) = world();
        let deps = world.deps();
        let root = head(&world, &lantern);
        let fields = "name = \"lantern\"\ncreated = 2026-08-01\nsummary = \"kept\"\n";
        let live = after(&[&root], fields, "live\n");
        let unread = (0..64)
            .map(|round| after(&[&root], "name = \"lantern\"\n", &format!("{round}\n")))
            .find(|unread| unread.id < live.id)
            .unwrap();
        world.store.put(&live).unwrap();
        world.store.put(&unread).unwrap();
        let document = world.store.document(&lantern).unwrap();
        assert_eq!(document.heads()[0].id, unread.id);

        resolve(&deps, lantern.as_str()).unwrap();
        let draft = world.drafts.read(&lantern).unwrap().expect("a draft");
        assert_eq!(draft.body, "live\n");
        let written = save(&deps, lantern.as_str()).unwrap();
        let document = world.store.document(&lantern).unwrap();
        let [only] = document.heads()[..] else {
            panic!("expected one head");
        };
        assert_eq!(only.id, written.version);
        assert_eq!(only.envelope.parents.len(), 2);
        assert_eq!(only.fields.to_string(), fields);
    }

    #[test]
    fn a_fork_no_head_of_which_reads_as_a_record_is_not_resolved_or_saved() {
        let (world, _) = world();
        let deps = world.deps();
        let atlas = topic(&world, "atlas", "");
        let heads = fork(&world, &atlas, "name = \"atlas\"\n");
        let wanted = format!(
            "{}: no head of this fork reads; a newer worklog may be needed",
            atlas.short()
        );
        assert_eq!(refused(resolve(&deps, atlas.as_str())), wanted);
        assert!(world.drafts.list().unwrap().is_empty());
        let found = crate::app::check::check(&deps).unwrap();
        let on_atlas: Vec<&str> = (found.problems.iter())
            .filter(|finding| finding.label == atlas.short())
            .map(|finding| finding.what.as_str())
            .collect();
        assert_eq!(on_atlas.len(), 2, "{:?}", found.problems);
        assert_eq!(found.fork_count, 1);

        let held = world.store.document(&atlas).unwrap();
        let mut draft = Draft::of(held.heads()[0]);
        draft.parents = heads;
        world.drafts.write(&draft).unwrap();
        let text = refused(save(&deps, atlas.as_str()));
        assert!(
            text.contains("no head of this fork reads; a newer worklog may be needed"),
            "{text}"
        );
        assert_eq!(world.store.document(&atlas).unwrap().heads().len(), 2);
    }

    fn named(name: &str, former: &str, rest: &str) -> String {
        format!("name = \"{name}\"\nformer_names = [{former}]\n{TOPIC}{rest}")
    }

    fn resolved_as(world: &World, heads: [&str; 2], name: &str) -> (DocumentId, Vec<String>) {
        let deps = world.deps();
        let lantern = crate::app::testing::topic(world, "lantern", "");
        let root = head(world, &lantern);
        for (fields, body) in heads.iter().zip(["one\n", "two\n"]) {
            world.store.put(&after(&[&root], fields, body)).unwrap();
        }
        resolve(&deps, lantern.as_str()).unwrap();
        let mut draft = world.drafts.read(&lantern).unwrap().expect("a draft");
        draft
            .fields
            .insert("name".to_owned(), toml::Value::from(name));
        world.drafts.write(&draft).unwrap();
        save(&deps, lantern.as_str()).unwrap();
        let stored = crate::app::testing::record(world, &lantern);
        let (current, former) = stored.content.naming().unwrap();
        assert_eq!(current.as_str(), name);
        let former = former.iter().map(ToString::to_string).collect();
        (lantern, former)
    }

    fn desk_world() -> World {
        let world = World::new();
        topic(&world, "desk", "");
        world
    }

    const RENAMED: &str = "\"lantern\", \"lamp\"";

    #[test]
    fn a_resolve_keeps_the_former_names_of_every_head() {
        let world = desk_world();
        let heads = [&named("lantern", "", ""), &named("lamptwo", RENAMED, "")];
        let (lantern, former) = resolved_as(&world, heads.map(String::as_str), "lamptwo");
        assert_eq!(former, ["lantern", "lamp"]);
        for address in ["lamp", "lantern", "lamptwo"] {
            assert_eq!(
                found(&world, address),
                Found::One(lantern.clone()),
                "{address}"
            );
        }
    }

    #[test]
    fn a_resolve_under_a_new_name_keeps_what_every_head_was_called() {
        let world = desk_world();
        let heads = [&named("lantern", "", ""), &named("lamptwo", RENAMED, "")];
        let (lantern, mut former) = resolved_as(&world, heads.map(String::as_str), "beacon");
        former.sort();
        assert_eq!(former, ["lamp", "lamptwo", "lantern"]);
        for address in ["lamp", "lantern", "lamptwo", "beacon"] {
            assert_eq!(
                found(&world, address),
                Found::One(lantern.clone()),
                "{address}"
            );
        }
    }

    #[test]
    fn a_resolve_of_heads_named_alike_changes_no_former_name() {
        let world = desk_world();
        let heads = [
            &named("lamptwo", RENAMED, ""),
            &named("lamptwo", RENAMED, ""),
        ];
        let (_, former) = resolved_as(&world, heads.map(String::as_str), "lamptwo");
        assert_eq!(former, ["lantern", "lamp"]);
    }

    #[test]
    fn a_resolve_keeps_the_names_of_an_ended_head() {
        let world = desk_world();
        let heads = [
            &named("lantern", "\"wick\"", ENDED),
            &named("lamptwo", "\"lamp\"", ""),
        ];
        let (lantern, former) = resolved_as(&world, heads.map(String::as_str), "lamptwo");
        assert_eq!(former, ["lamp", "wick", "lantern"]);
        assert_eq!(found(&world, "wick"), Found::One(lantern));
    }

    #[test]
    fn a_rename_of_a_live_document_keeps_the_one_name_it_had() {
        let (world, lantern) = world();
        let deps = world.deps();
        crate::app::draft::checkout(&deps, "lantern").unwrap();
        let mut draft = world.drafts.read(&lantern).unwrap().expect("a draft");
        draft
            .fields
            .insert("name".to_owned(), toml::Value::from("lamp"));
        world.drafts.write(&draft).unwrap();
        save(&deps, "lantern").unwrap();
        let stored = crate::app::testing::record(&world, &lantern);
        let (current, former) = stored.content.naming().unwrap();
        assert_eq!(current.as_str(), "lamp");
        assert_eq!(former.len(), 1);
        assert_eq!(former[0].as_str(), "lantern");
    }

    #[test]
    fn a_live_document_is_not_forked_whatever_else_holds() {
        let (world, _) = world();
        let deps = world.deps();
        let phone = topic(&world, "phone", "");
        crate::app::draft::checkout(&deps, "phone").unwrap();
        let text = refused(resolve(&deps, "phone"));
        assert!(text.contains("is not forked"), "{text}");
        let planted = after(
            &[&world.store.document(&phone).unwrap().heads()[0].clone()],
            "x = 1",
            "\n",
        )
        .id;
        world
            .store
            .plant_unreadable(&phone, planted, ReadError::Corrupt);
        let text = refused(resolve(&deps, "phone"));
        assert!(text.contains("is not forked"), "{text}");
    }
}
