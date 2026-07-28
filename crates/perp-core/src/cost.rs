//! What a call cost (`M-11`).
//!
//! Two things make this worth a module rather than a counter.
//!
//! **Cache-hit and cache-miss input are priced separately**, by roughly fifty
//! to one on DeepSeek. A ledger that adds them together reports a number that
//! is wrong by an order of magnitude and looks perfectly reasonable.
//!
//! **The ledger is rebuilt from the journal**, not accumulated in memory. A
//! process that dies mid-batch has still recorded every call it made, and
//! `perp cost` replays them — the same rule as every other projection (`L-4`).
//!
//! Prices come from configuration (`M-14`): they change, and a harness that
//! bakes them in reports yesterday's bill with total confidence.

use crate::error::{Error, Result};
use crate::journal::Record;
use crate::json::Value;

/// Price per million tokens, as configured.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Price {
    pub cache_hit: f64,
    pub cache_miss: f64,
    pub output: f64,
}

impl Price {
    /// A local link: no money, which is not the same as no cost (`L-10`).
    pub const FREE: Price = Price { cache_hit: 0.0, cache_miss: 0.0, output: 0.0 };

    pub fn is_free(&self) -> bool {
        self.cache_hit == 0.0 && self.cache_miss == 0.0 && self.output == 0.0
    }

    /// What one call's usage costs, in the price's currency unit.
    pub fn charge(&self, usage: &Usage) -> f64 {
        let per_million = |tokens: i64, rate: f64| (tokens as f64) * rate / 1_000_000.0;
        per_million(usage.cache_hit_tokens, self.cache_hit)
            + per_million(usage.cache_miss_tokens, self.cache_miss)
            + per_million(usage.output_tokens, self.output)
    }
}

/// The token counts of one call.
///
/// `cache_hit` and `cache_miss` are what the provider reported. When it reports
/// neither — every local link, and most OpenAI-compatible servers — the whole
/// input counts as a miss, which is the conservative reading: it prices as if
/// nothing was cached rather than assuming a discount nobody granted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
    pub output_tokens: i64,
}

impl Usage {
    pub fn from_reply(prompt: i64, output: i64, hit: i64, miss: i64) -> Usage {
        if hit == 0 && miss == 0 {
            Usage { cache_hit_tokens: 0, cache_miss_tokens: prompt, output_tokens: output }
        } else {
            Usage { cache_hit_tokens: hit, cache_miss_tokens: miss, output_tokens: output }
        }
    }

    pub fn input_tokens(&self) -> i64 {
        self.cache_hit_tokens + self.cache_miss_tokens
    }

    pub fn total(&self) -> i64 {
        self.input_tokens() + self.output_tokens
    }

    /// The share of input the provider served from its cache. `None` when
    /// nothing was reported — an unknown ratio, not a zero one.
    pub fn cache_ratio(&self) -> Option<f64> {
        let input = self.input_tokens();
        if input == 0 || (self.cache_hit_tokens == 0 && self.cache_miss_tokens == input) {
            return None;
        }
        Some(self.cache_hit_tokens as f64 / input as f64)
    }
}

/// One call, as the ledger sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub step: String,
    pub role: String,
    pub link: String,
    pub model: String,
    pub usage: Usage,
    pub latency_ms: i64,
    pub charge: f64,
}

/// The fields a call record carries so the ledger can be rebuilt (`N-8` keeps
/// them through any reader that does not know them).
pub const FIELDS: &[&str] =
    &["role", "link", "model", "cache_hit", "cache_miss", "output_tokens", "latency_ms", "charge"];

/// Attach the accounting to a journal record.
pub fn annotate(record: Record, entry: &Entry) -> Record {
    let mut record = record;
    record.extra.retain(|(key, _)| !FIELDS.contains(&key.as_str()));
    record.extra.extend([
        ("role".to_string(), Value::str(entry.role.clone())),
        ("link".to_string(), Value::str(entry.link.clone())),
        ("model".to_string(), Value::str(entry.model.clone())),
        ("cache_hit".to_string(), Value::int(entry.usage.cache_hit_tokens)),
        ("cache_miss".to_string(), Value::int(entry.usage.cache_miss_tokens)),
        ("output_tokens".to_string(), Value::int(entry.usage.output_tokens)),
        ("latency_ms".to_string(), Value::int(entry.latency_ms)),
        ("charge".to_string(), Value::Num(format!("{:.6}", entry.charge))),
    ]);
    record
}

/// Read a call back out of a journal record. `None` for a record that is not
/// one — most of them are not.
pub fn from_record(record: &Record) -> Option<Entry> {
    let field = |name: &str| record.extra.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    let int = |name: &str| field(name).and_then(Value::as_i64).unwrap_or_default();
    let text = |name: &str| field(name).and_then(Value::as_str).unwrap_or_default().to_string();

    field("link")?;
    Some(Entry {
        step: record.step.to_string(),
        role: text("role"),
        link: text("link"),
        model: text("model"),
        usage: Usage {
            cache_hit_tokens: int("cache_hit"),
            cache_miss_tokens: int("cache_miss"),
            output_tokens: int("output_tokens"),
        },
        latency_ms: int("latency_ms"),
        charge: field("charge")
            .and_then(|value| match value {
                Value::Num(text) => text.parse().ok(),
                _ => None,
            })
            .unwrap_or_default(),
    })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Total {
    pub calls: usize,
    pub usage: Usage,
    pub latency_ms: i64,
    pub charge: f64,
}

impl Total {
    fn add(&mut self, entry: &Entry) {
        self.calls += 1;
        self.usage.cache_hit_tokens += entry.usage.cache_hit_tokens;
        self.usage.cache_miss_tokens += entry.usage.cache_miss_tokens;
        self.usage.output_tokens += entry.usage.output_tokens;
        self.latency_ms += entry.latency_ms;
        self.charge += entry.charge;
    }
}

/// Every call in a journal, aggregated the ways a human asks about them.
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    pub entries: Vec<Entry>,
}

impl Ledger {
    /// Rebuild from journal records (`L-4`: derived, never accumulated).
    pub fn replay(records: &[Record]) -> Ledger {
        Ledger { entries: records.iter().filter_map(from_record).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn total(&self) -> Total {
        let mut total = Total::default();
        for entry in &self.entries {
            total.add(entry);
        }
        total
    }

    fn by(&self, key: impl Fn(&Entry) -> String) -> Vec<(String, Total)> {
        let mut grouped: Vec<(String, Total)> = Vec::new();
        for entry in &self.entries {
            let name = key(entry);
            match grouped.iter_mut().find(|(existing, _)| existing == &name) {
                Some((_, total)) => total.add(entry),
                None => {
                    let mut total = Total::default();
                    total.add(entry);
                    grouped.push((name, total));
                }
            }
        }
        grouped.sort_by(|a, b| {
            b.1.charge
                .partial_cmp(&a.1.charge)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        grouped
    }

    pub fn by_role(&self) -> Vec<(String, Total)> {
        self.by(|entry| entry.role.clone())
    }

    pub fn by_link(&self) -> Vec<(String, Total)> {
        self.by(|entry| entry.link.clone())
    }

    pub fn by_step(&self) -> Vec<(String, Total)> {
        self.by(|entry| entry.step.clone())
    }

    /// Which calls left the operator's hardware. `L-10`: local links cost no
    /// money and still cost wall-clock, so both are reported.
    pub fn spent_on(&self, links: &[String]) -> Total {
        let mut total = Total::default();
        for entry in self.entries.iter().filter(|entry| links.contains(&entry.link)) {
            total.add(entry);
        }
        total
    }
}

/// Parse `price.<link>.<field>` entries from the links configuration.
pub fn prices(entries: &[(String, String)]) -> Result<Vec<(String, Price)>> {
    let mut prices: Vec<(String, Price)> = Vec::new();
    for (key, value) in entries {
        let Some(rest) = key.strip_prefix("price.") else { continue };
        let Some((link, field)) = rest.split_once('.') else {
            return Err(Error::unbound(key.clone(), "expected `price.<link>.<field>`"));
        };
        let amount: f64 = value.trim().parse().map_err(|_| {
            Error::unbound(key.clone(), format!("`{value}` is not a number of currency units"))
        })?;
        if amount < 0.0 {
            return Err(Error::unbound(key.clone(), "a negative price is not a discount"));
        }

        let slot = match prices.iter_mut().find(|(name, _)| name == link) {
            Some((_, price)) => price,
            None => {
                prices.push((link.to_string(), Price::default()));
                let last = prices.len() - 1;
                &mut prices[last].1
            }
        };
        match field {
            "cache_hit" => slot.cache_hit = amount,
            "cache_miss" => slot.cache_miss = amount,
            "output" => slot.output = amount,
            other => {
                return Err(Error::unbound(
                    key.clone(),
                    format!("`{other}` is not cache_hit, cache_miss or output"),
                ))
            }
        }
    }
    Ok(prices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step::StepId;

    fn step(text: &str) -> StepId {
        StepId::parse(text).expect("step")
    }

    /// DeepSeek v4-flash, per million tokens, as of 2026-07-28.
    fn flash() -> Price {
        Price { cache_hit: 0.0028, cache_miss: 0.14, output: 0.28 }
    }

    fn entry(link: &str, role: &str, hit: i64, miss: i64, out: i64, price: Price) -> Entry {
        let usage = Usage {
            cache_hit_tokens: hit,
            cache_miss_tokens: miss,
            output_tokens: out,
        };
        Entry {
            step: "c2/b9/s01".into(),
            role: role.into(),
            link: link.into(),
            model: "m".into(),
            charge: price.charge(&usage),
            usage,
            latency_ms: 100,
        }
    }

    #[test]
    fn cache_hits_and_misses_are_priced_apart() {
        // The whole reason this is not one counter: fifty to one.
        let hits = Usage { cache_hit_tokens: 1_000_000, cache_miss_tokens: 0, output_tokens: 0 };
        let misses = Usage { cache_hit_tokens: 0, cache_miss_tokens: 1_000_000, output_tokens: 0 };
        assert!((flash().charge(&hits) - 0.0028).abs() < 1e-9);
        assert!((flash().charge(&misses) - 0.14).abs() < 1e-9);
        assert!(flash().charge(&misses) > flash().charge(&hits) * 40.0);
    }

    #[test]
    fn an_unreported_cache_split_counts_as_all_miss() {
        // The conservative reading: price as if nothing was cached, rather than
        // assume a discount nobody granted.
        let usage = Usage::from_reply(1000, 50, 0, 0);
        assert_eq!(usage.cache_miss_tokens, 1000);
        assert_eq!(usage.cache_hit_tokens, 0);
        assert_eq!(usage.cache_ratio(), None, "unknown, not zero");

        let reported = Usage::from_reply(1000, 50, 900, 100);
        assert_eq!(reported.cache_hit_tokens, 900);
        assert_eq!(reported.cache_ratio(), Some(0.9));
    }

    #[test]
    fn a_local_link_costs_no_money_and_still_costs_time() {
        // `L-10`.
        let local = entry("here", "compactor", 0, 5000, 200, Price::FREE);
        assert_eq!(local.charge, 0.0);
        assert!(local.latency_ms > 0);
        assert!(Price::FREE.is_free());
    }

    #[test]
    fn the_ledger_is_rebuilt_from_the_journal() {
        // `L-4` again: derived, not accumulated. A process that died mid-batch
        // still recorded what it spent.
        let spent = entry("cloud", "coder", 900, 100, 50, flash());
        let record = annotate(Record::outcome(step("c2/b9/s01"), 10, true, "called"), &spent);

        let ledger = Ledger::replay(&[record]);
        assert_eq!(ledger.entries.len(), 1);
        let read_back = &ledger.entries[0];
        assert_eq!(read_back.link, "cloud");
        assert_eq!(read_back.role, "coder");
        assert_eq!(read_back.usage.cache_hit_tokens, 900);
        assert!((read_back.charge - spent.charge).abs() < 1e-6, "{read_back:?}");
    }

    #[test]
    fn records_that_are_not_calls_are_skipped() {
        let ordinary = Record::outcome(step("c2/b9/s02"), 10, true, "wrote a file");
        assert!(from_record(&ordinary).is_none());
        assert!(Ledger::replay(&[ordinary]).is_empty());
    }

    #[test]
    fn totals_are_grouped_the_ways_a_human_asks() {
        let ledger = Ledger {
            entries: vec![
                entry("cloud", "coder", 0, 10_000, 1000, flash()),
                entry("cloud", "planner", 0, 1_000, 100, flash()),
                entry("here", "compactor", 0, 50_000, 500, Price::FREE),
            ],
        };

        let total = ledger.total();
        assert_eq!(total.calls, 3);
        assert_eq!(total.usage.total(), 62_600);

        let by_link = ledger.by_link();
        assert_eq!(by_link[0].0, "cloud", "sorted by what it cost");
        assert_eq!(by_link[0].1.calls, 2);
        assert_eq!(by_link.iter().find(|(name, _)| name == "here").expect("here").1.charge, 0.0);

        let by_role = ledger.by_role();
        assert_eq!(by_role[0].0, "coder", "the expensive role first");
        assert_eq!(by_role.len(), 3);
    }

    #[test]
    fn money_that_left_the_machine_is_separable_from_money_that_did_not() {
        let ledger = Ledger {
            entries: vec![
                entry("cloud", "coder", 0, 10_000, 1000, flash()),
                entry("here", "compactor", 0, 50_000, 500, Price::FREE),
            ],
        };
        let cloud = ledger.spent_on(&["cloud".to_string()]);
        assert_eq!(cloud.calls, 1);
        assert!(cloud.charge > 0.0);
        assert!(
            cloud.usage.total() < ledger.total().usage.total(),
            "the local link did most of the tokens and none of the spending"
        );
    }

    #[test]
    fn prices_come_from_configuration() {
        // `M-14`'s sibling: prices rot, so they are not baked in.
        let entries = vec![
            ("price.ds.cache_hit".to_string(), "0.0028".to_string()),
            ("price.ds.cache_miss".to_string(), "0.14".to_string()),
            ("price.ds.output".to_string(), "0.28".to_string()),
            ("link.ds.kind".to_string(), "deepseek".to_string()),
        ];
        let prices = prices(&entries).expect("parse");
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0].0, "ds");
        assert_eq!(prices[0].1, flash());
    }

    #[test]
    fn a_price_that_is_not_a_price_is_refused() {
        for bad in [
            vec![("price.ds.cache_hit".to_string(), "cheap".to_string())],
            vec![("price.ds.cache_hit".to_string(), "-1".to_string())],
            vec![("price.ds.vibes".to_string(), "1.0".to_string())],
            vec![("price.ds".to_string(), "1.0".to_string())],
        ] {
            assert!(prices(&bad).is_err(), "should have refused: {bad:?}");
        }
    }
}
