# Perpetum state

Cycle: 1 · Phase: D · Batch: 4 of 5 delivered · Updated: 2026-07-28

Binding: [`binding.md`](binding.md) · Journal: [`journal.md`](journal.md) +
`journal.jsonl` · Requirements: [`../perpetum.md`](../perpetum.md) ·
Batches: [`../prioritization/batches.md`](../prioritization/batches.md) ·
Board: [`progress-board.md`](progress-board.md)

Still hand-written. The engine rewrites its own projection after every outcome
(`L-4`), but that projection cannot yet carry parked conflicts, gated items or
the cycle history below, so `out.state` points at a file the operator maintains
until batch 10. A deliberate gap, not an oversight.

## Position

- Batch 1 — **the spine** — 7 of 9, 2 carried.
- Batch 2 — **gate runner and evidence** — 4 of 6, 2 carried.
- Batch 3 — **recovery and watchdogs** — 10 of 10 (including `L-4`).
- Batch 4 — **the git harness** — 7 of 8, 1 conflicting (plus `V-2` closed).
- Next: **batch 5 — the honesty machinery** (`V-1` `V-3` `V-4` `V-7` `V-8`
  `V-9` `G-7` `G-8`). Perpetum's Phase D exit needs five batches; this is the
  last one before Phase E.

Carried, not done:

| id | Why |
|---|---|
| `L-8` | An invariant, re-asserted every batch rather than a feature that finishes. |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — approval-gated, below. |

## Last green gates

Run by the harness itself: `perp gate all --root .. --step c1/b4/s09`, exit 0,
pinned to `3c2d8f6336cd3700dffb7fe54957aea4d330aa88`.

- lint: `cargo clippy --workspace --all-targets -- -D warnings` · exit 0 · clean
- build: `cargo build --workspace` · exit 0 · clean
- tests: `cargo test --workspace` · exit 0 · 109/109 passed

## Blocked

*(none. Two gate failures this cycle, both fixed inside the two attempts
Perpetum 0.5 allows — `c1/b3/s07` and `c1/b4/s05`–`s06`.)*

## Gated (carried, not counted as done)

- `X-4` — **approval-gated**. The stronger half (children die even if the engine
  is killed) needs a Windows job object, which needs the `windows-sys` crate.
  Adding a dependency is approval-gated under [`binding.md`](binding.md).

## Parked conflicts (asked, unanswered)

- `I-3` — contradicts vision clause 6, *"Never an IDE."* Recommendation: A.
- `O-6` — contradicts vision clause 10, *"Never instruction-taking from
  content."* Recommendation: A.
- `G-13` — contradicts the binding: it says the harness never commits its own
  state, and the binding puts the journal inside the repository on purpose.
  Found by implementing batch 4. Recommendation: split the requirement — ban
  churn, keep the journal as versioned documentation.

## Queued `/btw`

*(none — `C-11` is not built; the operator channel in cycle 1 is the chat
session itself)*

## Cycle history

| Cycle | Batches | Features | Tests added | Version |
|---|---|---|---|---|
| 1 | 4 of 5 delivered | 29 | 109 | 0.1.0 (unreleased) |
