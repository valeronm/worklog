use super::*;

const LAMP: &str = "2026-09/2026-09-04-lamp";

fn unused(planned: &Planned) -> Vec<String> {
    planned.unused.iter().map(Key::to_string).collect()
}

#[test]
fn a_placeholder_no_open_question_offers_is_listed_with_the_unused_decisions() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &["atlas"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.entry(LAMP, "Wired the lamp", &["relay"], desk(DAY_1));
    let pin = old.fact("lantern/pin", "Pin 5", &[], desk(DAY_1));
    old.removed(&pin, "Wrong", desk(DAY_2));
    let store = read(&old);
    let decisions = answers(&format!(
        "[tags]\nrelay = \"lantern\"\ntypo = \"?\"\n\
         [topics.lantern]\nuses = [\"atlas\", \"?\"]\npart_of = []\n\
         [documents.\"entry/{LAMP}\"]\ntopics = [\"?\"]\n\
         [documents.\"fact/lantern/pin\"]\nended = \"false\"\nended_by = \"?\"\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(planned.questions, []);
    assert_eq!(
        unused(&planned),
        [
            "tags.typo".to_owned(),
            "topics.lantern.uses[\"\"]".to_owned(),
            format!("documents.\"entry/{LAMP}\".topics"),
            "documents.\"fact/lantern/pin\".ended_by".to_owned(),
        ]
    );
    assert!(matches!(
        super::super::plan(&store, &decisions),
        Outcome::Open(_)
    ));
}

#[test]
fn a_placeholder_an_open_question_offers_is_no_unused_decision() {
    let old = with_desk();
    old.topic("lantern", "A lamp", &["atlas"], desk(DAY_1));
    old.topic("atlas", "A book of maps", &[], desk(DAY_1));
    old.entry(LAMP, "Wired the lamp", &["relay", "pin"], desk(DAY_1));
    let store = read(&old);
    let decisions = answers(&format!(
        "[tags]\nrelay = \"?\"\n\
         [topics.lantern]\nuses = [\"?\"]\n\
         [documents.\"entry/{LAMP}\"]\ntags.pin = \"?\"\n"
    ));
    let planned = plan(&store, &decisions);

    assert_eq!(opens(&planned).len(), 3, "{:?}", planned.questions);
    assert_eq!(unused(&planned), [""; 0]);
}
