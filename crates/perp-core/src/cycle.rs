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
/// Requirements this cycle has already attempted, from the journal.
///
/// The loop may not mark its own work done (`V-2`), so the requirements source
/// looks identical after a batch as before it. Without this the second batch
/// picks up the first batch's items again, and an hour-long run redoes the same
/// four things fifteen times — which is exactly what a long run is for finding.
///
/// Attempted, not *succeeded*: an item that failed is not retried inside the
/// same cycle either. `L-11`'s watchdog says repeating an unproductive thing is
/// an error rather than a retry, and the next cycle is where a second attempt
/// belongs.
pub fn attempted(records: &[crate::journal::Record], cycle: u32) -> Vec<String> {
    let mut seen = Vec::new();
    for record in records.iter().filter(|r| r.step.cycle == cycle) {
        for id in &record.requirements {
            if id != "V-2" && !seen.contains(id) {
                seen.push(id.clone());
            }
        }
    }
    seen
}

/// The backlog with everything this cycle has already touched removed.
pub fn remaining(source: &str, records: &[crate::journal::Record], cycle: u32, limit: usize) -> Vec<Item> {
    let done = attempted(records, cycle);
    backlog(source, usize::MAX)
        .into_iter()
        .filter(|item| !done.contains(&item.requirement))
        .take(limit)
        .collect()
}

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

    fn author(&self) -> Option<String> {
        self.first.author().or_else(|| self.second.author())
    }

    fn touched(&self) -> Vec<String> {
        let mut paths = self.first.touched();
        for path in self.second.touched() {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
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

            // A leg reporting `BacklogExhausted` means *that leg's* work ran
            // out, not that the cycle has none left. Conflating the two ended
            // the first hour-long run after 99 seconds and three of twenty-four
            // requirements — the driver asked for a batch, got "exhausted", and
            // stopped.
            //
            // The cycle's backlog is the requirements source minus what this
            // cycle has attempted, and that is what `L-14`'s condition means
            // here.
            if let Some(stop) = stop {
                let cycle_done = match Engine::open(&self.root) {
                    Ok(engine) => {
                        let seen = engine.session().journal().read_all().unwrap_or_default();
                        let source = engine
                            .session()
                            .binding()
                            .resolve("path.requirements")
                            .ok()
                            .and_then(|p| std::fs::read_to_string(p).ok())
                            .unwrap_or_default();
                        remaining(&source, &seen, cycle, 1).is_empty()
                    }
                    Err(_) => true,
                };
                if cycle_done || !matches!(stop, Stop::BacklogExhausted) {
                    outcome.stop = Some(stop);
                    break;
                }
            }

            // A red gate means the batch was not delivered. Starting the next
            // one would be piling work on a tree whose tests do not pass, and
            // the gate that then goes red belongs to nobody.
            if let Some(leg) = outcome.legs.last() {
                if leg.report.failed > 0 && leg.report.stop.is_none() {
                    let blocked = Stop::BatchBlocked {
                        batch: format!("b{batches_done}"),
                        why: format!("{} step(s) failed and the gate is red", leg.report.failed),
                    };
                    outcome.stop = Some(blocked);
                    break;
                }
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
        // Everything this cycle already attempted comes off the list first.
        let seen = engine.session().journal().read_all()?;
        let items = remaining(&source, &seen, cycle, self.items_per_batch);
        // Taken before the items move into the agent: the gate cites these too,
        // so each requirement in the batch can reach the transcript that cleared
        // it (`G-6`).
        let covered: Vec<String> = items.iter().map(|item| item.requirement.clone()).collect();

        // The branch opens before any work happens, so the batch's edits are on
        // it from the first patch rather than being moved onto it afterwards.
        let branch = if covered.is_empty() {
            None
        } else {
            open_branch(&self.root, engine.session().binding(), cycle, batch)
        };

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
        let gates = Gates::from_binding(engine.session().binding(), &target)?
            .covering(covered.clone());
        let mut leg = Then::new(agent, gates);
        let report = engine.run(cycle, &stage, &mut leg, 0)?;

        // Green gate, and only then. A red one leaves the tree exactly as it is
        // for someone to look at — the driver stops the cycle on it anyway.
        if branch.is_some() && report.failed == 0 {
            let step = report
                .last_step
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("c{cycle}/{stage}"));
            let subject = format!("Deliver {}", covered.join(", "));
            // The step's own files, plus the evidence the harness wrote about
            // them — a commit with the work and no journal is half a record.
            let mut paths = crate::engine::Work::touched(&leg);
            for path in evidence_paths(&self.root, engine.session().binding()) {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            match land_batch(&self.root, &step, &covered, &subject, &paths, crate::engine::Work::author(&leg)) {
                Ok(Some(sha)) => {
                    let mut report = report;
                    report.warnings.push(format!("committed {} as {sha}", covered.join(", ")));
                    return Ok(report);
                }
                Ok(None) => {}
                // A batch that ran green and could not be committed is not a
                // failed batch, and pretending otherwise would throw away work
                // that is sitting in the tree. It is said out loud instead.
                Err(e) => {
                    let mut report = report;
                    report.warnings.push(format!("gate was green but nothing was committed: {e}"));
                    return Ok(report);
                }
            }
        }
        Ok(report)
    }

    /// Every other phase: run the gates so the leg produces evidence, and let
    /// the exit predicate decide.
    fn gates_only(&self, cycle: u32, phase: Phase) -> Result<Report> {
        let mut engine = Engine::open(&self.root)?;
        let target = self.root.join("crates/target");
        let mut gates = Gates::from_binding(engine.session().binding(), &target)?;
        let report = engine.run(cycle, &phase.letter().to_string(), &mut gates, 0)?;

        // A leg that writes evidence and does not commit it leaves the tree
        // dirty, and the next run starts on someone else's mess. A three-batch
        // run landed all three batches and still ended with a modified journal,
        // because phase E ran after the last one.
        if report.failed == 0 {
            let paths = evidence_paths(&self.root, engine.session().binding());
            let step = report
                .last_step
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("c{cycle}/{}", phase.letter()));
            let subject = format!("Record {phase} evidence");
            let mut report = report;
            match land_batch(&self.root, &step, &["V-2".to_string()], &subject, &paths, None) {
                Ok(Some(sha)) => report.warnings.push(format!("committed evidence as {sha}")),
                Ok(None) => {}
                Err(e) => report.warnings.push(format!("evidence not committed: {e}")),
            }
            return Ok(report);
        }
        Ok(report)
    }
}

/// The evidence a leg writes about itself, for staging (`G-3`, `A-2`).
///
/// Named paths, because `git add .` is refused. The artifacts directory is
/// included as a directory: `git add` on it stages the files inside, and the
/// board is regenerated wholesale rather than edited.
fn evidence_paths(root: &std::path::Path, binding: &crate::Binding) -> Vec<String> {
    let mut paths = Vec::new();
    for key in ["out.journal", "out.state", "out.board"] {
        if let Ok(path) = binding.get(key) {
            let path = path.to_string();
            if root.join(&path).exists() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    let artifacts = "docs/perpetum/artifacts".to_string();
    if root.join(&artifacts).exists() {
        paths.push(artifacts);
    }
    paths
}

/// The branch a batch works on, from `git.branch.batch` (`G-1`).
///
/// Read from the binding rather than hardcoded, because the key has been in
/// every binding since the first one and was read by nothing — the loop did all
/// its work in whatever tree it started in and committed none of it. A pattern
/// that names no placeholders would put every batch on one branch, so a name
/// that does not change with the batch is refused rather than silently reused.
fn branch_for(binding: &crate::Binding, cycle: u32, batch: u32) -> String {
    let pattern = binding.get("git.branch.batch").unwrap_or("perp/c{cycle}/b{batch}");
    let named = pattern.replace("{cycle}", &cycle.to_string()).replace("{batch}", &batch.to_string());
    if named == pattern && pattern != crate::git::batch_branch(cycle, batch) {
        return crate::git::batch_branch(cycle, batch);
    }
    named
}

/// Put the batch's work on its own branch before any of it happens (`G-1`).
///
/// Returns the branch, or `None` when there is no repository to speak of — a
/// scratch directory is a legitimate place to run and not an error.
///
/// There is deliberately no protected-branch check here. `commit` refuses a
/// protected branch outright, which is the guard that matters and the one that
/// cannot be walked past; a second check on the way in was unreachable — every
/// pattern that could name `main` is already sent to the default by
/// [`branch_for`] — and a red run proved it by deleting it without turning
/// anything red.
fn open_branch(root: &std::path::Path, binding: &crate::Binding, cycle: u32, batch: u32) -> Option<String> {
    let repo = crate::git::Repo::at(root);
    repo.head_sha().ok()?;
    let branch = branch_for(binding, cycle, batch);
    match repo.current_branch() {
        Ok(Some(on)) if on == branch => Some(branch),
        _ => repo.create_branch(&branch).ok().map(|()| branch),
    }
}

/// Commit the batch's work, once its gate is green (`G-1`, `G-2`).
///
/// Green first, always. A commit made before the gate is a commit that says
/// work landed when what landed is unknown, and the whole point of `V-2` is
/// that only a gate may call something green.
fn land_batch(
    root: &std::path::Path,
    step: &str,
    requirements: &[String],
    subject: &str,
    paths: &[String],
    author: Option<String>,
) -> Result<Option<String>> {
    let repo = crate::git::Repo::at(root);
    if repo.is_clean().unwrap_or(true) {
        return Ok(None);
    }
    // `G-3`: named, never swept. A batch that touched nothing it can name has
    // nothing to commit, whatever else is lying around in the tree.
    let named: Vec<&str> = paths.iter().map(String::as_str).filter(|p| !p.is_empty()).collect();
    if named.is_empty() {
        return Ok(None);
    }
    repo.stage(&named)?;
    if repo.plumbing(&["diff", "--cached", "--name-only"]).unwrap_or_default().trim().is_empty() {
        return Ok(None);
    }
    let message = crate::git::CommitMessage::new(subject)
        .for_requirements(requirements.to_vec())
        .at_step(step);
    let message = match author {
        Some(who) => message.authored_by(who),
        None => message,
    };
    repo.commit(&message).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::expect_used)]
    fn bound(text: &str) -> (std::path::PathBuf, crate::Binding) {
        let dir = crate::testutil::tmpdir("cycle-branch");
        std::fs::create_dir_all(dir.join("docs/perpetum")).expect("dirs");
        std::fs::write(dir.join("docs/perpetum.md"), "# requirements
").expect("reqs");
        std::fs::write(
            dir.join("docs/perpetum/binding.md"),
            format!("```perp-binding
path.requirements = docs/perpetum.md
{text}```
"),
        )
        .expect("binding");
        let binding = crate::Binding::load(&dir).expect("load");
        (dir, binding)
    }

    #[test]
    fn the_batch_branch_comes_from_the_binding_key_that_declares_it() {
        // `git.branch.batch` was in every binding written and read by nothing.
        // The loop worked in whatever tree it started in and committed none of
        // it, through four unattended runs that all reported success.
        let (_dir, binding) = bound("git.branch.batch = work/c{cycle}-b{batch}
");
        assert_eq!(branch_for(&binding, 2, 7), "work/c2-b7");

        let (_dir, default) = bound("");
        assert_eq!(branch_for(&default, 1, 3), crate::git::batch_branch(1, 3));
    }

    #[test]
    fn a_pattern_naming_no_batch_is_refused_rather_than_reused() {
        // Every batch on one branch is worse than the hardcoded name: the
        // second batch's commit lands on the first batch's branch and the two
        // can no longer be told apart.
        let (_dir, binding) = bound("git.branch.batch = perp/fixed
");
        assert_eq!(branch_for(&binding, 1, 1), crate::git::batch_branch(1, 1));
        assert_eq!(branch_for(&binding, 1, 2), crate::git::batch_branch(1, 2));
        assert_ne!(branch_for(&binding, 1, 1), branch_for(&binding, 1, 2));
    }

    #[test]
    fn the_evidence_a_leg_writes_is_named_so_it_can_be_staged() {
        // A three-batch run landed all three batches and still ended with a
        // modified journal and an unstaged board, because phase E ran after the
        // last batch and nothing committed what it wrote.
        let (dir, binding) = bound("out.journal = docs/perpetum/journal.jsonl
out.state = docs/perpetum/state.md
");

        // Nothing written yet: nothing to name.
        assert!(evidence_paths(&dir, &binding).is_empty());

        std::fs::write(dir.join("docs/perpetum/journal.jsonl"), "{}
").expect("journal");
        std::fs::create_dir_all(dir.join("docs/perpetum/artifacts")).expect("artifacts");
        std::fs::write(dir.join("docs/perpetum/artifacts/board.html"), "<p>").expect("board");

        let paths = evidence_paths(&dir, &binding);
        assert!(paths.contains(&"docs/perpetum/journal.jsonl".to_string()), "{paths:?}");
        assert!(paths.contains(&"docs/perpetum/artifacts".to_string()), "{paths:?}");
        assert!(
            !paths.contains(&"docs/perpetum/state.md".to_string()),
            "a path the binding names but nothing has written is not staged: {paths:?}"
        );
    }

    #[test]
    fn a_directory_that_is_not_a_repository_is_not_an_error() {
        let (dir, binding) = bound("");
        assert_eq!(open_branch(&dir, &binding, 1, 1), None, "a scratch tree is a fine place to run");
    }

    #[test]
    fn a_batch_lands_on_its_own_branch_and_never_on_a_protected_one() {
        let (dir, binding) = bound("");
        let repo = crate::git::Repo::at(&dir);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "loop@perpetum.test"],
            vec!["config", "user.name", "Perpetum test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            repo.run_unchecked(&args).expect("git");
        }
        repo.stage(&["docs"]).expect("stage");
        // Seeded through plumbing: `commit` refuses `main` outright (`G-1`),
        // which is the rule under test further down. This stands for a
        // repository that already existed before the loop ever saw it.
        repo.run_unchecked(&["commit", "-q", "-m", "The starting point"]).expect("seed");
        assert_eq!(repo.current_branch().expect("branch").as_deref(), Some("main"));

        let opened = open_branch(&dir, &binding, 1, 1).expect("a branch");
        assert_eq!(opened, "perp/c1/b1");
        assert_eq!(repo.current_branch().expect("branch").as_deref(), Some("perp/c1/b1"));

        // Nothing to commit yet: a clean tree is not an empty commit.
        assert_eq!(land_batch(&dir, "c1/b1/s01", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None).expect("land"), None);

        std::fs::write(dir.join("src.txt"), "the work
").expect("write");
        let sha = land_batch(&dir, "c1/b1/s01", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None)
            .expect("land")
            .expect("a commit");

        assert!(!sha.is_empty());
        assert!(repo.is_clean().expect("clean"), "the tree is clean once the batch has landed");

        let log = repo.plumbing(&["log", "-1", "--format=%B"]).expect("log");
        assert!(log.contains("Deliver L-3"), "{log}");
        assert!(log.contains("L-3"), "the requirement is a trailer, not prose: {log}");
        assert!(log.contains("c1/b1/s01"), "and so is the step: {log}");
        assert!(
            !log.contains("Co-Authored-By"),
            "no author, no trailer — better than one git cannot parse: {log}"
        );

        // Given an identity, it is a well-formed one.
        std::fs::write(dir.join("src.txt"), "more work
").expect("write");
        land_batch(
            &dir,
            "c1/b1/s02",
            &["L-3".into()],
            "Deliver L-3 again",
            &["src.txt".into()],
            Some("deepseek-v4-flash via perp <ds-fast@perp.invalid>".into()),
        )
        .expect("land")
        .expect("a commit");
        let log = repo.plumbing(&["log", "-1", "--format=%B"]).expect("log");
        let trailer = log
            .lines()
            .find(|line| line.starts_with("Co-Authored-By:"))
            .expect("a co-author trailer");
        assert!(
            trailer.contains('<') && trailer.contains('>') && trailer.contains('@'),
            "a git identity, not a bare word: {trailer}"
        );

        // A file the step never named stays out, even though it is dirty.
        std::fs::write(dir.join("stray.txt"), "not this step's
").expect("write");
        assert_eq!(
            land_batch(&dir, "c1/b1/s02", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None)
                .expect("land"),
            None,
            "`G-3` stages what the step touched, not what happens to be lying around"
        );

        // The guard that matters: committing on a protected branch is refused,
        // whatever else has gone wrong upstream of it (`G-1`).
        repo.run_unchecked(&["switch", "-q", "main"]).expect("switch");
        std::fs::write(dir.join("src.txt"), "on main
").expect("write");
        let refused = land_batch(&dir, "c1/b1/s03", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None);
        assert!(refused.is_err(), "a commit onto main is refused, not made: {refused:?}");

        // And a step that named nothing commits nothing, however dirty the tree.
        repo.run_unchecked(&["switch", "-q", "perp/c1/b1"]).expect("switch");
        assert_eq!(
            land_batch(&dir, "c1/b1/s04", &["L-3".into()], "Deliver L-3", &[], None).expect("land"),
            None,
            "no named paths, no commit"
        );
    }

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
    fn a_cycle_does_not_pick_up_what_it_already_attempted() {
        // The loop may not mark its own work done, so the requirements source
        // looks identical after a batch as before it. Without this the second
        // batch redoes the first batch's items, and an hour-long run repeats
        // the same four things fifteen times.
        use crate::journal::Record;
        use crate::step::StepId;

        let step = |n: u32| StepId::new(1, "b1", n).expect("step");
        let records = vec![
            Record::outcome(step(1), 100, true, "did it").for_requirements(["L-2"]),
            // A gate cites `V-2` on every batch; that must not take `V-2` off a
            // backlog it was never on.
            Record::outcome(step(2), 200, true, "gate").for_requirements(["V-2"]),
        ];

        let left = remaining(SOURCE, &records, 1, 10);
        let ids: Vec<&str> = left.iter().map(|i| i.requirement.as_str()).collect();
        assert_eq!(ids, ["L-3"], "L-2 was attempted; L-3 was not");

        // A different cycle starts clean — a second attempt belongs there.
        let next = remaining(SOURCE, &records, 2, 10);
        assert_eq!(next.len(), 2, "cycle 2 may try L-2 again");
    }

    #[test]
    fn a_failed_item_is_not_retried_inside_the_same_cycle() {
        // `L-11`: repeating an unproductive thing is an error, not a retry.
        use crate::journal::Record;
        use crate::step::StepId;
        let records = vec![Record::outcome(
            StepId::new(1, "b1", 1).expect("step"),
            100,
            false,
            "L-2 made no progress",
        )
        .for_requirements(["L-2"])];
        let ids: Vec<String> =
            remaining(SOURCE, &records, 1, 10).into_iter().map(|i| i.requirement).collect();
        assert!(!ids.contains(&"L-2".to_string()), "{ids:?}");
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
