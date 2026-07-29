//! Replay and projection (`O-1`, `L-4`, `L-8`).
//!
//! The journal is the truth. The state file is a rendering of it, rewritten
//! after each outcome and never edited by hand — if the two disagree, the
//! journal wins, so the projection has to be derivable and nothing may live
//! only in the file.
//!
//! Replay is also where recovery starts: the last intent with no outcome is the
//! step that was in flight when the process died (`L-7`).

use crate::journal::{Kind, Record};
use crate::step::StepId;
use crate::time;

#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    pub step: StepId,
    pub summary: String,
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Blocked {
    pub step: StepId,
    pub summary: String,
    pub requirements: Vec<String>,
    /// The actual error, not a summary of it (Perpetum 0.5).
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Projection {
    pub cycle: Option<u32>,
    pub stage: Option<String>,
    pub last_step: Option<StepId>,
    /// An intent with no outcome — what was in flight when the run stopped.
    pub open_step: Option<StepId>,
    pub open_summary: Option<String>,
    pub done: Vec<Done>,
    pub blocked: Vec<Blocked>,
    pub requirements_touched: Vec<String>,
    /// Asides still waiting on something (`C-12`). In the projection rather
    /// than in a file of their own, because a queue that lives outside the
    /// journal is a queue that can disagree with it.
    pub pending_btw: Vec<crate::btw::Btw>,
}

impl Projection {
    pub fn steps_recorded(&self) -> usize {
        self.done.len() + self.blocked.len() + usize::from(self.open_step.is_some())
    }
}

/// Fold a journal into what the run believed.
pub fn replay(records: &[Record]) -> Projection {
    let mut projection = Projection {
        pending_btw: crate::btw::Queue::replay(records).pending().into_iter().cloned().collect(),
        ..Projection::default()
    };
    let mut open: Vec<(StepId, String)> = Vec::new();

    for record in records {
        projection.cycle = Some(record.step.cycle);
        projection.stage = Some(record.step.stage.clone());
        projection.last_step = Some(record.step.clone());

        for id in &record.requirements {
            if !projection.requirements_touched.iter().any(|seen| seen == id) {
                projection.requirements_touched.push(id.clone());
            }
        }

        match record.kind {
            Kind::Intent => open.push((record.step.clone(), record.summary.clone())),
            Kind::Outcome => {
                let intent = open.iter().position(|(step, _)| *step == record.step);
                let summary = match intent {
                    Some(index) => {
                        let (_, summary) = open.remove(index);
                        // An outcome's own summary is the more specific one when
                        // it has something to add.
                        if record.summary.is_empty() { summary } else { record.summary.clone() }
                    }
                    None => record.summary.clone(),
                };
                let entry_requirements = record.requirements.clone();
                if record.ok == Some(false) {
                    projection.blocked.push(Blocked {
                        step: record.step.clone(),
                        summary,
                        requirements: entry_requirements,
                        detail: record.detail.clone(),
                    });
                } else {
                    projection.done.push(Done {
                        step: record.step.clone(),
                        summary,
                        requirements: entry_requirements,
                    });
                }
            }
        }
    }

    // Only the newest unclosed intent is "in flight"; anything older that never
    // closed is a hole the reconcile has to look at, and it shows up as one
    // because its step never reaches `done`.
    if let Some((step, summary)) = open.pop() {
        projection.open_step = Some(step);
        projection.open_summary = Some(summary);
    }

    projection
}

/// Render the state file (`L-4`). Generated — the header says so, because a
/// hand-edit here is a bug that looks like a fact.
pub fn render(projection: &Projection, at: i64) -> String {
    let mut out = String::new();

    let cycle = projection.cycle.map(|c| c.to_string()).unwrap_or_else(|| "—".into());
    let stage = projection.stage.clone().unwrap_or_else(|| "—".into());

    out.push_str("# Perpetum state\n\n");
    out.push_str(&format!(
        "Cycle: {cycle} · Stage: {stage} · Updated: {}\n\n",
        time::format_date(at)
    ));
    out.push_str(
        "Generated from the journal by `perp state`. The journal is the truth; \
         if this file disagrees with it, this file is wrong.\n\n",
    );

    out.push_str("## Position\n\n");
    match (&projection.open_step, &projection.open_summary) {
        (Some(step), Some(summary)) => {
            out.push_str(&format!("- In flight: `{step}` — {summary}\n"));
            out.push_str("- That step recorded an intent and no outcome. Reconcile before continuing.\n");
        }
        _ => {
            match &projection.last_step {
                Some(step) => out.push_str(&format!("- Last step: `{step}` — closed\n")),
                None => out.push_str("- Nothing recorded yet.\n"),
            }
            out.push_str("- Nothing in flight.\n");
        }
    }
    out.push_str(&format!(
        "- Steps: {} done, {} blocked\n\n",
        projection.done.len(),
        projection.blocked.len()
    ));

    out.push_str("## Blocked\n\n");
    if projection.blocked.is_empty() {
        out.push_str("*(none)*\n\n");
    } else {
        for entry in &projection.blocked {
            out.push_str(&format!("- `{}` — {}\n", entry.step, entry.summary));
            if let Some(detail) = &entry.detail {
                out.push_str("\n  ```\n");
                for line in detail.lines() {
                    out.push_str(&format!("  {line}\n"));
                }
                out.push_str("  ```\n\n");
            }
        }
        out.push('\n');
    }

    // `C-12`: whatever the operator said that nobody has acted on yet, above
    // the counts, so someone resuming the loop reads it before the detail.
    if !projection.pending_btw.is_empty() {
        out.push_str("## Waiting from `/btw`\n\n");
        for item in &projection.pending_btw {
            out.push_str(&format!(
                "- #{} · **{}** · {} · from {} — {}\n",
                item.id,
                item.class,
                time::format_date(item.at),
                item.source,
                item.text
            ));
        }
        out.push_str(
            "\nThese survive a restart. A steer lands at the next step boundary; a requirement \
             is filed at Phase B (`C-11`).\n\n",
        );
    }

    out.push_str("## Requirements touched\n\n");
    if projection.requirements_touched.is_empty() {
        out.push_str("*(none)*\n\n");
    } else {
        let cited: Vec<String> = projection
            .requirements_touched
            .iter()
            .map(|id| format!("`{id}`"))
            .collect();
        out.push_str(&cited.join(" · "));
        out.push_str("\n\n");
    }

    out.push_str("## Steps\n\n");
    if projection.done.is_empty() && projection.blocked.is_empty() {
        out.push_str("*(none closed yet)*\n");
    } else {
        out.push_str("| Step | Outcome | Summary |\n|---|---|---|\n");
        let mut rows: Vec<(&StepId, &str, &str)> = Vec::new();
        for entry in &projection.done {
            rows.push((&entry.step, "done", entry.summary.as_str()));
        }
        for entry in &projection.blocked {
            rows.push((&entry.step, "blocked", entry.summary.as_str()));
        }
        rows.sort_by(|a, b| a.0.cmp(b.0));
        for (step, outcome, summary) in rows {
            out.push_str(&format!("| `{step}` | {outcome} | {summary} |\n"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::Record;

    fn step(text: &str) -> StepId {
        StepId::parse(text).expect("step id")
    }

    #[test]
    fn a_waiting_aside_reaches_the_state_file() {
        // `C-12`. The queue is not a separate file: it is replayed from the
        // journal like everything else in the projection, so a restart cannot
        // lose it and it cannot disagree with the record.
        let mut queue = crate::btw::Queue::new();
        queue.accept("we should show the transcript inline", "panel", 1_700_000_000, None);
        queue.accept("just noting the lint gate is the slow one", "cli", 1_700_000_000, None);
        let records: Vec<Record> =
            queue.items().iter().map(|item| item.record(step("c3/b13/s09"))).collect();

        let projection = replay(&records);
        assert_eq!(projection.pending_btw.len(), 1, "a note is not waiting for anything");

        let rendered = render(&projection, 1_700_000_000);
        let section = rendered
            .split("## Waiting from `/btw`")
            .nth(1)
            .expect("the section is there")
            .split("\n## ")
            .next()
            .expect("and ends at the next heading");
        assert!(section.contains("transcript inline"), "with the operator's own words:{section}");
        assert!(section.contains("from panel"), "and where it came from:{section}");
        // The note is elsewhere in the file — every `/btw` is a journal record
        // and shows in the step table — but it is not *waiting* for anything.
        assert!(!section.contains("slow one"), "and nothing that is not waiting:{section}");
    }

    #[test]
    fn a_state_file_with_no_asides_carries_no_empty_heading() {
        let rendered = render(&replay(&closed_pair("c1/b1/s01", true, "did a thing")), 100);
        assert!(!rendered.contains("Waiting from"), "{rendered}");
    }

    fn closed_pair(id: &str, ok: bool, summary: &str) -> Vec<Record> {
        vec![
            Record::intent(step(id), 100, summary),
            Record::outcome(step(id), 200, ok, summary),
        ]
    }

    #[test]
    fn an_empty_journal_projects_to_nothing_in_flight() {
        let projection = replay(&[]);
        assert_eq!(projection.open_step, None);
        assert!(projection.done.is_empty());
        assert!(render(&projection, 0).contains("Nothing recorded yet"));
    }

    #[test]
    fn a_closed_step_is_done() {
        let projection = replay(&closed_pair("c1/b1/s01", true, "load the binding"));
        assert_eq!(projection.done.len(), 1);
        assert_eq!(projection.open_step, None);
        assert_eq!(projection.last_step, Some(step("c1/b1/s01")));
    }

    #[test]
    fn an_intent_with_no_outcome_is_what_recovery_looks_at() {
        // `L-7`: this is the whole reason intent and outcome are two records.
        let records = vec![Record::intent(step("c1/b1/s04"), 100, "write the projection")];
        let projection = replay(&records);
        assert_eq!(projection.open_step, Some(step("c1/b1/s04")));
        assert_eq!(projection.open_summary.as_deref(), Some("write the projection"));
        assert!(render(&projection, 0).contains("Reconcile before continuing"));
    }

    #[test]
    fn a_failed_outcome_blocks_and_keeps_the_error_verbatim() {
        let transcript = "error[E0432]: unresolved import\n  --> src/lib.rs:3:5";
        let records = vec![
            Record::intent(step("c1/b1/s05"), 100, "compile"),
            Record::outcome(step("c1/b1/s05"), 200, false, "build failed")
                .with_detail(transcript)
                .for_requirements(["N-9"]),
        ];
        let projection = replay(&records);
        assert_eq!(projection.blocked.len(), 1);
        assert_eq!(projection.blocked[0].detail.as_deref(), Some(transcript));

        let rendered = render(&projection, 0);
        assert!(rendered.contains("unresolved import"), "the error survives to the file");
        assert!(rendered.contains("`N-9`"), "the requirement is cited");
    }

    #[test]
    fn replay_is_deterministic() {
        // `O-1`: the same journal renders the same state, every time.
        let mut records = closed_pair("c1/b1/s01", true, "one");
        records.extend(closed_pair("c1/b1/s02", false, "two"));
        let first = render(&replay(&records), 1_700_000_000);
        let second = render(&replay(&records), 1_700_000_000);
        assert_eq!(first, second);
    }

    #[test]
    fn requirements_are_collected_without_duplicates() {
        let mut records = closed_pair("c1/b1/s01", true, "one");
        records[1].requirements = vec!["L-3".into(), "L-4".into()];
        let mut more = closed_pair("c1/b1/s02", true, "two");
        more[1].requirements = vec!["L-4".into(), "O-1".into()];
        records.extend(more);

        let projection = replay(&records);
        assert_eq!(projection.requirements_touched, vec!["L-3", "L-4", "O-1"]);
    }

    #[test]
    fn steps_are_listed_in_step_order_not_outcome_order() {
        let mut records = closed_pair("c1/b1/s02", false, "later, blocked");
        records.extend(closed_pair("c1/b1/s01", true, "earlier, done"));
        let rendered = render(&replay(&records), 0);
        let first = rendered.find("c1/b1/s01").unwrap_or(usize::MAX);
        let second = rendered.find("c1/b1/s02").unwrap_or(0);
        assert!(first < second, "the table sorts by step id:\n{rendered}");
    }
}
