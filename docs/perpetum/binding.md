# Perpetum binding — JustCode / `perp` harness

Maps every path and command [Perpetum](../../../perpetum.md) names to what this
repository actually uses (Perpetum 0.1). If a path is not answered here, the
loop stops and asks rather than guessing.

Bound: 2026-07-28 · Cycle 1.

## Who is running the loop

**Cycle 1's operator is a Claude Code session**, executing Perpetum by hand
against this binding. The product being built — `perp` — is the engine that will
take the loop over. Until then, every rule that says "the engine enforces X"
is enforced by the operator, and the same evidence is required either way.

This is circular and worth stating plainly: the harness that checks work is
being built by a process that is not yet checked by it. The mitigation is that
every gate in this cycle is a real command with a real transcript in
[`journal.md`](journal.md), so the bootstrap is auditable after the fact.

## Paths

| Perpetum names | This repo uses | Notes |
|---|---|---|
| Vision | [`docs/initiation/vision.md`](../initiation/vision.md) | |
| NFRs | [`docs/initiation/nfrs.md`](../initiation/nfrs.md) | |
| Requirements source | [`docs/perpetum.md`](../perpetum.md) | **The only place ids are minted** (Perpetum 0.8, `V-9`) |
| Per-source inputs | `docs/requirements/<source>/` | one folder per Phase B source |
| Source-trust weights | `docs/requirements/weights.md` | |
| Impact weights | `docs/prioritization/weights.md` | separate file, separate meaning |
| Batches | `docs/prioritization/batches.md` | |
| Conflicts | `docs/prioritization/conflicts.md` | |
| State | [`docs/perpetum/state.md`](state.md) | **generated** — `perp run` rewrites it after every step (`L-4`) |
| Cycle notes | [`docs/perpetum/cycle-notes.md`](cycle-notes.md) | hand-written narrative; not read by the engine |
| Journal | [`docs/perpetum/journal.md`](journal.md) | steps + gate transcripts; the truth for cycle 1 |
| Progress board | `docs/perpetum/progress-board.md` | rewritten at every feature's step 7 |
| Release notes | `docs/perpetum/release-notes.md` | the harness's own, separate from the editor's |
| Deploy runbook | `docs/maintain/deploy.md` | created in Phase E if absent |

## The machine-readable binding

The block below is the binding the engine actually reads (`L-21`). The tables in
this document are commentary on it; this is the source. One file, one truth —
a separate config file would be a second place for these paths to be wrong.

`path.*` are **inputs** and must exist, or the loop stops and asks. `out.*` are
**written** by the engine; only their parent directory must exist.

```perp-binding
path.requirements = docs/perpetum.md
path.vision       = docs/initiation/vision.md
path.nfrs         = docs/initiation/nfrs.md
path.batches      = docs/prioritization/batches.md
path.conflicts    = docs/prioritization/conflicts.md
path.weights.source = docs/requirements/weights.md
path.weights.impact = docs/prioritization/weights.md
path.links          = docs/perpetum/links.md

out.journal = docs/perpetum/journal.jsonl
out.state   = docs/perpetum/state.md
out.board   = docs/perpetum/progress-board.md

gate.cwd     = crates
gate.timeout = 900
gate.lint    = cargo clippy --workspace --all-targets -- -D warnings
gate.build   = cargo build --workspace
gate.test    = cargo test --workspace

budget.cycle.money   = 5.00
budget.cycle.seconds = 28800
budget.batch.money   = 1.00
budget.batch.seconds = 5400

git.branch.batch = perp/c{cycle}/b{batch}
git.branch.init  = perp/c{cycle}/init
git.push         = approval
```

The budgets are the ceilings an unattended run stops at (`L-9`). Money is
counted from what the links actually reported, never estimated (`L-10`), so a
`local-only` cycle spends nothing against the money lines and everything against
the wall-clock ones — which is why both exist. **No token limit is declared**:
tokens are the currency this project has no calibration for yet, and inventing a
number would produce a ceiling that stops good runs and permits bad ones. It
goes in once a cycle has run long enough to say what normal looks like.

An eight-hour cycle and a dollar is deliberately an overnight run, not a
weekend: the design target in the vision is *a bill under a dollar* by morning.

Two journals, deliberately: [`journal.md`](journal.md) is **cycle 1's operator
journal**, written by hand while the engine is being built, and
`journal.jsonl` is what the engine writes from batch 1 onward. When the engine
takes the loop over, the markdown journal stops growing and stands as the record
of the bootstrap.

## Where new requirements may be written

**Yes — new ids are appended to `docs/perpetum.md`**, in the section table that
owns their prefix. Prefixes are fixed: `L` loop, `M` model links, `T` tools,
`G` git, `X` OS, `V` verification, `C` chat, `A` artifacts, `O` observability,
`I` JustCode integration, `S` security, `N` non-functional.

Ids are never reused and never renumbered. Everything else — batches, board,
state, journal — **cites** ids and may not introduce them.

## Code layout

| | Path | Contents |
|---|---|---|
| Harness | `crates/` | its own cargo workspace: `perp-core` (library), `perp` (CLI) |
| Editor | `src/`, `src-tauri/` | unchanged by the harness; JustCode must build with `crates/` deleted |

`crates/` is a **separate cargo workspace** from `src-tauri/`, deliberately: the
editor's build must not gain the harness's dependencies, and `cargo tauri build`
must behave exactly as it did before this work started.

## Gate commands

Harness gates — working directory `crates/`:

| Gate | Command |
|---|---|
| lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| build | `cargo build --workspace` |
| test | `cargo test --workspace` |

Editor gates — working directory repo root, run only when `src/`, `src-tauri/`,
`index.html` or `package.json` change:

| Gate | Command |
|---|---|
| build | `npm run build` |
| check | `npm run check:associations` |

Environment: no network required for any gate. Cargo must resolve from a warm
registry cache; a cold-cache failure blocks the batch (NFR 10) rather than
quietly changing the dependency plan.

If `cargo clippy` is unavailable on the machine, the lint gate is **failed, not
skipped**, and the batch is blocked with that as the reason.

## Status markers

Written in `docs/perpetum.md` next to the requirement id:

| Marker | Meaning |
|---|---|
| *(none)* | open — not started |
| 🟡 | in progress |
| ✅ + `~~id~~` | done: implemented, all gates green, transcript in the journal |
| 🚧 | blocked (Perpetum 0.5) — carries the verbatim error |
| 🔶 | conflicting (Perpetum C.2) — parked in `conflicts.md` with the vision or architecture line it contradicts |
| ⛔ | gated (Perpetum 0.6) — carries which kind and what is missing |
| ❌ | won't do — carries the reason |

A marker without a matching journal entry is not believed (Perpetum 0.7). The
reconcile step in C.1 checks markers against the code, not against each other.

## Git ritual

- Branches: `perp/c<cycle>/b<batch>` for batch work, `perp/c<cycle>/init` for
  phases A–C. `main` is never committed to directly (`G-1`).
- Commit subject follows this repo's existing style: **imperative, sentence
  case, no type prefix** ("Add the journal writer", not "feat(core): ...").
- Commit trailers, required on every harness commit:

  ```
  Requirement: L-3, L-4
  Perpetum-Step: c1/b1/s07
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```

  The `Co-Authored-By` line is also the `M-10`/`G-2` record of which model
  authored the change.
- Staging is explicit — the files the step touched. `git add -A` and
  `git commit -a` are forbidden (`G-3`).
- Hooks are never bypassed. `--no-verify` is forbidden (`G-4`).
- **Push, tag, merge to `main`, and PR creation are approval-gated** (`G-5`).
  Cycle 1 pushes nothing.

## Release ritual

The harness versions independently of the editor. JustCode's own release
process is unchanged and documented in [`RELEASING.md`](../RELEASING.md).

1. Version lives in `crates/perp-core/Cargo.toml` and `crates/perp/Cargo.toml`
   and must agree. Starts at `0.1.0`.
2. Release notes are appended to `docs/perpetum/release-notes.md`, with API
   breaking changes called out explicitly.
3. No tag, no push, no GitHub release for the harness until it is shipped with
   the editor — that is an approval decision, not a loop decision.

## Approval boundary for this project

Beyond Perpetum 0.4's standing list, these stop and ask here:

- Any change under `src/` or `src-tauri/` — the editor is a shipped product and
  the harness may not touch it unattended.
- Adding a third-party crate dependency.
- Anything that would run a model against a paid endpoint (cycle 1 is
  `local-only` by default; there is no model call in the loop yet).
- Merging any `perp/**` branch into `main`.
