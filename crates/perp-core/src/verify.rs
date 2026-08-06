//! The honesty machinery (`V-1`, `V-3`, `V-4`, `V-7`, `V-8`, `V-9`, `V-10`).
//!
//! Everything else in this crate makes the loop survive. This module is what
//! makes it worth surviving: the difference between a week of unattended
//! running that produced software and one that produced a convincing story
//! about software.
//!
//! Four mechanisms, each aimed at a specific lie an unattended loop tells:
//!
//! | Lie | Mechanism |
//! |---|---|
//! | "that feature doesn't exist yet" (it shipped months ago) | [`RealityCheck`] |
//! | "the tests pass" (they cannot fail) | [`RedRun`] |
//! | "the gate is green now" (the assertion was deleted) | [`tamper`] |
//! | "the batch is done" (three items are gated) | [`Marker`] and [`Counts`] |

use std::collections::BTreeSet;

use crate::error::{Error, Result};
use crate::gate::GateResult;
use crate::git::Repo;
use crate::journal::{Kind, Record};
use crate::watchdog::content_hash;

// ── V-1 ────────────────────────────────────────────────────────────────────

/// Whether a requirement is already built (`V-1`, Perpetum 0.7).
///
/// Implementing a feature twice is the common failure, ahead of implementing it
/// never — so this runs *before* implementation, and its result is recorded
/// whether or not it found anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealityCheck {
    pub requirement: String,
    pub needles: Vec<String>,
    pub in_tree: Vec<String>,
    pub in_history: Vec<String>,
}

impl RealityCheck {
    /// Look in the working tree *and* in the history. A grep answers "is it
    /// here"; `git log -S` answers "was it here and taken out", which is a
    /// different and equally useful answer.
    /// `V-21`: `exclude` names directories whose contents are the harness's own
    /// bookkeeping. They are where a requirement is *written down*, not where it
    /// is implemented, so counting them is counting the question as its answer.
    pub fn run(
        repo: &Repo,
        requirement: &str,
        needles: &[&str],
        exclude: &[String],
    ) -> Result<RealityCheck> {
        let mut in_tree = BTreeSet::new();
        let mut in_history = BTreeSet::new();
        for needle in needles {
            in_tree.extend(repo.tree_mentions(needle, exclude)?);
            in_history.extend(repo.history_mentions(needle, exclude)?);
        }
        Ok(RealityCheck {
            requirement: requirement.to_string(),
            needles: needles.iter().map(|n| (*n).to_string()).collect(),
            in_tree: in_tree.into_iter().collect(),
            in_history: in_history.into_iter().collect(),
        })
    }

    /// Present in the working tree — do not build it again.
    pub fn already_built(&self) -> bool {
        !self.in_tree.is_empty()
    }

    /// Absent now, but the history has touched it. Worth a human's attention:
    /// something removed it, and rebuilding it blind may re-break whatever
    /// that removal fixed.
    pub fn was_removed(&self) -> bool {
        self.in_tree.is_empty() && !self.in_history.is_empty()
    }

    pub fn evidence(&self) -> String {
        let mut out = format!("reality check for {}\n", self.requirement);
        out.push_str(&format!("searched: {}\n", self.needles.join(", ")));
        out.push_str(&format!(
            "in the tree: {}\n",
            if self.in_tree.is_empty() { "nothing".into() } else { self.in_tree.join(", ") }
        ));
        out.push_str(&format!(
            "in the history: {}\n",
            if self.in_history.is_empty() {
                "nothing".to_string()
            } else {
                format!("{} commit(s)", self.in_history.len())
            }
        ));
        out
    }
}

/// A feature may not enter implementation without a recorded reality check
/// (`V-1`). The engine asks this; the model does not get to skip it.
pub fn may_implement(records: &[Record], requirement: &str) -> std::result::Result<(), String> {
    let checked = records.iter().any(|record| {
        record.requirements.iter().any(|id| id == requirement)
            && record
                .detail
                .as_deref()
                .is_some_and(|d| d.starts_with("reality check for"))
    });
    if checked {
        Ok(())
    } else {
        Err(format!(
            "{requirement} has no recorded reality check — Perpetum 0.7 says look before building"
        ))
    }
}

// ── V-3, V-10 ──────────────────────────────────────────────────────────────

/// Did the test actually have to be there? (`V-3`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedVerdict {
    /// Failed without the change, passed with it. The test earns its place.
    Earned,
    /// Passed both ways — it cannot fail, so it proves nothing.
    ProvesNothing,
    /// Failed both ways — the change did not fix what the test checks.
    StillBroken,
    /// The tree was never actually different, so neither result means anything
    /// (`V-10`).
    NotActuallyChanged,
}

impl RedVerdict {
    pub fn is_earned(&self) -> bool {
        *self == RedVerdict::Earned
    }

    pub fn describe(&self) -> &'static str {
        match self {
            RedVerdict::Earned => "failed without the change and passed with it",
            RedVerdict::ProvesNothing => {
                "passed with and without the change — it cannot fail, so it is not a test"
            }
            RedVerdict::StillBroken => "failed both ways — the change does not fix it",
            RedVerdict::NotActuallyChanged => {
                "the tree was identical in both runs — the result is meaningless"
            }
        }
    }
}

/// A test run twice: once against the tree without the change, once with it.
#[derive(Debug, Clone)]
pub struct RedRun {
    pub without: GateResult,
    pub with: GateResult,
    /// Hash of the file under test in each run. Equal hashes mean the "before"
    /// run was not actually before anything (`V-10`).
    pub hash_without: u64,
    pub hash_with: u64,
    /// Where the displaced work went while the first run happened (`G-10`).
    pub stash_ref: Option<String>,
}

impl RedRun {
    pub fn new(without: GateResult, with: GateResult, before: &[u8], after: &[u8]) -> RedRun {
        RedRun {
            without,
            with,
            hash_without: content_hash(before),
            hash_with: content_hash(after),
            stash_ref: None,
        }
    }

    pub fn stashed_at(mut self, stash_ref: impl Into<String>) -> RedRun {
        self.stash_ref = Some(stash_ref.into());
        self
    }

    /// Actually run it (`V-3`).
    ///
    /// The type was written, tested, and never executed: nothing built a
    /// `RedRun`, so a test that cannot fail passed the gate exactly like one
    /// that earns its place — the lie this mechanism exists to catch, going
    /// uncaught.
    ///
    /// **Through a worktree, not a stash.** The original design stashed the
    /// change, ran the gate and put it back, which `T-26` was built to make
    /// unnecessary: an unattended loop that dies between the stash and the pop
    /// leaves the operator's work in a ref they have to know to look for. A
    /// throwaway worktree at `at` is the same tree without the change, and the
    /// live tree is never touched.
    ///
    /// `at` is **where the work started**, which is not `HEAD`. A step commits
    /// what it touched as it goes (`T-22`), so by the time a batch's red run
    /// happens `HEAD` already contains the change — and comparing against it
    /// compiles the same code twice, passes both times, and reports
    /// `ProvesNothing` about a test it never tried. Janitor's first batch wrote
    /// eleven real tests and had them written off exactly that way.
    ///
    /// The gate runs **with** the change first: it is the result the loop needs
    /// either way, so a sandbox that cannot be built costs the verdict rather
    /// than the gate.
    pub fn perform(repo: &Repo, gate: &crate::gate::Gate, at: &str) -> Result<RedRun> {
        let with = gate.run()?;

        // Whether the tree differs from `at` at all. Empty means both runs were
        // over identical trees and neither result means anything (`V-10`).
        //
        // Measured against `at`, not against `HEAD`. Those are the same thing
        // only while nothing has been committed since, and a step commits what
        // it touched as it goes (`T-22`) — so `status --porcelain` alone called
        // a batch's committed work "no change at all". Both halves matter: the
        // diff carries everything committed since `at` *and* the working tree,
        // and the untracked names carry a new file, which a diff never mentions
        // and which is how a new test usually arrives.
        let mut change = repo.plumbing_all(&["diff", at]).unwrap_or_default();
        for line in repo.plumbing_all(&["status", "--porcelain"]).unwrap_or_default().lines() {
            if let Some(path) = line.trim().strip_prefix("?? ") {
                change.push_str(path);
                change.push('\n');
            }
        }

        // The repository's own path is in the name, not just the gate's.
        // Without it every red run in one process shares a directory: `at` is
        // almost always `HEAD` and gate names repeat across workspaces, so two
        // running at once would hand each other their worktrees. Found by two
        // tests in this file doing exactly that.
        let scratch = std::env::temp_dir().join(format!(
            "perp-redrun-{}-{}",
            std::process::id(),
            crate::watchdog::content_hash(
                format!("{at}|{}|{}", gate.name, repo.root().display()).as_bytes()
            )
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        repo.add_worktree(&scratch, at)?;

        // The gate's working directory, in the copy rather than the live tree.
        // `gate.cwd` is absolute and under the repository, so the same relative
        // path is the same place in the worktree.
        let relative =
            gate.cwd.strip_prefix(repo.root()).unwrap_or(std::path::Path::new(""));
        let sandboxed = crate::gate::Gate { cwd: scratch.join(relative), ..gate.clone() };
        let without = sandboxed.run();

        // Removed before the result is examined, so an error path cannot leave
        // one behind — `G-11` says a worktree is never left stale, and a
        // sandbox that outlives its command is a stale worktree with a friendly
        // name.
        repo.remove_worktree(&scratch);
        let _ = std::fs::remove_dir_all(&scratch);

        Ok(RedRun::new(without?, with, b"", change.as_bytes()))
    }

    pub fn verdict(&self) -> RedVerdict {
        if self.hash_without == self.hash_with {
            return RedVerdict::NotActuallyChanged;
        }
        match (self.without.is_green(), self.with.is_green()) {
            (false, true) => RedVerdict::Earned,
            (true, true) => RedVerdict::ProvesNothing,
            (_, false) => RedVerdict::StillBroken,
        }
    }

    /// Both transcripts, verbatim. A red run that is only reported as a verdict
    /// is exactly the self-reported success `V-2` refuses.
    pub fn evidence(&self) -> String {
        let mut out = format!("red run: {}\n\n", self.verdict().describe());
        if let Some(stash) = &self.stash_ref {
            out.push_str(&format!("displaced work stashed at {stash}\n\n"));
        }
        out.push_str("--- without the change ---\n");
        out.push_str(&self.without.evidence());
        out.push_str("\n--- with the change ---\n");
        out.push_str(&self.with.evidence());
        out
    }
}

/// Did this change **add a test**? (`V-3`, `V-18`)
///
/// The trigger for a red run. `V-3` is about a new test, not about every edit,
/// so a refactor, a doc change and a config tweak are not worth a second gate
/// run — and a step that did add one is worth proving.
///
/// Added lines only. A diff that *removes* `#[test]` is `V-4`'s business, and
/// counting it here would run a red run to celebrate a deleted test.
///
/// Per language, and deliberately a list rather than a clever rule: there is no
/// syntax-independent way to recognise a test, and a rule that tried would be
/// wrong in both directions. An unusual framework is missed, which costs a red
/// run nobody ran; the alternative is firing on changes that added nothing.
pub fn adds_a_test(diff: &str) -> bool {
    const MARKERS: &[&str] = &[
        "#[test]",          // Rust
        "#[tokio::test]",   // Rust, async
        "def test_",        // Python
        "func Test",        // Go
        "@Test",            // Java, Kotlin
        "[Test]",           // C#, NUnit
        "[Fact]",           // C#, xUnit
        "it(",              // JS/TS, Jest and Mocha
        "test(",            // JS/TS
        "describe(",        // JS/TS
    ];
    diff.lines()
        .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
        .any(|line| {
            let text = line.trim_start_matches('+').trim();
            MARKERS.iter().any(|marker| text.contains(marker))
        })
}

// ── V-4 ────────────────────────────────────────────────────────────────────

pub mod tamper {
    //! Weakening a test to get past a gate (`V-4`).
    //!
    //! Reads a unified diff, because that is the artefact that exists at the
    //! moment the decision is made — before the commit, while the change is
    //! still reversible.

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Finding {
        pub kind: Kind,
        pub line: String,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        /// A whole test removed.
        TestDeleted,
        /// An assertion removed.
        AssertionRemoved,
        /// A test skipped rather than fixed.
        Skipped,
        /// `assert_eq!` softened to `assert!`, an exact match to a `contains`.
        AssertionWeakened,
    }

    impl Kind {
        pub fn describe(self) -> &'static str {
            match self {
                Kind::TestDeleted => "a test was deleted",
                Kind::AssertionRemoved => "an assertion was removed",
                Kind::Skipped => "a test was skipped instead of fixed",
                Kind::AssertionWeakened => "an assertion was weakened",
            }
        }
    }

    fn is_assertion(line: &str) -> bool {
        let trimmed = line.trim();
        trimmed.starts_with("assert")
            || trimmed.starts_with("expect_err")
            || trimmed.contains("assert_eq!")
            || trimmed.contains("assert_ne!")
    }

    /// Inspect a unified diff for the four ways a gate gets faked.
    ///
    /// Deliberately blunt: a false positive costs a sentence of explanation,
    /// and a false negative costs the entire point of the harness.
    pub fn inspect(diff: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut removed_assertions = 0usize;
        let mut added_assertions = 0usize;

        for raw in diff.lines() {
            if raw.starts_with("---") || raw.starts_with("+++") {
                continue;
            }
            if let Some(line) = raw.strip_prefix('-') {
                if line.trim_start().starts_with("#[test]") {
                    findings.push(Finding { kind: Kind::TestDeleted, line: line.trim().to_string() });
                }
                if is_assertion(line) {
                    removed_assertions += 1;
                }
            } else if let Some(line) = raw.strip_prefix('+') {
                let trimmed = line.trim_start();
                if trimmed.starts_with("#[ignore") || trimmed.contains("return; // skip") {
                    findings.push(Finding { kind: Kind::Skipped, line: line.trim().to_string() });
                }
                if is_assertion(line) {
                    added_assertions += 1;
                }
            }
        }

        if removed_assertions > added_assertions {
            findings.push(Finding {
                kind: Kind::AssertionRemoved,
                line: format!("{removed_assertions} removed, {added_assertions} added"),
            });
        }

        // An `assert_eq!` becoming an `assert!` keeps the count and loses the
        // check, so it needs its own look.
        let removed_eq = diff.lines().filter(|l| l.starts_with('-') && l.contains("assert_eq!")).count();
        let added_eq = diff.lines().filter(|l| l.starts_with('+') && l.contains("assert_eq!")).count();
        let added_loose = diff
            .lines()
            .filter(|l| l.starts_with('+') && l.contains("assert!") && !l.contains("assert_eq!"))
            .count();
        if removed_eq > added_eq && added_loose > 0 {
            findings.push(Finding {
                kind: Kind::AssertionWeakened,
                line: format!("{removed_eq} exact assertions became {added_loose} loose ones"),
            });
        }

        findings
    }

    /// Whether this diff may be part of a gate fix at all.
    pub fn allows_gate_fix(diff: &str) -> std::result::Result<(), Vec<Finding>> {
        let findings = inspect(diff);
        if findings.is_empty() {
            Ok(())
        } else {
            Err(findings)
        }
    }
}

// ── V-7, V-8 ───────────────────────────────────────────────────────────────

/// A requirement's status, derived rather than asserted (`V-7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Open,
    InProgress,
    Done,
    Blocked,
    Gated,
    Conflicting,
}

impl Marker {
    pub fn symbol(self) -> &'static str {
        match self {
            Marker::Open => "",
            Marker::InProgress => "🟡",
            Marker::Done => "✅",
            Marker::Blocked => "🚧",
            Marker::Gated => "⛔",
            Marker::Conflicting => "🔶",
        }
    }

    /// The word a person reads in a check's output. The symbol alone is a poor
    /// error message — `⛔` and `🚧` are one glyph apart in a terminal.
    pub fn name(self) -> &'static str {
        match self {
            Marker::Open => "open",
            Marker::InProgress => "in progress",
            Marker::Done => "done",
            Marker::Blocked => "blocked",
            Marker::Gated => "gated",
            Marker::Conflicting => "conflicting",
        }
    }

    /// Only these count as delivered. Gated is never one of them (`V-8`).
    pub fn counts_as_done(self) -> bool {
        self == Marker::Done
    }
}

/// Derive a requirement's marker from the journal (`V-7`).
///
/// The engine writes markers; the model proposes. A requirement is done when
/// an outcome record says so *and* carries evidence — a summary alone is a
/// claim, not a transcript.
/// A marker the journal does not support (`V-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disagreement {
    pub requirement: String,
    /// What the requirements source says.
    pub claimed: Marker,
    /// What the journal's records actually support.
    pub derived: Marker,
}

/// What checking every marker against the journal found (`V-7`).
///
/// The three buckets are kept apart on purpose, and `V-11` is why: a checker
/// that reports "no disagreements" while silently having checked nothing is
/// indistinguishable from one that checked everything and found it sound. What
/// was *not* checked, and why, is part of the answer rather than a footnote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkerCheck {
    pub checked: usize,
    pub disagreements: Vec<Disagreement>,
    /// The journal has no record citing these at all, so it has nothing to say
    /// about them. Not evidence of a wrong marker — evidence of a marker this
    /// engine cannot see the evidence for, which is the ordinary case for work
    /// done before `journal.jsonl` existed.
    pub unseen: Vec<String>,
    /// `⛔` and `🔶`. [`derive_marker`] has no path to either — they are a
    /// person's judgement about the world outside the repository, and a journal
    /// cannot confirm or refute one. Excluded rather than reported as wrong.
    pub not_derivable: Vec<String>,
}

impl MarkerCheck {
    pub fn is_clean(&self) -> bool {
        self.disagreements.is_empty()
    }

    pub fn describe(&self) -> String {
        let mut out = String::new();
        for entry in &self.disagreements {
            out.push_str(&format!(
                "  {} claims {} — the journal supports {}\n",
                entry.requirement,
                entry.claimed.name(),
                entry.derived.name(),
            ));
        }
        out.push_str(&format!(
            "\n{} checked, {} disagreeing, {} with no journal record, {} not derivable\n",
            self.checked,
            self.disagreements.len(),
            self.unseen.len(),
            self.not_derivable.len(),
        ));
        if !self.unseen.is_empty() {
            out.push_str(
                "A requirement with no journal record is not checked and not believed either — \
                 the engine has no evidence to read. Work predating `journal.jsonl` lands here.\n",
            );
        }
        out
    }
}

/// Check every claimed marker against what the journal supports (`V-7`).
///
/// **This does not write markers, and nothing here can.** `V-12` refuses the
/// requirements source to every writing tool, and `cycle.rs` says plainly that
/// the loop may not set a `✅` — a loop that awards itself one is a loop whose
/// status is worth nothing. `binding.md` already asked for exactly this
/// instead: *"a marker without a matching journal entry is not believed"*, and
/// a reconcile step that checks markers against the evidence. A person still
/// marks; this says when the file and the journal disagree.
pub fn check_markers(claimed: &[(String, Marker)], records: &[Record]) -> MarkerCheck {
    let mut out = MarkerCheck::default();
    for (requirement, claimed) in claimed {
        if matches!(claimed, Marker::Gated | Marker::Conflicting) {
            out.not_derivable.push(requirement.clone());
            continue;
        }
        let seen = records.iter().any(|r| r.requirements.iter().any(|id| id == requirement));
        if !seen {
            out.unseen.push(requirement.clone());
            continue;
        }
        out.checked += 1;
        let derived = derive_marker(records, requirement);
        if derived != *claimed {
            out.disagreements.push(Disagreement {
                requirement: requirement.clone(),
                claimed: *claimed,
                derived,
            });
        }
    }
    out
}

pub fn derive_marker(records: &[Record], requirement: &str) -> Marker {
    let mine: Vec<&Record> = records
        .iter()
        .filter(|record| record.requirements.iter().any(|id| id == requirement))
        .collect();

    if mine.is_empty() {
        return Marker::Open;
    }
    // The *last* outcome decides, not any outcome ever.
    //
    // This read `any(ok == false)`, so a requirement that failed in one batch
    // and passed in a later one was blocked for good — the journal is
    // append-only (`L-3`), so the failure never stops being in it. Measured on
    // this repository the moment `check_markers` gave the function its first
    // caller: `V-11` and `T-19` both run `false, true, true, false, true` and
    // both came back blocked, which is a fair description of neither.
    //
    // A defect that could not show up while nothing called this, which is the
    // argument the reachability list has been making all along.
    let Some(last) = mine.iter().rev().find(|r| r.kind == Kind::Outcome) else {
        // Intents and nothing else: started, never closed.
        return Marker::InProgress;
    };
    if last.ok == Some(false) {
        return Marker::Blocked;
    }
    // `V-2`: green is not enough on its own — an outcome that carries no
    // transcript is a claim, and a claim is not evidence.
    let delivered = mine.iter().any(|record| {
        record.kind == Kind::Outcome
            && record.ok == Some(true)
            && record.detail.as_deref().is_some_and(|d| !d.trim().is_empty())
    });
    if delivered {
        Marker::Done
    } else {
        Marker::InProgress
    }
}

/// What a batch actually delivered (`V-8`).
///
/// Gated and conflicting are carried in their own columns, forever. A batch
/// reporting "8 of 8" while three of them are waiting on a human is the
/// specific dishonesty this type exists to make impossible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub open: usize,
    pub in_progress: usize,
    pub done: usize,
    pub blocked: usize,
    pub gated: usize,
    pub conflicting: usize,
}

impl Counts {
    pub fn of(markers: &[Marker]) -> Counts {
        let mut counts = Counts::default();
        for marker in markers {
            match marker {
                Marker::Open => counts.open += 1,
                Marker::InProgress => counts.in_progress += 1,
                Marker::Done => counts.done += 1,
                Marker::Blocked => counts.blocked += 1,
                Marker::Gated => counts.gated += 1,
                Marker::Conflicting => counts.conflicting += 1,
            }
        }
        counts
    }

    pub fn total(&self) -> usize {
        self.open + self.in_progress + self.done + self.blocked + self.gated + self.conflicting
    }

    /// Never `done / total` — that would let a gated item drift into the
    /// numerator the moment someone rounds.
    pub fn describe(&self) -> String {
        let mut out = format!("{} of {} delivered", self.done, self.total());
        for (n, label) in [
            (self.in_progress, "in progress"),
            (self.blocked, "blocked"),
            (self.gated, "gated"),
            (self.conflicting, "conflicting"),
        ] {
            if n > 0 {
                out.push_str(&format!(", {n} {label}"));
            }
        }
        out
    }
}

// ── V-9 ────────────────────────────────────────────────────────────────────

/// An id used somewhere it was not defined (`V-9`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StrayId {
    pub id: String,
    pub file: String,
}

/// Ids are minted only in the requirements source (Perpetum 0.8).
///
/// Everything else cites. A batch file that invents `L-99` produces a plan
/// referring to a requirement that does not exist, and nobody notices until the
/// cycle that tries to build it.
/// Does this text introduce something shaped like a requirement id (`V-9`)?
///
/// Used to keep `note` (`T-23`) from becoming a side door into the backlog. A
/// note is for describing a problem; filing the requirement is a person's job,
/// and a loop that could mint ids in passing would have a second source of them
/// — which is the objection `V-9` makes and `L-22` makes about step ids.
///
/// Answers on shape alone and does not consult the source, because the point is
/// to refuse the *act*: citing `V-4` in a note is fine and normal, and this
/// only fires on a bare `X-123` written as a declaration would write it.
pub fn looks_like_new_id(text: &str) -> bool {
    ids_in(text).iter().any(|id| {
        // A citation reads "as `V-4` says"; a minting reads "V-99: the parser
        // should …". The colon after it is the tell.
        text.contains(&format!("{id}:")) || text.contains(&format!("{id} —"))
    })
}

pub fn stray_ids(source: &str, others: &[(&str, &str)]) -> Vec<StrayId> {
    let defined = ids_in(source);
    let mut stray = Vec::new();
    for (name, text) in others {
        for id in ids_in(text) {
            if !defined.contains(&id) {
                stray.push(StrayId { id, file: (*name).to_string() });
            }
        }
    }
    stray.sort();
    stray.dedup();
    stray
}

/// Every `X-9`-shaped id in a document, wherever it appears.
fn ids_in(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_uppercase() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_uppercase() {
                i += 1;
            }
            let letters: String = bytes[start..i].iter().collect();
            if i < bytes.len() && bytes[i] == '-' && letters.len() <= 2 {
                let dash = i;
                i += 1;
                let digits_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i > digits_start {
                    let digits: String = bytes[digits_start..i].iter().collect();
                    found.insert(format!("{letters}-{digits}"));
                    continue;
                }
                i = dash;
            }
            continue;
        }
        i += 1;
    }
    found
}

// ── V-15 ───────────────────────────────────────────────────────────────────

/// A requirement a change claimed, with no test standing under it (`V-15`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unbacked {
    pub id: String,
    pub file: String,
}

/// Citations a change adds without adding a test that cites the same id
/// (`V-15`).
///
/// Cycle 9 closed `X-13` with nine lines of prose: a module-doc paragraph
/// asserting `shell` was confined to the workspace, and `X-13` added to the
/// file's requirement header. No code, no test. Every gate went green because a
/// false docstring compiles, and `perp check ids` passed because the id it
/// cited is defined. The hole read as sealed in the one place a reader looks.
/// `V-13` could not catch it — the step *had* written, and what it wrote was
/// the description of the change it had not made.
///
/// Read from the diff rather than the tree, because the question is what this
/// change claimed and not what the file already said. Ids in removed lines are
/// ignored for the same reason: moving a citation is not making one.
///
/// Only ids in `open` are judged, and that is what makes this usable rather
/// than merely correct. Prose cites requirements constantly as *reasons* —
/// "`X-2` checks the `path` argument", "`G-3` refuses `git add .`" — and a rule
/// that reads those as claims reports three lies for every real one. Run over
/// this session's own commits it did exactly that. Citing a requirement already
/// marked done is a reference to behaviour that exists; citing an open one is a
/// claim to have built it, and only the second kind needs a test underneath it.
///
/// **What this proves and what it does not.** It is structural: it says a new
/// citation arrived with a test that names it. Whether that test would fail
/// without the change is [`RedRun`]'s question, and `V-3` already owns it. A
/// test written to name an id and assert nothing satisfies this and not `V-3`,
/// which is the honest division — one of them reads the diff, and the other has
/// to run the suite twice.
pub fn unbacked_citations(diff: &str, open: &BTreeSet<String>) -> Vec<Unbacked> {
    // Claims are remembered per file, so a report can say where; tests and
    // prior citations count across the whole change. Judging each file alone
    // was the first shape and it was wrong — an implementation is routinely
    // split, and this rule's own wiring lives in `main.rs` while its tests live
    // in `verify.rs`, which it duly reported as a lie. Requiring the test to
    // name the same id is what stops a claim riding on an unrelated test; the
    // file boundary was never what did that.
    let mut claims: Vec<(String, BTreeSet<String>)> = Vec::new();
    let mut file = String::new();
    let mut state = Judging::default();
    let mut tested: BTreeSet<String> = BTreeSet::new();
    let mut already: BTreeSet<String> = BTreeSet::new();

    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            state.harvest(&mut claims, &file, &mut tested, &mut already);
            file = path.trim().to_string();
            state = Judging::default();
            // Only where a test could live. Two separate failures argued for
            // this and neither was predicted.
            //
            // Prose is not a claim about behaviour, and the requirements source
            // least of all: `V-9` makes it the one place ids are *minted*, so
            // every new requirement arrives as an id nothing implements yet.
            // The first run reported the commit that filed `T-20` and `M-30` as
            // two lies, which is what defining a requirement looks like to a
            // checker that cannot tell a definition from an assertion.
            //
            // Worse, and quieter: the journal is a transcript of runs, and a
            // run's output contains the literal `#[test]`. Reading it flipped
            // this into "inside a test" and put every id after that into the
            // backed set — so a change carrying a journal entry could clear any
            // claim at all. A checker that reads its own logs believes them.
            if !file.ends_with(".rs") {
                file.clear();
            }
            continue;
        }
        if line.starts_with("---") || line.starts_with("+++") {
            continue;
        }
        // A citation the file already had is not a new claim. Editing a module
        // header re-adds every id on the line, and requiring a fresh test for
        // each would report a rename as a lie.
        if let Some(removed) = line.strip_prefix('-') {
            state.already.extend(ids_in(removed));
            continue;
        }
        let Some(added) = line.strip_prefix('+') else { continue };
        state.saw(added);
    }
    state.harvest(&mut claims, &file, &mut tested, &mut already);

    let mut unbacked = Vec::new();
    for (file, claimed) in claims {
        for id in claimed.difference(&tested) {
            if already.contains(id) || !open.contains(id) {
                continue;
            }
            unbacked.push(Unbacked { id: id.clone(), file: file.clone() });
        }
    }
    unbacked.sort();
    unbacked.dedup();
    unbacked
}

/// One file's worth of added lines, part-read.
#[derive(Default)]
struct Judging {
    /// Ids this change asserts.
    claimed: BTreeSet<String>,
    /// Ids a test in this change names.
    tested: BTreeSet<String>,
    /// Ids the file already cited before this change.
    already: BTreeSet<String>,
    /// A `///` block waiting to find out what it is attached to.
    ///
    /// The id a test is *about* is almost always in the doc comment above it
    /// rather than in its body — every test in this crate is written that way —
    /// so a scanner that only reads from `#[test]` downward finds nothing and
    /// calls every honest test a lie.
    pending: BTreeSet<String>,
    in_test: bool,
}

impl Judging {
    fn saw(&mut self, added: &str) {
        let text = added.trim();
        if text.contains("#[test]") {
            // The doc block above it was describing this test after all.
            self.tested.append(&mut self.pending);
            self.in_test = true;
            return;
        }
        let ids = ids_in(added);
        if self.in_test {
            self.tested.extend(ids);
            return;
        }
        if text.starts_with("///") {
            self.pending.extend(ids);
            return;
        }
        // Anything else ends the block: a doc comment attaches to the item
        // directly beneath it, and this is not one.
        self.claimed.append(&mut self.pending);
        self.claimed.extend(ids);
    }

    /// Hand this file's claims to the caller, and its tests and prior citations
    /// to the change-wide sets.
    fn harvest(
        &mut self,
        claims: &mut Vec<(String, BTreeSet<String>)>,
        file: &str,
        tested: &mut BTreeSet<String>,
        already: &mut BTreeSet<String>,
    ) {
        if file.is_empty() {
            return;
        }
        self.claimed.append(&mut self.pending);
        tested.append(&mut self.tested);
        already.append(&mut self.already);
        claims.push((file.to_string(), std::mem::take(&mut self.claimed)));
    }
}

/// Ids defined in the requirements source, for a caller that wants the set.
pub fn defined_ids(source: &str) -> Result<BTreeSet<String>> {
    let ids = ids_in(source);
    if ids.is_empty() {
        return Err(Error::unbound(
            "requirements",
            "the source defines no ids — that cannot be right",
        ));
    }
    Ok(ids)
}

/// Whether a review counts (`V-5`).
///
/// **The verifier must resolve to a different link than the one that authored
/// the change.** Self-review by the same model on the same context is not
/// review: it is the same distribution sampled twice, and it agrees with itself
/// for the same reasons it was wrong the first time.
///
/// Enforced by comparing the link the author used — read from the journal, not
/// asserted — against the link the verifier role resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Independence {
    /// Different link. The review counts.
    Independent { author: String, verifier: String },
    /// Same link. It does not.
    SameLink { link: String },
    /// Nothing in the journal says who authored it, so independence cannot be
    /// claimed. Reported as unknown rather than assumed either way.
    AuthorUnknown,
}

impl Independence {
    pub fn counts(&self) -> bool {
        matches!(self, Independence::Independent { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Independence::Independent { author, verifier } => {
                format!("{verifier} reviewed what {author} wrote")
            }
            Independence::SameLink { link } => format!(
                "{link} would be reviewing its own work. Self-review by the same model on the                  same context is not review (`V-5`)"
            ),
            Independence::AuthorUnknown => {
                "no journalled call says which link authored this, so the review cannot claim                  independence (`V-5`)"
                    .into()
            }
        }
    }
}

/// Which link authored a step, from the journal (`M-10`).
pub fn author_of(step: &crate::step::StepId, records: &[Record]) -> Option<String> {
    records
        .iter()
        .filter(|record| record.step == *step)
        .find_map(|record| crate::cost::from_record(record).map(|entry| entry.link))
}

/// Check a proposed review before it is run, so a pointless call is not paid
/// for (`V-5`).
pub fn independence(
    step: &crate::step::StepId,
    verifier_link: &str,
    records: &[Record],
) -> Independence {
    match author_of(step, records) {
        None => Independence::AuthorUnknown,
        Some(author) if author == verifier_link => Independence::SameLink { link: author },
        Some(author) => {
            Independence::Independent { author, verifier: verifier_link.to_string() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The open backlog, for the tests below.
    fn open(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    /// A real repository with one committed file, for the red run.
    #[allow(clippy::expect_used)]
    fn red_repo(tag: &str, committed: &str) -> (Repo, std::path::PathBuf) {
        let root = crate::testutil::tmpdir(tag);
        let repo = Repo::at(&root);
        for args in [
            vec!["init", "-q", "-b", "perp/fixture"],
            vec!["config", "user.email", "loop@perpetum.test"],
            vec!["config", "user.name", "Perpetum test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            repo.run_unchecked(&args).expect("git");
        }
        std::fs::write(root.join("marker.txt"), committed).expect("write");
        repo.stage(&["marker.txt"]).expect("stage");
        repo.commit(&crate::git::CommitMessage::new("Add the marker")).expect("commit");
        (repo, root)
    }

    /// A gate that passes only when `marker.txt` contains `new`.
    fn looks_for_new(root: &std::path::Path) -> crate::gate::Gate {
        let command = if cfg!(windows) {
            "cmd /C \"findstr new marker.txt\""
        } else {
            "sh -c \"grep new marker.txt\""
        };
        crate::gate::Gate {
            name: "check".into(),
            command: command.into(),
            cwd: root.to_path_buf(),
            timeout: std::time::Duration::from_secs(60),
            runtime: crate::runtime::Runtime::Host,
            lends: Vec::new(),
            offline: false,
        }
    }

    /// `V-18`: the trigger. A red run costs a second gate run, so it fires
    /// when a test was added and not on every edit.
    #[test]
    fn a_diff_that_adds_a_test_is_what_triggers_a_red_run() {
        let added = "\
--- a/src/lib.rs
+++ b/src/lib.rs
+    #[test]
+    fn a_thing_works() {
+        assert!(true);
+    }
";
        assert!(adds_a_test(added));

        // A refactor is not worth a second gate run.
        let refactor = "\
--- a/src/lib.rs
+++ b/src/lib.rs
-    let x = compute();
+    let x = compute_faster();
";
        assert!(!adds_a_test(refactor));

        // `V-4`'s business, not this one. Firing here would run a red run to
        // celebrate a deleted test.
        let removed = "\
--- a/src/lib.rs
+++ b/src/lib.rs
-    #[test]
-    fn a_thing_works() {}
";
        assert!(!adds_a_test(removed), "a removed test is not an added one");
    }

    /// The `+++` header names a file and is not an added line. Without the
    /// guard, editing any path containing `test` would trigger on every diff.
    #[test]
    fn the_diff_header_is_not_mistaken_for_an_added_test() {
        let header_only = "\
--- a/tests/it(works).rs
+++ b/tests/it(works).rs
-    let x = 1;
+    let x = 2;
";
        assert!(!adds_a_test(header_only), "the `+++` line is a header, not a change");
    }

    #[test]
    fn the_common_frameworks_are_recognised() {
        for line in [
            "+#[test]",
            "+    #[tokio::test]",
            "+def test_it_works():",
            "+func TestThing(t *testing.T) {",
            "+    @Test",
            "+    it('works', () => {",
        ] {
            assert!(adds_a_test(line), "{line}");
        }
    }

    /// `V-3`: the red run, actually run.
    ///
    /// Red without the change, green with it, against a real repository and a
    /// real gate. Nothing built one of these before, so a test that could not
    /// fail passed the gate exactly like one that earns its place.
    #[test]
    fn a_test_that_earns_its_place_fails_without_the_change_and_passes_with_it() {
        let (repo, root) = red_repo("verify-red-earned", "old\n");
        // The change, uncommitted — which is where a batch's edits live.
        std::fs::write(root.join("marker.txt"), "new\n").expect("write");

        let red = RedRun::perform(&repo, &looks_for_new(&root), "HEAD").expect("red run");

        assert_eq!(red.verdict(), RedVerdict::Earned, "{}", red.evidence());
        assert!(!red.without.is_green(), "the tree without the change must fail");
        assert!(red.with.is_green(), "and the tree with it must pass");
        // `V-2`: both transcripts are kept, not just the verdict.
        assert!(red.evidence().contains("without the change"), "{}", red.evidence());
        assert!(red.evidence().contains("with the change"), "{}", red.evidence());

        // The live tree is untouched — no stash, nothing to restore.
        assert_eq!(
            std::fs::read_to_string(root.join("marker.txt")).expect("read"),
            "new\n",
            "the working tree must survive the red run unchanged"
        );
    }

    /// `V-18`: work **committed** since the baseline is still the change.
    ///
    /// The bug this pins, found on Janitor's first real batch. A step commits
    /// what it touched as it goes (`T-22`), so passing `HEAD` compared the work
    /// against itself: same code both runs, both green, `ProvesNothing` — and
    /// eleven real tests written off as proving nothing. The baseline has to be
    /// where the batch *started*.
    #[test]
    fn a_change_already_committed_is_still_measured_against_where_work_began() {
        let (repo, root) = red_repo("verify-red-committed", "old\n");
        let base = repo.head_sha().expect("the starting sha");

        // The work, committed — which is what a checkpointing step leaves.
        std::fs::write(root.join("marker.txt"), "new\n").expect("write");
        repo.stage(&["marker.txt"]).expect("stage");
        repo.commit(&crate::git::CommitMessage::new("Do the work")).expect("commit");

        // Against where the batch began: red without it, green with it.
        let red = RedRun::perform(&repo, &looks_for_new(&root), &base).expect("red run");
        assert_eq!(red.verdict(), RedVerdict::Earned, "{}", red.evidence());

        // And against `HEAD` — the old behaviour — the same work proves
        // nothing, because `HEAD` is now the work.
        let against_head = RedRun::perform(&repo, &looks_for_new(&root), "HEAD").expect("red run");
        assert_eq!(
            against_head.verdict(),
            RedVerdict::NotActuallyChanged,
            "comparing committed work against HEAD compares it with itself"
        );
    }

    /// The lie the mechanism exists for: a test that passes either way.
    #[test]
    fn a_test_that_cannot_fail_proves_nothing() {
        let (repo, root) = red_repo("verify-red-useless", "new\n");
        // A change that has nothing to do with what the gate checks.
        std::fs::write(root.join("unrelated.txt"), "whatever\n").expect("write");

        let red = RedRun::perform(&repo, &looks_for_new(&root), "HEAD").expect("red run");

        assert_eq!(red.verdict(), RedVerdict::ProvesNothing, "{}", red.evidence());
        assert!(!red.verdict().is_earned(), "and it does not satisfy gate 4");
    }

    /// `V-10`: a step that changed nothing gets a meaningless result and is
    /// told so, rather than handed a green.
    #[test]
    fn a_red_run_over_an_unchanged_tree_is_meaningless() {
        let (repo, root) = red_repo("verify-red-unchanged", "new\n");

        let red = RedRun::perform(&repo, &looks_for_new(&root), "HEAD").expect("red run");

        assert_eq!(red.verdict(), RedVerdict::NotActuallyChanged, "{}", red.evidence());
    }

    /// `V-15`, on the change that argued for it.
    ///
    /// This is cycle 9's `X-13` diff, shortened but not otherwise altered: a
    /// module header gaining an id, a doc paragraph describing a guard, and no
    /// code and no test anywhere. It passed every gate at the time.
    #[test]
    fn a_citation_added_with_only_prose_is_unbacked() {
        let diff = "\
--- a/crates/perp-core/src/tool.rs
+++ b/crates/perp-core/src/tool.rs
-//! The tool host (`T-1`, `T-2`, `T-5`).
+//! The tool host (`T-1`, `T-2`, `T-5`, `X-13`).
+//! Only `X-13` is new here; the rest the file already cited.
+//! - **`shell` is confined to the workspace too** (`X-13`). Every token that
+//!   looks like a path is resolved and refused if it lands outside the root.
";
        let found = unbacked_citations(diff, &open(&["X-13"]));
        let ids: Vec<&str> = found.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, vec!["X-13"], "only the new claim, not the ones it already made");
        assert_eq!(found[0].file, "crates/perp-core/src/tool.rs");
    }

    /// And the same claim with a test under it is not reported.
    #[test]
    fn a_citation_with_a_test_naming_it_is_backed() {
        let diff = "\
--- a/crates/perp-core/src/tool.rs
+++ b/crates/perp-core/src/tool.rs
-//! The tool host (`T-1`).
+//! The tool host (`T-1`, `X-13`).
+    fn confined(&self, command: &str) -> Result<()> {
+        Ok(())
+    }
+    /// `X-13`: the boundary reaches inside a command line.
+    #[test]
+    fn a_shell_command_may_not_reach_outside_the_workspace() {
+        assert!(host.confined(\"cat /etc/passwd\").is_err());
+    }
";
        assert_eq!(unbacked_citations(diff, &open(&["X-13"])), vec![], "a test names it, so it stands");
    }

    /// A test naming a *different* requirement does not back this one.
    #[test]
    fn a_test_for_another_requirement_does_not_back_the_claim() {
        let diff = "\
--- a/src/a.rs
+++ b/src/a.rs
+//! Now also does `X-13`.
+    #[test]
+    fn something_about_t_2() {
+        // `T-2` is what this checks.
+    }
";
        let ids: Vec<String> =
            unbacked_citations(diff, &open(&["X-13"])).into_iter().map(|u| u.id).collect();
        assert_eq!(ids, vec!["X-13".to_string()]);
    }

    /// Defining a requirement is not claiming to have built it.
    ///
    /// `V-9` makes the requirements source the one place ids are minted, so a
    /// new row is always an id nothing implements yet. The first run of this
    /// rule reported the very commit that filed `T-20` and `M-30` as two lies.
    #[test]
    fn a_requirement_being_defined_is_not_a_claim() {
        let diff = "--- a/.harness/perpetum.md
+++ b/.harness/perpetum.md
+| `T-20` | A caller that needs a command's whole output can get it. |
+| `M-30` | The concurrency bound holds on every path that reaches a link. |
";
        assert_eq!(unbacked_citations(diff, &open(&["T-20", "M-30"])), vec![]);
    }

    /// A transcript that quotes `#[test]` does not make a claim true.
    ///
    /// The journal records what runs printed, and a run prints test names. The
    /// probe against real commits caught this: reading `journal.jsonl` flipped
    /// the scanner into "inside a test", and every id after it went into the
    /// backed set — so a change that happened to carry a journal entry could
    /// clear any claim at all. Only `.rs` is read now.
    #[test]
    fn a_journal_quoting_a_test_does_not_back_anything() {
        let diff = "--- a/crates/perp-core/src/thing.rs
+++ b/crates/perp-core/src/thing.rs
+//! Implements `L-24`.
--- a/.harness/journal.jsonl
+++ b/.harness/journal.jsonl
+{\"detail\":\"running #[test] about `L-24` ... ok\"}
";
        let ids: Vec<String> =
            unbacked_citations(diff, &open(&["L-24"])).into_iter().map(|u| u.id).collect();
        assert_eq!(ids, vec!["L-24".to_string()], "a log is not a test");
    }

    /// A change is judged whole, not file by file.
    ///
    /// Per-file was the first shape, and this rule's own wiring disproved it:
    /// the CLI half lives in `main.rs` and the tests in `verify.rs`, and a
    /// per-file reading reported that as a lie. What stops a claim riding on an
    /// unrelated test is that the test must name the same id — the file
    /// boundary was never doing that work.
    #[test]
    fn a_test_in_another_file_still_backs_the_claim() {
        let diff = "\
--- a/src/claim.rs
+++ b/src/claim.rs
+//! Implements `L-24`.
--- a/src/other.rs
+++ b/src/other.rs
+    /// `L-24` is what this checks.
+    #[test]
+    fn about_the_watchdog() {
+        assert!(true);
+    }
";
        assert_eq!(unbacked_citations(diff, &open(&["L-24"])), vec![]);
    }

    /// Removing a citation is not making one.
    #[test]
    fn a_removed_citation_is_not_a_claim() {
        let diff = "\
--- a/src/a.rs
+++ b/src/a.rs
-//! Implements `X-13`.
+//! Implements nothing in particular.
";
        assert_eq!(unbacked_citations(diff, &open(&["X-13"])), vec![]);
    }

    #[test]
    fn a_review_by_the_same_link_is_not_a_review() {
        // `V-5`. The same model on the same context is the same distribution
        // sampled twice — it agrees with itself for the same reasons it was
        // wrong the first time.
        let step = crate::step::StepId::new(4, "b19", 1).expect("step");
        let entry = crate::cost::Entry {
            step: step.to_string(),
            role: "coder".into(),
            link: "here".into(),
            model: "small".into(),
            usage: crate::cost::Usage::from_reply(10, 5, 0, 0),
            latency_ms: 10,
            charge: 0.0,
        };
        let records =
            vec![crate::cost::annotate(Record::outcome(step.clone(), 100, true, "wrote it"), &entry)];

        let same = independence(&step, "here", &records);
        assert!(!same.counts());
        assert!(same.describe().contains("V-5"), "{}", same.describe());

        let different = independence(&step, "ds-fast", &records);
        assert!(different.counts());
        assert!(different.describe().contains("ds-fast reviewed what here wrote"));
    }

    #[test]
    fn independence_cannot_be_claimed_when_the_author_is_unknown() {
        // Reported as unknown rather than assumed either way. Assuming
        // independent would let an unjournalled call launder a self-review.
        let step = crate::step::StepId::new(4, "b19", 2).expect("step");
        let records = vec![Record::outcome(step.clone(), 100, true, "no call recorded")];
        let verdict = independence(&step, "ds-fast", &records);
        assert_eq!(verdict, Independence::AuthorUnknown);
        assert!(!verdict.counts(), "unknown is not independent");
    }
    use crate::gate::GateResult;
    use crate::process::{Exit, Run};
    use crate::step::StepId;
    use std::path::PathBuf;

    fn gate(name: &str, code: i32) -> GateResult {
        GateResult {
            runtime: "host".to_string(),
            name: name.to_string(),
            run: Run {
                command: "cargo test".into(),
                cwd: PathBuf::from("."),
                env: "declared".into(),
                exit: Exit::Code(code),
                duration_ms: 10,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                truncated: false,
                stdout_truncated: false,
                stderr_truncated: false,
            },
            sha: None,
            dirty: None,
        }
    }

    fn step(text: &str) -> StepId {
        StepId::parse(text).expect("step")
    }

    // ── V-3 ────────────────────────────────────────────────────────────────

    #[test]
    fn a_test_that_fails_without_the_change_has_earned_its_place() {
        let run = RedRun::new(gate("test", 101), gate("test", 0), b"before", b"after");
        assert_eq!(run.verdict(), RedVerdict::Earned);
        assert!(run.evidence().contains("failed without the change"));
    }

    #[test]
    fn a_test_that_passes_both_ways_proves_nothing() {
        let run = RedRun::new(gate("test", 0), gate("test", 0), b"before", b"after");
        assert_eq!(run.verdict(), RedVerdict::ProvesNothing);
        assert!(!run.verdict().is_earned());
    }

    #[test]
    fn a_test_that_fails_both_ways_means_the_change_did_not_fix_it() {
        let run = RedRun::new(gate("test", 101), gate("test", 101), b"before", b"after");
        assert_eq!(run.verdict(), RedVerdict::StillBroken);
    }

    #[test]
    fn an_identical_tree_makes_the_red_run_meaningless() {
        // `V-10`, learned in batch 3: a mutation that did not apply reports a
        // passing test that was never challenged.
        let run = RedRun::new(gate("test", 101), gate("test", 0), b"same", b"same");
        assert_eq!(run.verdict(), RedVerdict::NotActuallyChanged);
        assert!(!run.verdict().is_earned(), "and it must not count as earned");
    }

    #[test]
    fn the_red_run_carries_both_transcripts() {
        let run = RedRun::new(gate("test", 101), gate("test", 0), b"a", b"b")
            .stashed_at("abc123");
        let evidence = run.evidence();
        assert!(evidence.contains("without the change"));
        assert!(evidence.contains("with the change"));
        assert!(evidence.contains("abc123"), "the displaced work is named");
    }

    // ── V-4 ────────────────────────────────────────────────────────────────

    #[test]
    fn deleting_a_test_during_a_gate_fix_is_caught() {
        let diff = "\
--- a/src/lib.rs
+++ b/src/lib.rs
-    #[test]
-    fn the_inconvenient_one() {
-        assert_eq!(compute(), 42);
-    }
";
        let findings = tamper::inspect(diff);
        assert!(findings.iter().any(|f| f.kind == tamper::Kind::TestDeleted), "{findings:?}");
        assert!(tamper::allows_gate_fix(diff).is_err());
    }

    #[test]
    fn skipping_a_test_instead_of_fixing_it_is_caught() {
        let diff = "\
+    #[ignore = \"flaky\"]
     #[test]
     fn the_inconvenient_one() {
";
        assert!(tamper::inspect(diff).iter().any(|f| f.kind == tamper::Kind::Skipped));
    }

    #[test]
    fn quietly_dropping_assertions_is_caught() {
        let diff = "\
     fn still_here() {
-        assert_eq!(compute(), 42);
-        assert!(other());
+        compute();
     }
";
        assert!(tamper::inspect(diff)
            .iter()
            .any(|f| f.kind == tamper::Kind::AssertionRemoved));
    }

    #[test]
    fn softening_an_exact_assertion_is_caught() {
        let diff = "\
-        assert_eq!(render(&state), expected);
+        assert!(render(&state).contains(\"some\"));
";
        assert!(tamper::inspect(diff)
            .iter()
            .any(|f| f.kind == tamper::Kind::AssertionWeakened));
    }

    #[test]
    fn ordinary_work_is_not_tampering() {
        let diff = "\
--- a/src/lib.rs
+++ b/src/lib.rs
     #[test]
     fn a_new_one() {
+        assert_eq!(compute(), 42);
+        assert!(other());
     }
";
        assert!(tamper::allows_gate_fix(diff).is_ok(), "{:?}", tamper::inspect(diff));
    }

    // ── V-7, V-8 ───────────────────────────────────────────────────────────

    #[test]
    fn a_requirement_nobody_touched_is_open() {
        assert_eq!(derive_marker(&[], "L-3"), Marker::Open);
    }

    #[test]
    fn a_claim_without_evidence_is_not_done() {
        // `V-7`: the engine writes markers from evidence. An outcome that says
        // "green" and carries no transcript is a claim.
        let records = vec![
            Record::outcome(step("c1/b1/s01"), 1, true, "implemented it, honest")
                .for_requirements(["L-3"]),
        ];
        assert_eq!(derive_marker(&records, "L-3"), Marker::InProgress);
    }

    #[test]
    fn an_outcome_with_a_transcript_is_done() {
        let records = vec![
            Record::outcome(step("c1/b1/s01"), 1, true, "gate test — green")
                .for_requirements(["L-3"])
                .with_detail("$ cargo test\nexit 0"),
        ];
        assert_eq!(derive_marker(&records, "L-3"), Marker::Done);
    }

    #[test]
    fn a_failure_anywhere_blocks_the_requirement() {
        let records = vec![
            Record::outcome(step("c1/b1/s01"), 1, true, "green").for_requirements(["L-3"])
                .with_detail("$ cargo test\nexit 0"),
            Record::outcome(step("c1/b1/s02"), 2, false, "gate build — exit 101")
                .for_requirements(["L-3"])
                .with_detail("error: it does not compile"),
        ];
        assert_eq!(derive_marker(&records, "L-3"), Marker::Blocked);
    }

    /// `V-7`: a requirement that failed and was then fixed is not blocked
    /// forever.
    ///
    /// The journal is append-only (`L-3`), so the failure never stops being in
    /// it — and the derivation read "any outcome ever failed". Measured on this
    /// repository the moment `check_markers` gave the function its first
    /// caller: `V-11` and `T-19` both run `false, true, true, false, true` and
    /// both came back blocked. Fifteen of thirty markers were flagged on this
    /// alone.
    #[test]
    fn the_last_outcome_decides_not_any_outcome_ever() {
        let step = |n: u32| StepId::new(1, "b1", n).expect("step");
        let records = vec![
            Record::outcome(step(1), 100, false, "gate red")
                .for_requirements(["L-3"])
                .with_detail("error: it does not compile"),
            Record::outcome(step(2), 200, true, "gate green")
                .for_requirements(["L-3"])
                .with_detail("$ cargo test\nexit 0"),
        ];
        assert_eq!(
            derive_marker(&records, "L-3"),
            Marker::Done,
            "fixed is fixed; the failure stays in the journal but stops being the answer"
        );

        // And the other order still blocks — this is about recency, not about
        // preferring good news.
        let regressed = vec![records[1].clone(), records[0].clone()];
        assert_eq!(derive_marker(&regressed, "L-3"), Marker::Blocked);
    }

    /// `V-7`: the reconcile `binding.md` always specified — check, never write.
    #[test]
    fn a_marker_the_journal_cannot_back_is_reported_and_the_rest_bucketed() {
        let step = |n: u32| StepId::new(1, "b1", n).expect("step");
        let records = vec![
            // Claimed done, and the journal agrees.
            Record::outcome(step(1), 100, true, "did it")
                .for_requirements(["L-3"])
                .with_detail("$ cargo test\nexit 0"),
            // Claimed done, but the outcome carries no transcript — a claim,
            // not evidence (`V-2`).
            Record::outcome(step(2), 200, true, "closed by the operator")
                .for_requirements(["L-4"]),
        ];
        let claimed = vec![
            ("L-3".to_string(), Marker::Done),
            ("L-4".to_string(), Marker::Done),
            ("M-25".to_string(), Marker::Gated),
            ("T-99".to_string(), Marker::Done),
        ];

        let check = check_markers(&claimed, &records);

        assert_eq!(check.checked, 2, "only what the journal mentions is checked");
        assert_eq!(check.disagreements.len(), 1);
        assert_eq!(check.disagreements[0].requirement, "L-4");
        assert_eq!(check.disagreements[0].claimed, Marker::Done);
        assert_eq!(check.disagreements[0].derived, Marker::InProgress);
        // A journal cannot confirm or refute a judgement about the outside
        // world, so it is excluded rather than reported as wrong.
        assert_eq!(check.not_derivable, vec!["M-25".to_string()]);
        // Work the engine has no evidence for is not checked, and not called
        // wrong either.
        assert_eq!(check.unseen, vec!["T-99".to_string()]);
        assert!(!check.is_clean());

        // `V-11`: what was *not* checked is part of the answer. A checker that
        // reports nothing wrong while having checked nothing looks identical to
        // one that checked everything.
        let described = check.describe();
        assert!(described.contains("2 checked"), "{described}");
        assert!(described.contains("1 with no journal record"), "{described}");
        assert!(described.contains("1 not derivable"), "{described}");
    }

    #[test]
    fn gated_is_never_counted_as_done() {
        // `V-8`: "8 of 8" while three wait on a human is the specific lie.
        let counts = Counts::of(&[
            Marker::Done,
            Marker::Done,
            Marker::Gated,
            Marker::Conflicting,
            Marker::InProgress,
        ]);
        assert_eq!(counts.done, 2);
        assert_eq!(counts.total(), 5);
        assert!(!Marker::Gated.counts_as_done());
        assert!(!Marker::Conflicting.counts_as_done());

        let text = counts.describe();
        assert!(text.starts_with("2 of 5 delivered"), "{text}");
        assert!(text.contains("1 gated"), "{text}");
        assert!(text.contains("1 conflicting"), "{text}");
    }

    // ── V-9 ────────────────────────────────────────────────────────────────

    #[test]
    fn an_id_invented_outside_the_source_is_found() {
        let source = "| `L-3` | the journal |\n| `V-9` | ids |\n";
        let batches = "Batch 1 members: `L-3`, `V-9`, `L-99`";
        let stray = stray_ids(source, &[("batches.md", batches)]);
        assert_eq!(stray.len(), 1);
        assert_eq!(stray[0].id, "L-99");
        assert_eq!(stray[0].file, "batches.md");
    }

    /// `V-21`: a requirement is not evidence of its own implementation.
    ///
    /// `already_built` was `git grep`'s answer over the whole repository, and
    /// the requirements source and the journal live in that repository — so
    /// every id matched the file it is defined in, and the check said "already
    /// built" for work nobody had started. Measured on Janitor: `J-25`, `J-26`
    /// and `J-27` were untouched and all three came back present, because
    /// `.harness/perpetum.md` and `.harness/journal.jsonl` name them.
    ///
    /// What it cost: the loop chose "build on what is there over implement it
    /// again", primed the coder with the claim, and the coder — after one glob
    /// that matched nothing — reported a full implementation of a repository
    /// that does not exist. Excluded now, so a match means code.
    #[test]
    fn the_requirements_file_is_not_evidence_that_a_requirement_is_built() {
        let (repo, root) = red_repo("verify-reality-harness-dir", "marker");

        // The id is defined where requirements are written, and implemented
        // nowhere — the state every open requirement is in.
        let harness = root.join(".harness");
        std::fs::create_dir_all(&harness).expect("harness dir");
        std::fs::write(harness.join("perpetum.md"), "| `Z-99` | never written |
")
            .expect("write the source");
        repo.stage(&[".harness/perpetum.md"]).expect("stage");
        repo.commit(&crate::git::CommitMessage::new("Write Z-99 down")).expect("commit");

        let searched = RealityCheck::run(&repo, "Z-99", &["Z-99"], &[]).expect("unfiltered");
        assert!(
            searched.already_built(),
            "the old behaviour, kept as the contrast: {:?}",
            searched.in_tree
        );

        let filtered =
            RealityCheck::run(&repo, "Z-99", &["Z-99"], &[".harness".to_string()])
                .expect("filtered");
        assert!(
            !filtered.already_built(),
            "written down is not built: {:?}",
            filtered.in_tree
        );
        assert!(
            !filtered.was_removed(),
            "and the history of writing it down is not a history of removing it: {:?}",
            filtered.in_history
        );
    }

    #[test]
    fn citing_existing_ids_is_fine() {
        let source = "| `L-3` | a |\n| `N-10` | b |\n| `G-14` | c |\n";
        let other = "see `L-3`, `N-10` and `G-14` — all defined elsewhere";
        assert!(stray_ids(source, &[("state.md", other)]).is_empty());
    }

    #[test]
    fn things_that_only_look_like_ids_are_left_alone() {
        // Dates, versions and UPPERCASE-9 words are not requirement ids.
        let source = "| `L-3` | a |";
        let other = "released 2026-07-28, UTF-8, HTTP-2, and `L-3`";
        let stray = stray_ids(source, &[("notes.md", other)]);
        assert!(
            stray.is_empty(),
            "false positives make the check ignorable: {stray:?}"
        );
    }

    // ── V-1 ────────────────────────────────────────────────────────────────

    #[test]
    fn implementation_needs_a_recorded_reality_check() {
        assert!(may_implement(&[], "L-3").is_err());

        let records = vec![Record::outcome(step("c1/b1/s01"), 1, true, "looked first")
            .for_requirements(["L-3"])
            .with_detail("reality check for L-3\nsearched: journal\nin the tree: nothing\n")];
        assert!(may_implement(&records, "L-3").is_ok());
        assert!(
            may_implement(&records, "L-4").is_err(),
            "one requirement's check does not cover another's"
        );
    }
}
