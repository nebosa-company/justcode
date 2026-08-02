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
    pub fn run(repo: &Repo, requirement: &str, needles: &[&str]) -> Result<RealityCheck> {
        let mut in_tree = BTreeSet::new();
        let mut in_history = BTreeSet::new();
        for needle in needles {
            in_tree.extend(repo.tree_mentions(needle)?);
            in_history.extend(repo.history_mentions(needle)?);
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
pub fn derive_marker(records: &[Record], requirement: &str) -> Marker {
    let mine: Vec<&Record> = records
        .iter()
        .filter(|record| record.requirements.iter().any(|id| id == requirement))
        .collect();

    if mine.is_empty() {
        return Marker::Open;
    }
    if mine.iter().any(|r| r.kind == Kind::Outcome && r.ok == Some(false)) {
        return Marker::Blocked;
    }
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
            },
            sha: None,
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
