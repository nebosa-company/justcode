//! Scaffold a workspace (`L-21`).
//!
//! There was no way to start one. The four files a binding needs had to be typed
//! by hand from an example, which is a poor first five minutes and an easy place
//! to get a path wrong — and a wrong path is a loop that stops and asks before it
//! does anything.
//!
//! Nothing here is clever. It writes the smallest binding that resolves, a
//! requirements directory with one file in it, a vision with the question worth
//! answering, and a links file with every provider commented out so choosing one
//! is deleting a `#` rather than remembering a schema.
//!
//! **It never overwrites.** A second `init` on a workspace that has some of these
//! fills in what is missing and reports what it left alone. Anything else would
//! make `init` a command you cannot run twice, and the second run is exactly when
//! someone reaches for it.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::layout;

/// What `init` did to one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wrote {
    Created(PathBuf),
    Kept(PathBuf),
}

impl Wrote {
    pub fn describe(&self) -> String {
        match self {
            Wrote::Created(path) => format!("created  {}", path.display()),
            Wrote::Kept(path) => format!("kept     {} (already there)", path.display()),
        }
    }

    pub fn created(&self) -> bool {
        matches!(self, Wrote::Created(_))
    }
}

/// The starter binding.
///
/// Points at a requirements *directory* rather than a single file, because a
/// project that outgrows one file should not have to change its binding to split
/// it — and one that never does is unbothered by the folder.
///
/// There is deliberately **no** `gate.test`.
///
/// The first attempt wrote a placeholder that was supposed to fail:
/// `echo '...' && exit 1`. It reported green. Commands are spawned directly with
/// no shell — `&&` went to `echo` as a literal argument, `exit 1` never ran, and a
/// freshly-initialised workspace would have called work green with no tests,
/// which is the exact thing `V-2` exists to prevent.
///
/// Nothing is needed in its place. With no `gate.*` key the harness already
/// refuses: "the binding declares no gates — there is nothing to be green". The
/// honest starting state is the absence, not a fake.
const BINDING: &str = "\
# Binding

What this project calls the things Perpetum names. The block below is what the
engine reads; everything outside it is for people.

```perp-binding
path.requirements = .harness/requirements
path.vision       = .harness/vision.md
path.links        = .harness/links.md

out.journal = .harness/journal.jsonl
out.state   = .harness/state.md

gate.cwd     = .
gate.timeout = 300
# gate.test  = <your test command>

budget.cycle.money   = 1.00
budget.cycle.seconds = 3600
budget.batch.money   = 0.30
budget.batch.seconds = 900

git.branch.batch = perp/c{cycle}/b{batch}
git.push         = approval
```

## The gate

Uncomment `gate.test` and give it your test command. It is the definition of
working: until it exists the harness refuses to run at all, because there is
nothing that could make anything green.

Add as many as you like — any key starting `gate.` is one, and they run in the
order written, stopping at the first red.

**Commands are run directly, not through a shell.** `python -m pytest tests -q`
is fine; `cd sub && make` is not, because `&&` arrives as an argument rather than
as shell logic. Put a sequence in a script and call the script.
";

const REQUIREMENTS: &str = "\
# Requirements

Ids are minted here and cited everywhere else. Never reused, never renumbered.

| id | Requirement |
|---|---|
| `R-1` | Replace this row. Name the behaviour, give one concrete example, and say which file it belongs in — a requirement the loop can test is one that says what \"working\" looks like. |

## Markers

Written next to the id, by a person and never by the loop.

| Marker | Meaning |
|---|---|
| *(none)* | open |
| 🟡 | in progress |
| ✅ + `~~id~~` | done: implemented, gates green, transcript in the journal |
| ⛔ | gated — carries what is missing |
| ❌ | won't do — carries the reason |

## More than one file

This directory is read recursively, so requirements can be split by area into
subdirectories once one file stops being comfortable. Ids stay unique across all
of them.
";

const VISION: &str = "\
# Vision

One paragraph on what this is for. The loop reads it when a requirement is
ambiguous, so write the thing a new colleague would need to resolve a judgement
call.

## What it will never be

The more useful half. A list of things that are out of scope, so a plausible
suggestion can be refused by pointing at a line rather than by arguing.

1.
2.
3.
";

/// Every provider, commented out.
///
/// Choosing one should be deleting a `#`, not remembering a schema. The
/// `auth_env` values name an environment variable and never hold a key (`S-2`).
const LINKS: &str = "\
# Links

Which models this project may use, and for what. Credentials are named here by
environment variable and never written here.

```perp-links
# Pick one or more, uncomment it, and set the variable it names.
#
# `base_url` can be left out for anything with an obvious address — it is only
# needed for a gateway, a proxy, or a local server on another port.

# --- local ---------------------------------------------------------------
# link.local.kind     = lmstudio
# link.local.model    = qwen2.5-coder-32b
# link.local.privacy  = local

# link.ollama.kind    = ollama
# link.ollama.model   = qwen2.5-coder
# link.ollama.privacy = local

# link.vllm.kind      = vllm
# link.vllm.model     = Qwen/Qwen2.5-Coder-32B-Instruct
# link.vllm.privacy   = local

# --- hosted --------------------------------------------------------------
# link.openai.kind      = openai
# link.openai.model     = gpt-5
# link.openai.auth_env  = OPENAI_API_KEY

# link.claude.kind      = anthropic
# link.claude.model     = claude-opus-5
# link.claude.auth_env  = ANTHROPIC_API_KEY

# link.deepseek.kind    = deepseek
# link.deepseek.model   = deepseek-v4
# link.deepseek.auth_env = DEEPSEEK_API_KEY

# link.grok.kind        = grok
# link.grok.model       = grok-4
# link.grok.auth_env    = XAI_API_KEY

# --- a command rather than an endpoint -----------------------------------
# link.claude-code.kind  = claude-cli
# link.claude-code.model = claude-opus-5

# --- roles ---------------------------------------------------------------
# Which link answers for what. A role may name several, tried in order.
# role.coder    = local
# role.chat     = local
# role.verifier = local
```

## Prices

Money is counted from what a link reports, never estimated, so a hosted link
needs its prices in dollars per million tokens:

```
# price.openai.cache_hit  = 0.13
# price.openai.cache_miss = 1.25
# price.openai.output     = 10.00
```

A local link needs none: it reports tokens and time, and spends no money.
";

/// Create whatever is missing under `.harness/`.
pub fn run(root: &Path) -> Result<Vec<Wrote>> {
    let mut done = Vec::new();
    let dir = layout::dir_in(root);
    std::fs::create_dir_all(&dir).map_err(|e| crate::error::Error::io(&dir, e))?;
    let requirements = root.join(layout::REQUIREMENTS);
    std::fs::create_dir_all(&requirements)
        .map_err(|e| crate::error::Error::io(&requirements, e))?;

    for (relative, body) in [
        (layout::BINDING, BINDING),
        (layout::VISION, VISION),
        (layout::LINKS, LINKS),
        (".harness/requirements/requirements.md", REQUIREMENTS),
    ] {
        let path = root.join(relative);
        if path.exists() {
            done.push(Wrote::Kept(PathBuf::from(relative)));
            continue;
        }
        std::fs::write(&path, body).map_err(|e| crate::error::Error::io(&path, e))?;
        done.push(Wrote::Created(PathBuf::from(relative)));
    }

    if layout::ensure_gitignore(root) {
        done.push(Wrote::Created(PathBuf::from(layout::GITIGNORE)));
    } else {
        done.push(Wrote::Kept(PathBuf::from(layout::GITIGNORE)));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    #[test]
    fn a_fresh_workspace_binds_immediately() {
        // The whole point: the four files it writes have to resolve, or `init`
        // has produced a workspace whose first command fails.
        let root = tmpdir("init-fresh");
        let done = run(&root).expect("init");
        assert!(done.iter().all(Wrote::created), "everything was new: {done:?}");

        let binding = crate::Binding::load(&root).expect("the binding it wrote loads");
        binding.verify().expect("and every path it names exists");
        assert_eq!(
            binding.get("path.requirements").expect("key"),
            ".harness/requirements"
        );
    }

    #[test]
    fn running_it_twice_keeps_what_is_there() {
        // `init` is reached for a second time exactly when a workspace is half
        // set up, so overwriting would eat the half somebody wrote.
        let root = tmpdir("init-twice");
        run(&root).expect("first");
        std::fs::write(root.join(crate::layout::VISION), "# My vision\n").expect("edit");

        let done = run(&root).expect("second");
        assert!(done.iter().all(|entry| !entry.created()), "nothing new: {done:?}");
        assert_eq!(
            std::fs::read_to_string(root.join(crate::layout::VISION)).expect("read"),
            "# My vision\n",
            "the edit survived"
        );
    }

    #[test]
    fn a_fresh_workspace_has_no_gate_and_so_cannot_go_green() {
        // The first version of this wrote a placeholder meant to fail:
        // `echo '...' && exit 1`. It reported green — commands are spawned with no
        // shell, so `&&` reached `echo` as an argument and `exit 1` never ran.
        // A workspace that calls work green with no tests is the one outcome
        // `V-2` exists to prevent, so there is no gate at all now.
        let root = tmpdir("init-gate");
        run(&root).expect("init");
        let binding = crate::Binding::load(&root).expect("binding");
        assert!(binding.get("gate.test").is_err(), "no test gate is configured");

        let gates = crate::gate::Gate::from_binding(&binding);
        assert!(
            gates.is_err() || gates.as_ref().is_ok_and(Vec::is_empty),
            "and nothing claims to be one: {gates:?}"
        );
    }

    #[test]
    fn the_starter_requirements_are_a_directory_that_reads() {
        let root = tmpdir("init-reqs");
        run(&root).expect("init");
        let dir = root.join(crate::layout::REQUIREMENTS);
        let files = crate::layout::requirement_sources(&dir);
        assert_eq!(files.len(), 1, "one file to start: {files:?}");

        // And a second file in a subdirectory is picked up with no binding change,
        // which is the reason the binding points at a directory at all.
        std::fs::create_dir_all(dir.join("editor")).expect("dirs");
        std::fs::write(dir.join("editor/tabs.md"), "| `E-1` | tabs |\n").expect("write");
        let text = crate::layout::requirements_text(&dir);
        assert!(text.contains("`R-1`"), "the first file is still read");
        assert!(text.contains("`E-1`"), "and so is the one in a subdirectory");
    }
}
