//! The parts of a repository the loop has to know about before it starts
//! (`G-9`, `G-11`, `G-12`, `T-11`).
//!
//! Everything here answers a question the harness would otherwise discover
//! halfway through a batch, when the answer is expensive:
//!
//! - Is there a submodule, LFS, or an in-repo hook this loop will silently get
//!   wrong? (`G-12`)
//! - When a merge conflicts, how hard does it try before parking? (`G-9`)
//! - Where does parallel work live? (`G-11`)
//! - What does a blocked feature leave behind? (`T-11`)

use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::git::Repo;
use crate::journal::Record;
use crate::step::StepId;

/// Something in the repository that changes what "committed" means (`G-12`).
///
/// Detected at binding time and **declared loudly**, supported or not. A loop
/// that silently skips a submodule ships half a change and passes its own
/// gates doing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feature {
    Submodules { count: usize },
    Lfs,
    /// Hooks committed into the repository (`core.hooksPath` pointing inside
    /// it), which run on every commit the loop makes.
    InRepoHooks { path: String },
}

impl Feature {
    pub fn is_supported(&self) -> bool {
        match self {
            // Hooks are supported because the harness already refuses
            // `--no-verify` (`G-4`) — they run, and that is the intended
            // behaviour rather than a gap.
            Feature::InRepoHooks { .. } => true,
            Feature::Submodules { .. } | Feature::Lfs => false,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Feature::Submodules { count } => format!(
                "{count} submodule(s): NOT SUPPORTED. The loop stages explicit paths and does \
                 not update submodule pointers, so a change spanning one would be committed \
                 half-applied (`G-12`)"
            ),
            Feature::Lfs => "git-lfs: NOT SUPPORTED. The loop would commit pointer files as \
                 though they were content (`G-12`)"
                .to_string(),
            Feature::InRepoHooks { path } => format!(
                "in-repo hooks at {path}: supported and they will run — `--no-verify` is \
                 refused (`G-4`)"
            ),
        }
    }
}

/// What was found in a repository (`G-12`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Survey {
    pub features: Vec<Feature>,
}

impl Survey {
    /// Look, at binding time. Cheap: three file checks and one config read.
    pub fn of(root: &Path) -> Survey {
        let mut features = Vec::new();

        if let Ok(text) = std::fs::read_to_string(root.join(".gitmodules")) {
            let count = text.lines().filter(|line| line.trim_start().starts_with("[submodule")).count();
            if count > 0 {
                features.push(Feature::Submodules { count });
            }
        }
        if root.join(".gitattributes").exists() {
            if let Ok(text) = std::fs::read_to_string(root.join(".gitattributes")) {
                if text.contains("filter=lfs") {
                    features.push(Feature::Lfs);
                }
            }
        }
        // A hooks path inside the repository is one the loop's own commits run.
        let repo = Repo::at(root);
        if let Ok(run) = repo.config("core.hooksPath") {
            let path = run.trim().to_string();
            if !path.is_empty() && !Path::new(&path).is_absolute() {
                features.push(Feature::InRepoHooks { path });
            }
        }
        Survey { features }
    }

    pub fn unsupported(&self) -> Vec<&Feature> {
        self.features.iter().filter(|feature| !feature.is_supported()).collect()
    }

    /// Whether the loop may run here unattended. An unsupported feature is not
    /// a crash — it is a decision for the operator, made before the batch
    /// rather than after it.
    pub fn is_safe_to_run(&self) -> bool {
        self.unsupported().is_empty()
    }

    pub fn render(&self) -> String {
        if self.features.is_empty() {
            return "no submodules, no LFS, no in-repo hooks\n".to_string();
        }
        self.features.iter().map(|feature| format!("- {}\n", feature.describe())).collect()
    }
}

/// How far the loop goes to resolve a merge conflict (`G-9`).
///
/// **One attempt, on non-overlapping hunks only, then park.** The loop never
/// resolves a semantic conflict by picking a side quietly — that is the failure
/// where both branches' tests pass and the merged behaviour is wrong, and it is
/// invisible in every artefact this harness produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Applied cleanly, no conflict.
    Clean,
    /// One automated attempt succeeded: the hunks did not overlap.
    Merged { files: Vec<String> },
    /// Parked, with the conflict text verbatim (Perpetum 0.5).
    Blocked { files: Vec<String>, conflict: String },
}

impl Resolution {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Resolution::Blocked { .. })
    }

    pub fn record(&self, step: StepId, at: i64) -> Record {
        match self {
            Resolution::Clean => Record::outcome(step, at, true, "merged cleanly"),
            Resolution::Merged { files } => Record::outcome(
                step,
                at,
                true,
                format!("merged {} file(s) — non-overlapping hunks", files.len()),
            )
            .with_detail(files.join("\n")),
            Resolution::Blocked { files, conflict } => Record::outcome(
                step,
                at,
                false,
                format!("blocked: {} file(s) conflict", files.len()),
            )
            // Verbatim. A summarised conflict is a conflict nobody can resolve
            // from the journal.
            .with_detail(conflict.clone()),
        }
    }
}

/// Read a merge's outcome. `output` is git's own text.
///
/// The distinction that matters: git already only reports a conflict when the
/// hunks *do* overlap — it merges non-overlapping ones itself. So there is no
/// second attempt to make. Anything git could not do is semantic, and semantic
/// is exactly what this must not touch.
pub fn read_merge(output: &str, exit_ok: bool) -> Resolution {
    if exit_ok && !output.contains("CONFLICT") {
        return Resolution::Clean;
    }
    let files: Vec<String> = output
        .lines()
        .filter_map(|line| {
            line.split_once("Merge conflict in ").map(|(_, path)| path.trim().to_string())
        })
        .collect();
    if files.is_empty() && exit_ok {
        return Resolution::Clean;
    }
    Resolution::Blocked { files, conflict: output.to_string() }
}

/// A per-feature working tree (`G-11`).
///
/// Created when `L-17` parallelism is on, removed on merge or abandon, **never
/// left stale**. A stale worktree is worse than no worktree: it holds a branch
/// checked out, so the branch cannot be deleted, and the next run finds a tree
/// nobody remembers making.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

impl fmt::Display for Worktree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} on {}", self.path.display(), self.branch)
    }
}

/// Lifecycle management for worktrees (`G-11`).
#[derive(Debug)]
pub struct Worktrees<'a> {
    repo: &'a Repo,
    root: PathBuf,
}

impl<'a> Worktrees<'a> {
    /// Worktrees live in a sibling directory, never inside the repository —
    /// a worktree under the repo is a directory the loop's own globs would
    /// walk into and whose files its gates would compile twice.
    pub fn new(repo: &'a Repo, root: &Path) -> Worktrees<'a> {
        Worktrees { repo, root: root.to_path_buf() }
    }

    pub fn dir_for(&self, branch: &str) -> PathBuf {
        self.root.join(branch.replace(['/', '\\'], "-"))
    }

    /// What git says exists right now. Read rather than remembered: a worktree
    /// list kept in the harness is a list that goes stale exactly when it
    /// matters.
    pub fn list(&self) -> Result<Vec<Worktree>> {
        let run = self.repo.plumbing(&["worktree", "list", "--porcelain"])?;
        let mut trees = Vec::new();
        let mut path: Option<PathBuf> = None;
        for line in run.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(rest.trim()));
            } else if let Some(rest) = line.strip_prefix("branch ") {
                if let Some(path) = path.take() {
                    let branch = rest.trim().trim_start_matches("refs/heads/").to_string();
                    trees.push(Worktree { path, branch });
                }
            }
        }
        Ok(trees)
    }

    /// Trees git knows about whose directory is gone — the stale case `G-11`
    /// names. Reported so the caller can prune them rather than discovering
    /// them as a confusing error later.
    pub fn stale(&self) -> Result<Vec<Worktree>> {
        Ok(self.list()?.into_iter().filter(|tree| !tree.path.exists()).collect())
    }
}

/// What a blocked feature must leave behind (`T-11`).
///
/// **A clean tree.** Its work is committed to its own branch or shelved, never
/// abandoned half-applied in the working copy — because the next feature's gate
/// would then run against a mixture of two features and blame the wrong one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing {
    /// Nothing was written; there is nothing to do.
    AlreadyClean,
    /// Committed to the feature's own branch.
    Committed { sha: String },
    /// Stashed, with the label needed to find it again.
    Shelved { label: String },
}

impl Landing {
    pub fn describe(&self) -> String {
        match self {
            Landing::AlreadyClean => "the tree was already clean".into(),
            Landing::Committed { sha } => format!("committed to the feature branch as {sha}"),
            Landing::Shelved { label } => {
                format!("shelved as `{label}` — recover with `git stash apply`")
            }
        }
    }
}

/// Land a blocked feature's work so the tree is clean (`T-11`).
///
/// Shelving rather than discarding, always. The work is evidence of what was
/// attempted, and a blocked feature is precisely the one someone will want to
/// look at.
pub fn land_blocked(repo: &Repo, step: &StepId, why: &str) -> Result<Landing> {
    let readiness = repo.readiness(None)?;
    if readiness.clean {
        return Ok(Landing::AlreadyClean);
    }
    let label = format!("perp-blocked-{step}");
    let run = repo.plumbing(&["stash", "push", "-u", "-m", &label])?;
    if run.contains("No local changes") {
        return Ok(Landing::AlreadyClean);
    }
    // Verified rather than assumed: a stash that did not happen leaves the
    // exact half-applied tree this function exists to prevent.
    let after = repo.readiness(None)?;
    if !after.clean {
        return Err(Error::refused(
            format!("blocked {step}"),
            format!(
                "the tree is still dirty after shelving ({why}). Refusing to continue — the \
                 next feature's gate would run against a mixture (`T-11`)"
            ),
        ));
    }
    Ok(Landing::Shelved { label })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    #[test]
    fn a_repository_with_nothing_special_says_so() {
        let dir = tmpdir("survey-plain");
        let survey = Survey::of(&dir);
        assert!(survey.is_safe_to_run());
        assert!(survey.render().contains("no submodules"), "{}", survey.render());
    }

    #[test]
    fn a_submodule_is_declared_unsupported_loudly() {
        // `G-12`. A loop that silently skips one ships half a change and
        // passes its own gates doing it.
        let dir = tmpdir("survey-submodule");
        std::fs::write(
            dir.join(".gitmodules"),
            "[submodule \"vendor/x\"]\n\tpath = vendor/x\n\turl = https://example/x\n",
        )
        .expect("write");

        let survey = Survey::of(&dir);
        assert!(!survey.is_safe_to_run(), "must not run unattended here");
        let text = survey.render();
        assert!(text.contains("NOT SUPPORTED"), "{text}");
        assert!(text.contains("half-applied"), "and says what would go wrong: {text}");
    }

    #[test]
    fn lfs_is_detected_from_the_attributes_that_enable_it() {
        let dir = tmpdir("survey-lfs");
        std::fs::write(dir.join(".gitattributes"), "*.psd filter=lfs diff=lfs merge=lfs -text\n")
            .expect("write");
        let survey = Survey::of(&dir);
        assert_eq!(survey.unsupported().len(), 1);
        assert!(survey.render().contains("pointer files"), "{}", survey.render());
    }

    #[test]
    fn a_clean_merge_and_a_conflicted_one_are_told_apart() {
        assert_eq!(read_merge("Fast-forward\n 2 files changed\n", true), Resolution::Clean);

        let conflicted = "Auto-merging src/main.rs\n\
                          CONFLICT (content): Merge conflict in src/main.rs\n\
                          Automatic merge failed; fix conflicts and then commit the result.\n";
        let Resolution::Blocked { files, conflict } = read_merge(conflicted, false) else {
            panic!("a conflict must block");
        };
        assert_eq!(files, ["src/main.rs"]);
        // Verbatim (Perpetum 0.5) — a summarised conflict is one nobody can
        // resolve from the journal.
        assert_eq!(conflict, conflicted);
    }

    #[test]
    fn a_conflict_is_parked_rather_than_resolved_by_picking_a_side() {
        // `G-9`. There is no second attempt, because git already merges the
        // non-overlapping hunks itself — anything it could not do is semantic,
        // and semantic is exactly what this must not touch.
        let conflicted = "CONFLICT (content): Merge conflict in a.rs\n\
                          CONFLICT (content): Merge conflict in b.rs\n";
        let resolution = read_merge(conflicted, false);
        assert!(resolution.is_blocked());

        let record = resolution.record(StepId::new(4, "b19", 1).expect("step"), 1_700_000_000);
        assert_eq!(record.ok, Some(false));
        assert!(record.summary.contains("2 file(s) conflict"), "{}", record.summary);
        assert_eq!(record.detail.as_deref(), Some(conflicted));
    }

    #[test]
    fn a_worktree_directory_is_a_sibling_and_not_a_child() {
        // A worktree under the repository is a directory the loop's own globs
        // walk into and whose files its gates compile twice.
        let repo = Repo::at(Path::new("/tmp/example"));
        let trees = Worktrees::new(&repo, Path::new("/tmp/worktrees"));
        let dir = trees.dir_for("perp/c4/b19");
        assert!(!dir.starts_with("/tmp/example"), "{}", dir.display());
        assert!(dir.ends_with("perp-c4-b19"), "{}", dir.display());
    }

    #[test]
    fn a_blocked_feature_that_wrote_nothing_needs_no_landing() {
        assert_eq!(Landing::AlreadyClean.describe(), "the tree was already clean");
        assert!(Landing::Shelved { label: "x".into() }.describe().contains("stash apply"));
    }

    #[test]
    fn shelving_says_how_to_get_the_work_back() {
        // `T-11` says never abandoned. Discarding would satisfy "clean tree"
        // and lose the evidence of what was attempted — and a blocked feature
        // is exactly the one someone will want to look at.
        let landing = Landing::Shelved { label: "perp-blocked-c4/b19/s03".into() };
        let text = landing.describe();
        assert!(text.contains("perp-blocked-c4/b19/s03"), "{text}");
        assert!(text.contains("recover"), "{text}");
    }

    #[test]
    fn in_repo_hooks_are_reported_as_supported_rather_than_ignored() {
        // They run. That is the intended behaviour, because `--no-verify` is
        // refused (`G-4`) — so this is a note, not a blocker.
        let hooks = Feature::InRepoHooks { path: ".githooks".into() };
        assert!(hooks.is_supported());
        assert!(hooks.describe().contains("they will run"), "{}", hooks.describe());
        assert!(Survey { features: vec![hooks] }.is_safe_to_run());
    }
}
