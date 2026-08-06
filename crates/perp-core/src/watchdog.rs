//! Watchdogs (`L-11`, `L-12`, `L-13`).
//!
//! A loop that survives a crash is only half the problem. The other half is a
//! loop that keeps running while achieving nothing — editing the same file back
//! and forth, re-running the same failing command, or thinking hard and
//! producing no change. Each of these has a cheap mechanical signature, and
//! catching them here costs nothing compared to the tokens they burn.
//!
//! Every watchdog answers the same question — keep going, or stop and say why.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// FNV-1a, so a content hash written into the journal today still means the
/// same thing after a restart, a rebuild, or a Rust upgrade. `DefaultHasher`
/// is explicitly not guaranteed to be stable across releases.
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    Continue,
    /// Stop the feature, carrying the reason a human will read.
    Stop { reason: String },
}

impl Watch {
    pub fn is_stop(&self) -> bool {
        matches!(self, Watch::Stop { .. })
    }
}

/// Steps that changed nothing (`L-11`).
///
/// "Nothing" means no workspace change *and* no gate-state change — a step that
/// only read files is fine once, and is a loop the fifth time.
#[derive(Debug, Clone)]
pub struct NoProgress {
    limit: u32,
    quiet: u32,
}

impl Default for NoProgress {
    fn default() -> NoProgress {
        NoProgress::new(5)
    }
}

impl NoProgress {
    pub fn new(limit: u32) -> NoProgress {
        NoProgress { limit, quiet: 0 }
    }

    pub fn quiet_steps(&self) -> u32 {
        self.quiet
    }

    pub fn observe(&mut self, changed_something: bool) -> Watch {
        if changed_something {
            self.quiet = 0;
            return Watch::Continue;
        }
        self.quiet += 1;
        if self.quiet >= self.limit {
            Watch::Stop {
                reason: format!(
                    "{} consecutive steps changed nothing in the workspace and moved no gate",
                    self.quiet
                ),
            }
        } else {
            Watch::Continue
        }
    }
}

/// The same call, with the same arguments, over and over (`L-12`).
///
/// A retry is a decision; the same call three times in a row is a stuck loop.
/// The window keeps this from firing on a legitimately repeated call much later
/// in a long batch.
#[derive(Debug, Clone)]
pub struct Repetition {
    limit: usize,
    window: usize,
    /// The call, and how much had been written when it was made (`L-31`).
    recent: VecDeque<(String, usize)>,
}

impl Default for Repetition {
    fn default() -> Repetition {
        Repetition::new(3, 10)
    }
}

impl Repetition {
    pub fn new(limit: usize, window: usize) -> Repetition {
        Repetition { limit, window, recent: VecDeque::new() }
    }

    /// `signature` identifies the call *and* its arguments — two different
    /// greps are not a repetition, and the same grep twice is.
    ///
    /// `written` is how many files the step has changed so far (`L-31`). Two
    /// identical calls with a write between them are not a repetition: the
    /// state they are asking about is different, and asking again is the
    /// correct thing to do. Only repeats made against unchanged state count.
    pub fn observe_after(&mut self, signature: &str, written: usize) -> Watch {
        self.recent.push_back((signature.to_string(), written));
        while self.recent.len() > self.window {
            self.recent.pop_front();
        }
        let count = self
            .recent
            .iter()
            .filter(|(seen, at)| seen == signature && *at == written)
            .count();
        if count >= self.limit {
            Watch::Stop {
                reason: format!(
                    "`{signature}` ran {count} times in the last {} calls — that is a stuck loop, not a retry",
                    self.recent.len()
                ),
            }
        } else {
            Watch::Continue
        }
    }
}

/// A file edited back to something it already was (`L-13`).
///
/// Two agents disagreeing across steps, or one agent undoing itself, both look
/// like this: content returning to a hash the batch has already seen. Once is
/// noted, twice stops the feature.
#[derive(Debug, Clone, Default)]
pub struct Thrash {
    seen: HashMap<PathBuf, Vec<u64>>,
    reverts: HashMap<PathBuf, u32>,
}

impl Thrash {
    pub fn new() -> Thrash {
        Thrash::default()
    }

    pub fn reverts(&self, path: &Path) -> u32 {
        self.reverts.get(path).copied().unwrap_or(0)
    }

    pub fn observe(&mut self, path: &Path, contents: &[u8]) -> Watch {
        let hash = content_hash(contents);
        let history = self.seen.entry(path.to_path_buf()).or_default();

        // The current content repeating is not a revert; the file simply was
        // not touched. Only a return to an *older* state counts.
        let returned = history.len() > 1 && history[..history.len() - 1].contains(&hash);
        if history.last() != Some(&hash) {
            history.push(hash);
        }

        if !returned {
            return Watch::Continue;
        }

        let count = self.reverts.entry(path.to_path_buf()).or_insert(0);
        *count += 1;
        if *count >= 2 {
            Watch::Stop {
                reason: format!(
                    "{} has been edited back to an earlier state {} times — the batch is thrashing",
                    path.display(),
                    count
                ),
            }
        } else {
            Watch::Continue
        }
    }
}

/// All three, watching one feature.
#[derive(Debug, Clone, Default)]
pub struct Watchdogs {
    pub no_progress: NoProgress,
    pub repetition: Repetition,
    pub thrash: Thrash,
}

impl Watchdogs {
    pub fn new() -> Watchdogs {
        Watchdogs::default()
    }

    pub fn step_finished(&mut self, changed_something: bool) -> Watch {
        self.no_progress.observe(changed_something)
    }

    pub fn call(&mut self, signature: &str, written: usize) -> Watch {
        self.repetition.observe_after(signature, written)
    }

    pub fn file_written(&mut self, path: &Path, contents: &[u8]) -> Watch {
        self.thrash.observe(path, contents)
    }
}

#[cfg(test)]
mod tests {

    /// `L-31`: re-running a check after an edit is not a stuck loop.
    ///
    /// Measured on Janitor's cycle 19 — the first cycle in nineteen where the
    /// loop wrote real code. It produced `guard.rs` at 407 lines, wired
    /// refusals through `plan.rs`, took the suite from 84 tests to 107, and
    /// passed lint and build. Then `L-12` killed the step for running
    /// `cargo test --workspace` three times in ten calls, and because the step
    /// failed the batch never committed: 800 lines of green, gated work left
    /// uncommitted in the tree.
    ///
    /// Re-running a verification command after changing something is what
    /// working looks like. What makes a repeat a loop is that nothing moved
    /// between the repeats.
    #[test]
    fn a_check_repeated_after_a_write_is_progress_not_a_loop() {
        let mut spinning = Repetition::default();
        assert_eq!(spinning.observe_after("cargo test", 0), Watch::Continue);
        assert_eq!(spinning.observe_after("cargo test", 0), Watch::Continue);
        assert!(
            matches!(spinning.observe_after("cargo test", 0), Watch::Stop { .. }),
            "same call, nothing written between: that is the loop the rule is for"
        );

        let mut working = Repetition::default();
        assert_eq!(working.observe_after("cargo test", 0), Watch::Continue);
        assert_eq!(working.observe_after("cargo test", 1), Watch::Continue);
        assert_eq!(
            working.observe_after("cargo test", 2),
            Watch::Continue,
            "a write between each run makes every run a question about new state"
        );
    }
    use super::*;

    #[test]
    fn the_hash_is_fnv_1a_and_stays_that_way() {
        // Published FNV-1a 64 vectors. If these ever change, a thrash record
        // written before the change stops meaning anything.
        assert_eq!(content_hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(content_hash(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(content_hash(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn quiet_steps_stop_the_feature() {
        let mut watchdog = NoProgress::new(3);
        assert_eq!(watchdog.observe(false), Watch::Continue);
        assert_eq!(watchdog.observe(false), Watch::Continue);
        let verdict = watchdog.observe(false);
        assert!(verdict.is_stop(), "three quiet steps must stop it");
        match verdict {
            Watch::Stop { reason } => assert!(reason.contains("changed nothing"), "{reason}"),
            Watch::Continue => unreachable!(),
        }
    }

    #[test]
    fn any_progress_resets_the_count() {
        let mut watchdog = NoProgress::new(3);
        watchdog.observe(false);
        watchdog.observe(false);
        assert_eq!(watchdog.observe(true), Watch::Continue);
        assert_eq!(watchdog.quiet_steps(), 0);
        assert_eq!(watchdog.observe(false), Watch::Continue, "the count restarted");
    }

    #[test]
    fn the_same_call_three_times_is_a_stuck_loop() {
        let mut watchdog = Repetition::new(3, 10);
        assert_eq!(watchdog.observe_after("grep(fn main)", 0), Watch::Continue);
        assert_eq!(watchdog.observe_after("grep(fn main)", 0), Watch::Continue);
        assert!(watchdog.observe_after("grep(fn main)", 0).is_stop());
    }

    #[test]
    fn different_arguments_are_different_calls() {
        let mut watchdog = Repetition::new(3, 10);
        assert_eq!(watchdog.observe_after("grep(a)", 0), Watch::Continue);
        assert_eq!(watchdog.observe_after("grep(b)", 0), Watch::Continue);
        assert_eq!(watchdog.observe_after("grep(c)", 0), Watch::Continue);
        assert_eq!(watchdog.observe_after("grep(a)", 0), Watch::Continue);
    }

    #[test]
    fn the_window_forgets_old_calls() {
        // limit 2 in a window of 3: without trimming, the two `cargo build`s
        // below would be a stop. They are far enough apart that they are not.
        let mut watchdog = Repetition::new(2, 3);
        assert_eq!(watchdog.observe_after("cargo build", 0), Watch::Continue);
        watchdog.observe_after("x", 0);
        watchdog.observe_after("y", 0);
        watchdog.observe_after("z", 0);
        assert_eq!(
            watchdog.observe_after("cargo build", 0),
            Watch::Continue,
            "the first `cargo build` has fallen out of the window"
        );
        // Two inside the window is still a stop.
        assert!(watchdog.observe_after("cargo build", 0).is_stop());
    }

    #[test]
    fn a_file_edited_back_to_an_earlier_state_is_thrash() {
        let mut watchdog = Thrash::new();
        let path = Path::new("src/main.rs");
        assert_eq!(watchdog.observe(path, b"version one"), Watch::Continue);
        assert_eq!(watchdog.observe(path, b"version two"), Watch::Continue);
        // Back to one — the first revert. Flagged, not fatal.
        assert_eq!(watchdog.observe(path, b"version one"), Watch::Continue);
        assert_eq!(watchdog.reverts(path), 1);
        // Back to two, which the file has also already been. A-B-A-B is a
        // fight between steps, and the second revert is where it stops.
        assert!(watchdog.observe(path, b"version two").is_stop());
        assert_eq!(watchdog.reverts(path), 2);
    }

    #[test]
    fn writing_the_same_content_twice_is_not_thrash() {
        // Re-writing a file with what it already contains is a no-op, not a
        // fight with another step.
        let mut watchdog = Thrash::new();
        let path = Path::new("src/lib.rs");
        for _ in 0..5 {
            assert_eq!(watchdog.observe(path, b"unchanged"), Watch::Continue);
        }
        assert_eq!(watchdog.reverts(path), 0);
    }

    #[test]
    fn thrash_is_tracked_per_file() {
        let mut watchdog = Thrash::new();
        let a = Path::new("a.rs");
        let b = Path::new("b.rs");
        watchdog.observe(a, b"one");
        watchdog.observe(a, b"two");
        watchdog.observe(a, b"one");
        watchdog.observe(b, b"one");
        watchdog.observe(b, b"two");
        assert_eq!(watchdog.reverts(a), 1);
        assert_eq!(watchdog.reverts(b), 0, "one file's history is not another's");
    }

    #[test]
    fn the_three_watch_independently() {
        let mut dogs = Watchdogs::new();
        assert_eq!(dogs.call("ls", 0), Watch::Continue);
        assert_eq!(dogs.file_written(Path::new("x"), b"a"), Watch::Continue);
        assert_eq!(dogs.step_finished(true), Watch::Continue);
        assert_eq!(dogs.no_progress.quiet_steps(), 0);
    }
}
