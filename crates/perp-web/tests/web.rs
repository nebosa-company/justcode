//! End to end: the real server, as a subprocess, over a real workspace
//! (`N-12`, `L-34`).
//!
//! A unit test over the router would not catch the things that actually go
//! wrong here — a header read case-sensitively, a body whose length was never
//! honoured, a guard that returns the wrong status. So this starts the binary,
//! talks HTTP to it over a socket, and asserts on the bytes that come back.
//!
//! The three that matter most are not features at all. This server writes a
//! person's requirements file, so the tests below assert that a request without
//! the page's own header cannot, that a `Host` which is not loopback cannot,
//! and that no request of any shape can put a `✅` in the file.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perp-web"))
}

const SOURCE: &str = "# requirements\n\n\
     | id | Requirement |\n|---|---|\n\
     | ✅ ~~`L-1`~~ | the journal is append-only. |\n\
     | ⛔ `L-2` | a thing. **Gated: dependency approval** |\n\
     | `L-3` | the journal |\n\
     | ❌ `L-4` | not doing this one. |\n";

/// A workspace and a server over it, stopped when the test ends.
struct Server {
    child: Child,
    port: u16,
    root: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(tag: &str) -> Server {
        let root = std::env::temp_dir().join(format!("perp-web-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".harness")).expect("dirs");
        std::fs::write(root.join(".harness/perpetum.md"), SOURCE).expect("requirements");
        std::fs::write(
            root.join(".harness/binding.md"),
            "# Binding\n\n\
             ```perp-binding\n\
             path.requirements = .harness/perpetum.md\n\
             out.journal       = .harness/journal.jsonl\n\
             out.state         = .harness/state.md\n\
             ```\n",
        )
        .expect("binding");

        // Ask the OS for a free port and hand it straight on. Two tests running
        // at once on a fixed port would fail each other, which is a flake in
        // the suite rather than in the code.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("a free port")
            .local_addr()
            .expect("its address")
            .port();

        let child = Command::new(binary())
            .args(["--root", &root.to_string_lossy(), "--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("perp-web should be runnable");

        let server = Server { child, port, root };
        server.wait_until_listening();
        server
    }

    fn wait_until_listening(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("perp-web never listened on {}", self.port);
    }

    /// One request, one response, `Connection: close`.
    fn request(&self, method: &str, path: &str, headers: &[&str], body: &str) -> Reply {
        let mut stream =
            TcpStream::connect(("127.0.0.1", self.port)).expect("the server is listening");
        stream.set_read_timeout(Some(Duration::from_secs(10))).expect("timeout");
        let mut request = format!("{method} {path} HTTP/1.1\r\n");
        for header in headers {
            request.push_str(header);
            request.push_str("\r\n");
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
        stream.write_all(request.as_bytes()).expect("write");
        stream.flush().expect("flush");

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read");
        let text = String::from_utf8_lossy(&raw).into_owned();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = text.split_once("\r\n\r\n").map_or(String::new(), |(_, rest)| rest.to_string());
        Reply { status, body }
    }

    fn get(&self, path: &str) -> Reply {
        self.request("GET", path, &["Host: 127.0.0.1"], "")
    }

    /// A write the way the page makes it: with the header the page sends.
    fn write(&self, body: &str) -> Reply {
        self.request(
            "POST",
            "/api/write",
            &[
                "Host: 127.0.0.1",
                "X-Perp-Web: 1",
                "Content-Type: application/x-www-form-urlencoded",
            ],
            body,
        )
    }

    fn source(&self) -> String {
        std::fs::read_to_string(self.root.join(".harness/perpetum.md")).unwrap_or_default()
    }
}

struct Reply {
    status: u16,
    body: String,
}

fn source_of(root: &Path) -> String {
    std::fs::read_to_string(root.join(".harness/perpetum.md")).unwrap_or_default()
}

// ── the read half (L-34) ───────────────────────────────────────────────────

#[test]
fn the_page_and_its_view_come_back_over_a_socket() {
    let server = Server::start("read");

    let page = server.get("/");
    assert_eq!(page.status, 200);
    assert!(page.body.contains("<title>Perpetum"), "{}", page.body);
    // `A-5`: self-contained. Nothing to fetch, so nothing to fail to fetch.
    assert!(!page.body.contains("http://"), "no outbound reference: {}", page.body);
    assert!(!page.body.contains("https://"), "no outbound reference: {}", page.body);

    let view = server.get("/api/view");
    assert_eq!(view.status, 200);
    assert!(view.body.contains("\"id\":\"L-3\""), "{}", view.body);
    // Four rows, one of them done: 25%. Counted from the markers, never stored.
    assert!(view.body.contains("\"done\":1"), "{}", view.body);
    assert!(view.body.contains("\"total\":4"), "{}", view.body);
    assert!(view.body.contains("\"percent\":25"), "{}", view.body);
    // Including the row nobody will build, which is the point of the catalogue.
    assert!(view.body.contains("won't do"), "{}", view.body);
    assert!(view.body.contains("\"next_id\":\"L-5\""), "{}", view.body);
}

// ── the write half, and what it will not do (L-34, V-2, V-12) ──────────────

#[test]
fn add_edit_and_delete_go_through_and_land_in_the_file() {
    let server = Server::start("write");

    let added = server.write("action=add&text=a+web+front+end+over+a+workspace");
    assert_eq!(added.status, 200, "{}", added.body);
    assert!(server.source().contains("| `L-5` | a web front end over a workspace |"), "{}", server.source());

    let edited = server.write("action=edit&id=L-5&text=a+web+front+end%2C+reworded");
    assert_eq!(edited.status, 200, "{}", edited.body);
    assert!(server.source().contains("| `L-5` | a web front end, reworded |"), "{}", server.source());

    let deleted = server.write("action=delete&id=L-5");
    assert_eq!(deleted.status, 200, "{}", deleted.body);
    assert!(deleted.body.contains("reworded"), "it says what it removed: {}", deleted.body);
    assert!(!server.source().contains("L-5"), "{}", server.source());
}

/// An edit reaches the text cell and never the status cell — the reason `L-34`
/// gives `edit` its own command instead of a file write.
#[test]
fn an_edit_over_a_done_row_leaves_the_marker_where_it_was() {
    let server = Server::start("marker");

    let edited = server.write("action=edit&id=L-1&text=the+journal+is+append-only%2C+always.");
    assert_eq!(edited.status, 200, "{}", edited.body);
    assert!(
        server.source().contains("| ✅ ~~`L-1`~~ | the journal is append-only, always. |"),
        "{}",
        server.source()
    );
    assert!(server.get("/api/view").body.contains("\"done\":1"), "still one done");
}

/// `V-2`, over HTTP. Nothing this server accepts puts a green in the file.
#[test]
fn no_request_shape_can_write_a_marker() {
    let server = Server::start("no-green");
    // The **status cells**, which is where a marker means something. A `✅` in
    // the text of a row is prose (`V-23`) — the requirements this project
    // writes about markers contain them — so counting the character in the
    // file would be testing the wrong thing, and would forbid the right one.
    let states = states_of(&server);

    for body in [
        "action=done&id=L-3",
        "action=mark&id=L-3&state=done",
        "action=state&id=L-3&text=done",
        // The obvious one: put the marker in the text and hope the cell moves.
        "action=edit&id=L-3&text=%E2%9C%85+~~%60L-3%60~~+done",
        // And the one that would end the row it is written into.
        "action=add&text=a+thing+%7C+%E2%9C%85+done",
    ] {
        let reply = server.write(body);
        assert!(reply.status == 200 || reply.status == 400, "{}: {}", reply.status, reply.body);
    }

    assert_eq!(states_of(&server), states, "{}", server.source());
    let view = server.get("/api/view").body;
    assert!(view.contains("\"done\":1"), "one row is done, the one that already was: {view}");
}

/// Every row's status cell, taken straight out of the file. The cell, never
/// the line (`V-23`).
fn states_of(server: &Server) -> Vec<String> {
    server
        .source()
        .lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter_map(|line| line.split('|').nth(1).map(str::to_string))
        .collect()
}

/// The write door is the allowlist, and an action that is not on it is refused
/// by name rather than being ignored.
#[test]
fn an_action_off_the_allowlist_is_refused_saying_what_is_on_it() {
    let server = Server::start("allowlist");
    let refused = server.write("action=rewind&id=L-3");
    assert_eq!(refused.status, 400);
    assert!(refused.body.contains("requirement add"), "{}", refused.body);
    assert!(refused.body.contains("requirement delete"), "{}", refused.body);
    assert!(refused.body.contains("ungate"), "{}", refused.body);
}

// ── the two ways a browser is talked into reaching a localhost server ───────

/// Any page open in the browser can post to this port. What it cannot do is
/// set a header — so the header is the check.
#[test]
fn a_write_without_the_pages_own_header_is_refused() {
    let server = Server::start("csrf");
    let before = server.source();

    let forged = server.request(
        "POST",
        "/api/write",
        &["Host: 127.0.0.1", "Content-Type: application/x-www-form-urlencoded"],
        "action=delete&id=L-3",
    );
    assert_eq!(forged.status, 403, "{}", forged.body);
    assert_eq!(server.source(), before, "and nothing was written");

    // The preflight that a cross-origin `fetch` would need is not answered
    // either, which is what stops the header being set from another origin.
    let preflight = server.request("OPTIONS", "/api/write", &["Host: 127.0.0.1"], "");
    assert_eq!(preflight.status, 405);
    assert!(!preflight.body.contains("Access-Control"), "{}", preflight.body);
}

/// A name someone else controls, resolving to 127.0.0.1, is how a localhost
/// server becomes a public one. The `Host` header is what carries it.
#[test]
fn a_request_that_arrived_under_another_name_is_refused() {
    let server = Server::start("rebind");
    let rebound = server.request("GET", "/api/view", &["Host: attacker.example"], "");
    assert_eq!(rebound.status, 403, "{}", rebound.body);
    assert!(rebound.body.contains("localhost"), "{}", rebound.body);

    // And the same for a write, which is the one that would cost something.
    let write = server.request(
        "POST",
        "/api/write",
        &["Host: attacker.example", "X-Perp-Web: 1"],
        "action=delete&id=L-3",
    );
    assert_eq!(write.status, 403);
    assert!(source_of(&server.root).contains("`L-3`"));
}

/// This server reads a repository. A path it can be talked into is a file it
/// can be talked out of.
#[test]
fn the_evidence_route_serves_evidence_and_nothing_else() {
    let server = Server::start("traversal");
    for path in [
        "/evidence/../binding.md",
        "/evidence/..%2Fbinding.md",
        "/evidence/%2e%2e%2f%2e%2e%2fperpetum.md",
        "/evidence/perpetum.md",
        "/evidence/",
    ] {
        let reply = server.get(path);
        assert!(reply.status >= 400, "{path} came back {}: {}", reply.status, reply.body);
        assert!(!reply.body.contains("perp-binding"), "{path} leaked the binding");
        assert!(!reply.body.contains("append-only"), "{path} leaked the requirements");
    }
}

/// No `product.run` in the binding means no command to run, and the refusal
/// names the key rather than guessing at what the product might be.
#[test]
fn capture_refuses_when_the_workspace_declares_no_product() {
    let server = Server::start("no-product");
    let reply = server.request(
        "POST",
        "/api/capture",
        &["Host: 127.0.0.1", "X-Perp-Web: 1"],
        "claim=the+window",
    );
    assert_eq!(reply.status, 400, "{}", reply.body);
    assert!(reply.body.contains("product.run"), "{}", reply.body);
    assert!(server.get("/api/view").body.contains("\"declared\":false"));
}

// ── ideas: a model drafts, a person files (L-34) ───────────────────────────

/// The dividing line the whole feature rests on: drafting and filing are two
/// requests, and the drafting one has no path to the file.
#[test]
fn deriving_from_an_idea_writes_nothing_by_itself() {
    let server = Server::start("ideas");
    let before = server.source();

    let derived = server.request(
        "POST",
        "/api/ideas",
        &["Host: 127.0.0.1", "X-Perp-Web: 1"],
        "idea=a+way+to+see+what+the+loop+is+doing",
    );
    // This fixture declares no links, so there is no model — and the refusal
    // names that rather than inventing candidates.
    assert_eq!(derived.status, 400, "{}", derived.body);
    assert!(derived.body.contains("links"), "{}", derived.body);
    assert_eq!(server.source(), before);

    // An empty idea is refused before any link is consulted.
    let empty = server.request(
        "POST",
        "/api/ideas",
        &["Host: 127.0.0.1", "X-Perp-Web: 1"],
        "idea=+++",
    );
    assert_eq!(empty.status, 400);
    assert!(empty.body.contains("nothing here"), "{}", empty.body);
    assert_eq!(server.source(), before);
}

/// The page says where an idea would go before there is one to send.
#[test]
fn the_view_says_which_link_would_see_an_idea() {
    let server = Server::start("ideas-where");
    let view = server.get("/api/view").body;
    assert!(view.contains("\"available\":false"), "{view}");
    assert!(view.contains("path.links"), "the reason names the key: {view}");
}

/// Drafting spends tokens and reaches a model, so it is a write-shaped
/// request even though it writes no file — and it goes through the same
/// same-origin guard.
#[test]
fn drafting_needs_the_pages_own_header_too() {
    let server = Server::start("ideas-csrf");
    let forged = server.request(
        "POST",
        "/api/ideas",
        &["Host: 127.0.0.1"],
        "idea=something",
    );
    assert_eq!(forged.status, 403, "{}", forged.body);
}

/// There is no batch write. Filing five candidates is five `requirement add`
/// calls, each through the allowlist, because a second way to write the file
/// is a second thing to get right.
#[test]
fn there_is_no_endpoint_that_files_more_than_one_row() {
    let server = Server::start("ideas-batch");
    for body in [
        "action=add&text=one&text=two",
        "action=add_many&text=one",
        "action=file&texts=one,two",
    ] {
        let reply = server.write(body);
        // The first is an ordinary add that takes the first value and nothing
        // more; the rest are refused by name.
        if reply.status == 200 {
            assert!(!server.source().contains("two"), "only one row: {}", server.source());
        } else {
            assert_eq!(reply.status, 400, "{}", reply.body);
        }
    }
}
