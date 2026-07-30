//! Screen evidence (`X-6`) and surviving a reboot (`X-9`).
//!
//! Two capabilities that only make sense at the ends of a long run: proving
//! what a thing looked like, and still being there tomorrow.
//!
//! Both are shaped by the same constraint — the harness must not become the
//! thing that quietly installed itself. Registration is approval-gated,
//! reversible, and names the exact command it would run before running it.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::approval::Policy;
use crate::error::{Error, Result};
use crate::journal::Record;
use crate::process::{self, Env, Spec};
use crate::step::StepId;

// ------------------------------------------------------------------ capture

/// A screenshot, stored beside the journal (`X-6`, `A-6`).
///
/// Beside the journal rather than in it: a PNG in a JSONL record would be a
/// megabyte of base64 in a file meant to be read with `tail`. The record holds
/// the path and the hash; the bytes live next to the transcripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub path: PathBuf,
    pub step: String,
    /// What it is evidence *of*. A screenshot with no claim attached is a
    /// picture, not evidence (`A-6`).
    pub claim: String,
    pub bytes: usize,
    /// Content hash, so a file swapped later is detectable.
    pub hash: u64,
}

impl Capture {
    /// Where captures live: beside the journal, in their own directory.
    pub fn dir(journal: &Path) -> PathBuf {
        journal.parent().unwrap_or(Path::new(".")).join("evidence")
    }

    /// The record that makes it findable (`A-6`).
    pub fn record(&self, step: StepId, at: i64) -> Record {
        Record::outcome(step, at, true, format!("captured: {}", self.claim)).with_detail(format!(
            "capture: {}\nbytes: {}\nhash: {:016x}\nstep: {}",
            crate::runtime::normalise(&self.path),
            self.bytes,
            self.hash,
            self.step
        ))
    }

    /// Whether the file on disk is still the one that was captured.
    ///
    /// Evidence that cannot be checked is decoration. A verifier reading this
    /// months later needs to know the PNG was not replaced.
    pub fn is_intact(&self) -> bool {
        match std::fs::read(&self.path) {
            Ok(bytes) => crate::watchdog::content_hash(&bytes) == self.hash,
            Err(_) => false,
        }
    }
}

/// The platform's screen-capture command, or `None` where there is not one the
/// harness can rely on.
///
/// Returned rather than run, so it is testable without a desktop session — and
/// so the operator can see exactly what would execute.
pub fn capture_command(target: &Path) -> Option<String> {
    let path = crate::runtime::normalise(target);
    if cfg!(target_os = "windows") {
        // No dependency and no extra tool: .NET's screen capture is in every
        // Windows install, reachable from the PowerShell that is already there.
        Some(format!(
            "powershell -NoProfile -Command \"Add-Type -AssemblyName System.Windows.Forms,\
             System.Drawing; $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; \
             $i=New-Object Drawing.Bitmap $b.Width,$b.Height; \
             [Drawing.Graphics]::FromImage($i).CopyFromScreen($b.Location,[Drawing.Point]::Empty,\
             $i.Size); $i.Save('{path}')\""
        ))
    } else if cfg!(target_os = "macos") {
        // `-x` suppresses the shutter sound. An unattended loop that beeps
        // every few minutes is one somebody unplugs.
        Some(format!("screencapture -x {path}"))
    } else {
        // `import` ships with ImageMagick, which is not guaranteed — hence the
        // `Option`, and hence a missing capture is a warning rather than a
        // failed step.
        Some(format!("import -window root {path}"))
    }
}

/// Take a screenshot and record what it is evidence of (`X-6`).
///
/// A capture that fails is an `Err` the caller turns into a warning: evidence
/// is worth having and never worth failing a batch over — the same rule as
/// `A-7`.
pub fn capture(journal: &Path, step: &StepId, claim: &str) -> Result<Capture> {
    let dir = Capture::dir(journal);
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    // Absolute, always. The capture tool runs with its own working directory,
    // and a relative path there resolves somewhere nobody intended — which is
    // how the first run wrote nothing and reported a missing file.
    let dir = dir.canonicalize().unwrap_or(dir);
    let path = dir.join(format!("{}.png", step.to_string().replace('/', "-")));

    let Some(command) = capture_command(&path) else {
        return Err(Error::refused("capture", "no screen-capture tool on this platform"));
    };
    let spec = Spec::new(command, dir.clone(), Duration::from_secs(30)).with_env(Env::declared());
    let run = process::run(&spec)?;

    // Read back through the same normalisation the command was given, so a
    // mismatch between the two is impossible rather than merely unlikely.
    let written = PathBuf::from(crate::runtime::normalise(&path));
    let bytes = std::fs::read(&written).map_err(|e| {
        Error::refused(
            "capture",
            format!(
                "wrote nothing to {}: {}",
                written.display(),
                if run.stderr_tail.trim().is_empty() { format!("{e}") } else { run.stderr_tail.trim().to_string() }
            ),
        )
    })?;
    if bytes.is_empty() {
        return Err(Error::refused(
            "capture",
            format!("produced an empty file: {}", run.stderr_tail.trim()),
        ));
    }
    Ok(Capture {
        path: written,
        step: step.to_string(),
        claim: claim.to_string(),
        bytes: bytes.len(),
        hash: crate::watchdog::content_hash(&bytes),
    })
}

// ---------------------------------------------------------------- scheduling

/// When a registered run fires (`X-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// At boot, which is the case the requirement is actually about: a cycle
    /// that survives the machine restarting.
    AtBoot,
    /// Every *n* minutes.
    Every { minutes: u32 },
}

impl fmt::Display for When {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            When::AtBoot => f.write_str("at boot"),
            When::Every { minutes } => write!(f, "every {minutes} minutes"),
        }
    }
}

/// A registration the operator is asked to approve (`X-9`).
///
/// Holds the exact command rather than describing it. An approval for
/// "register with the scheduler" is not an approval anyone can evaluate; an
/// approval for a command they can read is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub name: String,
    pub when: When,
    pub workspace: PathBuf,
    /// Exactly what would run.
    pub command: String,
    /// How to undo it.
    pub removal: String,
}

/// The task name. One, fixed, so a second registration replaces the first
/// rather than accumulating — the failure mode where a machine ends up running
/// four copies of the loop after four experiments.
pub const TASK_NAME: &str = "PerpetumHarness";

impl Registration {
    /// Build a registration. Nothing is installed; this is what would be.
    pub fn plan(program: &Path, workspace: &Path, when: When) -> Registration {
        let exe = crate::runtime::normalise(program);
        let root = crate::runtime::normalise(workspace);
        // `resume` rather than `run`: `L-7` says a process that restarts
        // reconciles what the last one left in flight *before* doing anything
        // else. Booting straight into new work is how a half-applied step gets
        // built on top of.
        let inner = format!("{exe} resume --root {root}");

        let (command, removal) = if cfg!(target_os = "windows") {
            let trigger = match when {
                When::AtBoot => "/SC ONSTART".to_string(),
                When::Every { minutes } => format!("/SC MINUTE /MO {minutes}"),
            };
            (
                format!("schtasks /Create /F /TN {TASK_NAME} {trigger} /TR \"{inner}\""),
                format!("schtasks /Delete /F /TN {TASK_NAME}"),
            )
        } else if cfg!(target_os = "macos") {
            (
                format!("launchctl submit -l {TASK_NAME} -- {inner}"),
                format!("launchctl remove {TASK_NAME}"),
            )
        } else {
            let timer = match when {
                When::AtBoot => "--on-boot=1min".to_string(),
                When::Every { minutes } => format!("--on-unit-active={minutes}min"),
            };
            (
                format!("systemd-run --user --unit={TASK_NAME} {timer} {inner}"),
                format!("systemctl --user stop {TASK_NAME}.timer"),
            )
        };

        Registration {
            name: TASK_NAME.to_string(),
            when,
            workspace: workspace.to_path_buf(),
            command,
            removal,
        }
    }

    /// What it costs (`X-9`). Always an approval.
    ///
    /// Registering outlives the cycle, the session, and the operator's memory
    /// of having agreed to it — which is exactly why it is asked every time
    /// rather than remembered as a setting.
    pub fn policy(&self) -> Policy {
        Policy::Approve {
            reason: format!(
                "registering `{}` to run {} outlives this cycle and this session (`X-9`). \
                 It would run: {}",
                self.name, self.when, self.command
            ),
        }
    }

    /// Install it. Refuses without a named person (`T-13`).
    pub fn install(&self, approved_by: &str) -> Result<String> {
        if approved_by.trim().is_empty() {
            return Err(Error::refused(
                self.name.clone(),
                "needs a person's approval — scheduler registration is never automatic (`X-9`)",
            ));
        }
        let spec = Spec::new(self.command.clone(), &self.workspace, Duration::from_secs(60))
            .with_env(Env::declared());
        let run = process::run(&spec)?;
        if !run.is_success() {
            return Err(Error::refused(
                self.name.clone(),
                format!("registration failed: {}", run.stderr_tail.trim()),
            ));
        }
        Ok(format!("registered by {approved_by}; remove with `{}`", self.removal))
    }

    pub fn record(&self, step: StepId, at: i64, approved_by: &str) -> Record {
        Record::outcome(step, at, true, format!("registered to run {}", self.when))
            .with_detail(format!(
                "approved_by: {approved_by}\ncommand: {}\nremoval: {}",
                self.command, self.removal
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    fn step() -> StepId {
        StepId::new(4, "b24", 1).expect("step")
    }

    #[test]
    fn evidence_lives_beside_the_journal_and_not_inside_it() {
        // A PNG in a JSONL record would be a megabyte of base64 in a file meant
        // to be read with `tail`.
        let dir = Capture::dir(Path::new(".harness/journal.jsonl"));
        assert!(dir.ends_with("evidence"), "{}", dir.display());
        assert!(dir.starts_with(".harness"), "{}", dir.display());
    }

    #[test]
    fn a_capture_carries_the_claim_it_is_evidence_of() {
        // `A-6`. A screenshot with no claim attached is a picture.
        let capture = Capture {
            path: PathBuf::from(".harness/evidence/c4-b24-s01.png"),
            step: "c4/b24/s01".into(),
            claim: "the panel shows three blocked steps".into(),
            bytes: 40_512,
            hash: 0x1234_5678_9abc_def0,
        };
        let record = capture.record(step(), 1_700_000_000);
        assert!(record.summary.contains("three blocked steps"), "{}", record.summary);
        let detail = record.detail.expect("detail");
        assert!(detail.contains("hash: 123456789abcdef0"), "{detail}");
        assert!(detail.contains("evidence/c4-b24-s01.png"), "{detail}");
    }

    #[test]
    fn a_swapped_file_is_detectable() {
        // Evidence that cannot be checked is decoration.
        let dir = tmpdir("capture-intact");
        let path = dir.join("shot.png");
        std::fs::write(&path, b"original bytes").expect("write");

        let capture = Capture {
            path: path.clone(),
            step: "c4/b24/s01".into(),
            claim: "x".into(),
            bytes: 14,
            hash: crate::watchdog::content_hash(b"original bytes"),
        };
        assert!(capture.is_intact());

        std::fs::write(&path, b"substituted!!!").expect("rewrite");
        assert!(!capture.is_intact(), "a replaced file must not read as intact");

        std::fs::remove_file(&path).expect("remove");
        assert!(!capture.is_intact(), "and a missing one is not intact either");
    }

    #[test]
    fn the_capture_command_names_the_file_it_writes() {
        let command = capture_command(Path::new(".harness/evidence/x.png"))
            .expect("a command on every supported platform");
        assert!(command.contains("evidence/x.png"), "{command}");
        if cfg!(target_os = "macos") {
            assert!(command.contains("-x"), "an unattended loop must not beep: {command}");
        }
    }

    #[test]
    fn a_registration_shows_the_command_before_it_runs() {
        // `X-9`. An approval for "register with the scheduler" is not one
        // anybody can evaluate; an approval for a command they can read is.
        let plan = Registration::plan(
            Path::new("C:/tools/perp.exe"),
            Path::new("D:/repos/justcode"),
            When::AtBoot,
        );
        let Policy::Approve { reason } = plan.policy() else { panic!("must be asked") };
        assert!(reason.contains("outlives"), "{reason}");
        assert!(reason.contains(&plan.command), "the exact command is in the ask: {reason}");
    }

    #[test]
    fn a_scheduled_run_resumes_rather_than_starting_new_work() {
        // `L-7`: a process that restarts reconciles what the last one left in
        // flight *before* doing anything else. Booting straight into new work
        // is how a half-applied step gets built on top of.
        let plan = Registration::plan(
            Path::new("/usr/local/bin/perp"),
            Path::new("/home/x/repo"),
            When::AtBoot,
        );
        assert!(plan.command.contains("resume"), "{}", plan.command);
        assert!(!plan.command.contains(" run "), "{}", plan.command);
    }

    #[test]
    fn there_is_one_task_name_so_a_second_registration_replaces_the_first() {
        // Otherwise a machine ends up running four copies of the loop after
        // four experiments.
        let a = Registration::plan(Path::new("perp"), Path::new("/a"), When::AtBoot);
        let b = Registration::plan(Path::new("perp"), Path::new("/b"), When::Every { minutes: 30 });
        assert_eq!(a.name, b.name);
        assert_eq!(a.name, TASK_NAME);
        if cfg!(target_os = "windows") {
            assert!(a.command.contains("/F"), "force-replace: {}", a.command);
        }
    }

    #[test]
    fn every_registration_says_how_to_undo_it() {
        for when in [When::AtBoot, When::Every { minutes: 15 }] {
            let plan = Registration::plan(Path::new("perp"), Path::new("/x"), when);
            assert!(!plan.removal.is_empty(), "{plan:?}");
            assert!(plan.removal.contains(TASK_NAME), "{}", plan.removal);
        }
    }

    #[test]
    fn installing_without_a_person_is_refused() {
        let plan = Registration::plan(Path::new("perp"), Path::new("/x"), When::AtBoot);
        let err = plan.install("  ").expect_err("never automatic");
        assert!(format!("{err}").contains("X-9"), "{err}");
    }

    #[test]
    fn the_record_names_who_approved_it_and_how_to_remove_it() {
        let plan = Registration::plan(Path::new("perp"), Path::new("/x"), When::AtBoot);
        let record = plan.record(step(), 1_700_000_000, "the operator");
        let detail = record.detail.expect("detail");
        assert!(detail.contains("approved_by: the operator"), "{detail}");
        assert!(detail.contains("removal:"), "{detail}");
    }
}
