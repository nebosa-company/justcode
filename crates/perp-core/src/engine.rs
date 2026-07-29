//! The loop driver.
//!
//! Everything up to here was a component the operator invoked by hand. This is
//! the thing that invokes them: it takes the write lock, walks steps until
//! something stops it, journals an intent before each and an outcome after, and
//! checks the budgets at every boundary.
//!
//! ## What a step is
//!
//! The engine does not know how to do any particular work — it knows the
//! *shape* of doing work. A [`Work`] hands it one [`Task`] at a time and says
//! how each one turned out. [`Gates`] is the implementation that ships with it:
//! run the project's gates, keep the transcripts, journal each. It needs no
//! model, which is why it is the one that exists first — a driver whose only
//! implementation requires a GPU is a driver nobody can test.
//!
//! ## Where it stops
//!
//! Only the three in `L-14`, plus a park for a budget. There is deliberately no
//! "ran out of ideas" or "seemed done": every exit from the loop names one of
//! four conditions and writes a record saying which.
//!
//! ## What it will not do
//!
//! Interrupt a step. A budget reached mid-step parks at the **next** boundary,
//! because a killed step leaves an open intent, an unclosed process group and
//! possibly half an edit, and reclaims a budget that has already been spent.

use std::path::{Path, PathBuf};

use crate::budget::{Budgets, Spend, Verdict};
use crate::error::{Error, Result};
use crate::gate::{self, Gate};
use crate::lock::{Concurrency, Lock};
use crate::phase::{Park, Stop};
use crate::session::Session;
use crate::step::StepId;
use crate::time;

/// One unit of work the engine will wrap in a journalled step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub summary: String,
    /// Cited, never minted (`V-9`).
    pub requirements: Vec<String>,
    /// Whether doing it twice is safe, which is what `L-5` reconciliation reads
    /// after a crash.
    pub idempotent: bool,
}

impl Task {
    pub fn new(summary: impl Into<String>) -> Task {
        Task { summary: summary.into(), requirements: Vec::new(), idempotent: false }
    }

    pub fn idempotent(mut self) -> Task {
        self.idempotent = true;
        self
    }

    pub fn for_requirements<I: IntoIterator<Item = S>, S: Into<String>>(mut self, ids: I) -> Task {
        self.requirements = ids.into_iter().map(Into::into).collect();
        self
    }
}

/// How a task turned out. The engine writes a different record for each, and
/// there is no variant that means "probably fine".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    Ok { summary: String, detail: Option<String> },
    /// It ran and did not work. The loop continues — a red gate is information.
    Failed { summary: String, detail: String },
    /// It cannot run, and no amount of retrying changes that. Stops the batch.
    Blocked { why: String },
    /// The work itself asked to stop, for one of `L-14`'s reasons.
    Halt(Stop),
}

impl Done {
    pub fn ok(summary: impl Into<String>) -> Done {
        Done::Ok { summary: summary.into(), detail: None }
    }

    pub fn ok_with(summary: impl Into<String>, detail: impl Into<String>) -> Done {
        Done::Ok { summary: summary.into(), detail: Some(detail.into()) }
    }
}

/// A source of tasks.
pub trait Work {
    /// The next task, or `None` when there is nothing left — which the engine
    /// reads as `L-14`'s exhausted backlog.
    fn next(&mut self) -> Option<Task>;

    fn perform(&mut self, task: &Task) -> Done;

    /// What this work has spent since the run began. Default: nothing, which is
    /// correct for work that never calls a model.
    fn spend(&self) -> Spend {
        Spend::default()
    }
}

/// What a run did.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub steps: u32,
    pub failed: u32,
    pub stop: Option<Stop>,
    pub park: Option<Park>,
    pub spend: Spend,
    /// The lock this run took over from a process that stopped beating, if any.
    pub took_over: Option<String>,
    pub first_step: Option<StepId>,
    pub last_step: Option<StepId>,
}

impl Report {
    pub fn describe(&self) -> String {
        let mut out = format!("{} steps", self.steps);
        if self.failed > 0 {
            out.push_str(&format!(", {} failed", self.failed));
        }
        if let (Some(first), Some(last)) = (&self.first_step, &self.last_step) {
            out.push_str(&format!(" ({first}–{last})"));
        }
        match (&self.stop, &self.park) {
            (Some(stop), _) => out.push_str(&format!(" — {}", stop.summary())),
            (None, Some(park)) => out.push_str(&format!(" — parked: {}", park.reason)),
            (None, None) => {}
        }
        out
    }
}

/// Drives a [`Work`] under a lock, a budget and a journal.
#[derive(Debug)]
pub struct Engine {
    session: Session,
    budgets: Budgets,
    concurrency: Concurrency,
    root: PathBuf,
    /// The clock. Injected so a budget test does not have to wait out a
    /// wall-clock limit in real seconds.
    now: fn() -> i64,
}

impl Engine {
    pub fn open(root: &Path) -> Result<Engine> {
        let session = Session::open(root)?;
        let entries: Vec<(String, String)> = session
            .binding()
            .entries()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let budgets = crate::budget::from_entries(&entries)?;
        Ok(Engine {
            session,
            budgets,
            concurrency: Concurrency::default(),
            root: root.to_path_buf(),
            now: time::now,
        })
    }

    pub fn with_budgets(mut self, budgets: Budgets) -> Engine {
        self.budgets = budgets;
        self
    }

    pub fn with_clock(mut self, now: fn() -> i64) -> Engine {
        self.now = now;
        self
    }

    pub fn budgets(&self) -> Budgets {
        self.budgets
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn concurrency(&self) -> Concurrency {
        self.concurrency
    }

    /// Run until something in `L-14` stops it or a budget parks it.
    ///
    /// `cycle` and `stage` place every step id this run mints. `in_flight` is
    /// how many features are already running elsewhere, which `L-17` checks
    /// before this one is allowed to start at all.
    pub fn run(
        &mut self,
        cycle: u32,
        stage: &str,
        work: &mut dyn Work,
        in_flight: u32,
    ) -> Result<Report> {
        self.concurrency.admit(in_flight, false)?;

        let started = (self.now)();
        let owner = Lock::this_process(started);
        let first = self.session.next_step(cycle, stage)?;
        let mut lock = Lock::write_lock(&self.root, &owner, &first, started)?;
        let took_over = lock.broke().map(|h| h.describe());

        // A lock taken from a dead process is journalled before any work, so
        // the record of what this run inherited exists even if it dies too.
        if let Some(previous) = &took_over {
            let step = self.session.next_step(cycle, stage)?;
            let guard = self.session.begin(step, "took over an abandoned write lock", true)?;
            guard.close_with(true, "took over an abandoned write lock", previous)?;
        }

        let mut report = Report {
            steps: 0,
            failed: 0,
            stop: None,
            park: None,
            spend: Spend::default(),
            took_over,
            first_step: None,
            last_step: None,
        };

        loop {
            // The boundary. Checked before the next task begins and never
            // during one.
            let elapsed = (self.now)() - started;
            let spend = work.spend();
            report.spend = Spend { seconds: elapsed, ..spend };
            if let Verdict::Exhausted { .. } = self.budgets.check(report.spend, report.spend) {
                let park = self
                    .budgets
                    .check(report.spend, report.spend)
                    .park()
                    .unwrap_or_else(|| Park::budget("exhausted"));
                self.record_end(cycle, stage, None, Some(&park))?;
                report.park = Some(park);
                break;
            }

            let Some(task) = work.next() else {
                let stop = Stop::BacklogExhausted;
                self.record_end(cycle, stage, Some(&stop), None)?;
                report.stop = Some(stop);
                break;
            };

            let step = self.session.next_step(cycle, stage)?;
            lock.at_step(&step, (self.now)())?;
            report.first_step.get_or_insert_with(|| step.clone());
            report.last_step = Some(step.clone());
            report.steps += 1;

            let guard = self.session.begin_for(
                step,
                &task.summary,
                task.idempotent,
                &task.requirements,
            )?;
            let done = work.perform(&task);

            match done {
                Done::Ok { summary, detail } => match detail {
                    Some(detail) => guard.close_with(true, &summary, &detail)?,
                    None => guard.close(true, &summary)?,
                },
                Done::Failed { summary, detail } => {
                    report.failed += 1;
                    guard.close_with(false, &summary, &detail)?;
                }
                Done::Blocked { why } => {
                    guard.close_with(false, &format!("blocked: {why}"), &why)?;
                    let stop = Stop::BatchBlocked { batch: stage.to_string(), why };
                    self.record_end(cycle, stage, Some(&stop), None)?;
                    report.stop = Some(stop);
                    break;
                }
                Done::Halt(stop) => {
                    guard.close(true, &stop.summary())?;
                    self.record_end(cycle, stage, Some(&stop), None)?;
                    report.stop = Some(stop);
                    break;
                }
            }
        }

        lock.release()?;
        Ok(report)
    }

    /// The terminal record. Every exit writes exactly one (`L-14`).
    fn record_end(
        &mut self,
        cycle: u32,
        stage: &str,
        stop: Option<&Stop>,
        park: Option<&Park>,
    ) -> Result<()> {
        let step = self.session.next_step(cycle, stage)?;
        let record = match (stop, park) {
            (Some(stop), _) => stop.record(step.clone(), (self.now)()),
            (None, Some(park)) => park.record(step.clone(), (self.now)()),
            (None, None) => return Ok(()),
        };
        let summary = record.summary.clone();
        let detail = record.detail.clone().unwrap_or_default();
        let ok = record.ok.unwrap_or(false);
        let guard = self.session.begin(step, &summary, true)?;
        guard.close_with(ok, &summary, &detail)
    }
}

/// The work that ships with the engine: run the project's gates, one step each,
/// keeping every transcript (`V-2`).
///
/// One gate per step rather than all of them in one is deliberate. A step that
/// runs four commands and reports one verdict cannot say which of them was the
/// red one without a reader parsing prose, and the step id is the thing every
/// other surface cites.
#[derive(Debug)]
pub struct Gates {
    gates: Vec<Gate>,
    next: usize,
    /// The build target, for the gate lock (`L-18`).
    target: PathBuf,
    owner: String,
    held: Option<Lock>,
    /// The commit every transcript is pinned to (`G-6`). Read once, at the top
    /// of the run: a transcript pinned to a sha the tree has since moved past
    /// would be worse than one with no sha at all.
    sha: Option<String>,
    pub results: Vec<gate::GateResult>,
}

impl Gates {
    pub fn from_binding(binding: &crate::Binding, target: &Path) -> Result<Gates> {
        Ok(Gates {
            gates: Gate::from_binding(binding)?,
            next: 0,
            target: target.to_path_buf(),
            owner: Lock::this_process(time::now()),
            held: None,
            // A repository that cannot answer is not an error — the gate still
            // ran — and the transcript then says nothing rather than implying a
            // commit it does not have.
            sha: crate::git::Repo::at(binding.root()).head_sha().ok(),
            results: Vec::new(),
        })
    }

    pub fn all_green(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(gate::GateResult::is_green)
    }
}

impl Work for Gates {
    fn next(&mut self) -> Option<Task> {
        let gate = self.gates.get(self.next)?;
        Some(Task::new(format!("gate: {}", gate.name)).idempotent().for_requirements(["V-2"]))
    }

    fn perform(&mut self, _task: &Task) -> Done {
        let Some(gate) = self.gates.get(self.next).cloned() else {
            return Done::Blocked { why: "the gate disappeared between planning and running".into() };
        };
        self.next += 1;

        // Two builds in one target directory is a false red (`L-18`). Held for
        // the whole run rather than per gate, because the gates share it.
        if self.held.is_none() {
            let step = StepId::new(0, "gate", 0).unwrap_or_else(|_| StepId {
                cycle: 0,
                stage: "gate".into(),
                seq: 0,
            });
            match Lock::gate_lock(&self.target, &self.owner, &step, time::now()) {
                Ok(lock) => self.held = Some(lock),
                Err(e) => return Done::Blocked { why: format!("{e}") },
            }
        }

        match gate.run() {
            Ok(result) => {
                let result = match &self.sha {
                    Some(sha) => result.at_sha(sha),
                    None => result,
                };
                let evidence = result.evidence();
                let green = result.is_green();
                let name = gate.name.clone();
                self.results.push(result);
                if green {
                    Done::ok_with(format!("gate {name} is green"), evidence)
                } else {
                    Done::Failed { summary: format!("gate {name} is red"), detail: evidence }
                }
            }
            Err(e) => Done::Blocked { why: format!("gate {} could not run: {e}", gate.name) },
        }
    }
}

/// Work that does nothing, for proving the driver's own behaviour without a
/// gate, a model or a workspace to mutate.
#[derive(Debug, Default)]
pub struct Fixed {
    pub tasks: Vec<(Task, Done)>,
    pub spend: Spend,
    at: usize,
}

impl Fixed {
    pub fn new(tasks: Vec<(Task, Done)>) -> Fixed {
        Fixed { tasks, spend: Spend::default(), at: 0 }
    }
}

impl Work for Fixed {
    fn next(&mut self) -> Option<Task> {
        self.tasks.get(self.at).map(|(task, _)| task.clone())
    }

    fn perform(&mut self, _task: &Task) -> Done {
        let done = self
            .tasks
            .get(self.at)
            .map(|(_, done)| done.clone())
            .unwrap_or(Done::Blocked { why: "no task".into() });
        self.at += 1;
        done
    }

    fn spend(&self) -> Spend {
        self.spend
    }
}

/// The engine refuses a root that is not bound, with the binding's own message
/// rather than a generic one.
pub fn describe_unbound(root: &Path, e: &Error) -> String {
    format!("{}: {e}", crate::session::describe_root(root).display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::Budget;
    use crate::journal::{Journal, Kind};
    use crate::testutil::tmpdir;

    #[allow(clippy::expect_used)]
    fn workspace(tag: &str) -> PathBuf {
        let dir = tmpdir(tag);
        std::fs::create_dir_all(dir.join("docs/perpetum")).expect("dirs");
        std::fs::write(dir.join("docs/perpetum.md"), "# requirements\n").expect("requirements");
        std::fs::write(
            dir.join("docs/perpetum/binding.md"),
            "```perp-binding\n\
             path.requirements = docs/perpetum.md\n\
             out.journal = docs/perpetum/journal.jsonl\n\
             out.state = docs/perpetum/state.md\n\
             gate.check = cargo --version\n\
             ```\n",
        )
        .expect("binding");
        dir
    }

    #[allow(clippy::expect_used)]
    fn records(root: &Path) -> Vec<crate::Record> {
        Journal::at(root.join("docs/perpetum/journal.jsonl")).read_all().expect("read")
    }

    fn tasks(n: u32) -> Vec<(Task, Done)> {
        (1..=n)
            .map(|i| (Task::new(format!("step {i}")).idempotent(), Done::ok(format!("did {i}"))))
            .collect()
    }

    #[test]
    fn a_run_journals_an_intent_and_an_outcome_for_every_step() {
        let root = workspace("engine-basic");
        let mut engine = Engine::open(&root).expect("open");
        let mut work = Fixed::new(tasks(3));
        let report = engine.run(3, "b12", &mut work, 0).expect("run");

        assert_eq!(report.steps, 3);
        assert_eq!(report.stop, Some(Stop::BacklogExhausted));

        let records = records(&root);
        let intents = records.iter().filter(|r| r.kind == Kind::Intent).count();
        let outcomes = records.iter().filter(|r| r.kind == Kind::Outcome).count();
        assert_eq!(intents, outcomes, "every intent is closed");
        assert_eq!(intents, 4, "three steps and the terminal record");
    }

    #[test]
    fn the_backlog_running_out_writes_its_own_terminal_record() {
        let root = workspace("engine-exhausted");
        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(3, "b12", &mut Fixed::new(tasks(1)), 0).expect("run");
        assert_eq!(report.stop, Some(Stop::BacklogExhausted));

        let last = records(&root).pop().expect("a record");
        assert_eq!(last.detail.as_deref(), Some("stop=backlog-exhausted"));
        assert_eq!(last.ok, Some(true), "finishing the work is a success");
    }

    #[test]
    fn a_blocked_task_stops_the_batch_and_names_it() {
        let root = workspace("engine-blocked");
        let mut engine = Engine::open(&root).expect("open");
        let mut work = Fixed::new(vec![
            (Task::new("first"), Done::ok("fine")),
            (Task::new("second"), Done::Blocked { why: "no GPU on this box".into() }),
            (Task::new("third"), Done::ok("never reached")),
        ]);
        let report = engine.run(3, "b12", &mut work, 0).expect("run");

        assert_eq!(report.steps, 2, "the third never started");
        let Some(Stop::BatchBlocked { batch, why }) = report.stop else {
            panic!("expected a blocked batch, got {:?}", report.stop);
        };
        assert_eq!(batch, "b12");
        assert!(why.contains("GPU"), "the reason survives to the stop record: {why}");
    }

    #[test]
    fn a_failed_step_is_recorded_and_the_loop_carries_on() {
        let root = workspace("engine-red");
        let mut engine = Engine::open(&root).expect("open");
        let mut work = Fixed::new(vec![
            (
                Task::new("gate"),
                Done::Failed { summary: "gate test is red".into(), detail: "2 failed".into() },
            ),
            (Task::new("next"), Done::ok("kept going")),
        ]);
        let report = engine.run(3, "b12", &mut work, 0).expect("run");

        assert_eq!(report.steps, 2, "a red gate is information, not a stop");
        assert_eq!(report.failed, 1);
        let red = records(&root)
            .into_iter()
            .find(|r| r.ok == Some(false))
            .expect("the failure is on the record");
        assert_eq!(red.detail.as_deref(), Some("2 failed"), "with its evidence (`L-16`)");
    }

    #[test]
    fn a_budget_parks_at_the_boundary_rather_than_interrupting() {
        let root = workspace("engine-budget");
        let budgets = Budgets { cycle: Budget::none().tokens(100), batch: Budget::none() };
        let mut engine = Engine::open(&root).expect("open").with_budgets(budgets);

        let mut work = Fixed::new(tasks(5));
        work.spend = Spend { tokens: 250, seconds: 0, money: 0.0 };
        let report = engine.run(3, "b12", &mut work, 0).expect("run");

        assert_eq!(report.steps, 0, "already over before the first step");
        assert!(report.stop.is_none(), "a budget is not one of `L-14`'s stops");
        let park = report.park.expect("parked");
        assert!(park.resumable);
        assert!(park.reason.contains("cycle tokens"), "{}", park.reason);

        let last = records(&root).pop().expect("a record");
        assert_eq!(last.detail.as_deref(), Some("stop=park"));
    }

    #[test]
    fn the_wall_clock_budget_uses_the_engines_clock() {
        let root = workspace("engine-clock");
        let budgets = Budgets { cycle: Budget::none().seconds(10), batch: Budget::none() };
        // Every read is a minute later than the last.
        fn ticking() -> i64 {
            use std::sync::atomic::{AtomicI64, Ordering};
            static T: AtomicI64 = AtomicI64::new(1_700_000_000);
            T.fetch_add(60, Ordering::Relaxed)
        }
        let mut engine =
            Engine::open(&root).expect("open").with_budgets(budgets).with_clock(ticking);
        let report = engine.run(3, "b12", &mut Fixed::new(tasks(5)), 0).expect("run");
        assert!(report.park.is_some(), "time ran out even though nothing cost money");
        assert!(report.spend.seconds >= 10, "{}", report.spend);
    }

    #[test]
    fn requirements_are_cited_on_the_step_that_serves_them() {
        let root = workspace("engine-cites");
        let mut engine = Engine::open(&root).expect("open");
        let mut work = Fixed::new(vec![(
            Task::new("wire the driver").for_requirements(["L-1", "L-2"]),
            Done::ok("done"),
        )]);
        engine.run(3, "b12", &mut work, 0).expect("run");

        let intent = records(&root)
            .into_iter()
            .find(|r| r.kind == Kind::Intent && r.summary.contains("driver"))
            .expect("the step");
        assert_eq!(intent.requirements, ["L-1", "L-2"]);
    }

    #[test]
    fn a_second_run_is_locked_out_while_the_first_holds_the_workspace() {
        let root = workspace("engine-lock");
        let held = Lock::write_lock(
            &root,
            "perp-other",
            &StepId::new(3, "b12", 1).expect("step"),
            time::now(),
        )
        .expect("first");

        let mut engine = Engine::open(&root).expect("open");
        let err = engine
            .run(3, "b12", &mut Fixed::new(tasks(1)), 0)
            .expect_err("one writer at a time (`L-20`)");
        assert!(format!("{err}").contains("perp-other"), "{err}");
        drop(held);
    }

    #[test]
    fn a_second_feature_is_refused_while_one_is_in_flight() {
        let root = workspace("engine-serial");
        let mut engine = Engine::open(&root).expect("open");
        let err = engine
            .run(3, "b12", &mut Fixed::new(tasks(1)), 1)
            .expect_err("one feature at a time (`L-17`)");
        assert!(format!("{err}").contains("L-17"), "{err}");
        assert!(records(&root).is_empty(), "and nothing was journalled");
    }

    #[test]
    fn the_lock_is_released_even_when_the_batch_is_blocked() {
        let root = workspace("engine-unlock");
        let mut engine = Engine::open(&root).expect("open");
        let mut work =
            Fixed::new(vec![(Task::new("a"), Done::Blocked { why: "nope".into() })]);
        engine.run(3, "b12", &mut work, 0).expect("run");

        Lock::write_lock(&root, "perp-next", &StepId::new(3, "b12", 9).expect("step"), time::now())
            .expect("the workspace is not wedged after a blocked batch");
    }

    #[test]
    fn steps_are_numbered_in_one_monotonic_run() {
        let root = workspace("engine-steps");
        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(3, "b12", &mut Fixed::new(tasks(3)), 0).expect("run");
        assert_eq!(report.first_step.map(|s| s.to_string()).as_deref(), Some("c3/b12/s01"));
        assert_eq!(report.last_step.map(|s| s.to_string()).as_deref(), Some("c3/b12/s03"));

        // And a second run continues rather than restarting at one.
        let mut engine = Engine::open(&root).expect("reopen");
        let report = engine.run(3, "b12", &mut Fixed::new(tasks(1)), 0).expect("run");
        assert_eq!(report.first_step.map(|s| s.to_string()).as_deref(), Some("c3/b12/s05"));
    }

    #[test]
    fn taking_over_a_dead_lock_is_journalled_before_any_work() {
        let root = workspace("engine-takeover");
        let dead = Lock::write_lock(
            &root,
            "perp-dead",
            &StepId::new(3, "b12", 1).expect("step"),
            time::now() - crate::lock::DEFAULT_TTL_SECONDS - 10,
        )
        .expect("first");
        std::mem::forget(dead);

        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(3, "b12", &mut Fixed::new(tasks(1)), 0).expect("run");
        assert!(report.took_over.is_some(), "the takeover is reported");

        let first = records(&root).into_iter().next().expect("a record");
        assert!(first.summary.contains("abandoned write lock"), "{}", first.summary);
    }

    #[test]
    fn a_gate_transcript_is_pinned_to_the_commit_it_ran_against() {
        // `G-6`. Found by reading an evidence chain the engine produced:
        // `perp gate` pinned its transcripts and `perp run` did not, so the
        // same gate had provenance from one entry point and none from the
        // other.
        let root = workspace("engine-sha");
        let binding = crate::Binding::load(&root).expect("binding");
        let gates = Gates::from_binding(&binding, &root.join("target")).expect("gates");
        // The fixture is not a repository, so there is no sha to pin to — and
        // the honest answer is `None` rather than a placeholder.
        assert!(gates.sha.is_none(), "no repository, no claim about a commit");

        // Make the fixture a repository and there is one, read once at the top
        // of the run and reused for every gate in it.
        let repo = crate::git::Repo::at(&root);
        for args in [
            vec!["init", "-q", "-b", "perp/fixture"],
            vec!["config", "user.email", "loop@perpetum.test"],
            vec!["config", "user.name", "Perpetum test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            repo.run_unchecked(&args).expect("git");
        }
        repo.stage(&["docs/perpetum.md"]).expect("stage");
        repo.commit(&crate::git::CommitMessage::new("Add the requirements")).expect("commit");

        let pinned = Gates::from_binding(&binding, &root.join("target")).expect("gates");
        assert!(pinned.sha.is_some(), "and here there is one");
    }

    #[test]
    fn a_halt_from_the_work_ends_the_run_with_its_own_reason() {
        let root = workspace("engine-halt");
        let mut engine = Engine::open(&root).expect("open");
        let mut work = Fixed::new(vec![
            (Task::new("a"), Done::Halt(Stop::HumanStop { who: "the operator".into() })),
            (Task::new("b"), Done::ok("never")),
        ]);
        let report = engine.run(3, "b12", &mut work, 0).expect("run");
        assert_eq!(report.stop, Some(Stop::HumanStop { who: "the operator".into() }));
        let last = records(&root).pop().expect("a record");
        assert_eq!(last.detail.as_deref(), Some("stop=human-stop"));
    }
}
