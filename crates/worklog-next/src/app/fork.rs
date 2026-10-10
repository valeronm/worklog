use crate::app::draft::{names, refuse_open};
use crate::app::lookup::{Lookup, unreadable};
use crate::app::{Deps, DraftRef, Failure};
use crate::domain::document::State;
use crate::domain::draft::Draft;
use crate::domain::schema::{KindOf, step};

/// Opens a draft holding the first head's fields and body, with every head as a parent.
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
    let first = heads[0].clone();
    let kind = KindOf::of(&first.envelope.kind)?;
    let mut draft = Draft::of(&first);
    draft.parents = heads.iter().map(|head| head.id.clone()).collect();
    draft.fields = names(&lookup, &step::shown(&first.fields), kind)?;
    let location = deps.drafts.write(&draft)?;
    Ok(DraftRef {
        document: id,
        location,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::save::save;
    use crate::app::testing::{TOPIC, fork, refused, topic, world};
    use crate::domain::ports::{Drafts, Store};
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
        let first = world.store.document(&lantern).unwrap().heads()[0].clone();
        assert_eq!(draft.body, first.body);
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
    fn the_draft_carries_the_first_heads_fields_with_references_as_names() {
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
        let first = held.heads()[0].clone();
        let summary = first.fields["summary"].as_str().unwrap().to_owned();
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
