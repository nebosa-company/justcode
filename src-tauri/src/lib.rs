use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// Reads a UTF-8 source file chosen by the user in the open dialog.
#[tauri::command(async)]
fn read_text_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("{e}"))
}

/// Writes the editor buffer back to disk, creating parent folders if the user
/// picked a path inside a directory that does not exist yet.
/// Written to a sibling temp file and renamed over the target, so a failure
/// part-way through (disk full, share dropped, antivirus lock) leaves the
/// previous file intact rather than truncating it to nothing.
#[tauri::command(async)]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| format!("{e}"))?;
        }
    }

    let temp = {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".justcode-tmp");
        path.with_file_name(name)
    };
    fs::write(&temp, contents).map_err(|e| format!("{e}"))?;
    // Windows `rename` fails if the destination exists, so replace explicitly.
    if let Err(error) = fs::rename(&temp, &path) {
        if path.exists() {
            let backup = fs::read(&path).ok();
            fs::remove_file(&path).map_err(|e| format!("{e}"))?;
            if let Err(error) = fs::rename(&temp, &path) {
                // Put the original back rather than leaving nothing behind.
                if let Some(bytes) = backup {
                    let _ = fs::write(&path, bytes);
                }
                let _ = fs::remove_file(&temp);
                return Err(format!("{error}"));
            }
        } else {
            let _ = fs::remove_file(&temp);
            return Err(format!("{error}"));
        }
    }
    Ok(())
}

/// Keeps a caller-supplied name usable as a single path component.
fn safe_stem(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(head, _)| head);
    let cleaned: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    if cleaned.is_empty() {
        "preview".to_string()
    } else {
        cleaned
    }
}

/// Writes rendered Markdown to a temp file and returns the path, so the preview
/// can be handed to the browser without littering the user's project folder.
#[tauri::command(async)]
fn write_preview(name: String, html: String) -> Result<String, String> {
    let mut dir = std::env::temp_dir();
    dir.push("justcode-preview");
    fs::create_dir_all(&dir).map_err(|e| format!("{e}"))?;
    // A hash of the full name keeps distinct documents from colliding on the
    // same file and keeps the stem clear of Windows reserved names (con, nul…),
    // while re-previewing the same document reuses its file rather than piling
    // up temp copies.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    let path = dir.join(format!("{}-{:x}.html", safe_stem(&name), hasher.finish()));
    fs::write(&path, html).map_err(|e| format!("{e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Hands an HTML file to the OS shell, which opens it in the default browser.
///
/// Only `.html`/`.htm`/`.xhtml` are allowed: `open_path` uses the default shell
/// association, so passing an executable extension (`.bat`, `.exe`, …) would
/// *run* it. Restricting to web pages keeps this a preview command, not an
/// arbitrary launcher.
#[tauri::command(async)]
fn open_in_browser(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    // Canonicalize *first*, then check the extension. The other way round, a
    // `preview.html` symlink pointing at `payload.exe` passes the check and the
    // shell then opens — that is, runs — the resolved target.
    let canonical = fs::canonicalize(&path).unwrap_or(path);
    let ext = canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("html") | Some("htm") | Some("xhtml")) {
        return Err(format!(
            "Only HTML files can be opened in the browser: {}",
            canonical.display()
        ));
    }
    // Windows canonicalization yields a verbatim prefix browsers reject: turn
    // \\?\UNC\server\share into \\server\share, and \\?\C:\… into C:\….
    let target = canonical
        .to_string_lossy()
        .replace(r"\\?\UNC\", r"\\")
        .replace(r"\\?\", "");
    app.opener()
        .open_path(target, None::<&str>)
        .map_err(|e| format!("{e}"))
}

/// Opens a web URL in the default browser. Restricted to web schemes so a
/// Ctrl-clicked link in a document can never launch an arbitrary handler.
#[tauri::command(async)]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = url.trim().to_string();
    let scheme = url.to_ascii_lowercase();
    let allowed = scheme.starts_with("http://")
        || scheme.starts_with("https://")
        || scheme.starts_with("mailto:");
    if !allowed {
        return Err(format!("Refusing to open non-web URL: {url}"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("{e}"))
}

/// Selects a file in the platform's file manager — Explorer on Windows, Finder
/// on macOS, whichever manager is configured on Linux.
#[tauri::command(async)]
fn reveal_in_file_manager(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("{} does not exist", target.display()));
    }
    app.opener()
        .reveal_item_in_dir(target)
        .map_err(|e| format!("{e}"))
}

/// Runs a script in its own console window, from the folder it lives in, so its
/// output stays on screen after it finishes. `kind` picks the interpreter; only
/// the shells below are accepted.
///
/// The file name is never handed to a command interpreter as text. This used to
/// go through `cmd /c start <shell> …`, which parsed the path three times over,
/// so a file called `build&calc&.ps1` ran `calc` whatever the script contained.
/// PowerShell now takes the path as a literal `-File` argument, and the batch
/// case expands it from the environment (see below) — both verified against a
/// file name carrying an injection payload.
#[tauri::command(async)]
fn run_script(path: String, kind: String) -> Result<(), String> {
    let script = PathBuf::from(&path);
    if !script.is_file() {
        return Err(format!("{} does not exist", script.display()));
    }
    let folder = script.parent().unwrap_or_else(|| std::path::Path::new("."));

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

        let mut command = match kind.as_str() {
            "powershell" => {
                let mut c = std::process::Command::new("powershell.exe");
                // -NoExit keeps the window up so output can be read.
                c.args(["-NoExit", "-ExecutionPolicy", "Bypass", "-File"]);
                c.arg(&script);
                c
            }
            "batch" => {
                // `cmd /k <path>` is NOT safe even when the path is quoted:
                // /k takes its argument as a *command line* and re-parses it,
                // so `ok&md PWNED&rem .cmd` runs `md`. Verified.
                //
                // Passing the command through the environment and expanding it
                // with delayed expansion (`!VAR!`) keeps it out of the parser
                // entirely — the value is substituted after the line has been
                // tokenised, so `&` in a file name stays part of the name.
                //
                // The *whole* command lives in the variable, not just the path.
                // With `call "!JUSTCODE_SCRIPT!"` on the command line the argv
                // quoter escapes the inner quotes to `\"`, which cmd treats as
                // literal characters — safe, but the script never ran.
                let mut c = std::process::Command::new("cmd.exe");
                c.env("JUSTCODE_CMD", format!("call \"{}\"", script.display()));
                c.args(["/v:on", "/k", "!JUSTCODE_CMD!"]);
                c
            }
            // .sh has no interpreter on a stock Windows box; say so plainly
            // rather than failing with a confusing "unknown kind".
            "shell" => {
                return Err("Shell scripts need WSL or Git Bash, which JustCode does not launch".into())
            }
            _ => return Err(format!("Don't know how to run '{kind}' scripts")),
        };
        command.creation_flags(CREATE_NEW_CONSOLE).current_dir(folder);
        command.spawn().map_err(|e| format!("{e}"))?;
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let program = match kind.as_str() {
            "powershell" => "pwsh",
            "shell" => "sh",
            _ => return Err(format!("Don't know how to run '{kind}' scripts")),
        };
        let mut child = std::process::Command::new(program)
            .arg(&script)
            .current_dir(folder)
            .spawn()
            .map_err(|e| format!("{e}"))?;
        // Reap it in the background rather than leaving a zombie behind.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

/// Picks the file paths out of a command line, ignoring flags and anything that
/// is not actually a file on disk.
fn files_from_args<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .filter(|arg| PathBuf::from(arg).is_file())
        .collect()
}

/// Files passed on the command line — how Explorer hands over a double-clicked
/// document once the extensions are associated with the app. Uses `args_os` so a
/// path that is not valid Unicode is decoded lossily rather than panicking.
#[tauri::command]
fn startup_files() -> Vec<String> {
    files_from_args(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()))
}

/// Wall-clock start of the process, used to measure how long booting takes.
struct BootClock(std::time::Instant);

/// Shows the window, maximised.
///
/// Maximising is done here rather than through `"maximized": true` in the
/// config: on Windows that setting is applied with `ShowWindow`, which reveals
/// the window immediately and defeats `"visible": false` — the frame appeared
/// about half a second before the editor inside it was ready.
fn reveal(window: &tauri::WebviewWindow) {
    let _ = window.maximize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// Called by the frontend once the editor is on screen. Returns the boot time in
/// milliseconds and also records it next to the temp files, so a build can be
/// timed without attaching a debugger to the webview.
#[tauri::command]
fn report_ready(app: tauri::AppHandle, clock: tauri::State<BootClock>, detail: String) -> u128 {
    let ms = clock.0.elapsed().as_millis();
    let mut path = std::env::temp_dir();
    path.push("justcode-boot.txt");
    let _ = fs::write(&path, format!("total={ms} {detail}"));
    if let Some(window) = app.get_webview_window("main") {
        reveal(&window);
    }
    ms
}

// ------------------------------------------------------------- file associations
//
// Everything below writes only to HKEY_CURRENT_USER\Software\Classes, which
// needs no elevation and affects only the signed-in user.
//
// A caveat worth knowing: since Windows 8, the *default* handler for an
// extension is pinned by a hash-protected `UserChoice` key that applications
// are not allowed to forge. So registering here makes JustCode the handler for
// extensions the user has never opened before, and always adds it to the
// "Open with" list — but where Windows has already recorded a choice, that
// choice stands and the user has to change it from the Explorer dialog.

#[cfg(windows)]
mod associations {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    /// The ProgID JustCode registers an extension under, e.g. `JustCode.pas`.
    fn prog_id(extension: &str) -> String {
        format!("JustCode.{extension}")
    }

    fn exe() -> Result<String, String> {
        std::env::current_exe()
            .map_err(|e| format!("{e}"))
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// The value the previous owner's ProgID is stashed under, so unticking an
    /// extension can hand it back instead of leaving it blank.
    const BACKUP: &str = "JustCode.previous";

    /// Normalises an extension to the form every function here expects.
    fn normalise(extension: &str) -> Result<String, String> {
        let extension = extension.trim_start_matches('.').to_lowercase();
        if extension.is_empty() || !extension.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!("refusing to register odd extension {extension:?}"));
        }
        Ok(extension)
    }

    /// Creates the ProgID and points the extension at it.
    ///
    /// The ProgID is written in full *before* the extension is pointed at it,
    /// so a failure part-way through cannot leave a file type aimed at a
    /// half-built handler; on any error the partial ProgID is removed again.
    pub fn associate(extension: &str, description: &str) -> Result<(), String> {
        let extension = normalise(extension)?;
        let exe = exe()?;
        let classes = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)
            .map_err(|e| format!("{e}"))?;
        let id = prog_id(&extension);

        let build = || -> Result<(), String> {
            let (key, _) = classes.create_subkey(&id).map_err(|e| format!("{e}"))?;
            key.set_value("", &description).map_err(|e| format!("{e}"))?;
            let (icon, _) = key.create_subkey("DefaultIcon").map_err(|e| format!("{e}"))?;
            icon.set_value("", &format!("\"{exe}\",0"))
                .map_err(|e| format!("{e}"))?;
            let (command, _) = key
                .create_subkey("shell\\open\\command")
                .map_err(|e| format!("{e}"))?;
            command
                .set_value("", &format!("\"{exe}\" \"%1\""))
                .map_err(|e| format!("{e}"))
        };
        if let Err(error) = build() {
            let _ = classes.delete_subkey_all(&id);
            return Err(error);
        }

        let (ext_key, _) = classes
            .create_subkey(format!(".{extension}"))
            .map_err(|e| format!("{e}"))?;
        // Remember whoever had it, so disassociate can give it back. Only the
        // first time — re-applying must not record our own ProgID as the
        // "previous" one.
        if let Ok(current) = ext_key.get_value::<String, _>("") {
            let ours = current == id;
            let already = ext_key.get_value::<String, _>(BACKUP).is_ok();
            if !ours && !already && !current.is_empty() {
                let _ = ext_key.set_value(BACKUP, &current);
            }
        }
        ext_key.set_value("", &id).map_err(|e| format!("{e}"))?;
        // Listing the ProgID here is what puts JustCode in "Open with" even
        // when another application holds the default.
        let (open_with, _) = ext_key
            .create_subkey("OpenWithProgids")
            .map_err(|e| format!("{e}"))?;
        open_with.set_value(&id, &"").map_err(|e| format!("{e}"))
    }

    /// Removes the ProgID again, and the extension's pointer to it if it still
    /// points at us — another application's association is left alone.
    pub fn disassociate(extension: &str) -> Result<(), String> {
        let extension = normalise(extension)?;
        let classes = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)
            .map_err(|e| format!("{e}"))?;
        let id = prog_id(&extension);

        if let Ok(ext_key) = classes.open_subkey_with_flags(
            format!(".{extension}"),
            KEY_READ | KEY_WRITE,
        ) {
            if ext_key.get_value::<String, _>("").ok().as_deref() == Some(id.as_str()) {
                // Hand the file type back to whoever had it before us rather
                // than leaving it pointing at nothing.
                match ext_key.get_value::<String, _>(BACKUP) {
                    Ok(previous) if !previous.is_empty() => {
                        let _ = ext_key.set_value("", &previous);
                    }
                    _ => {
                        let _ = ext_key.set_value("", &"");
                    }
                }
            }
            let _ = ext_key.delete_value(BACKUP);
            if let Ok(open_with) = ext_key
                .open_subkey_with_flags("OpenWithProgids", KEY_READ | KEY_WRITE)
            {
                let _ = open_with.delete_value(&id);
            }
        }
        let _ = classes.delete_subkey_all(&id);
        Ok(())
    }

    /// Which of `extensions` another application already claims, with the
    /// ProgID that claims it. Used to avoid quietly taking a file type away
    /// from a tool the user already relies on.
    pub fn claimed_elsewhere(extensions: &[String]) -> Vec<(String, String)> {
        let Ok(classes) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Classes") else {
            return Vec::new();
        };
        extensions
            .iter()
            .filter_map(|extension| {
                let normalised = normalise(extension).ok()?;
                let value: String = classes
                    .open_subkey(format!(".{normalised}"))
                    .ok()?
                    .get_value("")
                    .ok()?;
                if value.is_empty() || value == prog_id(&normalised) {
                    None
                } else {
                    Some((extension.clone(), value))
                }
            })
            .collect()
    }

    /// Extensions where Windows has recorded the user's own default app.
    ///
    /// That `UserChoice` key outranks anything under `Software\Classes`, and it
    /// is hash-protected precisely so applications cannot forge it — so these
    /// are the ones that will keep opening elsewhere no matter what we write.
    pub fn overridden_by_user_choice(extensions: &[String]) -> Vec<String> {
        let Ok(file_exts) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts",
        ) else {
            return Vec::new();
        };
        extensions
            .iter()
            .filter(|extension| {
                let Ok(extension) = normalise(extension) else { return false };
                file_exts
                    .open_subkey(format!(".{extension}\\UserChoice"))
                    .and_then(|key| key.get_value::<String, _>("ProgId"))
                    .map(|chosen| chosen != prog_id(&extension))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Registers JustCode as an *application*, not just as a handler for
    /// individual file types.
    ///
    /// Writing `Software\\Classes\\.ext` and a ProgID is enough for a
    /// double-click to work, but it does not put the app anywhere Windows can
    /// offer it: the "Open with" list is built from
    /// `Classes\\Applications\\<exe>`, and the Settings ▸ Default apps page from
    /// `RegisteredApplications` pointing at a Capabilities key. Without these
    /// two, JustCode is missing from every picker — and a file type it has not
    /// been made the default for silently opens the chooser instead.
    pub fn register_application(extensions: &[String]) -> Result<(), String> {
        let exe = exe()?;
        let file_name = std::path::Path::new(&exe)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "justcode.exe".to_string());

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let classes = hkcu
            .open_subkey_with_flags("Software\\Classes", KEY_READ | KEY_WRITE)
            .map_err(|e| format!("{e}"))?;

        // What the "Open with" list reads.
        let (app, _) = classes
            .create_subkey(format!("Applications\\{file_name}"))
            .map_err(|e| format!("{e}"))?;
        app.set_value("FriendlyAppName", &"JustCode")
            .map_err(|e| format!("{e}"))?;
        let (icon, _) = app.create_subkey("DefaultIcon").map_err(|e| format!("{e}"))?;
        icon.set_value("", &format!("\"{exe}\",0"))
            .map_err(|e| format!("{e}"))?;
        let (command, _) = app
            .create_subkey("shell\\open\\command")
            .map_err(|e| format!("{e}"))?;
        command
            .set_value("", &format!("\"{exe}\" \"%1\""))
            .map_err(|e| format!("{e}"))?;
        // Listing the types keeps the app out of pickers for unrelated files.
        let (supported, _) = app.create_subkey("SupportedTypes").map_err(|e| format!("{e}"))?;
        for extension in extensions {
            if let Ok(extension) = normalise(extension) {
                let _ = supported.set_value(format!(".{extension}"), &"");
            }
        }

        // What Settings ▸ Default apps reads.
        let (caps, _) = hkcu
            .create_subkey("Software\\JustCode\\Capabilities")
            .map_err(|e| format!("{e}"))?;
        caps.set_value("ApplicationName", &"JustCode")
            .map_err(|e| format!("{e}"))?;
        caps.set_value("ApplicationDescription", &"A small, fast code editor")
            .map_err(|e| format!("{e}"))?;
        caps.set_value("ApplicationIcon", &format!("{exe},0"))
            .map_err(|e| format!("{e}"))?;
        let (file_assocs, _) = caps.create_subkey("FileAssociations").map_err(|e| format!("{e}"))?;
        for extension in extensions {
            if let Ok(extension) = normalise(extension) {
                let _ = file_assocs.set_value(format!(".{extension}"), &prog_id(&extension));
            }
        }
        let (registered, _) = hkcu
            .create_subkey("Software\\RegisteredApplications")
            .map_err(|e| format!("{e}"))?;
        registered
            .set_value("JustCode", &"Software\\JustCode\\Capabilities")
            .map_err(|e| format!("{e}"))
    }

    /// Which of `extensions` currently resolve to JustCode.
    pub fn current(extensions: &[String]) -> Vec<String> {
        let Ok(classes) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Software\\Classes") else {
            return Vec::new();
        };
        extensions
            .iter()
            .filter(|extension| {
                let Ok(extension) = normalise(extension) else { return false };
                classes
                    .open_subkey(format!(".{extension}"))
                    .and_then(|key| key.get_value::<String, _>(""))
                    .map(|value| value == prog_id(&extension))
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Tells Explorer to re-read associations, so icons update without a logout.
    pub fn notify_shell() {
        use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        unsafe {
            // The event id is declared as i32 here but the constant is u32.
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            );
        }
    }
}

// ------------------------------------------------------------------ terminals
//
// Each terminal is a real pseudo-terminal running a shell. The child's output
// is pumped to the frontend as `terminal-output` events, keyed by id, and
// xterm.js renders it; keystrokes come back through `terminal_write`.
//
// Elevated shells are deliberately not run here. A non-elevated process cannot
// read the pipes of an elevated child, so an "administrator" terminal cannot be
// embedded — `open_external_terminal` launches one in its own window instead.

/// One terminal's handles. Each is behind its own lock so a blocking write or a
/// `wait()` on one shell never blocks every other terminal — the registry lock
/// is only ever held long enough to clone an `Arc` out of it.
struct Terminal {
    writer: std::sync::Mutex<Box<dyn std::io::Write + Send>>,
    master: std::sync::Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    child: std::sync::Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
}

type TerminalMap = std::collections::HashMap<u32, std::sync::Arc<Terminal>>;

#[derive(Default)]
struct Terminals(std::sync::Mutex<TerminalMap>);

impl Terminals {
    /// The registry, recovering from a poisoned lock rather than bricking every
    /// terminal command for the rest of the session.
    fn map(&self) -> std::sync::MutexGuard<'_, TerminalMap> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self, id: u32) -> Result<std::sync::Arc<Terminal>, String> {
        self.map().get(&id).cloned().ok_or_else(|| "no such terminal".to_string())
    }
}

/// Kills and reaps a terminal's shell. Never called with the registry locked.
fn shut_down(terminal: &Terminal) {
    let mut child = terminal.child.lock().unwrap_or_else(|p| p.into_inner());
    let _ = child.kill();
    let _ = child.wait();
}

/// Maps a profile name to the command that starts it.
fn shell_command(profile: &str) -> Result<portable_pty::CommandBuilder, String> {
    use portable_pty::CommandBuilder;
    let mut command = match profile {
        #[cfg(windows)]
        "powershell" => CommandBuilder::new("powershell.exe"),
        #[cfg(windows)]
        "cmd" => CommandBuilder::new("cmd.exe"),
        #[cfg(not(windows))]
        "zsh" => CommandBuilder::new("zsh"),
        #[cfg(not(windows))]
        "bash" => CommandBuilder::new("bash"),
        #[cfg(not(windows))]
        "sh" => CommandBuilder::new("sh"),
        other => return Err(format!("unknown terminal profile {other:?}")),
    };
    // Start where the user's files are, not in the install directory.
    if let Some(home) = dirs_home() {
        command.cwd(home);
    }
    Ok(command)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Builds the command that runs `script` under `kind` inside a terminal.
///
/// Deliberately *not* "open a shell and type the path into it": a shell
/// re-parses whatever it is given, so a file called `build&calc&.ps1` would run
/// `calc`. Every case below hands the path over as an argument vector, or —
/// for `cmd`, whose `/k` re-parses its argument — through the environment with
/// delayed expansion, exactly as `run_script` does.
#[cfg(windows)]
fn script_command(kind: &str, script: &Path) -> Result<portable_pty::CommandBuilder, String> {
    use portable_pty::CommandBuilder;
    let mut command = match kind {
        "powershell" => {
            let mut c = CommandBuilder::new("powershell.exe");
            // -NoExit leaves a usable prompt behind once the script finishes.
            c.args(["-NoExit", "-ExecutionPolicy", "Bypass", "-File"]);
            c.arg(script);
            c
        }
        "batch" => {
            let mut c = CommandBuilder::new("cmd.exe");
            // The whole command goes in the variable, not just the path, so the
            // argument left on the command line — `!JUSTCODE_CMD!` — has no
            // spaces and no quotes for the argv quoter to mangle. Passing
            // `call "!JUSTCODE_SCRIPT!"` here instead gets escaped to
            // `call \"!JUSTCODE_SCRIPT!\"`, which cmd reads as a literal.
            // Delayed expansion still substitutes after the line is tokenised,
            // so an `&` in the file name never becomes an operator.
            c.env("JUSTCODE_CMD", format!("call \"{}\"", script.display()));
            c.args(["/v:on", "/k", "!JUSTCODE_CMD!"]);
            c
        }
        "shell" => {
            return Err("Shell scripts need WSL or Git Bash, which JustCode does not launch".into())
        }
        other => return Err(format!("Don't know how to run '{other}' scripts")),
    };
    if let Some(folder) = script.parent() {
        command.cwd(folder);
    }
    Ok(command)
}

#[cfg(not(windows))]
fn script_command(kind: &str, script: &Path) -> Result<portable_pty::CommandBuilder, String> {
    use portable_pty::CommandBuilder;
    let mut command = match kind {
        "powershell" => CommandBuilder::new("pwsh"),
        "shell" => CommandBuilder::new("sh"),
        other => return Err(format!("Don't know how to run '{other}' scripts")),
    };
    command.arg(script);
    if let Some(folder) = script.parent() {
        command.cwd(folder);
    }
    Ok(command)
}

/// Starts a shell in a pseudo-terminal and streams its output to the frontend.
///
/// With `script` set, the interpreter named by `profile` runs that file instead
/// of an interactive shell being started.
#[tauri::command(async)]
fn terminal_open(
    app: tauri::AppHandle,
    terminals: tauri::State<Terminals>,
    id: u32,
    profile: String,
    cwd: Option<String>,
    script: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    use portable_pty::{native_pty_system, PtySize};

    // Reusing an id would drop the previous `Terminal` without reaping it,
    // orphaning a live shell whose reader thread keeps emitting under the same
    // id. Reachable whenever the webview reloads, since its ids restart at 1.
    if terminals.map().contains_key(&id) {
        return Err(format!("terminal {id} is already open"));
    }

    let mut command = match &script {
        Some(script) => {
            let script = PathBuf::from(script);
            if !script.is_file() {
                return Err(format!("{} does not exist", script.display()));
            }
            script_command(&profile, &script)?
        }
        None => shell_command(&profile)?,
    };
    // A script already starts in its own folder; only a plain shell takes the
    // editor's current directory.
    if script.is_none() {
        if let Some(cwd) = cwd.filter(|path| PathBuf::from(path).is_dir()) {
            command.cwd(cwd);
        }
    }

    let pty = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("{e}"))?;
    let child = pty.slave.spawn_command(command).map_err(|e| format!("{e}"))?;
    // The slave handle must be dropped or the shell never sees end-of-input.
    drop(pty.slave);

    let mut reader = pty.master.try_clone_reader().map_err(|e| format!("{e}"))?;
    let writer = pty.master.take_writer().map_err(|e| format!("{e}"))?;

    // One thread per terminal, pumping bytes out as they arrive.
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        // A multi-byte character can straddle a read boundary; decoding each
        // chunk on its own would turn it into U+FFFD permanently, so an
        // incomplete tail is carried into the next read instead.
        let mut carry: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    carry.extend_from_slice(&buffer[..count]);
                    let text = match std::str::from_utf8(&carry) {
                        Ok(whole) => {
                            let owned = whole.to_owned();
                            carry.clear();
                            owned
                        }
                        Err(error) => {
                            let good = error.valid_up_to();
                            // Only a truncated final character is worth keeping;
                            // genuinely invalid bytes are replaced as before.
                            let keep = if error.error_len().is_none() {
                                carry.len() - good
                            } else {
                                0
                            };
                            let split = carry.len() - keep;
                            let text = String::from_utf8_lossy(&carry[..split]).into_owned();
                            carry.drain(..split);
                            text
                        }
                    };
                    if !text.is_empty() && handle.emit("terminal-output", (id, text)).is_err() {
                        break;
                    }
                }
            }
        }
        // The shell exited on its own; drop the registry entry here or its
        // pseudoconsole and process handle stay alive until the app quits.
        if let Some(state) = handle.try_state::<Terminals>() {
            let entry = state.map().remove(&id);
            if let Some(terminal) = entry {
                shut_down(&terminal);
            }
        }
        let _ = handle.emit("terminal-closed", id);
    });

    terminals.map().insert(
        id,
        std::sync::Arc::new(Terminal {
            writer: std::sync::Mutex::new(writer),
            master: std::sync::Mutex::new(pty.master),
            child: std::sync::Mutex::new(child),
        }),
    );
    Ok(())
}

/// Sends keystrokes to a terminal.
#[tauri::command(async)]
fn terminal_write(terminals: tauri::State<Terminals>, id: u32, data: String) -> Result<(), String> {
    let terminal = terminals.get(id)?;
    let mut writer = terminal.writer.lock().unwrap_or_else(|p| p.into_inner());
    writer.write_all(data.as_bytes()).map_err(|e| format!("{e}"))?;
    writer.flush().map_err(|e| format!("{e}"))
}

/// Tells the shell the window changed size, so it can re-wrap its output.
#[tauri::command(async)]
fn terminal_resize(
    terminals: tauri::State<Terminals>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    use portable_pty::PtySize;
    let terminal = terminals.get(id)?;
    let master = terminal.master.lock().unwrap_or_else(|p| p.into_inner());
    master
        .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("{e}"))
}

/// Ends a terminal and reaps its shell. The registry lock is released before
/// the wait, so a slow reap cannot stall every other terminal.
#[tauri::command(async)]
fn terminal_close(terminals: tauri::State<Terminals>, id: u32) -> Result<(), String> {
    let entry = terminals.map().remove(&id);
    if let Some(terminal) = entry {
        shut_down(&terminal);
    }
    Ok(())
}

/// Opens a shell in its own window — the only way to offer an elevated one,
/// since a non-elevated parent cannot read an elevated child's pipes.
#[tauri::command(async)]
fn open_external_terminal(profile: String, elevated: bool, cwd: Option<String>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;

        let program = match profile.as_str() {
            "powershell" => "powershell.exe",
            "cmd" => "cmd.exe",
            other => return Err(format!("unknown terminal profile {other:?}")),
        };
        let directory = cwd
            .filter(|path| PathBuf::from(path).is_dir())
            .or_else(|| dirs_home().map(|p| p.to_string_lossy().into_owned()))
            .unwrap_or_default();

        let wide = |text: &str| {
            std::ffi::OsStr::new(text)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        // "runas" is what raises the UAC prompt; "open" starts it normally.
        let verb = wide(if elevated { "runas" } else { "open" });
        let file = wide(program);
        let dir = wide(&directory);

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                std::ptr::null(),
                dir.as_ptr(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            )
        };
        // ShellExecuteW reports failure as a "handle" of 32 or less.
        if (result as isize) <= 32 {
            return Err(format!("could not start {program} (code {})", result as isize));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (profile, elevated, cwd);
        Err("External terminals are only implemented on Windows".into())
    }
}

#[cfg(all(test, windows))]
mod association_tests {
    use super::associations;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    /// Round-trips a throwaway extension so the registry writes are exercised
    /// for real. Uses a name nothing else could own, and cleans up after itself.
    #[test]
    fn associate_then_disassociate() {
        let extension = "justcodeselftest";
        let classes = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Classes")
            .expect("HKCU\\Software\\Classes should be readable");

        associations::associate(extension, "JustCode self test").expect("associate");

        let prog_id: String = classes
            .open_subkey(format!(".{extension}"))
            .expect("extension key")
            .get_value("")
            .expect("default value");
        assert_eq!(prog_id, "JustCode.justcodeselftest");

        let command: String = classes
            .open_subkey(format!("{prog_id}\\shell\\open\\command"))
            .expect("command key")
            .get_value("")
            .expect("command value");
        assert!(command.contains("\"%1\""), "command should pass the file: {command}");

        classes
            .open_subkey(format!(".{extension}\\OpenWithProgids"))
            .expect("OpenWithProgids")
            .get_value::<String, _>(&prog_id)
            .expect("listed in OpenWithProgids");

        assert_eq!(
            associations::current(&[extension.to_string()]),
            vec![extension.to_string()],
            "should report itself as associated"
        );

        associations::disassociate(extension).expect("disassociate");
        assert!(
            associations::current(&[extension.to_string()]).is_empty(),
            "should report nothing after removal"
        );
        assert!(
            classes.open_subkey(&prog_id).is_err(),
            "ProgID should be gone"
        );

        // Tidy up the extension key itself, which disassociate leaves behind
        // (blank) in case another application also listed itself under it.
        let _ = classes.delete_subkey_all(format!(".{extension}"));
    }
}

/// Reports which extensions JustCode is currently registered for.
#[tauri::command(async)]
fn associated_extensions(extensions: Vec<String>) -> Vec<String> {
    #[cfg(windows)]
    {
        associations::current(&extensions)
    }
    #[cfg(not(windows))]
    {
        let _ = extensions;
        Vec::new()
    }
}

/// Reports extensions another application currently handles, as `[ext, owner]`.
#[tauri::command(async)]
fn foreign_extensions(extensions: Vec<String>) -> Vec<(String, String)> {
    #[cfg(windows)]
    {
        associations::claimed_elsewhere(&extensions)
    }
    #[cfg(not(windows))]
    {
        let _ = extensions;
        Vec::new()
    }
}

/// Opens the Windows "Default apps" settings page, the only place a user can
/// override a `UserChoice` that an application is forbidden to write.
#[tauri::command(async)]
fn open_default_apps_settings() -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;

        let wide = |text: &str| {
            std::ffi::OsStr::new(text)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        };
        let verb = wide("open");
        let target = wide("ms-settings:defaultapps");
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            )
        };
        if (result as isize) <= 32 {
            return Err(format!("could not open settings (code {})", result as isize));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("Only available on Windows".into())
    }
}

/// Registers `associate` and unregisters `remove`, then refreshes the shell.
/// Returns the extensions that could not be written, with the reason.
#[tauri::command(async)]
fn set_file_associations(
    associate: Vec<(String, String)>,
    remove: Vec<String>,
) -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        let mut failures = Vec::new();

        // Register the app itself first, so it appears in "Open with" and in
        // Settings even for types the user leaves unticked.
        let known: Vec<String> = associate
            .iter()
            .map(|(extension, _)| extension.clone())
            .chain(remove.iter().cloned())
            .collect();
        if let Err(error) = associations::register_application(&known) {
            failures.push(format!("application registration: {error}"));
        }

        for (extension, description) in &associate {
            if let Err(error) = associations::associate(extension, description) {
                failures.push(format!(".{extension}: {error}"));
            }
        }
        for extension in &remove {
            if let Err(error) = associations::disassociate(extension) {
                failures.push(format!(".{extension}: {error}"));
            }
        }
        associations::notify_shell();
        // Anything Windows will keep sending elsewhere is reported separately,
        // prefixed so the frontend can tell it apart from a real failure.
        let wanted: Vec<String> = associate.iter().map(|(ext, _)| ext.clone()).collect();
        for extension in associations::overridden_by_user_choice(&wanted) {
            failures.push(format!("userchoice:{extension}"));
        }
        Ok(failures)
    }
    #[cfg(not(windows))]
    {
        let _ = (associate, remove);
        Err("File associations are only supported on Windows".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let boot_clock = BootClock(std::time::Instant::now());
    tauri::Builder::default()
        .manage(boot_clock)
        .manage(Terminals::default())
        .setup(|app| {
            // The window starts hidden so the first thing shown is the painted
            // editor rather than an empty white frame. The frontend reveals it
            // when ready; this is the fallback if that never happens.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(4));
                if let Some(window) = handle.get_webview_window("main") {
                    reveal(&window);
                }
            });
            Ok(())
        })
        // Must be registered first: a second launch (Explorer opening another
        // file) forwards its arguments here instead of starting a new window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let files = files_from_args(argv);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
                let _ = window.unminimize();
            }
            if !files.is_empty() {
                let _ = app.emit("open-files", files);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            read_text_file,
            write_text_file,
            write_preview,
            open_in_browser,
            open_url,
            run_script,
            reveal_in_file_manager,
            startup_files,
            report_ready,
            associated_extensions,
            foreign_extensions,
            open_default_apps_settings,
            set_file_associations,
            terminal_open,
            terminal_write,
            terminal_resize,
            terminal_close,
            open_external_terminal
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // The frontend closes terminals itself, but a webview crash or an
            // unhandled error there would otherwise leave shells running.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Terminals>() {
                    let open: Vec<_> = state.map().drain().map(|(_, t)| t).collect();
                    for terminal in open {
                        shut_down(&terminal);
                    }
                }
            }
        });
}

#[cfg(test)]
mod terminal_tests {
    use portable_pty::{native_pty_system, PtySize};
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    /// Exercises the same pty plumbing `terminal_open` uses: start a shell,
    /// send a command, and read its output back. Verifies the pipe wiring
    /// rather than the Tauri command wrapper around it.
    #[test]
    fn shell_echoes_a_command() {
        let pty = native_pty_system()
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .expect("openpty");

        let command = super::shell_command(if cfg!(windows) { "cmd" } else { "sh" })
            .expect("known profile");
        let mut child = pty.slave.spawn_command(command).expect("spawn shell");
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader().expect("reader");
        let mut writer = pty.master.take_writer().expect("writer");
        writer.write_all(b"echo JUSTCODE_PTY_OK\r\n").expect("write");
        writer.flush().expect("flush");

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut seen = String::new();
        let mut buffer = [0u8; 4096];
        while Instant::now() < deadline {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    seen.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    // Twice: once echoed by the shell, once as the result.
                    if seen.matches("JUSTCODE_PTY_OK").count() >= 2 {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();

        assert!(
            seen.contains("JUSTCODE_PTY_OK"),
            "shell should have run the command; saw: {seen:?}"
        );
    }

    /// The "Run in Terminal" path end to end: a real script file, started by
    /// the same command `terminal_open` builds, with its output read back off
    /// the pty. The file name carries an injection payload, so this also fails
    /// if the path is ever handed to a parser instead of being passed through.
    #[test]
    fn run_in_terminal_executes_the_script() {
        let dir = std::env::temp_dir().join("justcode-run-in-terminal-test");
        let _ = std::fs::create_dir_all(&dir);
        let evidence = dir.join("PWNED");
        let _ = std::fs::remove_dir_all(&evidence);

        let (name, body, kind) = if cfg!(windows) {
            ("ok&md PWNED&rem .cmd", "@echo off\r\necho JUSTCODE_RUN_OK\r\n", "batch")
        } else {
            ("ok;mkdir PWNED;: .sh", "echo JUSTCODE_RUN_OK\n", "shell")
        };
        let script = dir.join(name);
        std::fs::write(&script, body).expect("write script");

        let pty = native_pty_system()
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .expect("openpty");
        let command = super::script_command(kind, &script).expect("known kind");
        let mut child = pty.slave.spawn_command(command).expect("spawn interpreter");
        drop(pty.slave);

        let mut reader = pty.master.try_clone_reader().expect("reader");
        // The interpreter is left at a prompt on purpose (`/k`, `-NoExit`), and
        // on Windows killing it does not close the ConPTY master — so a read on
        // this thread would block past any deadline. Read on its own thread and
        // bound the wait here instead.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buffer[..n]).into_owned();
                        if tx.send(chunk).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut seen = String::new();
        while Instant::now() < deadline && !seen.contains("JUSTCODE_RUN_OK") {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(chunk) => seen.push_str(&chunk),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();

        let injected = evidence.is_dir();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(!injected, "the file name was re-parsed as a command");
        assert!(
            seen.contains("JUSTCODE_RUN_OK"),
            "the script should have run; saw: {seen:?}"
        );
    }

    #[test]
    fn unknown_profile_is_rejected() {
        assert!(super::shell_command("evil --rm-rf").is_err());
    }

    #[test]
    fn unknown_script_kind_is_rejected() {
        let script = std::path::Path::new("x");
        assert!(super::script_command("evil --rm-rf", script).is_err());
    }

    /// "Run in Terminal" must not degrade into typing the path at a prompt.
    /// PowerShell takes it as a literal `-File` argument; cmd never sees it at
    /// all, because `/k` re-parses its argument and would run the `&calc&` in
    /// this name. Both are the same defence `run_script` uses.
    #[cfg(windows)]
    #[test]
    fn script_command_keeps_the_file_name_out_of_the_command_line() {
        let script = std::path::Path::new(r"C:\tmp\ok&calc&.ps1");
        let argv = |kind| {
            super::script_command(kind, script)
                .expect("known kind")
                .get_argv()
                .iter()
                .map(|part| part.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };

        let powershell = argv("powershell");
        assert_eq!(powershell.last().map(String::as_str), Some(r"C:\tmp\ok&calc&.ps1"));
        assert!(powershell.contains(&"-File".to_string()));

        let batch = argv("batch");
        assert!(
            !batch.iter().any(|part| part.contains("calc")),
            "the path reached cmd's command line: {batch:?}"
        );
        assert!(batch.contains(&"/v:on".to_string()));
    }
}

#[cfg(test)]
mod startup_arg_tests {
    use super::files_from_args;

    /// Explorer launches the registered command as `justcode.exe "C:\path\x.ps1"`,
    /// so argv[0] is the executable and the file is argv[1]. The same shape is
    /// forwarded by the single-instance plugin for a second launch.
    #[test]
    fn takes_the_first_argument_as_a_file() {
        let script = std::env::temp_dir().join("justcode-arg-test.ps1");
        std::fs::write(&script, "Write-Host 'hi'").expect("write temp script");
        let path = script.to_string_lossy().into_owned();

        let argv = vec![r"C:\Program Files\JustCode\justcode.exe".to_string(), path.clone()];
        assert_eq!(files_from_args(argv), vec![path.clone()]);

        // Flags are ignored, and several files may arrive at once.
        let argv = vec![
            "justcode.exe".to_string(),
            "--some-flag".to_string(),
            path.clone(),
            r"C:\does
ot\exist.ps1".to_string(),
        ];
        assert_eq!(files_from_args(argv), vec![path.clone()]);

        // The executable itself is never treated as a file to open.
        let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        assert!(files_from_args(vec![exe]).is_empty());

        let _ = std::fs::remove_file(&script);
    }
}

#[cfg(all(test, windows))]
mod launch_safety_tests {
    /// A file name may legally contain `&`, and `cmd` re-parses anything handed
    /// to `/k` or `start`. This pins the two launch shapes that were verified
    /// safe, so a future simplification back to `cmd /k <path>` fails here
    /// rather than silently restoring the injection.
    #[test]
    fn batch_launch_does_not_reparse_the_file_name() {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

        let dir = std::env::temp_dir().join("justcode-inj-test");
        let _ = std::fs::create_dir_all(&dir);
        let evidence = dir.join("PWNED");
        let _ = std::fs::remove_dir_all(&evidence);

        // Payload uses only characters that are legal in a Windows file name.
        let proof = dir.join("RAN");
        let _ = std::fs::remove_file(&proof);
        let script = dir.join("ok&md PWNED&rem .cmd");
        std::fs::write(&script, "@echo off\r\necho ran > \"%~dp0RAN\"\r\nexit\r\n")
            .expect("write script");

        // Exactly what `run_script` builds, minus the new console so the test
        // does not open a window. `/k` is kept: it is what makes the argument a
        // command line, which is the thing under test.
        let mut command = std::process::Command::new("cmd.exe");
        command.env("JUSTCODE_CMD", format!("call \"{}\"", script.display()));
        command.args(["/v:on", "/k", "!JUSTCODE_CMD!"]);
        command.creation_flags(CREATE_NEW_CONSOLE).current_dir(&dir);
        if let Ok(mut child) = command.spawn() {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let _ = child.kill();
            let _ = child.wait();
        }

        let injected = evidence.is_dir();
        // Asserting only "did not inject" passed happily while the quoting was
        // broken and nothing ran at all; this pins down that it still works.
        let ran = proof.is_file();
        let _ = std::fs::remove_dir_all(&evidence);
        let _ = std::fs::remove_file(&proof);
        let _ = std::fs::remove_file(&script);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!injected, "file name was re-parsed as a command");
        assert!(ran, "the script did not actually run");
    }

    /// The extension check must run on the resolved path, or a `.html` symlink
    /// pointing at an executable slips through.
    #[test]
    fn browser_check_uses_the_resolved_extension() {
        let dir = std::env::temp_dir().join("justcode-link-test");
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("payload.exe.txt");
        std::fs::write(&target, "x").expect("write target");

        let link = dir.join("preview.html");
        let _ = std::fs::remove_file(&link);
        let linked = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        if linked {
            let resolved = std::fs::canonicalize(&link).expect("canonicalize");
            let ext = resolved
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            assert_ne!(
                ext.as_deref(),
                Some("html"),
                "resolved extension should not be html"
            );
            let _ = std::fs::remove_file(&link);
        }
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Saving must never leave the previous contents truncated.
    #[test]
    fn write_is_atomic_over_an_existing_file() {
        let dir = std::env::temp_dir().join("justcode-write-test");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("doc.txt");
        std::fs::write(&file, "original").expect("seed");

        super::write_text_file(file.to_string_lossy().into_owned(), "replaced".into())
            .expect("write");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "replaced");
        // No temp file left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("justcode-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
