use std::os::unix::fs::PermissionsExt;

use super::super::place::Staging;
use super::*;

fn refused(scratch: &Scratch) -> Failure {
    let old = rich();
    let store = read(&old);
    let complete = finished(&store);
    match write(&complete, &store, scratch) {
        Ok(_) => panic!("a store was written to {}", scratch.target().display()),
        Err(refused) => refused,
    }
}

fn target_refused(scratch: &Scratch, path: &Path, why: &str) {
    let before = tree(scratch.kept.path());
    let expected = Failure::Target {
        path: path.display().to_string(),
        why: why.to_owned(),
    };
    assert_eq!(refused(scratch), expected);
    assert_eq!(tree(scratch.kept.path()), before);
}

#[test]
fn a_target_that_is_a_file_is_refused_and_left() {
    let scratch = Scratch::new();
    std::fs::write(scratch.target(), "notes").unwrap();
    target_refused(&scratch, &scratch.target(), "is not a directory");
}

#[test]
fn a_target_that_is_a_link_to_an_empty_directory_is_refused_and_left() {
    let scratch = Scratch::new();
    std::fs::create_dir(scratch.path("elsewhere")).unwrap();
    std::os::unix::fs::symlink(scratch.path("elsewhere"), scratch.target()).unwrap();
    let why = "is a symbolic link; give the directory it leads to";
    target_refused(&scratch, &scratch.target(), why);
}

#[test]
fn a_target_that_holds_anything_at_all_is_refused_and_left() {
    let scratch = Scratch::new();
    std::fs::create_dir(scratch.target()).unwrap();
    std::fs::write(scratch.target().join("notes"), "kept").unwrap();
    target_refused(&scratch, &scratch.target(), "is not empty: it holds notes");

    let marked = Scratch::new();
    std::fs::create_dir_all(marked.target().join(".stfolder")).unwrap();
    let why = "is not empty: it holds .stfolder";
    target_refused(&marked, &marked.target(), why);

    let ignoring = Scratch::new();
    std::fs::create_dir(ignoring.target()).unwrap();
    std::fs::write(ignoring.target().join(".stignore"), "*.swp\n").unwrap();
    let why = "is not empty: it holds .stignore";
    target_refused(&ignoring, &ignoring.target(), why);

    let held = Scratch::new();
    let old = rich();
    written(&read(&old), &held);
    let before = tree(held.kept.path());
    let Failure::Target { path, why } = refused(&held) else {
        panic!("a store was written over a store");
    };
    assert_eq!(path, held.target().display().to_string());
    assert!(why.starts_with("is not empty: it holds "), "{why}");
    assert_eq!(tree(held.kept.path()), before);
}

fn in_the_way(scratch: &Scratch) -> String {
    let before = tree(scratch.kept.path());
    let found = refused(scratch);
    assert!(matches!(found, Failure::InTheWay { .. }), "{found:?}");
    assert_eq!(tree(scratch.kept.path()), before);
    let text = found.to_string();
    let opening = format!(
        "{}: is in the way, where the store is staged before it is moved to its place. ",
        scratch.staging().display()
    );
    text.strip_prefix(&opening)
        .unwrap_or_else(|| panic!("{text}"))
        .to_owned()
}

#[test]
fn a_whole_store_left_where_it_was_staged_is_said_to_be_one_with_the_command_that_places_it() {
    let old = rich();
    let scratch = Scratch::new();
    written(&read(&old), &scratch);
    std::fs::rename(scratch.target(), scratch.staging()).unwrap();
    let (staging, target) = (scratch.staging(), scratch.target());
    let (staging, target) = (staging.display(), target.display());
    let whole = "It holds the whole store this conversion writes, as the converter reads it \
                 back, left by a run that was interrupted. To put it in its place: ";

    assert_eq!(
        in_the_way(&scratch),
        format!("{whole}mv {staging} {target}")
    );

    std::fs::create_dir(scratch.target()).unwrap();
    assert_eq!(
        in_the_way(&scratch),
        format!("{whole}rmdir {target} && mv {staging} {target}")
    );
}

#[test]
fn part_of_a_store_left_where_it_was_staged_is_said_to_be_part_and_left() {
    let part = "It does not hold the whole store this conversion writes; a run that was \
                interrupted may have left it. Remove it and run again; it is not removed here";
    let old = rich();
    let scratch = Scratch::new();
    let (composed, _) = written(&read(&old), &scratch);
    let lost = &composed[0].versions[0];
    std::fs::remove_file(scratch.target().join(file_of(lost))).unwrap();
    std::fs::rename(scratch.target(), scratch.staging()).unwrap();
    assert_eq!(in_the_way(&scratch), part);

    let more = Scratch::new();
    written(&read(&old), &more);
    std::fs::write(more.target().join(".stignore"), "*.swp\n").unwrap();
    std::fs::rename(more.target(), more.staging()).unwrap();
    assert_eq!(in_the_way(&more), part);

    let unrelated = Scratch::new();
    std::fs::create_dir(unrelated.staging()).unwrap();
    std::fs::write(unrelated.staging().join("kept"), "kept").unwrap();
    assert_eq!(in_the_way(&unrelated), part);
}

#[test]
fn a_staging_path_that_is_no_directory_is_refused_and_left() {
    let other = "It is no directory, and is left as it is";
    let scratch = Scratch::new();
    std::fs::write(scratch.staging(), "kept").unwrap();
    assert_eq!(in_the_way(&scratch), other);

    let scratch = Scratch::new();
    std::fs::create_dir(scratch.path("elsewhere")).unwrap();
    std::os::unix::fs::symlink(scratch.path("elsewhere"), scratch.staging()).unwrap();
    assert_eq!(in_the_way(&scratch), other);
}

#[test]
fn a_target_with_no_directory_to_stand_in_or_no_name_is_refused() {
    let scratch = Scratch::new();
    std::fs::remove_dir(scratch.at()).unwrap();
    target_refused(&scratch, &scratch.at(), "is not a directory");

    let scratch = Scratch::new();
    let unnamed = scratch.at().join("..");
    let unusable = Place::checked(&unnamed, &scratch.path("store")).err();
    let expected = Failure::Target {
        path: unnamed.display().to_string(),
        why: "names no directory".to_owned(),
    };
    assert_eq!(unusable, Some(expected));
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

fn apart(scratch: &Scratch, target: &Path, old: &Path, why: &str) {
    let before = tree(scratch.kept.path());
    let expected = Failure::Target {
        path: target.display().to_string(),
        why: why.to_owned(),
    };
    assert_eq!(Place::checked(target, old).err(), Some(expected));
    assert_eq!(tree(scratch.kept.path()), before);
}

#[test]
fn a_target_that_is_holds_or_is_inside_the_store_being_converted_is_refused() {
    let never = "the store being converted, which is never written to";
    let scratch = Scratch::new();
    let old = scratch.path("store");
    std::fs::create_dir(old.join("topic")).unwrap();

    apart(&scratch, &old, &old, &format!("is {never}"));
    apart(
        &scratch,
        &old.join("new"),
        &old,
        &format!("is inside {never}"),
    );
    apart(
        &scratch,
        &old.join("topic/new"),
        &old,
        &format!("is inside {never}"),
    );
    let around = scratch.path("at/../store/new");
    apart(&scratch, &around, &old, &format!("is inside {never}"));
    apart(
        &scratch,
        scratch.kept.path(),
        &old,
        &format!("holds {never}"),
    );

    std::os::unix::fs::symlink(&old, scratch.path("at/link")).unwrap();
    apart(
        &scratch,
        &scratch.path("at/link"),
        &old,
        &format!("is {never}"),
    );
    apart(
        &scratch,
        &scratch.path("at/link/new"),
        &old,
        &format!("is inside {never}"),
    );
    apart(
        &scratch,
        &old.join("new"),
        &scratch.path("at/link"),
        &format!("is inside {never}"),
    );
}

#[test]
fn a_target_that_touches_a_kind_directory_the_store_keeps_through_a_link_is_refused() {
    let never = "directory of the store being converted, which is never written to";
    let scratch = Scratch::new();
    let old = scratch.path("store");
    let linked = scratch.path("far/topics");
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(&linked, old.join("topic")).unwrap();
    std::fs::create_dir(old.join("fact")).unwrap();

    apart(
        &scratch,
        &linked.join("new"),
        &old,
        &format!("is inside the topic {never}"),
    );
    apart(&scratch, &linked, &old, &format!("is the topic {never}"));
    apart(
        &scratch,
        &scratch.path("far"),
        &old,
        &format!("holds the topic {never}"),
    );
    let beside = scratch.path("far/topics.partial");
    std::fs::remove_file(old.join("topic")).unwrap();
    std::os::unix::fs::symlink(&beside, old.join("followup")).unwrap();
    std::fs::create_dir(&beside).unwrap();
    let staged = format!(
        "is staged at {} before it is moved to its place, and that is the followup {never}",
        beside.display()
    );
    apart(&scratch, &linked, &old, &staged);
    assert!(Place::checked(&scratch.path("far/new"), &old).is_ok());
}

#[test]
fn a_target_staged_at_or_around_the_store_being_converted_is_refused() {
    let never = "the store being converted, which is never written to";
    let scratch = Scratch::new();
    let old = scratch.staging();
    std::fs::create_dir_all(old.join("topic")).unwrap();
    let staged = format!(
        "is staged at {} before it is moved to its place, and that",
        scratch.staging().display()
    );

    apart(
        &scratch,
        &scratch.target(),
        &old,
        &format!("{staged} is {never}"),
    );
    apart(
        &scratch,
        &scratch.target(),
        &old.join("topic"),
        &format!("{staged} holds {never}"),
    );

    let linked = Scratch::new();
    let old = linked.path("store");
    std::os::unix::fs::symlink(&old, linked.staging()).unwrap();
    let staged = format!(
        "is staged at {} before it is moved to its place, and that",
        linked.staging().display()
    );
    apart(
        &linked,
        &linked.target(),
        &old,
        &format!("{staged} is {never}"),
    );
}

#[test]
fn a_store_being_converted_that_cannot_be_found_stops_the_target_from_being_used() {
    let scratch = Scratch::new();
    let gone = scratch.path("gone");
    let refused = Place::checked(&scratch.target(), &gone).err();
    let Some(Failure::Io { path, .. }) = &refused else {
        panic!("{refused:?}");
    };
    assert_eq!(*path, gone.display().to_string());
}

#[test]
fn a_target_that_is_absent_or_an_empty_directory_takes_the_store() {
    let old = rich();
    let store = read(&old);
    let (plain, empty) = (Scratch::new(), Scratch::new());
    std::fs::create_dir(empty.target()).unwrap();

    written(&store, &plain);
    written(&store, &empty);

    assert_eq!(tree(&empty.target()), tree(&plain.target()));
    for scratch in [&plain, &empty] {
        let beside: Vec<PathBuf> = tree(&scratch.at())
            .into_keys()
            .filter(|path| path.components().count() == 1)
            .collect();
        assert_eq!(beside, [PathBuf::from("new")]);
    }
}

fn placed<'p>(place: &'p Place, scratch: &Scratch) -> Staging<'p> {
    let staging = place.stage(&[]).unwrap();
    std::fs::create_dir(scratch.staging().join("4c")).unwrap();
    std::fs::write(scratch.staging().join("4c/version"), "staged").unwrap();
    staging
}

fn mode(path: &Path) -> u32 {
    std::fs::symlink_metadata(path)
        .unwrap()
        .permissions()
        .mode()
        & 0o7777
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn a_target_filled_while_the_store_was_staged_is_left_with_everything_it_holds() {
    for late in ["notes", ".stignore"] {
        let scratch = Scratch::new();
        std::fs::create_dir(scratch.target()).unwrap();
        let place = scratch.place().unwrap();
        let staging = placed(&place, &scratch);
        std::fs::write(scratch.target().join(late), "late").unwrap();
        let before = tree(&scratch.target());

        let refused = staging.settle().unwrap_err();
        let expected = Failure::Target {
            path: scratch.target().display().to_string(),
            why: format!("is not empty: it holds {late}"),
        };
        assert_eq!(refused, expected);
        assert_eq!(tree(&scratch.target()), before);
        assert!(!scratch.staging().exists());
    }
}

#[test]
fn a_target_made_while_the_store_was_staged_is_left_or_taken_as_one_found_there() {
    let scratch = Scratch::new();
    let place = scratch.place().unwrap();
    let staging = placed(&place, &scratch);
    std::fs::write(scratch.target(), "late").unwrap();
    let refused = staging.settle().unwrap_err();
    let expected = Failure::Target {
        path: scratch.target().display().to_string(),
        why: "is not a directory".to_owned(),
    };
    assert_eq!(refused, expected);
    assert_eq!(std::fs::read(scratch.target()).unwrap(), b"late");
    assert!(!scratch.staging().exists());

    let scratch = Scratch::new();
    let place = scratch.place().unwrap();
    let staging = placed(&place, &scratch);
    std::fs::create_dir(scratch.target()).unwrap();
    staging.settle().unwrap();
    let expected: Vec<PathBuf> = ["4c", "4c/version"].map(PathBuf::from).into();
    let held: Vec<PathBuf> = tree(&scratch.target()).into_keys().collect();
    assert_eq!(held, expected);
    assert!(!scratch.staging().exists());
}

#[test]
fn a_private_target_stays_private() {
    let old = rich();
    let store = read(&old);
    let empty = Scratch::new();
    std::fs::create_dir(empty.target()).unwrap();
    set_mode(&empty.target(), 0o700);

    written(&store, &empty);

    assert_eq!(mode(&empty.target()), 0o700);
}

#[test]
fn a_panic_while_the_store_is_staged_removes_the_staging_directory() {
    let scratch = Scratch::new();
    let place = scratch.place().unwrap();
    let staging = placed(&place, &scratch);
    assert!(scratch.staging().join("4c/version").exists());

    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _held = staging;
        panic!("while staged");
    }));
    assert!(unwound.is_err());
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

#[test]
fn a_version_that_cannot_be_written_fails_the_write_and_leaves_nothing() {
    let old = rich();
    let store = read(&old);
    let scratch = Scratch::new();
    let complete = finished(&store);
    let composed = compose(&complete).unwrap();
    let place = scratch.place().unwrap();
    let staging = place.stage(&composed).unwrap();
    let bucket = composed[0].id.bucket();
    std::fs::write(scratch.staging().join(bucket), "in the way").unwrap();

    let refused = super::super::stored(&composed, staging).unwrap_err();
    let Failure::Io { path, .. } = &refused else {
        panic!("{refused:?}");
    };
    assert_eq!(*path, scratch.staging().display().to_string());
    assert_eq!(tree(&scratch.at()), BTreeMap::new());
}

#[test]
fn a_staging_directory_that_cannot_be_removed_is_named_beside_the_failure() {
    let scratch = Scratch::new();
    let place = scratch.place().unwrap();
    let staging = placed(&place, &scratch);
    let cause = Failure::Unsound(Vec::new());
    set_mode(&scratch.at(), 0o555);

    let left = staging.abandon(cause.clone());
    set_mode(&scratch.at(), 0o755);
    let Failure::Left {
        path, cause: after, ..
    } = &left
    else {
        panic!("{left:?}");
    };
    assert_eq!(*path, scratch.staging().display().to_string());
    assert_eq!(**after, cause);
    let text = left.to_string();
    assert!(text.contains(path.as_str()), "{text}");
    assert!(text.contains("defect of the converter"), "{text}");
    assert!(scratch.staging().exists());
}
