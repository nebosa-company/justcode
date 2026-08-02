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

/// How to say what was cached, when zero may not mean what it looks like
/// (`M-29`).
///
/// A subprocess link replaces its system prompt on every call — that is what
/// buys the tool protocol — so `M-12`'s stable prefix is not stable and there
/// is nothing for a cache to hit. The zero it reports is a property of the
/// link, and it reads exactly like a ledger that has stopped counting.
///
/// `None` for the kind means the link is not in the current configuration:
/// a ledger replayed after a link was renamed or removed still has its
/// entries, and guessing on its behalf would be inventing the one fact this
/// exists to stop inventing. It reports the number and says nothing more.
pub fn cached_as_text(kind: Option<crate::link::Kind>, cached: i64) -> String {
    match kind {
        Some(kind) if !kind.prefix_caches() => {
            "no prefix cache on this link — its system prompt changes every call".to_string()
        }
        _ => format!("{cached} cached"),
    }
}

/// The same for a whole ledger, which is rarely all one link (`M-29`).
///
/// All-or-nothing was the first shape and it was useless on real data: this
/// repository's own ledger is 1519 `claude-cli` calls out of 1523, and the four
/// stragglers on an older link made it fall back to a bare `0 cached` — the
/// exact reading `M-29` exists to prevent, on the exact ledger that argued for
/// it. So a mixed ledger says how much of it could not have cached, and a
/// reader can tell a quiet cache from an absent one either way.
pub fn cached_total_as_text(cached: i64, uncacheable: usize, calls: usize) -> String {
    if calls > 0 && uncacheable == calls {
        return "no prefix cache on any link here — their system prompts change every call"
            .to_string();
    }
    if uncacheable > 0 {
        return format!(
            "{cached} cached; {uncacheable} of {calls} calls on links with no prefix cache"
        );
    }
    format!("{cached} cached")
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

/// Whether a call did the work or ran the harness (`N-7`).
///
/// **Engine overhead is bounded and reported**, and it can only be reported if
/// it is counted separately. Compaction, classification, routing probes and
/// summarising are the machine talking to itself; a cost report that folds them
/// into "work" makes the loop look more productive per dollar than it is, and
/// hides the one number that says whether the harness is worth its own cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Planning, coding, gate-fixing, verifying — the batch's actual work.
    Work,
    /// Compaction, classification, summarising, embedding for retrieval.
    Overhead,
}

impl Kind {
    /// From the role that made the call. The role is already on every record
    /// (`M-11`), so this needs no new field and cannot disagree with one.
    pub fn of(role: &str) -> Kind {
        match role {
            "compactor" | "classifier" | "summarizer" | "embedder" => Kind::Overhead,
            _ => Kind::Work,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Work => "work",
            Kind::Overhead => "overhead",
        }
    }
}

/// Work and overhead, side by side (`N-7`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Split {
    pub work: Total,
    pub overhead: Total,
}

impl Split {
    pub fn of(ledger: &Ledger) -> Split {
        let mut split = Split::default();
        for (role, total) in ledger.by_role() {
            let target = match Kind::of(&role) {
                Kind::Work => &mut split.work,
                Kind::Overhead => &mut split.overhead,
            };
            target.calls += total.calls;
            target.usage.cache_hit_tokens += total.usage.cache_hit_tokens;
            target.usage.cache_miss_tokens += total.usage.cache_miss_tokens;
            target.usage.output_tokens += total.usage.output_tokens;
            target.latency_ms += total.latency_ms;
            target.charge += total.charge;
        }
        split
    }

    /// Overhead as a share of everything, or `None` when nothing was spent —
    /// a percentage of zero is a number that means nothing.
    pub fn overhead_share(&self) -> Option<f64> {
        let total = self.work.usage.total() + self.overhead.usage.total();
        (total > 0).then(|| self.overhead.usage.total() as f64 / total as f64)
    }

    pub fn render(&self) -> String {
        let share = match self.overhead_share() {
            Some(share) => format!("{:.1}%", share * 100.0),
            None => "—".into(),
        };
        format!(
            "work      {:>5} calls  {:>9} tokens  {:>11.6}
             overhead  {:>5} calls  {:>9} tokens  {:>11.6}   ({share} of tokens)
",
            self.work.calls,
            self.work.usage.total(),
            self.work.charge,
            self.overhead.calls,
            self.overhead.usage.total(),
            self.overhead.charge,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

        /// `M-29`: a zero that is a property of the link says so.
    ///
    /// A subprocess link replaces its system prompt every call, which is what
    /// buys the tool protocol, so `M-12`'s stable prefix is not stable and
    /// there is nothing to hit. The zero is right and reads exactly like a
    /// ledger that stopped counting — the two were indistinguishable.
    #[test]
    fn a_link_that_cannot_cache_says_so_instead_of_reporting_zero() {
        let said = cached_as_text(Some(crate::link::Kind::ClaudeCli), 0);
        assert!(said.contains("no prefix cache"), "{said}");
        assert!(!said.contains("0 cached"), "the zero was the whole problem: {said}");

        // A link that can cache reports the number, including when it is zero:
        // there, zero means the cache missed, which is a fact about the run.
        let http = cached_as_text(Some(crate::link::Kind::DeepSeek), 0);
        assert_eq!(http, "0 cached");
        assert_eq!(cached_as_text(Some(crate::link::Kind::DeepSeek), 512), "512 cached");
    }

    /// A ledger is rarely all one link, and the mixed case is the real one.
    ///
    /// All-or-nothing was the first shape. This repository's own ledger is 1519
    /// `claude-cli` calls out of 1523, and the stragglers on an older link made
    /// it fall back to a bare `0 cached` — the exact reading `M-29` exists to
    /// prevent, on the exact ledger that argued for it.
    #[test]
    fn a_mixed_ledger_says_how_much_of_it_could_not_have_cached() {
        // All of it.
        assert!(cached_total_as_text(0, 12, 12).contains("no prefix cache on any link"));
        // Nearly all of it — the case that was falling through before.
        let mixed = cached_total_as_text(0, 1522, 1523);
        assert!(mixed.contains("1522 of 1523"), "{mixed}");
        assert!(mixed.contains("0 cached"), "the number is still there: {mixed}");
        // None of it: an ordinary ledger says an ordinary thing.
        assert_eq!(cached_total_as_text(4096, 0, 20), "4096 cached");
        // Nothing at all: no calls, no claim about links.
        assert_eq!(cached_total_as_text(0, 0, 0), "0 cached");
    }

    /// An unknown link is reported, not guessed about.
    ///
    /// A ledger replayed after a link was renamed or removed still has its
    /// entries. Answering on its behalf would invent the one fact this exists
    /// to stop inventing.
    #[test]
    fn a_link_no_longer_configured_is_not_spoken_for() {
        assert_eq!(cached_as_text(None, 0), "0 cached");
    }

#[test]
    fn the_harness_talking_to_itself_is_counted_apart() {
        // `N-7`. A report that folds compaction into "work" makes the loop look
        // more productive per dollar than it is, and hides the number that says
        // whether the harness is worth its own cost.
        assert_eq!(Kind::of("coder"), Kind::Work);
        assert_eq!(Kind::of("verifier"), Kind::Work);
        assert_eq!(Kind::of("compactor"), Kind::Overhead);
        assert_eq!(Kind::of("classifier"), Kind::Overhead);
        assert_eq!(Kind::of("summarizer"), Kind::Overhead);
        assert_eq!(Kind::of("embedder"), Kind::Overhead);
    }

    #[test]
    fn the_split_adds_up_and_reports_a_share() {
        let call = |role: &str, seq: u32, tokens: i64| {
            let entry = Entry {
                step: format!("c4/b19/s{seq:02}"),
                role: role.to_string(),
                link: "here".into(),
                model: "m".into(),
                usage: Usage::from_reply(tokens, tokens, 0, 0),
                latency_ms: 10,
                charge: 0.0,
            };
            annotate(
                Record::outcome(
                    crate::step::StepId::new(4, "b19", seq).expect("step"),
                    100,
                    true,
                    "call",
                ),
                &entry,
            )
        };
        let ledger = Ledger::replay(&[
            call("coder", 1, 300),
            call("compactor", 2, 100),
            call("classifier", 3, 100),
        ]);

        let split = Split::of(&ledger);
        assert_eq!(split.work.calls, 1);
        assert_eq!(split.overhead.calls, 2);
        assert_eq!(split.work.usage.total(), 600);
        assert_eq!(split.overhead.usage.total(), 400);

        let share = split.overhead_share().expect("something was spent");
        assert!((share - 0.4).abs() < 1e-9, "{share}");
        assert!(split.render().contains("40.0% of tokens"), "{}", split.render());
    }

    #[test]
    fn a_share_of_nothing_is_not_a_percentage() {
        // Zero out of zero is not "0% overhead" — it is no data.
        assert_eq!(Split::default().overhead_share(), None);
        assert!(Split::default().render().contains("(— of tokens)"));
    }
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
