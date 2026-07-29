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
use perp_core::btw;
use perp_core::chat;
use perp_core::command::{self, Chain, Command, Input, Subject};
use perp_core::engine::{Engine, Gates};
use perp_core::phase::{Machine, Measured, Phase};
use perp_core::gate::{self, Gate};
use perp_core::git::Repo;
use perp_core::journal::{Journal, Record};
use perp_core::client::{ChatRequest, Client, Message};
use perp_core::cost::Ledger;
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

  perp ask <prompt> [--role <role>] [--local-only] [--system <text>]
           [--step <id>] [--root <dir>]
      Resolve a role to a link and ask it. Prints the reply, the provenance of
      whichever link answered, and any link that was tried first and failed.

  perp run [--cycle <n>] [--stage <b12|D>] [--root <dir>] [--dry-run]
      Drive the loop: take the write lock, run the project's gates one step at
      a time, journal an intent before each and an outcome after, and stop for
      one of exactly three reasons — the backlog is exhausted, the batch is
      blocked, or a budget parked it. Exits non-zero if it stopped blocked.

  perp phase [--phase <A-G>] [--cycle <n>] [--root <dir>]
      Measure the workspace and say whether the current phase's exit condition
      is met, and why not if it is not. Measured, never asserted.

  perp chat [--local-only] [--root <dir>]
      Conversation. Slash commands are resolved by the engine and never sent to
      a model; an unknown one is an error, not a prompt. Both sides of the
      conversation go in the loop's own journal. Read-only while the loop holds
      the workspace.

  perp btw <text> [--source <where>] [--root <dir>]
      File an aside. Accepted at any time, classified as steer, requirement,
      constraint or note, and never able to cross the approval boundary.
      `perp btw \"<id> <class>\"` corrects a classification.

  perp explain <id|step|sha> [--root <dir>]
      Render the evidence chain: the steps that cited it, the gate transcripts,
      the commit it was pinned to, and which link wrote it.

  perp cost [--root <dir>]
      Replay the journal and report what the loop spent, by link and by role.
      Local links show tokens and time and no money.

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
        Some("cost") => cmd_cost(&args[1..]),
        Some("run") => cmd_run(&args[1..]),
        Some("phase") => cmd_phase(&args[1..]),
        Some("chat") => cmd_chat(&args[1..]),
        Some("btw") => cmd_btw(&args[1..]),
        Some("explain") => cmd_explain(&args[1..]),
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

/// What the loop spent (`M-11`).
///
/// Replayed from the journal, not accumulated: a process that died mid-batch
/// still recorded every call it made.
fn cmd_cost(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let ledger = Ledger::replay(&journal.read_all().map_err(|e| e.to_string())?);

    if ledger.is_empty() {
        println!("no model calls in the journal — nothing has been spent");
        return Ok(());
    }

    let total = ledger.total();
    println!(
        "{} calls · {} tokens in ({} cached) · {} out · {:.1}s · {:.6}",
        total.calls,
        total.usage.input_tokens(),
        total.usage.cache_hit_tokens,
        total.usage.output_tokens,
        total.latency_ms as f64 / 1000.0,
        // Six places, not four: a single call costs tens of millionths, and a
        // report that rounds every real amount to 0.0000 is decoration.
        total.charge
    );

    println!("
by link");
    for (name, sub) in ledger.by_link() {
        println!(
            "  {name:<12} {:>5} calls  {:>9} tokens  {:>11.6}{}",
            sub.calls,
            sub.usage.total(),
            sub.charge,
            if sub.charge == 0.0 { "  (local — time, not money)" } else { "" }
        );
    }

    println!("
by role");
    for (name, sub) in ledger.by_role() {
        println!("  {name:<12} {:>5} calls  {:>9} tokens  {:>11.6}", sub.calls, sub.usage.total(), sub.charge);
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
    if let Some(step) = flag(args, "--step") {
        let step = StepId::parse(step).map_err(|e| e.to_string())?;
        let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
        journal
            .append(&served.to_record(step, time::now(), links.price(&served.link)))
            .map_err(|e| e.to_string())?;
    }
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

/// Drive the loop. The point of the whole harness: the operator stops typing
/// `perp gate` and the engine does it, under a lock, with a budget, writing a
/// terminal record that says which of `L-14`'s three reasons ended it.
fn cmd_run(args: &[&str]) -> std::result::Result<(), String> {
    let root = root_of(args);
    let binding = load(args).map_err(|e| e.to_string())?;
    let cycle: u32 = flag(args, "--cycle")
        .map(|text| text.parse().map_err(|_| format!("--cycle takes a number, not `{text}`")))
        .transpose()?
        .unwrap_or(1);
    let stage = flag(args, "--stage").unwrap_or("D");

    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| binding.root().join("crates/target"));
    let mut work = Gates::from_binding(&binding, &target).map_err(|e| e.to_string())?;

    if args.contains(&"--dry-run") {
        // What it would do, without taking the lock, running a gate or writing
        // a record.
        println!("would run in {}, cycle {cycle}, stage {stage}:", root.display());
        for gate in Gate::from_binding(&binding).map_err(|e| e.to_string())? {
            println!("  step: gate {} — {}", gate.name, gate.command);
        }
        println!("  and a terminal record saying why it stopped");
        return Ok(());
    }

    let mut engine = Engine::open(&root).map_err(|e| e.to_string())?;
    let budgets = engine.budgets();
    if budgets.cycle.is_unlimited() && budgets.batch.is_unlimited() {
        // Not an error: an unattended run with no ceiling is a choice, and one
        // the operator should have made on purpose rather than by omission.
        eprintln!("note: no budget is declared — this run has no ceiling in any currency");
    }

    let report = engine.run(cycle, stage, &mut work, 0).map_err(|e| e.to_string())?;
    if let Some(previous) = &report.took_over {
        println!("took over an abandoned lock from {previous}");
    }
    println!("{}", report.describe());
    println!("spent {}", report.spend);

    match &report.stop {
        Some(perp_core::phase::Stop::BatchBlocked { why, .. }) => Err(format!("blocked: {why}")),
        _ if report.failed > 0 => Err(format!("{} gates red", report.failed)),
        _ => Ok(()),
    }
}

/// Say whether the current phase is over, from the workspace rather than from
/// anyone's opinion of it (`L-2`).
fn cmd_phase(args: &[&str]) -> std::result::Result<(), String> {
    let root = root_of(args);
    let binding = load(args).map_err(|e| e.to_string())?;
    let cycle: u32 = flag(args, "--cycle")
        .map(|text| text.parse().map_err(|_| format!("--cycle takes a number, not `{text}`")))
        .transpose()?
        .unwrap_or(1);

    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let projection = replay(&records);

    let vision = binding.resolve("path.vision").map(|p| p.exists()).unwrap_or(false);

    // `L-4`: the state file is a projection, so "matches" means re-rendering it
    // produces the same thing. The `Updated:` line is dropped from both sides —
    // it is the render time, not a fact about the journal, and comparing it
    // would report every state file as stale the moment the clock moved.
    let state_matches = match binding.resolve("out.state") {
        Ok(path) => match std::fs::read_to_string(&path) {
            Ok(text) => without_timestamp(&text) == without_timestamp(&render(&projection, 0)),
            Err(_) => false,
        },
        Err(_) => true,
    };

    let measured = Measured::from_workspace(
        true,
        vision,
        0,
        1,
        0,
        u32::try_from(projection.done.len()).unwrap_or(u32::MAX),
        false,
        false,
        u32::from(projection.open_step.is_some()),
        state_matches,
    );

    let phase = flag(args, "--phase")
        .and_then(|text| text.chars().next())
        .and_then(Phase::parse)
        .unwrap_or(Phase::F);
    let machine = Machine::at(cycle, phase);
    let exit = machine.phase().exit(5);

    println!("{} in {}", machine.phase(), root.display());
    println!("  exit condition: {}", exit.describe());
    match exit.check(&measured) {
        perp_core::phase::Outcome::Met => {
            println!("  met");
            Ok(())
        }
        perp_core::phase::Outcome::NotMet { because } => {
            println!("  not met: {because}");
            Err(because)
        }
    }
}

/// The projection without its render time, for comparing a state file against
/// the journal it claims to be a view of.
fn without_timestamp(text: &str) -> String {
    text.lines().filter(|line| !line.contains("Updated:")).collect::<Vec<_>>().join("\n")
}

/// The evidence chain for one decision (`C-7`).
fn cmd_explain(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let subject = positionals(args)
        .first()
        .copied()
        .ok_or_else(|| "expected a requirement id, a step id, or a commit sha".to_string())?;
    let subject = Subject::parse(subject).map_err(|e| e.to_string())?;

    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let chain = Chain::build(&subject, &records);
    print!("{}", chain.render());

    // An empty chain is an answer, not a failure — but it is a non-zero one, so
    // a script asking "is there evidence for this" gets a usable exit code.
    if chain.is_empty() {
        return Err("no evidence".into());
    }
    Ok(())
}

/// File an aside from the CLI (`C-8`, `C-11`).
fn cmd_btw(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;

    let text = positionals(args)
        .first()
        .copied()
        .ok_or_else(|| "expected something to note".to_string())?;
    let source = flag(args, "--source").unwrap_or("cli");
    let mut queue = btw::Queue::replay(&records);
    let step = next_step_for(&records, "btw");

    // `perp btw "<id> <class>"` — the correction path (`C-9`).
    if let Some((head, rest)) = text.split_once(char::is_whitespace) {
        if let (Ok(id), Some(class)) = (head.parse::<u64>(), btw::Class::parse(rest)) {
            let item = queue.reclassify(id, class).map_err(|e| e.to_string())?;
            println!("#{} is now a {}: {}", item.id, item.class, item.class.effect());
            return journal.append(&item.record(step)).map_err(|e| e.to_string());
        }
    }

    let item = queue.accept(text, source, time::now(), None);
    println!("{}", item.acknowledgement());
    journal.append(&item.record(step)).map_err(|e| e.to_string())
}

/// Conversation (`C-1`). Reads lines: slash commands are resolved here and
/// never sent to a model, everything else is a message.
fn cmd_chat(args: &[&str]) -> std::result::Result<(), String> {
    use std::io::BufRead;

    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let links = Links::load(&binding.resolve("path.links").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mode = if args.contains(&"--local-only") { Mode::LocalOnly } else { Mode::Any };

    // `C-3`: if the loop holds the write lock, this session is read-only. The
    // lock file on disk is the authority — not a flag, not an assumption.
    let held = root_of(args).join("write.lock").exists();
    let chat_mode = perp_core::lock::chat_mode(held, false);
    if chat_mode == perp_core::lock::ChatMode::ReadOnly {
        println!("the loop is writing — this session is read-only (`C-3`)");
    }
    println!("perp chat · {} slash commands, or just type. ctrl-d to leave.", command::NAMES.len());

    let transport = Curl::new();
    let mut client = Client::new(&transport);
    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let records = journal.read_all().map_err(|e| e.to_string())?;
        let step = next_step_for(&records, "chat");

        match command::parse(&line) {
            // The load-bearing branch: an unknown slash command stops here and
            // is never handed to a model as a prompt (`C-6`).
            Err(e) => println!("{e}"),
            Ok(Input::Command(command)) => {
                if command.writes() && chat_mode == perp_core::lock::ChatMode::ReadOnly {
                    println!("{command} writes, and the loop holds the workspace (`C-3`)");
                    continue;
                }
                match run_command(&command, &binding, &journal, &records, step) {
                    Ok(text) => println!("{text}"),
                    Err(e) => println!("{e}"),
                }
            }
            Ok(Input::Message(text)) => {
                journal
                    .append(&chat::Turn::operator(&text, time::now()).record(step.clone()))
                    .map_err(|e| e.to_string())?;

                let request = ChatRequest::new(vec![Message::user(text)]);
                match client.call(&links, Role::Chat, &request, &AssumeHealthy, mode, time::now()) {
                    Ok(served) => {
                        println!("{}", served.reply.content);
                        println!("[via {}]", served.provenance());
                        let next = step.next();
                        let turn =
                            chat::Turn::assistant(&served.reply.content, time::now(), &served.link);
                        journal.append(&turn.record(next.clone())).map_err(|e| e.to_string())?;
                        journal
                            .append(&served.to_record(next, time::now(), links.price(&served.link)))
                            .map_err(|e| e.to_string())?;
                    }
                    Err(e) => println!("no link answered: {e}"),
                }
            }
        }
    }
    Ok(())
}

/// The commands that can be answered without a running engine. Read commands
/// work while the loop runs; the rest say what they would need.
fn run_command(
    command: &Command,
    binding: &Binding,
    journal: &Journal,
    records: &[Record],
    step: StepId,
) -> std::result::Result<String, String> {
    match command {
        Command::Explain { subject } => Ok(Chain::build(subject, records).render()),
        Command::Status => Ok(render(&replay(records), time::now())),
        Command::Cost => {
            let total = Ledger::replay(records).total();
            Ok(format!(
                "{} calls · {} tokens · {:.6}",
                total.calls,
                total.usage.total(),
                total.charge
            ))
        }
        Command::Board => {
            std::fs::read_to_string(binding.resolve("out.board").map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())
        }
        Command::Links => Ok(format!(
            "links: {}",
            binding.resolve("path.links").map_err(|e| e.to_string())?.display()
        )),
        Command::Btw { text } => {
            let mut queue = btw::Queue::replay(records);
            let item = queue.accept(text, "chat", time::now(), None);
            let ack = item.acknowledgement();
            journal.append(&item.record(step)).map_err(|e| e.to_string())?;
            Ok(ack)
        }
        // Everything else moves the loop, and the loop driver owns it. Said
        // plainly rather than half-implemented: a `/pause` that printed
        // "paused" without pausing anything is worse than one that is absent.
        other => Err(format!(
            "{other} moves the loop, and this build answers it from `perp run` only \
             — it is not yet wired to a running engine"
        )),
    }
}

/// The next step id for a conversational record. Chat shares the loop's stream
/// (`C-5`), so it continues the sequence rather than starting its own.
fn next_step_for(records: &[Record], stage: &str) -> StepId {
    let cycle = records.last().map(|r| r.step.cycle).unwrap_or(1);
    let seq = records.iter().map(|r| r.step.seq).max().unwrap_or(0) + 1;
    StepId::new(cycle, stage, seq).unwrap_or(StepId { cycle, stage: stage.into(), seq })
}
