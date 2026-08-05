//! Gates and the evidence they leave (`V-2`, `L-16`).
//!
//! A gate is green because the harness ran the command and kept the transcript.
//! Model prose asserting success is not evidence and never reaches a status
//! marker — so [`Gate::run`] returns a [`GateResult`] that *contains* the
//! transcript, and there is no way to record a pass without one.
//!
//! [`Attempts`] is the other half: two tries at a failing gate, then blocked
//! with the verbatim error. The count is held here, in the engine, so the thing
//! being judged cannot ask for a third.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::binding::Binding;
use crate::error::{Error, Result};
use crate::journal::Record;
use crate::process::{self, Env, Run, Spec};
use crate::step::StepId;

/// Long enough for a cold `cargo build`, short enough that a hung gate does not
/// eat the night. Overridable per project with `gate.timeout` in the binding.
pub const DEFAULT_TIMEOUT_SECS: u64 = 900;

/// How many times a failing gate is retried before the feature is blocked
/// (Perpetum 0.5).
pub const MAX_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone)]
pub struct Gate {
    pub name: String,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Where the command executes (`T-9`). The command itself is the binding's
    /// and is never rewritten — a binding holding `wsl -d Ubuntu -- cargo test`
    /// works on exactly one machine and stops describing what green means.
    pub runtime: crate::runtime::Runtime,
    /// Credentials the operator declared this gate may borrow (`S-9`).
    ///
    /// Gates only, and deliberately: a gate command is a line the *operator*
    /// wrote in `binding.md`. A `shell` call is a line the model composed, and
    /// lending to one would let the model choose where a secret goes — which is
    /// the thing `S-1` exists to prevent, arriving by the back door.
    pub lends: Vec<crate::security::Lend>,
    /// Run without reaching the network (`N-6`), from `gate.offline`.
    ///
    /// Not a firewall — the switches the common toolchains already honour. A
    /// gate that can fail because of a flaky connection manufactures reds, and
    /// a red that is not the code's fault teaches everyone to ignore reds.
    pub offline: bool,
}

impl Gate {
    /// Read the gates the binding declares — `gate.lint`, `gate.build`,
    /// `gate.test` — resolving `gate.cwd` and `gate.timeout` if present.
    pub fn from_binding(binding: &Binding) -> Result<Vec<Gate>> {
        let entries: Vec<(String, String)> =
            binding.entries().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let runtime = crate::runtime::Runtime::from_entries(&entries)?;
        let lends = crate::security::lends_from_entries(&entries);
        // `N-6`: opt-in, because a project whose gates genuinely need the
        // network must not have it taken away by a default.
        let offline = binding
            .get("gate.offline")
            .map(|v| v.trim().eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let cwd = match binding.get("gate.cwd") {
            Ok(rel) => binding.root().join(rel),
            Err(_) => binding.root().to_path_buf(),
        };
        let timeout = match binding.get("gate.timeout") {
            Ok(text) => Duration::from_secs(text.trim().parse().map_err(|_| {
                Error::unbound("gate.timeout", format!("`{text}` is not a number of seconds"))
            })?),
            Err(_) => Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        };

        let mut gates: Vec<Gate> = binding
            .entries()
            .filter_map(|(key, value)| {
                let name = key.strip_prefix("gate.")?;
                // Settings, not gates. Without `offline` here it becomes a
                // gate named `offline` whose command is `true`, which passes
                // and means nothing — a green with no code behind it, which is
                // the one thing this file exists to prevent.
                if matches!(name, "cwd" | "timeout" | "offline") {
                    return None;
                }
                Some(Gate {
                    name: name.to_string(),
                    command: value.to_string(),
                    cwd: cwd.clone(),
                    timeout,
                    runtime: runtime.clone(),
                    lends: lends.clone(),
                    offline,
                })
            })
            .collect();

        if gates.is_empty() {
            return Err(Error::unbound(
                "gate.*",
                "the binding declares no gates — there is nothing to be green",
            ));
        }

        // Perpetum 0.3's order: lint, then compile, then tests. Anything the
        // project adds runs after those, in declaration order.
        gates.sort_by_key(|gate| match gate.name.as_str() {
            "lint" => 0,
            "build" => 1,
            "test" => 2,
            _ => 3,
        });
        Ok(gates)
    }

    pub fn named(binding: &Binding, name: &str) -> Result<Gate> {
        Gate::from_binding(binding)?
            .into_iter()
            .find(|gate| gate.name == name)
            .ok_or_else(|| Error::unbound(format!("gate.{name}"), "not declared in the binding"))
    }

    pub fn run(&self) -> Result<GateResult> {
        if !self.cwd.exists() {
            return Err(Error::unbound(
                "gate.cwd",
                format!("{} does not exist", self.cwd.display()),
            ));
        }
        if let Some(complaint) = self_lock_complaint(&self.cwd) {
            return Err(Error::unbound("gate", complaint));
        }
        // `S-9`: the operator said this gate may borrow these, by name. The
        // values are read here and nowhere else, and never enter `command` —
        // argv is world-readable in a process listing (`S-2`).
        let lent = crate::security::Lent::resolve(&self.lends);
        let mut env = Env::declared();
        // `N-6` before the lending, so a lent credential wins if a project
        // declares both. The combination is odd rather than wrong — a token
        // for a registry the gate has been told not to reach — but the
        // operator declared each of them on purpose, and silently dropping one
        // would be the harness deciding which it meant.
        for (key, value) in crate::security::gate_environment(self.offline) {
            env = env.with(key, value);
        }
        for lend in &lent {
            let (var, value) = lend.as_env();
            env = env.with(var, value);
        }
        let spec = Spec::new(&self.command, &self.cwd, self.timeout).with_env(env);
        // `T-9`: the command is the binding's; the runtime only decides where
        // it executes. `Runtime::Host` returns it untouched.
        let spec = self.runtime.wrap(&spec)?;
        let run = process::run(&spec)?;
        // Before the transcript reaches a journal, a verbose line or an
        // operator's terminal: the child had the value and may have echoed it.
        let run = scrub(run, &lent);
        crate::verbose::say(
            "gate",
            &format!("{} · {} · {}", self.name, self.command, run.exit.describe()),
        );
        crate::verbose::body("gate/output", &run.stdout_tail);
        Ok(GateResult {
            name: self.name.clone(),
            run,
            sha: None,
            dirty: None,
            runtime: self.runtime.to_string(),
        })
    }

    /// Run this gate somewhere other than the host (`T-9`).
    pub fn in_runtime(mut self, runtime: crate::runtime::Runtime) -> Gate {
        self.runtime = runtime;
        self
    }
}

/// Replace every lent value in a run's output with the mask (`S-2`, `S-9`).
///
/// Both streams, because a toolchain writes its diagnostics to whichever it
/// prefers and a secret echoed on stderr is as leaked as one on stdout. The
/// command line is not scrubbed — the value never went into it, and a gate
/// whose *text* matched a secret would be an operator writing the secret into
/// `binding.md`, which `S-2` already forbids.
fn scrub(run: Run, lent: &[crate::security::Lent]) -> Run {
    if lent.is_empty() {
        return run;
    }
    let patterns = crate::security::redactions(lent);
    Run {
        stdout_tail: crate::security::redact(&run.stdout_tail, &patterns).text,
        stderr_tail: crate::security::redact(&run.stderr_tail, &patterns).text,
        ..run
    }
}

/// Would this gate be asked to rebuild the binary that is running it? (`T-18`)
///
/// On Windows a running executable cannot be replaced, so a harness launched
/// from the workspace's own `target/` fails its own build gate with
/// `Access is denied (os error 5)` — an error that says nothing about the code
/// and costs an operator an hour. Refused up front, with the fix in the message.
///
/// Found in `c1/b5/s07` by running the gates through the harness on its own
/// repository, which is the only way this was ever going to show up.
pub fn self_lock_complaint(cwd: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let workspace = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    if !exe.starts_with(&workspace) {
        return None;
    }
    Some(format!(
        "this binary is running from {}, inside the workspace the gates build ({}). \
         On Windows the build cannot replace a running executable. \
         Copy `perp` somewhere outside the tree and run it from there.",
        exe.display(),
        workspace.display()
    ))
}

/// A gate that has actually been run. There is no constructor that does not
/// carry a [`Run`] — the transcript is the point (`V-2`).
#[derive(Debug, Clone)]
pub struct GateResult {
    pub name: String,
    pub run: Run,
    /// Where it ran (`T-9`). On the transcript so nobody compares a container
    /// green with a host red and averages them.
    pub runtime: String,
    /// The commit the gate ran against. A green gate at a sha that no longer
    /// exists is not evidence (`G-6`); filling this in is batch 4's job.
    pub sha: Option<String>,
    /// Whether the working tree had uncommitted changes when the gate ran.
    ///
    /// The sha alone is not the provenance it looks like. A gate runs against
    /// the *tree*, and when the tree is dirty the sha names the parent commit —
    /// which does not contain the code that was gated. Running this harness
    /// against a Flutter backlog, every gate transcript for `R-1` to `R-3` was
    /// pinned to the commit that added the binding, because the code the gates
    /// went green on had not been committed yet. Nothing looked wrong: the sha
    /// resolved, and a reader following it would have found a tree with none of
    /// the work in it.
    ///
    /// `G-6` exists to stop a green gate being credited to the wrong tree. It
    /// caught the missing-sha case and not this one, which is worse — a sha
    /// that resolves is trusted, and an absent one is questioned.
    pub dirty: Option<bool>,
}

impl GateResult {
    pub fn is_green(&self) -> bool {
        self.run.is_success()
    }

    pub fn at_sha(mut self, sha: impl Into<String>) -> GateResult {
        self.sha = Some(sha.into());
        self
    }

    /// Say what the tree looked like, not just which commit HEAD was on.
    pub fn with_tree(mut self, clean: bool) -> GateResult {
        self.dirty = Some(!clean);
        self
    }

    /// The verbatim evidence block. Never summarised — Perpetum 0.5 wants the
    /// actual error text, not a description of it.
    pub fn evidence(&self) -> String {
        let mut out = format!("gate: {}\n", self.name);
        if let Some(sha) = &self.sha {
            out.push_str(&format!("sha: {sha}\n"));
            // Said in the transcript rather than left for a reader to infer.
            // The sha is the honest half of the answer only when the tree it
            // names is the tree that ran.
            if self.dirty == Some(true) {
                out.push_str(
                    "tree: dirty — this sha is the parent commit and does NOT contain what was \
                     gated (`G-6`)\n",
                );
            }
        }
        out.push_str(&self.run.transcript());
        out
    }

    /// The journal record for this gate run.
    pub fn to_record(&self, step: StepId, at: i64) -> Record {
        let summary = format!(
            "gate {} — {}",
            self.name,
            if self.is_green() { "green".to_string() } else { self.run.exit.describe() }
        );
        Record::outcome(step, at, self.is_green(), summary).with_detail(self.evidence())
    }
}

/// What to do after a gate run (`L-16`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Green. Move on.
    Pass,
    /// Failed, and there is an attempt left. Fix it.
    Retry { attempt: u32, remaining: u32 },
    /// Failed twice. The feature is blocked, carrying the actual error.
    Blocked { verbatim: String },
}

/// The attempt counter for one gate on one feature.
///
/// Lives in the engine rather than in a prompt: the model cannot ask for a
/// third attempt if nothing is listening for the request.
#[derive(Debug, Clone)]
pub struct Attempts {
    max: u32,
    used: u32,
}

impl Default for Attempts {
    fn default() -> Attempts {
        Attempts::new(MAX_ATTEMPTS)
    }
}

impl Attempts {
    pub fn new(max: u32) -> Attempts {
        Attempts { max, used: 0 }
    }

    pub fn used(&self) -> u32 {
        self.used
    }

    pub fn remaining(&self) -> u32 {
        self.max.saturating_sub(self.used)
    }

    /// Record a run and say what happens next.
    pub fn record(&mut self, result: &GateResult) -> Verdict {
        if result.is_green() {
            return Verdict::Pass;
        }
        self.used += 1;
        if self.used >= self.max {
            Verdict::Blocked { verbatim: result.evidence() }
        } else {
            Verdict::Retry { attempt: self.used, remaining: self.max - self.used }
        }
    }
}

/// Run every declared gate in Perpetum 0.3's order, stopping at the first red —
/// a build that failed makes the test result meaningless, and running it anyway
/// produces a second error that hides the first.
pub fn run_all(gates: &[Gate]) -> Result<Vec<GateResult>> {
    let mut results = Vec::new();
    for gate in gates {
        let result = gate.run()?;
        let green = result.is_green();
        results.push(result);
        if !green {
            break;
        }
    }
    Ok(results)
}

/// Where a gate's evidence is written, beside the journal.
pub fn evidence_dir(journal: &Path) -> PathBuf {
    journal
        .parent()
        .unwrap_or(Path::new("."))
        .join("gates")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::Exit;
    use crate::testutil::tmpdir;

    fn fake_run(command: &str, exit: Exit, stderr: &str) -> Run {
        Run {
            command: command.to_string(),
            cwd: PathBuf::from("."),
            env: "declared: kept 3, set 0".to_string(),
            exit,
            duration_ms: 12,
            stdout_tail: String::new(),
            stderr_tail: stderr.to_string(),
            truncated: false,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    fn at_tree(sha: &str, clean: bool) -> GateResult {
        GateResult {
            runtime: "host".to_string(),
            name: "test".into(),
            run: fake_run("cargo test", Exit::Code(0), ""),
            sha: Some(sha.into()),
            dirty: None,
        }
        .with_tree(clean)
    }

    /// `G-6`: a sha that resolves is trusted, so one naming the wrong tree is
    /// worse evidence than none at all.
    ///
    /// Running this harness on a Flutter backlog, every transcript for `R-1` to
    /// `R-3` was pinned to the commit that added the binding — the code the
    /// gates went green on had not been committed yet. A reader following that
    /// sha finds a tree with none of the work in it, and nothing said so.
    #[test]
    fn a_gate_on_a_dirty_tree_says_the_sha_is_not_what_ran() {
        let evidence = at_tree("72ebbdd", false).evidence();
        assert!(evidence.contains("72ebbdd"), "the sha is still recorded: {evidence}");
        assert!(
            evidence.contains("does NOT contain what was gated"),
            "and is qualified, so it cannot be read as provenance: {evidence}"
        );
    }

    #[test]
    fn a_gate_on_a_clean_tree_pins_without_a_caveat() {
        let evidence = at_tree("072c401", true).evidence();
        assert!(evidence.contains("072c401"));
        assert!(!evidence.contains("does NOT contain"), "a clean tree needs no caveat: {evidence}");
    }

    fn red(stderr: &str) -> GateResult {
        GateResult {
            runtime: "host".to_string(),
            name: "build".into(),
            run: fake_run("cargo build", Exit::Code(101), stderr),
            sha: None,
            dirty: None,
        }
    }

    fn green() -> GateResult {
        GateResult {
            runtime: "host".to_string(),
            name: "build".into(),
            run: fake_run("cargo build", Exit::Code(0), ""),
            sha: None,
            dirty: None,
        }
    }

    fn binding_with(root: &Path, block: &str) -> Binding {
        std::fs::create_dir_all(root.join(".harness")).expect("dirs");
        std::fs::write(
            root.join(".harness/binding.md"),
            format!("```perp-binding\n{block}\n```\n"),
        )
        .expect("write");
        Binding::load(root).expect("load")
    }

    #[test]
    fn reads_the_gates_the_binding_declares_in_perpetum_order() {
        let root = tmpdir("gate-binding");
        let binding = binding_with(
            &root,
            "gate.cwd   = crates\n\
             gate.test  = cargo test --workspace\n\
             gate.lint  = cargo clippy -- -D warnings\n\
             gate.build = cargo build --workspace",
        );
        let gates = Gate::from_binding(&binding).expect("gates");
        let names: Vec<&str> = gates.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["lint", "build", "test"], "lint, compile, tests — in that order");
        assert_eq!(gates[0].cwd, root.join("crates"));
        assert_eq!(gates[0].timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    }

    #[test]
    fn a_binding_with_no_gates_is_an_error_not_an_empty_pass() {
        let root = tmpdir("gate-none");
        let binding = binding_with(&root, "path.requirements = .harness/perpetum.md");
        let err = Gate::from_binding(&binding).expect_err("must refuse");
        assert!(format!("{err}").contains("nothing to be green"), "{err}");
    }

    #[test]
    fn the_timeout_comes_from_the_binding_when_it_says_so() {
        let root = tmpdir("gate-timeout");
        let binding = binding_with(&root, "gate.timeout = 5\ngate.test = cargo test");
        let gates = Gate::from_binding(&binding).expect("gates");
        assert_eq!(gates[0].timeout, Duration::from_secs(5));

        let bad = binding_with(&tmpdir("gate-timeout-bad"), "gate.timeout = soon\ngate.test = x");
        assert!(Gate::from_binding(&bad).is_err(), "a timeout that is not a number is an error");
    }

    #[test]
    fn evidence_carries_the_command_and_the_verbatim_error() {
        let stderr = "error[E0308]: mismatched types\n  --> src/main.rs:4:9";
        let evidence = red(stderr).evidence();
        assert!(evidence.contains("gate: build"));
        assert!(evidence.contains("$ cargo build"));
        assert!(evidence.contains("exit 101"));
        assert!(evidence.contains("mismatched types"), "the actual error, not a summary");
    }

    #[test]
    fn a_gate_record_is_an_outcome_carrying_its_transcript() {
        let step = StepId::parse("c1/b2/s03").expect("step");
        let record = red("boom").to_record(step.clone(), 1_700_000_000);
        assert_eq!(record.ok, Some(false));
        assert_eq!(record.step, step);
        assert!(record.summary.contains("gate build"));
        assert!(record.detail.as_deref().unwrap_or_default().contains("boom"));

        let good = green().to_record(step, 1_700_000_000);
        assert_eq!(good.ok, Some(true));
        assert!(good.summary.contains("green"));
        assert!(
            good.detail.as_deref().unwrap_or_default().contains("$ cargo build"),
            "a pass carries its transcript too — that is what makes it checkable"
        );
    }

    #[test]
    fn two_failures_block_and_the_third_attempt_is_not_offered() {
        // `L-16` / Perpetum 0.5.
        let mut attempts = Attempts::default();
        assert_eq!(attempts.record(&red("first")), Verdict::Retry { attempt: 1, remaining: 1 });
        match attempts.record(&red("second, with the real error")) {
            Verdict::Blocked { verbatim } => {
                assert!(verbatim.contains("second, with the real error"), "verbatim: {verbatim}");
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
        assert_eq!(attempts.remaining(), 0);
        // A third run does not reopen it.
        assert!(matches!(attempts.record(&red("third")), Verdict::Blocked { .. }));
    }

    #[test]
    fn a_green_run_never_counts_against_the_attempts() {
        let mut attempts = Attempts::default();
        assert_eq!(attempts.record(&red("once")), Verdict::Retry { attempt: 1, remaining: 1 });
        assert_eq!(attempts.record(&green()), Verdict::Pass);
        assert_eq!(attempts.used(), 1, "a pass does not consume an attempt");
    }

    #[test]
    fn the_gate_runs_where_the_runtime_says_and_records_it() {
        // `T-9`, found missing by the red run: nothing checked that `Gate::run`
        // actually applies the runtime, so removing the call left every test
        // green.
        //
        // A container engine that is not installed is the cheap proof: the
        // command has to reach it to fail on it.
        let dir = tmpdir("gate-runtime");
        let gate = Gate {
            name: "test".into(),
            command: "cargo test".into(),
            cwd: dir.clone(),
            timeout: Duration::from_secs(30),
            runtime: crate::runtime::Runtime::Container {
                image: "rust:1".into(),
                engine: "perp-not-a-real-container-engine".into(),
            },
            lends: Vec::new(),
            offline: false,
        };

        match gate.run() {
            Ok(result) => {
                assert!(!result.is_green(), "there is no such engine");
                assert_eq!(result.runtime, "perp-not-a-real-container-engine:rust:1");
                assert!(
                    result.evidence().contains("perp-not-a-real-container-engine"),
                    "the wrapped command is on the transcript: {}",
                    result.evidence()
                );
            }
            // Spawning a program that does not exist fails before it runs on
            // some platforms. Either way the runtime was applied — an
            // unwrapped `cargo test` would not mention the engine at all.
            Err(e) => assert!(
                format!("{e}").contains("perp-not-a-real-container-engine")
                    || format!("{e}").to_lowercase().contains("not found"),
                "{e}"
            ),
        }
    }

    /// `S-9` end to end: the child gets the real value, and the transcript
    /// that goes in the journal does not.
    ///
    /// The gate command echoes the variable on purpose — that is the failure
    /// being tested. A build script logging its own configuration, a test
    /// printing the request it sent, or `curl -v` all do this without meaning
    /// to, and `S-2` says the journal must not carry it either way.
    #[test]
    fn a_gate_may_borrow_a_credential_without_it_reaching_the_transcript() {
        let secret = "7c1d-gate-lent-passphrase";
        std::env::set_var("PERP_TEST_GATE_LEND", secret);
        let dir = tmpdir("gate-lend");

        let echo = if cfg!(windows) {
            "cmd /C \"echo token=%PERP_TEST_GATE_LEND%\""
        } else {
            "sh -c \"echo token=$PERP_TEST_GATE_LEND\""
        };
        let gate = Gate {
            runtime: crate::runtime::Runtime::Host,
            name: "build".into(),
            command: echo.into(),
            cwd: dir,
            timeout: Duration::from_secs(30),
            lends: vec![crate::security::Lend {
                name: "registry".into(),
                var: "PERP_TEST_GATE_LEND".into(),
            }],
            offline: false,
        };

        let result = gate.run().expect("the gate ran");
        let evidence = result.evidence();

        // The child really did receive it — otherwise this test would pass
        // against a version that lends nothing at all.
        assert!(
            evidence.contains("token=") && !evidence.contains("token=\n"),
            "the child never got the value: {evidence}"
        );
        assert!(!evidence.contains(secret), "the lent value reached the journal: {evidence}");
        assert!(evidence.contains(crate::security::MASK), "and was masked: {evidence}");
    }

    /// `N-6`: the switches reach the child, and only when asked for.
    ///
    /// Asserted on what the command actually sees rather than on the `Gate`
    /// field, because the field being set and the variable arriving are two
    /// different claims and only the second one is the requirement.
    #[test]
    fn an_offline_gate_hands_the_toolchains_their_switches() {
        let dir = tmpdir("gate-offline");
        let show = if cfg!(windows) {
            "cmd /C \"echo cargo=%CARGO_NET_OFFLINE% pip=%PIP_NO_INDEX%\""
        } else {
            "sh -c \"echo cargo=$CARGO_NET_OFFLINE pip=$PIP_NO_INDEX\""
        };
        let base = Gate {
            runtime: crate::runtime::Runtime::Host,
            name: "check".into(),
            command: show.into(),
            cwd: dir,
            timeout: Duration::from_secs(30),
            lends: Vec::new(),
            offline: false,
        };

        let on = Gate { offline: true, ..base.clone() }.run().expect("ran").evidence();
        assert!(on.contains("cargo=true"), "CARGO_NET_OFFLINE never arrived: {on}");
        assert!(on.contains("pip=1"), "PIP_NO_INDEX never arrived: {on}");

        // Opt-in: a project whose gates need the network keeps it.
        let off = base.run().expect("ran").evidence();
        assert!(!off.contains("cargo=true"), "offline was imposed rather than asked for: {off}");
    }

    /// `gate.offline` is a setting, not a gate.
    ///
    /// `from_binding` turns every `gate.*` key into a gate, so without an
    /// exclusion this one becomes a gate named `offline` whose command is
    /// `true` — which passes, and means nothing. A green with no code behind
    /// it is the single thing this file exists to prevent.
    #[test]
    fn the_offline_setting_does_not_become_a_gate_that_runs_true() {
        let root = tmpdir("gate-offline-not-a-gate");
        std::fs::create_dir_all(root.join(".harness")).expect("dirs");
        std::fs::write(root.join(".harness/perpetum.md"), "# requirements\n").expect("reqs");
        std::fs::write(
            root.join(".harness/binding.md"),
            "```perp-binding\n\
             path.requirements = .harness/perpetum.md\n\
             out.journal = .harness/journal.jsonl\n\
             out.state = .harness/state.md\n\
             gate.offline = true\n\
             gate.check = cargo --version\n\
             ```\n",
        )
        .expect("binding");

        let binding = crate::Binding::load(&root).expect("binding");
        let gates = Gate::from_binding(&binding).expect("gates");
        let names: Vec<&str> = gates.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["check"], "offline is a setting: {names:?}");
        assert!(gates[0].offline, "and it was read");
    }

    /// A gate that borrows nothing is untouched — no mask, no scrubbing pass.
    #[test]
    fn a_gate_that_borrows_nothing_has_its_output_left_alone() {
        let dir = tmpdir("gate-no-lend");
        let echo =
            if cfg!(windows) { "cmd /C \"echo plain\"" } else { "sh -c \"echo plain\"" };
        let gate = Gate {
            runtime: crate::runtime::Runtime::Host,
            name: "build".into(),
            command: echo.into(),
            cwd: dir,
            timeout: Duration::from_secs(30),
            lends: Vec::new(),
            offline: false,
        };
        let evidence = gate.run().expect("ran").evidence();
        assert!(evidence.contains("plain"), "{evidence}");
        assert!(!evidence.contains(crate::security::MASK), "nothing to mask: {evidence}");
    }

    #[test]
    fn a_missing_working_directory_is_an_error_not_a_red_gate() {
        let root = tmpdir("gate-nocwd");
        let gate = Gate {
            runtime: crate::runtime::Runtime::Host,
            name: "test".into(),
            command: "cargo test".into(),
            cwd: root.join("does-not-exist"),
            timeout: Duration::from_secs(5),
            lends: Vec::new(),
            offline: false,
        };
        let err = gate.run().expect_err("must refuse");
        assert!(format!("{err}").contains("does not exist"), "{err}");
    }

    #[test]
    fn runs_stop_at_the_first_red() {
        let dir = tmpdir("gate-stop");
        let fail = if cfg!(windows) { "cmd /C \"exit 1\"" } else { "sh -c \"exit 1\"" };
        let pass = if cfg!(windows) { "cmd /C \"exit 0\"" } else { "sh -c \"exit 0\"" };
        let gates = vec![
            Gate { runtime: crate::runtime::Runtime::Host, name: "lint".into(), command: fail.into(), cwd: dir.clone(), timeout: Duration::from_secs(30), lends: Vec::new(), offline: false },
            Gate { runtime: crate::runtime::Runtime::Host, name: "build".into(), command: pass.into(), cwd: dir.clone(), timeout: Duration::from_secs(30), lends: Vec::new(), offline: false },
        ];
        let results = run_all(&gates).expect("run");
        assert_eq!(results.len(), 1, "the build must not run after lint went red");
        assert!(!results[0].is_green());
    }

    #[test]
    fn a_gate_that_would_rebuild_the_running_binary_is_refused() {
        // `T-18`. The test binary genuinely lives inside `crates/target/`, so
        // this is the real condition, not a simulated one.
        let workspace = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .expect("the test binary has a directory");
        let complaint = self_lock_complaint(&workspace).expect("must complain");
        assert!(complaint.contains("running from"), "{complaint}");
        assert!(complaint.contains("Copy `perp` somewhere outside"), "the fix is in the message");

        let gate = Gate {
            runtime: crate::runtime::Runtime::Host,
            name: "build".into(),
            command: "cargo build".into(),
            cwd: workspace,
            timeout: Duration::from_secs(5),
            lends: Vec::new(),
            offline: false,
        };
        let err = gate.run().expect_err("must refuse before spawning cargo");
        assert!(format!("{err}").contains("cannot replace a running executable"), "{err}");
    }

    #[test]
    fn a_gate_outside_the_running_binarys_tree_is_fine() {
        let elsewhere = tmpdir("gate-elsewhere");
        assert_eq!(self_lock_complaint(&elsewhere), None);
    }

    #[test]
    fn evidence_lands_beside_the_journal() {
        assert_eq!(
            evidence_dir(Path::new(".harness/journal.jsonl")),
            PathBuf::from(".harness/gates")
        );
    }
}
