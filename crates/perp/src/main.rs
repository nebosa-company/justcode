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
use perp_core::journal::{Journal, Record};
use perp_core::state::{render, replay};
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

fn write_state(path: &Path, rendered: &str) -> Result<()> {
    atomic::write_atomic(path, rendered)?;
    println!("wrote {}", path.display());
    Ok(())
}
