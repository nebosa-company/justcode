//! Prompt shape and compaction (`M-12`, `M-13`).
//!
//! A provider's prefix cache only pays when the prefix is **byte-identical**
//! to last time. DeepSeek prices a cache hit at roughly one fiftieth of a miss,
//! which turns prompt layout from a style question into an engineering one: put
//! everything stable first, in a fixed order, and let only the tail move.
//!
//! The failure this prevents is subtle. Reordering two system segments between
//! calls costs nothing visible — the model behaves the same, the tests pass,
//! and every call silently pays full price. So the order is enforced by a
//! fingerprint rather than by a convention ([`PrefixGuard`]).
//!
//! Compaction (`M-13`) is the other half: when the tail outgrows the window,
//! something has to go, and what went has to be recoverable from the journal.

use crate::error::{Error, Result};
use crate::link::{Link, Privacy};
use crate::watchdog::content_hash;

/// One labelled piece of a prompt. The label is what makes a compaction record
/// readable a month later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub content: String,
}

impl Segment {
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Segment {
        Segment { name: name.into(), content: content.into() }
    }

    /// Tokens, **estimated**.
    ///
    /// Named for what it is. Four characters per token is a rough average for
    /// English and code; the provider's reported usage is the truth, and this
    /// is only for deciding what to send before there is a report to read.
    pub fn estimated_tokens(&self) -> i64 {
        estimate_tokens(&self.content)
    }
}

/// See [`Segment::estimated_tokens`] — an estimate, never presented as a count.
pub fn estimate_tokens(text: &str) -> i64 {
    (text.chars().count() as i64).div_euclid(4) + 1
}

/// A prompt in two halves (`M-12`).
///
/// `stable` is the region a cache can match: system rules, the binding, tool
/// schemas, then slowly-changing state. `volatile` is the task at hand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prompt {
    stable: Vec<Segment>,
    volatile: Vec<Segment>,
}

impl Prompt {
    pub fn new() -> Prompt {
        Prompt::default()
    }

    /// Append to the stable region. Order is significant and preserved.
    pub fn stable(mut self, segment: Segment) -> Prompt {
        self.stable.push(segment);
        self
    }

    /// Append to the volatile tail — the only part allowed to move.
    pub fn volatile(mut self, segment: Segment) -> Prompt {
        self.volatile.push(segment);
        self
    }

    pub fn stable_segments(&self) -> &[Segment] {
        &self.stable
    }

    pub fn volatile_segments(&self) -> &[Segment] {
        &self.volatile
    }

    /// Identifies the stable region exactly — contents **and** order.
    ///
    /// Two prompts with the same segments in a different order have different
    /// fingerprints, because to a prefix cache they are different prompts.
    ///
    /// Names are deliberately **not** hashed. A cache matches bytes, not
    /// labels, so renaming a segment changes nothing it can see — and a guard
    /// that fired on a rename would be crying wolf about a cache hit that
    /// happened perfectly well.
    pub fn prefix_fingerprint(&self) -> u64 {
        let mut joined = String::new();
        for segment in &self.stable {
            joined.push_str(&segment.content);
            joined.push('\u{2}');
        }
        content_hash(joined.as_bytes())
    }

    pub fn estimated_tokens(&self) -> i64 {
        self.stable.iter().chain(&self.volatile).map(Segment::estimated_tokens).sum()
    }

    pub fn stable_tokens(&self) -> i64 {
        self.stable.iter().map(Segment::estimated_tokens).sum()
    }

    /// The text to send: stable first, in order, then the tail.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for segment in self.stable.iter().chain(&self.volatile) {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&segment.content);
        }
        out
    }
}

/// Holds a batch's stable prefix still (`M-12`).
///
/// The first prompt of a batch sets the fingerprint; any later prompt whose
/// stable region differs is a defect, and one that would otherwise be invisible
/// — everything works, and every call quietly pays cache-miss rates.
#[derive(Debug, Clone, Default)]
pub struct PrefixGuard {
    expected: Option<(String, u64)>,
}

impl PrefixGuard {
    pub fn new() -> PrefixGuard {
        PrefixGuard::default()
    }

    /// Start a new batch. The next prompt sets the shape.
    pub fn start(&mut self, batch: impl Into<String>) {
        self.expected = Some((batch.into(), 0));
    }

    pub fn check(&mut self, prompt: &Prompt) -> Result<()> {
        let fingerprint = prompt.prefix_fingerprint();
        match &mut self.expected {
            None => {
                self.expected = Some((String::new(), fingerprint));
                Ok(())
            }
            Some((batch, expected)) if *expected == 0 => {
                *expected = fingerprint;
                let _ = batch;
                Ok(())
            }
            Some((batch, expected)) if *expected == fingerprint => {
                let _ = batch;
                Ok(())
            }
            Some((batch, _)) => Err(Error::unbound(
                "prompt",
                format!(
                    "the stable prefix changed inside batch `{batch}`. Every call from here \
                     pays cache-miss rates, and nothing else would have told you. Segments now: [{}]",
                    prompt
                        .stable_segments()
                        .iter()
                        .map(|segment| segment.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }
}

/// What compaction removed (`M-13`).
///
/// Journalled, so what was dropped is recoverable — the point is not to save
/// tokens quietly but to save them accountably.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compaction {
    pub summary: String,
    pub dropped: Vec<String>,
    pub tokens_before: i64,
    pub tokens_after: i64,
}

impl Compaction {
    pub fn saved(&self) -> i64 {
        self.tokens_before - self.tokens_after
    }

    pub fn evidence(&self) -> String {
        format!(
            "compaction: {} → {} estimated tokens (saved {})\ndropped: {}\nsummary:\n{}",
            self.tokens_before,
            self.tokens_after,
            self.saved(),
            if self.dropped.is_empty() { "nothing".to_string() } else { self.dropped.join(", ") },
            self.summary
        )
    }
}

/// Summarises what is about to be dropped. Implemented by the `compactor` role
/// against a local link; a fake in tests.
pub trait Summariser {
    fn summarise(&self, dropped: &[Segment]) -> Result<String>;
}

/// Compact a prompt down to a token budget (`M-13`).
///
/// Only the volatile tail is compacted, oldest first: the stable prefix is the
/// thing a cache matches, and dropping part of it to save tokens would cost
/// more than it saves.
///
/// Refuses to run on a cloud link. Compaction reads everything the loop has
/// seen — it is the highest-volume, most context-rich call in the whole system,
/// and `M-13` puts it on local hardware for that reason.
pub fn compact(
    prompt: &Prompt,
    budget_tokens: i64,
    link: &Link,
    summariser: &dyn Summariser,
) -> Result<(Prompt, Option<Compaction>)> {
    if link.privacy != Privacy::Local {
        return Err(Error::unbound(
            format!("link.{}", link.name),
            "compaction runs on a local link (`M-13`) — it reads everything the loop has seen",
        ));
    }

    let before = prompt.estimated_tokens();
    if before <= budget_tokens {
        return Ok((prompt.clone(), None));
    }

    let floor = prompt.stable_tokens();
    if floor >= budget_tokens {
        return Err(Error::unbound(
            "prompt",
            format!(
                "the stable prefix alone is {floor} estimated tokens, over a budget of \
                 {budget_tokens}. Compaction cannot help; the prefix is too big for this link."
            ),
        ));
    }

    // Drop from the oldest end of the tail until it fits, keeping the most
    // recent turn whatever happens — a summary of everything including the
    // thing just said is not a prompt.
    let mut kept: Vec<Segment> = prompt.volatile_segments().to_vec();
    let mut dropped: Vec<Segment> = Vec::new();
    while kept.len() > 1
        && floor + kept.iter().map(Segment::estimated_tokens).sum::<i64>() > budget_tokens
    {
        dropped.push(kept.remove(0));
    }

    if dropped.is_empty() {
        return Ok((prompt.clone(), None));
    }

    let summary = summariser.summarise(&dropped)?;
    let mut compacted = Prompt::new();
    for segment in prompt.stable_segments() {
        compacted = compacted.stable(segment.clone());
    }
    compacted = compacted.volatile(Segment::new("compacted", summary.clone()));
    for segment in kept {
        compacted = compacted.volatile(segment);
    }

    let compaction = Compaction {
        summary,
        dropped: dropped.into_iter().map(|segment| segment.name).collect(),
        tokens_before: before,
        tokens_after: compacted.estimated_tokens(),
    };
    Ok((compacted, Some(compaction)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::Links;

    struct Fake(&'static str);

    impl Summariser for Fake {
        fn summarise(&self, dropped: &[Segment]) -> Result<String> {
            Ok(format!("{} (from {} segments)", self.0, dropped.len()))
        }
    }

    struct Broken;

    impl Summariser for Broken {
        fn summarise(&self, _dropped: &[Segment]) -> Result<String> {
            Err(Error::unbound("compactor", "the local link is down"))
        }
    }

    fn links() -> Links {
        Links::parse(
            "```perp-links\n\
             link.here.kind = lmstudio\n\
             link.here.base_url = http://localhost:1234\n\
             link.here.model = small\n\
             link.cloud.kind = deepseek\n\
             link.cloud.base_url = https://api.deepseek.com\n\
             link.cloud.model = deepseek-v4-flash\n\
             role.compactor = here\n```\n",
        )
        .expect("parse")
    }

    fn long(name: &str, times: usize) -> Segment {
        Segment::new(name, "x".repeat(times))
    }

    fn prompt() -> Prompt {
        Prompt::new()
            .stable(Segment::new("rules", "the system rules"))
            .stable(Segment::new("binding", "the binding"))
            .volatile(Segment::new("task", "do the thing"))
    }

    #[test]
    fn the_stable_region_comes_first_and_keeps_its_order() {
        let rendered = prompt().render();
        let rules = rendered.find("system rules").expect("rules");
        let binding = rendered.find("the binding").expect("binding");
        let task = rendered.find("do the thing").expect("task");
        assert!(rules < binding && binding < task, "{rendered}");
    }

    #[test]
    fn reordering_the_stable_region_changes_the_fingerprint() {
        // The whole point of `M-12`: to a prefix cache these are different
        // prompts, and nothing else in the system would notice.
        let forwards = prompt();
        let backwards = Prompt::new()
            .stable(Segment::new("binding", "the binding"))
            .stable(Segment::new("rules", "the system rules"))
            .volatile(Segment::new("task", "do the thing"));
        assert_ne!(forwards.prefix_fingerprint(), backwards.prefix_fingerprint());
    }

    #[test]
    fn renaming_a_segment_does_not_change_the_fingerprint() {
        // A cache matches bytes, not labels. The red run found this: hashing
        // the names would fire the guard on a rename whose cache hit was
        // perfect, which is the worst kind of alarm — the one you learn to
        // ignore.
        let named = Prompt::new().stable(Segment::new("rules", "identical bytes"));
        let renamed = Prompt::new().stable(Segment::new("system-rules", "identical bytes"));
        assert_eq!(named.prefix_fingerprint(), renamed.prefix_fingerprint());
    }

    #[test]
    fn a_segment_boundary_still_counts() {
        // One segment of "ab" and two of "a","b" send different bytes.
        let one = Prompt::new().stable(Segment::new("x", "ab"));
        let two = Prompt::new().stable(Segment::new("x", "a")).stable(Segment::new("y", "b"));
        assert_ne!(one.prefix_fingerprint(), two.prefix_fingerprint());
    }

    #[test]
    fn changing_the_tail_does_not_change_the_fingerprint() {
        let first = prompt();
        let second = Prompt::new()
            .stable(Segment::new("rules", "the system rules"))
            .stable(Segment::new("binding", "the binding"))
            .volatile(Segment::new("task", "do a completely different thing"));
        assert_eq!(
            first.prefix_fingerprint(),
            second.prefix_fingerprint(),
            "only the tail moved, which is what the tail is for"
        );
    }

    #[test]
    fn the_guard_catches_a_prefix_that_moved_mid_batch() {
        let mut guard = PrefixGuard::new();
        guard.start("b9");
        guard.check(&prompt()).expect("the first call sets the shape");
        guard.check(&prompt()).expect("the same shape again");

        let changed = prompt().stable(Segment::new("extra", "a tool schema someone added"));
        let err = guard.check(&changed).expect_err("must complain");
        let text = format!("{err}");
        assert!(text.contains("stable prefix changed inside batch `b9`"), "{text}");
        assert!(text.contains("cache-miss rates"), "it says why it matters: {text}");
        assert!(text.contains("extra"), "and names the segments: {text}");
    }

    #[test]
    fn a_prompt_inside_its_budget_is_left_alone() {
        let links = links();
        let (result, compaction) =
            compact(&prompt(), 10_000, links.get("here").expect("here"), &Fake("unused"))
                .expect("compact");
        assert_eq!(result, prompt());
        assert!(compaction.is_none(), "nothing to do, so nothing was done");
    }

    #[test]
    fn compaction_drops_the_oldest_tail_and_records_what_went() {
        let links = links();
        let big = Prompt::new()
            .stable(Segment::new("rules", "short"))
            .volatile(long("turn-1", 4000))
            .volatile(long("turn-2", 4000))
            .volatile(long("turn-3", 4000));

        let (result, compaction) =
            compact(&big, 1500, links.get("here").expect("here"), &Fake("they discussed the parser"))
                .expect("compact");
        let compaction = compaction.expect("something had to go");

        assert_eq!(compaction.dropped, vec!["turn-1", "turn-2"]);
        assert!(compaction.saved() > 0);
        assert!(result.estimated_tokens() < big.estimated_tokens());

        // The newest turn survives — a summary of everything including the
        // thing just said is not a prompt.
        assert_eq!(result.volatile_segments().last().expect("last").name, "turn-3");
        assert_eq!(result.volatile_segments()[0].name, "compacted");

        let evidence = compaction.evidence();
        assert!(evidence.contains("turn-1, turn-2"), "{evidence}");
        assert!(evidence.contains("they discussed the parser"), "recoverable: {evidence}");
    }

    #[test]
    fn the_stable_prefix_is_never_compacted() {
        let links = links();
        let big = Prompt::new()
            .stable(Segment::new("rules", "short"))
            .volatile(long("turn-1", 4000))
            .volatile(long("turn-2", 4000));

        let (result, _) =
            compact(&big, 1500, links.get("here").expect("here"), &Fake("summary")).expect("compact");
        assert_eq!(
            result.prefix_fingerprint(),
            big.prefix_fingerprint(),
            "compacting the prefix would cost more cache than it saves tokens"
        );
    }

    #[test]
    fn a_stable_prefix_bigger_than_the_budget_is_an_error_not_a_silent_truncation() {
        let links = links();
        let huge = Prompt::new().stable(long("rules", 40_000)).volatile(Segment::new("task", "go"));
        let err = compact(&huge, 1000, links.get("here").expect("here"), &Fake("x"))
            .expect_err("must refuse");
        assert!(format!("{err}").contains("too big for this link"), "{err}");
    }

    #[test]
    fn compaction_refuses_to_run_on_a_cloud_link() {
        // `M-13`: it is the highest-volume, most context-rich call in the
        // system, which is exactly why it stays on the operator's hardware.
        let links = links();
        let big = Prompt::new().volatile(long("a", 8000)).volatile(long("b", 8000));
        let err = compact(&big, 100, links.get("cloud").expect("cloud"), &Fake("x"))
            .expect_err("must refuse");
        assert!(format!("{err}").contains("runs on a local link"), "{err}");
    }

    #[test]
    fn a_summariser_that_fails_fails_the_compaction() {
        // Not a silent pass-through: an over-budget prompt that was not
        // compacted would be sent and rejected by the link.
        let links = links();
        let big = Prompt::new().volatile(long("a", 8000)).volatile(long("b", 8000));
        assert!(compact(&big, 100, links.get("here").expect("here"), &Broken).is_err());
    }

    #[test]
    fn token_counts_are_estimates_and_say_so_in_the_name() {
        assert_eq!(estimate_tokens(""), 1);
        assert!(estimate_tokens(&"x".repeat(400)) > estimate_tokens(&"x".repeat(40)));
        assert_eq!(Segment::new("n", "abcd").estimated_tokens(), 2);
    }
}
