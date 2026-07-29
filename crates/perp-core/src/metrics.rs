//! Counted facts (`O-5`, `O-7`).
//!
//! Perpetum F.5 asks for cycle metrics. The word doing the work in `O-7` is
//! **counted**: every number here is derived from journal records, and a number
//! that cannot be derived does not appear. The failure mode this exists to
//! prevent is the plausible summary — "a productive cycle, 12 requirements
//! delivered" — written by the thing whose performance it describes.
//!
//! [`Snapshot`] is the same discipline applied live: `O-5`'s watch mode is a
//! projection rendered on a timer, not a running tally kept in memory. A tally
//! survives neither a restart nor a second process, and disagrees with the
//! journal the moment either happens.

use std::fmt;

use crate::btw;
use crate::cost::Ledger;
use crate::journal::{Kind, Record};
use crate::state::Projection;

/// What one cycle cost and produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cycle {
    pub cycle: u32,
    pub steps: usize,
    pub closed_green: usize,
    pub closed_red: usize,
    pub still_open: usize,
    pub gates_run: usize,
    pub gates_green: usize,
    pub requirements_touched: usize,
    pub model_calls: usize,
    pub tokens: i64,
    pub money: f64,
    /// Seconds between the first and last record. Elapsed, not worked —
    /// a cycle left overnight is mostly the machine sitting idle, and calling
    /// that "time spent" would flatter every number derived from it.
    pub elapsed_seconds: i64,
    pub btw_received: usize,
    pub btw_waiting: usize,
}

impl Cycle {
    /// Count a whole journal.
    pub fn count(records: &[Record]) -> Cycle {
        Cycle::count_cycle(records, None)
    }

    /// Count one cycle's records, or all of them when `only` is `None`.
    pub fn count_cycle(records: &[Record], only: Option<u32>) -> Cycle {
        let mine: Vec<&Record> =
            records.iter().filter(|r| only.is_none_or(|c| r.step.cycle == c)).collect();

        let mut counted = Cycle {
            cycle: only.or_else(|| mine.last().map(|r| r.step.cycle)).unwrap_or(0),
            ..Cycle::default()
        };

        let mut open: Vec<String> = Vec::new();
        let mut requirements: Vec<String> = Vec::new();

        for record in &mine {
            for id in &record.requirements {
                if !requirements.contains(id) {
                    requirements.push(id.clone());
                }
            }
            match record.kind {
                Kind::Intent => open.push(record.step.to_string()),
                Kind::Outcome => {
                    counted.steps += 1;
                    if let Some(index) = open.iter().position(|s| *s == record.step.to_string()) {
                        open.remove(index);
                    }
                    match record.ok {
                        Some(false) => counted.closed_red += 1,
                        _ => counted.closed_green += 1,
                    }
                    if record.detail.as_deref().is_some_and(|d| d.starts_with("gate:")) {
                        counted.gates_run += 1;
                        if record.ok == Some(true) {
                            counted.gates_green += 1;
                        }
                    }
                }
            }
        }

        counted.still_open = open.len();
        counted.requirements_touched = requirements.len();

        let owned: Vec<Record> = mine.iter().map(|r| (*r).clone()).collect();
        let ledger = Ledger::replay(&owned);
        let total = ledger.total();
        counted.model_calls = total.calls;
        counted.tokens = total.usage.total();
        counted.money = total.charge;

        let queue = btw::Queue::replay(&owned);
        counted.btw_received = queue.items().len();
        counted.btw_waiting = queue.pending().len();

        if let (Some(first), Some(last)) = (mine.first(), mine.last()) {
            counted.elapsed_seconds = (last.at - first.at).max(0);
        }
        counted
    }

    /// Name and value pairs, for a table. One place, so a surface cannot show a
    /// metric another surface does not have.
    pub fn rows(&self) -> Vec<(String, String)> {
        vec![
            ("steps closed".into(), self.steps.to_string()),
            ("green".into(), self.closed_green.to_string()),
            ("red".into(), self.closed_red.to_string()),
            ("left open".into(), self.still_open.to_string()),
            ("gates run".into(), format!("{} ({} green)", self.gates_run, self.gates_green)),
            ("requirements touched".into(), self.requirements_touched.to_string()),
            ("model calls".into(), self.model_calls.to_string()),
            ("tokens".into(), self.tokens.to_string()),
            ("money".into(), format!("${:.6}", self.money)),
            ("elapsed".into(), format_duration(self.elapsed_seconds)),
            ("/btw".into(), format!("{} received, {} waiting", self.btw_received, self.btw_waiting)),
        ]
    }

    /// One row of the state file's history table (`O-7`).
    pub fn history_row(&self) -> String {
        format!(
            "| {} | {} | {} | {} | {} | {} | ${:.4} | {} |",
            self.cycle,
            self.steps,
            self.closed_red,
            self.gates_green,
            self.requirements_touched,
            self.tokens,
            self.money,
            format_duration(self.elapsed_seconds),
        )
    }

    pub const HISTORY_HEADER: &'static str = "\
| Cycle | Steps | Red | Gates green | Requirements | Tokens | Money | Elapsed |\n\
|---|---|---|---|---|---|---|---|";
}

/// Every cycle in a journal, oldest first (`O-7`).
pub fn history(records: &[Record]) -> Vec<Cycle> {
    let mut cycles: Vec<u32> = Vec::new();
    for record in records {
        if !cycles.contains(&record.step.cycle) {
            cycles.push(record.step.cycle);
        }
    }
    cycles.sort_unstable();
    cycles.into_iter().map(|c| Cycle::count_cycle(records, Some(c))).collect()
}

pub fn format_duration(seconds: i64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    format!("{}h{:02}m", minutes / 60, minutes % 60)
}

/// The live view (`O-5`).
///
/// Rendered from a projection every time rather than accumulated, so two
/// watchers agree with each other and with the journal.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub cycle: Option<u32>,
    pub stage: Option<String>,
    pub in_flight: Option<String>,
    pub link: Option<String>,
    pub tokens: i64,
    pub money: f64,
    pub gates: String,
    pub blocked: usize,
    pub gated: usize,
    pub approvals_pending: usize,
    pub btw_queued: usize,
}

impl Snapshot {
    pub fn of(projection: &Projection, records: &[Record], approvals_pending: usize) -> Snapshot {
        let counted = Cycle::count(records);
        let link = records
            .iter()
            .rev()
            .find_map(|r| crate::cost::from_record(r).map(|entry| entry.link));

        Snapshot {
            cycle: projection.cycle,
            stage: projection.stage.clone(),
            in_flight: projection.open_step.as_ref().map(ToString::to_string),
            link,
            tokens: counted.tokens,
            money: counted.money,
            gates: if counted.gates_run == 0 {
                "none run".into()
            } else {
                format!("{}/{} green", counted.gates_green, counted.gates_run)
            },
            blocked: projection.blocked.len(),
            // A step that ended waiting on a person, distinct from one that
            // failed: the first needs someone, the second needs a fix.
            gated: projection
                .blocked
                .iter()
                .filter(|b| b.summary.contains("approval") || b.summary.contains("parked"))
                .count(),
            approvals_pending,
            btw_queued: projection.pending_btw.len(),
        }
    }

    /// One block, for a terminal that is redrawing it.
    pub fn render(&self) -> String {
        let cycle = self.cycle.map(|c| c.to_string()).unwrap_or_else(|| "—".into());
        let stage = self.stage.clone().unwrap_or_else(|| "—".into());
        let in_flight = self.in_flight.clone().unwrap_or_else(|| "nothing".into());
        let link = self.link.clone().unwrap_or_else(|| "none yet".into());
        format!(
            "cycle {cycle} · stage {stage}\n\
             in flight   {in_flight}\n\
             link        {link}\n\
             spent       {} tokens · ${:.6}\n\
             gates       {}\n\
             blocked {} · gated {} · approvals {} · /btw queued {}\n",
            self.tokens, self.money, self.gates, self.blocked, self.gated,
            self.approvals_pending, self.btw_queued,
        )
    }
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::replay;
    use crate::step::StepId;

    const T: i64 = 1_700_000_000;

    fn step(cycle: u32, n: u32) -> StepId {
        StepId::new(cycle, "b14", n).expect("step")
    }

    fn journal() -> Vec<Record> {
        vec![
            Record::intent(step(2, 1), T, "old cycle work").for_requirements(["L-1"]),
            Record::outcome(step(2, 1), T + 60, true, "done").for_requirements(["L-1"]),
            Record::intent(step(3, 2), T + 100, "run the gate").for_requirements(["V-2"]),
            Record::outcome(step(3, 2), T + 160, true, "gate test is green")
                .for_requirements(["V-2"])
                .with_detail("gate: test\nexit 0\n"),
            Record::intent(step(3, 3), T + 170, "run the build").for_requirements(["V-2"]),
            Record::outcome(step(3, 3), T + 200, false, "gate build is red")
                .for_requirements(["V-2"])
                .with_detail("gate: build\nexit 101\n"),
            // An intent with no outcome: what a crash looks like.
            Record::intent(step(3, 4), T + 3_700, "something that never closed"),
        ]
    }

    #[test]
    fn metrics_are_counted_from_records_not_summarised() {
        let counted = Cycle::count_cycle(&journal(), Some(3));
        assert_eq!(counted.steps, 2, "two outcomes");
        assert_eq!(counted.closed_green, 1);
        assert_eq!(counted.closed_red, 1);
        assert_eq!(counted.still_open, 1, "the crash is visible rather than tidied away");
        assert_eq!(counted.gates_run, 2);
        assert_eq!(counted.gates_green, 1);
        assert_eq!(counted.requirements_touched, 1, "V-2, once, not four times");
    }

    #[test]
    fn a_cycle_counts_only_its_own_records() {
        let all = journal();
        let two = Cycle::count_cycle(&all, Some(2));
        let three = Cycle::count_cycle(&all, Some(3));
        assert_eq!(two.steps, 1);
        assert_eq!(three.steps, 2);
        assert_eq!(two.requirements_touched, 1, "L-1 belongs to cycle 2 alone");
    }

    #[test]
    fn the_history_table_has_one_row_per_cycle_oldest_first() {
        let rows = history(&journal());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].cycle, 2);
        assert_eq!(rows[1].cycle, 3);
        let row = rows[1].history_row();
        assert!(row.starts_with("| 3 |"), "{row}");
        assert!(row.contains("$0.0000"), "money is on the row even when it is zero: {row}");
    }

    #[test]
    fn elapsed_is_wall_clock_and_says_so() {
        let counted = Cycle::count_cycle(&journal(), Some(3));
        assert_eq!(counted.elapsed_seconds, 3_600, "first record to last");
        assert_eq!(format_duration(counted.elapsed_seconds), "1h00m");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(600), "10m");
    }

    #[test]
    fn a_zero_cost_cycle_reports_zero_rather_than_nothing() {
        let counted = Cycle::count_cycle(&journal(), Some(3));
        assert_eq!(counted.model_calls, 0);
        assert_eq!(counted.money, 0.0);
        let rows = counted.rows();
        assert!(
            rows.iter().any(|(name, value)| name == "money" && value == "$0.000000"),
            "a free cycle still has a money row: {rows:?}"
        );
    }

    #[test]
    fn the_watch_snapshot_shows_what_is_in_flight() {
        let records = journal();
        let projection = replay(&records);
        let snapshot = Snapshot::of(&projection, &records, 2);

        assert_eq!(snapshot.cycle, Some(3));
        assert_eq!(snapshot.in_flight.as_deref(), Some("c3/b14/s04"));
        assert_eq!(snapshot.approvals_pending, 2);
        assert_eq!(snapshot.gates, "1/2 green");

        let rendered = snapshot.render();
        for expected in ["cycle 3", "c3/b14/s04", "1/2 green", "approvals 2"] {
            assert!(rendered.contains(expected), "missing {expected} in:\n{rendered}");
        }
    }

    #[test]
    fn the_snapshot_is_derived_every_time_rather_than_accumulated() {
        // Two watchers, one journal, same answer — which is only true because
        // neither of them keeps a tally.
        let records = journal();
        let projection = replay(&records);
        let first = Snapshot::of(&projection, &records, 0);
        let second = Snapshot::of(&replay(&records), &records, 0);
        assert_eq!(first, second);
    }

    #[test]
    fn an_empty_journal_produces_zeroes_and_not_a_panic() {
        let counted = Cycle::count(&[]);
        assert_eq!(counted.steps, 0);
        assert_eq!(counted.elapsed_seconds, 0);
        assert!(history(&[]).is_empty());
        let snapshot = Snapshot::of(&replay(&[]), &[], 0);
        assert_eq!(snapshot.gates, "none run");
    }
}
