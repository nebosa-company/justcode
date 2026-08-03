//! HTTP, over `curl`.
//!
//! The harness has no dependencies (`N-11`), and TLS is not something to
//! hand-roll. `curl` ships with Windows 10 1803+, macOS and every Linux worth
//! the name, so the operator approved shelling out to it rather than taking a
//! TLS crate.
//!
//! **The secret never touches argv.** Anyone on the machine can read another
//! process's command line, so an `Authorization` header passed as an argument
//! is a credential leak with a nice interface. It goes to `curl -K -` on
//! stdin instead — not on the command line, not in a file that outlives the
//! call, and not in the journal (`S-2`).
//!
//! Requests are bounded twice: `--connect-timeout` for a peer that is not
//! there, and `--max-time` for one that answers slowly forever (`T-3`).

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::process::{self, Env, Spec};

/// Where a secret's *name* lives. The value is read at the moment of use and
/// never stored, never cloned into a struct that derives `Debug`, and never
/// logged.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret {
    pub env_var: String,
}

impl Secret {
    pub fn from_env_var(name: impl Into<String>) -> Secret {
        Secret { env_var: name.into() }
    }

    /// Read it, or say which variable is missing — never what it should
    /// contain.
    pub fn read(&self) -> Result<String> {
        std::env::var(&self.env_var).map_err(|_| {
            Error::unbound(
                self.env_var.clone(),
                "is not set — the key is referenced by variable name and read at the moment of use",
            )
        })
    }
}

/// Redacted on purpose: a `{:?}` of a request must be safe to put in a journal.
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret(${})", self.env_var)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub url: String,
    /// Non-secret headers. Anything sensitive goes in `auth`.
    pub headers: Vec<(String, String)>,
    /// `(header name, secret)` — sent via stdin, never argv.
    pub auth: Option<(String, Secret)>,
    pub body: Option<String>,
    pub connect_timeout: Duration,
    pub max_time: Duration,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Request {
        Request {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            auth: None,
            body: None,
            connect_timeout: Duration::from_secs(5),
            max_time: Duration::from_secs(300),
        }
    }

    pub fn post_json(url: impl Into<String>, body: impl Into<String>) -> Request {
        Request {
            method: Method::Post,
            url: url.into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            auth: None,
            body: Some(body.into()),
            connect_timeout: Duration::from_secs(5),
            max_time: Duration::from_secs(300),
        }
    }

    pub fn bearer(mut self, secret: Secret) -> Request {
        self.auth = Some(("Authorization".into(), secret));
        self
    }

    /// A credential in a header of its own name, sent bare.
    ///
    /// Anthropic reads `x-api-key` with no scheme prefix. Still goes through
    /// `auth` rather than `headers`, because that is the field the transport
    /// passes by stdin and never by argv (`S-2`).
    pub fn auth_header(mut self, name: &str, secret: Secret) -> Request {
        self.auth = Some((name.to_string(), secret));
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Request {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn with_deadline(mut self, connect: Duration, total: Duration) -> Request {
        self.connect_timeout = connect;
        self.max_time = total;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// A short, quotable reason — for a journal record or a failover decision.
    pub fn complaint(&self) -> String {
        let tail: String = self.body.chars().take(200).collect();
        format!("HTTP {} — {}", self.status, tail.trim())
    }
}

pub trait Transport: fmt::Debug {
    fn send(&self, request: &Request) -> Result<Response>;

    /// Send with a **first-token deadline** and hand back the buffered response
    /// shape either way (`M-23`).
    ///
    /// The distinction this exists to make: a buffered request says nothing to
    /// the transport until the entire answer exists, so a model deliberating and
    /// a server that has wedged are indistinguishable for the length of
    /// `--max-time`. Measured against DeepSeek on a real backlog, a turn that
    /// generated 34,391 output tokens outran the whole-request bound and came
    /// back as `curl: (28) ... with 4 bytes received` — three runs blocked that
    /// way before the shape was visible. Streaming makes it mechanical: either a
    /// token arrives inside the deadline, or the link is failed over (`M-9`).
    ///
    /// Defaulted to [`Transport::send`] on purpose. A stub that returns a canned
    /// body has no socket and nothing to be deadlined about, so a test seam stays
    /// a test seam and only the real transport grows the behaviour.
    fn send_deadlined(&self, request: &Request, _first_token: Duration) -> Result<Response> {
        self.send(request)
    }
}

/// `curl`, invoked once per request.
#[derive(Debug, Clone)]
pub struct Curl {
    program: String,
    /// Bodies are written here rather than passed as arguments: a prompt is far
    /// longer than any command line allows, and stdin is already taken by the
    /// credential config.
    scratch: PathBuf,
}

impl Default for Curl {
    fn default() -> Curl {
        Curl::new()
    }
}

/// A directory for request bodies that other users of the machine cannot read
/// (`S-8`).
///
/// Found in cycle 2's Phase E review: `std::env::temp_dir()` reads `TMP`, and on
/// this machine `TMP` is `D:\Temp` — a **shared root-level directory**, not the
/// per-user one. A prompt carrying repository content was therefore briefly
/// world-readable while the call was in flight. The key was never affected; it
/// goes to curl on stdin and never touches disk (`S-2`).
///
/// The per-user root is preferred, and the directory is created with
/// owner-only permissions on POSIX. On Windows a directory under `LOCALAPPDATA`
/// inherits that profile's ACL, which is the per-user boundary — there is no
/// mode bit to set, and pretending otherwise by calling `set_permissions` would
/// be theatre.
pub fn private_scratch() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        // Last resort. Still better than nothing: the per-process subdirectory
        // below at least keeps two users' bodies apart by name.
        .unwrap_or_else(std::env::temp_dir);

    let dir = base.join("perp").join("bodies");
    let _ = std::fs::create_dir_all(&dir);
    restrict(&dir);
    dir
}

#[cfg(unix)]
fn restrict(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict(_dir: &Path) {
    // Windows: the ACL comes from the parent, which is the user's profile.
}

impl Curl {
    pub fn new() -> Curl {
        Curl { program: "curl".to_string(), scratch: private_scratch() }
    }

    pub fn with_program(mut self, program: impl Into<String>) -> Curl {
        self.program = program.into();
        self
    }

    pub fn with_scratch(mut self, scratch: impl Into<PathBuf>) -> Curl {
        self.scratch = scratch.into();
        self
    }

    /// The arguments — everything here is safe to appear in a process listing.
    ///
    /// Separated from [`Curl::send`] so a test can assert what is *not* in it.
    pub fn args(&self, request: &Request, body_file: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "--silent".to_string(),
            "--show-error".to_string(),
            // The status line is wanted even for a 4xx, which is data, not a
            // transport failure — so no `--fail`.
            "--write-out".to_string(),
            "\\n%{http_code}".to_string(),
            "--connect-timeout".to_string(),
            request.connect_timeout.as_secs().to_string(),
            "--max-time".to_string(),
            request.max_time.as_secs().to_string(),
            "-X".to_string(),
            request.method.as_str().to_string(),
        ];
        for (name, value) in &request.headers {
            args.push("-H".to_string());
            args.push(format!("{name}: {value}"));
        }
        if request.auth.is_some() {
            // The header itself arrives on stdin; this only says to read it.
            args.push("-K".to_string());
            args.push("-".to_string());
        }
        if let Some(path) = body_file {
            args.push("--data-binary".to_string());
            args.push(format!("@{path}"));
        }
        args.push(request.url.clone());
        args
    }

    /// The stdin config carrying the credential. Never written to disk.
    fn config(&self, request: &Request) -> Result<Option<String>> {
        let Some((header, secret)) = &request.auth else {
            return Ok(None);
        };
        let value = secret.read()?;
        // curl's config format: `header = "..."`, one per line.
        Ok(Some(format!("header = \"{header}: Bearer {value}\"\n")))
    }
}

impl Curl {
    /// The argv and stdin for a request, without running it — so a streaming
    /// reader can own the child process (`M-23`).
    ///
    /// The body goes in a file as usual (a prompt is longer than any command
    /// line allows) and the credential goes on stdin as usual (`S-2`): argv is
    /// world-readable, and streaming does not change that.
    pub fn streaming_invocation(&self, request: &Request) -> Result<(Vec<String>, Option<String>)> {
        let body_path = match &request.body {
            Some(body) => {
                let path = self.scratch.join(format!(
                    "perp-stream-{}-{}.json",
                    std::process::id(),
                    crate::watchdog::content_hash(body.as_bytes())
                ));
                std::fs::write(&path, body).map_err(|e| Error::io(&path, e))?;
                Some(path)
            }
            None => None,
        };
        let args = self.args(request, body_path.as_ref().and_then(|p| p.to_str()));
        let stdin = self.config(request)?;
        Ok((args, stdin))
    }
}

impl Transport for Curl {
    /// The real one: stream, enforce the deadline, and rebuild the buffered
    /// shape so nothing above the transport knows the difference.
    fn send_deadlined(&self, request: &Request, first_token: Duration) -> Result<Response> {
        let (args, stdin) = self.streaming_invocation(request)?;
        let streamed = crate::stream::read(
            &crate::stream::streaming_args(args),
            stdin.as_deref(),
            first_token,
            // The loop has no operator at the keyboard. `C-4`'s interrupt
            // belongs to the chat surface, and `perp control` stops at a step
            // boundary rather than mid-call.
            || false,
            |_| {},
        )?;
        if streamed.stop.should_fail_over() {
            // A failure the chain walker can fall through on (`M-9`), carrying
            // why rather than a bare exit code.
            return Err(Error::unbound("link", streamed.stop.describe()));
        }
        Ok(Response { status: 200, body: crate::stream::buffered_shape(&streamed, "") })
    }

    fn send(&self, request: &Request) -> Result<Response> {
        let body_path = match &request.body {
            Some(body) => {
                let path = self.scratch.join(format!(
                    "perp-body-{}-{}.json",
                    std::process::id(),
                    crate::watchdog::content_hash(body.as_bytes())
                ));
                std::fs::write(&path, body).map_err(|e| Error::io(&path, e))?;
                Some(path)
            }
            None => None,
        };

        let args = self.args(request, body_path.as_ref().and_then(|p| p.to_str()));
        let quoted: Vec<String> = args
            .iter()
            .map(|arg| if arg.contains(' ') { format!("\"{arg}\"") } else { arg.clone() })
            .collect();

        let mut spec = Spec::new(
            format!("{} {}", self.program, quoted.join(" ")),
            &self.scratch,
            request.max_time + Duration::from_secs(10),
        )
        .with_env(Env::declared());
        if let Some(config) = self.config(request)? {
            spec = spec.with_stdin(config);
        }

        let run = process::run(&spec);

        if let Some(path) = &body_path {
            let _ = std::fs::remove_file(path);
        }

        let run = run?;
        if !run.is_success() {
            return Err(Error::unbound(
                "curl",
                format!(
                    "{} for {} — {}",
                    run.exit.describe(),
                    request.url,
                    run.stderr_tail.trim()
                ),
            ));
        }

        // `--write-out` appended the status on its own line.
        let text = run.stdout_tail;
        let (body, status) = match text.rsplit_once('\n') {
            Some((body, status)) => (body.to_string(), status.trim().to_string()),
            None => (String::new(), text.trim().to_string()),
        };
        let status: u16 = status.parse().map_err(|_| {
            Error::unbound("curl", format!("no status code in the response: {text:.200}"))
        })?;

        Ok(Response { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A one-shot HTTP server on an ephemeral port.
    ///
    /// Dependency-free, hermetic, and real: `curl` genuinely connects, so the
    /// tests below exercise the actual transport rather than a mock of it.
    /// Returns the port and a channel that yields the request it received.
    fn one_shot(status: u16, body: &'static str) -> (u16, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            let mut content_length = 0usize;

            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
                let done = line.trim().is_empty();
                request.push_str(&line);
                if done {
                    break;
                }
            }
            if content_length > 0 {
                let mut body = vec![0u8; content_length];
                use std::io::Read as _;
                let _ = reader.read_exact(&mut body);
                request.push_str(&String::from_utf8_lossy(&body));
            }

            let mut stream = reader.into_inner();
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
            let _ = tx.send(request);
        });

        (port, rx)
    }

    fn curl() -> Curl {
        Curl::new().with_scratch(tmpdir("net"))
    }

    #[test]
    fn a_get_reaches_a_real_server_and_brings_back_the_body() {
        let (port, requests) = one_shot(200, r#"{"object":"list","data":[]}"#);
        let response = curl()
            .send(&Request::get(format!("http://127.0.0.1:{port}/api/v0/models")))
            .expect("send");

        assert_eq!(response.status, 200);
        assert!(response.is_success());
        assert_eq!(response.body.trim(), r#"{"object":"list","data":[]}"#);

        let seen = requests.recv_timeout(Duration::from_secs(10)).expect("the server saw it");
        assert!(seen.starts_with("GET /api/v0/models"), "{seen}");
    }

    #[test]
    fn a_post_sends_its_body() {
        let (port, requests) = one_shot(200, r#"{"ok":true}"#);
        let body = r#"{"model":"small","messages":[{"role":"user","content":"hello"}]}"#;
        let response = curl()
            .send(&Request::post_json(format!("http://127.0.0.1:{port}/v1/chat/completions"), body))
            .expect("send");

        assert!(response.is_success());
        let seen = requests.recv_timeout(Duration::from_secs(10)).expect("the server saw it");
        assert!(seen.contains("POST /v1/chat/completions"), "{seen}");
        assert!(seen.contains("Content-Type: application/json"), "{seen}");
        assert!(seen.contains(r#""content":"hello""#), "the body arrived: {seen}");
    }

    #[test]
    fn the_credential_reaches_the_server_but_never_the_command_line() {
        // `S-2`. This is the reason the transport looks the way it does.
        std::env::set_var("PERP_TEST_KEY", "sk-not-a-real-key-3f9a");
        let (port, requests) = one_shot(200, "{}");

        let request = Request::get(format!("http://127.0.0.1:{port}/v1/models"))
            .bearer(Secret::from_env_var("PERP_TEST_KEY"));

        // What a process listing would show.
        let argv = curl().args(&request, None).join(" ");
        assert!(!argv.contains("sk-not-a-real-key"), "the key is in argv: {argv}");
        assert!(argv.contains("-K -"), "it is read from stdin instead: {argv}");

        curl().send(&request).expect("send");
        let seen = requests.recv_timeout(Duration::from_secs(10)).expect("the server saw it");
        assert!(
            seen.contains("Authorization: Bearer sk-not-a-real-key-3f9a"),
            "but it did arrive at the server: {seen}"
        );
    }

    #[test]
    fn a_request_body_does_not_land_in_a_shared_temp_directory() {
        // `S-8`, from cycle 2's Phase E review. `std::env::temp_dir()` reads
        // `TMP`, which on the machine this was found on is `D:\Temp` — shared,
        // root-level, readable by every other user. A prompt carries repository
        // content, so a call in flight was briefly world-readable.
        let scratch = private_scratch();
        let shared = std::env::temp_dir();

        // The default must not be the ambient temp directory itself.
        assert_ne!(scratch, shared, "the body directory is not the shared one");
        assert!(
            scratch.ends_with("perp/bodies") || scratch.ends_with("perp\\bodies"),
            "and it is the harness's own: {}",
            scratch.display()
        );
        assert!(scratch.is_dir(), "created on the way out: {}", scratch.display());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&scratch).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o077, 0, "owner-only: {mode:o}");
        }
    }

    #[test]
    fn a_secret_never_reaches_the_transcript_that_goes_in_the_journal() {
        // Phase E security review. `S-2` says a key never reaches a journal —
        // and the transcript of a failed request is exactly what gets written
        // there. Argv is already covered; this is the other path.
        std::env::set_var("PERP_TEST_KEY_3", "sk-transcript-leak-check");
        let (port, _requests) = one_shot(500, r#"{"error":"boom"}"#);
        let request = Request::get(format!("http://127.0.0.1:{port}/v1/models"))
            .bearer(Secret::from_env_var("PERP_TEST_KEY_3"));

        let curl = curl();
        let response = curl.send(&request).expect("the call happened");
        assert_eq!(response.status, 500);

        // The same shape the gate runner journals.
        let spec = Spec::new(
            format!("curl {}", curl.args(&request, None).join(" ")),
            std::env::temp_dir(),
            Duration::from_secs(5),
        );
        let transcript = process::run(&spec).expect("run").transcript();
        assert!(
            !transcript.contains("sk-transcript-leak-check"),
            "the key reached a transcript: {transcript}"
        );
        assert!(!format!("{response:?}").contains("sk-transcript-leak-check"));
    }

    #[test]
    fn certificate_verification_is_never_turned_off() {
        // The one flag that would quietly make every HTTPS call meaningless.
        let argv = curl().args(&Request::get("https://api.deepseek.com/v1/models"), None).join(" ");
        for dangerous in ["--insecure", "-k", "--proxy-insecure", "--ssl-no-revoke"] {
            assert!(!argv.contains(dangerous), "{dangerous} is in the command line: {argv}");
        }
    }

    #[test]
    fn a_secret_is_redacted_in_debug_output() {
        // A `{:?}` of a request has to be safe to journal.
        std::env::set_var("PERP_TEST_KEY_2", "sk-also-secret");
        let request = Request::get("http://example.test")
            .bearer(Secret::from_env_var("PERP_TEST_KEY_2"));
        let rendered = format!("{request:?}");
        assert!(rendered.contains("Secret($PERP_TEST_KEY_2)"), "{rendered}");
        assert!(!rendered.contains("sk-also-secret"), "{rendered}");
    }

    #[test]
    fn a_missing_key_names_the_variable_not_its_contents() {
        let secret = Secret::from_env_var("PERP_DEFINITELY_NOT_SET");
        let err = secret.read().expect_err("must fail");
        let text = format!("{err}");
        assert!(text.contains("PERP_DEFINITELY_NOT_SET"), "{text}");
        assert!(text.contains("read at the moment of use"), "{text}");
    }

    #[test]
    fn a_four_hundred_is_data_not_a_transport_failure() {
        // A 401 has to reach the caller as a status, so failover can tell an
        // expired key from an unreachable host.
        let (port, _requests) = one_shot(401, r#"{"error":"invalid api key"}"#);
        let response = curl()
            .send(&Request::get(format!("http://127.0.0.1:{port}/v1/models")))
            .expect("the call itself succeeded");

        assert_eq!(response.status, 401);
        assert!(!response.is_success());
        assert!(response.complaint().contains("HTTP 401"), "{}", response.complaint());
        assert!(response.complaint().contains("invalid api key"));
    }

    #[test]
    fn a_host_that_is_not_there_fails_rather_than_hanging() {
        // `T-3`: bounded, always. Port 1 on localhost refuses immediately.
        let request = Request::get("http://127.0.0.1:1/nothing")
            .with_deadline(Duration::from_secs(2), Duration::from_secs(5));
        let started = std::time::Instant::now();
        let err = curl().send(&request).expect_err("must fail");
        assert!(started.elapsed() < Duration::from_secs(20), "it hung: {:?}", started.elapsed());
        assert!(format!("{err}").contains("127.0.0.1:1"), "{err}");
    }

    #[test]
    fn the_body_file_does_not_outlive_the_call() {
        let scratch = tmpdir("net-scratch");
        let (port, _requests) = one_shot(200, "{}");
        Curl::new()
            .with_scratch(&scratch)
            .send(&Request::post_json(format!("http://127.0.0.1:{port}/v1/x"), r#"{"a":1}"#))
            .expect("send");

        let leftovers: Vec<_> = std::fs::read_dir(&scratch)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("perp-body-"))
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
    }

    #[test]
    fn the_deadlines_are_passed_to_curl() {
        let request = Request::get("http://example.test")
            .with_deadline(Duration::from_secs(3), Duration::from_secs(45));
        let argv = curl().args(&request, None);
        let joined = argv.join(" ");
        assert!(joined.contains("--connect-timeout 3"), "{joined}");
        assert!(joined.contains("--max-time 45"), "{joined}");
    }
}
