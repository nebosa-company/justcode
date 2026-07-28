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
use std::path::PathBuf;
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

impl Curl {
    pub fn new() -> Curl {
        Curl { program: "curl".to_string(), scratch: std::env::temp_dir() }
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

impl Transport for Curl {
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
