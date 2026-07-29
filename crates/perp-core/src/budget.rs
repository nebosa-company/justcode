//! Budgets in three currencies (`L-9`), counted from real usage (`L-10`).
//!
//! Tokens, wall-clock and money. The three are not interchangeable and the loop
//! does not try to convert between them: a local run costs no money and plenty
//! of hours, and a cloud run can burn a dollar in ninety seconds. Either can be
//! the thing that should have stopped it.
//!
//! Two rules give this its shape:
//!
//! - **Money is counted, not estimated.** Every figure comes from a
//!   [`crate::cost::Ledger`] replayed from the journal, which is replayed from
//!   what the links actually reported. There is no "roughly $x" anywhere.
//! - **A limit parks at the next step boundary, it does not interrupt.**
//!   Killing a step mid-flight leaves an open intent, an unclosed process
//!   group and half an edit, and buys back seconds of a budget that is already
//!   spent.

use std::fmt;

use crate::cost::Ledger;
use crate::phase::Park;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Currency {
    Tokens,
    Seconds,
    Money,
}

impl Currency {
    pub fn as_str(self) -> &'static str {
        match self {
            Currency::Tokens => "tokens",
            Currency::Seconds => "wall-clock",
            Currency::Money => "money",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Cycle,
    Batch,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Cycle => "cycle",
            Scope::Batch => "batch",
        }
    }
}

/// A limit in each currency. `None` is "no limit in this currency", which is a
/// deliberate choice rather than a default: a local-only loop usually wants a
/// wall-clock limit and no money limit, and forcing a number for money there
/// would invent one.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Budget {
    pub tokens: Option<i64>,
    pub seconds: Option<i64>,
    pub money: Option<f64>,
}

impl Budget {
    pub fn none() -> Budget {
        Budget::default()
    }

    pub fn tokens(mut self, n: i64) -> Budget {
        self.tokens = Some(n);
        self
    }

    pub fn seconds(mut self, n: i64) -> Budget {
        self.seconds = Some(n);
        self
    }

    pub fn money(mut self, n: f64) -> Budget {
        self.money = Some(n);
        self
    }

    pub fn is_unlimited(&self) -> bool {
        self.tokens.is_none() && self.seconds.is_none() && self.money.is_none()
    }
}

/// What has actually been used.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spend {
    pub tokens: i64,
    pub seconds: i64,
    pub money: f64,
}

impl Spend {
    /// From the ledger, which came from the journal, which came from what the
    /// links reported (`L-10`, `M-11`).
    ///
    /// The ledger knows how long the *links* took (`latency_ms`) but not how
    /// long the loop took: a gate that runs for ten minutes appears in it
    /// nowhere. So money and tokens are read off the ledger, where they were
    /// reported, and elapsed seconds are passed in by the engine, which
    /// observed them. Nothing here derives one from the other.
    pub fn from_ledger(ledger: &Ledger, elapsed_seconds: i64) -> Spend {
        let total = ledger.total();
        Spend { tokens: total.usage.total(), seconds: elapsed_seconds, money: total.charge }
    }

    /// How much of the elapsed time was spent waiting on a link. Not a budget
    /// currency — a diagnostic, for telling "the loop is slow" from "the link
    /// is slow".
    pub fn link_seconds(ledger: &Ledger) -> i64 {
        ledger.total().latency_ms / 1_000
    }

    pub fn plus(self, other: Spend) -> Spend {
        Spend {
            tokens: self.tokens + other.tokens,
            seconds: self.seconds + other.seconds,
            money: self.money + other.money,
        }
    }
}

impl fmt::Display for Spend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} tokens, {}s, ${:.6}", self.tokens, self.seconds, self.money)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Room left in every currency that has a limit.
    Room,
    /// A limit is reached. The work parks at the next step boundary.
    Exhausted { scope: Scope, currency: Currency, limit: String, spent: String },
}

impl Verdict {
    pub fn has_room(&self) -> bool {
        matches!(self, Verdict::Room)
    }

    /// The park record this verdict produces, or `None` if there is room.
    pub fn park(&self) -> Option<Park> {
        match self {
            Verdict::Room => None,
            Verdict::Exhausted { scope, currency, limit, spent } => Some(Park::budget(format!(
                "{} {} — {spent} of {limit}",
                scope.as_str(),
                currency.as_str()
            ))),
        }
    }
}

/// The limits for a run, per cycle and per batch.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Budgets {
    pub cycle: Budget,
    pub batch: Budget,
}

impl Budgets {
    /// Check both scopes at a step boundary (`L-9`).
    ///
    /// The batch is checked first: when both are exhausted, the tighter scope
    /// is the more useful thing to be told, because it is the one the operator
    /// can change without abandoning the cycle.
    pub fn check(&self, cycle_spend: Spend, batch_spend: Spend) -> Verdict {
        match check_one(Scope::Batch, &self.batch, batch_spend) {
            Verdict::Room => check_one(Scope::Cycle, &self.cycle, cycle_spend),
            exhausted => exhausted,
        }
    }
}

fn check_one(scope: Scope, budget: &Budget, spend: Spend) -> Verdict {
    if let Some(limit) = budget.tokens {
        if spend.tokens >= limit {
            return Verdict::Exhausted {
                scope,
                currency: Currency::Tokens,
                limit: limit.to_string(),
                spent: spend.tokens.to_string(),
            };
        }
    }
    if let Some(limit) = budget.seconds {
        if spend.seconds >= limit {
            return Verdict::Exhausted {
                scope,
                currency: Currency::Seconds,
                limit: format!("{limit}s"),
                spent: format!("{}s", spend.seconds),
            };
        }
    }
    if let Some(limit) = budget.money {
        if spend.money >= limit {
            return Verdict::Exhausted {
                scope,
                currency: Currency::Money,
                limit: format!("${limit:.6}"),
                spent: format!("${:.6}", spend.money),
            };
        }
    }
    Verdict::Room
}

/// Read budgets out of binding entries: `budget.cycle.tokens`,
/// `budget.batch.money`, and so on. Absent keys mean no limit in that currency,
/// and an unreadable number is reported rather than silently treated as absent.
pub fn from_entries(entries: &[(String, String)]) -> crate::error::Result<Budgets> {
    let mut budgets = Budgets::default();
    for (key, value) in entries {
        let Some(rest) = key.strip_prefix("budget.") else { continue };
        let mut parts = rest.split('.');
        let (Some(scope), Some(currency), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(crate::error::Error::unbound(
                key,
                "expected `budget.<cycle|batch>.<tokens|seconds|money>`",
            ));
        };
        let target = match scope {
            "cycle" => &mut budgets.cycle,
            "batch" => &mut budgets.batch,
            _ => {
                return Err(crate::error::Error::unbound(key, "the scope is `cycle` or `batch`"));
            }
        };
        let bad = |what: &str| crate::error::Error::unbound(key.clone(), format!("{what}: `{value}`"));
        match currency {
            "tokens" => target.tokens = Some(value.parse().map_err(|_| bad("not a whole number"))?),
            "seconds" => {
                target.seconds = Some(value.parse().map_err(|_| bad("not a whole number"))?)
            }
            "money" => target.money = Some(value.parse().map_err(|_| bad("not an amount"))?),
            _ => return Err(crate::error::Error::unbound(key, "the currency is `tokens`, `seconds` or `money`")),
        }
    }
    Ok(budgets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::{annotate, Entry, Price, Usage};
    use crate::journal::Record;
    use crate::step::StepId;

    fn step(n: u32) -> StepId {
        StepId::new(3, "b12", n).expect("step")
    }

    fn ledger_of(entries: Vec<(Price, Usage, &str)>) -> Ledger {
        let records: Vec<Record> = entries
            .into_iter()
            .enumerate()
            .map(|(i, (price, usage, link))| {
                let seq = u32::try_from(i).unwrap_or(0) + 1;
                let entry = Entry {
                    step: step(seq).to_string(),
                    link: link.to_string(),
                    role: "build".into(),
                    model: "m".into(),
                    usage,
                    latency_ms: 4_000,
                    charge: price.charge(&usage),
                };
                annotate(Record::outcome(step(seq), 1_700_000_000, true, "call"), &entry)
            })
            .collect();
        Ledger::replay(&records)
    }

    #[test]
    fn money_comes_from_the_ledger_not_an_estimate() {
        let price = Price { cache_hit: 0.014, cache_miss: 0.14, output: 0.28 };
        let ledger = ledger_of(vec![(price, Usage::from_reply(1_000_000, 500_000, 0, 0), "ds-fast")]);
        let spend = Spend::from_ledger(&ledger, 90);
        // A million input tokens priced as a miss, half a million out.
        assert!((spend.money - 0.28).abs() < 1e-9, "0.14 + 0.14 = {}", spend.money);
        assert_eq!(spend.tokens, 1_500_000);
        assert_eq!(spend.seconds, 90, "elapsed is observed, never derived from tokens");
        assert_eq!(Spend::link_seconds(&ledger), 4, "and the link's share is separable");
    }

    #[test]
    fn a_local_link_is_free_and_still_costs_hours() {
        let ledger = ledger_of(vec![(Price::FREE, Usage::from_reply(400_000, 200_000, 0, 0), "here")]);
        let spend = Spend::from_ledger(&ledger, 7_200);

        assert_eq!(spend.money, 0.0, "local links count as zero money (`L-10`)");
        let budgets =
            Budgets { cycle: Budget::none().money(5.0).seconds(3_600), batch: Budget::none() };
        let verdict = budgets.check(spend, Spend::default());
        assert!(
            matches!(verdict, Verdict::Exhausted { currency: Currency::Seconds, .. }),
            "a free loop still runs out of the day: {verdict:?}"
        );
    }

    #[test]
    fn the_tighter_scope_is_the_one_reported() {
        let budgets = Budgets {
            cycle: Budget::none().tokens(1_000_000),
            batch: Budget::none().tokens(100_000),
        };
        let spend = Spend { tokens: 2_000_000, seconds: 0, money: 0.0 };
        let Verdict::Exhausted { scope, .. } = budgets.check(spend, spend) else {
            panic!("both are blown");
        };
        assert_eq!(scope, Scope::Batch, "the batch is the one the operator can raise");
    }

    #[test]
    fn a_currency_with_no_limit_never_stops_anything() {
        let budgets = Budgets { cycle: Budget::none().money(1.0), batch: Budget::none() };
        let spend = Spend { tokens: i64::MAX, seconds: i64::MAX, money: 0.5 };
        assert!(budgets.check(spend, spend).has_room(), "only money was limited");
    }

    #[test]
    fn exhaustion_parks_rather_than_stops() {
        let budgets = Budgets { cycle: Budget::none().money(1.0), batch: Budget::none() };
        let spend = Spend { tokens: 0, seconds: 0, money: 1.5 };
        let park = budgets.check(spend, Spend::default()).park().expect("parked");
        assert!(park.resumable, "a budget is not a failure, it is a boundary");
        assert!(park.reason.contains("cycle money"), "{}", park.reason);
        assert!(park.reason.contains("$1.500000"), "carries the real number: {}", park.reason);
    }

    #[test]
    fn the_limit_is_reached_at_the_limit_not_past_it() {
        let budgets = Budgets { cycle: Budget::none().tokens(100), batch: Budget::none() };
        let at = Spend { tokens: 100, seconds: 0, money: 0.0 };
        let under = Spend { tokens: 99, seconds: 0, money: 0.0 };
        assert!(!budgets.check(at, Spend::default()).has_room(), "100 of 100 is spent");
        assert!(budgets.check(under, Spend::default()).has_room());
    }

    #[test]
    fn budgets_come_out_of_the_binding() {
        let entries = vec![
            ("budget.cycle.money".to_string(), "2.50".to_string()),
            ("budget.batch.tokens".to_string(), "250000".to_string()),
            ("path.requirements".to_string(), "docs/perpetum.md".to_string()),
        ];
        let budgets = from_entries(&entries).expect("parse");
        assert_eq!(budgets.cycle.money, Some(2.50));
        assert_eq!(budgets.batch.tokens, Some(250_000));
        assert_eq!(budgets.cycle.tokens, None, "unset is unlimited, not zero");
    }

    #[test]
    fn an_unreadable_budget_is_reported_not_ignored() {
        let entries = vec![("budget.cycle.money".to_string(), "lots".to_string())];
        let err = from_entries(&entries).expect_err("must refuse");
        assert!(format!("{err}").contains("lots"), "names the value: {err}");

        let entries = vec![("budget.forever.money".to_string(), "1".to_string())];
        assert!(from_entries(&entries).is_err(), "an unknown scope is a typo, not a no-op");
    }
}
