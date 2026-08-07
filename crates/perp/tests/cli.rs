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
    std::fs::create_dir_all(root.join(".harness")).expect("dirs");
    std::fs::write(root.join(".harness/perpetum.md"), "# requirements\n\n| `L-3` | the journal |\n")
        .expect("requirements");
    std::fs::write(
        root.join(".harness/binding.md"),
        "# Binding\n\n\
         ```perp-binding\n\
         path.requirements = .harness/perpetum.md\n\
         out.journal       = .harness/journal.jsonl\n\
         out.state         = .harness/state.md\n\
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
    std::fs::remove_file(root.join(".harness/perpetum.md")).expect("remove");
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
    let written = std::fs::read_to_string(root.join(".harness/state.md")).expect("state file");
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
    // `docs/` is the project's own, and the fixture stopped creating it when the
    // harness moved out of it — which is the point of the move.
    std::fs::create_dir_all(root.join("docs")).expect("docs");
    std::fs::write(root.join("docs/plan.md"), "Batch 1: `L-3`, then `L-99`.\n").expect("write");

    let out = run(&root, &["check", "ids"]);
    assert!(!out.ok(), "a stray id must fail the command");
    assert!(out.says("L-99"), "{}{}", out.stdout, out.stderr);

    std::fs::write(root.join("docs/plan.md"), "Batch 1: `L-3`.\n").expect("rewrite");
    assert!(run(&root, &["check", "ids"]).ok(), "and citing a real id must pass");
}

#[test]
fn check_ids_fails_rather_than_pass_on_finding_nothing_to_check() {
    // `V-11`: no `docs/` at all — the fixture never creates one unless a test
    // asks for it, which is exactly the state a workspace is in before anyone
    // has written a document. A checker that reports `documents: 0` and exits
    // 0 has not checked anything; it must fail loudly instead.
    let root = fixture("no-documents");
    let out = run(&root, &["check", "ids"]);
    assert!(!out.ok(), "zero documents must not be a passing check");
    assert!(out.says("no documents"), "{}{}", out.stdout, out.stderr);
}

#[test]
fn check_ids_fails_on_an_empty_docs_directory_too() {
    // The directory existing but holding nothing is the same emptiness by a
    // different path — an empty `docs/` must fail exactly like a missing one.
    let root = fixture("empty-docs");
    std::fs::create_dir_all(root.join("docs")).expect("docs");
    let out = run(&root, &["check", "ids"]);
    assert!(!out.ok(), "an empty docs directory must not be a passing check");
    assert!(out.says("no documents"), "{}{}", out.stdout, out.stderr);
}

// ── the router (M-2, M-4, M-5) ─────────────────────────────────────────────

fn with_links(tag: &str, block: &str) -> PathBuf {
    let root = fixture(tag);
    std::fs::write(
        root.join(".harness/links.md"),
        format!("# Links\n\n```perp-links\n{block}\n```\n"),
    )
    .expect("links");
    let binding = root.join(".harness/binding.md");
    let text = std::fs::read_to_string(&binding).expect("read binding");
    std::fs::write(
        &binding,
        text.replace(
            "out.journal",
            "path.links       = .harness/links.md\nout.journal",
        ),
    )
    .expect("rebind");
    root
}

const MIXED: &str = "\
link.here.kind = lmstudio
link.here.base_url = http://localhost:1234
link.here.model = small
link.cloud.kind = deepseek
link.cloud.base_url = https://api.deepseek.com
link.cloud.model = deepseek-v4-flash
role.coder = cloud, here
role.embedder = cloud";

#[test]
fn links_resolves_a_role_and_says_it_contacted_nothing() {
    let root = with_links("links-resolve", MIXED);
    let out = run(&root, &["links", "--role", "coder"]);
    assert!(out.ok(), "stderr: {}", out.stderr);
    assert!(out.says("cloud → here"), "the chain, in order: {}", out.stdout);
    assert!(out.says("resolves to: cloud"), "{}", out.stdout);
    assert!(out.says("health assumed"), "it must not imply it pinged anything");
}

#[test]
fn local_only_skips_the_cloud_link_rather_than_preferring_it() {
    // `M-4`/`M-5`: the whole point is that nothing crosses the privacy
    // boundary quietly.
    let root = with_links("links-local", MIXED);
    let out = run(&root, &["links", "--role", "coder", "--local-only"]);
    assert!(out.ok(), "stderr: {}", out.stderr);
    assert!(out.says("resolves to: here"), "{}", out.stdout);
    assert!(out.says("(skipped: cloud)"), "{}", out.stdout);
}

#[test]
fn a_role_with_no_local_option_fails_local_only_instead_of_falling_back() {
    let root = with_links("links-gap", MIXED);
    let out = run(&root, &["links", "--local-only"]);
    assert!(!out.ok(), "embedder is cloud-only, so this run cannot be local-only");
    assert!(out.says("no local option for: embedder"), "{}", out.stdout);
    assert!(out.says("no cloud link is substituted"), "{}{}", out.stdout, out.stderr);
}

// ── a call, and its bill (M-9, M-10, M-11) ─────────────────────────────────

/// A tiny OpenAI-compatible server, for as many requests as it is given
/// answers. Real socket, real curl, real journal — the whole chain.
fn fake_link(answers: Vec<(u16, String)>) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();

    std::thread::spawn(move || {
        for (status, body) in answers {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream);
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            if length > 0 {
                use std::io::Read as _;
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
            }
            let mut stream = reader.into_inner();
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.flush();
        }
    });

    port
}

#[test]
fn a_real_call_over_a_real_socket_lands_in_the_journal_and_the_ledger() {
    let models = r#"{"object":"list","data":[{"id":"small","type":"llm","quantization":"Q4_K_M","state":"loaded","max_context_length":4096}]}"#;
    // `chat_raw` asks for a stream (`M-23`), so the fixture has to answer like
    // one — SSE chunks, not a single buffered object — or the reader that
    // parses `data: ` lines sees a body it does not recognise and comes back
    // empty without ever failing the call.
    let chat = "data: {\"choices\":[{\"delta\":{\"content\":\"hello back\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
                data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":20,\"prompt_cache_hit_tokens\":800}}\n\n\
                data: [DONE]\n\n";
    let port = fake_link(vec![(200, models.to_string()), (200, chat.to_string())]);

    let root = with_links(
        "cost-e2e",
        &format!(
            "link.local.kind = openai-compat\n\
             link.local.base_url = http://127.0.0.1:{port}\n\
             link.local.model = small\n\
             link.local.privacy = local\n\
             role.chat = local\n\
             price.local.cache_hit = 0.0028\n\
             price.local.cache_miss = 0.14\n\
             price.local.output = 0.28"
        ),
    );

    let ask = run(&root, &["ask", "say hello", "--role", "chat", "--step", "c2/b9/s01"]);
    assert!(ask.ok(), "stderr: {}", ask.stderr);
    assert!(ask.says("hello back"), "the reply: {}", ask.stdout);
    assert!(ask.says("via: link local"), "the provenance: {}", ask.stdout);
    assert!(ask.says("Q4_K_M"), "including the quantization: {}", ask.stdout);
    assert!(ask.says("800 cached"), "and the cache split: {}", ask.stdout);

    // And the bill, replayed out of the journal by a separate process.
    let cost = run(&root, &["cost"]);
    assert!(cost.ok(), "stderr: {}", cost.stderr);
    assert!(cost.says("1 calls"), "{}", cost.stdout);
    assert!(cost.says("1000 tokens in (800 cached)"), "{}", cost.stdout);
    assert!(cost.says("chat"), "grouped by role: {}", cost.stdout);
    // 800 hits at 0.0028 + 200 misses at 0.14 + 20 out at 0.28, per million
    // = 0.00003584. Asserted to six places, because four would round every
    // real call to zero — which is what this test caught.
    assert!(cost.says("0.000036"), "the charge: {}", cost.stdout);
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

// ── a person editing the list (O-18, L-34, V-2) ────────────────────────────

/// A source with one of each marker, so an edit and a delete can be checked
/// against a row that claims something.
fn with_markers(root: &Path) {
    std::fs::write(
        root.join(".harness/perpetum.md"),
        "# requirements\n\n\
         | id | Requirement |\n|---|---|\n\
         | ✅ ~~`L-1`~~ | the journal is append-only. |\n\
         | ⛔ `L-2` | a thing. **Gated: dependency approval** |\n\
         | `L-3` | the journal |\n",
    )
    .expect("requirements");
}

#[test]
fn requirement_add_files_a_row_with_no_marker() {
    let root = fixture("req-add");
    let out = run(&root, &["requirement", "add", "a web front end over a workspace"]);
    assert!(out.ok(), "stderr: {}", out.stderr);
    assert!(out.says("filed L-4"), "{}", out.stdout);

    let text = std::fs::read_to_string(root.join(".harness/perpetum.md")).expect("read");
    assert!(text.contains("| `L-4` | a web front end over a workspace |"), "{text}");
    assert!(!text.contains('✅'), "nothing here writes a marker: {text}");
}

/// `L-34`: the reason edit is its own command and not a file write. It changes
/// what a row says; it cannot change what the row claims.
#[test]
fn requirement_edit_replaces_the_text_and_leaves_the_marker_alone() {
    let root = fixture("req-edit");
    with_markers(&root);

    let out = run(&root, &["requirement", "edit", "L-1", "the journal is append-only, always."]);
    assert!(out.ok(), "stderr: {}", out.stderr);

    let text = std::fs::read_to_string(root.join(".harness/perpetum.md")).expect("read");
    assert!(text.contains("| ✅ ~~`L-1`~~ | the journal is append-only, always. |"), "{text}");
    // And the panel — the surface a person actually reads — still says done.
    let panel = run(&root, &["panel"]);
    assert!(panel.says("append-only, always"), "{}", panel.stdout);
    assert!(panel.says("\"state\":\"done\""), "{}", panel.stdout);
}

/// `V-2`, through the CLI: there is no argument that promotes a row. The text
/// is a text cell, and a marker written into it is prose (`V-23`).
#[test]
fn nothing_on_this_command_can_mark_a_requirement_done() {
    let root = fixture("req-no-green");
    with_markers(&root);

    // The obvious attempts, all of them refused or inert.
    assert!(!run(&root, &["requirement", "done", "L-3"]).ok());
    assert!(!run(&root, &["requirement", "mark", "L-3", "done"]).ok());
    assert!(!run(&root, &["requirement", "edit", "L-3", "--state", "done"]).ok());
    assert!(run(&root, &["requirement", "edit", "L-3", "✅ done now, honestly"]).ok());

    let panel = run(&root, &["panel"]);
    assert!(panel.says("\"id\":\"L-3\""), "{}", panel.stdout);
    // One `done` in the whole catalogue, and it is `L-1` — the row that
    // already had it.
    assert_eq!(panel.stdout.matches("\"state\":\"done\"").count(), 1, "{}", panel.stdout);
}

#[test]
fn requirement_delete_removes_the_row_and_says_what_it_said() {
    let root = fixture("req-delete");
    with_markers(&root);

    let out = run(&root, &["requirement", "delete", "L-2"]);
    assert!(out.ok(), "stderr: {}", out.stderr);
    assert!(out.says("a thing."), "the text comes back out with it: {}", out.stdout);

    let text = std::fs::read_to_string(root.join(".harness/perpetum.md")).expect("read");
    assert!(!text.contains("`L-2`"), "{text}");
    assert!(text.contains("| ✅ ~~`L-1`~~ |"), "its neighbours are untouched: {text}");
    assert!(text.contains("| `L-3` | the journal |"), "{text}");

    // And the id is not handed to the next requirement filed.
    assert!(run(&root, &["requirement", "add", "something else"]).says("filed L-4"));
}

#[test]
fn a_write_against_a_row_that_is_not_there_fails_and_changes_nothing() {
    let root = fixture("req-missing");
    with_markers(&root);
    let before = std::fs::read_to_string(root.join(".harness/perpetum.md")).expect("read");

    for args in [
        &["requirement", "edit", "L-9", "x"][..],
        &["requirement", "delete", "L-9"][..],
        &["ungate", "L-9"][..],
        &["requirement", "edit", "L-3", "ends | the row"][..],
    ] {
        let out = run(&root, args);
        assert!(!out.ok(), "`{args:?}` should fail: {}", out.stdout);
    }
    assert_eq!(std::fs::read_to_string(root.join(".harness/perpetum.md")).ok(), Some(before));
}
