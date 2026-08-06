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

    /// The requirements this work actually delivered something for (`V-14`).
    ///
    /// Delivered, not attempted. A requirement whose step read forty files and
    /// wrote none of them is not in this list, and so collects no gate.
    ///
    /// Default: none. A work that cannot say what it delivered attributes no
    /// evidence, which is the safe direction — the failure this exists for is
    /// a requirement wearing a green gate it did nothing to earn.
    fn delivered(&self) -> Vec<String> {
        Vec::new()
    }

    /// Actions a person has approved **in this cycle**, and who approved them
    /// (`T-15`).
    ///
    /// Told before each step, because the answer changes between them — that
    /// is the whole point of a queue somebody answers while the loop runs. A
    /// work that cached this once would act on yesterday's permission.
    fn granted(&mut self, _actions: Vec<(String, String)>) {}

    /// Calls this work refused for want of a person, to be raised into the
    /// queue (`T-14`).
    ///
    /// Drained by the engine after each step, like [`Work::drain_records`]. The
    /// work does not raise them itself: only the engine knows the step and
    /// cycle, and only the queue can mint an id.
    fn drain_approvals(&mut self) -> Vec<crate::approval::Ask> {
        Vec::new()
    }

    /// Ask an independent link to review what the step just produced (`V-5`).
    ///
    /// Returns the verdict text, or `None` when there is nothing to review or
    /// nobody independent to ask. The engine journals it; the work does not,
    /// because a reviewer that files its own verdict is a reviewer whose verdict
    /// is worth what `V-2` says self-reported success is worth.
    ///
    /// Default: `None`. Work with no model has nobody to ask, which is the
    /// honest answer for the gate runner.
    fn review(&mut self) -> Option<String> {
        None
    }

    /// Told what the preceding work delivered, before this one runs (`V-14`).
    ///
    /// The gate is built before the agent runs, so it cannot know at
    /// construction which requirements will still be standing by the time it
    /// runs. [`crate::cycle::Then`] tells it at the hand-over.
    fn covers(&mut self, _delivered: Vec<String>) {}
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
    /// Calls this run could not make without a person (`T-14`).
    pub approvals_raised: u32,
    /// Whether the gates ran green — `None` when none ran.
    ///
    /// A failed step and a red gate are different things, and a report that
    /// carries only the first cannot tell the difference. The driver said "1
    /// step(s) failed and the gate is red" on a cycle whose last two gate runs
    /// both exited 0; the step had failed on a tool error. The stop reason is
    /// the one line an operator reads to decide whether to look, so a run that
    /// ends green while announcing red teaches them to stop reading it.
    pub gates_green: Option<bool>,
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
    /// Requests waiting for a person (`T-14`), replayed from the journal on
    /// open so that killing the loop does not empty it.
    approvals: crate::approval::Queue,
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
        // Rebuilt from the journal, so killing the loop does not empty the
        // queue (`N-2`: a cold start reads the binding and the journal, and
        // killing the loop is the normal way to stop it).
        let approvals = match session.journal().read_all() {
            Ok(records) => crate::approval::Queue::replay(&records),
            Err(_) => crate::approval::Queue::new(),
        };
        Ok(Engine {
            session,
            budgets,
            concurrency: Concurrency::default(),
            root: root.to_path_buf(),
            channel: crate::control::Channel::at(root),
            worktrees: None,
            now: time::now,
            approvals,
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

    /// What `cycle` has already spent, from the journal (`L-10`, `M-11`).
    ///
    /// Scoped to the cycle, because the budget is: `budget.cycle.*` is a
    /// ceiling for one cycle, and replaying every record ever written would
    /// charge cycle 12 for cycle 1.
    ///
    /// Counted from what the links actually reported and never estimated
    /// (`L-10`) — the ledger's entries are the `M-11` call records, so a
    /// `local-only` run carries zero money forward and a real one carries what
    /// it was charged. A journal that cannot be read is treated as nothing
    /// carried rather than as a reason to refuse to start: the budget then
    /// behaves as it did before this existed, which is the safe direction for
    /// a read that is itself best-effort.
    fn carried_spend(&self, cycle: u32) -> Spend {
        let Ok(records) = self.session.journal().read_all() else {
            return Spend::default();
        };
        let mine: Vec<crate::journal::Record> =
            records.into_iter().filter(|r| r.step.cycle == cycle).collect();
        let ledger = crate::cost::Ledger::replay(&mine);
        // Seconds are this run's; see the note at the call site.
        Spend::from_ledger(&ledger, 0)
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

        // `T-15`, the other half: a grant belongs to the cycle it was given in
        // and to no later one. Dropped here rather than at the end of the
        // previous cycle, because a cycle can end by being killed — and a rule
        // enforced only on the clean path is not enforced. Whatever survived in
        // the journal from an earlier cycle stops counting the moment a new one
        // starts.
        let dropped = self.approvals.close_cycle(cycle);
        if dropped > 0 {
            crate::verbose::say(
                "approvals",
                &format!("{dropped} from an earlier cycle no longer count (`T-15`)"),
            );
        }

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
            gates_green: None,
            spend: Spend::default(),
            took_over,
            first_step: None,
            last_step: None,
            controls: Vec::new(),
            warnings: Vec::new(),
            approvals_raised: 0,
        };

        // `L-10`: what this cycle spent before this process existed.
        //
        // `M-11` writes a record per call so "the ledger and the budget survive
        // a restart" — and nothing read it back, so they did not. Spend was the
        // in-memory accumulator of one run, and `L-7` treats a restart as
        // ordinary: a loop that crashed and resumed began its cycle budget
        // again at zero, and could spend the ceiling once per crash while every
        // individual run reported itself inside it.
        //
        // Read **once**, before the loop. The agent's own records land in this
        // same journal as the run proceeds, so re-reading would count this
        // run's calls twice — once here and once in `work.spend()`.
        let carried = self.carried_spend(cycle);

        loop {
            // The boundary. Checked before the next task begins and never
            // during one.
            let elapsed = (self.now)() - started;
            let spend = work.spend();
            // Wall-clock is deliberately *not* carried. The journal records
            // what each call cost, not how long the loop was awake, and the
            // only figure derivable from it is calendar time between the
            // cycle's first record and now — which would charge a cycle parked
            // overnight for the hours nobody was running it. Tokens and money
            // accumulate; seconds are this run's.
            report.spend = Spend {
                seconds: elapsed,
                tokens: carried.tokens + spend.tokens,
                money: carried.money + spend.money,
            };
            // The two scopes get the two figures. `check` has always taken them
            // separately and the engine had always passed the same value twice,
            // which was a smell while that value was one run's spend and became
            // a bug the moment `L-10`'s carry-forward went in above: the batch
            // ceiling was then compared against the *cycle's* accumulated
            // total, so a `budget.batch.tokens` lower than the cycle's parked
            // the whole cycle once cumulative spend passed it — before doing
            // any work at all.
            //
            // Measured on Janitor: `0 steps — parked: budget: batch tokens —
            // 1712557 of 1500000`, on a batch that had spent nothing.
            //
            // A batch's spend is this run's. The carry-forward is a fact about
            // the cycle and belongs only to the cycle.
            let batch_spend = Spend { seconds: elapsed, ..spend };
            if let Verdict::Exhausted { .. } = self.budgets.check(report.spend, batch_spend) {
                let park = self
                    .budgets
                    .check(report.spend, batch_spend)
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

            // `V-1`, Perpetum 0.7: look before building, and record what was
            // found whether or not it found anything.
            //
            // The machinery for this existed and nothing called it.
            // `RealityCheck::run` and `verify::may_implement` were both written,
            // both tested, and had no caller in the engine — so every
            // requirement built by this harness carried "no reality check
            // recorded (`V-1`)" in its evidence chain, and the mandatory step
            // was mandatory only in prose.
            self.record_reality_check(&step, &task)?;

            work.at_step(&step);
            // `T-15`: what a person has approved for *this* cycle, refreshed
            // before every step — the queue is answered while the loop runs, so
            // reading it once would act on a stale answer.
            work.granted(self.approvals.granted_in(cycle));
            let step_for_verdict = step.clone();
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
            // `V-5`: an independent link reads what this step wrote, and its
            // verdict goes on the record. Asked here rather than inside the
            // work, because a reviewer that files its own verdict is worth what
            // `V-2` says self-reported success is worth.
            // `T-14`: what the step could not do without a person goes on the
            // record and into the queue, and the loop carries on (`L-19`).
            for ask in work.drain_approvals() {
                let request = ask.into_request(step_for_verdict.clone(), cycle, (self.now)());
                let id = self.approvals.raise(request.clone());
                let mut raised = request;
                raised.id = id;
                guard
                    .journal()
                    .append(&crate::approval::Queue::raised_record(&raised, (self.now)()))?;
                report.approvals_raised += 1;
            }

            // `T-16`: a request nobody answered inside its window is parked,
            // not waited on. The reason is its own — `approval-gated` is
            // counted apart from done and from blocked (`V-8`), because it is
            // neither the loop's fault nor its to fix.
            for expired in self.approvals.expire((self.now)()) {
                guard.journal().append(
                    &crate::journal::Record::outcome(
                        step_for_verdict.clone(),
                        (self.now)(),
                        false,
                        format!("approval #{} expired unanswered: {}", expired.id, expired.what),
                    )
                    .with_detail(format!(
                        "approval-expired\nid={}\nparked with reason approval-gated (`T-16`)\n",
                        expired.id
                    )),
                )?;
                // `O-10`: an expiry is a fork nobody attended — the window
                // closed and the loop chose to park rather than proceed
                // unapproved (`T-16`).
                guard.journal().append(
                    &crate::decision::Decision::new(
                        "park as approval-gated",
                        vec!["proceed unapproved".into(), "keep waiting".into()],
                        format!("nobody answered approval #{} inside its window", expired.id),
                        crate::decision::Decider::Rule { cites: "T-16".into() },
                        step_for_verdict.clone(),
                        (self.now)(),
                    )
                    .to_record(),
                )?;
                report.warnings.push(format!(
                    "approval #{} expired unanswered — parked as approval-gated",
                    expired.id
                ));
            }

            let verdict = work.review();

            for record in work.drain_records() {
                guard.journal().append(&record)?;
            }
            if let Some(verdict) = verdict {
                guard.journal().append(
                    &crate::journal::Record::outcome(
                        step_for_verdict.clone(),
                        (self.now)(),
                        true,
                        "independent review",
                    )
                    .with_detail(verdict)
                    .for_requirements(task.requirements.clone()),
                )?;
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

    /// Look before building, and put what was found on the record (`V-1`).
    ///
    /// Runs on the engine's side of the boundary on purpose. Perpetum 0.7 makes
    /// the search mandatory, and a mandatory step the model performs is one the
    /// model can decline to perform — `V-2`'s rule about self-reported success
    /// applies to self-reported searching for exactly the same reason.
    ///
    /// A check that finds the work already present does not stop the step. It
    /// is evidence, and the loop is entitled to build on top of something that
    /// exists; what it is not entitled to do is not look. `already_built` and
    /// `was_removed` ride along in the record so a reader can see which it was.
    ///
    /// The needles are the requirement id and the words of its summary long
    /// enough to mean anything — the same thing a person would grep for.
    fn record_reality_check(&mut self, step: &StepId, task: &Task) -> Result<()> {
        let repo = crate::git::Repo::at(&self.root);
        for requirement in &task.requirements {
            let mut needles: Vec<String> = vec![requirement.clone()];
            needles.extend(
                task.summary
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .filter(|word| word.len() > 4)
                    .take(4)
                    .map(str::to_string),
            );
            let borrowed: Vec<&str> = needles.iter().map(String::as_str).collect();
            let Ok(check) = crate::verify::RealityCheck::run(&repo, requirement, &borrowed) else {
                // A repository that cannot be searched is not a reason to skip
                // the step silently — it is recorded as the check failing, so
                // the gap is visible rather than invisible.
                self.session.journal().append(
                    &crate::journal::Record::outcome(
                        step.clone(),
                        (self.now)(),
                        false,
                        "reality check could not run",
                    )
                    .for_requirements([requirement.clone()]),
                )?;
                continue;
            };
            // A citation is not an implementation.
            //
            // The requirement id is the obvious needle and the misleading one:
            // this codebase cites ids in doc comments on purpose, so
            // `threshold.dart` matched `R-12` because it says "the contour
            // tracer of `R-12` consumes this representation" — a forward
            // reference to work that did not exist. The check then reported
            // `R-12` already present, which is the one answer that tells a loop
            // to stop. Measured on a Flutter backlog, where it was the only
            // match outside the requirements file itself.
            //
            // Matches in the requirements source are discounted for the same
            // reason: that is where ids are *defined* (`V-9`), so finding one
            // there says only that the requirement exists.
            let implemented: Vec<&String> = check
                .in_tree
                .iter()
                .filter(|path| !path.contains("requirements"))
                .filter(|path| !path.ends_with(".md"))
                .collect();
            let summary = if !implemented.is_empty() {
                format!(
                    "reality check: {requirement} may already be present — {}",
                    implemented.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")
                )
            } else if check.already_built() {
                format!(
                    "reality check: {requirement} is cited in documentation but not implemented"
                )
            } else if check.was_removed() {
                format!("reality check: {requirement} is absent now but the history touched it")
            } else {
                format!("reality check: {requirement} is not there yet")
            };
            self.session.journal().append(
                &crate::journal::Record::outcome(step.clone(), (self.now)(), true, summary)
                    .with_detail(check.evidence())
                    .for_requirements([requirement.clone()]),
            )?;

            // `O-10`: a reality check that finds the work already present is a
            // fork — build on it, or build it again. The loop takes the first
            // silently, and a reader a week later cannot tell it was ever a
            // question (`V-1`).
            if !implemented.is_empty() {
                self.session.journal().append(
                    &crate::decision::Decision::new(
                        "build on what is there",
                        vec!["implement it again".into()],
                        {
                            // Named, not listed. A generic requirement matches
                            // half the repository, and a reason that is
                            // twenty-four paths long is one nobody reads.
                            let shown: Vec<&str> =
                                implemented.iter().take(3).map(|p| p.as_str()).collect();
                            let rest = implemented.len().saturating_sub(shown.len());
                            if rest > 0 {
                                format!(
                                    "already present in {} and {rest} other file(s)",
                                    shown.join(", ")
                                )
                            } else {
                                format!("already present in {}", shown.join(", "))
                            }
                        },
                        crate::decision::Decider::Rule { cites: "V-1".into() },
                        step.clone(),
                        (self.now)(),
                    )
                    .for_requirement(requirement.clone())
                    .to_record(),
                )?;
            }
        }
        Ok(())
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

    /// Get the default runtime from the first gate, if available.
    pub fn default_runtime(&self) -> Option<&crate::runtime::Runtime> {
        self.gates.first().map(|gate| &gate.runtime)
    }

    /// Override the runtime for all gates in this set (`L-17`).
    ///
    /// Used to route gates through persistent containers when batch-level
    /// parallelism is enabled. The command itself is never rewritten; only
    /// the runtime wrapper changes.
    pub fn with_runtime(mut self, runtime: crate::runtime::Runtime) -> Gates {
        self.gates = self
            .gates
            .into_iter()
            .map(|gate| gate.in_runtime(runtime.clone()))
            .collect();
        self
    }

    pub fn all_green(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(gate::GateResult::is_green)
    }

    /// Green, red, or never ran — as three answers rather than two.
    ///
    /// [`Gates::all_green`] folds "no gate ran" in with "a gate failed", which
    /// is right for deciding whether to commit and wrong for saying what
    /// happened. A caller that reports the second as the first tells an
    /// operator the gate is red when nothing was ever run.
    pub fn verdict(&self) -> Option<bool> {
        if self.results.is_empty() {
            return None;
        }
        Some(self.results.iter().all(gate::GateResult::is_green))
    }

    /// May a batch's code be committed on the strength of these gates (`G-16`)?
    ///
    /// Not the same question as "did anything fail", and the difference is the
    /// whole point. A run that stops before its gate steps — paused, out of
    /// budget, killed — has a failure count of zero, because zero gates ran.
    /// Deciding on that count lands a commit saying `Deliver J-23` on a tree
    /// nothing has compiled.
    ///
    /// A project that declares no gate at all is a different case and keeps its
    /// behaviour: it opted out, and there is no verdict being ignored.
    pub fn approved_for_commit(&self) -> bool {
        self.gates.is_empty() || self.all_green()
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
                // `G-6`: a sha is provenance only if the tree it names is the
                // tree that ran. Asked at gate time rather than at construction,
                // because the step in between is the one that dirties it.
                let result = match crate::git::Repo::at(&self.target).is_clean() {
                    Ok(clean) => result.with_tree(clean),
                    Err(_) => result,
                };
                let evidence = result.evidence();
                let green = result.is_green();
                let name = gate.name.clone();
                self.results.push(result);
                if green {
                    Done::ok_with(format!("gate {name} is green"), evidence)
                } else {
                    // `S-5`: an authentication wall is not a red gate. A red
                    // gate is information and the loop retries it (`L-16`);
                    // retrying a 401 changes nothing, and the obvious next
                    // thing a model reaches for after a failed retry is a
                    // credential — from the environment, from a config file,
                    // from a guess. `Blocked` rather than `Failed` is what
                    // stops that path existing: no retry, and the reason
                    // carries the verbatim wall.
                    //
                    // A gate that may legitimately need one says so in the
                    // binding instead, and borrows it (`S-9`).
                    if let Some(wall) = crate::security::credential_gate(&name, &evidence) {
                        return Done::Blocked { why: wall.to_string() };
                    }
                    Done::Failed { summary: format!("gate {name} is red"), detail: evidence }
                }
            }
            Err(e) => Done::Blocked { why: format!("gate {} could not run: {e}", gate.name) },
        }
    }

    /// Cite what the work delivered, not what it was handed (`V-14`).
    ///
    /// The batch's whole list goes in at construction, because the branch needs
    /// it before anything runs. By the time the gate itself runs the agent has
    /// finished, and the honest list is usually shorter. Cycle 10 filed three
    /// green gates against three requirements that changed nothing: the gates
    /// were real and green, and had measured a tree none of them had touched.
    fn covers(&mut self, delivered: Vec<String>) {
        self.covering = delivered;
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

    /// The batch ceiling is a fact about the batch, not about the cycle.
    ///
    /// `check` takes the two spends separately and the engine passed the same
    /// value to both. Harmless while that value was one run's spend; a bug the
    /// moment the cycle's carry-forward went into it, because the batch ceiling
    /// then saw the cycle's running total and parked a batch that had spent
    /// nothing. A cycle already past `budget.batch.tokens` could do no further
    /// work at all, whatever its own ceiling said.
    #[test]
    fn a_batch_ceiling_is_not_charged_for_what_the_cycle_already_spent() {
        let root = workspace("engine-budget-scopes");

        // 250 tokens already on this cycle's ledger, from a previous run.
        let spent = crate::Record::outcome(
            StepId::new(7, "b1", 1).expect("step"),
            1_700_000_000,
            true,
            "a model call",
        );
        let spent = crate::Record {
            extra: vec![
                ("link".to_string(), crate::json::Value::str("here")),
                ("cache_miss".to_string(), crate::json::Value::int(200)),
                ("output_tokens".to_string(), crate::json::Value::int(50)),
            ],
            ..spent
        };
        Journal::at(root.join(".harness/journal.jsonl")).append(&spent).expect("append");

        // A batch ceiling the cycle is already past, and a cycle ceiling it is
        // not. The batch itself spends nothing.
        let budgets = Budgets {
            cycle: Budget::none().tokens(10_000),
            batch: Budget::none().tokens(100),
        };
        let mut engine = Engine::open(&root).expect("open").with_budgets(budgets);
        let mut work = Fixed::new(tasks(2));
        work.spend = Spend::default();

        let report = engine.run(7, "b2", &mut work, 0).expect("run");

        assert!(
            report.park.is_none(),
            "a batch that spent nothing must not be parked for the cycle's history: {:?}",
            report.park
        );
        assert!(report.steps > 0, "and it must actually do its work");
        assert_eq!(report.spend.tokens, 250, "while the cycle still counts what it spent");
    }

    /// `L-10`: a restart does not hand the cycle its budget back.
    ///
    /// `M-11` writes a record per call so "the ledger and the budget survive a
    /// restart", and nothing read it back, so they did not. `L-7` treats a
    /// restart as ordinary — a loop that crashed and resumed began the cycle
    /// budget again at zero and could spend the ceiling once per crash while
    /// every run reported itself inside it.
    ///
    /// The second engine here is a genuinely separate one over the same
    /// workspace, which is what a resume is.
    #[test]
    fn a_cycle_budget_is_not_refunded_by_restarting() {
        let root = workspace("engine-budget-restart");

        // What a previous run spent, on the journal exactly as `M-11` leaves
        // it: 250 tokens against cycle 3.
        let spent = crate::Record::outcome(
            StepId::new(3, "b1", 1).expect("step"),
            1_700_000_000,
            true,
            "a model call",
        )
        ;
        let spent = crate::Record {
            extra: vec![
                ("link".to_string(), crate::json::Value::str("here")),
                ("role".to_string(), crate::json::Value::str("coder")),
                ("cache_miss".to_string(), crate::json::Value::int(200)),
                ("output_tokens".to_string(), crate::json::Value::int(50)),
            ],
            ..spent
        };
        Journal::at(root.join(".harness/journal.jsonl")).append(&spent).expect("append");

        // A new process, over the same workspace, spending nothing itself.
        let budgets = Budgets { cycle: Budget::none().tokens(100), batch: Budget::none() };
        let mut engine = Engine::open(&root).expect("open").with_budgets(budgets);
        let mut work = Fixed::new(tasks(5));
        work.spend = Spend::default();

        let report = engine.run(3, "b2", &mut work, 0).expect("run");

        assert_eq!(report.steps, 0, "the cycle was already over its ceiling before this run");
        let park = report.park.expect("a restart must not refund the budget");
        assert!(park.reason.contains("cycle tokens"), "{}", park.reason);
        assert_eq!(report.spend.tokens, 250, "carried from the journal, not from memory");

        // And a *different* cycle is not charged for it — the ceiling is per
        // cycle, so replaying every record ever written would bill cycle 4 for
        // cycle 3.
        let mut fresh = Engine::open(&root)
            .expect("open")
            .with_budgets(Budgets { cycle: Budget::none().tokens(100), batch: Budget::none() });
        let mut more = Fixed::new(tasks(1));
        more.spend = Spend::default();
        let next = fresh.run(4, "b1", &mut more, 0).expect("run");
        assert!(next.park.is_none(), "cycle 4 does not inherit cycle 3's spend");
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

    /// `G-16`: a gate that never ran is not a gate that passed.
    ///
    /// Measured, not hypothetical. Janitor's cycle 9 was paused after the coder
    /// committed and before the three gate steps, and the commit guard asked
    /// `report.failed == 0` — true, because nothing ran to fail. The branch
    /// kept `Deliver J-23` on a tree with a stray token in it that `cargo
    /// build` rejects, and the next run started from a base that would not
    /// compile. `all_green` already answered this correctly and the decision
    /// read a different value; this is the wiring, not the logic.
    #[test]
    fn a_gate_that_never_ran_does_not_approve_a_commit() {
        let root = workspace("engine-unrun-gates");
        let binding = crate::Binding::load(&root).expect("binding");
        let target = root.join("target");

        let gates = Gates::from_binding(&binding, &target).expect("gates");
        assert!(!gates.gates.is_empty(), "this fixture declares gates, else the test proves nothing");
        assert_eq!(gates.verdict(), None, "nothing ran, so there is no verdict to have");
        assert!(
            !gates.approved_for_commit(),
            "unrun is unverified: the old guard read `failed == 0` here and committed"
        );
    }

    /// `S-5`: a gate that hits an authentication wall is blocked, not retried.
    ///
    /// The distinction is the whole requirement. A red gate is information and
    /// the loop gets another go at it (`L-16`); a 401 is the same answer every
    /// time, and the obvious thing a model reaches for after a failed retry is
    /// a credential. `Done::Blocked` is what stops that path existing.
    #[test]
    fn a_gate_that_hits_an_authentication_wall_is_blocked_rather_than_retried() {
        let root = tmpdir("engine-credential-wall");
        std::fs::create_dir_all(root.join(".harness")).expect("dirs");
        std::fs::write(root.join(".harness/perpetum.md"), "# requirements\n").expect("reqs");
        // A gate that fails the way a push against an expired token does.
        let wall = if cfg!(windows) {
            "cmd /C \"echo fatal: Authentication failed for https://example.test/ & exit 1\""
        } else {
            "sh -c \"echo 'fatal: Authentication failed for https://example.test/'; exit 1\""
        };
        std::fs::write(
            root.join(".harness/binding.md"),
            format!(
                "```perp-binding\n\
                 path.requirements = .harness/perpetum.md\n\
                 out.journal = .harness/journal.jsonl\n\
                 out.state = .harness/state.md\n\
                 gate.check = {wall}\n\
                 ```\n"
            ),
        )
        .expect("binding");

        let binding = crate::Binding::load(&root).expect("binding");
        let mut gates = Gates::from_binding(&binding, &root.join("target")).expect("gates");
        let task = Work::next(&mut gates).expect("one gate");
        let done = gates.perform(&task);

        match done {
            Done::Blocked { why } => {
                assert!(why.contains("credential-gated"), "{why}");
                assert!(why.contains("parked for a person"), "{why}");
                assert!(
                    why.contains("Authentication failed"),
                    "it carries the verbatim wall: {why}"
                );
            }
            other => panic!("a wall must not be an ordinary red gate: {other:?}"),
        }
    }

    /// And an ordinary red gate is still an ordinary red gate — the check must
    /// not turn every failure into a block.
    #[test]
    fn an_ordinary_red_gate_is_still_failed_and_not_blocked() {
        let root = tmpdir("engine-ordinary-red");
        std::fs::create_dir_all(root.join(".harness")).expect("dirs");
        std::fs::write(root.join(".harness/perpetum.md"), "# requirements\n").expect("reqs");
        let red = if cfg!(windows) {
            "cmd /C \"echo error[E0308]: mismatched types & exit 1\""
        } else {
            "sh -c \"echo 'error[E0308]: mismatched types'; exit 1\""
        };
        std::fs::write(
            root.join(".harness/binding.md"),
            format!(
                "```perp-binding\n\
                 path.requirements = .harness/perpetum.md\n\
                 out.journal = .harness/journal.jsonl\n\
                 out.state = .harness/state.md\n\
                 gate.check = {red}\n\
                 ```\n"
            ),
        )
        .expect("binding");

        let binding = crate::Binding::load(&root).expect("binding");
        let mut gates = Gates::from_binding(&binding, &root.join("target")).expect("gates");
        let task = Work::next(&mut gates).expect("one gate");
        assert!(
            matches!(gates.perform(&task), Done::Failed { .. }),
            "a compile error is information, and the loop gets another go at it"
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
        crate::control::Channel::at(&root)
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
        let channel = crate::control::Channel::at(&root);
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
        crate::control::Channel::at(&root)
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
        let channel = crate::control::Channel::at(&root);
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
