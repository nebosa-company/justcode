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
use perp_core::artifact;
use perp_core::btw;
use perp_core::chat;
use perp_core::control::{Channel, Control, Rewind};
use perp_core::command::{self, Chain, Command, Input, Subject};
use perp_core::engine::{Engine, Gates};
use perp_core::approval::Queue as Approvals;
use perp_core::metrics::Snapshot;
use perp_core::panel::View;
use perp_core::repo::Survey;
use perp_core::phase::{Machine, Measured, Phase};
use perp_core::gate::{self, Gate};
use perp_core::git::Repo;
use perp_core::journal::{Journal, Record};
use perp_core::client::{ChatRequest, Client, Message};
use perp_core::cost::Ledger;
use perp_core::link::{AssumeHealthy, Links, Mode, Role};
use perp_core::net::{Curl, Transport};
use perp_core::session::{Decision, Finding, Probe, Session};
use perp_core::state::{render, replay};
use perp_core::verify;
use perp_core::step::StepId;
use perp_core::{atomic, time, Result, VERSION};

const USAGE: &str = "\
perp — the Perpetum harness

Any command takes --verbose (-v): a running commentary on stderr of the calls
made, the prompts sent, the replies received, the tools run and the gates. It is
redacted with the same patterns as everything else that leaves the process, and
it goes to stderr so it never mixes into the JSON that `panel` writes.

usage:
  perp init [--root <dir>]
      Create the files a workspace needs under `.harness/`: a binding that
      resolves, a requirements directory, a vision and a links file with every
      provider commented out. Never overwrites — a second run fills in what is
      missing and says what it left alone.

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
           [--requirement <id> [--brief <text>] [--local-only]]
      Drive the loop: take the write lock, run the project's gates one step at
      a time, journal an intent before each and an outcome after, and stop for
      one of exactly three reasons — the backlog is exhausted, the batch is
      blocked, or a budget parked it. Exits non-zero if it stopped blocked.

      With --requirement, the loop asks a model instead of running gates: its
      answer is parsed for tool calls through the degradation ladder, the calls
      go through the permission classifier, and the results come back as data.
      The id is mandatory — work that is not on the record is not built.

  perp phase [--phase <A-G>] [--cycle <n>] [--root <dir>]
      Measure the workspace and say whether the current phase's exit condition
      is met, and why not if it is not. Measured, never asserted.

  perp chat [--once <text>] [--local-only] [--root <dir>]
      Conversation. Slash commands are resolved by the engine and never sent to
      a model; an unknown one is an error, not a prompt. Both sides of the
      conversation go in the loop's own journal. Read-only while the loop holds
      the workspace.

      With --once, answers one message and exits, for a caller that is not a
      terminal. The same turn either way: journalled, streamed, priced.

  perp btw <text> [--source <where>] [--root <dir>]
      File an aside. Accepted at any time, classified as steer, requirement,
      constraint or note, and never able to cross the approval boundary.
      `perp btw \"<id> <class>\"` corrects a classification.

  perp explain <id|step|sha> [--root <dir>]
      Render the evidence chain: the steps that cited it, the gate transcripts,
      the commit it was pinned to, and which link wrote it.

  perp artifact [<kind>|all] [--root <dir>]
      Render artifacts from the journal into .harness/artifacts/. One
      stable file per kind, self-contained, no server and no build step. A
      render that fails is a warning: artifacts are never on the critical path.

  perp watch [--every <seconds>] [--root <dir>]
      Phase, batch, link, tokens, money, gate state, blocked and gated counts
      and queued /btw. Derived from the journal every time rather than tallied,
      so two watchers agree.

  perp control [status|pause|resume|step|inject <text>|redirect <id>|abort]
      Live control. Written to a file the engine reads at the next step
      boundary, never mid-step. Works whether or not a loop is running.

  perp decisions [--since <step>] [--decider <who>] [--root <dir>]
      Every fork the loop took: what was chosen, what else was available, why,
      and who decided - `rule`, `model` or `person`. Replayed from the journal,
      never stored, so the same journal gives the same log anywhere. Filter by
      `--since` a step id, or by `--decider`.

  perp changelog --since <ref> [--until <ref>] [--root <dir>]
      Commits between two refs, grouped by the `Requirement:` trailer each one
      carries and nothing else - no model narrates it. A commit with no
      trailer is listed apart rather than folded into a neighbour's section.
      `--until` defaults to `HEAD`.

  perp approvals [--root <dir>]
      What is waiting for a person: what was asked for, why, the command it
      would run, and the requirement it serves. Rebuilt from the journal, so a
      request survives the run that raised it being killed.

  perp approve <id> --by <your name> [--root <dir>]
  perp reject  <id> --by <your name> [--root <dir>]
      Answer one. The name is required and goes in the journal - an approval
      with nobody's name against it is one nobody gave. Applies to the cycle it
      was raised in and to no later one (`T-15`).

  perp unlock [--root <dir>]
      Break the write and gate locks a killed run left behind, naming who held
      them. A lock stops being honoured on its own once its holder stops
      beating; this is for not waiting. If the name printed is a process still
      running, you have just arranged for two writers.

  perp rewind <step> [--approve <your name>]
      Show what returning to a step would undo, and — with an approval — do it
      by reverting rather than resetting, so nothing is destroyed on either
      side. The journal keeps every superseded step; it is append-only.

  perp panel [--root <dir>]
      One JSON document of everything a panel shows: position, spend, timeline,
      chat, pending /btw and artifacts. Rendered from the journal every time —
      the panel is a view, not a second source of truth.

  perp capture <what it shows> [--root <dir>]
      Take a screenshot as evidence, stored beside the journal and recorded
      with its hash. The claim is required: a screenshot with no claim attached
      is a picture, not evidence.

  perp schedule [--every <minutes>] [--approve <your name>] [--root <dir>]
      Show the exact command that would register the harness with this
      platform's scheduler, and — with an approval — run it. The scheduled run
      is `perp resume`, never `perp run`: a restart reconciles what the last
      process left in flight before doing anything else.

  perp cycle [--cycle <n>] [--phase <A-G>] [--batches <n>] [--items <n>]
             [--local-only] [--root <dir>]
      Run a whole cycle unattended. Reads the backlog off the requirements
      source, works it against a model, gates every batch, journals everything,
      and stops for one of exactly three reasons or a budget. Refuses to start
      with no budget declared, or with a link whose credential is unset.

      It does not mark its own work done. That marker means gates green with a
      transcript, and a loop that awards it to itself is a loop whose status is
      worth nothing.

  perp cost [--root <dir>]
      Replay the journal and report what the loop spent, by link and by role.
      Local links show tokens and time and no money.

  perp version
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();

    // Read before dispatch and honoured by every command, rather than being a
    // flag each one has to remember to accept. It takes no value, so
    // `positionals` already skips it — it is listed there among the flags that
    // stand alone.
    if refs.iter().any(|arg| *arg == "--verbose" || *arg == "-v") {
        perp_core::verbose::enable();
        perp_core::verbose::say("perp", &format!("{VERSION} · verbose"));
    }

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
        Some("init") => cmd_init(&args[1..]).map_err(|e| e.to_string()),
        Some("bind") => cmd_bind(&args[1..]).map_err(|e| e.to_string()),
        Some("record") => cmd_record(&args[1..]).map_err(|e| e.to_string()),
        Some("state") => cmd_state(&args[1..]).map_err(|e| e.to_string()),
        Some("gate") => cmd_gate(&args[1..]),
        Some("resume") => cmd_resume(&args[1..]),
        Some("check") => cmd_check(&args[1..]),
        Some("redrun") => cmd_redrun(&args[1..]),
        Some("links") => cmd_links(&args[1..]),
        Some("ask") => cmd_ask(&args[1..]),
        Some("cost") => cmd_cost(&args[1..]),
        Some("run") => cmd_run(&args[1..]),
        Some("phase") => cmd_phase(&args[1..]),
        Some("chat") => cmd_chat(&args[1..]),
        Some("btw") => cmd_btw(&args[1..]),
        Some("explain") => cmd_explain(&args[1..]),
        Some("artifact") => cmd_artifact(&args[1..]),
        Some("watch") => cmd_watch(&args[1..]),
        Some("control") => cmd_control(&args[1..]),
        Some("unlock") => cmd_unlock(&args[1..]),
        Some("approvals") => cmd_approvals(&args[1..]),
        Some("decisions") => cmd_decisions(&args[1..]),
        Some("changelog") => cmd_changelog(&args[1..]),
        Some("approve") => cmd_answer(&args[1..], true),
        Some("reject") => cmd_answer(&args[1..], false),
        Some("rewind") => cmd_rewind(&args[1..]),
        Some("panel") => cmd_panel(&args[1..]),
        Some("cycle") => cmd_cycle(&args[1..]),
        Some("capture") => cmd_capture(&args[1..]),
        Some("schedule") => cmd_schedule(&args[1..]),
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

/// Where a command was pointed, taken literally: `--root`, or here.
///
/// What `init` wants, because there is no workspace to find yet and creating one
/// in a parent that was never named would be a surprise. Everything else wants
/// [`workspace_of`].
fn root_of(args: &[&str]) -> PathBuf {
    flag(args, "--root").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// The workspace a command should act on: `--root` if given, else the nearest
/// one at or above the current directory.
///
/// An explicit `--root` is never second-guessed. Without one, this is the same
/// search the editor does, so `perp state` from `src/` answers about the project
/// the way the panel beside it does.
///
/// The current directory is returned as `.` when it is itself the workspace, so
/// the ordinary case prints exactly what it printed before. Falling back to `.`
/// when nothing is found keeps the error the same too: the binding is reported
/// missing here, rather than a search failure being reported instead.
fn workspace_of(args: &[&str]) -> PathBuf {
    if let Some(given) = flag(args, "--root") {
        return PathBuf::from(given);
    }
    let here = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(_) => return PathBuf::from("."),
    };
    match perp_core::layout::enclosing(&here) {
        Some(found) if found == here => PathBuf::from("."),
        Some(found) => found,
        None => PathBuf::from("."),
    }
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
            skip_next = !matches!(*arg, "--ok" | "--failed" | "--verbose" | "--dry-run");
            continue;
        }
        out.push(*arg);
    }
    out
}

fn load(args: &[&str]) -> Result<Binding> {
    let root = workspace_of(args);
    let binding = Binding::load(&root)?;
    binding.verify()?;
    Ok(binding)
}

/// The operator's own redaction patterns, from `redact.<name> = <literal>`
/// (`S-3`).
///
/// `S-3` says outbound content is redacted against a **configurable** set, and
/// the configurable half did not exist: `Client::redact` started empty, its
/// setter had no caller, and the binding reader had none either — so a
/// `redact.*` line was documented, plausible, and inert, and only the standing
/// vendor prefixes ever fired. An operator's internal hostname or customer name
/// went to a cloud link in the clear.
///
/// Read here rather than inside `Client` because the client is handed a
/// transport and links, not a binding, and giving it one would make every
/// caller that has no binding invent something to satisfy it.
/// Requirements counted by what they are, for the state file (`V-8`).
///
/// Every renderer of that file must use this, including the one that
/// *re-renders it to check it is current* — `L-4` calls the file a projection
/// and "matches" means re-rendering produces the same bytes, so a comparison
/// that skipped the counts would report every state file as stale the moment
/// one was written with them.
fn requirement_counts(binding: &Binding) -> Option<perp_core::verify::Counts> {
    let path = binding.resolve("path.requirements").ok()?;
    let source = perp_core::layout::requirements_text(&path);
    Some(perp_core::verify::Counts::of(&perp_core::cycle::markers(&source)))
}

/// Where a model call may go (`S-4`): the links the operator declared, and
/// nothing else.
///
/// `Client::egress` was `None` and `check_egress` returns `Ok` on `None`, so
/// every check on the model-call path was a no-op — the allowlist was
/// fail-open, and `Egress::allowing_links`, which exists to build exactly this,
/// had no caller either. Both ends written, nothing between them.
///
/// Deliberately *not* the binding's `egress.allow`: that list is `fetch`'s, and
/// a host allowed for fetching is not thereby a place to send a prompt. The
/// client only ever calls a link's own `base_url`, so this is the honest set.
/// A link addressed by device rather than URL (`lmlink`) contributes no host,
/// which is correct — there is nothing to allow.
fn egress_for(links: &Links) -> perp_core::security::Egress {
    perp_core::security::Egress::new(Vec::new()).allowing_links(links.all())
}

fn redaction(binding: &Binding) -> Vec<perp_core::security::Pattern> {
    let entries: Vec<(String, String)> =
        binding.entries().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    perp_core::security::patterns_from_entries(&entries)
}

fn cmd_init(args: &[&str]) -> Result<()> {
    let root = root_of(args);
    for entry in perp_core::init::run(&root)? {
        println!("  {}", entry.describe());
    }
    println!();
    println!("Next: set `gate.test` in {}, then add requirements.", perp_core::layout::BINDING);
    println!("It refuses to run until the gate is a real command, which is deliberate.");
    Ok(())
}

fn cmd_bind(args: &[&str]) -> Result<()> {
    let binding = load(args)?;
    println!("bound: {}", binding.source().display());

    // `bind` is the command for setting a workspace up, so this is where the
    // harness's own ignore file appears — and it says so rather than writing into
    // someone's repository quietly. A run does it too, so a workspace that was
    // never bound by hand still gets one.
    if perp_core::layout::ensure_gitignore(binding.root()) {
        println!("wrote: {}", perp_core::layout::GITIGNORE);
    }
    for (key, value) in binding.entries() {
        if key.starts_with("path.") || key.starts_with("out.") {
            let resolved = binding.root().join(value);
            let state = if resolved.exists() { "ok" } else { "to be written" };
            println!("  {key:<22} {value}  [{state}]");
        } else {
            println!("  {key:<22} {value}");
        }
    }

    // `G-12`: submodules, LFS and in-repo hooks are detected here and declared
    // loudly. A loop that silently skips a submodule ships half a change and
    // passes its own gates doing it.
    let survey = Survey::of(binding.root());
    println!();
    println!("repository");
    print!("{}", survey.render());

    // `M-24`: a link whose `auth_env` names an unset variable fails here, not
    // on the eleventh call of a batch — by which point the loop has spent an
    // hour to discover a typo, and the 401 blames the request rather than the
    // configuration.
    if let Ok(path) = binding.resolve("path.links") {
        if let Ok(links) = Links::load(&path) {
            let missing = links.missing_credentials();
            if missing.is_empty() {
                println!("credentials      every declared link's variable is set");
            } else {
                println!();
                for (link, variable) in &missing {
                    // The variable name, never a value (`S-2`).
                    println!("link `{link}` needs ${variable}, which is not set");
                }
                return Err(perp_core::Error::unbound(
                    "credentials",
                    format!("{} link(s) declare a variable that is not set (`M-24`)", missing.len()),
                ));
            }
        }
    }

    if !survey.is_safe_to_run() {
        return Err(perp_core::Error::refused(
            "repository",
            "has a feature the loop does not support (`G-12`). Run attended, or remove it              from the workspace",
        ));
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
    let counts = requirement_counts(&binding);
    let rendered = render(&projection, time::now(), counts.as_ref());

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
/// Get the local links ready before a run needs them (`M-16`, `M-17`, `M-19`).
///
/// LM Studio JIT-loads a model on first use. Without this an unattended batch
/// paid a cold load — minutes, for anything large — inside its first step and
/// against that step's first-token deadline, which is exactly the failure
/// `M-16` describes and which nothing prevented, because none of it was called.
///
/// Best-effort on purpose. A machine with no `lms` on its PATH is the common
/// case for a cloud-only run, and refusing to start would make a local-server
/// convenience into a hard dependency.
fn warm_local_links(
    binding: &perp_core::binding::Binding,
    verbose: bool,
) -> std::result::Result<(), String> {
    let path = binding.resolve("path.links").map_err(|e| e.to_string())?;
    let links = Links::load(&path).map_err(|e| e.to_string())?;
    let local: Vec<&perp_core::link::Link> =
        links.all().iter().filter(|link| link.is_local()).collect();
    if local.is_empty() {
        return Ok(());
    }

    let lms = perp_core::local::Lms::new();
    let transport = Curl::new();

    // What each link says about itself (`M-7`): `state`, `quantization`,
    // `max_context_length`. Read from the native surface rather than guessed,
    // and read *before* loading anything — the whole question is whether a load
    // is needed, and finding out by loading answers it too late.
    let facts = |link: &perp_core::link::Link| -> Option<perp_core::probe::ModelFacts> {
        let base = link.base_url.as_deref()?;
        let base = base.trim_end_matches('/');
        let url = format!("{base}/api/v0/models");

        let response = match transport.send(&perp_core::net::Request::get(url.clone())) {
            Ok(r) => r,
            Err(e) => {
                if verbose {
                    eprintln!("  M-7 error: {}: failed to reach {}: {}", link.name, url, e);
                }
                return None;
            }
        };

        let models = match perp_core::probe::parse_models(&response.body) {
            Ok(m) => m,
            Err(e) => {
                if verbose {
                    eprintln!("  M-7 error: {}: failed to parse /api/v0/models response: {}", link.name, e);
                }
                return None;
            }
        };

        let found = models.iter().find(|facts| facts.id == link.model).cloned();
        if found.is_none() && verbose {
            let available: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
            eprintln!("  M-7 error: {}: model '{}' not in /api/v0/models. Available: {}",
                    link.name, link.model, available.join(", "));
        }
        found
    };

    // `M-31`: how much each host may hold is a fact about a machine, so it
    // comes from the binding. With nothing declared every host keeps the
    // default of one, which is what it had before there was a way to say
    // otherwise.
    let entries: Vec<(String, String)> =
        binding.entries().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let vram = perp_core::local::Vram::from_entries(&entries);

    let prepared = perp_core::local::prepare(
        &lms,
        &local,
        &facts,
        std::time::Duration::from_secs(perp_core::local::DEFAULT_TTL_SECS),
        vram,
    );
    if verbose && !prepared.describe().is_empty() {
        eprint!("· warm\n{}", prepared.describe());
    }
    Ok(())
}

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
    // `M-29`: a total is only allowed to say "no prefix cache" when that is
    // true of every link in it. One caching link among them makes the number
    // meaningful again, and a mixed run reported as uncacheable would be the
    // same lie in the other direction.
    // `M-29`: how much of this ledger could not have cached, rather than a
    // number that looks like a broken one. Counted rather than reduced to a
    // yes/no — a ledger is rarely all one link, and this repository's own is
    // 1519 subprocess calls out of 1523.
    let uncacheable = match binding
        .resolve("path.links")
        .map_err(|e| e.to_string())
        .and_then(|path| Links::load(&path).map_err(|e| e.to_string()))
    {
        Ok(links) => ledger
            .entries
            .iter()
            .filter(|entry| {
                links.get(&entry.link).ok().is_some_and(|link| !link.kind.prefix_caches())
            })
            .count(),
        // Unknown links are not spoken for: an unreadable config is a reason to
        // report the number and say nothing more.
        Err(_) => 0,
    };
    let cached = perp_core::cost::cached_total_as_text(
        total.usage.cache_hit_tokens,
        uncacheable,
        ledger.entries.len(),
    );
    println!(
        "{} calls · {} tokens in ({}) · {} out · {:.1}s · {:.6}",
        total.calls,
        total.usage.input_tokens(),
        cached,
        total.usage.output_tokens,
        total.latency_ms as f64 / 1000.0,
        // Six places, not four: a single call costs tens of millionths, and a
        // report that rounds every real amount to 0.0000 is decoration.
        total.charge
    );

    // `N-7`: the harness talking to itself, counted apart from the work.
    println!();
    print!("{}", perp_core::cost::Split::of(&ledger).render());

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
    let mut client = Client::new(&transport)
        .with_redaction(redaction(&binding))
        .with_egress(egress_for(&links));
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
        "tokens: {} in ({}) / {} out",
        served.reply.usage.prompt_tokens,
        perp_core::cost::cached_as_text(
            links.get(&served.link).ok().map(|link| link.kind),
            served.reply.usage.cache_hit_tokens,
        ),
        served.reply.usage.completion_tokens
    );
    Ok(())
}

/// `V-9`: ids are minted in the requirements source and cited everywhere else.
///
/// The failure this catches is quiet — a batch file naming `L-99`, a plan built
/// around a requirement that does not exist, and nobody noticing until the
/// cycle that tries to build it.
/// `perp check markers` — the reconcile `binding.md` has always specified.
///
/// *"A marker without a matching journal entry is not believed. The reconcile
/// step in C.1 checks markers against the code, not against each other."* That
/// step did not exist; `derive_marker` was written for it and had no caller.
///
/// It **checks and never writes**. `V-12` refuses the requirements source to
/// every writing tool and `cycle.rs` says the loop may not set a `✅`; a person
/// reads the evidence and marks. This only says where the file and the journal
/// disagree.
fn cmd_check_markers(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let source_path = binding.resolve("path.requirements").map_err(|e| e.to_string())?;
    let source = perp_core::layout::requirements_text(&source_path);
    if source.trim().is_empty() {
        return Err(format!("{}: no requirements found", source_path.display()));
    }
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;

    let claimed = perp_core::cycle::marked(&source);
    let check = perp_core::verify::check_markers(&claimed, &records);

    print!("{}", check.describe());
    if check.is_clean() {
        return Ok(());
    }
    Err(format!(
        "{} marker(s) the journal does not support. A person marks after reading the \
         evidence (`V-2`, `V-12`); this only says which claims the journal cannot back.",
        check.disagreements.len()
    ))
}

/// `perp redrun --gate <name> [--at <ref>]` — `V-3`'s red run.
///
/// Runs the gate against the tree **without** the working-tree change (a
/// throwaway worktree at `--at`, default `HEAD`) and then **with** it, stores
/// both transcripts, and says whether the test earned its place. A test that
/// passes both ways does not satisfy Perpetum's gate 4.
///
/// Operator-invoked, and that is the limitation worth naming: `V-3` says a new
/// test *must* be run this way, and nothing in the batch does it automatically
/// yet. Making that automatic needs a rule for "this change added a test" that
/// holds for any project, and a decision about paying for a second gate run on
/// every step. Until then the mechanism exists, runs, and is a command rather
/// than a promise.
fn cmd_redrun(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let name = flag(args, "--gate")
        .ok_or_else(|| "which gate? `perp redrun --gate test`".to_string())?;
    let at = flag(args, "--at").unwrap_or("HEAD");

    let gate = perp_core::gate::Gate::named(&binding, name).map_err(|e| e.to_string())?;
    let repo = perp_core::git::Repo::at(binding.root());
    let red = perp_core::verify::RedRun::perform(&repo, &gate, at).map_err(|e| e.to_string())?;

    // The transcripts, not just the verdict — a red run reported as a verdict
    // alone is the self-reported success `V-2` refuses.
    print!("{}", red.evidence());

    if red.verdict().is_earned() {
        return Ok(());
    }
    Err(format!(
        "the red run did not earn a green: {} (`V-3`)",
        red.verdict().describe()
    ))
}

fn cmd_check(args: &[&str]) -> std::result::Result<(), String> {
    let which = positionals(args).first().copied().unwrap_or("ids");
    match which {
        "ids" => {}
        "citations" => return cmd_check_citations(args),
        "markers" => return cmd_check_markers(args),
        other => {
            return Err(format!("unknown check `{other}` — `ids`, `citations` or `markers`"))
        }
    }

    let binding = load(args).map_err(|e| e.to_string())?;
    let source_path = binding.resolve("path.requirements").map_err(|e| e.to_string())?;
    // A file or a directory of them: `requirements_text` joins whichever it is
    // given. Reading the path straight meant a directory came back as
    // "Access is denied", which is what a `read` on one says on Windows and is
    // not a sentence about requirements.
    let source = perp_core::layout::requirements_text(&source_path);
    if source.trim().is_empty() {
        return Err(format!("{}: no requirements found", source_path.display()));
    }

    let mut documents = Vec::new();
    collect_markdown(&binding.root().join("docs"), &source_path, &mut documents)?;

    // `V-11`: zero documents is not a clean bill of health, it is a checker
    // that never ran. Exiting green here would look identical to a run that
    // scanned every document and found nothing wrong — the same shape of
    // green as a test suite that never executed the code it names.
    if documents.is_empty() {
        return Err(format!(
            "{}: no documents found to check — a checker that finds no input has not checked anything",
            binding.root().join("docs").display()
        ));
    }

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

/// `V-15`: a change may not claim an open requirement without a test naming it.
///
/// Reads the working diff against `HEAD`, because that is the change being
/// judged and the only artefact that distinguishes what this change asserts
/// from what the file already said.
///
/// Only open requirements count. Prose cites done ones constantly as reasons,
/// and treating those as claims reported three false alarms for every real one
/// when it was run over the commits that argued for this rule.
fn cmd_check_citations(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let source_path = binding.resolve("path.requirements").map_err(|e| e.to_string())?;
    let source = perp_core::layout::requirements_text(&source_path);
    if source.trim().is_empty() {
        return Err(format!("{}: no requirements found", source_path.display()));
    }

    let open: std::collections::BTreeSet<String> = perp_core::cycle::backlog(&source, usize::MAX)
        .into_iter()
        .map(|item| item.requirement)
        .collect();

    let repo = perp_core::git::Repo::at(binding.root());
    // `plumbing_all`, not `plumbing`: the latter caps each stream at forty
    // lines (`T-6`), and the first version of this read that tail, found no
    // claim in it, and reported the change clean — a false negative inside the
    // rule whose job is catching a claim nobody checked. `T-20` exists because
    // of it, and this is the caller it was written for.
    let diff = repo.plumbing_all(&["diff", "HEAD"]).map_err(|e| e.to_string())?;

    println!("open:      {}", open.len());
    if diff.trim().is_empty() {
        // `V-11`'s lesson: nothing to check is not a clean bill of health.
        println!("no change against HEAD — nothing to check");
        return Ok(());
    }

    let unbacked = verify::unbacked_citations(&diff, &open);
    if unbacked.is_empty() {
        println!("every open requirement this change claims has a test naming it");
        return Ok(());
    }
    for item in &unbacked {
        println!("  unbacked: {} claimed in {}", item.id, item.file);
    }
    Err(format!(
        "{} citation(s) with no test naming them — a citation is a claim about behaviour (`V-15`)",
        unbacked.len()
    ))
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

    let root = workspace_of(args);
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
    let root = workspace_of(args);
    let binding = load(args).map_err(|e| e.to_string())?;
    // `M-16`: before the batch, not inside its first step.
    warm_local_links(&binding, true)?;
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

    // `M-8`/`C-2`: with `--requirement`, the loop asks a model instead of
    // running gates. The id is mandatory — the loop does not build work that is
    // not on the record.
    if let Some(requirement) = flag(args, "--requirement") {
        // Look the requirement up where ids are minted (`V-9`).
        //
        // This used to pass the id as its own summary, so `perp run
        // --requirement R-17` told the model "R-17" and "do the work this
        // requirement describes" — and the model's first act was to grep the
        // repository for its own instructions. `perp cycle` has always read the
        // backlog; the single-requirement path never did, and the difference
        // was invisible because every real invocation happened to pass
        // `--brief`.
        //
        // Found by measuring something else: an A/B whose runs all opened with
        // `grep(pattern=R-17)`.
        // `requirements_text`, not `read_to_string`: the source is a file or a
        // directory of them, and reading a directory on Windows returns
        // "Access is denied" — which this then swallowed into an empty string
        // and a lookup that found nothing. The first version of this fix had
        // that bug and looked exactly like the one it was fixing.
        let source = perp_core::layout::requirements_text(
            &binding.resolve("path.requirements").map_err(|e| e.to_string())?,
        );
        // `stated`, not `backlog_all`: that one truncates to 160 characters for
        // a list a person scrolls, and using it here handed the model a
        // requirement cut off mid-sentence with nothing to say so.
        let stated = perp_core::cycle::stated(&source, requirement);

        let summary = flag(args, "--summary")
            .map(str::to_string)
            .or_else(|| stated.clone())
            .unwrap_or_else(|| requirement.to_string());
        let brief = flag(args, "--brief")
            .map(str::to_string)
            .or(stated)
            .unwrap_or_else(|| "Do the work this requirement describes.".to_string());
        let item = perp_core::agent::Item::new(requirement, &summary, &brief)
            .map_err(|e| e.to_string())?;

        let links = Links::load(&binding.resolve("path.links").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let transport = Curl::new();
        let mode = if args.contains(&"--local-only") { Mode::LocalOnly } else { Mode::Any };
        let mut agent = perp_core::agent::Agent::new(
            Client::new(&transport)
                .with_redaction(redaction(&binding))
                .with_egress(egress_for(&links)),
            &links,
            &AssumeHealthy,
            perp_core::agent::host_for(binding.root()),
            vec![item],
        );
        if mode == Mode::LocalOnly {
            agent = agent.local_only();
        }
        // `T-27`: the binding may size the repo map, or turn it off with 0.
        if let Ok(text) = binding.get("map.budget") {
            if let Ok(bytes) = text.trim().parse::<usize>() {
                agent = agent.with_map_budget(bytes);
            }
        }

        let report = engine.run(cycle, stage, &mut agent, 0).map_err(|e| e.to_string())?;
        println!("{}", report.describe());
        println!("spent {}", report.spend);
        for turn in &agent.turns {
            println!("  turn: {} rung, {} calls, via {}", turn.rung, turn.calls, turn.link);
        }
        // `L-29`: the same field the cycle now prints. A single-requirement run
        // lands its work through the same `land_batch`, so it can fail to
        // commit in the same silence.
        for warning in &report.warnings {
            println!("  · {warning}");
        }
        return match &report.stop {
            Some(perp_core::phase::Stop::BatchBlocked { why, .. }) => Err(format!("blocked: {why}")),
            _ if report.failed > 0 => Err(format!("{} steps failed", report.failed)),
            _ => Ok(()),
        };
    }

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
    let root = workspace_of(args);
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
            Ok(text) => without_timestamp(&text)
                    == without_timestamp(&render(&projection, 0, requirement_counts(&binding).as_ref())),
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
    // : the diff is read from the repository at the pinned commit, not
    // stored in the journal — git already keeps it, and two copies disagree.
    let chain = Chain::build(&subject, &records).with_diff(&Repo::at(binding.root()));
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

    // One message and out, for a caller that is not a terminal — the editor's
    // panel, most of it. Deliberately the *same* loop rather than a second
    // implementation beside it: a chat turn journals both sides, streams, is
    // read-only while the loop writes and records what it spent, and a shorter
    // path that skipped any of those would be a second set of rules nobody
    // audited. It differs in where the line comes from and nothing else.
    let once = flag(args, "--once");

    // `C-3`: if the loop holds the write lock, this session is read-only. The
    // lock file on disk is the authority — not a flag, not an assumption.
    let held = workspace_of(args).join("write.lock").exists();
    let chat_mode = perp_core::lock::chat_mode(held, false);
    if chat_mode == perp_core::lock::ChatMode::ReadOnly {
        println!("the loop is writing — this session is read-only (`C-3`)");
    }
    if once.is_none() {
        println!(
            "perp chat · {} slash commands, or just type. ctrl-d to leave.",
            command::NAMES.len()
        );
    }

    let transport = Curl::new();
    let mut client = Client::new(&transport)
        .with_redaction(redaction(&binding))
        .with_egress(egress_for(&links));
    let stdin = std::io::stdin();

    let lines: Box<dyn Iterator<Item = std::io::Result<String>>> = match once {
        Some(text) => Box::new(std::iter::once(Ok(text.to_string()))),
        None => Box::new(stdin.lock().lines()),
    };

    for line in lines {
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

                // What the workspace knows, ahead of the question. Without it a
                // model answers about a requirement id it has never seen, which
                // it does fluently and wrongly. Read per turn rather than once
                // at startup so an edit to the requirements is answered from
                // the file as it is now, and put in a system message so it is a
                // byte-identical prefix between turns and the provider's cache
                // pays for it (`M-12`).
                let vision = binding
                    .resolve("path.vision")
                    .ok()
                    .and_then(|path| std::fs::read_to_string(path).ok());
                let requirements = binding
                    .resolve("path.requirements")
                    .ok()
                    .map(|path| perp_core::layout::requirements_text(&path));
                let projection = replay(&records);
                let context = chat::Context {
                    vision: vision.as_deref(),
                    requirements: requirements.as_deref(),
                    projection: Some(&projection),
                };
                // Two system messages, not one. The first is byte-identical
                // between turns and is what a provider's cache can charge a
                // fiftieth for; the second carries the position, which moves
                // every turn because a turn itself appends to the journal.
                let request = ChatRequest::new(vec![
                    Message::system(context.stable()),
                    Message::system(context.volatile()),
                    Message::user(text),
                ]);
                let link = match links.resolve(Role::Chat, &AssumeHealthy, mode) {
                    Ok(link) => link.clone(),
                    Err(e) => {
                        println!("no link for chat: {e}");
                        continue;
                    }
                };

                // `C-4`: streamed and interruptible. The partial is journalled,
                // never discarded — the half a model produced before someone
                // stopped it is the interesting half when an answer was going
                // wrong.
                let mut printed = false;
                let streamed = client.stream(
                    &link,
                    &request,
                    || false,
                    |event| {
                        if let perp_core::stream::Event::Delta(delta) = event {
                            print!("{delta}");
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                            printed = true;
                        }
                    },
                );

                match streamed {
                    Ok(streamed) => {
                        if printed {
                            println!();
                        }
                        println!("[via {} — {}]", link.name, streamed.stop.describe());
                        if streamed.has_content() {
                            let next = step.next();
                            let mut turn = chat::Turn::assistant(
                                &streamed.content,
                                time::now(),
                                &link.name,
                            );
                            if streamed.stop == perp_core::stream::Stop::Interrupted {
                                turn = turn.interrupted();
                            }
                            journal
                                .append(&turn.record(next.clone()))
                                .map_err(|e| e.to_string())?;

                            // `M-11`: a streamed call costs the same as a
                            // buffered one. Losing the accounting because the
                            // tokens arrived a few at a time would make every
                            // cost report quietly wrong in the direction that
                            // flatters it.
                            // The cached half of the prompt is priced at a
                            // fraction of the rest, so passing zero here
                            // charged every hit at miss price and made the
                            // chat ledger wrong in the expensive direction.
                            let cached = streamed.cached_tokens;
                            let usage = perp_core::cost::Usage::from_reply(
                                streamed.prompt_tokens,
                                streamed.completion_tokens,
                                cached,
                                (streamed.prompt_tokens - cached).max(0),
                            );
                            let price = links.price(&link.name);
                            let entry = perp_core::cost::Entry {
                                step: next.to_string(),
                                role: "chat".into(),
                                link: link.name.clone(),
                                model: link.model.clone(),
                                usage,
                                latency_ms: streamed
                                    .first_token
                                    .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
                                    .unwrap_or_default(),
                                charge: price.charge(&usage),
                            };
                            journal
                                .append(&perp_core::cost::annotate(
                                    Record::outcome(
                                        next,
                                        time::now(),
                                        true,
                                        format!("chat call to {}", link.name),
                                    ),
                                    &entry,
                                ))
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    // A silent or broken link falls back to the non-streaming
                    // path, which walks the whole role chain (`M-9`).
                    Err(e) => {
                        println!("[streaming failed: {e} — falling back]");
                        match client.call(
                            &links,
                            Role::Chat,
                            &request,
                            &AssumeHealthy,
                            mode,
                            time::now(),
                        ) {
                            Ok(served) => {
                                println!("{}", served.reply.content);
                                println!("[via {}]", served.provenance());
                                let next = step.next();
                                let turn = chat::Turn::assistant(
                                    &served.reply.content,
                                    time::now(),
                                    &served.link,
                                );
                                journal
                                    .append(&turn.record(next.clone()))
                                    .map_err(|e| e.to_string())?;
                                journal
                                    .append(&served.to_record(
                                        next,
                                        time::now(),
                                        links.price(&served.link),
                                    ))
                                    .map_err(|e| e.to_string())?;
                            }
                            Err(e) => println!("no link answered: {e}"),
                        }
                    }
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
        Command::Status => {
            Ok(render(&replay(records), time::now(), requirement_counts(binding).as_ref()))
        }
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
            // The artifact path is the authority. `out.board` named
            // `.harness/progress-board.md`, which nothing has ever
            // written — this command read a file that did not exist while the
            // board sat in the artifacts directory beside it.
            let path = perp_core::artifact::Kind::Board.path_in(binding.root());
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
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
        // `O-3`: these write the control file the engine reads at its next
        // step boundary. They work whether or not a loop is running, which is
        // the point of a file rather than a signal.
        Command::Pause => ask_control(binding, &Control::Pause),
        Command::Resume => ask_control(binding, &Control::Run),
        Command::Step => ask_control(binding, &Control::Step),
        // `/rewind` and `/approve` stay out of chat deliberately. Both are
        // `needs_confirmation()`, and a confirmation typed into the same box as
        // everything else is not a confirmation.
        Command::Rewind { to } => Err(format!(
            "rewinding to {to} undoes work - run `perp rewind {to}` to see what, \
             then `--approve <your name>` (`O-4`)"
        )),
        Command::Approve { id } | Command::Reject { id } => Err(format!(
            "approval #{id} is answered where the queue is, not in chat - an approval \
             typed into the same box as everything else is not an approval (`T-7`)"
        )),
        Command::Gate { name } => Err(format!(
            "run `perp gate {}` - a gate is only green if the harness ran it and kept \
             the transcript (`V-2`)",
            name.clone().unwrap_or_else(|| "all".into())
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

/// Render artifacts from the journal (`A-1`–`A-7`, `O-2`).
///
/// A render that fails is reported and does not change the exit code: `A-7`
/// says artifact generation is never on the critical path, and an exit code is
/// how a caller decides whether the critical path held.
fn cmd_artifact(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let projection = replay(&records);

    let wanted = positionals(args).first().copied().unwrap_or("all");
    let kinds = match wanted {
        "all" => artifact::Kind::ALL.to_vec(),
        name => vec![artifact::Kind::parse(name).map_err(|e| e.to_string())?],
    };

    let dir = perp_core::artifact::dir_in(binding.root());
    let provenance = artifact::Provenance::from_journal(
        projection.cycle.unwrap_or(1),
        time::now(),
        &records,
    )
    .sha(Repo::at(binding.root()).head_sha().ok())
    .batch(projection.stage.clone().unwrap_or_else(|| "—".into()));

    let mut written = 0;
    for kind in kinds {
        match artifact::try_render(kind, &projection, &records, &provenance) {
            Ok(rendered) => match rendered.write(&dir) {
                Ok(path) => {
                    println!("{}", path.display());
                    written += 1;
                }
                Err(e) => println!("warning: {kind} could not be written: {e} (`A-7`)"),
            },
            Err(warning) => println!("warning: {warning}"),
        }
    }
    println!("{written} written to {}", dir.display());
    Ok(())
}

/// The live view (`O-5`). One snapshot, or repeated with `--every`.
fn cmd_watch(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let every: Option<u64> = flag(args, "--every")
        .map(|text| text.parse().map_err(|_| format!("--every takes seconds, not `{text}`")))
        .transpose()?;

    loop {
        let records = journal.read_all().map_err(|e| e.to_string())?;
        let projection = replay(&records);
        // Replayed, not guessed. This reported a hard-coded zero and said so:
        // "approvals live in the engine's queue, which only exists inside a
        // run". The queue is rebuilt from the journal now, so a request raised
        // by a run that has since been killed is still counted — which is
        // exactly the moment an operator wants to know about it.
        let waiting = Approvals::replay(&records).pending(perp_core::time::now()).len();
        let snapshot = Snapshot::of(&projection, &records, waiting);
        println!("{snapshot}");

        let Some(seconds) = every else { return Ok(()) };
        println!("---");
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
}

/// Live control (`O-3`). Writes what the operator asked for; the engine reads
/// it at the next step boundary and never mid-step.
/// The approvals queue: list it, or answer one (`T-14`, `T-15`).
///
/// Answered here and not in chat, deliberately. An approval typed into the
/// same box as everything else is indistinguishable from a model repeating
/// what it was told to say, and `T-7` makes tool output data rather than
/// instruction for exactly that reason.
/// Every fork the loop took (`O-12`).
///
/// The journal answers "what happened"; this answers "what else was on the
/// table", which is the question nobody can reconstruct afterwards and the one
/// an auditor actually asks.
fn cmd_decisions(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let log = perp_core::decision::log(&records);

    let since = flag(args, "--since");
    let decider = flag(args, "--decider");
    let shown: Vec<&perp_core::decision::Decision> = log
        .iter()
        .filter(|d| since.is_none_or(|s| d.step.to_string().as_str() >= s))
        .filter(|d| decider.is_none_or(|w| d.decider.kind() == w))
        .collect();

    if shown.is_empty() {
        println!("no decisions recorded — the loop has not reached a fork it writes down");
        return Ok(());
    }
    println!("{} decision(s) of {} in the journal
", shown.len(), log.len());
    for decision in shown {
        print!("{}", decision.describe());
        // A reversal is the most useful record in the log, so it is not left
        // for the reader to notice (`O-11`).
        if let Some(later) = decision.superseded_by(&log) {
            println!("  SUPERSEDED by #{later}");
        }
        println!();
    }
    Ok(())
}

/// Release notes, derived from the commits themselves (`O-14`).
///
/// `git log <since>..<until>`, grouped by the `Requirement:` trailer (`G-2`)
/// each commit already carries. No prose is generated — a commit with no
/// trailer is listed apart rather than folded into a neighbour's section
/// (`T-6`), and the operator pastes what they want into `release-notes.md`.
fn cmd_changelog(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let Some(since) = flag(args, "--since") else {
        return Err("which range? `perp changelog --since <ref> [--until <ref>]`".into());
    };
    let until = flag(args, "--until").unwrap_or("HEAD");

    let source_path = binding.resolve("path.requirements").map_err(|e| e.to_string())?;
    let source = perp_core::layout::requirements_text(&source_path);

    let repo = perp_core::git::Repo::at(binding.root());
    let raw = repo.log_between(since, until).map_err(|e| e.to_string())?;
    let entries = perp_core::changelog::parse(&raw);

    if entries.is_empty() {
        println!("no commits between {since} and {until}");
        return Ok(());
    }
    print!("{}", perp_core::changelog::render(&entries, &source));
    Ok(())
}

fn cmd_approvals(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let queue = Approvals::replay(&records);

    let waiting = queue.pending(perp_core::time::now());
    if waiting.is_empty() {
        println!("nothing waiting for a person");
        return Ok(());
    }
    println!("{} waiting:\n", waiting.len());
    for entry in waiting {
        println!("{}", entry.request.describe());
    }
    println!("answer with: perp approve <id> --by <your name>   (or `perp reject`)");
    Ok(())
}

/// Grant or refuse one request, by id (`T-14`).
fn cmd_answer(args: &[&str], granted: bool) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let Some(id_text) = positionals(args).first().copied() else {
        return Err("which one? `perp approve <id> --by <your name>`".into());
    };
    let id: u64 = id_text.parse().map_err(|_| format!("`{id_text}` is not an approval id"))?;
    // Named on purpose: an approval with nobody's name against it is an
    // approval nobody gave, and the journal is the only record of who did.
    let Some(who) = flag(args, "--by") else {
        return Err(
            "who is approving? `--by <your name>` — an approval with no name on it is not one"
                .into(),
        );
    };

    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let mut queue = Approvals::replay(&records);
    let now = perp_core::time::now();

    let Some(entry) = queue.entries().iter().find(|e| e.request.id == id) else {
        return Err(format!("no approval #{id} — `perp approvals` lists what is waiting"));
    };
    let step = entry.request.step.clone();
    let entry_what = entry.request.what.clone();

    if granted {
        queue.grant(id, who, now).map_err(|e| e.to_string())?;
    } else {
        queue.refuse(id, who, now).map_err(|e| e.to_string())?;
    }
    journal
        .append(&Approvals::answered_record(step.clone(), id, granted, who, now))
        .map_err(|e| e.to_string())?;

    // `O-10`: an approval is the clearest fork there is — a person chose, and
    // the alternative was the opposite. `O-9`'s `person` decider exists for
    // exactly this, and it is the only one where the name is the whole point.
    let (chose, other) = if granted {
        ("grant it", "refuse it")
    } else {
        ("refuse it", "grant it")
    };
    let mut decision = perp_core::decision::Decision::new(
        chose,
        vec![other.to_string()],
        format!("approval #{id}: {}", entry_what),
        perp_core::decision::Decider::Person { name: who.to_string() },
        step,
        now,
    );
    // `O-11`: answering an approval that was already answered the other way is
    // a reversal, and the reversal is the most useful record in the log — the
    // only one carrying what was learned. Named rather than left for a reader
    // to spot by comparing timestamps.
    if let Some(earlier) = perp_core::decision::log(&records)
        .iter()
        .rfind(|d| d.why.starts_with(&format!("approval #{id}:")) && d.chose != chose)
    {
        decision = decision.superseding(earlier.id);
    }
    journal.append(&decision.to_record()).map_err(|e| e.to_string())?;

    println!(
        "approval #{id} {} by {who}. It applies to this cycle and no later one (`T-15`).",
        if granted { "granted" } else { "refused" }
    );
    Ok(())
}

/// Break a lock a dead run left behind (`N-1`, `L-20`).
///
/// The operator had no way to do this. A killed run leaves a lock that outlives
/// it, and until the heartbeat TTL lapses every later command refuses — so the
/// only move was deleting the file by hand, which is the one move that is
/// dangerous when the holder is actually alive.
///
/// It prints who held it rather than doing it quietly: if that names a process
/// still running, the operator has just been told they are about to have two
/// writers, which is the thing `L-20` exists to prevent.
fn cmd_unlock(args: &[&str]) -> std::result::Result<(), String> {
    let root = workspace_of(args);
    let dir = perp_core::layout::dir_in(&root);

    let mut broke_any = false;
    for kind in [perp_core::lock::Kind::Write, perp_core::lock::Kind::Gate] {
        let path = dir.join(kind.file_name());
        match perp_core::lock::Lock::break_at(&path).map_err(|e| e.to_string())? {
            Some(holder) => {
                broke_any = true;
                println!("broke the {} lock — held by {}", kind.as_str(), holder.describe());
            }
            None if path.exists() => {
                broke_any = true;
                println!("removed an unreadable {} lock at {}", kind.as_str(), path.display());
            }
            None => {}
        }
    }
    if !broke_any {
        println!("no lock held — nothing to break");
    }
    Ok(())
}

fn cmd_control(args: &[&str]) -> std::result::Result<(), String> {
    let root = workspace_of(args);
    load(args).map_err(|e| e.to_string())?;
    let channel = Channel::at(&root);

    let what = positionals(args).first().copied().unwrap_or("status");
    let control = match what {
        "status" => {
            let current = channel.read().map_err(|e| e.to_string())?;
            println!("{current}");
            return Ok(());
        }
        "run" | "resume" => Control::Run,
        "pause" => Control::Pause,
        "step" => Control::Step,
        "abort" => Control::Abort {
            who: flag(args, "--who").unwrap_or("the operator").to_string(),
        },
        "inject" => Control::Inject {
            text: positionals(args)
                .get(1)
                .copied()
                .ok_or_else(|| "inject takes a message".to_string())?
                .to_string(),
        },
        "redirect" => Control::Redirect {
            requirement: positionals(args)
                .get(1)
                .copied()
                .ok_or_else(|| "redirect takes a requirement id".to_string())?
                .to_string(),
        },
        other => {
            return Err(format!(
                "`{other}` is not a control — try status, pause, resume, step, inject, \
                 redirect, abort"
            ))
        }
    };

    channel.ask(&control).map_err(|e| e.to_string())?;
    println!("{control} — takes effect at the next step boundary");
    Ok(())
}

/// Rewind (`O-4`). Prints what would be discarded and refuses to do it without
/// `--approve`, because it is the one operation here that destroys work.
fn cmd_rewind(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;

    let target = positionals(args)
        .first()
        .copied()
        .ok_or_else(|| "expected a step id to rewind to".to_string())?;
    let target = StepId::parse(target).map_err(|e| e.to_string())?;
    let plan = Rewind::plan(&target, &records).map_err(|e| e.to_string())?;

    println!("{}", plan.describe());

    if plan.discards_nothing() {
        return Ok(());
    }
    if !plan.is_restorable() {
        return Err(
            "that step has no commit, so there is nothing to revert back to. Moving the \
             journal alone would leave the record and the tree disagreeing (`L-4`)"
                .into(),
        );
    }
    let Some(who) = flag(args, "--approve") else {
        // The plan is free; the destruction is not. Printing what would happen
        // and stopping is the whole design of this command.
        return Err(
            "not done — rewind discards work and needs `--approve <your name>` (`O-4`)".into(),
        );
    };

    // The record goes in first. The journal is append-only (`L-3`): rewinding
    // the workspace does not rewind the record, and a reader later sees both
    // the work and the decision to abandon it.
    let step = next_step_for(&records, "rewind");
    journal
        .append(&plan.record(step.clone(), time::now()))
        .map_err(|e| e.to_string())?;

    let sha = plan.commit.clone().unwrap_or_default();
    let repo = Repo::at(binding.root());
    // Revert, never reset: `G-10` makes `reset --hard` a Never and `G-8` makes
    // revert the right answer. Nothing is destroyed on either side.
    let outcome = repo.revert_since(&sha, who);
    let record = match &outcome {
        Ok(_) => Record::outcome(step, time::now(), true, format!("reverted to {sha} by {who}")),
        Err(e) => Record::outcome(step, time::now(), false, format!("rewind failed: {e}")),
    };
    journal.append(&record).map_err(|e| e.to_string())?;
    outcome.map_err(|e| e.to_string())?;

    println!("workspace returned to {sha} by revert — the old commits are still in history");
    println!("the journal keeps every superseded step too; it is append-only (`L-3`)");
    Ok(())
}

/// Write a control from chat. The engine picks it up at the next boundary.
fn ask_control(binding: &Binding, control: &Control) -> std::result::Result<String, String> {
    Channel::at(binding.root()).ask(control).map_err(|e| e.to_string())?;
    Ok(format!("{control} — takes effect at the next step boundary"))
}

/// The panel's data surface (`I-1`, `I-5`). One JSON document from the journal,
/// which is what the editor renders — it holds nothing of its own.
fn cmd_panel(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;

    let dir = perp_core::artifact::dir_in(binding.root());
    let artifacts = std::fs::read_dir(&dir)
        .map(|entries| {
            let mut names: Vec<String> = entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".html"))
                .collect();
            names.sort();
            names
        })
        .unwrap_or_default();

    // Approvals live in a running engine's memory, so a panel reading the
    // journal alone reports none rather than guessing at some.
    // `I-3`: the diff travels with the view, so the panel cannot offer an
    // approve button before it has something to show.
    // The requirements text travels with the view so a panel can say what an id
    // means. Read here rather than in `View::of`, which folds records and knows
    // nothing about files.
    let source = binding
        .resolve("path.requirements")
        .ok()
        .map(|path| perp_core::layout::requirements_text(&path))
        .unwrap_or_default();
    let view = View::of(&records, &Approvals::new(), artifacts, time::now())
        .with_requirements(&source)
        .with_review(&Repo::at(binding.root()), &Approvals::new(), time::now());
    println!("{}", view.to_json());
    Ok(())
}

/// Screen evidence (`X-6`) and scheduler registration (`X-9`).
fn cmd_capture(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let journal_path = binding.resolve("out.journal").map_err(|e| e.to_string())?;
    let journal = Journal::at(&journal_path);
    let records = journal.read_all().map_err(|e| e.to_string())?;

    let claim = positionals(args)
        .first()
        .copied()
        .ok_or_else(|| "expected what this is evidence of — a screenshot with no claim \
                        attached is a picture, not evidence (`A-6`)".to_string())?;

    let step = next_step_for(&records, "evidence");
    let shot = perp_core::capture::capture(&journal_path, &step, claim)
        .map_err(|e| e.to_string())?;
    journal
        .append(&shot.record(step, time::now()))
        .map_err(|e| e.to_string())?;

    println!("{} · {} bytes", shot.path.display(), shot.bytes);
    println!("evidence of: {claim}");
    Ok(())
}

/// Register with the OS scheduler so a cycle survives a reboot (`X-9`).
///
/// Prints the exact command and refuses without `--approve`: an approval for
/// "register with the scheduler" is not one anybody can evaluate.
fn cmd_schedule(args: &[&str]) -> std::result::Result<(), String> {
    let binding = load(args).map_err(|e| e.to_string())?;
    let when = match flag(args, "--every") {
        Some(text) => perp_core::capture::When::Every {
            minutes: text.parse().map_err(|_| format!("--every takes minutes, not `{text}`"))?,
        },
        None => perp_core::capture::When::AtBoot,
    };

    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let plan = perp_core::capture::Registration::plan(&program, binding.root(), when);

    println!("would register `{}` to run {}", plan.name, plan.when);
    println!("  {}", plan.command);
    println!("undo with:");
    println!("  {}", plan.removal);

    let Some(who) = flag(args, "--approve") else {
        return Err(
            "not done — registering outlives this cycle and this session, and needs \
             `--approve <your name>` (`X-9`)"
                .into(),
        );
    };

    let journal = Journal::at(binding.resolve("out.journal").map_err(|e| e.to_string())?);
    let records = journal.read_all().map_err(|e| e.to_string())?;
    let step = next_step_for(&records, "schedule");
    let outcome = plan.install(who);
    match &outcome {
        Ok(message) => {
            journal
                .append(&plan.record(step, time::now(), who))
                .map_err(|e| e.to_string())?;
            println!("{message}");
        }
        Err(e) => {
            journal
                .append(&Record::outcome(
                    step,
                    time::now(),
                    false,
                    format!("scheduler registration failed: {e}"),
                ))
                .map_err(|err| err.to_string())?;
        }
    }
    outcome.map(|_| ()).map_err(|e| e.to_string())
}

/// Run a whole cycle unattended (`L-1`, `L-14`).
///
/// The one command that leaves the machine alone with the work. Everything it
/// can do is bounded before it starts: a budget from the binding, a batch
/// allowance, a backlog it reads rather than invents, and a permission
/// classifier it cannot argue with.
fn cmd_cycle(args: &[&str]) -> std::result::Result<(), String> {
    let root = workspace_of(args);
    let binding = load(args).map_err(|e| e.to_string())?;
    // `M-16`: before the batch, not inside its first step.
    warm_local_links(&binding, true)?;

    let cycle: u32 = flag(args, "--cycle")
        .map(|t| t.parse().map_err(|_| format!("--cycle takes a number, not `{t}`")))
        .transpose()?
        .unwrap_or(1);
    let batches: u32 = flag(args, "--batches")
        .map(|t| t.parse().map_err(|_| format!("--batches takes a number, not `{t}`")))
        .transpose()?
        .unwrap_or(5);
    let per_batch: usize = flag(args, "--items")
        .map(|t| t.parse().map_err(|_| format!("--items takes a number, not `{t}`")))
        .transpose()?
        .unwrap_or(3);
    let from = flag(args, "--phase")
        .and_then(|t| t.chars().next())
        .and_then(Phase::parse)
        .unwrap_or(Phase::D);

    let links = Links::load(&binding.resolve("path.links").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // `M-24`, before anything is spent: a link whose variable is unset fails
    // here rather than on the eleventh call.
    let missing = links.missing_credentials();
    if !missing.is_empty() {
        for (link, variable) in &missing {
            eprintln!("link `{link}` needs ${variable}, which is not set");
        }
        return Err("cannot run unattended with a link that cannot authenticate (`M-24`)".into());
    }

    let budgets = Engine::open(&root).map_err(|e| e.to_string())?.budgets();
    if budgets.cycle.is_unlimited() && budgets.batch.is_unlimited() {
        return Err(
            "refusing to run unattended with no budget in any currency. Declare `budget.*` in \
             the binding — an overnight run with no ceiling is a decision, and it should be \
             made on purpose (`L-9`)"
                .into(),
        );
    }

    let source = perp_core::layout::requirements_text(
        &binding.resolve("path.requirements").map_err(|e| e.to_string())?,
    );
    let waiting = perp_core::cycle::backlog(&source, usize::MAX).len();

    // `O-16`: an empty backlog that is empty because everything left is waiting
    // on a person is not the same as one with nothing in it, and until now they
    // printed the same line. A loop blocked on you must not look finished.
    let waiting_on_you = perp_core::cycle::gated(&source);

    println!("cycle {cycle}, from phase {from}");
    println!("  backlog     {waiting} open requirement(s)");
    if !waiting_on_you.is_empty() {
        println!(
            "  gated       {} waiting on a person: {}",
            waiting_on_you.len(),
            waiting_on_you.iter().map(|g| g.id.as_str()).collect::<Vec<_>>().join(", ")
        );
        if waiting == 0 {
            println!(
                "              the backlog is empty because of these, not because                  there is nothing to do"
            );
        }
    }
    println!("  allowance   {batches} batches x {per_batch} items");
    println!("  budget      cycle {:?} / batch {:?}", budgets.cycle, budgets.batch);
    println!("  workspace   {}", root.display());
    println!();

    let transport = Curl::new();
    let mode = if args.contains(&"--local-only") { Mode::LocalOnly } else { Mode::Any };
    let driver = perp_core::cycle::Driver {
        root: root.clone(),
        links: &links,
        health: &AssumeHealthy,
        mode,
        batches,
        items_per_batch: per_batch,
        transport: &transport,
    };

    let outcome = driver.run(cycle, from).map_err(|e| e.to_string())?;
    print!("{}", outcome.describe());

    // The loop does not mark its own work done (`V-2`, Perpetum 0.7). It says
    // what it did; a person reads the evidence and sets the marker.
    println!();
    println!("nothing was marked done — the loop writes evidence, a person reads it and marks");
    println!("read it with: perp explain <requirement>");

    // A cycle that ended with a red gate exits non-zero. The first hour-long
    // run left failing tests behind and exited 0, because only a blocked batch
    // was checked — and an unattended run's exit code is the one thing a
    // scheduler reads.
    let failed: u32 = outcome.legs.iter().map(|leg| leg.report.failed).sum();
    match &outcome.stop {
        Some(perp_core::phase::Stop::BatchBlocked { why, .. }) => Err(format!("blocked: {why}")),
        _ if failed > 0 => Err(format!("{failed} step(s) failed")),
        _ => Ok(()),
    }
}
