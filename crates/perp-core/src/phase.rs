//! The phase machine (`L-1`), its exit predicates (`L-2`), and the three ways a
//! loop is allowed to end (`L-14`).
//!
//! Perpetum has seven phases. A runs once, B→F is the loop, and G is terminal.
//! The machine here is deliberately small and deliberately boring: the interest
//! is not in the transitions but in **who is allowed to say a phase is
//! finished**.
//!
//! `L-2` answers that: an exit condition is a checkable predicate over a
//! [`Measured`], and a `Measured` records where its numbers came from. The
//! engine reads the workspace and gets [`Source::Workspace`]; anything a model
//! says arrives as [`Source::ModelClaim`] and cannot advance the machine at all.
//! This is structural rather than cryptographic — a caller inside this crate
//! could construct either — and the claim being made is only that there is
//! exactly one function that reads the workspace, and that everything a model
//! produces has to pass through the other door.
//!
//! The alternative, which every agent harness reaches for eventually, is asking
//! the model whether it is done. It always says yes.

use std::fmt;

use crate::error::{Error, Result};
use crate::journal::Record;
use crate::step::StepId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
}

impl Phase {
    pub fn letter(self) -> char {
        match self {
            Phase::A => 'A',
            Phase::B => 'B',
            Phase::C => 'C',
            Phase::D => 'D',
            Phase::E => 'E',
            Phase::F => 'F',
            Phase::G => 'G',
        }
    }

    pub fn parse(letter: char) -> Option<Phase> {
        match letter.to_ascii_uppercase() {
            'A' => Some(Phase::A),
            'B' => Some(Phase::B),
            'C' => Some(Phase::C),
            'D' => Some(Phase::D),
            'E' => Some(Phase::E),
            'F' => Some(Phase::F),
            'G' => Some(Phase::G),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Phase::A => "initiation",
            Phase::B => "requirements gathering",
            Phase::C => "prioritisation",
            Phase::D => "development",
            Phase::E => "release",
            Phase::F => "clean-up",
            Phase::G => "sunset",
        }
    }

    /// What has to be true before this phase is over (`L-2`).
    pub fn exit(self, batches_per_cycle: u32) -> Exit {
        match self {
            Phase::A => Exit::Initiated,
            Phase::B => Exit::RequirementsMarked,
            Phase::C => Exit::BatchesPlanned,
            Phase::D => Exit::BatchesDelivered(batches_per_cycle),
            Phase::E => Exit::ReleaseRecorded,
            Phase::F => Exit::Reconciled,
            Phase::G => Exit::NeverByTheEngine,
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.letter(), self.name())
    }
}

/// Where a measurement came from. The whole value of [`Measured`] is in this
/// field, so it is not constructible by setting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Read off the workspace by the engine.
    Workspace,
    /// Asserted by a model, a tool result, or a file the loop was told to read.
    /// Never advances a phase.
    ModelClaim,
}

/// The facts an exit predicate is allowed to consult.
///
/// Every field is a count or a boolean that something in the workspace can be
/// checked against. There is no `done: bool`, deliberately: the moment a
/// predicate can read a summary judgement, the summary judgement is what gets
/// optimised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured {
    pub binding_loads: bool,
    pub vision_exists: bool,
    /// Requirements with no status marker at all.
    pub requirements_unmarked: u32,
    pub batches_planned: u32,
    /// Conflicts still waiting on a person.
    pub conflicts_open: u32,
    pub batches_delivered: u32,
    pub release_notes: bool,
    /// Every gate green *and* pinned to a commit sha (`G-6`).
    pub gates_green_at_sha: bool,
    /// Steps with an intent and no outcome.
    pub open_steps: u32,
    pub state_matches_projection: bool,
    source: Source,
}

impl Measured {
    /// The only constructor that produces a value a phase will accept. Called
    /// by the engine after it has read the workspace, and by nothing else.
    #[allow(clippy::too_many_arguments)]
    pub fn from_workspace(
        binding_loads: bool,
        vision_exists: bool,
        requirements_unmarked: u32,
        batches_planned: u32,
        conflicts_open: u32,
        batches_delivered: u32,
        release_notes: bool,
        gates_green_at_sha: bool,
        open_steps: u32,
        state_matches_projection: bool,
    ) -> Measured {
        Measured {
            binding_loads,
            vision_exists,
            requirements_unmarked,
            batches_planned,
            conflicts_open,
            batches_delivered,
            release_notes,
            gates_green_at_sha,
            open_steps,
            state_matches_projection,
            source: Source::Workspace,
        }
    }

    /// The same numbers, from a model. Kept so the claim can be *recorded* and
    /// compared against the measurement — not so it can be acted on.
    pub fn claimed(mut self) -> Measured {
        self.source = Source::ModelClaim;
        self
    }

    pub fn source(&self) -> Source {
        self.source
    }

    /// Where a model's account of the workspace differs from the workspace.
    /// Both must be about the same moment for this to mean anything; the engine
    /// measures immediately after reading the claim.
    pub fn disagreements(&self, claim: &Measured) -> Vec<String> {
        let mut out = Vec::new();
        let mut note = |field: &str, mine: String, theirs: String| {
            if mine != theirs {
                out.push(format!("{field}: measured {mine}, claimed {theirs}"));
            }
        };
        note("open_steps", self.open_steps.to_string(), claim.open_steps.to_string());
        note(
            "batches_delivered",
            self.batches_delivered.to_string(),
            claim.batches_delivered.to_string(),
        );
        note(
            "gates_green_at_sha",
            self.gates_green_at_sha.to_string(),
            claim.gates_green_at_sha.to_string(),
        );
        note(
            "requirements_unmarked",
            self.requirements_unmarked.to_string(),
            claim.requirements_unmarked.to_string(),
        );
        note("conflicts_open", self.conflicts_open.to_string(), claim.conflicts_open.to_string());
        out
    }
}

/// A phase's exit condition, as data rather than prose or a closure — so it can
/// be printed in a status line, cited in the journal, and argued with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    Initiated,
    RequirementsMarked,
    BatchesPlanned,
    BatchesDelivered(u32),
    ReleaseRecorded,
    Reconciled,
    NeverByTheEngine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Met,
    NotMet { because: String },
}

impl Outcome {
    pub fn is_met(&self) -> bool {
        matches!(self, Outcome::Met)
    }
}

impl Exit {
    /// What this condition says in one line, for a status surface.
    pub fn describe(&self) -> String {
        match self {
            Exit::Initiated => "the binding loads and the vision is on disk".into(),
            Exit::RequirementsMarked => "every requirement carries a status".into(),
            Exit::BatchesPlanned => "at least one batch is planned and no conflict is open".into(),
            Exit::BatchesDelivered(n) => format!("{n} batches delivered with gates green at a sha"),
            Exit::ReleaseRecorded => "release notes written and gates green at a sha".into(),
            Exit::Reconciled => "nothing open in the journal and state matches it".into(),
            Exit::NeverByTheEngine => "a person ends the product; the engine never does".into(),
        }
    }

    pub fn check(&self, m: &Measured) -> Outcome {
        let no = |because: String| Outcome::NotMet { because };
        match self {
            Exit::Initiated => {
                if !m.binding_loads {
                    no("the binding does not load".into())
                } else if !m.vision_exists {
                    no("there is no vision document to check conflicts against".into())
                } else {
                    Outcome::Met
                }
            }
            Exit::RequirementsMarked => {
                if m.requirements_unmarked > 0 {
                    no(format!("{} requirements carry no status", m.requirements_unmarked))
                } else {
                    Outcome::Met
                }
            }
            Exit::BatchesPlanned => {
                if m.batches_planned == 0 {
                    no("no batch is planned".into())
                } else if m.conflicts_open > 0 {
                    no(format!("{} conflicts are still waiting on a person", m.conflicts_open))
                } else {
                    Outcome::Met
                }
            }
            Exit::BatchesDelivered(n) => {
                if m.batches_delivered < *n {
                    no(format!("{} of {n} batches delivered", m.batches_delivered))
                } else if !m.gates_green_at_sha {
                    no("the gates are not green at a commit sha".into())
                } else {
                    Outcome::Met
                }
            }
            Exit::ReleaseRecorded => {
                if !m.release_notes {
                    no("no release notes for this cycle".into())
                } else if !m.gates_green_at_sha {
                    no("the gates are not green at a commit sha".into())
                } else {
                    Outcome::Met
                }
            }
            Exit::Reconciled => {
                if m.open_steps > 0 {
                    no(format!("{} steps are still open in the journal", m.open_steps))
                } else if !m.state_matches_projection {
                    no("the state file disagrees with the journal".into())
                } else {
                    Outcome::Met
                }
            }
            // Not "not met yet". Not met, by construction, forever.
            Exit::NeverByTheEngine => no("sunset is a person's decision (Perpetum 0.4)".into()),
        }
    }
}

/// Why a loop ended. Exactly Perpetum F's three, and no others — a fourth would
/// be a way to stop without saying which of these happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// Nothing left in the backlog worth a batch.
    BacklogExhausted,
    /// A batch cannot proceed and the reason is outside the loop.
    BatchBlocked { batch: String, why: String },
    /// A person said stop.
    HumanStop { who: String },
}

impl Stop {
    pub fn tag(&self) -> &'static str {
        match self {
            Stop::BacklogExhausted => "backlog-exhausted",
            Stop::BatchBlocked { .. } => "batch-blocked",
            Stop::HumanStop { .. } => "human-stop",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Stop::BacklogExhausted => "stopped: the backlog is exhausted".into(),
            Stop::BatchBlocked { batch, why } => format!("stopped: batch {batch} is blocked — {why}"),
            Stop::HumanStop { who } => format!("stopped: {who} said stop"),
        }
    }

    /// A distinct terminal record per condition (`L-14`). `ok` is true for an
    /// exhausted backlog and false for the other two, because finishing the
    /// work and being unable to continue are not the same outcome and a reader
    /// scanning for failures should see the difference without reading prose.
    pub fn record(&self, step: StepId, at: i64) -> Record {
        let ok = matches!(self, Stop::BacklogExhausted);
        Record::outcome(step, at, ok, self.summary()).with_detail(format!("stop={}", self.tag()))
    }
}

/// Why a loop paused. Not a [`Stop`]: the work is intact and resuming is the
/// expected next move (`L-9`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Park {
    pub reason: String,
    pub resumable: bool,
}

impl Park {
    pub fn budget(detail: impl Into<String>) -> Park {
        Park { reason: format!("budget: {}", detail.into()), resumable: true }
    }

    pub fn awaiting_approval(what: impl Into<String>) -> Park {
        Park { reason: format!("awaiting approval: {}", what.into()), resumable: true }
    }

    pub fn record(&self, step: StepId, at: i64) -> Record {
        Record::outcome(step, at, false, format!("parked: {}", self.reason))
            .with_detail("stop=park")
    }
}

/// The phase machine itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    cycle: u32,
    phase: Phase,
    /// A runs once for the repository, not once per cycle.
    initiated: bool,
    stopped: Option<Stop>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advance {
    /// The exit condition is not met; the phase continues.
    Stay { phase: Phase, because: String },
    /// Moved on within the same cycle.
    Moved { from: Phase, to: Phase },
    /// F closed and B reopened. The cycle number is the new one.
    Cycled { cycle: u32 },
}

impl Machine {
    /// A fresh repository: phase A, cycle 1.
    pub fn start() -> Machine {
        Machine { cycle: 1, phase: Phase::A, initiated: false, stopped: None }
    }

    /// Resume mid-loop, as the engine does after reading the journal.
    pub fn at(cycle: u32, phase: Phase) -> Machine {
        Machine { cycle, phase, initiated: phase > Phase::A, stopped: None }
    }

    pub fn cycle(&self) -> u32 {
        self.cycle
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn stopped(&self) -> Option<&Stop> {
        self.stopped.as_ref()
    }

    /// The step id the next piece of work in this phase should carry, given the
    /// stage label the caller is in (`D` uses batch labels, everything else its
    /// own letter).
    pub fn stage(&self, batch: Option<u32>) -> String {
        match (self.phase, batch) {
            (Phase::D, Some(n)) => format!("b{n}"),
            _ => self.phase.letter().to_string(),
        }
    }

    pub fn stop(&mut self, stop: Stop) {
        self.stopped = Some(stop);
    }

    /// Evaluate the current phase's exit condition and move if it is met.
    ///
    /// Refuses a measurement a model produced (`L-2`) and refuses to run at all
    /// once stopped, because a stopped loop that can be advanced is a loop that
    /// stopped for decoration.
    pub fn advance(&mut self, m: &Measured, batches_per_cycle: u32) -> Result<Advance> {
        if let Some(stop) = &self.stopped {
            return Err(Error::refused("advance", format!("the loop {}", stop.summary())));
        }
        if m.source() != Source::Workspace {
            return Err(Error::refused(
                format!("exit {}", self.phase.letter()),
                "a phase exit is measured from the workspace, never asserted (`L-2`)",
            ));
        }

        let exit = self.phase.exit(batches_per_cycle);
        if let Outcome::NotMet { because } = exit.check(m) {
            return Ok(Advance::Stay { phase: self.phase, because });
        }

        let from = self.phase;
        match from {
            Phase::A => {
                self.initiated = true;
                self.phase = Phase::B;
                Ok(Advance::Moved { from, to: Phase::B })
            }
            Phase::B => {
                self.phase = Phase::C;
                Ok(Advance::Moved { from, to: Phase::C })
            }
            Phase::C => {
                self.phase = Phase::D;
                Ok(Advance::Moved { from, to: Phase::D })
            }
            Phase::D => {
                self.phase = Phase::E;
                Ok(Advance::Moved { from, to: Phase::E })
            }
            Phase::E => {
                self.phase = Phase::F;
                Ok(Advance::Moved { from, to: Phase::F })
            }
            // The loop. Not back to A: initiation happened once and re-running
            // it would rewrite the vision the conflict checks measure against.
            Phase::F => {
                self.cycle += 1;
                self.phase = Phase::B;
                Ok(Advance::Cycled { cycle: self.cycle })
            }
            Phase::G => Err(Error::refused("phase G", "sunset is terminal")),
        }
    }

    /// Enter sunset. Approval-gated in full (`L-1`, Perpetum 0.4) — which in
    /// practice means the engine cannot do it, and this exists so the refusal
    /// has a name rather than being an unhandled case.
    pub fn enter_sunset(&mut self, _approved_by: &str) -> Result<()> {
        Err(Error::refused(
            "phase G",
            "all of sunset is on the Never list; no approval unlocks it (Perpetum 0.4)",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> Measured {
        Measured::from_workspace(true, true, 0, 3, 0, 5, true, true, 0, true)
    }

    #[test]
    fn a_runs_once_and_the_loop_returns_to_b() {
        let mut m = Machine::start();
        assert_eq!(m.phase(), Phase::A);
        let facts = ready();
        assert_eq!(m.advance(&facts, 5).expect("A"), Advance::Moved { from: Phase::A, to: Phase::B });
        for (from, to) in
            [(Phase::B, Phase::C), (Phase::C, Phase::D), (Phase::D, Phase::E), (Phase::E, Phase::F)]
        {
            assert_eq!(m.advance(&facts, 5).expect("step"), Advance::Moved { from, to });
        }
        assert_eq!(m.advance(&facts, 5).expect("F"), Advance::Cycled { cycle: 2 });
        assert_eq!(m.phase(), Phase::B, "the loop reopens at B, never at A");
    }

    #[test]
    fn a_phase_exit_is_measured_not_asserted() {
        let mut m = Machine::at(2, Phase::D);
        let claim = ready().claimed();
        let err = m.advance(&claim, 5).expect_err("a model claim must not advance a phase");
        assert!(err.is_refusal(), "it is a decision, not a defect: {err}");
        assert!(format!("{err}").contains("L-2"), "cites the requirement: {err}");
        assert_eq!(m.phase(), Phase::D, "and nothing moved");
    }

    #[test]
    fn development_needs_the_batches_and_the_evidence() {
        let mut m = Machine::at(1, Phase::D);
        let mut facts = ready();
        facts.batches_delivered = 3;
        let Advance::Stay { because, .. } = m.advance(&facts, 5).expect("stay") else {
            panic!("three of five is not five");
        };
        assert!(because.contains("3 of 5"), "{because}");

        facts.batches_delivered = 5;
        facts.gates_green_at_sha = false;
        let Advance::Stay { because, .. } = m.advance(&facts, 5).expect("stay") else {
            panic!("five batches with no evidence is not a delivered cycle");
        };
        assert!(because.contains("commit sha"), "{because}");
    }

    #[test]
    fn clean_up_will_not_close_over_an_open_step() {
        let mut m = Machine::at(1, Phase::F);
        let mut facts = ready();
        facts.open_steps = 1;
        let Advance::Stay { because, .. } = m.advance(&facts, 5).expect("stay") else {
            panic!("an open intent is exactly what F exists to catch");
        };
        assert!(because.contains("still open"), "{because}");
    }

    #[test]
    fn sunset_is_never_reached_by_the_engine() {
        assert!(matches!(
            Phase::G.exit(5).check(&ready()),
            Outcome::NotMet { .. },
            ));
        let mut m = Machine::at(9, Phase::F);
        assert!(m.enter_sunset("the operator").is_err(), "no approval unlocks G");
        // And the ordinary loop never routes there.
        assert_eq!(m.advance(&ready(), 5).expect("F"), Advance::Cycled { cycle: 10 });
    }

    #[test]
    fn the_three_stops_are_distinguishable_in_the_journal() {
        let step = StepId::parse("c3/b12/s01").expect("step");
        let records: Vec<Record> = [
            Stop::BacklogExhausted,
            Stop::BatchBlocked { batch: "b12".into(), why: "needs a GPU".into() },
            Stop::HumanStop { who: "the operator".into() },
        ]
        .iter()
        .map(|s| s.record(step.clone(), 1_700_000_000))
        .collect();

        let tags: Vec<String> = records.iter().map(|r| r.detail.clone().unwrap_or_default()).collect();
        assert_eq!(tags, ["stop=backlog-exhausted", "stop=batch-blocked", "stop=human-stop"]);
        assert_eq!(records[0].ok, Some(true), "finishing the backlog is a success");
        assert_eq!(records[1].ok, Some(false), "being unable to continue is not");
        assert_eq!(records[2].ok, Some(false));
    }

    #[test]
    fn a_park_is_not_a_stop() {
        let step = StepId::parse("c3/b12/s01").expect("step");
        let park = Park::budget("cycle tokens: 120000 of 100000");
        let record = park.record(step, 1_700_000_000);
        assert!(park.resumable);
        assert_eq!(record.detail.as_deref(), Some("stop=park"));
        assert!(record.summary.contains("120000"), "carries the number that stopped it");
    }

    #[test]
    fn a_stopped_loop_stays_stopped() {
        let mut m = Machine::at(3, Phase::D);
        m.stop(Stop::HumanStop { who: "the operator".into() });
        let err = m.advance(&ready(), 5).expect_err("a stopped loop must not advance");
        assert!(format!("{err}").contains("said stop"), "{err}");
    }

    #[test]
    fn disagreement_is_reported_field_by_field() {
        let measured = Measured::from_workspace(true, true, 4, 3, 1, 3, false, false, 2, true);
        let claim = ready().claimed();
        let diff = measured.disagreements(&claim);
        assert!(diff.iter().any(|d| d.contains("open_steps: measured 2, claimed 0")), "{diff:?}");
        assert!(
            diff.iter().any(|d| d.contains("batches_delivered: measured 3, claimed 5")),
            "{diff:?}"
        );
    }

    #[test]
    fn stages_label_batches_in_development_and_letters_elsewhere() {
        assert_eq!(Machine::at(3, Phase::D).stage(Some(12)), "b12");
        assert_eq!(Machine::at(3, Phase::D).stage(None), "D");
        assert_eq!(Machine::at(3, Phase::B).stage(Some(12)), "B");
    }
}
