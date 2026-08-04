//! What was chosen, and what else was there (`O-8`–`O-13`).
//!
//! The journal records what happened. This records what *else could have
//! happened* — and that is the half nobody can reconstruct afterwards.
//!
//! "The coder is `ds-pro`" is a fact. "`ds-pro` over `ds-fast`, because
//! `ds-fast` declined this requirement five times" is a decision. Reading the
//! journal a week later, the first is recoverable from any call record and the
//! second is gone: the alternatives were never written down, so the fork looks
//! like a foregone conclusion. Every silent fork reads that way in hindsight,
//! which is why `O-8` makes the alternatives mandatory rather than nice.
//!
//! ## Three deciders, and no fourth (`O-9`)
//!
//! `rule` — the engine applied a stated rule, and cites it. `model` — the model
//! chose, and the record names the link and model, as `M-10` does for anything
//! a model produced. `person` — an operator chose, and is named.
//!
//! There is deliberately no "the system decided". A decision with no decider is
//! refused at construction, because attributing a choice to the harness is what
//! gets written when nobody looked, and it is indistinguishable afterwards from
//! a choice nobody made on purpose.
//!
//! ## Derived, never stored (`O-13`)
//!
//! The log is replayed from the journal like the ledger and the state file, so
//! there is one source of truth and no second file to disagree with it (`O-1`,
//! `L-4`). Replaying the same journal produces the same log on any machine
//! (`N-5`).
//!
//! And only what the engine *enacted* is here. A model that says "I chose the
//! simpler approach" has asserted a decision, not made one — `V-2`'s rule about
//! self-reported success, applied to self-reported reasoning.

use crate::journal::Record;
use crate::step::StepId;

/// Who chose (`O-9`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decider {
    /// The engine applied a stated rule, and says which.
    Rule { cites: String },
    /// The model chose. Named as `M-10` names anything a model produced.
    Model { link: String, model: String },
    /// An operator chose, and is named.
    Person { name: String },
}

impl Decider {
    /// The word `perp decisions --decider` filters on.
    pub fn kind(&self) -> &'static str {
        match self {
            Decider::Rule { .. } => "rule",
            Decider::Model { .. } => "model",
            Decider::Person { .. } => "person",
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Decider::Rule { cites } => format!("rule ({cites})"),
            Decider::Model { link, model } => format!("model ({link} · {model})"),
            Decider::Person { name } => format!("person ({name})"),
        }
    }
}

/// A fork the loop took, with the road not taken (`O-8`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub id: u64,
    pub chose: String,
    /// What else was available. The part that cannot be reconstructed later,
    /// and therefore the part that must be written down.
    pub over: Vec<String>,
    pub why: String,
    pub decider: Decider,
    pub step: StepId,
    pub at: i64,
    pub requirement: Option<String>,
    /// The decision this one replaces (`O-11`).
    pub supersedes: Option<u64>,
}

impl Decision {
    /// Ids come from the step and a counter within it, so replaying the same
    /// journal on another machine numbers them identically (`N-5`).
    pub fn new(
        chose: impl Into<String>,
        over: Vec<String>,
        why: impl Into<String>,
        decider: Decider,
        step: StepId,
        at: i64,
    ) -> Decision {
        Decision {
            id: 0,
            chose: chose.into(),
            over,
            why: why.into(),
            decider,
            step,
            at,
            requirement: None,
            supersedes: None,
        }
    }

    pub fn for_requirement(mut self, id: impl Into<String>) -> Decision {
        self.requirement = Some(id.into());
        self
    }

    /// Replace an earlier decision (`O-11`).
    ///
    /// A new record rather than an edit: `L-3` makes the journal append-only,
    /// and a log that quietly loses its reversals reads as though nobody was
    /// ever wrong. The reversal is the most useful record in it — the only one
    /// carrying what was learned.
    pub fn superseding(mut self, earlier: u64) -> Decision {
        self.supersedes = Some(earlier);
        self
    }

    /// The journal record. One line per field, so a human reading the raw
    /// journal can see the fork without a tool.
    pub fn to_record(&self) -> Record {
        let mut detail = String::from("decision\n");
        detail.push_str(&format!("chose={}\n", self.chose));
        if !self.over.is_empty() {
            detail.push_str(&format!("over={}\n", self.over.join(" | ")));
        }
        detail.push_str(&format!("why={}\n", self.why));
        detail.push_str(&format!("decider={}\n", self.decider.kind()));
        match &self.decider {
            Decider::Rule { cites } => detail.push_str(&format!("cites={cites}\n")),
            Decider::Model { link, model } => {
                detail.push_str(&format!("link={link}\nmodel={model}\n"));
            }
            Decider::Person { name } => detail.push_str(&format!("by={name}\n")),
        }
        if let Some(requirement) = &self.requirement {
            detail.push_str(&format!("for={requirement}\n"));
        }
        if let Some(earlier) = self.supersedes {
            detail.push_str(&format!("supersedes={earlier}\n"));
        }

        let summary = if self.over.is_empty() {
            format!("chose {}", self.chose)
        } else {
            format!("chose {} over {}", self.chose, self.over.join(", "))
        };
        let record = Record::outcome(self.step.clone(), self.at, true, summary).with_detail(detail);
        match &self.requirement {
            Some(id) => record.for_requirements([id.clone()]),
            None => record,
        }
    }

    /// Whether this decision has been replaced by a later one.
    pub fn superseded_by(&self, log: &[Decision]) -> Option<u64> {
        log.iter().find(|other| other.supersedes == Some(self.id)).map(|other| other.id)
    }

    pub fn describe(&self) -> String {
        let mut out = format!("#{} {}", self.id, self.chose);
        if !self.over.is_empty() {
            out.push_str(&format!("  (over {})", self.over.join(", ")));
        }
        out.push('\n');
        out.push_str(&format!("   why: {}\n", self.why));
        out.push_str(&format!("   who: {}\n", self.decider.describe()));
        out.push_str(&format!("  step: {}\n", self.step));
        if let Some(earlier) = self.supersedes {
            out.push_str(&format!("  replaces: #{earlier}\n"));
        }
        out
    }
}

/// The log, replayed from the journal (`O-13`).
///
/// Derived and never stored. Ids are assigned in journal order, so the same
/// journal produces the same log anywhere (`N-5`) — which is what lets a
/// reversal name the thing it reverses across machines.
pub fn log(records: &[Record]) -> Vec<Decision> {
    let mut out: Vec<Decision> = Vec::new();
    let mut next_id = 1;

    for record in records {
        let Some(detail) = &record.detail else { continue };
        if !detail.starts_with("decision\n") {
            continue;
        }
        let field = |name: &str| -> Option<String> {
            detail
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{name}=")))
                .map(str::to_string)
        };

        // `O-9`: no decider means the record is not a decision. Skipped rather
        // than admitted as an anonymous one — the whole point is that nothing
        // in this log is attributable to nobody.
        let decider = match field("decider").as_deref() {
            Some("rule") => Decider::Rule { cites: field("cites").unwrap_or_default() },
            Some("model") => Decider::Model {
                link: field("link").unwrap_or_default(),
                model: field("model").unwrap_or_default(),
            },
            Some("person") => Decider::Person { name: field("by").unwrap_or_default() },
            _ => continue,
        };
        let Some(chose) = field("chose") else { continue };

        out.push(Decision {
            id: next_id,
            chose,
            over: field("over")
                .map(|text| text.split(" | ").map(str::trim).map(str::to_string).collect())
                .unwrap_or_default(),
            why: field("why").unwrap_or_default(),
            decider,
            step: record.step.clone(),
            at: record.at,
            requirement: field("for"),
            supersedes: field("supersedes").and_then(|v| v.parse().ok()),
        });
        next_id += 1;
    }
    out
}

/// The decisions bearing on one requirement (`O-12`).
pub fn bearing_on<'a>(log: &'a [Decision], requirement: &str) -> Vec<&'a Decision> {
    log.iter().filter(|d| d.requirement.as_deref() == Some(requirement)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step() -> StepId {
        StepId::parse("c3/b1/s07").expect("step")
    }

    /// `O-8`: the alternatives are the point.
    ///
    /// "the coder is ds-pro" is a fact and survives in any call record. "ds-pro
    /// over ds-fast, because ds-fast declined this five times" is a decision,
    /// and the second half of it exists nowhere else.
    #[test]
    fn a_decision_carries_what_was_not_chosen() {
        let decision = Decision::new(
            "ds-pro",
            vec!["ds-fast".into(), "here".into()],
            "ds-fast declined this requirement five times",
            Decider::Rule { cites: "M-9".into() },
            step(),
            1_700_000_000,
        )
        .for_requirement("R-12");

        let replayed = log(&[decision.to_record()]);

        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].chose, "ds-pro");
        assert_eq!(replayed[0].over, vec!["ds-fast".to_string(), "here".to_string()]);
        assert_eq!(replayed[0].why, "ds-fast declined this requirement five times");
        assert_eq!(replayed[0].requirement.as_deref(), Some("R-12"));
    }

    /// `O-9`: all three deciders survive the round trip, with what identifies
    /// each of them.
    #[test]
    fn every_decider_names_itself() {
        let deciders = [
            Decider::Rule { cites: "L-11".into() },
            Decider::Model { link: "ds-pro".into(), model: "deepseek-v4-pro".into() },
            Decider::Person { name: "ivelin".into() },
        ];
        let records: Vec<Record> = deciders
            .iter()
            .map(|d| {
                Decision::new("x", vec!["y".into()], "because", d.clone(), step(), 1)
                    .to_record()
            })
            .collect();

        let replayed = log(&records);

        assert_eq!(replayed.len(), 3);
        assert_eq!(replayed[0].decider, Decider::Rule { cites: "L-11".into() });
        assert_eq!(
            replayed[1].decider,
            Decider::Model { link: "ds-pro".into(), model: "deepseek-v4-pro".into() }
        );
        assert_eq!(replayed[2].decider, Decider::Person { name: "ivelin".into() });
    }

    /// `O-9`: "the system decided" is what gets written when nobody looked, so
    /// a record with no decider is not a decision at all.
    #[test]
    fn a_record_with_no_decider_is_not_in_the_log() {
        let orphan = Record::outcome(step(), 1, true, "chose something")
            .with_detail("decision\nchose=ds-pro\nwhy=felt right\n");

        assert!(
            log(&[orphan]).is_empty(),
            "an unattributable choice must be refused, not admitted anonymously"
        );
    }

    /// `O-11`: a reversal is a new record naming the one it replaces, and both
    /// survive — a log that loses its reversals reads as though nobody was
    /// ever wrong.
    #[test]
    fn a_reversal_keeps_both_and_names_what_it_replaced() {
        let first = Decision::new(
            "the repo map is on",
            vec!["off".into()],
            "a model given only grep spends its turns exploring",
            Decider::Rule { cites: "T-27".into() },
            step(),
            1,
        );
        let second = Decision::new(
            "the repo map is off",
            vec!["on".into()],
            "measured over six pairs: 21% more cost, no gain",
            Decider::Person { name: "ivelin".into() },
            step(),
            2,
        )
        .superseding(1);

        let replayed = log(&[first.to_record(), second.to_record()]);

        assert_eq!(replayed.len(), 2, "the reversed decision is kept, not removed");
        assert_eq!(replayed[1].supersedes, Some(1));
        assert_eq!(
            replayed[0].superseded_by(&replayed),
            Some(2),
            "and the original knows it was replaced"
        );
    }

    /// `O-13`/`N-5`: the same journal produces the same log, ids included.
    #[test]
    fn replaying_the_same_journal_gives_the_same_log() {
        let records: Vec<Record> = (0..4)
            .map(|n| {
                Decision::new(
                    format!("choice {n}"),
                    vec!["other".into()],
                    "why",
                    Decider::Rule { cites: "L-14".into() },
                    step(),
                    n,
                )
                .to_record()
            })
            .collect();

        assert_eq!(log(&records), log(&records));
        assert_eq!(log(&records).iter().map(|d| d.id).collect::<Vec<_>>(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn decisions_can_be_read_per_requirement() {
        let records = vec![
            Decision::new("a", vec![], "why", Decider::Rule { cites: "X".into() }, step(), 1)
                .for_requirement("R-1")
                .to_record(),
            Decision::new("b", vec![], "why", Decider::Rule { cites: "X".into() }, step(), 2)
                .for_requirement("R-2")
                .to_record(),
        ];
        let replayed = log(&records);

        let found = bearing_on(&replayed, "R-1");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].chose, "a");
    }
}
