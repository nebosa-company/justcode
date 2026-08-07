//! A web front end over a Perpetum workspace (`L-34`).
//!
//! Requirements on the left, with add, edit and delete. Progress on the right,
//! as a percentage of the marker states rather than a number anybody types.
//! Screenshots of the product under construction underneath it.
//!
//! Three decisions shape the whole binary, and the first is the one everything
//! else follows from.
//!
//! **1. There is no file-write endpoint, and there will not be one.** `V-12`
//! reserves writes to the requirements source to a person. The tempting design
//! — a textarea and a `POST /requirements` that writes what it is handed — puts
//! the file in reach of anything that can reach this port, and turns the person
//! into the transport rather than the author. So every write is parsed into a
//! [`perp_core::requirement::Write`]: `add`, `edit`, `delete`, `ungate`, and a
//! request that is not one of those four is refused by name. That type is the
//! allowlist, shared with the CLI and the editor's panel, so three doors cannot
//! drift into three opinions about what a person may do.
//!
//! **2. Nothing here can write a `✅`** (`V-2`). Not the add form, which files
//! a row with no marker; not the edit form, which replaces a row's text cell
//! and carries its status cell across verbatim. A marker means gates went green
//! with a transcript. A front end that can type one is a front end that makes
//! every green meaningless.
//!
//! **3. It listens on loopback only, and says so.** `127.0.0.1`, no `--host`,
//! no configuration to make it otherwise. A page that writes a person's
//! requirements file is not a page to put on a network, and the guards against
//! the two ways a browser gets tricked into reaching it anyway are in
//! [`http`].
//!
//! The read half is `perp panel`'s and is not rebuilt here: the catalogue comes
//! from [`perp_core::cycle::catalogue`] and the screenshots from the journal's
//! own capture records, so this page cannot show anything the CLI cannot.

mod http;
mod page;

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use perp_core::binding::Binding;
use perp_core::capture::{Capture, Product};
use perp_core::journal::Journal;
use perp_core::json::{to_string, Value};
use perp_core::requirement::Write;

use http::{Request, Response};

const USAGE: &str = "\
perp-web — the requirements of a Perpetum workspace, in a browser (`L-34`)

  perp-web [--root <dir>] [--port <n>] [--open]

  --root <dir>   the workspace. Defaults to the nearest one at or above here.
  --port <n>     the port on 127.0.0.1. Defaults to 7878.
  --open         open the page in the default browser once it is listening.

It serves one page: every requirement the source declares with add, edit and
delete; the percentage done, counted from the markers; and the screenshots the
journal recorded, with what each one is evidence of.

It listens on 127.0.0.1 and there is no flag to change that. The page writes the
requirements source, which `V-12` reserves to a person — a port on a network is
not a person.

Add to the binding to photograph the product under construction:

  product.run    = <the command that starts it>
  product.settle = <seconds to let it draw, default 4>
";

const DEFAULT_PORT: u16 = 7878;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    if refs.iter().any(|arg| matches!(*arg, "--help" | "-h" | "help")) {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    match serve(&refs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("perp-web: {message}");
            ExitCode::FAILURE
        }
    }
}

fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter().position(|arg| *arg == name).and_then(|at| args.get(at + 1)).copied()
}

fn serve(args: &[&str]) -> Result<(), String> {
    let root = match flag(args, "--root") {
        Some(given) => PathBuf::from(given),
        None => perp_core::layout::enclosing(&std::env::current_dir().unwrap_or_default())
            .unwrap_or_else(|| PathBuf::from(".")),
    };
    let port: u16 = match flag(args, "--port") {
        Some(text) => text.parse().map_err(|_| format!("--port takes a number, not `{text}`"))?,
        None => DEFAULT_PORT,
    };

    // Loaded once to fail early with the binding's own message, and reloaded
    // per request so a `product.run` added while the page is open is picked up
    // without a restart.
    let binding = Binding::load(&root).map_err(|e| e.to_string())?;
    binding.verify().map_err(|e| e.to_string())?;
    let root = binding.root().to_path_buf();

    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = TcpListener::bind(address).map_err(|e| {
        format!("cannot listen on {address}: {e} — another perp-web, or another program on {port}")
    })?;

    let url = format!("http://127.0.0.1:{port}/");
    println!("perp-web · {}", root.display());
    println!("{url}");
    println!("requirements: {}", binding.get("path.requirements").unwrap_or("?"));
    match Product::declared(&binding) {
        Ok(product) => println!("product:      {}", product.command),
        Err(_) => println!("product:      none declared — set `product.run` to photograph it"),
    }
    println!("stop with ctrl-c");

    if args.contains(&"--open") {
        // Best effort, and loudly not fatal: a server that refuses to start
        // because a browser did not is a server that is harder to use than the
        // URL it just printed.
        if let Err(e) = perp_core::browser::launch_url(&url, Some(0)) {
            eprintln!("perp-web: could not open a browser ({e}) — the URL is above");
        }
    }

    for incoming in listener.incoming() {
        let mut stream = match incoming {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("perp-web: dropped a connection: {e}");
                continue;
            }
        };
        let root = root.clone();
        // A thread per connection, with `Connection: close` on every response
        // so none of them outlive their request. One browser is the whole load;
        // a pool would be machinery for a queue that is never more than one
        // deep.
        std::thread::spawn(move || {
            // A client that opens a socket and says nothing would otherwise
            // hold the thread for as long as the process lives.
            let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
            let response = match Request::read(&mut stream) {
                Some(request) => route(&request, &root),
                None => Response::refused(400, "could not read that request"),
            };
            let _ = response.write_to(&mut stream);
        });
    }
    Ok(())
}

fn route(request: &Request, root: &Path) -> Response {
    // Before anything else, and for both reads and writes: the browser must
    // believe it is talking to loopback. A name resolving here is how a
    // localhost server becomes a public one.
    if !request.is_local_host() {
        return Response::refused(
            403,
            "perp-web answers to localhost only. That Host header is not one.",
        );
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => Response::html(page::HTML.to_string()),
        ("GET", "/api/view") => match view(root) {
            Ok(json) => Response::json(json),
            Err(why) => Response::refused(400, why),
        },
        ("GET", path) if path.starts_with("/evidence/") => evidence(root, path),
        ("POST", "/api/write") => guard(request, |request| write(request, root)),
        ("POST", "/api/capture") => guard(request, |request| photograph(request, root)),
        ("POST", "/api/ideas") => guard(request, |request| ideas(request, root)),
        ("OPTIONS", _) => Response::refused(
            405,
            "no preflight is answered here, which is deliberate: it is what keeps another \
             origin's script from writing this workspace's requirements.",
        ),
        ("GET" | "POST", path) => Response::refused(404, format!("nothing is served at {path}")),
        (method, _) => Response::refused(405, format!("{method} is not answered here")),
    }
}

/// Every write goes through here, so the same-origin check cannot be forgotten
/// by whoever adds the next route.
fn guard(request: &Request, then: impl FnOnce(&Request) -> Response) -> Response {
    if !request.is_same_origin() {
        return Response::refused(
            403,
            "this write did not come from the page perp-web served. Writes carry \
             `X-Perp-Web: 1`, which a cross-origin form cannot set.",
        );
    }
    then(request)
}

// --------------------------------------------------------------------- reads

fn binding_of(root: &Path) -> Result<Binding, String> {
    Binding::load(root).map_err(|e| e.to_string())
}

/// Everything the page draws, in one document.
///
/// One request rather than three, for the reason `I-3` gives the panel: a page
/// that fetched the catalogue and the progress separately could show a
/// percentage that disagrees with the list under it.
fn view(root: &Path) -> Result<String, String> {
    let binding = binding_of(root)?;
    let resolved = binding.resolve("path.requirements").map_err(|e| e.to_string())?;
    let source = perp_core::layout::requirements_text(&resolved);
    let catalogue = perp_core::cycle::catalogue(&source);

    // Counted from the rows, never tracked. A stored total is a second thing to
    // update and the one that goes stale.
    let done = catalogue.iter().filter(|entry| entry.state == "done").count();
    let total = catalogue.len();
    let mut states: Vec<(String, usize)> = Vec::new();
    for entry in &catalogue {
        match states.iter_mut().find(|(state, _)| *state == entry.state) {
            Some((_, n)) => *n += 1,
            None => states.push((entry.state.clone(), 1)),
        }
    }

    let product = match Product::declared(&binding) {
        Ok(product) => Value::Obj(vec![
            ("declared".into(), Value::Bool(true)),
            ("command".into(), Value::str(product.command)),
            ("settle".into(), Value::int(as_i64(product.settle.as_secs() as usize))),
        ]),
        Err(why) => Value::Obj(vec![
            ("declared".into(), Value::Bool(false)),
            ("why".into(), Value::str(format!("{why}"))),
        ]),
    };

    Ok(to_string(&Value::Obj(vec![
        ("root".into(), Value::str(perp_core::runtime::normalise(root))),
        ("source".into(), Value::str(perp_core::runtime::normalise(&resolved))),
        (
            "catalogue".into(),
            Value::Arr(
                catalogue
                    .iter()
                    .map(|entry| {
                        Value::Obj(vec![
                            ("id".into(), Value::str(entry.id.clone())),
                            ("name".into(), Value::str(entry.name.clone())),
                            ("text".into(), Value::str(entry.text.clone())),
                            ("state".into(), Value::str(entry.state.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "progress".into(),
            Value::Obj(vec![
                ("done".into(), Value::int(as_i64(done))),
                ("total".into(), Value::int(as_i64(total))),
                // An empty source is 0%, not a division by zero and not 100%:
                // a project that has declared nothing has not finished it.
                ("percent".into(), Value::int(as_i64((done * 100).checked_div(total).unwrap_or(0)))),
                (
                    "states".into(),
                    Value::Arr(
                        states
                            .into_iter()
                            .map(|(state, n)| {
                                Value::Obj(vec![
                                    ("state".into(), Value::str(state)),
                                    ("n".into(), Value::int(as_i64(n))),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
        ),
        ("evidence".into(), Value::Arr(evidence_of(&binding))),
        ("product".into(), product),
        // Where an idea would go, read **before** the dialog is opened. An
        // idea is a person's unpublished thought about their own product, and
        // being told after the fact that it went to a third party is being
        // told after the fact.
        ("ideas".into(), ideas_destination(&binding)),
        ("next_id".into(), perp_core::requirement::next_requirement_id(&source).map_or(Value::Null, Value::str)),
        (
            "writes".into(),
            Value::Arr(Write::ALLOWED.iter().map(|write| Value::str(*write)).collect()),
        ),
    ])))
}

/// The screenshots, read out of the journal rather than off the disk (`I-5`).
///
/// A directory listing would show files; the journal shows *evidence*, because
/// the record carries the claim the picture was taken to support (`A-6`). A PNG
/// whose bytes no longer hash to what was recorded is listed and marked, not
/// hidden: a swapped file is the thing a reader most needs to be told about.
fn evidence_of(binding: &Binding) -> Vec<Value> {
    let Ok(journal_path) = binding.resolve("out.journal") else { return Vec::new() };
    let Ok(records) = Journal::at(&journal_path).read_all() else { return Vec::new() };
    let dir = Capture::dir(&journal_path);

    let mut shots: Vec<Value> = Vec::new();
    for record in &records {
        let Some(shot) = Capture::from_record(record) else { continue };
        let Some(file) = shot.path.file_name().map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        // Served from the evidence directory under this workspace, by name —
        // never by the path in the record. The record is a file the loop
        // appends to, and a path out of it is a path this server would follow.
        if !dir.join(&file).exists() {
            continue;
        }
        shots.push(Value::Obj(vec![
            ("file".into(), Value::str(file)),
            ("claim".into(), Value::str(shot.claim.clone())),
            ("step".into(), Value::str(shot.step.clone())),
            ("at".into(), Value::int(record.at)),
            ("bytes".into(), Value::int(as_i64(shot.bytes))),
            ("intact".into(), Value::Bool(shot.is_intact())),
        ]));
    }
    shots.reverse();
    shots
}

/// Which link would draft from an idea, or why none would.
fn ideas_destination(binding: &Binding) -> Value {
    let unavailable = |why: String| {
        Value::Obj(vec![
            ("available".into(), Value::Bool(false)),
            ("why".into(), Value::str(why)),
        ])
    };
    let Ok(path) = binding.resolve("path.links") else {
        return unavailable("`path.links` is not declared, so there is no model to ask".into());
    };
    let links = match perp_core::link::Links::load(&path) {
        Ok(links) => links,
        Err(why) => return unavailable(format!("{why}")),
    };
    match perp_core::idea::destination(&links) {
        Ok(to) => Value::Obj(vec![
            ("available".into(), Value::Bool(true)),
            ("link".into(), Value::str(to.link)),
            ("model".into(), Value::str(to.model)),
            ("privacy".into(), Value::str(to.privacy)),
        ]),
        Err(why) => unavailable(format!("{why}")),
    }
}

/// Serve one screenshot, by name, out of the evidence directory.
///
/// The name is checked rather than the path: `..` and separators are refused
/// outright, so there is no traversal to resolve and no canonicalisation to get
/// subtly wrong. This server reads a person's repository — a path it can be
/// talked into is a file it can be talked out of.
fn evidence(root: &Path, path: &str) -> Response {
    let file = http::percent_decode(path.trim_start_matches("/evidence/"));
    let safe = !file.is_empty()
        && file.len() < 200
        && file.ends_with(".png")
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !file.contains("..");
    if !safe {
        return Response::refused(403, "that is not an evidence file name");
    }
    let Ok(binding) = binding_of(root) else {
        return Response::refused(400, "the binding could not be read");
    };
    let Ok(journal) = binding.resolve("out.journal") else {
        return Response::refused(400, "`out.journal` is not declared");
    };
    match std::fs::read(Capture::dir(&journal).join(&file)) {
        Ok(bytes) => Response::png(bytes),
        Err(e) => Response::refused(404, format!("{file}: {e}")),
    }
}

// -------------------------------------------------------------------- writes

/// The one write route (`L-34`).
///
/// It takes an action and its arguments, hands them to the allowlist, and does
/// what comes back. There is no branch here per action — adding one would be
/// adding a way for this file to decide what a person may do, and that decision
/// belongs in exactly one place.
fn write(request: &Request, root: &Path) -> Response {
    let action = request.field("action").unwrap_or_default();
    let id = request.field("id").unwrap_or_default();
    let text = request.field("text").unwrap_or_default();

    let (subcommand, args): (&str, Vec<&str>) = match action.as_str() {
        "add" => ("requirement", vec!["add", &text]),
        "edit" => ("requirement", vec!["edit", &id, &text]),
        "delete" => ("requirement", vec!["delete", &id]),
        "ungate" => ("ungate", vec![&id]),
        other => {
            return Response::refused(
                400,
                format!(
                    "`{other}` is not an action. This page does {}.",
                    Write::ALLOWED.join(", ")
                ),
            )
        }
    };

    let write = match Write::parse(subcommand, &args) {
        Ok(write) => write,
        Err(why) => return Response::refused(400, format!("{why}")),
    };
    let binding = match binding_of(root) {
        Ok(binding) => binding,
        Err(why) => return Response::refused(400, why),
    };
    let resolved = match binding.resolve("path.requirements") {
        Ok(path) => path,
        Err(why) => return Response::refused(400, format!("{why}")),
    };
    match write.apply(&resolved) {
        Ok(done) => Response::json(to_string(&Value::Obj(vec![
            ("id".into(), Value::str(done.id)),
            ("summary".into(), Value::str(done.summary)),
            ("note".into(), done.note.map_or(Value::Null, Value::str)),
            ("source".into(), Value::str(perp_core::runtime::normalise(&done.source))),
        ]))),
        Err(why) => Response::refused(400, format!("{why}")),
    }
}

/// Draft candidate requirements from an idea (`L-34`).
///
/// **This route writes nothing, and that is the point.** It is deliberately not
/// `/api/write` with a fifth action: what comes back are strings, and every one
/// of them reaches the requirements source only by a person ticking it and the
/// page posting it to `/api/write` as `requirement add` — through the same
/// allowlist as a typed command. A route that drafted and filed in one call
/// would be a model filing its own requirements with a click as the fig leaf,
/// which is `V-12` gone.
///
/// It blocks for as long as the model takes, which for a `claude-cli` link is
/// tens of seconds. That is fine here and would not be in the editor's panel:
/// every connection has its own thread, so a slow draft holds up nothing but
/// the tab that asked for it.
fn ideas(request: &Request, root: &Path) -> Response {
    let binding = match binding_of(root) {
        Ok(binding) => binding,
        Err(why) => return Response::refused(400, why),
    };
    let idea = request.field("idea").unwrap_or_default();
    if idea.trim().is_empty() {
        return Response::refused(400, "type the idea first — there is nothing here to draft from");
    }

    let links = match binding
        .resolve("path.links")
        .and_then(|path| perp_core::link::Links::load(&path))
    {
        Ok(links) => links,
        Err(why) => return Response::refused(400, format!("{why}")),
    };

    // The list travels with the idea so the model does not repropose what the
    // project already decided — by opening sentence, never the paragraphs.
    let source = binding
        .resolve("path.requirements")
        .map(|path| perp_core::layout::requirements_text(&path))
        .unwrap_or_default();
    let catalogue = perp_core::cycle::catalogue(&source);

    let now = perp_core::time::now();
    let draft = match perp_core::idea::derive(&binding, &links, &idea, &catalogue, now) {
        Ok(draft) => draft,
        Err(why) => return Response::refused(400, format!("{why}")),
    };

    // Journalled, because it spent tokens. `N-7` counts what the engine costs
    // separately from the work, and a model call no record mentions is a call
    // `perp cost` cannot report — the page would be quietly free.
    if let Ok(journal_path) = binding.resolve("out.journal") {
        let journal = Journal::at(&journal_path);
        let records = journal.read_all().unwrap_or_default();
        let step = perp_core::capture::next_step(&records, "ideas");
        let price = links.price(&draft.served.link);
        let _ = journal.append(&draft.served.to_record(step, now, price));
    }

    Response::json(to_string(&Value::Obj(vec![
        (
            "candidates".into(),
            Value::Arr(draft.candidates.into_iter().map(Value::str).collect()),
        ),
        ("dropped".into(), Value::int(as_i64(draft.dropped))),
        ("via".into(), Value::str(draft.via)),
    ])))
}

/// Run the product and photograph the screen (`L-34`, `X-6`).
///
/// The browser supplies the **claim** and nothing else. What runs is
/// `product.run` from the binding — a line the operator wrote, the same
/// standing `S-9` gives a gate's command. A page that could name the command
/// would be a page that runs anything on the machine it is open on.
fn photograph(request: &Request, root: &Path) -> Response {
    let binding = match binding_of(root) {
        Ok(binding) => binding,
        Err(why) => return Response::refused(400, why),
    };
    let product = match Product::declared(&binding) {
        Ok(product) => product,
        Err(why) => return Response::refused(400, format!("{why}")),
    };
    let claim = request.field("claim").unwrap_or_default();
    let claim = claim.trim();
    if claim.is_empty() {
        return Response::refused(
            400,
            "say what this is evidence of — a screenshot with no claim attached is a picture, \
             not evidence (`A-6`)",
        );
    }

    let journal_path = match binding.resolve("out.journal") {
        Ok(path) => path,
        Err(why) => return Response::refused(400, format!("{why}")),
    };
    let journal = Journal::at(&journal_path);
    let records = journal.read_all().unwrap_or_default();
    let step = perp_core::capture::next_step(&records, "evidence");

    // The claim says what was photographed and how, because the picture is of
    // the whole screen while the product ran — not of its window. Describing it
    // as more than that is the failure `A-6` names.
    let claim = format!("{claim} — the screen while `{}` ran", product.command);
    let shot = match product.photograph(&journal_path, &step, &claim) {
        Ok(shot) => shot,
        Err(why) => return Response::refused(400, format!("{why}")),
    };
    if let Err(why) = journal.append(&shot.capture.record(step, perp_core::time::now())) {
        return Response::refused(400, format!("captured, but not recorded: {why}"));
    }
    Response::json(to_string(&Value::Obj(vec![
        ("claim".into(), Value::str(shot.capture.claim)),
        ("bytes".into(), Value::int(as_i64(shot.capture.bytes))),
        // Whether the window was *asked* forward, which is not the same as
        // whether it came — Windows refuses activation from a background
        // process. A person looking at a picture that does not contain the
        // product should be told which of those happened rather than left to
        // wonder, so the page says it in those words.
        ("focused".into(), Value::Bool(shot.focused)),
    ])))
}

fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}
