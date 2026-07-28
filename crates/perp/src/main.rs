//! `perp` — the harness CLI.
//!
//! Batch 1 ships the three commands that prove the spine works end to end:
//! `bind` refuses to run unbound, `record` appends to the journal, and `state`
//! renders the projection. Nothing here decides anything; the loop that will
//! is batch 3 onward.
//!
//! Argument parsing is by hand — a dependency is an approval step under this
//! project's binding, and four commands do not justify one (`N-11`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use perp_core::binding::Binding;
use perp_core::gate::{self, Gate};
use perp_core::git::Repo;
use perp_core::journal::{Journal, Record};
use perp_core::client::{ChatRequest, Client, Message};
use perp_core::link::{AssumeHealthy, Links, Mode, Role};
use perp_core::net::Curl;
use perp_core::session::{Decision, Finding, Probe, Session};
use perp_core::state::{render, replay};
use perp_core::verify;
use perp_core::step::StepId;
use perp_core::{atomic, time, Result, VERSION};

const USAGE: &str = "\
perp — the Perpetum harness

usage:
  perp bind [--root <dir>]
      Load the binding, check every path it names, and print what resolved.
      Exits non-zero if the project is unbound or a path is missing.

  perp record <step> <intent|outcome> <summary> [--ok|--failed]
             [--requirements <id,id>] [--detail <text>] [--root <dir>]
      Append one record to the journal named by the binding.

  perp state [--out <file>] [--root <dir>]
      Replay the journal and render the state file. Without --out, writes the
      path the binding names; with -, prints to stdout.

  perp gate [<name>|all] [--step <id>] [--root <dir>]
      Run the gates the binding declares, in Perpetum order, stopping at the
      first red. Prints the transcript. With --step, appends an outcome record
      per gate to the journal. Exits non-zero if any gate is red.

  perp resume [--root <dir>]
      Read the journal and say what a previous process left in flight, and what
      to do about it. Reports Unclear rather than guessing; exits non-zero if
      the answer is to park.

  perp check ids [--root <dir>]
      Check that every requirement id cited anywhere under docs/ is defined in
      the requirements source. Exits non-zero if one was invented elsewhere.

  perp links [--role <role>] [--local-only] [--root <dir>]
      List the configured links and role chains. With --role, show which link
      that role resolves to. With --local-only, do it as a run with no cloud —
      and report any role that loses its last option.

  perp ask <prompt> [--role <role>] [--local-only] [--system <text>] [--root <dir>]
      Resolve a role to a link and ask it. Prints the reply, the provenance of
      whichever link answered, and any link that was tried first and failed.

  perp version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    match run(&refs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("perp: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> std::result::Result<(), String> {
    match args.first().copied() {
        None | Some("help") | Some("--help") | Some("-h") => {
            print!("{USAGE}");
            Ok(())
        }
        Some("version") | Some("--version") => {
            println!("perp {VERSION}");
            Ok(())
        }
        Some("bind") => cmd_bind(&args[1..]).map_err(|e| e.to_string()),
        Some("record") => cmd_record(&args[1..]).map_err(|e| e.to_string()),
        Some("state") => cmd_state(&args[1..]).map_err(|e| e.to_string()),
        Some("gate") => cmd_gate(&args[1..]),
        Some("resume") => cmd_resume(&args[1..]),
        Some("check") => cmd_check(&args[1..]),
        Some("links") => cmd_links(&args[1..]),
        Some("ask") => cmd_ask(&args[1..]),
        Some(other) => Err(format!("unknown command `{other}` — try `perp help`")),
    }
}

/// A `--flag value` lookup. Returns `None` when the flag is absent.
fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| *arg == name)
        .and_then(|index| args.get(index + 1))
        .copied()
}

fn root_of(args: &[&str]) -> PathBuf {
    flag(args, "--root").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn positionals<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if let Some(_name) = arg.strip_prefix("--") {
            // Every flag in this CLI except --ok/--failed takes a value.
            skip_next = !matches!(*arg, "--ok" | "--failed");
            continue;
        }
        out.push(*arg);
    }
    out
}

fn load(args: &[&str]) -> Result<Binding> {
    let root = root_of(args);
    let binding = Binding::load(&root)?;
    binding.verify()?;
    Ok(binding)
}

fn cmd_bind(args: &[&str]) -> Result<()> {
    let binding = load(args)?;
    println!("bound: {}", binding.source().display());
    for (key, value) in binding.entries() {
        if key.starts_with("path.") || key.starts_with("out.") {
            let resolved = binding.root().join(value);
            let state = if resolved.exists() { "ok" } else { "to be written" };
            println!("  {key:<22} {value}  [{state}]");
        } else {
            println!("  {key:<22} {value}");
        }
    }
    Ok(())
}

fn cmd_record(args: &[&str]) -> Result<()> {
    let binding = load(args)?;
    let journal = Journal::at(binding.resolve("out.journal")?);
    let positional = positionals(args);

    let (Some(step), Some(kind), Some(summary)) =
        (positional.first(), positional.get(1), positional.get(2))
    else {
        return Err(perp_core::Error::unbound(
            "record",
            "expected <step> <intent|outcome> <summary>",
        ));
    };

    let step = StepId::parse(step)?;
    let at = time::now();
    let mut record = match *kind {
        "intent" => Record::intent(step, at, *summary),
        "outcome" => {
            let ok = !args.contains(&"--failed");
            Record::outcome(step, at, ok, *summary)
        }
        other => {
            return Err(perp_core::Error::unbound(
                "record",
                format!("`{other}` is neither `intent` nor `outcome`"),
            ))
        }
    };

    if let Some(ids) = flag(args, "--requirements") {
        record = record.for_requirements(ids.split(',').map(str::trim).filter(|s| !s.is_empty()));
    }
    if let Some(detail) = flag(args, "--detail") {
        record = record.with_detail(detail);
    }

    journal.append(&record)?;
    println!("recorded {} {}", record.step, journal.path().display());
    Ok(())
}

fn cmd_state(args: &[&str]) -> Result<()> {
    let binding = load(args)?;
    let journal = Journal::at(binding.resolve("out.journal")?);
    let projection = replay(&journal.read_all()?);
    let rendered = render(&projection, time::now());

    match flag(args, "--out") {
        Some("-") => print!("{rendered}"),
        Some(path) => write_state(Path::new(path), &rendered)?,
        None => write_state(&binding.resolve("out.state")?, &rendered)?,
    }
    Ok(())
}

/// Show the links, the role chains, and what a role resolves to (`M-2`–`M-5`).
///
/// Reachability is not checked here — that needs the transport batch 8 adds.
/// Health is assumed, and the output says so, because a listing that implies it
/// pinged something it did not is the kind of quiet lie this harness exists to
/// avoid.
fn cmd_links(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let path = binding.resolve("path.links").map_err(|e| e.to_string())?;
    let links = Links::load(&path).map_err(|e| e.to_string())?;
    let mode = if args.contains(&"--local-only") { Mode::LocalOnly } else { Mode::Any };

    println!("links: {}", path.display());
    println!("mode:  {}", if mode == Mode::LocalOnly { "local-only" } else { "any" });
    println!();

    for link in links.all() {
        let usable = if mode == Mode::Any || link.is_local() { "" } else { "  (skipped: cloud)" };
        println!("  {}{usable}", link.describe());
    }

    println!();
    if let Some(role) = flag(args, "--role") {
        let role = Role::parse(role).map_err(|e| e.to_string())?;
        let chain = links.chain(role).map_err(|e| e.to_string())?;
        println!(
            "role {role}: {}",
            chain.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(" → ")
        );
        let chosen = links
            .resolve(role, &AssumeHealthy, mode)
            .map_err(|e| e.to_string())?;
        println!("resolves to: {} (health assumed — nothing was contacted)", chosen.describe());
        return Ok(());
    }

    for (role, chain) in links.roles() {
        println!("  {role:<11} {}", chain.join(" → "));
    }

    let gaps = links.local_only_gaps();
    println!();
    if gaps.is_empty() {
        println!("every role has a local option — this project can run local-only");
    } else {
        let named: Vec<&str> = gaps.iter().map(|role| role.as_str()).collect();
        println!("no local option for: {}", named.join(", "));
        if mode == Mode::LocalOnly {
            return Err(format!(
                "{} role(s) cannot run local-only, and no cloud link is substituted",
                gaps.len()
            ));
        }
    }
    Ok(())
}

/// Ask a role's link something (`M-9`, `M-10`).
///
/// Prints where the answer came from, always — including the links that were
/// tried and failed first. A reply whose author is invisible is the thing
/// `M-10` exists to prevent.
fn cmd_ask(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let links = Links::load(&binding.resolve("path.links").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    let prompt = positionals(args)
        .first()
        .copied()
        .ok_or_else(|| "expected a prompt".to_string())?;
    let role = Role::parse(flag(args, "--role").unwrap_or("chat")).map_err(|e| e.to_string())?;
    let mode = if args.contains(&"--local-only") { Mode::LocalOnly } else { Mode::Any };

    let mut messages = Vec::new();
    if let Some(system) = flag(args, "--system") {
        messages.push(Message::system(system));
    }
    messages.push(Message::user(prompt));

    let transport = Curl::new();
    let mut client = Client::new(&transport);
    let served = client
        .call(&links, role, &ChatRequest::new(messages), &AssumeHealthy, mode, time::now())
        .map_err(|e| e.to_string())?;

    println!("{}", served.reply.content);
    println!();
    if let Some(reasoning) = &served.reply.reasoning {
        println!("[reasoning, {} chars — kept out of the message]", reasoning.len());
    }
    println!("via: {}", served.provenance());
    println!(
        "tokens: {} in ({} cached) / {} out",
        served.reply.usage.prompt_tokens,
        served.reply.usage.cache_hit_tokens,
        served.reply.usage.completion_tokens
    );
    Ok(())
}

/// `V-9`: ids are minted in the requirements source and cited everywhere else.
///
/// The failure this catches is quiet — a batch file naming `L-99`, a plan built
/// around a requirement that does not exist, and nobody noticing until the
/// cycle that tries to build it.
fn cmd_check(args: &[&str]) -> std::result::Result<(), String> {
    let which = positionals(args).first().copied().unwrap_or("ids");
    if which != "ids" {
        return Err(format!("unknown check `{which}` — only `ids` so far"));
    }

    let binding = load(args).map_err(|e| e.to_string())?;
    let source_path = binding.resolve("path.requirements").map_err(|e| e.to_string())?;
    let source = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("{}: {e}", source_path.display()))?;

    let mut documents = Vec::new();
    collect_markdown(&binding.root().join("docs"), &source_path, &mut documents)?;

    let borrowed: Vec<(&str, &str)> =
        documents.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect();
    let stray = verify::stray_ids(&source, &borrowed);

    println!("source:    {}", source_path.display());
    println!(
        "defined:   {}",
        verify::defined_ids(&source).map_err(|e| e.to_string())?.len()
    );
    println!("documents: {}", documents.len());

    if stray.is_empty() {
        println!("no stray ids — everything cited is defined");
        return Ok(());
    }
    for item in &stray {
        println!("  stray: {} in {}", item.id, item.file);
    }
    Err(format!("{} id(s) used but never defined", stray.len()))
}

fn collect_markdown(
    dir: &Path,
    skip: &Path,
    out: &mut Vec<(String, String)>,
) -> std::result::Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, skip, out)?;
        } else if path.extension().is_some_and(|e| e == "md") && path != skip {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((path.display().to_string(), text));
            }
        }
    }
    Ok(())
}

/// Report what a previous process left behind (`L-7`).
///
/// The CLI's probe is deliberately the ignorant one: from outside the loop
/// there is no way to tell whether a step's effect landed, so it answers
/// Unclear and lets the rules decide — redo if the step declared itself
/// idempotent, park otherwise. Guessing here is how a loop sends the same
/// email twice.
fn cmd_resume(args: &[&str]) -> std::result::Result<(), String> {
    struct CannotTell;
    impl Probe for CannotTell {
        fn finding(&self, _step: &StepId, _summary: &str) -> Finding {
            Finding::Unclear
        }
    }

    let root = root_of(args);
    let session = Session::open(&root).map_err(|e| e.to_string())?;
    let decision = session.resume(&CannotTell).map_err(|e| e.to_string())?;
    let projection = session.projection().map_err(|e| e.to_string())?;

    println!("journal: {}", session.journal_path().display());
    println!("steps:   {} done, {} blocked", projection.done.len(), projection.blocked.len());
    println!("resume:  {}", decision.describe());

    match decision {
        Decision::Park { step, why } => Err(format!("{step} needs a human: {why}")),
        _ => Ok(()),
    }
}

/// Run gates and print what actually happened. The exit code is the gates', not
/// an opinion about them.
fn cmd_gate(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let wanted = positionals(args).first().copied().unwrap_or("all");

    let gates = match wanted {
        "all" => Gate::from_binding(&binding).map_err(|e| e.to_string())?,
        name => vec![Gate::named(&binding, name).map_err(|e| e.to_string())?],
    };

    let journal = match flag(args, "--step") {
        Some(_) => Some(Journal::at(
            binding.resolve("out.journal").map_err(|e| e.to_string())?,
        )),
        None => None,
    };
    let step = match flag(args, "--step") {
        Some(text) => Some(StepId::parse(text).map_err(|e| e.to_string())?),
        None => None,
    };

    // `G-6`: pin every transcript to the commit it ran against. A repository
    // that cannot answer is not an error — the gate still ran — but the
    // transcript then says so rather than implying a sha it does not have.
    let sha = Repo::at(binding.root()).head_sha().ok();

    let results: Vec<_> = gate::run_all(&gates)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|result| match &sha {
            Some(sha) => result.at_sha(sha),
            None => result,
        })
        .collect();
    let mut red = 0;

    for result in &results {
        print!("{}", result.evidence());
        println!();
        if !result.is_green() {
            red += 1;
        }
        if let (Some(journal), Some(step)) = (&journal, &step) {
            journal
                .append(&result.to_record(step.clone(), time::now()))
                .map_err(|e| e.to_string())?;
        }
    }

    if results.len() < gates.len() {
        println!(
            "stopped after {} of {} gates — a red gate makes the rest meaningless",
            results.len(),
            gates.len()
        );
    }

    if red > 0 {
        return Err(format!("{red} gate(s) red"));
    }
    println!("all {} gates green", results.len());
    Ok(())
}

fn write_state(path: &Path, rendered: &str) -> Result<()> {
    atomic::write_atomic(path, rendered)?;
    println!("wrote {}", path.display());
    Ok(())
}
