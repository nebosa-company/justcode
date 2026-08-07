//! Enough HTTP to serve one page to one browser on one machine.
//!
//! Hand-written, because `N-11` puts the standard library first and this is a
//! server that accepts connections from `127.0.0.1` and nowhere else. A crate
//! for that would be a dependency taken for the part of the problem that is
//! already easy — HTTP/1.1 with `Connection: close` is a request line, some
//! headers and a length.
//!
//! What is *not* easy is the two attacks a localhost server actually faces, and
//! both are answered here rather than in the routes:
//!
//! - **Cross-site request forgery.** Any page in the browser can POST to
//!   `http://127.0.0.1:7878/`, and this server writes a person's requirements
//!   file. A form post cannot set a custom header without a CORS preflight, and
//!   nothing here answers a preflight — so [`Request::is_same_origin`] requires
//!   `X-Perp-Web: 1` on every write, and the page's own `fetch` sends it.
//! - **DNS rebinding.** A hostile name that resolves to `127.0.0.1` turns a
//!   localhost server into a public one. [`Request::is_local_host`] refuses any
//!   `Host` that is not loopback, which is the header that attack cannot forge.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

/// A request, read far enough to route it.
pub struct Request {
    pub method: String,
    /// The path only. A query string is split off and dropped — nothing here
    /// is addressed by one, and a route that quietly accepted `?action=delete`
    /// would be a write reachable from a link.
    pub path: String,
    headers: Vec<(String, String)>,
    pub body: String,
}

/// The most a request body may be. A requirement is a sentence; anything past
/// this is either a mistake or somebody exploring.
const MAX_BODY: usize = 64 * 1024;

impl Request {
    pub fn read(stream: &mut TcpStream) -> Option<Request> {
        let mut reader = BufReader::new(stream.try_clone().ok()?);

        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        let mut parts = line.split_whitespace();
        let method = parts.next()?.to_string();
        let target = parts.next()?;
        let path = target.split_once('?').map_or(target, |(path, _)| path).to_string();

        let mut headers = Vec::new();
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).ok()? == 0 {
                break;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
            }
        }

        let length: usize = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .and_then(|(_, value)| value.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length.min(MAX_BODY)];
        if !body.is_empty() {
            reader.read_exact(&mut body).ok()?;
        }

        Some(Request {
            method,
            path,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// Whether this came from the page this server served (`L-34`).
    ///
    /// The custom header is the whole check, and it is enough: a cross-origin
    /// `<form>` or `<img>` cannot set one, and a cross-origin `fetch` that
    /// tries triggers a preflight this server answers with `405` — so the write
    /// never arrives. Checked on writes only; a GET changes nothing.
    pub fn is_same_origin(&self) -> bool {
        self.header("x-perp-web") == Some("1")
    }

    /// Whether the browser thinks it is talking to loopback.
    ///
    /// The socket is bound to `127.0.0.1`, so every connection is local — but
    /// a name under someone else's control can resolve there, and then the
    /// *browser* treats the origin as theirs while the request still arrives
    /// here. The `Host` header is what carries that name, and refusing anything
    /// that is not loopback is what closes it.
    pub fn is_local_host(&self) -> bool {
        let Some(host) = self.header("host") else { return false };
        let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
        matches!(name.trim_matches(['[', ']']), "localhost" | "127.0.0.1" | "::1")
    }

    /// One field of a form-encoded body.
    ///
    /// Form encoding rather than JSON on purpose: the harness has a JSON writer
    /// and the browser has `URLSearchParams`, and a percent decoder is twenty
    /// lines that cannot fail in an interesting way.
    pub fn field(&self, name: &str) -> Option<String> {
        for pair in self.body.split('&') {
            let Some((key, value)) = pair.split_once('=') else { continue };
            if percent_decode(key) == name {
                return Some(percent_decode(value));
            }
        }
        None
    }
}

/// `%XX` and `+`, and nothing else. Invalid escapes are left as written rather
/// than dropped: a requirement containing a stray `%` should arrive containing
/// a stray `%`.
pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            b'%' if at + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        at += 3;
                    }
                    Err(_) => {
                        out.push(bytes[at]);
                        at += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What goes back. Bytes rather than a string, because a PNG is one of them.
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    pub fn html(body: String) -> Response {
        Response { status: 200, content_type: "text/html; charset=utf-8", body: body.into_bytes() }
    }

    pub fn json(body: String) -> Response {
        Response {
            status: 200,
            content_type: "application/json; charset=utf-8",
            body: body.into_bytes(),
        }
    }

    pub fn png(body: Vec<u8>) -> Response {
        Response { status: 200, content_type: "image/png", body }
    }

    /// A refusal, as text the page shows verbatim.
    ///
    /// Verbatim matters here: the write allowlist refuses by naming what is
    /// allowed, and an error the front end rewrites into "something went wrong"
    /// is the one sentence a person cannot act on.
    pub fn refused(status: u16, why: impl Into<String>) -> Response {
        Response {
            status,
            content_type: "text/plain; charset=utf-8",
            body: why.into().into_bytes(),
        }
    }

    pub fn write_to(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let reason = match self.status {
            200 => "OK",
            400 => "Bad Request",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            _ => "Error",
        };
        let head = format!(
            "HTTP/1.1 {} {reason}\r\n\
             Content-Type: {}\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-store\r\n\
             X-Content-Type-Options: nosniff\r\n\
             Connection: close\r\n\r\n",
            self.status,
            self.content_type,
            self.body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding_survives_prose() {
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%25"), "100%");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
        // A stray escape is left as written rather than swallowed.
        assert_eq!(percent_decode("50% of"), "50% of");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
