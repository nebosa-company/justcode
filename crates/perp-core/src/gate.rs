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
}

impl Gate {
    /// Read the gates the binding declares — `gate.lint`, `gate.build`,
    /// `gate.test` — resolving `gate.cwd` and `gate.timeout` if present.
    pub fn from_binding(binding: &Binding) -> Result<Vec<Gate>> {
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
                if matches!(name, "cwd" | "timeout") {
                    return None;
                }
                Some(Gate {
                    name: name.to_string(),
                    command: value.to_string(),
                    cwd: cwd.clone(),
                    timeout,
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
        let spec = Spec::new(&self.command, &self.cwd, self.timeout).with_env(Env::declared());
        let run = process::run(&spec)?;
        Ok(GateResult { name: self.name.clone(), run, sha: None })
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
    /// The commit the gate ran against. A green gate at a sha that no longer
    /// exists is not evidence (`G-6`); filling this in is batch 4's job.
    pub sha: Option<String>,
}

impl GateResult {
    pub fn is_green(&self) -> bool {
        self.run.is_success()
    }

    pub fn at_sha(mut self, sha: impl Into<String>) -> GateResult {
        self.sha = Some(sha.into());
        self
    }

    /// The verbatim evidence block. Never summarised — Perpetum 0.5 wants the
    /// actual error text, not a description of it.
    pub fn evidence(&self) -> String {
        let mut out = format!("gate: {}\n", self.name);
        if let Some(sha) = &self.sha {
            out.push_str(&format!("sha: {sha}\n"));
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
        }
    }

    fn red(stderr: &str) -> GateResult {
        GateResult {
            name: "build".into(),
            run: fake_run("cargo build", Exit::Code(101), stderr),
            sha: None,
        }
    }

    fn green() -> GateResult {
        GateResult {
            name: "build".into(),
            run: fake_run("cargo build", Exit::Code(0), ""),
            sha: None,
        }
    }

    fn binding_with(root: &Path, block: &str) -> Binding {
        std::fs::create_dir_all(root.join("docs/perpetum")).expect("dirs");
        std::fs::write(
            root.join("docs/perpetum/binding.md"),
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
        let binding = binding_with(&root, "path.requirements = docs/perpetum.md");
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
    fn a_missing_working_directory_is_an_error_not_a_red_gate() {
        let root = tmpdir("gate-nocwd");
        let gate = Gate {
            name: "test".into(),
            command: "cargo test".into(),
            cwd: root.join("does-not-exist"),
            timeout: Duration::from_secs(5),
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
            Gate { name: "lint".into(), command: fail.into(), cwd: dir.clone(), timeout: Duration::from_secs(30) },
            Gate { name: "build".into(), command: pass.into(), cwd: dir.clone(), timeout: Duration::from_secs(30) },
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
            name: "build".into(),
            command: "cargo build".into(),
            cwd: workspace,
            timeout: Duration::from_secs(5),
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
            evidence_dir(Path::new("docs/perpetum/journal.jsonl")),
            PathBuf::from("docs/perpetum/gates")
        );
    }
}
