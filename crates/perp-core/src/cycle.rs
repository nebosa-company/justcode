//! Driving a whole cycle, unattended (`L-1`, `L-14`).
//!
//! Everything before this ran *one* thing: a batch of gates, or one requirement
//! against a model. This is the piece that walks the phase machine — picks work
//! off the backlog, does it, gates it, measures whether the phase is over, and
//! moves on — until one of `L-14`'s three conditions stops it or a budget parks
//! it.
//!
//! ## The backlog is the requirements source
//!
//! Not a separate list. A requirement is work if it is in `docs/perpetum.md`
//! without a done marker, and it stops being work when the marker changes.
//! There is deliberately no second place to look: `C-2` already says a
//! conversation does not get its own backlog, and the same reasoning applies to
//! the loop.
//!
//! ## What it will not do
//!
//! Mark its own work done. The loop can write code, run gates and journal
//! results; **it cannot set a `✅`**. That marker means "implemented, gates
//! green, transcript in the journal" and a loop that awards it to itself is a
//! loop whose status is worth nothing. A person reads the evidence and marks.

use crate::agent::{Agent, Item};
use crate::budget::Spend;
use crate::engine::{Engine, Gates, Report};
use crate::error::Result;
use crate::phase::{Advance, Machine, Measured, Phase, Stop};

/// Requirements still open, read from the requirements source.
///
/// A row is work when it has an id and no status marker. `🟡` is included —
/// in progress means unfinished — and `✅`, `⛔`, `🔶` and `❌` are not: done,
/// externally gated, conflicting and declined are all "not the loop's to pick
/// up".
pub fn backlog(source: &str, limit: usize) -> Vec<Item> {
    let mut items = Vec::new();
    // Checked before the loop, not inside it: the earlier version pushed an
    // item and *then* noticed the limit, so a limit of zero returned one.
    if limit == 0 {
        return items;
    }
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        if line.contains('✅') || line.contains('⛔') || line.contains('🔶') || line.contains('❌') {
            continue;
        }
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };

        let id = first.trim().trim_start_matches('🟡').trim().trim_matches('`').trim();
        if !is_requirement_id(id) {
            continue;
        }
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let summary: String = text.chars().take(70).collect();
        if let Ok(item) = Item::new(id, &summary, text) {
            items.push(item);
        }
        if items.len() >= limit {
            break;
        }
    }
    items
}

fn is_requirement_id(text: &str) -> bool {
    let Some((prefix, number)) = text.split_once('-') else { return false };
    prefix.len() == 1
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Two `Work`s run as one (`L-14`).
///
/// A leg is *work then gates*, and it must produce **one** terminal record. The
/// first unattended cycle ran them as two `Engine::run` calls and the journal
/// came back reading `work · stopped · gate · stopped` — two exhausted-backlog
/// records for one leg, with a gate in between them. `L-14` says each stop
/// writes a distinct terminal record; it does not say a leg may write two.
pub struct Then<A: crate::engine::Work, B: crate::engine::Work> {
    first: A,
    second: B,
    on_second: bool,
}

impl<A: crate::engine::Work, B: crate::engine::Work> Then<A, B> {
    pub fn new(first: A, second: B) -> Then<A, B> {
        Then { first, second, on_second: false }
    }

    pub fn second(&self) -> &B {
        &self.second
    }
}

impl<A: crate::engine::Work, B: crate::engine::Work> crate::engine::Work for Then<A, B> {
    fn next(&mut self) -> Option<crate::engine::Task> {
        if !self.on_second {
            if let Some(task) = self.first.next() {
                return Some(task);
            }
            // The first is spent. Not a stop — the second half of the leg has
            // not run yet.
            self.on_second = true;
        }
        self.second.next()
    }

    fn perform(&mut self, task: &crate::engine::Task) -> crate::engine::Done {
        if self.on_second {
            self.second.perform(task)
        } else {
            self.first.perform(task)
        }
    }

    fn spend(&self) -> Spend {
        self.first.spend().plus(self.second.spend())
    }

    fn drain_records(&mut self) -> Vec<crate::journal::Record> {
        let mut records = self.first.drain_records();
        records.extend(self.second.drain_records());
        records
    }

    fn at_step(&mut self, step: &crate::step::StepId) {
        self.first.at_step(step);
        self.second.at_step(step);
    }
}

/// What one phase of the cycle did.
#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    pub phase: Phase,
    pub report: Report,
    /// Why the machine did or did not move on.
    pub advanced: String,
}

/// What a whole cycle did.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub cycle: u32,
    pub legs: Vec<Leg>,
    pub stop: Option<Stop>,
    pub parked: Option<String>,
    pub spend: Spend,
    pub ended_in: Phase,
}

impl Outcome {
    pub fn describe(&self) -> String {
        let mut out = format!("cycle {} — {} legs\n", self.cycle, self.legs.len());
        for leg in &self.legs {
            out.push_str(&format!("  {} · {}\n", leg.phase, leg.report.describe()));
            out.push_str(&format!("       {}\n", leg.advanced));
        }
        out.push_str(&format!("ended in phase {}\n", self.ended_in));
        match (&self.stop, &self.parked) {
            (Some(stop), _) => out.push_str(&format!("{}\n", stop.summary())),
            (None, Some(why)) => out.push_str(&format!("parked: {why}\n")),
            (None, None) => out.push_str("still running\n"),
        }
        out.push_str(&format!("spent {}\n", self.spend));
        out
    }
}

/// Measure the workspace for the phase machine (`L-2`).
///
/// Every field comes from a file or the journal. Nothing here asks a model, and
/// nothing a model produced reaches it — which is the point of the type.
pub fn measure(engine: &Engine, batches_done: u32) -> Result<Measured> {
    let binding = engine.session().binding();
    let records = engine.session().journal().read_all()?;
    let projection = crate::state::replay(&records);

    let source = binding
        .resolve("path.requirements")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let open = u32::try_from(backlog(&source, usize::MAX).len()).unwrap_or(u32::MAX);

    let gates_green_at_sha = records.iter().rev().take(20).any(|record| {
        record.ok == Some(true)
            && record.detail.as_deref().is_some_and(|d| d.starts_with("gate:") && d.contains("sha:"))
    });

    Ok(Measured::from_workspace(
        true,
        binding.resolve("path.vision").map(|p| p.exists()).unwrap_or(false),
        open,
        binding.resolve("path.batches").map(|p| p.exists()).unwrap_or(false).into(),
        0,
        batches_done,
        binding.root().join("docs/perpetum/release-notes.md").exists(),
        gates_green_at_sha,
        u32::from(projection.open_step.is_some()),
        true,
    ))
}

/// Run a cycle until something stops it.
///
/// `batches` bounds Phase D the way Perpetum does — five per cycle by default —
/// and `items_per_batch` bounds how much work one batch attempts. Both exist
/// because an unattended loop with no bound on its own appetite is a loop that
/// spends a budget discovering it had one.
pub struct Driver<'a> {
    pub root: std::path::PathBuf,
    pub links: &'a crate::link::Links,
    pub health: &'a dyn crate::link::Health,
    pub mode: crate::link::Mode,
    pub batches: u32,
    pub items_per_batch: usize,
    pub transport: &'a dyn crate::net::Transport,
}

impl Driver<'_> {
    /// Walk the machine. Returns when `L-14` stops it, a budget parks it, or
    /// Phase D's batch allowance is used up.
    pub fn run(&self, cycle: u32, from: Phase) -> Result<Outcome> {
        let mut machine = Machine::at(cycle, from);
        let mut outcome = Outcome {
            cycle,
            legs: Vec::new(),
            stop: None,
            parked: None,
            spend: Spend::default(),
            ended_in: from,
        };
        let mut batches_done = 0;

        loop {
            outcome.ended_in = machine.phase();
            let phase = machine.phase();

            // Only D has work a machine can do. B, C, E and F are a person's —
            // gathering requirements, prioritising, writing release notes,
            // reconciling — and pretending otherwise would have the loop
            // asserting it had done them.
            let report = if phase == Phase::D {
                batches_done += 1;
                self.batch(cycle, batches_done)?
            } else {
                self.gates_only(cycle, phase)?
            };

            outcome.spend = outcome.spend.plus(report.spend);
            let stop = report.stop.clone();
            let park = report.park.clone();

            let measured = {
                let engine = Engine::open(&self.root)?;
                measure(&engine, batches_done)?
            };
            let advanced = match machine.advance(&measured, self.batches) {
                Ok(Advance::Stay { because, .. }) => format!("stays in {phase}: {because}"),
                Ok(Advance::Moved { from, to }) => format!("{from} → {to}"),
                Ok(Advance::Cycled { cycle }) => format!("F closed, cycle {cycle} opens at B"),
                Err(e) => format!("cannot advance: {e}"),
            };
            let moved = advanced.contains('→') || advanced.contains("opens at B");

            outcome.legs.push(Leg { phase, report, advanced });

            if let Some(stop) = stop {
                outcome.stop = Some(stop);
                break;
            }
            if let Some(park) = park {
                outcome.parked = Some(park.reason);
                break;
            }
            if phase == Phase::D && batches_done >= self.batches && !moved {
                outcome.parked = Some(format!(
                    "{} batches attempted and Phase D's exit is still not met",
                    self.batches
                ));
                break;
            }
            // A phase that cannot move and has no work left would spin.
            if !moved && phase != Phase::D {
                outcome.parked =
                    Some(format!("{phase} cannot advance and has no work a loop can do"));
                break;
            }
            if machine.phase() == Phase::B && phase == Phase::F {
                break;
            }
        }
        Ok(outcome)
    }

    /// Phase D: pick work off the backlog, do it, then gate it.
    fn batch(&self, cycle: u32, batch: u32) -> Result<Report> {
        let stage = format!("b{batch}");
        let mut engine = Engine::open(&self.root)?;

        let source = engine
            .session()
            .binding()
            .resolve("path.requirements")
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        let items = backlog(&source, self.items_per_batch);

        // An empty backlog is `L-14`'s exhausted condition, and the engine
        // reaches it by being handed no work rather than by being told.
        let mut agent = Agent::new(
            crate::client::Client::new(self.transport),
            self.links,
            self.health,
            crate::agent::host_for(&self.root),
            items,
        );
        if self.mode == crate::link::Mode::LocalOnly {
            agent = agent.local_only();
        }
        // Work then gates, as one run, so the leg produces one terminal record
        // and the evidence sits beside the work it is evidence for.
        let target = self.root.join("crates/target");
        let gates = Gates::from_binding(engine.session().binding(), &target)?;
        let mut leg = Then::new(agent, gates);
        engine.run(cycle, &stage, &mut leg, 0)
    }

    /// Every other phase: run the gates so the leg produces evidence, and let
    /// the exit predicate decide.
    fn gates_only(&self, cycle: u32, phase: Phase) -> Result<Report> {
        let mut engine = Engine::open(&self.root)?;
        let target = self.root.join("crates/target");
        let mut gates = Gates::from_binding(engine.session().binding(), &target)?;
        engine.run(cycle, &phase.letter().to_string(), &mut gates, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "\
| id | Requirement |\n\
|---|---|\n\
| ✅ ~~`L-1`~~ | already done |\n\
| 🟡 `L-2` | in progress, so still work |\n\
| `L-3` | never started |\n\
| ⛔ `M-25` | external-gated |\n\
| ❌ `X-11` | declined |\n\
| 🔶 `I-3` | conflicting |\n\
| not a row | ignored |\n";

    #[test]
    fn the_backlog_is_what_the_requirements_source_says_is_open() {
        let items = backlog(SOURCE, 10);
        let ids: Vec<&str> = items.iter().map(|i| i.requirement.as_str()).collect();
        assert_eq!(ids, ["L-2", "L-3"], "in progress is unfinished; the rest are not the loop's");
    }

    #[test]
    fn a_done_requirement_is_never_picked_up_again() {
        // The marker is the only thing that takes work off the backlog, which
        // is why the loop may not set one.
        let items = backlog(SOURCE, 10);
        assert!(!items.iter().any(|i| i.requirement == "L-1"));
        assert!(!items.iter().any(|i| i.requirement == "M-25"), "external-gated is not blocked-on-us");
    }

    #[test]
    fn the_batch_size_bounds_what_one_batch_attempts() {
        // An unattended loop with no bound on its own appetite is one that
        // spends a budget discovering it had one.
        assert_eq!(backlog(SOURCE, 1).len(), 1);
        assert_eq!(backlog(SOURCE, 0).len(), 0);
    }

    #[test]
    fn an_empty_backlog_is_the_exhausted_condition() {
        // `L-14`. The engine reaches it by being handed no work, rather than by
        // being told there is none.
        assert!(backlog("| ✅ ~~`L-1`~~ | done |\n", 10).is_empty());
        assert!(backlog("", 10).is_empty());
    }

    #[test]
    fn only_development_has_work_a_machine_can_do() {
        // B, C, E and F are a person's — gathering, prioritising, writing
        // release notes, reconciling. A loop that ran them would be asserting
        // it had done them, which is exactly what `L-2` forbids.
        assert_eq!(Phase::D.exit(5), crate::phase::Exit::BatchesDelivered(5));
        for phase in [Phase::B, Phase::C, Phase::E, Phase::F] {
            assert_ne!(phase.exit(5), crate::phase::Exit::BatchesDelivered(5));
        }
    }

    #[test]
    fn a_leg_is_work_then_gates_and_stops_once() {
        // Found by the first unattended cycle: running the two as separate
        // engine calls produced `work · stopped · gate · stopped` — two
        // terminal records for one leg, with a gate between them.
        use crate::engine::{Done, Task, Work};

        struct Fixed(Vec<&'static str>, usize);
        impl Work for Fixed {
            fn next(&mut self) -> Option<Task> {
                self.0.get(self.1).map(|s| Task::new(*s))
            }
            fn perform(&mut self, _task: &Task) -> Done {
                self.1 += 1;
                Done::ok("did it")
            }
        }

        let mut leg = Then::new(Fixed(vec!["item"], 0), Fixed(vec!["gate"], 0));
        let mut seen = Vec::new();
        while let Some(task) = leg.next() {
            seen.push(task.summary.clone());
            leg.perform(&task);
        }
        assert_eq!(seen, ["item", "gate"], "the first is spent, then the second runs");
        assert!(leg.next().is_none(), "and only then is the leg exhausted — once");
    }

    #[test]
    fn an_outcome_reads_as_a_sequence_of_legs() {
        let outcome = Outcome {
            cycle: 5,
            legs: Vec::new(),
            stop: Some(Stop::BacklogExhausted),
            parked: None,
            spend: Spend { tokens: 1200, seconds: 90, money: 0.0021 },
            ended_in: Phase::D,
        };
        let text = outcome.describe();
        assert!(text.contains("cycle 5"), "{text}");
        assert!(text.contains("backlog is exhausted"), "{text}");
        assert!(text.contains("$0.002100"), "the real number: {text}");
    }
}
