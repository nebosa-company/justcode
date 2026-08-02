//! The git harness (`G-1`–`G-14`).
//!
//! Git is not a tool the loop happens to call. It is the loop's memory of what
//! it actually did, and the only thing that can undo it. So the rules live
//! here, in code, rather than in a prompt:
//!
//! - `git add -A` and `commit -a` are **refused** — an unattended loop must
//!   never sweep up a change it did not make (`G-3`).
//! - `--no-verify` is **refused**; a hook failure is a gate failure (`G-4`).
//! - Force-push and history rewriting are **refused** (`G-10`).
//! - Anything that throws away uncommitted work is **refused** — `reset
//!   --hard`, `clean`, `restore`, `checkout -- <path>`, a forced switch, and
//!   emptying the stash the others send it to (`G-10`).
//! - Push, tag and merge need an explicit approval argument, which the CLI
//!   cannot supply on its own (`G-5`).
//!
//! ## The line is recoverability
//!
//! `G-3` was written about staging and read as being about staging, so the
//! refusals grew one loud spelling at a time — `reset --hard` and `clean` were
//! listed, and `restore` was not. A cycle then ran `git restore -- lib/…` and
//! uncommitted work stopped existing, with nothing in the way.
//!
//! A commit is recoverable, a stash is recoverable, and a working-tree change
//! is the one thing git holds no copy of. That is the test a new subcommand has
//! to be put to, rather than whether it resembles one already on the list.
//!
//! Every one of those is enforced by [`classify`] before the process is
//! spawned, so there is no path where a command runs and the policy is
//! consulted afterwards.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::process::{self, Env, Exit, Run, Spec};

/// Long enough for a commit hook, short enough to notice a hang.
pub const GIT_TIMEOUT_SECS: u64 = 120;

/// The branch the loop works on for a batch (`G-1`).
pub fn batch_branch(cycle: u32, batch: u32) -> String {
    format!("perp/c{cycle}/b{batch}")
}

/// The branch phases A–C write on.
pub fn init_branch(cycle: u32) -> String {
    format!("perp/c{cycle}/init")
}

/// Branches the loop must never commit to directly (`G-1`).
pub const PROTECTED: &[&str] = &["main", "master", "trunk", "develop"];

pub fn is_protected(branch: &str) -> bool {
    PROTECTED.contains(&branch)
}

/// What the harness may do with a git command, decided before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Policy {
    /// Local, reversible, and the loop's own business.
    Auto,
    /// Leaves the machine or rewrites shared state — a human says yes, per
    /// action, per cycle (`G-5`).
    Approve { reason: String },
    /// Not available at any approval level. The loop cannot argue past these.
    Never { reason: String },
}

impl Policy {
    pub fn is_never(&self) -> bool {
        matches!(self, Policy::Never { .. })
    }

    pub fn needs_approval(&self) -> bool {
        matches!(self, Policy::Approve { .. })
    }
}

/// Whether a human said yes to *this* action. Not a mode, not a setting — a
/// value that has to be passed in at the call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    NotGranted,
    Granted { by: String },
}

/// Classify a git invocation (`G-3`, `G-4`, `G-5`, `G-10`).
pub fn classify(args: &[&str]) -> Policy {
    let never = |reason: &str| Policy::Never { reason: reason.to_string() };
    let approve = |reason: &str| Policy::Approve { reason: reason.to_string() };

    let subcommand = args.first().copied().unwrap_or("");
    let rest = &args[1.min(args.len())..];
    let has = |flag: &str| rest.contains(&flag);

    // Short flags cluster: `-fdx` is three flags, and `commit -am` is the
    // sweep-up-everything case wearing two letters. Anything that only looks
    // for `-a` misses it.
    let short = |letter: char| {
        rest.iter().any(|arg| {
            arg.starts_with('-') && !arg.starts_with("--") && arg[1..].contains(letter)
        })
    };

    if has("--no-verify") || (subcommand == "commit" && short('n')) {
        return never("hooks are never bypassed — a hook failure is a gate failure (`G-4`)");
    }

    match subcommand {
        "add" if short('A') || has("--all") || has(".") || has(":/") => {
            never("staging is explicit — the loop stages the files its step touched (`G-3`)")
        }
        "commit" if short('a') || has("--all") => {
            never("`commit -a` sweeps up unrelated changes (`G-3`)")
        }
        "push" if short('f') || has("--force") || has("--force-with-lease") => {
            never("force-push rewrites what others have (`G-10`)")
        }
        "push" => approve("pushing puts work on a machine that is not this one (`G-5`)"),
        "tag" if rest.is_empty() || rest.iter().all(|a| *a == "-l" || *a == "--list") => {
            Policy::Auto // listing tags changes nothing
        }
        "tag" => approve("a tag is a release marker (`G-5`)"),
        "merge" => approve("merging changes an integration branch (`G-5`)"),
        "rebase" if has("--onto") || short('i') || has("--interactive") => {
            never("rewriting history is not available to the loop (`G-10`)")
        }
        "filter-branch" | "filter-repo" => never("history rewriting (`G-10`)"),
        "reset" if has("--hard") => {
            never("`reset --hard` discards work; stash first, then reset (`G-10`)")
        }
        // Every form of `clean` deletes untracked work except a dry run. There
        // is no combination of letters worth allow-listing.
        "clean" if !(short('n') || has("--dry-run")) => {
            never("`git clean` deletes untracked work (`G-10`)")
        }
        // `restore` is `reset --hard` for named paths, and was reached first:
        // a cycle ran `git restore -- lib/…` and uncommitted work stopped
        // existing, with nothing refusing it because only the loud spellings
        // were listed. The line is recoverability, not the subcommand.
        //
        // `--staged` alone only unstages — the content stays in the working
        // tree and git still has it — so that form is left alone. Adding
        // `--worktree` to it is the destructive one again.
        // Spelled out rather than as a negation: `-S` is `--staged` and `-W` is
        // `--worktree`, and a condition that checks only the long forms reads
        // as correct while letting the short ones through.
        "restore"
            if has("--worktree")
                || short('W')
                || !(has("--staged") || short('S')) =>
        {
            never("`git restore` discards uncommitted work; commit or stash it first (`G-10`)")
        }
        // The same act in the older spelling. `checkout -- <path>` throws away
        // the working tree copy; `checkout <branch>` does not, and git refuses
        // it by itself when it would clobber something.
        "checkout" if has("--") => {
            never("`checkout -- <path>` discards uncommitted work; use `git restore --staged` \
                   to unstage, or commit first (`G-10`)")
        }
        "checkout" | "switch" if short('f') || has("--force") || has("--discard-changes") => {
            never("forcing a switch discards uncommitted work (`G-10`)")
        }
        // A stash is where the other refusals send the loop. Emptying it is
        // where that work stops being recoverable.
        "stash" if rest.first().is_some_and(|a| *a == "drop" || *a == "clear") => {
            never("dropping a stash discards the work it was holding (`G-10`)")
        }
        "update-ref" if short('d') => never("deleting a ref by hand (`G-10`)"),
        _ => Policy::Auto,
    }
}

/// What a repository looks like before a batch starts (`G-14`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    pub branch: Option<String>,
    pub clean: bool,
    pub detached: bool,
    pub mid_operation: Option<String>,
    pub complaints: Vec<String>,
}

impl Readiness {
    pub fn is_ready(&self) -> bool {
        self.complaints.is_empty()
    }

    pub fn describe(&self) -> String {
        if self.is_ready() {
            format!(
                "on {}, clean",
                self.branch.clone().unwrap_or_else(|| "an unknown branch".into())
            )
        } else {
            self.complaints.join("; ")
        }
    }
}

#[derive(Debug, Clone)]
pub struct Repo {
    root: PathBuf,
    timeout: Duration,
}

impl Repo {
    pub fn at(root: impl Into<PathBuf>) -> Repo {
        Repo { root: root.into(), timeout: Duration::from_secs(GIT_TIMEOUT_SECS) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run git, with the policy checked first.
    fn run_inner(&self, args: &[&str], approval: &Approval, keep_all: bool) -> Result<Run> {
        match classify(args) {
            Policy::Never { reason } => {
                return Err(Error::unbound(format!("git {}", args.join(" ")), reason))
            }
            Policy::Approve { reason } => {
                if *approval == Approval::NotGranted {
                    return Err(Error::unbound(
                        format!("git {}", args.join(" ")),
                        format!("needs approval: {reason}"),
                    ));
                }
            }
            Policy::Auto => {}
        }
        if keep_all {
            return self.run_unchecked_keeping_all(args);
        }
        self.run_unchecked(args)
    }

    fn run(&self, args: &[&str], approval: &Approval) -> Result<Run> {
        self.run_inner(args, approval, false)
    }

    /// Undo everything since `sha` by **reverting**, not resetting (`O-4`,
    /// `G-8`).
    ///
    /// `G-10` classifies `reset --hard` as `Never`, and `O-4` asks for the
    /// workspace to go back to a step's commit. Those look like a conflict and
    /// are not: `G-8` already says a feature is reverted cleanly, and a revert
    /// **destroys nothing**. New commits undo the old ones, git history keeps
    /// both, and the journal — which is append-only anyway (`L-3`) — ends up
    /// agreeing with the repository instead of contradicting it.
    ///
    /// Resetting would have made the two disagree in the one direction this
    /// whole design exists to prevent: a record of work that the tree no longer
    /// shows, with nothing saying it was withdrawn.
    pub fn revert_since(&self, sha: &str, by: &str) -> Result<Run> {
        if sha.trim().is_empty() {
            return Err(Error::refused("revert", "needs the commit to revert back to"));
        }
        let range = format!("{sha}..HEAD");
        let message = format!("Revert to {sha} (rewind approved by {by})");
        self.run(&["revert", "--no-edit", "--no-merges", &range], &Approval::Granted {
            by: by.to_string(),
        })
        .and_then(|run| {
            if run.is_success() {
                Ok(run)
            } else {
                Err(Error::refused(
                    "revert",
                    format!("{message} did not apply cleanly: {}", run.stderr_tail.trim()),
                ))
            }
        })
    }

    /// Read-only plumbing whose output is wanted as text. Classified like
    /// everything else — these are all `Auto`, and going through `run` keeps it
    /// that way if one ever stops being.
    ///
    /// **This can hand back a tail, and it says so when it does** (`T-20`).
    /// `T-6` caps each stream at [`process::TAIL_LINES`], which is right for a
    /// transcript and wrong for output being read as data. This returned the
    /// tail silently: `V-15`'s first wiring parsed a forty-line slice of a
    /// long diff, found no claim in it, and reported the change clean. A
    /// caller that needs every byte wants [`Repo::plumbing_all`]; a caller that
    /// only displays this now has a line telling it what it is looking at.
    pub fn plumbing(&self, args: &[&str]) -> Result<String> {
        let run = self.run(args, &Approval::NotGranted)?;
        if run.stdout_truncated {
            return Ok(format!(
                "{}
[only the last {} lines — the rest was not read (`T-20`)]",
                run.stdout_tail,
                process::TAIL_LINES
            ));
        }
        Ok(run.stdout_tail.clone())
    }

    /// The same, whole (`T-20`).
    ///
    /// For output that is data rather than a transcript — a diff to parse, a
    /// file list to walk — where a shorter answer is not a smaller answer but
    /// a wrong one.
    pub fn plumbing_all(&self, args: &[&str]) -> Result<String> {
        Ok(self.run_inner(args, &Approval::NotGranted, true)?.stdout_tail)
    }

    /// One config value, or empty when it is not set. Not an error: "unset" is
    /// the ordinary answer for most keys.
    pub fn config(&self, key: &str) -> Result<String> {
        let run = self.run_unchecked(&["config", "--get", key])?;
        Ok(run.stdout_tail.trim().to_string())
    }

    /// Run without the classifier. `pub(crate)` so a test fixture elsewhere in
    /// the crate can build a repository; every caller outside `git` goes
    /// through [`Repo::run`], which classifies first.
    pub(crate) fn run_unchecked(&self, args: &[&str]) -> Result<Run> {
        let quoted: Vec<String> = args
            .iter()
            .map(|arg| if arg.contains(' ') { format!("\"{arg}\"") } else { (*arg).to_string() })
            .collect();
        let spec = Spec::new(format!("git {}", quoted.join(" ")), &self.root, self.timeout)
            .with_env(Env::declared());
        process::run(&spec)
    }

    /// As [`Repo::run_unchecked`], keeping both streams whole (`T-20`).
    fn run_unchecked_keeping_all(&self, args: &[&str]) -> Result<Run> {
        let quoted: Vec<String> = args
            .iter()
            .map(|arg| if arg.contains(' ') { format!("\"{arg}\"") } else { (*arg).to_string() })
            .collect();
        let spec = Spec::new(format!("git {}", quoted.join(" ")), &self.root, self.timeout)
            .with_env(Env::declared())
            .keeping_all();
        process::run(&spec)
    }

    fn first_line(&self, args: &[&str]) -> Result<String> {
        let run = self.run_unchecked(args)?;
        if !run.is_success() {
            return Err(Error::unbound(
                format!("git {}", args.join(" ")),
                format!("{}: {}", run.exit.describe(), run.stderr_tail.trim()),
            ));
        }
        Ok(run.stdout_tail.lines().next().unwrap_or_default().trim().to_string())
    }

    /// The commit a gate ran against (`G-6`).
    pub fn head_sha(&self) -> Result<String> {
        self.first_line(&["rev-parse", "HEAD"])
    }

    pub fn short_sha(&self) -> Result<String> {
        self.first_line(&["rev-parse", "--short", "HEAD"])
    }

    /// `None` when HEAD is detached.
    ///
    /// `branch --show-current` rather than `rev-parse --abbrev-ref HEAD`: the
    /// latter cannot answer before the first commit, and "the repository is
    /// too new to have a branch" is not the same as "detached".
    pub fn current_branch(&self) -> Result<Option<String>> {
        let name = self.first_line(&["branch", "--show-current"])?;
        Ok(if name.is_empty() { None } else { Some(name) })
    }

    pub fn status_porcelain(&self) -> Result<String> {
        let run = self.run_unchecked(&["status", "--porcelain"])?;
        Ok(run.stdout_tail)
    }

    pub fn is_clean(&self) -> Result<bool> {
        Ok(self.status_porcelain()?.trim().is_empty())
    }

    /// Assert the repository is in a state worth committing into (`G-14`).
    ///
    /// A surprising state parks the batch rather than being committed into —
    /// a loop that starts work on top of somebody's half-finished rebase
    /// produces a mess nobody can untangle afterwards.
    pub fn readiness(&self, expected_branch: Option<&str>) -> Result<Readiness> {
        let branch = self.current_branch()?;
        let clean = self.is_clean()?;
        let detached = branch.is_none();

        let git_dir = self.root.join(".git");
        let mid_operation = [
            ("rebase-merge", "a rebase is in progress"),
            ("rebase-apply", "a rebase or am is in progress"),
            ("MERGE_HEAD", "a merge is in progress"),
            ("CHERRY_PICK_HEAD", "a cherry-pick is in progress"),
            ("BISECT_LOG", "a bisect is in progress"),
        ]
        .iter()
        .find(|(marker, _)| git_dir.join(marker).exists())
        .map(|(_, message)| (*message).to_string());

        let mut complaints = Vec::new();
        if detached {
            complaints.push("HEAD is detached".to_string());
        }
        if !clean {
            complaints.push("the working tree has changes that are not this batch's".to_string());
        }
        if let Some(operation) = &mid_operation {
            complaints.push(operation.clone());
        }
        if let (Some(expected), Some(actual)) = (expected_branch, branch.as_deref()) {
            if expected != actual {
                complaints.push(format!("on `{actual}`, expected `{expected}`"));
            }
        }
        if let Some(actual) = branch.as_deref() {
            if is_protected(actual) {
                complaints.push(format!("`{actual}` is protected — the loop branches first (`G-1`)"));
            }
        }

        Ok(Readiness { branch, clean, detached, mid_operation, complaints })
    }

    /// Stage exactly these paths (`G-3`).
    pub fn stage(&self, paths: &[&str]) -> Result<()> {
        if paths.is_empty() {
            return Err(Error::unbound("git add", "no paths — staging is always explicit"));
        }
        let mut args = vec!["add", "--"];
        args.extend_from_slice(paths);
        let run = self.run(&args, &Approval::NotGranted)?;
        if !run.is_success() {
            return Err(Error::unbound(
                "git add",
                format!("{}: {}", run.exit.describe(), run.stderr_tail.trim()),
            ));
        }
        Ok(())
    }

    /// Commit what is staged. Hooks run; `--no-verify` is not reachable from
    /// here, because [`classify`] refuses it (`G-4`).
    pub fn commit(&self, message: &CommitMessage) -> Result<String> {
        // `G-1`: refused here, not merely reported by `readiness`. A check that
        // only warns is a check the loop can walk past.
        if let Some(branch) = self.current_branch()? {
            if is_protected(&branch) {
                return Err(Error::unbound(
                    "git commit",
                    format!("`{branch}` is protected — the loop branches first (`G-1`)"),
                ));
            }
        }
        let text = message.render();
        let run = self.run(&["commit", "-m", &text], &Approval::NotGranted)?;
        if !run.is_success() {
            return Err(Error::unbound(
                "git commit",
                format!(
                    "{}: {} {}",
                    run.exit.describe(),
                    run.stdout_tail.trim(),
                    run.stderr_tail.trim()
                ),
            ));
        }
        self.head_sha()
    }

    /// Has this ever existed in the history? (`G-7`)
    ///
    /// `git log -S` finds commits that added or removed the string, which
    /// answers "was this built and then removed" — a question grep on the
    /// working tree cannot answer, and the reason `V-1` looks here too.
    pub fn history_mentions(&self, needle: &str) -> Result<Vec<String>> {
        let run = self.run_unchecked(&["log", "-S", needle, "--oneline", "--all"])?;
        if !run.is_success() {
            return Ok(Vec::new());
        }
        Ok(run.stdout_tail.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// Does the working tree contain it right now?
    pub fn tree_mentions(&self, needle: &str) -> Result<Vec<String>> {
        let run = self.run_unchecked(&["grep", "-l", "--", needle])?;
        // Exit 1 from `git grep` means "no matches", which is an answer.
        Ok(run.stdout_tail.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// The commits that carry a step's trailer (`G-8`).
    ///
    /// A feature's commits are contiguous and findable, which is what makes a
    /// single feature revertible without touching the rest of the batch.
    pub fn commits_for_step(&self, step: &str) -> Result<Vec<String>> {
        let needle = format!("Perpetum-Step: {step}");
        let run = self.run_unchecked(&["log", "--grep", &needle, "--format=%H", "--all"])?;
        if !run.is_success() {
            return Ok(Vec::new());
        }
        Ok(run.stdout_tail.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// Stash the working tree, run something, and put it back (`V-3`).
    ///
    /// The stash ref is returned to the caller even on success, because
    /// `G-10` says a destructive operation journals what it displaced — and if
    /// the pop fails, the error names the ref rather than leaving the operator
    /// to find it.
    pub fn with_stashed<T>(&self, label: &str, body: impl FnOnce() -> T) -> Result<(T, String)> {
        let message = format!("perp {label}");
        let push = self.run(&["stash", "push", "--include-untracked", "-m", &message], &Approval::NotGranted)?;
        if !push.is_success() {
            return Err(Error::unbound(
                "git stash push",
                format!("{}: {}", push.exit.describe(), push.stderr_tail.trim()),
            ));
        }
        let stashed = !push.stdout_tail.contains("No local changes");
        let stash_ref = if stashed {
            self.first_line(&["rev-parse", "stash@{0}"]).unwrap_or_else(|_| "unknown".to_string())
        } else {
            "nothing to stash".to_string()
        };

        let outcome = body();

        if stashed {
            let pop = self.run(&["stash", "pop"], &Approval::NotGranted)?;
            if !pop.is_success() {
                return Err(Error::unbound(
                    "git stash pop",
                    format!(
                        "the work is safe in {stash_ref} — restore it by hand: {}",
                        pop.stderr_tail.trim()
                    ),
                ));
            }
        }
        Ok((outcome, stash_ref))
    }

    pub fn create_branch(&self, name: &str) -> Result<()> {
        let run = self.run(&["switch", "-c", name], &Approval::NotGranted)?;
        if !run.is_success() && !matches!(run.exit, Exit::Code(0)) {
            return Err(Error::unbound(
                format!("git switch -c {name}"),
                run.stderr_tail.trim().to_string(),
            ));
        }
        Ok(())
    }
}

/// A commit message with the trailers that make a line of code traceable back
/// to a requirement and to whatever wrote it (`G-2`).
#[derive(Debug, Clone, Default)]
pub struct CommitMessage {
    pub subject: String,
    pub body: String,
    pub requirements: Vec<String>,
    pub step: Option<String>,
    pub authored_by: Option<String>,
}

impl CommitMessage {
    pub fn new(subject: impl Into<String>) -> CommitMessage {
        CommitMessage { subject: subject.into(), ..CommitMessage::default() }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> CommitMessage {
        self.body = body.into();
        self
    }

    pub fn for_requirements<I, S>(mut self, ids: I) -> CommitMessage
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.requirements = ids.into_iter().map(Into::into).collect();
        self
    }

    pub fn at_step(mut self, step: impl Into<String>) -> CommitMessage {
        self.step = Some(step.into());
        self
    }

    /// Which link and model wrote it (`M-10`), or which human.
    pub fn authored_by(mut self, who: impl Into<String>) -> CommitMessage {
        self.authored_by = Some(who.into());
        self
    }

    pub fn render(&self) -> String {
        let mut out = self.subject.trim().to_string();
        if !self.body.trim().is_empty() {
            out.push_str("\n\n");
            out.push_str(self.body.trim());
        }
        let mut trailers = Vec::new();
        if !self.requirements.is_empty() {
            trailers.push(format!("Requirement: {}", self.requirements.join(", ")));
        }
        if let Some(step) = &self.step {
            trailers.push(format!("Perpetum-Step: {step}"));
        }
        if let Some(who) = &self.authored_by {
            trailers.push(format!("Co-Authored-By: {who}"));
        }
        if !trailers.is_empty() {
            out.push_str("\n\n");
            out.push_str(&trailers.join("\n"));
        }
        out.push('\n');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    fn repo(tag: &str) -> Repo {
        let root = tmpdir(tag);
        let repo = Repo::at(&root);
        for args in [
            // `perp` is not a protected name, so the fixture's first commit is
            // allowed. A fixture on `main` could not commit at all — which is
            // the rule working.
            vec!["init", "-q", "-b", "perp/fixture"],
            vec!["config", "user.email", "loop@perpetum.test"],
            vec!["config", "user.name", "Perpetum test"],
            // Hermetic: the suite must not need the operator's signing key.
            // This is the fixture's own config, not a project setting.
            vec!["config", "commit.gpgsign", "false"],
        ] {
            let run = repo.run_unchecked(&args).expect("git");
            assert!(run.is_success(), "git {args:?} failed: {}", run.stderr_tail);
        }
        std::fs::write(root.join("first.txt"), "one\n").expect("write");
        repo.stage(&["first.txt"]).expect("stage");
        repo.commit(&CommitMessage::new("Add the first file")).expect("commit");
        repo
    }

    /// `T-20`: a caller that needs the whole output can get it, and one handed
    /// a tail is told so.
    ///
    /// The failure this exists for: `V-15`'s first wiring read a diff through
    /// `plumbing`, got the last forty lines of it, found no requirement claimed
    /// in that slice, and reported the change clean. Nothing anywhere said the
    /// diff had been cut.
    #[test]
    fn a_long_output_is_whole_when_asked_for_and_labelled_when_not() {
        let repo = repo("git-t20");
        // Comfortably past the forty-line cap, and every line distinct so the
        // first one is proof the whole thing came back.
        let long: String = (0..300).map(|n| format!("line {n}
")).collect();
        std::fs::write(repo.root().join("long.txt"), &long).expect("write");
        repo.stage(&["long.txt"]).expect("stage");

        let whole = repo.plumbing_all(&["diff", "--cached"]).expect("whole");
        assert!(whole.contains("line 0"), "the head of the diff is missing");
        assert!(whole.contains("line 299"), "the tail of the diff is missing");
        assert!(!whole.contains("T-20"), "a whole answer needs no apology: {}", &whole[..80]);

        let tail = repo.plumbing(&["diff", "--cached"]).expect("tail");
        assert!(!tail.contains("line 0"), "this should have been cut");
        assert!(tail.contains("line 299"), "the tail keeps the end");
        // The part that was actually missing before: it says what it is.
        assert!(tail.contains("only the last"), "a tail that does not say so: {tail:.200}");
        assert!(tail.contains("T-20"), "and names the rule: {tail:.200}");
    }

    /// A short output is not labelled, or the label means nothing.
    #[test]
    fn a_short_output_is_returned_untouched() {
        let repo = repo("git-t20-short");
        std::fs::write(repo.root().join("short.txt"), "one line
").expect("write");
        repo.stage(&["short.txt"]).expect("stage");

        let out = repo.plumbing(&["diff", "--cached", "--stat"]).expect("stat");
        assert!(!out.contains("only the last"), "nothing was cut: {out}");
        assert_eq!(out, repo.plumbing_all(&["diff", "--cached", "--stat"]).expect("all"));
    }

    #[test]
    fn branch_names_follow_the_layout() {
        assert_eq!(batch_branch(1, 4), "perp/c1/b4");
        assert_eq!(init_branch(12), "perp/c12/init");
        assert!(is_protected("main"));
        assert!(!is_protected("perp/c1/b4"));
    }

    #[test]
    fn the_refusals_are_refusals() {
        for args in [
            vec!["add", "-A"],
            vec!["add", "."],
            vec!["commit", "-a", "-m", "everything"],
            // Combined short flags — the form people actually type.
            vec!["commit", "-am", "everything"],
            vec!["commit", "--no-verify", "-m", "skip the hooks"],
            vec!["commit", "-n", "-m", "skip the hooks"],
            vec!["push", "--force"],
            vec!["push", "--force-with-lease"],
            vec!["push", "-uf", "origin", "main"],
            vec!["reset", "--hard", "HEAD~1"],
            vec!["rebase", "-i", "HEAD~3"],
            vec!["filter-branch", "--all"],
            vec!["clean", "-fdx"],
            vec!["clean", "-fd"],
            vec!["clean", "-f"],
            // The one a real cycle actually ran, verbatim. It was `Auto`, and
            // the uncommitted work it named stopped existing.
            vec!["restore", "--", "lib/object_spec.dart", "lib/object_spec_form_screen.dart"],
            vec!["restore", "lib/main.dart"],
            vec!["restore", "--worktree", "lib/main.dart"],
            // `--staged` alone is fine; adding the working tree back is not.
            vec!["restore", "--staged", "--worktree", "lib/main.dart"],
            vec!["restore", "-SW", "lib/main.dart"],
            // The same act, older spelling.
            vec!["checkout", "--", "lib/main.dart"],
            vec!["checkout", "HEAD", "--", "lib/main.dart"],
            vec!["checkout", "-f", "main"],
            vec!["checkout", "--force", "main"],
            vec!["switch", "--discard-changes", "main"],
            vec!["switch", "-f", "main"],
            // Where the other refusals send the loop, so emptying it ends the
            // recoverability the advice depends on.
            vec!["stash", "drop"],
            vec!["stash", "clear"],
        ] {
            assert!(
                classify(&args).is_never(),
                "git {args:?} must be refused outright"
            );
        }
    }

    /// The refusals have to stop at destruction. A loop that cannot unstage,
    /// switch branches or read its own history is a loop that cannot work, and
    /// the way a rule like this fails in practice is by growing until it does.
    #[test]
    fn what_git_still_holds_a_copy_of_is_not_refused() {
        for args in [
            // Unstaging only: the content stays in the working tree.
            vec!["restore", "--staged", "lib/main.dart"],
            vec!["restore", "-S", "lib/main.dart"],
            // Moving between branches. Git refuses this itself when it would
            // clobber something, which is the check this would be duplicating.
            vec!["checkout", "main"],
            vec!["checkout", "-b", "perp/c1/b2"],
            vec!["switch", "main"],
            // Stashing is the advice the refusals give; taking it back is too.
            vec!["stash"],
            vec!["stash", "push", "-m", "before the risky bit"],
            vec!["stash", "pop"],
            vec!["stash", "list"],
            // Reading changes nothing.
            vec!["status", "--porcelain"],
            vec!["diff", "--cached"],
            vec!["log", "-1"],
            vec!["clean", "--dry-run"],
        ] {
            assert!(
                !classify(&args).is_never(),
                "git {args:?} destroys nothing and must not be refused"
            );
        }
    }

    #[test]
    fn leaving_the_machine_needs_an_approval() {
        for args in [vec!["push"], vec!["push", "origin", "main"], vec!["tag", "v0.1.0"], vec!["merge", "perp/c1/b1"]] {
            assert!(
                classify(&args).needs_approval(),
                "git {args:?} must ask before it runs"
            );
        }
    }

    #[test]
    fn ordinary_work_needs_no_ceremony() {
        for args in [
            vec!["status", "--porcelain"],
            vec!["add", "--", "src/main.rs"],
            vec!["commit", "-m", "a message"],
            vec!["switch", "-c", "perp/c1/b5"],
            vec!["log", "--oneline", "-5"],
            vec!["stash"],
            vec!["tag"],
        ] {
            assert_eq!(classify(&args), Policy::Auto, "git {args:?} should just run");
        }
    }

    #[test]
    fn a_refused_command_never_reaches_git() {
        let repo = repo("git-refuse");
        let err = repo.run(&["add", "-A"], &Approval::NotGranted).expect_err("must refuse");
        assert!(format!("{err}").contains("staging is explicit"), "{err}");

        // And approval does not unlock it — `Never` is not `Approve`.
        let err = repo
            .run(&["add", "-A"], &Approval::Granted { by: "operator".into() })
            .expect_err("must still refuse");
        assert!(format!("{err}").contains("staging is explicit"), "{err}");
    }

    #[test]
    fn an_unapproved_push_is_refused_and_an_approved_one_is_attempted() {
        let repo = repo("git-push");
        let err = repo.run(&["push"], &Approval::NotGranted).expect_err("must refuse");
        assert!(format!("{err}").contains("needs approval"), "{err}");

        // With approval it runs — and fails on its own terms, because this
        // fixture has no remote. That is the point: the policy stopped being
        // the thing in the way.
        let run = repo
            .run(&["push"], &Approval::Granted { by: "operator".into() })
            .expect("the command should have been attempted");
        assert!(!run.is_success(), "no remote configured, so git itself refuses");
    }

    #[test]
    fn a_commit_carries_its_trailers() {
        let message = CommitMessage::new("Add the journal writer")
            .with_body("Two lines of why.")
            .for_requirements(["L-3", "N-8"])
            .at_step("c1/b1/s07")
            .authored_by("Claude Opus 5 <noreply@anthropic.com>");
        let rendered = message.render();

        assert!(rendered.starts_with("Add the journal writer\n\nTwo lines of why."));
        assert!(rendered.contains("Requirement: L-3, N-8"));
        assert!(rendered.contains("Perpetum-Step: c1/b1/s07"));
        assert!(rendered.contains("Co-Authored-By: Claude Opus 5"));
    }

    #[test]
    fn a_commit_is_traceable_in_the_repository_afterwards() {
        // `G-2`: a line of code traces back to a requirement and to whatever
        // wrote it — which is only true if the trailers survive the commit.
        let repo = repo("git-trailers");
        std::fs::write(repo.root().join("second.txt"), "two\n").expect("write");
        repo.stage(&["second.txt"]).expect("stage");
        let sha = repo
            .commit(
                &CommitMessage::new("Add the second file")
                    .for_requirements(["G-2"])
                    .at_step("c1/b4/s03"),
            )
            .expect("commit");

        assert_eq!(sha.len(), 40, "a full sha: {sha}");
        let log = repo.run_unchecked(&["log", "-1", "--format=%B"]).expect("log");
        assert!(log.stdout_tail.contains("Requirement: G-2"), "{}", log.stdout_tail);
        assert!(log.stdout_tail.contains("Perpetum-Step: c1/b4/s03"), "{}", log.stdout_tail);
    }

    #[test]
    fn staging_needs_paths() {
        let repo = repo("git-stage-empty");
        assert!(repo.stage(&[]).is_err(), "there is no `stage everything`");
    }

    #[test]
    fn a_clean_repository_on_the_expected_branch_is_ready() {
        let repo = repo("git-ready");
        repo.create_branch("perp/c1/b9").expect("branch");
        let readiness = repo.readiness(Some("perp/c1/b9")).expect("readiness");
        assert!(readiness.is_ready(), "{}", readiness.describe());
        assert_eq!(readiness.branch.as_deref(), Some("perp/c1/b9"));
    }

    #[test]
    fn readiness_complains_about_a_dirty_tree_and_the_wrong_branch() {
        // `G-14`: a surprising state parks the batch rather than being
        // committed into.
        let repo = repo("git-not-ready");
        std::fs::write(repo.root().join("stray.txt"), "not this batch's\n").expect("write");

        let readiness = repo.readiness(Some("perp/c1/b9")).expect("readiness");
        assert!(!readiness.is_ready());
        let text = readiness.describe();
        assert!(text.contains("not this batch's") || text.contains("working tree"), "{text}");
        assert!(text.contains("expected `perp/c1/b9`"), "{text}");
    }

    #[test]
    fn readiness_refuses_a_protected_branch() {
        let repo = repo("git-protected");
        repo.create_branch("main").expect("branch");
        let readiness = repo.readiness(None).expect("readiness");
        assert!(!readiness.is_ready(), "main is protected — the loop branches first");
        assert!(readiness.describe().contains("protected"), "{}", readiness.describe());
    }

    #[test]
    fn committing_to_a_protected_branch_is_refused_not_just_reported() {
        // `G-1`: a check that only warns is a check the loop can walk past.
        let repo = repo("git-protected-commit");
        repo.create_branch("main").expect("branch");
        std::fs::write(repo.root().join("sneaky.txt"), "straight onto main\n").expect("write");
        repo.stage(&["sneaky.txt"]).expect("stage");

        let err = repo
            .commit(&CommitMessage::new("Commit onto main"))
            .expect_err("must refuse");
        assert!(format!("{err}").contains("protected"), "{err}");

        // On a batch branch the same commit is fine.
        repo.create_branch("perp/c1/b4").expect("branch");
        repo.commit(&CommitMessage::new("Commit onto the batch branch")).expect("commit");
    }

    #[test]
    fn head_sha_is_what_a_gate_would_be_pinned_to() {
        // `G-6`: a green gate at a sha that no longer exists is not evidence.
        let repo = repo("git-sha");
        let sha = repo.head_sha().expect("sha");
        let short = repo.short_sha().expect("short sha");
        assert_eq!(sha.len(), 40, "{sha}");
        assert!(sha.starts_with(&short), "{short} should prefix {sha}");
    }
}
