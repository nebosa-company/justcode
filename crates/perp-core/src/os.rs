//! The operating system, at arm's length (`X-1`–`X-3`, `X-5`–`X-11`).
//!
//! Everything here is a *capability the harness may be asked for*, and the
//! module's real content is where each one sits on the permission scale. The
//! code that opens a URL is four lines; deciding which URLs may be opened
//! without asking is the requirement.
//!
//! The scale, applied consistently:
//!
//! - **The workspace root is the boundary** (`X-2`). Reading outside it needs
//!   an approval unless allowlisted; writing outside it always does; deleting
//!   outside it is on the Never list and no approval unlocks it.
//! - **User data the loop did not create is `approve`** (`X-8`). The clipboard
//!   holds whatever the operator last copied, which is routinely a password.
//! - **Anything that survives the process is `approve`** (`X-9`). Registering
//!   with the OS scheduler outlives the cycle, the session, and the operator's
//!   memory of having agreed to it.
//!
//! And one thing that is *not* here on purpose: GUI automation (`X-11`).

use std::fmt;
use std::path::{Path, PathBuf};

use crate::approval::Policy;
use crate::error::{Error, Result};
use crate::journal::Record;
use crate::process::{Env, Spec};
use crate::step::StepId;

/// The surface (`X-1`). One enum, so the permission table below is total and a
/// new capability cannot be added without deciding what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// Read a path. `inside` is whether it is under the workspace root.
    Read { path: PathBuf, inside: bool },
    Write { path: PathBuf, inside: bool },
    Delete { path: PathBuf, inside: bool },
    /// Discover a toolchain's version.
    Discover { tool: String },
    Notify { about: String },
    /// Open a file or URL in the default application.
    Open { target: String },
    Screenshot,
    ClipboardRead,
    ClipboardWrite,
    /// Register with Task Scheduler, systemd or launchd.
    Schedule { when: String },
    /// Drive another application's mouse and keyboard.
    GuiAutomation,
}

impl Capability {
    pub fn describe(&self) -> String {
        match self {
            Capability::Read { path, .. } => format!("read {}", path.display()),
            Capability::Write { path, .. } => format!("write {}", path.display()),
            Capability::Delete { path, .. } => format!("delete {}", path.display()),
            Capability::Discover { tool } => format!("discover {tool}"),
            Capability::Notify { about } => format!("notify: {about}"),
            Capability::Open { target } => format!("open {target}"),
            Capability::Screenshot => "screenshot".into(),
            Capability::ClipboardRead => "read the clipboard".into(),
            Capability::ClipboardWrite => "write the clipboard".into(),
            Capability::Schedule { when } => format!("schedule a run {when}"),
            Capability::GuiAutomation => "drive another application".into(),
        }
    }
}

/// Paths outside the workspace the operator has allowed reading (`X-2`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadAllowlist {
    prefixes: Vec<PathBuf>,
}

impl ReadAllowlist {
    pub fn new(prefixes: Vec<PathBuf>) -> ReadAllowlist {
        ReadAllowlist { prefixes }
    }

    /// From binding entries: `os.read.allow = C:/toolchains, /usr/share`.
    pub fn from_entries(entries: &[(String, String)]) -> ReadAllowlist {
        let mut prefixes = Vec::new();
        for (key, value) in entries {
            if key == "os.read.allow" {
                prefixes.extend(
                    value.split(',').map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from),
                );
            }
        }
        ReadAllowlist { prefixes }
    }

    pub fn covers(&self, path: &Path) -> bool {
        self.prefixes.iter().any(|prefix| path.starts_with(prefix))
    }
}

/// What a capability costs.
///
/// The whole module in one function, deliberately: a permission table scattered
/// across ten call sites is a permission table nobody can read as a whole, and
/// this one has to be arguable.
pub fn classify(capability: &Capability, allowlist: &ReadAllowlist) -> Policy {
    match capability {
        Capability::Read { inside: true, .. }
        | Capability::Write { inside: true, .. }
        | Capability::Delete { inside: true, .. } => Policy::Auto,

        Capability::Read { path, .. } => {
            if allowlist.covers(path) {
                Policy::Auto
            } else {
                Policy::Approve {
                    reason: format!(
                        "{} is outside the workspace and not allowlisted (`X-2`)",
                        path.display()
                    ),
                }
            }
        }
        Capability::Write { path, .. } => Policy::Approve {
            reason: format!("writing outside the workspace is always asked (`X-2`): {}", path.display()),
        },
        // No allowlist, no approval, no exception. The one irreversible
        // capability on this list, and the one an unattended loop has the least
        // business exercising.
        Capability::Delete { path, .. } => Policy::Never {
            reason: format!(
                "deleting outside the workspace is on the Never list (`X-2`): {}",
                path.display()
            ),
        },

        Capability::Discover { .. } => Policy::Auto,
        Capability::Notify { .. } => Policy::Auto,
        Capability::Screenshot => Policy::Auto,
        // The clipboard holds whatever the operator last copied, which is
        // routinely a password (`X-8`).
        Capability::ClipboardRead => Policy::Approve {
            reason: "the clipboard is user data the loop did not create (`X-8`)".into(),
        },
        Capability::ClipboardWrite => Policy::Auto,

        Capability::Open { target } => {
            if opens_freely(target) {
                Policy::Auto
            } else {
                Policy::Approve {
                    reason: format!(
                        "opening {target} leaves the workspace and localhost (`X-7`)"
                    ),
                }
            }
        }
        Capability::Schedule { when } => Policy::Approve {
            reason: format!(
                "registering with the OS scheduler ({when}) outlives this cycle and this \
                 session (`X-9`)"
            ),
        },
        // `X-11`. Not "approve" — out of scope, and if it is ever added it is
        // `never` while unattended. A capability that does not exist cannot be
        // reached by an approval that should not have been given.
        Capability::GuiAutomation => Policy::Never {
            reason: "GUI automation is out of scope (`X-11`)".into(),
        },
    }
}

/// A workspace-relative or localhost target opens without asking (`X-7`).
fn opens_freely(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    if let Some(host) = crate::security::host_of(&lower) {
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1";
        }
        // A scheme that is neither http nor https — `file:`, `mailto:`,
        // `vscode:` — is not something to open unasked. `mailto:` in particular
        // is a message the operator did not write.
        if lower.contains("://") || lower.contains(':') && !is_windows_drive(&lower) {
            return false;
        }
    }
    // A relative path. Traversal is caught by the caller resolving it against
    // the root before it gets here; a `..` that survives to this point is not
    // treated as workspace-relative.
    !target.contains("..")
}

fn is_windows_drive(target: &str) -> bool {
    let mut chars = target.chars();
    matches!((chars.next(), chars.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic())
}

/// One tool the binding names, and what was found (`X-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    /// The version string the tool printed, verbatim and untrimmed of meaning.
    pub version: Option<String>,
    /// What went wrong, when nothing was found.
    pub missing: Option<String>,
}

impl Tool {
    pub fn found(name: &str, version: &str) -> Tool {
        Tool {
            name: name.to_string(),
            version: Some(version.trim().to_string()),
            missing: None,
        }
    }

    pub fn missing(name: &str, why: &str) -> Tool {
        Tool { name: name.to_string(), version: None, missing: Some(why.trim().to_string()) }
    }

    pub fn is_present(&self) -> bool {
        self.version.is_some()
    }
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.version, &self.missing) {
            (Some(version), _) => write!(f, "{}: {version}", self.name),
            (None, Some(why)) => write!(f, "{}: not found — {why}", self.name),
            (None, None) => write!(f, "{}: not found", self.name),
        }
    }
}

/// The toolchain, recorded at cycle start (`X-3`).
///
/// *"Works on my machine"* becomes a journal entry instead of a mystery. The
/// value is not in the versions being right — it is in them being **written
/// down before anything failed**, so the comparison exists later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolchain {
    pub tools: Vec<Tool>,
}

impl Toolchain {
    /// Ask each tool for its version. A tool that is not there is recorded as
    /// missing rather than skipped — an absent entry reads as "not asked".
    pub fn discover(names: &[String], cwd: &Path, timeout: std::time::Duration) -> Toolchain {
        let tools = names
            .iter()
            .map(|name| {
                let spec = Spec::new(format!("{name} --version"), cwd, timeout)
                    .with_env(Env::declared());
                match crate::process::run(&spec) {
                    Ok(run) if run.is_success() => {
                        let text = if run.stdout_tail.trim().is_empty() {
                            &run.stderr_tail
                        } else {
                            &run.stdout_tail
                        };
                        Tool::found(name, text.lines().next().unwrap_or(""))
                    }
                    Ok(run) => Tool::missing(name, &format!("exit {:?}", run.exit)),
                    Err(e) => Tool::missing(name, &format!("{e}")),
                }
            })
            .collect();
        Toolchain { tools }
    }

    /// Read `toolchain = cargo, git, node` out of binding entries.
    pub fn names_from_entries(entries: &[(String, String)]) -> Vec<String> {
        entries
            .iter()
            .filter(|(key, _)| key == "toolchain")
            .flat_map(|(_, value)| value.split(','))
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    }

    pub fn missing(&self) -> Vec<&Tool> {
        self.tools.iter().filter(|tool| !tool.is_present()).collect()
    }

    pub fn render(&self) -> String {
        self.tools.iter().map(|tool| format!("{tool}\n")).collect()
    }

    /// Into the journal, so a later failure can be compared against it.
    pub fn record(&self, step: StepId, at: i64) -> Record {
        let missing = self.missing().len();
        let summary = if missing == 0 {
            format!("toolchain: {} tools found", self.tools.len())
        } else {
            format!("toolchain: {} of {} tools missing", missing, self.tools.len())
        };
        // `ok` is true even with something missing: discovery succeeded, and
        // whether the absence matters is the binding's question, not this one.
        Record::outcome(step, at, true, summary).with_detail(self.render())
    }
}

/// What a notification is about (`X-5`, one implementation of `O-6`'s sink).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ApprovalNeeded { what: String },
    BatchBlocked { batch: String, why: String },
    BudgetHit { detail: String },
    CycleComplete { cycle: u32 },
}

impl Event {
    pub fn title(&self) -> String {
        match self {
            Event::ApprovalNeeded { .. } => "perp — approval needed".into(),
            Event::BatchBlocked { batch, .. } => format!("perp — batch {batch} blocked"),
            Event::BudgetHit { .. } => "perp — budget reached".into(),
            Event::CycleComplete { cycle } => format!("perp — cycle {cycle} complete"),
        }
    }

    pub fn body(&self) -> String {
        match self {
            Event::ApprovalNeeded { what } => {
                format!("{what}\n\nApprove at the machine — approvals never arrive over the network (`O-6`).")
            }
            Event::BatchBlocked { why, .. } => why.clone(),
            Event::BudgetHit { detail } => detail.clone(),
            Event::CycleComplete { cycle } => format!("Cycle {cycle} closed."),
        }
    }

    /// Notifications are **outbound only** (`O-6`). This type has no reply
    /// path, deliberately: the only thing that may come back is a `/btw`, and
    /// that arrives through `btw::Queue` with its own rules — including that it
    /// cannot cross the approval boundary (`C-10`).
    pub fn is_outbound_only(&self) -> bool {
        true
    }
}

/// Where a notification goes. Pluggable per `O-6`; the OS notifier is `X-5`.
pub trait Sink: fmt::Debug {
    fn notify(&self, event: &Event) -> Result<()>;
}

/// Writes to stdout. The sink that always works, including over ssh with no
/// desktop session — which is how an unattended rig is usually reached.
#[derive(Debug, Default)]
pub struct Stdout;

impl Sink for Stdout {
    fn notify(&self, event: &Event) -> Result<()> {
        println!("[{}] {}", event.title(), event.body());
        Ok(())
    }
}

/// The desktop notifier for the platform (`X-5`).
#[derive(Debug, Clone)]
pub struct Desktop {
    timeout: std::time::Duration,
}

impl Default for Desktop {
    fn default() -> Desktop {
        Desktop { timeout: std::time::Duration::from_secs(10) }
    }
}

impl Desktop {
    /// The command for this platform, or `None` where there is no notifier the
    /// harness can rely on. Returned rather than run, so it is testable without
    /// a desktop session.
    pub fn command(title: &str, body: &str) -> Option<String> {
        let title = shell_quote(title);
        let body = shell_quote(body);
        if cfg!(target_os = "windows") {
            // BurntToast is not present by default, and `msg` needs a session.
            // PowerShell's toast API is the one that is always there.
            Some(format!(
                "powershell -NoProfile -Command \"[Windows.UI.Notifications.ToastNotificationManager]\
                 ::CreateToastNotifier('perp').Show([Windows.UI.Notifications.ToastNotification]::new(\
                 ([xml]'<toast><visual><binding template=\\\"ToastGeneric\\\"><text>{title}</text>\
                 <text>{body}</text></binding></visual></toast>').DocumentElement)))\""
            ))
        } else if cfg!(target_os = "macos") {
            Some(format!(
                "osascript -e 'display notification \"{body}\" with title \"{title}\"'"
            ))
        } else {
            Some(format!("notify-send \"{title}\" \"{body}\""))
        }
    }
}

impl Sink for Desktop {
    fn notify(&self, event: &Event) -> Result<()> {
        let Some(command) = Desktop::command(&event.title(), &event.body()) else {
            return Err(Error::refused("notify", "no desktop notifier on this platform"));
        };
        let spec = Spec::new(command, std::env::current_dir().unwrap_or_default(), self.timeout)
            .with_env(Env::declared());
        // A notifier that fails must not stop the loop. The point of the
        // notification is that nobody is watching; failing the batch because
        // the toast did not render inverts that entirely.
        let _ = crate::process::run(&spec);
        Ok(())
    }
}

fn shell_quote(text: &str) -> String {
    text.replace('"', "'").replace('\n', " ").chars().take(200).collect()
}

/// What a loop must re-check after the machine slept (`X-10`).
///
/// A process that wakes up and carries on is a process reasoning from facts
/// that were true hours ago: the peer is gone, the token expired, the clock
/// jumped. None of those announce themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeCheck {
    /// The gap between the clock the loop last saw and the clock now.
    pub clock_jumped_seconds: i64,
    pub must_reprobe_links: bool,
    pub must_reconcile: bool,
    pub reasons: Vec<String>,
}

/// How long a gap counts as "the machine was asleep" rather than "a step took a
/// while". Ten minutes: longer than any gate, shorter than a lunch break.
pub const SLEEP_GAP_SECONDS: i64 = 600;

impl WakeCheck {
    /// `last_seen` is the timestamp of the last journal record; `now` is the
    /// clock on waking.
    pub fn after(last_seen: i64, now: i64) -> WakeCheck {
        let gap = now - last_seen;
        let mut reasons = Vec::new();

        if gap < 0 {
            // Backwards. An NTP correction, a VM restore, a dual-boot. Every
            // duration the loop computed is now wrong in a direction that
            // makes things look faster than they were.
            reasons.push(format!("the clock moved backwards by {}s", -gap));
        } else if gap > SLEEP_GAP_SECONDS {
            reasons.push(format!("{gap}s passed with nothing recorded — the machine slept"));
        }

        let slept = !reasons.is_empty();
        if slept {
            reasons.push("link health is stale: a peer may be gone and a token may have expired".into());
            reasons.push("the step in flight, if any, is reconciled before anything else (`L-7`)".into());
        }

        WakeCheck {
            clock_jumped_seconds: gap,
            must_reprobe_links: slept,
            must_reconcile: slept,
            reasons,
        }
    }

    pub fn is_clean(&self) -> bool {
        self.reasons.is_empty()
    }

    pub fn record(&self, step: StepId, at: i64) -> Record {
        Record::outcome(
            step,
            at,
            true,
            format!("woke after {}s — reconciling before continuing", self.clock_jumped_seconds),
        )
        .with_detail(self.reasons.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing() -> ReadAllowlist {
        ReadAllowlist::default()
    }

    #[test]
    fn the_workspace_root_is_the_boundary() {
        let inside = PathBuf::from("crates/perp-core/src/os.rs");
        let outside = PathBuf::from("/etc/shadow");

        for capability in [
            Capability::Read { path: inside.clone(), inside: true },
            Capability::Write { path: inside.clone(), inside: true },
            Capability::Delete { path: inside, inside: true },
        ] {
            assert_eq!(classify(&capability, &nothing()), Policy::Auto, "{capability:?}");
        }

        assert!(matches!(
            classify(&Capability::Read { path: outside.clone(), inside: false }, &nothing()),
            Policy::Approve { .. }
        ));
        assert!(matches!(
            classify(&Capability::Write { path: outside.clone(), inside: false }, &nothing()),
            Policy::Approve { .. }
        ));
    }

    #[test]
    fn deleting_outside_the_workspace_is_never_and_no_list_unlocks_it() {
        let outside = PathBuf::from("/home/someone/photos");
        // Even allowlisted for reading.
        let generous = ReadAllowlist::new(vec![PathBuf::from("/home")]);
        let policy = classify(&Capability::Delete { path: outside, inside: false }, &generous);
        let Policy::Never { reason } = policy else { panic!("must be Never: {policy:?}") };
        assert!(reason.contains("X-2"), "{reason}");
    }

    #[test]
    fn an_allowlisted_read_outside_the_workspace_is_free() {
        let allowlist = ReadAllowlist::from_entries(&[(
            "os.read.allow".to_string(),
            "C:/toolchains, /usr/share".to_string(),
        )]);
        let allowed = Capability::Read { path: PathBuf::from("/usr/share/dict/words"), inside: false };
        assert_eq!(classify(&allowed, &allowlist), Policy::Auto);

        let elsewhere = Capability::Read { path: PathBuf::from("/root/.ssh/id_rsa"), inside: false };
        assert!(matches!(classify(&elsewhere, &allowlist), Policy::Approve { .. }));
    }

    #[test]
    fn the_clipboard_is_asymmetric_because_reading_it_takes_user_data() {
        assert_eq!(classify(&Capability::ClipboardWrite, &nothing()), Policy::Auto);
        let Policy::Approve { reason } = classify(&Capability::ClipboardRead, &nothing()) else {
            panic!("reading must be asked");
        };
        assert!(reason.contains("did not create"), "{reason}");
    }

    #[test]
    fn localhost_opens_freely_and_the_internet_does_not() {
        let auto = |target: &str| {
            classify(&Capability::Open { target: target.into() }, &nothing()) == Policy::Auto
        };
        assert!(auto("http://localhost:1420/"));
        assert!(auto("http://127.0.0.1:5173/index.html"));
        assert!(auto(".harness/artifacts/board.html"));

        assert!(!auto("https://example.com/"), "the internet is asked (`X-7`)");
        assert!(!auto("mailto:someone@example.com"), "a mail draft is not a file");
        assert!(!auto("../../etc/passwd"), "traversal is not workspace-relative");
        assert!(
            !auto("http://localhost.evil.test/"),
            "a host that merely starts with localhost is not localhost"
        );
    }

    #[test]
    fn scheduling_is_asked_because_it_outlives_the_session() {
        let policy = classify(&Capability::Schedule { when: "at boot".into() }, &nothing());
        let Policy::Approve { reason } = policy else { panic!("must be asked") };
        assert!(reason.contains("outlives"), "{reason}");
    }

    #[test]
    fn gui_automation_is_never_rather_than_approve() {
        // `X-11`. A capability that does not exist cannot be reached by an
        // approval that should not have been given.
        let policy = classify(&Capability::GuiAutomation, &nothing());
        assert!(matches!(policy, Policy::Never { .. }), "{policy:?}");
    }

    #[test]
    fn a_missing_tool_is_recorded_rather_than_skipped() {
        // An absent entry reads as "not asked"; a recorded absence is a fact.
        let toolchain = Toolchain {
            tools: vec![
                Tool::found("cargo", "cargo 1.97.0 (abc 2026-01-01)"),
                Tool::missing("node", "exit 127"),
            ],
        };
        assert_eq!(toolchain.missing().len(), 1);
        let record = toolchain.record(StepId::new(4, "b16", 1).expect("step"), 1_700_000_000);
        assert_eq!(record.ok, Some(true), "discovery succeeded even though a tool is absent");
        assert!(record.summary.contains("1 of 2 tools missing"), "{}", record.summary);
        let detail = record.detail.expect("detail");
        assert!(detail.contains("cargo 1.97.0"), "the version verbatim: {detail}");
        assert!(detail.contains("node: not found — exit 127"), "{detail}");
    }

    #[test]
    fn discovery_actually_asks_and_records_what_it_got() {
        // The red run found this gap: every other test built a `Toolchain` by
        // hand, so `discover` — the part that talks to the machine — was
        // untested, and a mutation that reported every tool as present passed.
        let names = vec!["cargo".to_string(), "perp-not-a-real-tool".to_string()];
        let toolchain = Toolchain::discover(
            &names,
            &std::env::current_dir().unwrap_or_default(),
            std::time::Duration::from_secs(30),
        );

        let cargo = &toolchain.tools[0];
        assert!(cargo.is_present(), "cargo runs the test suite, so it is on this machine");
        let version = cargo.version.clone().unwrap_or_default();
        assert!(version.starts_with("cargo "), "the real output, verbatim: {version}");

        let absent = &toolchain.tools[1];
        assert!(!absent.is_present(), "a tool that is not there must not be reported present");
        assert!(absent.missing.is_some(), "and the reason is recorded: {absent:?}");
        assert_eq!(toolchain.missing().len(), 1);
    }

    #[test]
    fn the_toolchain_list_comes_from_the_binding() {
        let names = Toolchain::names_from_entries(&[
            ("toolchain".to_string(), "cargo, git , node".to_string()),
            ("path.requirements".to_string(), ".harness/perpetum.md".to_string()),
        ]);
        assert_eq!(names, ["cargo", "git", "node"]);
    }

    #[test]
    fn a_notification_says_to_walk_to_the_machine() {
        // `O-6`: approvals never arrive over the network, so the notification
        // that says one is needed must not read like something you can answer.
        let event = Event::ApprovalNeeded { what: "push to origin".into() };
        assert!(event.body().contains("Approve at the machine"), "{}", event.body());
        assert!(event.body().contains("never arrive over the network"));
        assert!(event.is_outbound_only());
    }

    #[test]
    fn every_notifiable_event_has_a_title_and_a_body() {
        for event in [
            Event::ApprovalNeeded { what: "x".into() },
            Event::BatchBlocked { batch: "b16".into(), why: "no GPU".into() },
            Event::BudgetHit { detail: "cycle money".into() },
            Event::CycleComplete { cycle: 4 },
        ] {
            assert!(event.title().starts_with("perp"), "{}", event.title());
            assert!(!event.body().is_empty(), "{event:?}");
        }
    }

    #[test]
    fn the_desktop_command_carries_the_text_and_survives_quotes() {
        let command = Desktop::command("perp — approval needed", "push to \"origin\"\nnow")
            .expect("a notifier on every supported platform");
        assert!(command.contains("approval needed"), "{command}");
        assert!(!command.contains("\"origin\""), "quotes would break the command: {command}");
        assert!(!command.contains('\n'), "and so would a newline: {command}");
    }

    #[test]
    fn waking_after_a_long_gap_forces_a_reconcile() {
        let clean = WakeCheck::after(1_700_000_000, 1_700_000_030);
        assert!(clean.is_clean(), "half a minute is a slow step, not a sleep");
        assert!(!clean.must_reprobe_links);

        let slept = WakeCheck::after(1_700_000_000, 1_700_000_000 + 8 * 3600);
        assert!(!slept.is_clean());
        assert!(slept.must_reconcile && slept.must_reprobe_links);
        assert!(
            slept.reasons.iter().any(|r| r.contains("token may have expired")),
            "{:?}",
            slept.reasons
        );
    }

    #[test]
    fn a_clock_that_moved_backwards_is_caught_too() {
        // An NTP correction, a VM restore, a dual boot. Every duration the loop
        // computed is now wrong in the direction that flatters it.
        let back = WakeCheck::after(1_700_000_000, 1_700_000_000 - 4_000);
        assert!(!back.is_clean());
        assert!(back.reasons[0].contains("backwards"), "{:?}", back.reasons);
        assert!(back.must_reconcile);
    }
}
