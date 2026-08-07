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

    /// Read a capture back out of the journal record that recorded it.
    ///
    /// The journal is the source of truth, so a surface listing evidence lists
    /// **records**, not the contents of a directory (`I-5`). The difference is
    /// the claim: a PNG on disk with no record is a picture of something, and
    /// `A-6` is the requirement that says a picture is not evidence.
    ///
    /// `None` for a record that is not a capture, which is nearly all of them.
    pub fn from_record(record: &Record) -> Option<Capture> {
        let claim = record.summary.strip_prefix("captured: ")?.to_string();
        let detail = record.detail.as_deref()?;
        let mut path = None;
        let mut bytes = 0usize;
        let mut hash = 0u64;
        for line in detail.lines() {
            let Some((key, value)) = line.split_once(": ") else { continue };
            match key {
                "capture" => path = Some(PathBuf::from(value)),
                "bytes" => bytes = value.trim().parse().unwrap_or_default(),
                "hash" => hash = u64::from_str_radix(value.trim(), 16).unwrap_or_default(),
                _ => {}
            }
        }
        Some(Capture { path: path?, step: record.step.to_string(), claim, bytes, hash })
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

/// The next step id in `stage`, for a command that is not part of a batch.
///
/// A capture's file is named from its step, so two captures sharing a step id
/// share a file and the second overwrites the first — which is a journal
/// carrying two records for one picture, the same failure `L-14` names for
/// terminal records. Lives here because the surfaces that take evidence are
/// what need it, and there were two of them.
pub fn next_step(records: &[Record], stage: &str) -> StepId {
    let cycle = records.last().map_or(1, |record| record.step.cycle);
    let seq = records.iter().map(|record| record.step.seq).max().unwrap_or(0) + 1;
    StepId::new(cycle, stage, seq).unwrap_or(StepId { cycle, stage: stage.into(), seq })
}

// ------------------------------------------------------------------ product

/// The product under construction, and how to put it on screen (`L-34`).
///
/// A screenshot of a workspace is a screenshot of an editor. What `L-34` asks
/// for is a picture of *the thing being built*, and getting one means running
/// it — so the command is declared in the binding, next to the gates, and read
/// from there:
///
/// ```text
/// product.run    = cargo run -p janitor-gui
/// product.settle = 4
/// product.cwd    = .
/// product.window = janitor
/// ```
///
/// **Declared, never composed.** This is the same line `S-9` draws around a
/// gate: a command the operator wrote is a command they chose to run, and one
/// assembled from a request is the front end choosing what executes on the
/// machine. A browser can ask for the picture; it cannot say what to run to get
/// it, and a workspace that declares nothing gets a refusal that names the key
/// rather than a best guess at what the product might be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub command: String,
    /// How long to let it draw itself before the shutter. Four seconds by
    /// default, which is a cold `cargo run` short of enough — hence a key, and
    /// hence the capture says how long it waited.
    pub settle: Duration,
    pub cwd: PathBuf,
    /// The start of the product's window title, when it has one.
    ///
    /// A window opened by a process that is not in the foreground does not
    /// come to the front — Windows refuses the activation — so the product
    /// draws itself *behind* whatever was already maximised, and the shutter
    /// catches a screen with no product on it. That is the failure this key
    /// exists to fix, and it was found by taking the picture and looking.
    ///
    /// Declared rather than guessed: `cargo run -q -p janitor-gui` does not
    /// name a window, and picking the newest one on the desktop would be the
    /// harness deciding which window is the product.
    pub window: Option<String>,
}

/// What a photograph of the product came to (`L-34`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Photograph {
    pub capture: Capture,
    /// Whether the platform's *activate this window* command ran and reported
    /// success — **not** whether the window ended up on top.
    ///
    /// The distinction is the whole reason this field is worded that way.
    /// Windows refuses foreground activation requested by a process that is not
    /// itself in the foreground: the call succeeds, the taskbar button flashes,
    /// and the window stays behind whatever was already maximised. So `true`
    /// means *asked, and the window was found*, and a caller that renders it as
    /// "the product is in this picture" would be making a claim neither this
    /// function nor the operating system can support (`A-6`).
    ///
    /// `false` covers a title that matched nothing, no `product.window`
    /// declared, and a platform with no such command.
    pub focused: bool,
}

impl Product {
    /// Read it out of the binding, or refuse naming the key that is missing.
    pub fn declared(binding: &crate::binding::Binding) -> Result<Product> {
        let command = binding.get("product.run").map_err(|_| {
            Error::unbound(
                "product.run",
                "no command to run — a screenshot of the product needs the product, and \
                 nothing else here knows how to start it",
            )
        })?;
        let settle = binding
            .get("product.settle")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(4);
        let cwd = binding.get("product.cwd").map(|value| binding.root().join(value));
        Ok(Product {
            command: command.to_string(),
            settle: Duration::from_secs(settle),
            cwd: cwd.unwrap_or_else(|_| binding.root().to_path_buf()),
            window: binding
                .get("product.window")
                .ok()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_string),
        })
    }

    /// Start it, photograph the screen, and stop it (`L-34`, `X-6`).
    ///
    /// The product is run with a timeout of `settle` plus a small hold, and the
    /// timeout is how it is stopped: `process::run` already kills a whole
    /// process tree at its deadline through the job object `X-4` needed, and a
    /// second way to kill a child is a second way to leak one.
    ///
    /// What comes back is a picture of the **screen**, not of a window — the
    /// platform commands in [`capture_command`] photograph the virtual desktop
    /// and there is no cross-platform way to single out a window. The claim
    /// says so, because a screenshot that is described as more specific than it
    /// is is the failure `A-6` exists to prevent.
    ///
    /// **A product that was not running at the shutter is not photographed.**
    /// The run is watched, and a command that ended before its deadline ended
    /// before the picture was taken — so the picture cannot be evidence of it,
    /// the file is removed, and the error carries what the command actually
    /// said. Journalling it instead would be `L-33` exactly: a claim that
    /// something is present, made without checking whether it is.
    pub fn photograph(&self, journal: &Path, step: &StepId, claim: &str) -> Result<Photograph> {
        let deadline = self.settle + Duration::from_secs(2);
        let spec = Spec::new(self.command.clone(), self.cwd.clone(), deadline)
            .with_env(Env::declared());

        // The product runs on its own thread because it has to still be running
        // when the shutter goes.
        let running = std::thread::spawn(move || process::run(&spec));
        std::thread::sleep(self.settle);
        let focused = self.window.as_deref().is_some_and(bring_forward);
        let shot = capture(journal, step, claim);
        // Joined rather than detached, so the child is gone before this
        // returns and a second capture does not photograph the first one's
        // window.
        let ended = running.join();

        let capture = shot?;
        // `TimedOut` is the *expected* outcome: the deadline is how the product
        // is stopped, so hitting it means it was still on screen.
        if let Ok(Ok(run)) = &ended {
            if run.exit != process::Exit::TimedOut {
                let _ = std::fs::remove_file(&capture.path);
                return Err(Error::refused(
                    "capture",
                    format!(
                        "`{}` {} after {}ms, before the shutter — so this picture could not \
                         show it, and was not kept. {}",
                        self.command,
                        run.exit.describe(),
                        run.duration_ms,
                        first_line(&run.stderr_tail)
                    ),
                ));
            }
        }
        Ok(Photograph { capture, focused })
    }
}

/// The first line of a command's stderr, for an error message that has to fit
/// on a screen. The whole transcript is what a gate keeps; this is a reason.
fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default().to_string()
}

/// Ask for the product's window to come to the front.
///
/// **Asking is all this can do**, and the return value says only that the ask
/// was made and the window was found — see [`Photograph::focused`]. It is worth
/// making anyway: a window that does come forward is the difference between a
/// picture of the product and a picture of whatever was already open.
fn bring_forward(title: &str) -> bool {
    let Some(command) = focus_command(title) else { return false };
    let spec = Spec::new(command, std::env::current_dir().unwrap_or_default(), Duration::from_secs(10))
        .with_env(Env::declared());
    let focused = process::run(&spec).is_ok_and(|run| run.exit.is_success());
    if focused {
        // The activation is asynchronous — the window is told to come forward
        // and the compositor draws it a moment later. Photographing on the same
        // instruction catches the screen mid-swap.
        std::thread::sleep(Duration::from_millis(700));
    }
    focused
}

/// The platform's "activate the window called this" command, or `None`.
///
/// Returned rather than run, for the same reason [`capture_command`] is: an
/// operator can see exactly what would execute, and it is testable without a
/// desktop session.
pub fn focus_command(title: &str) -> Option<String> {
    // A title is an operator's binding value, not a model's string, but it goes
    // inside a quoted PowerShell literal all the same — so a quote in it would
    // end the literal and the rest would be a statement. Doubled, which is how
    // PowerShell escapes one.
    let safe = title.replace('\'', "''");
    if cfg!(target_os = "windows") {
        // `AppActivate` matches a title or its start, and is in the Visual
        // Basic assembly that ships with .NET — no extra tool, and no `unsafe`
        // in this crate for a window handle.
        Some(format!(
            "powershell -NoProfile -Command \"Add-Type -AssemblyName Microsoft.VisualBasic; \
             [Microsoft.VisualBasic.Interaction]::AppActivate('{safe}')\""
        ))
    } else {
        // `wmctrl` is not installed by default anywhere, so this is `None`
        // rather than a command that usually fails — a missing focus is a
        // picture of the screen, which is what the claim already says.
        None
    }
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

    /// The record is what a surface reads captures back out of (`I-5`), so it
    /// has to survive the round trip — claim, path, size and hash.
    #[test]
    fn a_capture_reads_back_out_of_its_own_record() {
        let capture = Capture {
            path: PathBuf::from(".harness/evidence/c4-b24-s01.png"),
            step: "c4/b24/s01".into(),
            claim: "the window lists eleven rules".into(),
            bytes: 40_512,
            hash: 0x1234_5678_9abc_def0,
        };
        let read = Capture::from_record(&capture.record(step(), 1_700_000_000));
        assert_eq!(read, Some(capture));

        // And a record that is not a capture is not read as one — which is
        // nearly every record in a journal.
        let ordinary = crate::journal::Record::outcome(step(), 1_700_000_000, true, "did the thing");
        assert_eq!(Capture::from_record(&ordinary), None);
    }

    /// `L-34`: what runs is the operator's line, and it is read from the
    /// binding rather than guessed at. A workspace that declares nothing is
    /// refused by the key's name.
    #[test]
    fn the_product_is_declared_or_it_is_refused_by_name() {
        let root = tmpdir("product");
        let write = |body: &str| {
            std::fs::create_dir_all(root.join(".harness")).expect("dirs");
            std::fs::write(
                root.join(crate::layout::BINDING),
                format!("```perp-binding
{body}```
"),
            )
            .expect("binding");
            crate::binding::Binding::load(&root).expect("load")
        };

        let refused = Product::declared(&write("path.requirements = r.md
"))
            .expect_err("nothing declared");
        assert!(format!("{refused}").contains("product.run"), "{refused}");

        let product = Product::declared(&write(
            "product.run    = cargo run -p janitor-gui
             product.settle = 9
             product.window = janitor
",
        ))
        .expect("declared");
        assert_eq!(product.command, "cargo run -p janitor-gui");
        assert_eq!(product.settle, Duration::from_secs(9));
        assert_eq!(product.window.as_deref(), Some("janitor"));

        // The settle has a default; the command deliberately does not.
        let bare = Product::declared(&write("product.run = ./app
")).expect("declared");
        assert_eq!(bare.settle, Duration::from_secs(4));
        assert_eq!(bare.window, None);
    }

    /// The title goes inside a quoted PowerShell literal, and a quote in it
    /// would close the literal and leave the rest as a statement.
    #[test]
    fn a_window_title_cannot_end_the_command_it_is_quoted_in() {
        let Some(command) = focus_command("it's here'; Remove-Item C:/ -Recurse; '") else {
            return; // No focus command on this platform, which is a valid answer.
        };
        assert!(command.contains("it''s here"), "{command}");
        // Every quote in the title is doubled, so the literal never closes
        // early and `Remove-Item` stays a string.
        let inside = command.split_once("AppActivate('").and_then(|(_, rest)| rest.split_once("')"));
        assert!(inside.is_some(), "the literal closes exactly once: {command}");
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
