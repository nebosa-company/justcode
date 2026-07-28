//! End to end: the real binary, as a subprocess, against a real repository.
//!
//! Perpetum's Phase D extends end-to-end coverage every five batches, and
//! cycle 1 delivered five with only the spine covered (`N-12`). Unit tests over
//! a library cannot catch an argument parsed wrongly, a path resolved from the
//! wrong root, or an exit code that lies — and the exit code is what an
//! unattended loop actually reads.
//!
//! Each test builds its own fixture repository and runs `perp` in it. Nothing
//! here touches the repository the tests live in.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The binary under test.
///
/// `CARGO_BIN_EXE_perp` is set by cargo for this package's integration tests,
/// which also guarantees the binary is built before they run — the reason this
/// suite lives in `perp` rather than beside the library's own tests.
fn perp() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perp"))
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Output {
    fn ok(&self) -> bool {
        self.code == 0
    }

    fn says(&self, needle: &str) -> bool {
        self.stdout.contains(needle) || self.stderr.contains(needle)
    }
}

fn run(root: &Path, args: &[&str]) -> Output {
    let out = Command::new(perp())
        .args(args)
        .arg("--root")
        .arg(root)
        .stdin(Stdio::null())
        .output()
        .expect("perp should be runnable");
    Output {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("perp-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("docs/perpetum")).expect("dirs");
    std::fs::write(root.join("docs/perpetum.md"), "# requirements\n\n| `L-3` | the journal |\n")
        .expect("requirements");
    std::fs::write(
        root.join("docs/perpetum/binding.md"),
        "# Binding\n\n\
         ```perp-binding\n\
         path.requirements = docs/perpetum.md\n\
         out.journal       = docs/perpetum/journal.jsonl\n\
         out.state         = docs/perpetum/state.md\n\
         ```\n",
    )
    .expect("binding");
    root
}

// ── the binding boundary (L-21) ────────────────────────────────────────────

#[test]
fn an_unbound_directory_exits_non_zero_and_says_why() {
    let root = std::env::temp_dir().join(format!("perp-cli-unbound-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("dir");

    let out = run(&root, &["bind"]);
    assert!(!out.ok(), "an unbound project must not exit 0");
    assert!(out.says("nothing runs unbound"), "stderr: {}", out.stderr);
}

#[test]
fn bind_lists_what_resolved_and_what_is_still_to_be_written() {
    let root = fixture("bind");
    let out = run(&root, &["bind"]);
    assert!(out.ok(), "stderr: {}", out.stderr);
    assert!(out.says("path.requirements"), "{}", out.stdout);
    assert!(out.says("[ok]"), "an input that exists: {}", out.stdout);
    assert!(out.says("[to be written]"), "an output that does not: {}", out.stdout);
}

#[test]
fn a_binding_naming_a_missing_input_fails_the_command_not_just_the_check() {
    let root = fixture("missing-input");
    std::fs::remove_file(root.join("docs/perpetum.md")).expect("remove");
    let out = run(&root, &["bind"]);
    assert!(!out.ok());
    assert!(out.says("path.requirements"), "names the key: {}", out.stderr);
}

// ── the journal and its projection (L-3, L-4, O-1, N-5) ────────────────────

#[test]
fn record_then_state_round_trips_through_two_separate_processes() {
    // `L-8`: nothing lives in memory between these. Two processes, one journal.
    let root = fixture("round-trip");

    assert!(run(&root, &["record", "c1/b1/s01", "intent", "do the thing"]).ok());
    assert!(run(
        &root,
        &["record", "c1/b1/s01", "outcome", "did the thing", "--requirements", "L-3"]
    )
    .ok());

    let state = run(&root, &["state", "--out", "-"]);
    assert!(state.ok(), "stderr: {}", state.stderr);
    assert!(state.says("did the thing"), "{}", state.stdout);
    assert!(state.says("1 done, 0 blocked"), "{}", state.stdout);
    assert!(state.says("`L-3`"), "the requirement is cited: {}", state.stdout);
}

#[test]
fn the_same_journal_renders_the_same_state_twice() {
    // `N-5`: deterministic replay. The date line is the only thing allowed to
    // move, and it does not move within a run.
    let root = fixture("deterministic");
    assert!(run(&root, &["record", "c1/b1/s01", "intent", "one"]).ok());
    assert!(run(&root, &["record", "c1/b1/s01", "outcome", "one"]).ok());

    let first = run(&root, &["state", "--out", "-"]).stdout;
    let second = run(&root, &["state", "--out", "-"]).stdout;
    assert_eq!(first, second, "replay is not deterministic");
}

#[test]
fn a_failed_outcome_carries_its_verbatim_detail_all_the_way_to_the_state_file() {
    let root = fixture("verbatim");
    let error = "error[E0308]: mismatched types\n  --> src/main.rs:4:9";
    assert!(run(&root, &["record", "c1/b1/s02", "intent", "compile"]).ok());
    assert!(run(
        &root,
        &["record", "c1/b1/s02", "outcome", "build failed", "--failed", "--detail", error]
    )
    .ok());

    assert!(run(&root, &["state"]).ok());
    let written = std::fs::read_to_string(root.join("docs/perpetum/state.md")).expect("state file");
    assert!(written.contains("mismatched types"), "{written}");
    assert!(written.contains("0 done, 1 blocked"), "{written}");
}

// ── recovery (L-7, N-1, N-2) ───────────────────────────────────────────────

#[test]
fn resume_reports_nothing_in_flight_for_a_clean_journal() {
    let root = fixture("resume-clean");
    assert!(run(&root, &["record", "c1/b1/s01", "intent", "work"]).ok());
    assert!(run(&root, &["record", "c1/b1/s01", "outcome", "done"]).ok());

    let out = run(&root, &["resume"]);
    assert!(out.ok(), "stderr: {}", out.stderr);
    assert!(out.says("nothing in flight"), "{}", out.stdout);
}

#[test]
fn resume_parks_a_step_that_died_between_intent_and_outcome() {
    // `N-1`/`L-7` across a real process boundary: the first process writes an
    // intent and exits; a second, holding nothing, works out what to do.
    let root = fixture("resume-park");
    assert!(run(&root, &["record", "c1/b1/s03", "intent", "post the release"]).ok());

    let out = run(&root, &["resume"]);
    assert!(!out.ok(), "parking must not exit 0 — an unattended caller reads this");
    assert!(out.says("park c1/b1/s03"), "{}", out.stdout);
    assert!(out.says("not safe to repeat"), "{}{}", out.stdout, out.stderr);
}

// ── the id check (V-9) ─────────────────────────────────────────────────────

#[test]
fn check_ids_fails_when_a_document_invents_a_requirement() {
    let root = fixture("stray-id");
    std::fs::write(root.join("docs/plan.md"), "Batch 1: `L-3`, then `L-99`.\n").expect("write");

    let out = run(&root, &["check", "ids"]);
    assert!(!out.ok(), "a stray id must fail the command");
    assert!(out.says("L-99"), "{}{}", out.stdout, out.stderr);

    std::fs::write(root.join("docs/plan.md"), "Batch 1: `L-3`.\n").expect("rewrite");
    assert!(run(&root, &["check", "ids"]).ok(), "and citing a real id must pass");
}

// ── the shape of the CLI itself (N-3) ──────────────────────────────────────

#[test]
fn the_binary_stands_alone() {
    // `N-3`: no daemon, no server, no side files. Everything above ran by
    // invoking one executable, and this asserts the last piece — it answers
    // without a project at all.
    let out = Command::new(perp()).arg("version").output().expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("perp "), "{text}");

    let help = Command::new(perp()).output().expect("run");
    assert!(help.status.success(), "no arguments should print usage, not fail");
    assert!(String::from_utf8_lossy(&help.stdout).contains("usage:"));
}

#[test]
fn an_unknown_command_fails_loudly() {
    let out = Command::new(perp()).arg("frobnicate").output().expect("run");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown command"));
}
