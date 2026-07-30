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

    /// Records the work produced that are not its step outcome — a model call's
    /// accounting, most importantly (`M-11`).
    ///
    /// Drained after each step and appended by the engine. Without this the
    /// agent's spend lived only in memory: the run reported a real number and
    /// `perp cost` reported nothing, because the ledger is replayed from the
    /// journal. Worse, a restarted cycle began its budget at zero — and a
    /// budget that resets on restart is not a budget, which matters most for
    /// the one thing that restarts on purpose (`X-9`).
    fn drain_records(&mut self) -> Vec<crate::journal::Record> {
        Vec::new()
    }

    /// Who to credit for what this work produced, as a git identity (`G-2`).
    ///
    /// `None` when there is nobody to name — better than a trailer git cannot
    /// parse. A first run of the landing wiring wrote `Co-Authored-By: perp`,
    /// which is not an identity and which no forge would read as one.
    fn author(&self) -> Option<String> {
        None
    }

    /// Workspace paths this work wrote to, for staging (`G-3`).
    ///
    /// Default: none, which is correct for work that only reads and runs
    /// commands. Staging is explicit — `git add .` is refused — so a batch that
    /// cannot say what it touched commits nothing rather than sweeping the tree.
    fn touched(&self) -> Vec<String> {
        Vec::new()
    }

    /// Which step the engine is about to run this task under.
    ///
    /// Told rather than guessed: a work that mints its own step ids would be a
    /// second source of them, and every surface cites the same string (`L-22`).
    fn at_step(&mut self, _step: &StepId) {}
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
    /// Controls the operator applied mid-run, in order (`O-3`).
    pub controls: Vec<String>,
    /// Things that went wrong without failing anything (`A-7`).
    pub warnings: Vec<String>,
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
    /// Where the operator asks for a pause, a single step, or a stop (`O-3`).
    channel: crate::control::Channel,
    /// Where per-feature worktrees live, when parallelism is on (`G-11`).
    /// `None` means serial only — and `L-17` then refuses a second feature by
    /// name rather than running two in one tree.
    worktrees: Option<PathBuf>,
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
            channel: crate::control::Channel::at(&crate::layout::dir_in(root)),
            worktrees: None,
            now: time::now,
        })
    }

    pub fn with_budgets(mut self, budgets: Budgets) -> Engine {
        self.budgets = budgets;
        self
    }

    /// Turn on batch-level parallelism (`L-17`), which needs a worktree per
    /// feature (`G-11`). Opt-in: the default is one feature in one tree.
    pub fn with_worktrees(mut self, root: impl Into<PathBuf>) -> Engine {
        self.worktrees = Some(root.into());
        self
    }

    pub fn with_concurrency(mut self, concurrency: Concurrency) -> Engine {
        self.concurrency = concurrency;
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
        // `L-17`: parallel batches need a worktree each, and the engine only
        // claims they are available when it has somewhere to put them.
        self.concurrency.admit(in_flight, self.worktrees.is_some())?;

        let started = (self.now)();
        let owner = Lock::this_process(started);
        let first = self.session.next_step(cycle, stage)?;
        // Under `.harness/`, not at the top of the repository: a lock file loose
        // in someone's project root is litter, and it is the harness's business.
        let mut lock =
            Lock::write_lock(&crate::layout::dir_in(&self.root), &owner, &first, started)?;
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
            controls: Vec::new(),
            warnings: Vec::new(),
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

            // `O-3`: the operator's control, read at the boundary and never
            // mid-step — the same rule as the budget, for the same reason.
            let control = self.channel.read()?;
            if let crate::control::Control::Abort { who } = &control {
                let stop = Stop::HumanStop { who: who.clone() };
                self.record_end(cycle, stage, Some(&stop), None)?;
                report.stop = Some(stop);
                break;
            }
            if !control.keeps_running() {
                let park = Park {
                    reason: format!("paused by the operator ({control})"),
                    resumable: true,
                };
                self.record_end(cycle, stage, None, Some(&park))?;
                report.park = Some(park);
                break;
            }
            if !matches!(control, crate::control::Control::Run) {
                report.controls.push(control.to_string());
            }
            self.channel.consume(&control)?;

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

            work.at_step(&step);
            let guard = self.session.begin_for(
                step,
                &task.summary,
                task.idempotent,
                &task.requirements,
            )?;
            let done = work.perform(&task);

            // `M-11`: whatever the work needs on the record goes on it *before*
            // the step closes, so a crash between the two loses the outcome and
            // not the accounting. Written through the guard's journal because
            // the guard holds the borrow.
            for record in work.drain_records() {
                guard.journal().append(&record)?;
            }

            let halted = match done {
                Done::Ok { summary, detail } => {
                    match detail {
                        Some(detail) => guard.close_with(true, &summary, &detail)?,
                        None => guard.close(true, &summary)?,
                    }
                    false
                }
                Done::Failed { summary, detail } => {
                    report.failed += 1;
                    guard.close_with(false, &summary, &detail)?;
                    false
                }
                Done::Blocked { why } => {
                    guard.close_with(false, &format!("blocked: {why}"), &why)?;
                    let stop = Stop::BatchBlocked { batch: stage.to_string(), why };
                    self.record_end(cycle, stage, Some(&stop), None)?;
                    report.stop = Some(stop);
                    true
                }
                Done::Halt(stop) => {
                    guard.close(true, &stop.summary())?;
                    self.record_end(cycle, stage, Some(&stop), None)?;
                    report.stop = Some(stop);
                    true
                }
            };

            // `A-3`: regenerated by the engine after each closed step, not by
            // someone remembering to ask — and after every closed step, not only
            // the ones that went well. A run whose steps all failed used to
            // write nothing at all, which is precisely the run someone needs a
            // board for. `A-7` keeps a render that fails a warning: losing a
            // step because a diagram would not draw is absurd.
            report.warnings.extend(self.refresh_artifacts());
            if halted {
                break;
            }
        }

        lock.release()?;
        Ok(report)
    }

    /// Rewrite every artifact from the journal (`A-1`, `A-3`, `O-2`).
    ///
    /// All seven, not just the board. They are all projections of the same
    /// records, so the expensive half — reading the journal and replaying it —
    /// is done once and shared; what each kind adds is the string it formats.
    /// Six of them used to need `perp artifact all` typed by hand, which meant
    /// that in practice they were stale or absent, and an artifact nobody has is
    /// not evidence of anything.
    ///
    /// Warnings rather than errors, one per kind that failed: artifacts are
    /// never on the critical path (`A-7`), and a harness that can lose a batch
    /// because a diagram would not draw will eventually do it at 3am. A kind
    /// that fails does not stop the other six from being written.
    fn refresh_artifacts(&self) -> Vec<String> {
        let Ok(records) = self.session.journal().read_all() else { return Vec::new() };
        let projection = crate::state::replay(&records);
        let provenance = crate::artifact::Provenance::from_journal(
            projection.cycle.unwrap_or(1),
            (self.now)(),
            &records,
        )
        .sha(crate::git::Repo::at(&self.root).head_sha().ok());

        let dir = crate::artifact::dir_in(&self.root);
        let mut warnings = Vec::new();
        for kind in crate::artifact::Kind::ALL {
            match crate::artifact::try_render(kind, &projection, &records, &provenance) {
                Ok(artifact) => {
                    if let Err(e) = artifact.write(&dir) {
                        warnings.push(format!("{}: {e}", kind.as_str()));
                    }
                }
                Err(warning) => warnings.push(format!("{warning}")),
            }
        }
        warnings
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
    /// The requirements this gate is evidence *for* (`G-6`).
    ///
    /// A gate step used to cite `V-2` and nothing else, so `perp explain T-2`
    /// answered "no gate transcript on any of these steps" for a requirement
    /// whose batch had gone green minutes earlier. The transcript existed; no
    /// requirement could reach it. Citing the batch's ids as well is what makes
    /// the evidence chain a chain.
    covering: Vec<String>,
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
            covering: Vec::new(),
            results: Vec::new(),
        })
    }

    /// Name the requirements this gate stands as evidence for (`G-6`).
    pub fn covering<I, S>(mut self, ids: I) -> Gates
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.covering = ids.into_iter().map(Into::into).collect();
        self
    }

    pub fn all_green(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(gate::GateResult::is_green)
    }
}

impl Work for Gates {
    fn next(&mut self) -> Option<Task> {
        let gate = self.gates.get(self.next)?;
        // The batch's own ids, and nothing else.
        //
        // This used to cite `V-2` as well — the harness's own requirement for
        // "only a gate may call something green". True, and the wrong place to
        // say it: `V-2` is defined in the harness's requirements, not in the
        // project being worked on, so every run stamped an id into the target's
        // journal that the target had never heard of. `perp check ids` then
        // reported it as used-but-never-defined and could not pass on any
        // workspace the harness drove — the tool contradicting the loop that
        // shipped with it.
        //
        // Nothing is lost. The step summary says `gate: test`, the transcript is
        // in the record, and the step id is what every other surface cites. An
        // empty list is honest for a phase with no requirements in flight; every
        // reader already renders it as no trailer rather than as a blank one.
        Some(
            Task::new(format!("gate: {}", gate.name))
                .idempotent()
                .for_requirements(self.covering.clone()),
        )
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
        std::fs::create_dir_all(dir.join(".harness")).expect("dirs");
        std::fs::write(dir.join(".harness/perpetum.md"), "# requirements\n").expect("requirements");
        std::fs::write(
            dir.join(".harness/binding.md"),
            "```perp-binding\n\
             path.requirements = .harness/perpetum.md\n\
             out.journal = .harness/journal.jsonl\n\
             out.state = .harness/state.md\n\
             gate.check = cargo --version\n\
             ```\n",
        )
        .expect("binding");
        dir
    }

    #[allow(clippy::expect_used)]
    fn records(root: &Path) -> Vec<crate::Record> {
        Journal::at(root.join(".harness/journal.jsonl")).read_all().expect("read")
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
            &crate::layout::dir_in(&root),
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

        Lock::write_lock(&crate::layout::dir_in(&root), "perp-next", &StepId::new(3, "b12", 9).expect("step"), time::now())
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
            &crate::layout::dir_in(&root),
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
    fn a_gate_is_evidence_for_the_batch_it_gated_and_cites_nothing_else() {
        // Two findings, a day apart, pulling the same string.
        //
        // First: `perp explain T-2` said "no gate transcript on any of these
        // steps" for a requirement whose batch had gone green in the same leg,
        // because the gate cited `V-2` and nothing else — so no requirement in
        // the batch could reach the transcript that cleared it.
        //
        // Then: citing `V-2` at all put the harness's own requirement id into
        // the target project's journal, where it is not defined, so
        // `perp check ids` reported a stray on every workspace the harness
        // drove. The batch's ids are the answer to both.
        let root = workspace("engine-covering");
        let binding = crate::Binding::load(&root).expect("binding");
        let target = root.join("target");

        // A phase with nothing in flight cites nothing. The step summary and the
        // step id carry the provenance, and every reader renders an empty list as
        // no trailer rather than as a blank one.
        let mut bare = Gates::from_binding(&binding, &target).expect("gates");
        let task = Work::next(&mut bare).expect("a gate");
        assert!(task.requirements.is_empty(), "nothing covered, nothing cited: {task:?}");

        let mut covering = Gates::from_binding(&binding, &target)
            .expect("gates")
            .covering(["T-2", "T-3"]);
        let task = Work::next(&mut covering).expect("a gate");
        assert_eq!(
            task.requirements,
            vec!["T-2".to_string(), "T-3".to_string()],
            "the batch's own ids, so each can find this transcript"
        );
        assert!(
            !task.requirements.contains(&"V-2".to_string()),
            "and never the harness's own id, which the target has never heard of"
        );
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
        repo.stage(&[".harness/perpetum.md"]).expect("stage");
        repo.commit(&crate::git::CommitMessage::new("Add the requirements")).expect("commit");

        let pinned = Gates::from_binding(&binding, &root.join("target")).expect("gates");
        assert!(pinned.sha.is_some(), "and here there is one");
    }

    #[test]
    fn a_pause_stops_the_loop_at_the_next_boundary_and_parks() {
        // `O-3`. Not a signal: the operator and the loop are separate
        // processes, often on separate days, and a control file works when
        // only one of them is alive.
        let root = workspace("engine-pause");
        crate::control::Channel::at(&crate::layout::dir_in(&root))
            .ask(&crate::control::Control::Pause)
            .expect("ask");

        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b17", &mut Fixed::new(tasks(3)), 0).expect("run");

        assert_eq!(report.steps, 0, "paused before the first step");
        assert!(report.stop.is_none(), "a pause is not one of `L-14`'s stops");
        let park = report.park.expect("parked");
        assert!(park.resumable, "and it resumes");
        assert!(park.reason.contains("operator"), "{}", park.reason);
    }

    #[test]
    fn a_single_step_runs_exactly_one_and_then_pauses() {
        let root = workspace("engine-single-step");
        let channel = crate::control::Channel::at(&crate::layout::dir_in(&root));
        channel.ask(&crate::control::Control::Step).expect("ask");

        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b17", &mut Fixed::new(tasks(5)), 0).expect("run");

        assert_eq!(report.steps, 1, "exactly one, out of five available");
        assert!(report.park.is_some(), "and then it parked");
        assert_eq!(
            channel.read().expect("read"),
            crate::control::Control::Pause,
            "the control consumed itself"
        );
    }

    #[test]
    fn an_abort_writes_a_human_stop_rather_than_a_park() {
        let root = workspace("engine-abort");
        crate::control::Channel::at(&crate::layout::dir_in(&root))
            .ask(&crate::control::Control::Abort { who: "the operator".into() })
            .expect("ask");

        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b17", &mut Fixed::new(tasks(3)), 0).expect("run");

        assert_eq!(report.stop, Some(Stop::HumanStop { who: "the operator".into() }));
        assert!(report.park.is_none(), "an abort is terminal, a pause is not");
        let last = records(&root).pop().expect("a record");
        assert_eq!(last.detail.as_deref(), Some("stop=human-stop"));
    }

    #[test]
    fn an_injection_applies_once_and_is_on_the_report() {
        let root = workspace("engine-inject");
        let channel = crate::control::Channel::at(&crate::layout::dir_in(&root));
        channel
            .ask(&crate::control::Control::Inject { text: "prefer the other helper".into() })
            .expect("ask");

        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b17", &mut Fixed::new(tasks(2)), 0).expect("run");

        assert_eq!(report.steps, 2, "an injection does not stop the loop");
        assert_eq!(report.controls.len(), 1, "and it applied once, not at every boundary");
        assert!(report.controls[0].contains("other helper"), "{:?}", report.controls);
    }

    #[test]
    fn parallel_batches_are_reachable_only_with_somewhere_to_put_them() {
        // `L-17` with `G-11`. Before this the engine passed `false`
        // unconditionally, so opting in was unreachable rather than merely
        // unused — and marking the requirement done would have been a claim
        // about a code path nothing could take.
        let root = workspace("engine-parallel");

        let mut serial = Engine::open(&root)
            .expect("open")
            .with_concurrency(Concurrency::Parallel { max: 2 });
        let err = serial
            .run(4, "b19", &mut Fixed::new(tasks(1)), 0)
            .expect_err("no worktree root, no parallelism");
        assert!(format!("{err}").contains("G-11"), "{err}");

        let mut parallel = Engine::open(&root)
            .expect("open")
            .with_concurrency(Concurrency::Parallel { max: 2 })
            .with_worktrees(root.join("../worktrees"));
        let report = parallel
            .run(4, "b19", &mut Fixed::new(tasks(1)), 1)
            .expect("one already in flight, under the maximum of two");
        assert_eq!(report.steps, 1);
    }

    #[test]
    fn the_board_is_rewritten_by_the_engine_rather_than_on_request() {
        // `A-3`. On demand means "when someone remembers", and the board is the
        // surface a person checks precisely when they were not watching.
        let root = workspace("engine-board");
        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b20", &mut Fixed::new(tasks(2)), 0).expect("run");

        let board = root.join(".harness/artifacts/board.html");
        assert!(board.is_file(), "no board at {}", board.display());
        let html = std::fs::read_to_string(&board).expect("read");
        assert!(html.contains("Progress board"), "{}", &html[..200.min(html.len())]);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }


    #[test]
    fn a_run_whose_step_failed_still_leaves_a_board() {
        // The board existed only on the success path, so the run someone most
        // needs to look at — the one where everything went red — wrote nothing
        // to look at.
        let root = workspace("engine-artifacts-failed");
        let mut engine = Engine::open(&root).expect("open");
        let red = vec![(
            Task::new("a step that goes red"),
            Done::Failed { summary: "gate red".into(), detail: "1 of 1 failed".into() },
        )];
        let report = engine
            .run(4, "b22", &mut Fixed::new(red), 0)
            .expect("a failed step is not a failed run");
        assert_eq!(report.failed, 1, "the step failed");

        let dir = crate::artifact::dir_in(&root);
        for kind in crate::artifact::Kind::ALL {
            let path = dir.join(kind.file_name());
            assert!(path.is_file(), "no {} after a failed step", kind.as_str());
        }
    }
    #[test]
    fn every_kind_is_written_by_a_run_and_not_only_the_board() {
        // `A-1` says there are seven. The engine wrote one and the other six
        // needed `perp artifact all` typed by hand, so in practice they were
        // absent or stale — and an artifact nobody has is not evidence.
        let root = workspace("engine-artifacts-all");
        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b21", &mut Fixed::new(tasks(1)), 0).expect("run");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let dir = crate::artifact::dir_in(&root);
        for kind in crate::artifact::Kind::ALL {
            let path = dir.join(kind.file_name());
            assert!(path.is_file(), "no {} at {}", kind.as_str(), path.display());
            let html = std::fs::read_to_string(&path).expect("read");
            assert!(
                html.contains(kind.title()),
                "{} does not carry its own title",
                kind.as_str()
            );
            // `A-6`: provenance or it is decoration.
            assert!(html.contains("Provenance"), "{} has no provenance", kind.as_str());
        }
    }

    #[test]
    fn a_board_that_will_not_render_is_a_warning_and_not_a_failed_step() {
        // `A-7` on the engine's own path: the directory is made a file, so the
        // write cannot succeed. The steps still close green.
        let root = workspace("engine-board-warn");
        let artifacts = crate::artifact::dir_in(&root);
        std::fs::create_dir_all(artifacts.parent().unwrap_or(&root)).expect("dirs");
        std::fs::write(&artifacts, "not a directory").expect("write");

        let mut engine = Engine::open(&root).expect("open");
        let report = engine.run(4, "b20", &mut Fixed::new(tasks(1)), 0).expect("run");

        assert_eq!(report.steps, 1, "the step still ran");
        assert_eq!(report.failed, 0, "and still passed");
        assert!(!report.warnings.is_empty(), "and the board failure is reported, not swallowed");
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
