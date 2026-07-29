//! One writer at a time (`L-20`), serialised gate runs (`L-18`), and one
//! feature in flight (`L-17`).
//!
//! All three are the same problem wearing different hats: two things editing
//! one workspace produce a result neither of them intended, and the evidence
//! afterwards cannot say which did what.
//!
//! The lock is a file, created with `create_new` so the creation is the
//! acquisition — there is no check-then-create window for a second process to
//! slip through. It is readable on purpose: an operator finding a stuck loop
//! should be able to `cat` the thing and see which process, which step, and
//! since when.
//!
//! ## Staleness
//!
//! A crashed process leaves its lock behind. Asking the OS whether a pid is
//! alive needs platform calls this crate does not have (`N-11`), so instead the
//! holder writes a heartbeat and a lock that stops beating for longer than its
//! TTL can be broken. Breaking one is **never silent**: it returns the previous
//! holder so the caller journals what it took over and from whom. A lock that
//! could be broken quietly would be a lock that reports success while two
//! processes write.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::atomic::write_atomic;
use crate::error::{Error, Result};
use crate::step::StepId;

/// How long a lock may go without a heartbeat before it is considered
/// abandoned. Generous on purpose: a gate that takes four minutes is normal,
/// and stealing a live lock is far worse than waiting out a dead one.
pub const DEFAULT_TTL_SECONDS: i64 = 300;

/// What the lock is protecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The workspace, for anything that writes (`L-20`).
    Write,
    /// A build target directory (`L-18`).
    Gate,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Write => "write",
            Kind::Gate => "gate",
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Kind::Write => "write.lock",
            Kind::Gate => "gate.lock",
        }
    }
}

/// Who holds a lock, as it appears on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub owner: String,
    pub kind: String,
    pub step: String,
    pub since: i64,
    pub beat: i64,
}

impl Holder {
    fn render(&self) -> String {
        format!(
            "owner={}\nkind={}\nstep={}\nsince={}\nbeat={}\n",
            self.owner, self.kind, self.step, self.since, self.beat
        )
    }

    fn parse(text: &str) -> Option<Holder> {
        let mut owner = None;
        let mut kind = None;
        let mut step = None;
        let mut since = None;
        let mut beat = None;
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match key {
                "owner" => owner = Some(value.to_string()),
                "kind" => kind = Some(value.to_string()),
                "step" => step = Some(value.to_string()),
                "since" => since = value.parse().ok(),
                "beat" => beat = value.parse().ok(),
                _ => {}
            }
        }
        Some(Holder {
            owner: owner?,
            kind: kind.unwrap_or_default(),
            step: step.unwrap_or_default(),
            since: since?,
            beat: beat?,
        })
    }

    pub fn describe(&self) -> String {
        format!("{} at step {} since {}", self.owner, self.step, crate::time::format_utc(self.since))
    }
}

/// A held lock. Released on drop, because every early return in the engine is a
/// path that would otherwise leak one.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    holder: Holder,
    /// Set when this lock was taken over from a holder that stopped beating.
    broke: Option<Holder>,
    released: bool,
}

impl Lock {
    /// Take the lock, or say who has it.
    ///
    /// `owner` should identify the process (`Lock::this_process` builds one).
    /// `now` is passed in rather than read so the staleness rule is testable
    /// without sleeping through a TTL.
    pub fn acquire(
        path: &Path,
        kind: Kind,
        owner: &str,
        step: &StepId,
        now: i64,
        ttl: i64,
    ) -> Result<Lock> {
        let holder = Holder {
            owner: owner.to_string(),
            kind: kind.as_str().to_string(),
            step: step.to_string(),
            since: now,
            beat: now,
        };

        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }

        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => {
                file.write_all(holder.render().as_bytes()).map_err(|e| Error::io(path, e))?;
                file.sync_all().map_err(|e| Error::io(path, e))?;
                Ok(Lock { path: path.to_path_buf(), holder, broke: None, released: false })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let text = fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
                let existing = Holder::parse(&text);

                // An unreadable lock is not a free lock. Someone wrote it, and
                // guessing it is junk is how two processes end up writing.
                let Some(existing) = existing else {
                    return Err(Error::refused(
                        path.display().to_string(),
                        "a lock file is there but unreadable; delete it by hand if you are sure",
                    ));
                };

                if now - existing.beat < ttl {
                    return Err(Error::refused(
                        format!("{} lock", kind.as_str()),
                        format!("held by {}", existing.describe()),
                    ));
                }

                write_atomic(path, &holder.render())?;
                Ok(Lock {
                    path: path.to_path_buf(),
                    holder,
                    broke: Some(existing),
                    released: false,
                })
            }
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// The write lock for a workspace (`L-20`).
    pub fn write_lock(dir: &Path, owner: &str, step: &StepId, now: i64) -> Result<Lock> {
        Lock::acquire(
            &dir.join(Kind::Write.file_name()),
            Kind::Write,
            owner,
            step,
            now,
            DEFAULT_TTL_SECONDS,
        )
    }

    /// The gate lock for a build target (`L-18`).
    ///
    /// Keyed on the **target directory**, not the repository: two worktrees
    /// sharing one `CARGO_TARGET_DIR` collide and must serialise, and two
    /// worktrees with their own targets do not and must not. Locking the repo
    /// would get the second case wrong and serialise work that was safe.
    pub fn gate_lock(target_dir: &Path, owner: &str, step: &StepId, now: i64) -> Result<Lock> {
        Lock::acquire(
            &target_dir.join(Kind::Gate.file_name()),
            Kind::Gate,
            owner,
            step,
            now,
            DEFAULT_TTL_SECONDS,
        )
    }

    /// An owner string for this process. Includes the pid so a human can find
    /// it, and the start time so a recycled pid is not mistaken for the
    /// original.
    pub fn this_process(started: i64) -> String {
        format!("perp-{}-{started}", std::process::id())
    }

    pub fn holder(&self) -> &Holder {
        &self.holder
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The holder this lock was taken from, if it was taken. `None` on the
    /// ordinary path.
    pub fn broke(&self) -> Option<&Holder> {
        self.broke.as_ref()
    }

    /// Keep the lock alive. Call between steps; a step that outlives the TTL
    /// without one is indistinguishable from a crash, and should be.
    pub fn beat(&mut self, now: i64) -> Result<()> {
        self.holder.beat = now;
        write_atomic(&self.path, &self.holder.render())
    }

    /// Move the lock onto the step now running, so a stuck loop names the right
    /// one.
    pub fn at_step(&mut self, step: &StepId, now: i64) -> Result<()> {
        self.holder.step = step.to_string();
        self.beat(now)
    }

    pub fn release(mut self) -> Result<()> {
        self.released = true;
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::io(&self.path, e)),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if !self.released {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Whether chat may write while the loop holds the write lock (`L-20`, `C-3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMode {
    /// Messages are accepted and queued; nothing they say takes effect yet.
    ReadOnly,
    /// The loop is paused or idle and chat has the workspace.
    Interactive,
}

/// Chat writes only when the loop is not writing. Pausing is the handover, and
/// there is deliberately no third state where both write "carefully".
pub fn chat_mode(loop_holds_write_lock: bool, paused: bool) -> ChatMode {
    if loop_holds_write_lock && !paused {
        ChatMode::ReadOnly
    } else {
        ChatMode::Interactive
    }
}

/// How many features may be in flight (`L-17`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Concurrency {
    /// The default. One feature, one branch, one working tree.
    #[default]
    Serial,
    /// Opt-in, and only with a worktree per feature (`G-11`).
    Parallel { max: u32 },
}

impl Concurrency {
    /// May another feature start?
    ///
    /// `worktrees_available` is not a preference — parallel batches sharing one
    /// working tree produce interleaved edits and a diff nobody can attribute,
    /// so the answer without them is no rather than "slower".
    pub fn admit(&self, in_flight: u32, worktrees_available: bool) -> Result<()> {
        match self {
            Concurrency::Serial => {
                if in_flight == 0 {
                    Ok(())
                } else {
                    Err(Error::refused(
                        "second feature",
                        format!("{in_flight} already in flight and the loop is serial (`L-17`)"),
                    ))
                }
            }
            Concurrency::Parallel { max } => {
                if !worktrees_available {
                    return Err(Error::refused(
                        "parallel batches",
                        "need a git worktree per feature (`G-11`), which is not built yet",
                    ));
                }
                if in_flight < *max {
                    Ok(())
                } else {
                    Err(Error::refused(
                        "another feature",
                        format!("{in_flight} in flight is the configured maximum"),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    const T: i64 = 1_700_000_000;

    fn step() -> StepId {
        StepId::new(3, "b12", 4).expect("step")
    }

    #[test]
    fn the_second_writer_is_told_who_has_it() {
        let dir = tmpdir("lock-write");
        let first = Lock::write_lock(&dir, "perp-1", &step(), T).expect("first");
        assert!(first.broke().is_none());

        let err = Lock::write_lock(&dir, "perp-2", &step(), T + 5).expect_err("only one writer");
        let text = format!("{err}");
        assert!(text.contains("perp-1"), "names the holder: {text}");
        assert!(text.contains("c3/b12/s04"), "and the step it is on: {text}");
    }

    #[test]
    fn releasing_lets_the_next_one_in() {
        let dir = tmpdir("lock-release");
        let lock = Lock::write_lock(&dir, "perp-1", &step(), T).expect("first");
        lock.release().expect("release");
        let second = Lock::write_lock(&dir, "perp-2", &step(), T + 1).expect("second");
        assert!(second.broke().is_none(), "an orderly handover breaks nothing");
    }

    #[test]
    fn dropping_releases_so_an_early_return_does_not_wedge_the_workspace() {
        let dir = tmpdir("lock-drop");
        {
            let _held = Lock::write_lock(&dir, "perp-1", &step(), T).expect("first");
        }
        Lock::write_lock(&dir, "perp-2", &step(), T + 1).expect("the lock was released on drop");
    }

    #[test]
    fn a_lock_that_stopped_beating_can_be_taken_and_says_so() {
        let dir = tmpdir("lock-stale");
        let dead = Lock::write_lock(&dir, "perp-dead", &step(), T).expect("first");
        std::mem::forget(dead); // a crash: no release, no drop.

        let fresh = Lock::write_lock(&dir, "perp-live", &step(), T + DEFAULT_TTL_SECONDS + 1)
            .expect("a lock that stopped beating is takeable");
        let broke = fresh.broke().expect("taking one over is never silent");
        assert_eq!(broke.owner, "perp-dead", "and names who it was taken from");
    }

    #[test]
    fn a_beating_lock_is_never_taken() {
        let dir = tmpdir("lock-beat");
        let mut held = Lock::write_lock(&dir, "perp-1", &step(), T).expect("first");
        held.beat(T + DEFAULT_TTL_SECONDS).expect("beat");
        Lock::write_lock(&dir, "perp-2", &step(), T + DEFAULT_TTL_SECONDS + 1)
            .expect_err("still alive");
        std::mem::forget(held);
    }

    #[test]
    fn an_unreadable_lock_is_not_a_free_lock() {
        let dir = tmpdir("lock-junk");
        let path = dir.join("write.lock");
        fs::write(&path, "garbage from something else").expect("write");
        let err = Lock::acquire(&path, Kind::Write, "perp-1", &step(), T, DEFAULT_TTL_SECONDS)
            .expect_err("must refuse rather than assume");
        assert!(format!("{err}").contains("by hand"), "{err}");
    }

    #[test]
    fn gates_serialise_on_the_target_not_the_repository() {
        let repo = tmpdir("lock-gate");
        let shared = repo.join("target");
        let own = repo.join("worktree-b/target");

        let _first = Lock::gate_lock(&shared, "perp-1", &step(), T).expect("first");
        Lock::gate_lock(&shared, "perp-2", &step(), T)
            .expect_err("two builds in one target is a false red (`L-18`)");
        Lock::gate_lock(&own, "perp-2", &step(), T).expect("a separate target does not collide");
    }

    #[test]
    fn chat_writes_only_when_the_loop_is_not() {
        assert_eq!(chat_mode(true, false), ChatMode::ReadOnly);
        assert_eq!(chat_mode(true, true), ChatMode::Interactive, "pausing is the handover");
        assert_eq!(chat_mode(false, false), ChatMode::Interactive);
    }

    #[test]
    fn serial_is_the_default_and_admits_one() {
        let c = Concurrency::default();
        assert_eq!(c, Concurrency::Serial);
        c.admit(0, false).expect("the first feature");
        assert!(c.admit(1, true).is_err(), "worktrees do not make a serial loop parallel");
    }

    #[test]
    fn parallel_without_worktrees_is_refused_by_name() {
        let c = Concurrency::Parallel { max: 3 };
        let err = c.admit(0, false).expect_err("must refuse");
        assert!(format!("{err}").contains("G-11"), "cites what is missing: {err}");
        c.admit(2, true).expect("under the maximum");
        assert!(c.admit(3, true).is_err(), "at the maximum");
    }

    #[test]
    fn the_lock_file_says_who_what_and_since_when() {
        let dir = tmpdir("lock-readable");
        let lock = Lock::write_lock(&dir, "perp-7", &step(), T).expect("lock");
        let text = fs::read_to_string(lock.path()).expect("read");
        for expected in ["owner=perp-7", "kind=write", "step=c3/b12/s04", "since=1700000000"] {
            assert!(text.contains(expected), "missing {expected} in:\n{text}");
        }
    }
}
