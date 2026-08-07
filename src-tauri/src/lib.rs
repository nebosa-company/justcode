use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{BuildHasher, Hash, Hasher};
use std::io::BufRead;
use std::io::Read;
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// Rejects anything that is not a web page. `open_path`/`open_url` use the
/// default shell association, so passing an executable extension (`.bat`,
/// `.exe`, …) would *run* it rather than preview it.
fn require_html_extension(path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if matches!(ext.as_deref(), Some("html") | Some("htm") | Some("xhtml")) {
        Ok(())
    } else {
        Err(format!("Only HTML files can be opened in the browser: {}", path.display()))
    }
}

/// Windows canonicalization yields a verbatim prefix browsers reject: turns
/// `\\?\UNC\server\share` into `\\server\share`, and `\\?\C:\…` into `C:\…`.
fn strip_verbatim_prefix(path: &Path) -> String {
    path.to_string_lossy().replace(r"\\?\UNC\", r"\\").replace(r"\\?\", "")
}

/// One previewed file's reload state: bumping `generation` wakes any
/// in-flight `/wait` long-poll for this path, which tells the page to
/// `location.reload()`.
struct PreviewEntry {
    generation: u64,
    last_seen: Instant,
}

#[derive(Default)]
struct PreviewRegistry {
    /// Directories previews have been served from — a `/file` request may
    /// only read from inside one of these, never an arbitrary path.
    roots: HashSet<PathBuf>,
    entries: HashMap<PathBuf, PreviewEntry>,
}

/// A loopback-only HTTP server that lets repeat `Run`s refresh a tab that is
/// already open instead of opening a new one. The OS shell has no concept of
/// "the tab already showing this file" — it always opens a new one — so a
/// direct `file://` open can never do this. Serving over HTTP instead gives
/// the page somewhere to long-poll back to: the app wakes that poll to
/// trigger a reload rather than asking the shell to open anything.
///
/// Bound to 127.0.0.1 and gated by a random per-launch token in the URL path,
/// with a matching `Host` header required on every request, so another local
/// process or a page loaded from the web can't probe or use this server.
struct PreviewServer {
    port: std::sync::Mutex<Option<u16>>,
    token: String,
    registry: std::sync::Mutex<PreviewRegistry>,
}

impl Default for PreviewServer {
    fn default() -> Self {
        // Not cryptographically strong, just unguessable: two independent
        // `RandomState`s, each reseeded from the OS on construction.
        let token = format!(
            "{:016x}{:016x}",
            std::collections::hash_map::RandomState::new().build_hasher().finish(),
            std::collections::hash_map::RandomState::new().build_hasher().finish(),
        );
        Self { port: std::sync::Mutex::new(None), token, registry: Default::default() }
    }
}

impl PreviewServer {
    fn registry(&self) -> std::sync::MutexGuard<'_, PreviewRegistry> {
        self.registry.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), percent_decode(v)))
        .collect()
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html") | Some("htm") | Some("xhtml") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn write_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
}

/// Builds the poll-and-reload script embedded into every served HTML page.
/// `generation` is the value current *at serve time* — starting the poll from
/// 0 unconditionally would race a reload that already happened between the
/// previous serve and this one, reloading the page in an endless loop.
fn reload_script(token: &str, path: &str, generation: u64) -> String {
    format!(
        "<script>(function(){{\
var gen={generation},token={},path={};\
function poll(){{\
fetch('/'+token+'/wait?path='+encodeURIComponent(path)+'&gen='+gen,{{cache:'no-store'}})\
.then(function(r){{return r.text();}})\
.then(function(g){{var n=parseInt(g,10);if(n!==gen){{location.reload();}}else{{poll();}}}})\
.catch(function(){{setTimeout(poll,1000);}});\
}}\
poll();\
}})();</script>",
        serde_json::to_string(token).unwrap_or_else(|_| "\"\"".into()),
        serde_json::to_string(path).unwrap_or_else(|_| "\"\"".into()),
    )
}

fn inject_reload_script(html: &[u8], token: &str, path: &str, generation: u64) -> Vec<u8> {
    let script = reload_script(token, path, generation);
    let text = String::from_utf8_lossy(html);
    let lower = text.to_ascii_lowercase();
    if let Some(index) = lower.rfind("</body>") {
        let mut out = String::with_capacity(text.len() + script.len());
        out.push_str(&text[..index]);
        out.push_str(&script);
        out.push_str(&text[index..]);
        out.into_bytes()
    } else {
        let mut out = text.into_owned();
        out.push_str(&script);
        out.into_bytes()
    }
}

/// Serves one request. Best-effort: a malformed request just gets its
/// connection dropped rather than a response, which is fine for a preview
/// server whose only clients are the browser tabs this app itself opened.
fn handle_preview_connection(app: &tauri::AppHandle, mut stream: TcpStream, port: u16) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut request_line = String::new();
    let mut reader = std::io::BufReader::new(&stream);
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    drop(reader);

    let mut parts = request_line.trim_end().split(' ');
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else { return };
    if method != "GET" {
        write_response(&mut stream, "405 Method Not Allowed", "text/plain", b"");
        return;
    }
    if headers.get("host").map(String::as_str) != Some(format!("127.0.0.1:{port}").as_str()) {
        write_response(&mut stream, "403 Forbidden", "text/plain", b"bad host");
        return;
    }

    let (target_path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = parse_query(query);

    let server = app.state::<PreviewServer>();
    let prefix = format!("/{}/", server.token);
    let Some(route) = target_path.strip_prefix(&prefix) else {
        write_response(&mut stream, "404 Not Found", "text/plain", b"");
        return;
    };

    let Some(raw_path) = query.get("path") else {
        write_response(&mut stream, "400 Bad Request", "text/plain", b"missing path");
        return;
    };
    let requested = PathBuf::from(raw_path);
    let Ok(canonical) = fs::canonicalize(&requested) else {
        write_response(&mut stream, "404 Not Found", "text/plain", b"");
        return;
    };
    let allowed = server.registry().roots.iter().any(|root| canonical.starts_with(root));
    if !allowed || !canonical.is_file() {
        write_response(&mut stream, "403 Forbidden", "text/plain", b"outside preview root");
        return;
    }

    match route {
        "file" => {
            let Ok(bytes) = fs::read(&canonical) else {
                write_response(&mut stream, "404 Not Found", "text/plain", b"");
                return;
            };
            let ctype = content_type(&canonical);
            if ctype.starts_with("text/html") {
                let generation = {
                    let mut registry = server.registry();
                    registry
                        .entries
                        .entry(canonical.clone())
                        .or_insert_with(|| PreviewEntry {
                            generation: 0,
                            last_seen: Instant::now() - Duration::from_secs(999),
                        })
                        .generation
                };
                let path_str = canonical.to_string_lossy();
                let body = inject_reload_script(&bytes, &server.token, &path_str, generation);
                write_response(&mut stream, "200 OK", ctype, &body);
            } else {
                write_response(&mut stream, "200 OK", ctype, &bytes);
            }
        }
        "wait" => {
            let requested_gen: u64 = query.get("gen").and_then(|g| g.parse().ok()).unwrap_or(0);
            let start = Instant::now();
            loop {
                let current = {
                    let mut registry = server.registry();
                    let entry = registry.entries.entry(canonical.clone()).or_insert_with(|| {
                        PreviewEntry { generation: requested_gen, last_seen: Instant::now() }
                    });
                    entry.last_seen = Instant::now();
                    entry.generation
                };
                if current != requested_gen || start.elapsed() > Duration::from_secs(30) {
                    write_response(&mut stream, "200 OK", "text/plain", current.to_string().as_bytes());
                    return;
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        }
        _ => write_response(&mut stream, "404 Not Found", "text/plain", b""),
    }
}

/// Starts the preview server on first use and returns its port, reusing it
/// on every later call for the lifetime of the app.
fn ensure_preview_server(app: &tauri::AppHandle) -> Result<u16, String> {
    let server = app.state::<PreviewServer>();
    let mut port = server.port.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(port) = *port {
        return Ok(port);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("{e}"))?;
    let bound = listener.local_addr().map_err(|e| format!("{e}"))?.port();
    let handle = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let handle = handle.clone();
            std::thread::spawn(move || handle_preview_connection(&handle, stream, bound));
        }
    });
    *port = Some(bound);
    Ok(bound)
}

/// Shows an HTML file in the default browser, by way of the loopback preview
/// server: the first `Run` for a given file opens a new tab, exactly like
/// handing the file straight to the OS shell did before. A later `Run` of the
/// *same* file, while that tab is still open and polling, instead just tells
/// the page to reload — no second tab. If the tab was closed, this falls back
/// to opening a new one, same as the first time.
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
    require_html_extension(&canonical)?;

    let port = ensure_preview_server(&app)?;
    let server = app.state::<PreviewServer>();
    let live = {
        let mut registry = server.registry();
        if let Some(parent) = canonical.parent() {
            registry.roots.insert(parent.to_path_buf());
        }
        match registry.entries.get_mut(&canonical) {
            Some(entry) if entry.last_seen.elapsed() < Duration::from_secs(2) => {
                entry.generation = entry.generation.wrapping_add(1);
                true
            }
            _ => {
                registry.entries.insert(
                    canonical.clone(),
                    PreviewEntry { generation: 0, last_seen: Instant::now() - Duration::from_secs(999) },
                );
                false
            }
        }
    };
    if live {
        return Ok(());
    }

    let target = strip_verbatim_prefix(&canonical);
    let url = format!(
        "http://127.0.0.1:{port}/{}/file?path={}",
        server.token,
        percent_encode(&target)
    );
    app.opener().open_url(url, None::<&str>).map_err(|e| format!("{e}"))
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

/// A file to open, plus where in it to put the caret — from a command-line
/// argument of the form `path`, `path:line` or `path:line:column` (1-based),
/// the convention VS Code, Zed and Cursor all also accept.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
struct FileTarget {
    path: String,
    line: Option<u32>,
    column: Option<u32>,
}

/// Tries to peel a trailing `:line` or `:line:column` off `arg` and confirms
/// what is left really is a file. Order matters: `:line:column` is tried
/// first since it is more specific, and a plain path is tried before either —
/// a Windows path's drive-letter colon (`C:\...`) must never be mistaken for
/// one of these, which checking real existence at each step guarantees: the
/// stripped candidate wins only if `PathBuf::is_file()` agrees.
fn split_line_column(arg: &str) -> FileTarget {
    if PathBuf::from(arg).is_file() {
        return FileTarget { path: arg.to_string(), line: None, column: None };
    }
    if let Some((head, col)) = arg.rsplit_once(':') {
        if let Ok(column) = col.parse::<u32>() {
            if let Some((path, line)) = head.rsplit_once(':') {
                if let Ok(line) = line.parse::<u32>() {
                    if PathBuf::from(path).is_file() {
                        return FileTarget {
                            path: path.to_string(),
                            line: Some(line),
                            column: Some(column),
                        };
                    }
                }
            }
        }
        if let Ok(line) = col.parse::<u32>() {
            if PathBuf::from(head).is_file() {
                return FileTarget { path: head.to_string(), line: Some(line), column: None };
            }
        }
    }
    FileTarget { path: arg.to_string(), line: None, column: None }
}

/// Picks the file paths out of a command line, ignoring flags and anything
/// that is not — once a trailing `:line[:column]` is accounted for —
/// actually a file on disk.
fn files_from_args<I: IntoIterator<Item = String>>(args: I) -> Vec<FileTarget> {
    args.into_iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| split_line_column(&arg))
        .filter(|target| PathBuf::from(&target.path).is_file())
        .collect()
}

/// Files passed on the command line — how Explorer hands over a double-clicked
/// document once the extensions are associated with the app. Uses `args_os` so a
/// path that is not valid Unicode is decoded lossily rather than panicking.
/// Run the Perpetum harness and hand back what it printed (`I-2`).
///
/// The harness is a **sidecar**: a separate process this editor invokes, never
/// a library it links. An agent loop must not be able to take the editor down
/// with it, and must outlive the editor window — both are properties of being
/// a different process rather than of careful coding.
///
/// Three rules here, and they are the whole function:
///
/// - **The subcommand comes from a fixed list.** The panel cannot ask for an
///   arbitrary one, so a bug or an injected string in the front-end cannot turn
///   into `perp rewind --approve`. Anything that needs an approval is not on
///   the list at all.
/// - **Arguments go as argv, never through a shell.** No quoting to get wrong.
/// - **A missing binary is an ordinary answer.** JustCode must build, start and
///   work with `crates/` deleted, so "not installed" comes back as a message
///   rather than an error the panel has to special-case.
/// The panel's writing door, separate from `perp_run` on purpose (`O-17`).
///
/// `perp_run`'s allowlist is every subcommand that only reads, and its message
/// says so: *the panel reads, and may leave a `/btw`*. Adding a writing command
/// to that list would make the rule "the panel reads, except" — and a rule with
/// an except is a rule that grows one more each time somebody needs it.
///
/// So this is a second door with its own list, and the list is short because
/// widening it is a decision somebody has to come here and make. What passes
/// through it changes a file a person owns, which is why the caller is expected
/// to have asked them first: this runs the command, it does not ask the
/// question.
#[tauri::command]
fn perp_write(subcommand: String, args: Vec<String>, root: String) -> Result<String, String> {
    // Both of these change the requirements source, which `V-12` keeps out of
    // the loop's reach — the loop's host still refuses that path, and these are
    // a person's click, not the loop's write. `ungate` clears a `⛔`;
    // `requirement add` appends a row (`O-18`).
    //
    // What is still absent is the one that matters: nothing here can write a
    // `✅`. A person may put work on the list and may unblock it, and the
    // marker that says it is finished remains something only gates earn.
    const ALLOWED_WRITES: &[&str] = &["ungate", "requirement"];
    if !ALLOWED_WRITES.contains(&subcommand.as_str()) {
        return Err(format!(
            "`{subcommand}` is not a panel write. Only {} may change anything from here.",
            ALLOWED_WRITES.join(", ")
        ));
    }
    perp_invoke(&subcommand, &args, &root)
}

#[tauri::command]
fn perp_run(subcommand: String, args: Vec<String>, root: String) -> Result<String, String> {
    // Reads, plus the one write the harness designed for outside messages.
    //
    // `run`, `rewind`, `control` and `approve` change what the loop does and are
    // deliberately absent: the panel shows the journal, and acting on it is done
    // where the confirmation is.
    //
    // `btw` is here because it is the harness's own door for a message from
    // outside, and the least powerful object in the system: `control` has
    // exactly one inbound function, everything arriving on it becomes a `/btw`,
    // and `btw::classify` then applies `C-10` so even that cannot reach the
    // approval boundary. Sending a note is not acting on the loop — the loop
    // decides whether to pick it up.
    //
    // `chat` is deliberately *not* here, and not because it is dangerous — a
    // turn runs no tools and takes no lock. It waits on a model for seconds,
    // and everything on this list returns in milliseconds because it reads a
    // file. Blocking here blocks the window. It gets its own door, for the same
    // reason `cycle` does.
    const ALLOWED: &[&str] =
        &["panel", "state", "cost", "explain", "check", "links", "version", "btw"];
    if !ALLOWED.contains(&subcommand.as_str()) {
        return Err(format!(
            "`{subcommand}` is not a panel subcommand. The panel reads, and may leave a `/btw`."
        ));
    }

    perp_invoke(&subcommand, &args, &root)
}

/// Run the binary. Shared by both doors so they cannot drift: the difference
/// between reading and writing is which list let you in, never what happens
/// afterwards.
fn perp_invoke(subcommand: &str, args: &[String], root: &str) -> Result<String, String> {
    let program = std::env::var("PERP_BIN").unwrap_or_else(|_| "perp".to_string());
    let mut command = std::process::Command::new(&program);
    command.arg(subcommand).args(args).arg("--root").arg(root);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    match command.output() {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        }
        Ok(output) => Err(format!(
            "perp {subcommand} exited {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(format!(
            "not-installed: the Perpetum harness ({program}) was not found.              JustCode works without it; the panel has nothing to show."
        )),
        Err(e) => Err(format!("could not run {program}: {e}")),
    }
}

/// Start an unattended cycle, and do not wait for it (`I-1`).
///
/// Deliberately **not** part of `perp_run`'s list. That list is reads, and its
/// own comment says the panel reads and does not act; quietly adding `cycle` to
/// it would have turned a read allowlist into a mixed one, where the next person
/// to add an entry has no rule to follow. Starting a run is a different kind of
/// thing and gets a different door.
///
/// Two properties matter more than the plumbing:
///
/// - **It is spawned, not awaited.** A cycle runs for minutes to an hour, and
///   `output()` would block the UI for all of it. The panel needs nothing back:
///   it watches the journal, so progress arrives the same way it does for a run
///   started from a terminal.
/// - **The numbers are bounded here, not trusted.** They arrive from a dialog
///   and go into argv, so a nonsense count is refused rather than passed on.
///
/// Everything the harness refuses stays refused. This starts a cycle; it cannot
/// approve one, and `git.push = approval` still means a person says yes.
#[tauri::command]
fn perp_start(batches: u32, items: u32, root: String, verbose: bool) -> Result<String, String> {
    if !(1..=64).contains(&batches) || !(1..=32).contains(&items) {
        return Err(format!(
            "{batches} batches x {items} items is outside what this can start (1-64 by 1-32)"
        ));
    }

    // Beside the journal, truncated per start: this is the last run's console,
    // not a history. The journal is the history.
    let log_path = std::path::Path::new(&root).join(".harness/cycle.log");
    let log = std::fs::File::create(&log_path)
        .map_err(|e| format!("{}: {e}", log_path.display()))?;

    let program = std::env::var("PERP_BIN").unwrap_or_else(|_| "perp".to_string());
    let mut command = std::process::Command::new(&program);
    command
        .arg("cycle")
        .arg("--batches")
        .arg(batches.to_string())
        .arg("--items")
        .arg(items.to_string());
    // `I-7`: the reasoning, kept. Everything `--verbose` prints goes to stderr,
    // and both streams already land in the log below — so this is the whole of
    // making it available, and `Harness -> Settings -> Run log` is where it is
    // read. Off unless asked: a log full of prompts is a log nobody reads.
    if verbose {
        command.arg("--verbose");
    }
    command
        .arg("--root")
        .arg(&root)
        // To a file, not a pipe and not `null`.
        //
        // A pipe nobody reads eventually fills and stalls the cycle, so that was
        // never an option — but `null` was worse. The first version of this
        // discarded both streams, and when the harness refused to start (no
        // credential for its link, `M-24`) the refusal went to the void: no
        // process, no journal entry, no dialog, nothing. Silence that looks
        // exactly like success is the one outcome worth engineering against.
        .stdout(log.try_clone().map_err(|e| format!("{e}"))?)
        .stderr(log);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("not-installed: the Perpetum harness ({program}) was not found."));
        }
        Err(e) => return Err(format!("could not start {program}: {e}")),
    };

    // A spawn that worked only means a process started. Everything the harness
    // refuses up front — no budget declared, a link whose credential is unset,
    // another run holding the write lock — it refuses in the first moment, so a
    // short look is enough to turn that into an answer instead of a silence. A
    // cycle that is actually running is still going after this and is left alone.
    std::thread::sleep(std::time::Duration::from_millis(1200));
    match child.try_wait() {
        Ok(Some(status)) if !status.success() => {
            let said = std::fs::read_to_string(&log_path).unwrap_or_default();
            let reason = said.trim().lines().last().unwrap_or("it said nothing").to_string();
            Err(format!("the harness refused to start: {reason}"))
        }
        // Exited cleanly in a second: nothing to do, and saying so is better than
        // implying a run is under way.
        Ok(Some(_)) => Ok(format!("finished immediately — see {}", log_path.display())),
        _ => Ok(format!("started {batches} x {items} as pid {}", child.id())),
    }
}

/// The workspace a file belongs to: the nearest ancestor holding a binding.
///
/// The panel used to take the folder of the open file and call it the root,
/// under a comment that said it walked up to find the binding. It did not, so
/// the panel worked only when the open file happened to sit at the top of the
/// project and reported `cannot read .../src/docs/perpetum/binding.md` for
/// every file in a subdirectory.
///
/// `None` when no ancestor has one, which is an ordinary answer: most files are
/// not in a Perpetum workspace and the panel says so rather than guessing.
#[tauri::command]
fn perp_root(from: String) -> Option<String> {
    let owned = directory_of(&from);
    let mut dir = owned.as_path();
    // Bounded by the filesystem: `parent()` yields `None` at the root.
    loop {
        if dir.join(".harness/binding.md").is_file() {
            return Some(dir.to_string_lossy().replace('\\', "/"));
        }
        dir = dir.parent()?;
    }
}

/// The directory a path denotes: itself, or its parent when it names a file.
///
/// Every caller here means "the folder to search from", and the searches walk
/// upwards, so being handed a file was harmless for them by luck. It was not
/// harmless for `init`, which joins `.harness` onto what it is given: pointed at
/// `todo.md` it asked Windows to create `todo.md\.harness` and got "cannot
/// create a file when that file already exists". Answering the question the
/// callers are actually asking costs one `is_file` and cannot be got wrong twice.
fn directory_of(from: &str) -> std::path::PathBuf {
    let given = std::path::Path::new(from);
    if given.is_file() {
        if let Some(parent) = given.parent() {
            return parent.to_path_buf();
        }
    }
    given.to_path_buf()
}

/// Bumped whenever the panel points at a workspace. A watcher thread whose
/// generation is no longer current exits on its next tick, so re-attaching does
/// not leave two threads emitting into the same window.
static PERP_WATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How often the watcher looks. Fast enough that a step closing feels immediate,
/// slow enough to be two `stat` calls a second on one file.
const PERP_WATCH_MS: u64 = 400;

/// Watch a workspace's journal and emit `perp:changed` when it grows (`I-2`).
///
/// The panel used to poll `perp panel` every four seconds, which meant a step
/// could close four seconds before anyone saw it and that nothing moved at all
/// while the panel was shut. This watches the file instead and tells the window,
/// so the panel re-reads because something happened rather than because a timer
/// went off.
///
/// It keeps running when the panel closes. Watching costs a `stat`; stopping
/// means reopening the panel shows a stale view until the next tick, and the
/// toggle cannot show that a run is under way.
///
/// The journal's conventional path is used rather than `out.journal` from the
/// binding, because resolving that needs a subprocess and this runs twice a
/// second. A binding that moves the journal loses the events, not the panel: the
/// slow refresh and the Refresh button both still work.
#[tauri::command]
fn perp_watch(app: tauri::AppHandle, root: String) {
    use std::sync::atomic::Ordering;

    let generation = PERP_WATCH.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let dir = std::path::Path::new(&root).join(".harness");
        // Empty until the first tick, so a workspace that gains a `.harness` while
        // the editor is open reports its arrival as a change rather than as the
        // baseline.
        let mut seen: Vec<(String, u64, std::time::SystemTime)> = Vec::new();
        let mut first = true;
        loop {
            if PERP_WATCH.load(Ordering::SeqCst) != generation {
                return;
            }
            let now = harness_snapshot(&dir);
            if now != seen {
                let changed = changed_between(&seen, &now);
                seen = now;
                // The first tick only establishes the baseline. Reporting every
                // file as changed the moment the panel opens would rebuild
                // everything for nothing.
                if !first && app.emit("perp:changed", changed).is_err() {
                    return;
                }
                first = false;
            }
            std::thread::sleep(std::time::Duration::from_millis(PERP_WATCH_MS));
        }
    });
}

/// Every file under `.harness`, with its size and mtime.
///
/// Sorted, so two snapshots compare by value. A dozen `stat` calls twice a second
/// is cheaper than being wrong about whether anything moved, and this directory
/// never holds enough files for a cleverer answer to earn its code.
///
/// It used to watch `journal.jsonl` alone — and kept watching the pre-`.harness`
/// path after everything moved, so live progress was dead for a day behind a
/// fifteen-second backstop poll that hid it.
fn harness_snapshot(dir: &std::path::Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    walk_harness(dir, dir, &mut out);
    out.sort();
    out
}

fn walk_harness(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<(String, u64, std::time::SystemTime)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_harness(root, &path, out);
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(when) = meta.modified() else { continue };
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((relative, meta.len(), when));
    }
}

/// Which paths differ between two snapshots — added, removed or rewritten.
///
/// Named rather than a bare "something changed", because the front end does
/// different work for each: a journal write refreshes the panel, a binding or
/// links edit means the setup itself moved, and a requirements edit changes a menu.
fn changed_between(
    before: &[(String, u64, std::time::SystemTime)],
    after: &[(String, u64, std::time::SystemTime)],
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in after {
        if before.iter().find(|old| old.0 == entry.0) != Some(entry) {
            names.push(entry.0.clone());
        }
    }
    for entry in before {
        if !after.iter().any(|new| new.0 == entry.0) {
            names.push(entry.0.clone());
        }
    }
    names.sort();
    names.dedup();
    names
}

/// What the Harness menu needs to know, in one call.
///
/// Three questions that would otherwise be three round trips every time a menu
/// opens: whether this is a workspace at all, where `init` would put one if not,
/// and which requirements files exist.
#[tauri::command]
fn harness_state(from: String) -> serde_json::Value {
    let start = directory_of(&from);
    let start = start.as_path();

    let mut root: Option<std::path::PathBuf> = None;
    let mut dir = Some(start);
    while let Some(candidate) = dir {
        if candidate.join(".harness/binding.md").is_file() {
            root = Some(candidate.to_path_buf());
            break;
        }
        dir = candidate.parent();
    }

    // Where `init` would go: the top of the repository if there is one, because a
    // harness belongs beside the project rather than beside whichever file happens
    // to be open.
    let mut init_root = start.to_path_buf();
    let mut dir = Some(start);
    while let Some(candidate) = dir {
        if candidate.join(".git").exists() {
            init_root = candidate.to_path_buf();
            break;
        }
        dir = candidate.parent();
    }

    let requirements = root
        .as_ref()
        .map(|found| {
            let dir = found.join(".harness/requirements");
            let mut names = Vec::new();
            collect_requirements(&dir, &dir, &mut names);
            names.sort();
            names
        })
        .unwrap_or_default();

    serde_json::json!({
        "root": root.map(|path| path.to_string_lossy().replace('\\', "/")),
        "initRoot": init_root.to_string_lossy().replace('\\', "/"),
        "requirements": requirements,
    })
}

fn collect_requirements(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_requirements(root, &path, out);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
            out.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

/// Scaffold a workspace (`L-21`).
///
/// Its own command rather than an entry on `perp_run`'s list, for the same reason
/// `perp_start` is: that list is reads. `init` writes — though it never
/// overwrites, so a second run fills gaps and keeps what it finds.
#[tauri::command]
fn perp_init(root: String) -> Result<String, String> {
    let program = std::env::var("PERP_BIN").unwrap_or_else(|_| "perp".to_string());
    let mut command = std::process::Command::new(&program);
    command.arg("init").arg("--root").arg(&root);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    match command.output() {
        Ok(out) if out.status.success() => {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
        Ok(out) => Err(format!(
            "perp init exited {}: {}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("not-installed: the Perpetum harness ({program}) was not found."))
        }
        Err(e) => Err(format!("could not run {program}: {e}")),
    }
}

/// One conversation turn, asked and answered (`C-1`–`C-5`).
///
/// Its own door rather than an entry in [`perp_run`]'s list, and `async` rather
/// than a plain command, for one reason: this waits on a model. Everything on
/// that list reads a file and returns in milliseconds, and a synchronous command
/// runs on the main thread — so a chat turn there would freeze the window for
/// however long the model took to think. The blocking wait happens on the
/// runtime's own pool, and the window keeps drawing.
///
/// It is `perp chat --once` and not a second implementation of a turn. Both
/// sides go in the loop's journal, the call is priced (`M-11`), and while the
/// loop holds the write lock the session is read-only (`C-3`) — none of which
/// the editor should be reimplementing or, worse, quietly skipping.
///
/// The reply comes back on stdout, but the panel does not need it: both turns
/// are in the journal by then, and the watcher redraws from there. It is
/// returned anyway so a failure has something to say.
#[tauri::command]
async fn perp_chat(message: String, root: String) -> Result<String, String> {
    let text = message.trim().to_string();
    if text.is_empty() {
        return Err("nothing to say".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let program = std::env::var("PERP_BIN").unwrap_or_else(|_| "perp".to_string());
        let mut command = std::process::Command::new(&program);
        command.arg("chat").arg("--once").arg(&text).arg("--root").arg(&root);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        match command.output() {
            Ok(out) if out.status.success() => {
                Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
            }
            Ok(out) => {
                let said = String::from_utf8_lossy(&out.stderr);
                let said = said.trim();
                // Its own words where it has any: `M-24` and a missing link both
                // say exactly what is wrong, and "exited 1" says nothing.
                if said.is_empty() {
                    Err(format!("perp chat exited {}", out.status.code().unwrap_or(-1)))
                } else {
                    Err(said.to_string())
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(format!("not-installed: the Perpetum harness ({program}) was not found."))
            }
            Err(e) => Err(format!("could not run {program}: {e}")),
        }
    })
    .await
    .map_err(|e| format!("chat did not finish: {e}"))?
}

/// Bumped whenever the set of open files changes, so a watcher thread whose
/// generation is stale exits on its next tick rather than emitting alongside
/// its replacement.
static FILE_WATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How often open files are checked. Slower than the harness watcher: a journal
/// grows while you look at it, and a source file changes when a person or a
/// branch changes it, which is not a per-frame event.
const FILE_WATCH_MS: u64 = 1_000;

/// Watch the files that are open in tabs, and say which ones moved.
///
/// The editor had no idea a file had changed underneath it. The harness watcher
/// has always covered `.harness/`, so the panel and the menus were current while
/// the tab showing one of those very files sat there holding what it read when
/// it opened — and saving from it would have written that stale text back over
/// whatever arrived in the meantime, silently.
///
/// Size and modified time, not content. This runs every second against every
/// open file, and hashing them to answer a question that is almost always "no"
/// is work for nothing. It is deliberately a coarse signal: the editor reads the
/// file and compares it against what the tab holds before doing anything, so a
/// touched-but-identical file — including the one the editor just saved itself —
/// costs one read and no interruption.
#[tauri::command]
fn watch_files(app: tauri::AppHandle, paths: Vec<String>) {
    use std::sync::atomic::Ordering;

    let generation = FILE_WATCH.fetch_add(1, Ordering::SeqCst) + 1;
    if paths.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let stamp = |path: &str| -> Option<(u64, std::time::SystemTime)> {
            let meta = std::fs::metadata(path).ok()?;
            Some((meta.len(), meta.modified().ok()?))
        };
        // The baseline is taken before the first sleep, so a file is reported
        // when it changes from how it was when the watch began — not when the
        // watch began.
        let mut seen: Vec<Option<(u64, std::time::SystemTime)>> =
            paths.iter().map(|path| stamp(path)).collect();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(FILE_WATCH_MS));
            if FILE_WATCH.load(Ordering::SeqCst) != generation {
                return;
            }
            let mut moved = Vec::new();
            for (index, path) in paths.iter().enumerate() {
                let now = stamp(path);
                // A file that has gone is not reported. Its buffer is the only
                // copy left, and replacing it with nothing would be the one
                // outcome worse than being out of date.
                if now.is_some() && now != seen[index] {
                    moved.push(path.clone());
                }
                seen[index] = now;
            }
            if !moved.is_empty() && app.emit("files:changed", moved).is_err() {
                return;
            }
        }
    });
}

/// Stop watching. Called when the last file closes.
#[tauri::command]
fn unwatch_files() {
    FILE_WATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Stop watching. Called when the panel is pointed at nothing.
#[tauri::command]
fn perp_unwatch() {
    PERP_WATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

#[tauri::command]
fn startup_files() -> Vec<FileTarget> {
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
        .manage(PreviewServer::default())
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

            // Puts JustCode in Explorer's "Open with" list — and Settings ▸
            // Default apps — for every extension it understands, the same way
            // VS Code, Zed and Cursor register themselves on install. This used
            // to happen only the first time a user opened File Associations…
            // and clicked Apply, so a file type nobody had explicitly ticked
            // never got JustCode as an offered choice, even though nothing
            // here makes it anyone's default. Cheap and idempotent — a handful
            // of registry writes of the same values on every launch — so no
            // "only do this once" bookkeeping is worth the complexity.
            #[cfg(windows)]
            {
                let extensions: Vec<String> = app
                    .config()
                    .bundle
                    .file_associations
                    .iter()
                    .flatten()
                    .flat_map(|association| association.ext.iter().map(|ext| ext.0.clone()))
                    .collect();
                std::thread::spawn(move || {
                    if associations::register_application(&extensions).is_ok() {
                        associations::notify_shell();
                    }
                });
            }

            Ok(())
        })
        // Must be registered first: a second launch (Explorer opening another
        // file) forwards its arguments here instead of starting a new window.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let files = files_from_args(argv);
            if let Some(window) = app.get_webview_window("main") {
                // Opening a document from Explorer should land you in the
                // editor the same way starting the app does — which is
                // maximised (see `reveal`). Unminimising alone left a window
                // that had been dropped to the taskbar restored to whatever
                // small size it last had, with the new file somewhere inside it.
                let _ = window.unminimize();
                let _ = window.maximize();
                let _ = window.set_focus();
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
            perp_run,
            perp_write,
            perp_root,
            perp_start,
            perp_init,
            watch_files,
            unwatch_files,
            perp_chat,
            harness_state,
            perp_watch,
            perp_unwatch,
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
    use super::{files_from_args, split_line_column, FileTarget};

    fn target(path: &str) -> FileTarget {
        FileTarget { path: path.to_string(), line: None, column: None }
    }

    /// Explorer launches the registered command as `justcode.exe "C:\path\x.ps1"`,
    /// so argv[0] is the executable and the file is argv[1]. The same shape is
    /// forwarded by the single-instance plugin for a second launch.
    #[test]
    fn takes_the_first_argument_as_a_file() {
        let script = std::env::temp_dir().join("justcode-arg-test.ps1");
        std::fs::write(&script, "Write-Host 'hi'").expect("write temp script");
        let path = script.to_string_lossy().into_owned();

        let argv = vec![r"C:\Program Files\JustCode\justcode.exe".to_string(), path.clone()];
        assert_eq!(files_from_args(argv), vec![target(&path)]);

        // Flags are ignored, and several files may arrive at once.
        let argv = vec![
            "justcode.exe".to_string(),
            "--some-flag".to_string(),
            path.clone(),
            r"C:\does
ot\exist.ps1".to_string(),
        ];
        assert_eq!(files_from_args(argv), vec![target(&path)]);

        // The executable itself is never treated as a file to open.
        let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        assert!(files_from_args(vec![exe]).is_empty());

        let _ = std::fs::remove_file(&script);
    }

    /// The `path:line` / `path:line:column` convention VS Code, Zed and Cursor
    /// all accept on their own command lines.
    #[test]
    fn splits_a_trailing_line_and_column_off_a_real_file() {
        let script = std::env::temp_dir().join("justcode-lc-test.ps1");
        std::fs::write(&script, "Write-Host 'hi'").expect("write temp script");
        let path = script.to_string_lossy().into_owned();

        assert_eq!(
            split_line_column(&format!("{path}:42")),
            FileTarget { path: path.clone(), line: Some(42), column: None }
        );
        assert_eq!(
            split_line_column(&format!("{path}:42:7")),
            FileTarget { path: path.clone(), line: Some(42), column: Some(7) }
        );
        // No file at that path once the suffix is stripped — not a match, the
        // whole string is kept as a (nonexistent) path instead.
        assert_eq!(split_line_column("C:\\nope\\nope.rs:42"), target("C:\\nope\\nope.rs:42"));

        let _ = std::fs::remove_file(&script);
    }

    /// A Windows absolute path's drive-letter colon must never be mistaken for
    /// a `path:line` separator.
    #[test]
    fn drive_letter_colon_is_not_treated_as_a_line_number() {
        let script = std::env::temp_dir().join("justcode-drive-test.ps1");
        std::fs::write(&script, "Write-Host 'hi'").expect("write temp script");
        let path = script.to_string_lossy().into_owned();

        assert_eq!(split_line_column(&path), target(&path));
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

#[cfg(test)]
mod preview_server_tests {
    use super::*;

    #[test]
    fn percent_round_trips_reserved_and_unicode_characters() {
        let original = "C:\\Users\\a b\\café & friends?.html";
        assert_eq!(percent_decode(&percent_encode(original)), original);
    }

    #[test]
    fn query_values_are_percent_decoded() {
        let parsed = parse_query("path=C%3A%5Cfoo%20bar.html&gen=3");
        assert_eq!(parsed.get("path").map(String::as_str), Some(r"C:\foo bar.html"));
        assert_eq!(parsed.get("gen").map(String::as_str), Some("3"));
    }

    // A page served with its poll starting from a stale generation would see a
    // mismatch on its very first `/wait` and reload immediately, which reloads
    // again, forever. The starting generation embedded in the page must match
    // whatever the registry holds *right now*, not always start at 0.
    #[test]
    fn injected_script_polls_from_the_current_generation_not_zero() {
        let html = inject_reload_script(b"<html><body>hi</body></html>", "tok", "/a.html", 7);
        let text = String::from_utf8(html).unwrap();
        assert!(text.contains("var gen=7"), "expected gen=7 in: {text}");
        // Injected before the closing tag, not appended after the document.
        assert!(text.trim_end().ends_with("</html>"));
    }

    #[test]
    fn injected_script_is_appended_when_there_is_no_body_tag() {
        let html = inject_reload_script(b"<html>no body here", "tok", "/a.html", 0);
        let text = String::from_utf8(html).unwrap();
        assert!(text.starts_with("<html>no body here"));
        assert!(text.contains("var gen=0"));
    }

    #[test]
    fn content_type_recognizes_common_web_asset_extensions() {
        assert_eq!(content_type(Path::new("a.html")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("a.css")), "text/css; charset=utf-8");
        assert_eq!(content_type(Path::new("a.js")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("a.unknownext")), "application/octet-stream");
    }

    #[test]
    fn require_html_extension_rejects_non_web_files() {
        assert!(require_html_extension(Path::new("a.html")).is_ok());
        assert!(require_html_extension(Path::new("a.htm")).is_ok());
        assert!(require_html_extension(Path::new("a.exe")).is_err());
        assert!(require_html_extension(Path::new("a.bat")).is_err());
    }
}

#[cfg(test)]
mod harness_root_tests {
    use super::directory_of;

    /// The editor knows which file is open, not which folder it is in, so a file
    /// path reaching these commands is ordinary rather than a mistake to guard
    /// against. `perp_root` survived one by luck — it walks upwards, so starting
    /// one level too deep still found the workspace. `harness_state` did not:
    /// with no `.git` above it, `init_root` fell back to the path as given, and
    /// `init` was asked to create `.harness` inside a file.
    #[test]
    fn a_file_resolves_to_the_folder_holding_it() {
        let dir = std::env::temp_dir().join("justcode-directory-of-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("todo.md");
        std::fs::write(&file, "# notes\n").unwrap();

        assert_eq!(directory_of(&file.to_string_lossy()), dir);

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir(&dir).ok();
    }

    /// A directory is already the answer. Stepping up from one would search the
    /// parent of the folder the user is actually in.
    #[test]
    fn a_directory_is_left_alone() {
        let dir = std::env::temp_dir();
        assert_eq!(directory_of(&dir.to_string_lossy()), dir);
    }

    /// Nothing on disk to inspect, so it cannot be a file: pass it through
    /// rather than guessing at a parent.
    #[test]
    fn a_path_that_does_not_exist_is_passed_through() {
        let missing = std::env::temp_dir().join("justcode-no-such-dir-here");
        assert_eq!(directory_of(&missing.to_string_lossy()), missing);
    }
}
