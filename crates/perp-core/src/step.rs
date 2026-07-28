//! Step ids (`L-22`).
//!
//! `c1/b1/s07` — cycle 1, batch 1, seventh step. Phases use their letter:
//! `c1/A/s05`. The journal, the commit trailer, the board, the status marker
//! and `/explain` all name the same step with the same string, which is the
//! whole point: one string, one meaning, greppable from any of them.
//!
//! Ordering is by `(cycle, seq)`. The stage is a **label, not a sort key** —
//! `seq` is monotonic within a cycle and already orders the work, and pretending
//! phases and batches interleave in some sortable way would invent an order the
//! loop does not have.

use std::cmp::Ordering as CmpOrdering;
use std::fmt;

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StepId {
    pub cycle: u32,
    /// `A`–`G` for a phase, `b1`, `b2`, … for a batch.
    pub stage: String,
    pub seq: u32,
}

impl StepId {
    pub fn new(cycle: u32, stage: impl Into<String>, seq: u32) -> Result<StepId> {
        let stage = stage.into();
        let id = StepId { cycle, stage, seq };
        id.validate()?;
        Ok(id)
    }

    fn validate(&self) -> Result<()> {
        if self.stage.is_empty() {
            return Err(Error::Step {
                text: self.to_string(),
                reason: "the stage is empty".into(),
            });
        }
        if !self.stage.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Error::Step {
                text: self.to_string(),
                reason: "the stage must be alphanumeric — `A` or `b1`".into(),
            });
        }
        Ok(())
    }

    pub fn parse(text: &str) -> Result<StepId> {
        let fail = |reason: &str| Error::Step { text: text.to_string(), reason: reason.into() };

        let mut parts = text.split('/');
        let (Some(cycle), Some(stage), Some(seq), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(fail("expected three parts, `c<cycle>/<stage>/s<seq>`"));
        };

        let cycle = cycle
            .strip_prefix('c')
            .ok_or_else(|| fail("the cycle must start with `c`"))?
            .parse()
            .map_err(|_| fail("the cycle is not a number"))?;
        let seq = seq
            .strip_prefix('s')
            .ok_or_else(|| fail("the sequence must start with `s`"))?
            .parse()
            .map_err(|_| fail("the sequence is not a number"))?;

        let id = StepId { cycle, stage: stage.to_string(), seq };
        id.validate()?;
        Ok(id)
    }

    /// The next step in the same stage.
    pub fn next(&self) -> StepId {
        StepId { cycle: self.cycle, stage: self.stage.clone(), seq: self.seq + 1 }
    }
}

impl fmt::Display for StepId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "c{}/{}/s{:02}", self.cycle, self.stage, self.seq)
    }
}

impl Ord for StepId {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        self.cycle.cmp(&other.cycle).then(self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for StepId {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_batch_step() {
        let id = StepId::parse("c1/b1/s07").expect("parse");
        assert_eq!(id.cycle, 1);
        assert_eq!(id.stage, "b1");
        assert_eq!(id.seq, 7);
        assert_eq!(id.to_string(), "c1/b1/s07");
    }

    #[test]
    fn round_trips_a_phase_step() {
        let id = StepId::parse("c12/A/s05").expect("parse");
        assert_eq!(id.to_string(), "c12/A/s05");
    }

    #[test]
    fn pads_the_sequence_but_not_past_two_digits() {
        let id = StepId::new(1, "b1", 123).expect("new");
        assert_eq!(id.to_string(), "c1/b1/s123");
    }

    #[test]
    fn rejects_what_cannot_be_cited() {
        for bad in ["", "c1/b1", "1/b1/s07", "c1/b1/7", "cx/b1/s07", "c1//s07", "c1/b1/s07/x"] {
            assert!(StepId::parse(bad).is_err(), "should have rejected: {bad}");
        }
    }

    #[test]
    fn rejects_a_stage_that_would_break_the_id() {
        assert!(StepId::new(1, "b/1", 1).is_err());
        assert!(StepId::new(1, "", 1).is_err());
    }

    #[test]
    fn orders_by_cycle_then_sequence() {
        let a = StepId::parse("c1/A/s05").expect("parse");
        let b = StepId::parse("c1/b1/s06").expect("parse");
        let c = StepId::parse("c2/A/s01").expect("parse");
        assert!(a < b, "sequence orders within a cycle regardless of stage");
        assert!(b < c, "a later cycle always sorts later");
    }

    #[test]
    fn next_stays_in_the_stage() {
        let id = StepId::parse("c1/b1/s07").expect("parse");
        assert_eq!(id.next().to_string(), "c1/b1/s08");
    }
}
