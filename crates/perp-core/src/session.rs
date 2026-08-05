//! Sessions, steps and recovery (`L-5`, `L-6`, `L-7`, `L-15`, `N-1`, `N-2`).
//!
//! A session is one process's turn at the loop. It opens from the binding and
//! the journal and nothing else (`N-2`), writes an intent before each step's
//! side effect and an outcome after (`L-3`), and can be killed at any moment
//! losing at most the step in flight (`N-1`).
//!
//! The interesting part is what happens next time. A step whose intent has no
//! outcome is not assumed to have failed *or* succeeded — the workspace is
//! asked (`L-7`). A step that declared itself idempotent can simply be redone;
//! one that did not must be probed first (`L-5`).

use std::path::{Path, PathBuf};

use crate::binding::Binding;
use crate::error::Result;
use crate::journal::{Journal, Kind, Record};
use crate::process::Nursery;
use crate::state::{replay, Projection};
use crate::step::StepId;
use crate::time;

/// What the workspace says about a step that never closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finding {
    /// The effect is present — the process died after doing the work.
    Happened,
    /// The effect is absent — it died before.
    DidNot,
    /// Cannot be told apart. Never guessed.
    Unclear,
}

/// What to do about it (`L-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Nothing was in flight.
    Continue,
    /// Close the step and move on; the work is already there.
    Skip { step: StepId, why: String },
    /// Run it again.
    Redo { step: StepId, why: String },
    /// Stop and ask. A step that cannot be told apart is not retried blindly.
    Park { step: StepId, why: String },
}

impl Decision {
    pub fn step(&self) -> Option<&StepId> {
        match self {
            Decision::Continue => None,
            Decision::Skip { step, .. } | Decision::Redo { step, .. } | Decision::Park { step, .. } => {
                Some(step)
            }
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Decision::Continue => "nothing in flight".to_string(),
            Decision::Skip { step, why } => format!("skip {step}: {why}"),
            Decision::Redo { step, why } => format!("redo {step}: {why}"),
            Decision::Park { step, why } => format!("park {step}: {why}"),
        }
    }
}

/// Asks the workspace whether a step's effect is there. The engine never
/// answers this from the journal alone — that is the whole point of `L-7`.
pub trait Probe {
    fn finding(&self, step: &StepId, summary: &str) -> Finding;
}

/// The probe for a step that declared itself idempotent: no need to look, doing
/// it twice is safe (`L-5`).
#[derive(Debug, Default, Clone, Copy)]
pub struct AssumeRedoable;

impl Probe for AssumeRedoable {
    fn finding(&self, _step: &StepId, _summary: &str) -> Finding {
        Finding::DidNot
    }
}

/// Decide what to do about an unclosed step.
pub fn reconcile(projection: &Projection, records: &[Record], probe: &dyn Probe) -> Decision {
    let Some(step) = projection.open_step.clone() else {
        return Decision::Continue;
    };
    let summary = projection.open_summary.clone().unwrap_or_default();

    // A step that did not say it was idempotent is treated as though it was
    // not. Absent evidence, the conservative reading is the safe one.
    let idempotent = records
        .iter()
        .rev()
        .find(|record| record.step == step && record.kind == Kind::Intent)
        .and_then(Record::is_idempotent)
        .unwrap_or(false);

    match probe.finding(&step, &summary) {
        Finding::Happened => Decision::Skip {
            step,
            why: "the workspace already has the effect".to_string(),
        },
        Finding::DidNot => Decision::Redo {
            step,
            why: "the effect is absent, so the step never landed".to_string(),
        },
        Finding::Unclear if idempotent => Decision::Redo {
            step,
            why: "cannot tell, but the step declared itself idempotent".to_string(),
        },
        Finding::Unclear => Decision::Park {
            step,
            why: "cannot tell whether it landed, and it is not safe to repeat".to_string(),
        },
    }
}

/// A step in progress. Its outcome is written by [`StepGuard::close`]; dropping
/// it without closing leaves the intent open on purpose — that is what a crash
/// looks like, and pretending otherwise would hide it (`N-1`).
#[derive(Debug)]
pub struct StepGuard<'a> {
    journal: &'a Journal,
    /// Where the projection goes after each outcome (`L-4`). `None` when the
    /// binding declares no state file.
    state_path: Option<PathBuf>,
    step: StepId,
    /// Carried from the intent onto the outcome. Without this the record that
    /// holds the transcript does not say which requirement it is evidence for,
    /// and `/explain <id>` finds an intent with no result — which is how a
    /// requirement with a green gate reads as never worked on (`C-7`).
    requirements: Vec<String>,
    /// The requirements source, for `V-8`'s counts. `None` when the binding
    /// does not resolve one — the state file then simply has no Requirements
    /// section, rather than one full of zeroes that reads as "nothing left".
    requirements_source: Option<PathBuf>,
    closed: bool,
}

impl StepGuard<'_> {
    pub fn id(&self) -> &StepId {
        &self.step
    }

    /// The journal this step is being written to, so a caller holding the
    /// guard can append alongside the step without a second borrow of the
    /// session (`M-11`).
    pub fn journal(&self) -> &Journal {
        self.journal
    }

    pub fn close(mut self, ok: bool, summary: &str) -> Result<()> {
        self.journal.append(&self.outcome(ok, summary))?;
        self.closed = true;
        self.project()
    }

    pub fn close_with(mut self, ok: bool, summary: &str, detail: &str) -> Result<()> {
        self.journal.append(&self.outcome(ok, summary).with_detail(detail))?;
        self.closed = true;
        self.project()
    }

    fn outcome(&self, ok: bool, summary: &str) -> Record {
        Record::outcome(self.step.clone(), time::now(), ok, summary)
            .for_requirements(self.requirements.iter().map(String::as_str))
    }

    pub fn was_closed(&self) -> bool {
        self.closed
    }

    /// Rewrite the state file from the journal (`L-4`).
    ///
    /// After the outcome is on the record, never before: a projection that
    /// described a step the journal has not yet accepted would be exactly the
    /// disagreement `L-4` says the journal wins.
    fn project(&self) -> Result<()> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        let projection = replay(&self.journal.read_all()?);
        // `V-8`: counted from the source's own markers each time the state is
        // rewritten, so the figure cannot drift from the file it describes.
        let counts = self.requirements_source.as_ref().map(|path| {
            let source = crate::layout::requirements_text(path);
            crate::verify::Counts::of(&crate::cycle::markers(&source))
        });
        crate::atomic::write_atomic(
            path,
            &crate::state::render(&projection, time::now(), counts.as_ref()),
        )
    }
}

/// One process's turn at the loop.
#[derive(Debug)]
pub struct Session {
    binding: Binding,
    journal: Journal,
    nursery: Nursery,
    next_seq: u32,
}

impl Session {
    /// Open from the binding and the journal — and nothing else (`N-2`).
    pub fn open(root: &Path) -> Result<Session> {
        let binding = Binding::load(root)?;
        binding.verify()?;
        // The harness owns `.harness/`, so it keeps the rule about its own scratch
        // there rather than asking the project to carry it. Written only when
        // absent, and a failure is not worth stopping a run for.
        crate::layout::ensure_gitignore(root);
        let journal = Journal::at(binding.resolve("out.journal")?);
        let records = journal.read_all()?;
        let next_seq = records.iter().map(|r| r.step.seq).max().unwrap_or(0) + 1;
        Ok(Session { binding, journal, nursery: Nursery::new(), next_seq })
    }

    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn nursery(&mut self) -> &mut Nursery {
        &mut self.nursery
    }

    pub fn journal_path(&self) -> &Path {
        self.journal.path()
    }

    pub fn projection(&self) -> Result<Projection> {
        Ok(replay(&self.journal.read_all()?))
    }

    /// What a previous process left behind (`L-7`).
    pub fn resume(&self, probe: &dyn Probe) -> Result<Decision> {
        let records = self.journal.read_all()?;
        let projection = replay(&records);
        Ok(reconcile(&projection, &records, probe))
    }

    /// The next step id in a stage. Sequence numbers are monotonic within a
    /// cycle, continuing from whatever is already on the record — so two
    /// processes' work reads as one ordered history (`L-22`).
    pub fn next_step(&self, cycle: u32, stage: &str) -> Result<StepId> {
        StepId::new(cycle, stage, self.next_seq)
    }

    /// Declare a step. The intent hits the journal before anything happens, so
    /// a kill between here and `close` is visible rather than silent.
    pub fn begin(&mut self, step: StepId, summary: &str, idempotent: bool) -> Result<StepGuard<'_>> {
        self.begin_for(step, summary, idempotent, &[])
    }

    /// The same, citing the requirements the step serves (`V-9` — cited here,
    /// minted only in the requirements document).
    pub fn begin_for(
        &mut self,
        step: StepId,
        summary: &str,
        idempotent: bool,
        requirements: &[String],
    ) -> Result<StepGuard<'_>> {
        let record = Record::intent(step.clone(), time::now(), summary)
            .for_requirements(requirements.iter().map(String::as_str))
            .idempotent(idempotent);
        self.journal.append(&record)?;
        self.next_seq = self.next_seq.max(step.seq + 1);
        Ok(StepGuard {
            journal: &self.journal,
            state_path: self.binding.resolve("out.state").ok(),
            step,
            requirements: requirements.to_vec(),
            requirements_source: self.binding.resolve("path.requirements").ok(),
            closed: false,
        })
    }

    /// Stop cleanly (`L-15`): nothing left running, nothing half-applied.
    ///
    /// Reports what it had to kill rather than doing it quietly — a step that
    /// leaves a server behind is a defect, and a silent cleanup hides it.
    pub fn stop(&mut self) -> Result<StopReport> {
        let killed = self.nursery.kill_all();
        let projection = self.projection()?;
        Ok(StopReport {
            children_killed: killed,
            in_flight: projection.open_step,
            steps_done: projection.done.len(),
            steps_blocked: projection.blocked.len(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReport {
    pub children_killed: usize,
    /// A step still open at stop time. Not an error — it is what the next
    /// session reconciles — but it is reported rather than hidden.
    pub in_flight: Option<StepId>,
    pub steps_done: usize,
    pub steps_blocked: usize,
}

impl StopReport {
    pub fn is_clean(&self) -> bool {
        self.children_killed == 0 && self.in_flight.is_none()
    }

    pub fn describe(&self) -> String {
        let mut out = format!(
            "{} done, {} blocked, {} children killed",
            self.steps_done, self.steps_blocked, self.children_killed
        );
        if let Some(step) = &self.in_flight {
            out.push_str(&format!(", {step} left in flight"));
        }
        out
    }
}

/// The path a session was opened from, for messages.
pub fn describe_root(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    struct Says(Finding);

    impl Probe for Says {
        fn finding(&self, _step: &StepId, _summary: &str) -> Finding {
            self.0
        }
    }

    fn fixture(tag: &str) -> PathBuf {
        let root = tmpdir(tag);
        std::fs::create_dir_all(root.join(".harness")).expect("dirs");
        std::fs::write(root.join(".harness/perpetum.md"), "# requirements\n").expect("reqs");
        std::fs::write(
            root.join(".harness/binding.md"),
            "```perp-binding\n\
             path.requirements = .harness/perpetum.md\n\
             out.journal       = .harness/journal.jsonl\n\
             out.state         = .harness/state.md\n\
             ```\n",
        )
        .expect("binding");
        root
    }

    fn step(text: &str) -> StepId {
        StepId::parse(text).expect("step id")
    }

    #[test]
    fn a_session_opens_from_the_binding_and_the_journal_alone() {
        // `N-2`: nothing else is read, so nothing else can be missing.
        let root = fixture("session-open");
        let session = Session::open(&root).expect("open");
        assert_eq!(session.projection().expect("projection").done.len(), 0);
        assert_eq!(session.next_step(1, "b3").expect("id").to_string(), "c1/b3/s01");
    }

    #[test]
    fn an_unbound_directory_cannot_open_a_session() {
        let root = tmpdir("session-unbound");
        assert!(Session::open(&root).is_err());
    }

    #[test]
    fn a_closed_step_leaves_nothing_in_flight() {
        let root = fixture("session-closed");
        let mut session = Session::open(&root).expect("open");
        let id = session.next_step(1, "b3").expect("id");
        let guard = session.begin(id, "do a thing", true).expect("begin");
        guard.close(true, "did the thing").expect("close");

        let decision = session.resume(&Says(Finding::Unclear)).expect("resume");
        assert_eq!(decision, Decision::Continue);
    }

    #[test]
    fn a_dropped_guard_leaves_the_step_in_flight() {
        // `N-1`: this is what kill -9 looks like, and it must be visible.
        let root = fixture("session-dropped");
        {
            let mut session = Session::open(&root).expect("open");
            let id = session.next_step(1, "b3").expect("id");
            let guard = session.begin(id, "write the file", false).expect("begin");
            assert!(!guard.was_closed());
            // Dropped without close — the process died here.
        }
        let session = Session::open(&root).expect("reopen");
        let projection = session.projection().expect("projection");
        assert_eq!(projection.open_step, Some(step("c1/b3/s01")));
    }

    #[test]
    fn a_step_that_already_landed_is_skipped_not_repeated() {
        let root = fixture("session-skip");
        {
            let mut session = Session::open(&root).expect("open");
            let id = session.next_step(1, "b3").expect("id");
            let _guard = session.begin(id, "create the release tag", false).expect("begin");
        }
        let session = Session::open(&root).expect("reopen");
        match session.resume(&Says(Finding::Happened)).expect("resume") {
            Decision::Skip { step: id, why } => {
                assert_eq!(id.to_string(), "c1/b3/s01");
                assert!(why.contains("already"), "{why}");
            }
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn a_step_that_never_landed_is_redone() {
        let root = fixture("session-redo");
        {
            let mut session = Session::open(&root).expect("open");
            let id = session.next_step(1, "b3").expect("id");
            let _guard = session.begin(id, "write the file", false).expect("begin");
        }
        let session = Session::open(&root).expect("reopen");
        assert!(matches!(
            session.resume(&Says(Finding::DidNot)).expect("resume"),
            Decision::Redo { .. }
        ));
    }

    #[test]
    fn an_unclear_step_parks_unless_it_declared_itself_idempotent() {
        // `L-5`: repeating a non-idempotent step blind is how a loop sends the
        // same email twice.
        let unsafe_root = fixture("session-unclear-unsafe");
        {
            let mut session = Session::open(&unsafe_root).expect("open");
            let id = session.next_step(1, "b3").expect("id");
            let _guard = session.begin(id, "post the release", false).expect("begin");
        }
        let session = Session::open(&unsafe_root).expect("reopen");
        assert!(matches!(
            session.resume(&Says(Finding::Unclear)).expect("resume"),
            Decision::Park { .. }
        ));

        let safe_root = fixture("session-unclear-safe");
        {
            let mut session = Session::open(&safe_root).expect("open");
            let id = session.next_step(1, "b3").expect("id");
            let _guard = session.begin(id, "rewrite the projection", true).expect("begin");
        }
        let session = Session::open(&safe_root).expect("reopen");
        assert!(matches!(
            session.resume(&Says(Finding::Unclear)).expect("resume"),
            Decision::Redo { .. }
        ));
    }

    #[test]
    fn a_step_with_no_recorded_idempotence_is_treated_as_unsafe() {
        let root = fixture("session-legacy");
        let journal = Journal::at(root.join(".harness/journal.jsonl"));
        // An intent written by something that never heard of the flag.
        journal
            .append(&Record::intent(step("c1/b3/s01"), 10, "from an older writer"))
            .expect("append");

        let session = Session::open(&root).expect("open");
        assert!(matches!(
            session.resume(&Says(Finding::Unclear)).expect("resume"),
            Decision::Park { .. }
        ));
    }

    #[test]
    fn closing_a_step_rewrites_the_state_file() {
        // `L-4`: the projection is rewritten after each outcome, not at the end
        // of a run that might never reach its end.
        let root = fixture("session-project");
        let state_path = root.join(".harness/state.md");
        let mut session = Session::open(&root).expect("open");

        let first = session.next_step(1, "b3").expect("id");
        session.begin(first, "first", true).expect("begin").close(true, "first landed").expect("close");
        let after_one = std::fs::read_to_string(&state_path).expect("state written");
        assert!(after_one.contains("first landed"), "{after_one}");
        assert!(after_one.contains("1 done, 0 blocked"), "{after_one}");

        let second = session.next_step(1, "b3").expect("id");
        session
            .begin(second, "second", true)
            .expect("begin")
            .close_with(false, "second failed", "error: the actual text")
            .expect("close");
        let after_two = std::fs::read_to_string(&state_path).expect("state rewritten");
        assert!(after_two.contains("1 done, 1 blocked"), "{after_two}");
        assert!(after_two.contains("error: the actual text"), "verbatim, in the projection");
    }

    #[test]
    fn an_open_step_is_not_yet_in_the_state_file() {
        // The projection follows the journal; it never runs ahead of it.
        let root = fixture("session-project-open");
        let state_path = root.join(".harness/state.md");
        let mut session = Session::open(&root).expect("open");
        let id = session.next_step(1, "b3").expect("id");
        let _guard = session.begin(id, "in flight", true).expect("begin");
        assert!(!state_path.exists(), "an intent alone must not produce a projection");
    }

    #[test]
    fn the_outcome_carries_the_requirements_the_intent_declared() {
        // Found by running `perp explain V-2` against a journal the engine had
        // just written: the transcript was on the outcome and the requirement
        // id was on the intent, so the evidence chain for a green gate came
        // back empty. The record that holds the proof has to say what it is
        // proof of (`C-7`, `V-9`).
        let root = fixture("session-requirements");
        let mut session = Session::open(&root).expect("open");
        let step = session.next_step(3, "b13").expect("step");
        let guard = session
            .begin_for(step, "run the gate", true, &["V-2".to_string()])
            .expect("begin");
        guard.close_with(true, "gate test is green", "gate: test
exit 0
").expect("close");

        let records = session.journal().read_all().expect("read");
        let outcome = records
            .iter()
            .find(|record| record.kind == crate::journal::Kind::Outcome)
            .expect("an outcome");
        assert_eq!(outcome.requirements, ["V-2"]);
        assert!(outcome.detail.is_some(), "and it is the record with the transcript on it");
    }

    #[test]
    fn sequence_numbers_continue_across_sessions() {
        // `L-6`: a new process picks up where the last one stopped, and the two
        // sessions' work reads as one ordered history.
        let root = fixture("session-seq");
        {
            let mut session = Session::open(&root).expect("open");
            let id = session.next_step(1, "b3").expect("id");
            session.begin(id, "first", true).expect("begin").close(true, "first").expect("close");
        }
        let session = Session::open(&root).expect("reopen");
        assert_eq!(session.next_step(1, "b3").expect("id").to_string(), "c1/b3/s02");
    }

    #[test]
    fn stopping_reports_what_it_had_to_clean_up() {
        // `L-15`: a clean stop, and a stop that was not clean says so.
        let root = fixture("session-stop");
        let mut session = Session::open(&root).expect("open");
        let id = session.next_step(1, "b3").expect("id");
        session.begin(id, "work", true).expect("begin").close(true, "done").expect("close");

        let report = session.stop().expect("stop");
        assert!(report.is_clean(), "{}", report.describe());
        assert_eq!(report.steps_done, 1);
        assert_eq!(report.children_killed, 0);
    }

    #[test]
    fn stopping_kills_what_the_step_left_running() {
        // `L-15` with something to actually clean up. Without this the stop
        // path is only ever exercised with an empty nursery, and a stop that
        // never kills anything would pass.
        use crate::process::Spec;
        use std::time::Duration;

        let root = fixture("session-stop-children");
        let mut session = Session::open(&root).expect("open");
        let sleeper = if cfg!(windows) {
            "cmd /C \"ping -n 60 127.0.0.1 > nul\"".to_string()
        } else {
            "sh -c \"sleep 60\"".to_string()
        };
        session
            .nursery()
            .spawn(&Spec::new(sleeper, &root, Duration::from_secs(60)))
            .expect("spawn");

        let report = session.stop().expect("stop");
        assert_eq!(report.children_killed, 1, "the child had to be killed");
        assert!(!report.is_clean(), "a stop that had to kill something is not clean");
        assert!(report.describe().contains("1 children killed"), "{}", report.describe());
    }

    #[test]
    fn stopping_mid_step_reports_the_step_left_open() {
        let root = fixture("session-stop-dirty");
        let mut session = Session::open(&root).expect("open");
        let id = session.next_step(1, "b3").expect("id");
        let guard = session.begin(id.clone(), "half a thing", false).expect("begin");
        drop(guard);

        let report = session.stop().expect("stop");
        assert!(!report.is_clean());
        assert_eq!(report.in_flight, Some(id));
        assert!(report.describe().contains("left in flight"), "{}", report.describe());
    }
}
