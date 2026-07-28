//! The spine, end to end.
//!
//! The unit tests check each piece; this checks the claim batch 1 actually
//! makes — that a run can be killed at any point and a new one can work out
//! where it was from the binding and the journal alone (`L-8`, `L-7`, `O-1`).

use std::path::{Path, PathBuf};

use perp_core::atomic::write_atomic;
use perp_core::binding::Binding;
use perp_core::journal::{Journal, Record};
use perp_core::state::{render, replay};
use perp_core::step::StepId;

// clippy's in-test exemption covers `#[test]` functions; these are scaffolding
// beside them, held to the same rule — a panic here is a broken fixture, which
// is what we want to hear about, loudly and immediately.
#[allow(clippy::expect_used)]
fn fixture(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("perp-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs/perpetum")).expect("create fixture");
    std::fs::write(dir.join("docs/perpetum.md"), "# requirements\n").expect("requirements");
    std::fs::write(
        dir.join("docs/perpetum/binding.md"),
        "# Binding\n\n\
         ```perp-binding\n\
         path.requirements = docs/perpetum.md\n\
         out.journal       = docs/perpetum/journal.jsonl\n\
         out.state         = docs/perpetum/state.md\n\
         ```\n",
    )
    .expect("binding");
    dir
}

#[allow(clippy::expect_used)]
fn step(text: &str) -> StepId {
    StepId::parse(text).expect("step id")
}

#[test]
fn a_killed_run_is_recoverable_from_the_journal_alone() {
    let root = fixture("recover");
    let binding = Binding::load(&root).expect("load");
    binding.verify().expect("verify");

    let journal = Journal::at(binding.resolve("out.journal").expect("journal path"));

    // Two steps complete.
    journal.append(&Record::intent(step("c1/b1/s01"), 10, "load the binding")).expect("a");
    journal
        .append(
            &Record::outcome(step("c1/b1/s01"), 20, true, "bound").for_requirements(["L-21"]),
        )
        .expect("b");
    journal.append(&Record::intent(step("c1/b1/s02"), 30, "write the journal")).expect("c");
    journal
        .append(&Record::outcome(step("c1/b1/s02"), 40, true, "appended").for_requirements(["L-3"]))
        .expect("d");

    // The third declares its intent — and the process dies here.
    journal.append(&Record::intent(step("c1/b1/s03"), 50, "render the projection")).expect("e");

    // A new process, holding nothing but the binding and the journal.
    let reopened = Binding::load(&root).expect("reload");
    let records = Journal::at(reopened.resolve("out.journal").expect("path"))
        .read_all()
        .expect("read");
    let projection = replay(&records);

    assert_eq!(projection.done.len(), 2, "two steps closed before the kill");
    assert_eq!(
        projection.open_step,
        Some(step("c1/b1/s03")),
        "the in-flight step is the one to reconcile"
    );
    assert_eq!(projection.requirements_touched, vec!["L-21", "L-3"]);
}

#[test]
fn the_state_file_is_a_projection_and_survives_a_rewrite() {
    let root = fixture("projection");
    let binding = Binding::load(&root).expect("load");
    let journal = Journal::at(binding.resolve("out.journal").expect("path"));
    let state_path = binding.resolve("out.state").expect("path");

    journal.append(&Record::intent(step("c1/b1/s01"), 10, "first")).expect("a");
    journal.append(&Record::outcome(step("c1/b1/s01"), 20, true, "first")).expect("b");
    let long = render(&replay(&journal.read_all().expect("read")), 1_700_000_000);
    write_atomic(&state_path, &long).expect("write");

    journal
        .append(
            &Record::outcome(step("c1/b1/s02"), 30, false, "gate failed")
                .with_detail("error: something specific and verbatim"),
        )
        .expect("c");
    let longer = render(&replay(&journal.read_all().expect("read")), 1_700_000_000);
    write_atomic(&state_path, &longer).expect("rewrite");

    let on_disk = std::fs::read_to_string(&state_path).expect("read state");
    assert_eq!(on_disk, longer, "the file is exactly the projection");
    assert!(on_disk.contains("something specific and verbatim"));
    assert!(
        !on_disk.contains("*(none)*\n\n## Requirements touched\n\n*(none)*\n\n## Steps\n\n*(none closed yet)*"),
        "the shorter earlier version must not be left behind in part"
    );
}

#[test]
fn an_unbound_project_refuses_to_run() {
    let empty = std::env::temp_dir().join(format!("perp-it-unbound-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).expect("create");

    let err = Binding::load(&empty).expect_err("must refuse");
    let text = format!("{err}");
    assert!(text.contains("nothing runs unbound"), "{text}");
}

#[test]
fn a_binding_naming_a_missing_input_stops_the_loop() {
    let root = fixture("missing-input");
    std::fs::remove_file(root.join("docs/perpetum.md")).expect("remove the requirements file");

    let binding = Binding::load(&root).expect("the binding itself still parses");
    let err = binding.verify().expect_err("but verification must fail");
    let text = format!("{err}");
    assert!(text.contains("path.requirements"), "names the key: {text}");
    assert!(text.contains("does not exist"), "says why: {text}");
}

#[test]
fn the_journal_is_never_rewritten() {
    let root = fixture("append-only");
    let binding = Binding::load(&root).expect("load");
    let path: &Path = &binding.resolve("out.journal").expect("path");
    let journal = Journal::at(path);

    let mut previous = String::new();
    for n in 1..=5 {
        let id = format!("c1/b1/s{n:02}");
        journal.append(&Record::intent(step(&id), n as i64, "work")).expect("append");
        let now = std::fs::read_to_string(path).expect("read");
        assert!(now.starts_with(&previous), "record {n} rewrote what was already there");
        previous = now;
    }
    assert_eq!(previous.lines().count(), 5);
}
