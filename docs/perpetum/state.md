# Perpetum state

Cycle: 1 · Phase: D · Batch: 3 of 5 delivered · Updated: 2026-07-28

Binding: [`binding.md`](binding.md) · Journal: [`journal.md`](journal.md) +
`journal.jsonl` · Requirements: [`../perpetum.md`](../perpetum.md) ·
Batches: [`../prioritization/batches.md`](../prioritization/batches.md) ·
Board: [`progress-board.md`](progress-board.md)

Still hand-written. The engine now *can* own this file — `StepGuard::close`
rewrites it from the journal after every outcome (`L-4`) — but its projection
cannot yet carry parked conflicts, gated items or the cycle history below, so
`out.state` is left pointing at a file the operator maintains until batch 10.
That is a deliberate gap, not an oversight.

## Position

- Batch 1 — **the spine** — closed: 7 of 9 delivered, 2 carried.
- Batch 2 — **gate runner and evidence** — closed: 4 of 6 delivered, 2 carried.
- Batch 3 — **recovery and watchdogs** — closed: 10 of 10, including `L-4` from
  batch 1.
- Next: **batch 4 — the git harness** (`G-1` `G-2` `G-3` `G-4` `G-6` `G-10`
  `G-13` `G-14`).

Carried, not done:

| id | Why |
|---|---|
| `L-8` | An invariant, re-asserted every batch rather than a feature that finishes. |
| `V-2` | Transcript is complete except the commit sha; `GateResult.sha` is `None` until `G-6` in batch 4. |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — see gated, below. |

## Last green gates

Run by the harness itself: `perp gate all --root .. --step c1/b3/s08`, exit 0.

- lint: `cargo clippy --workspace --all-targets -- -D warnings` · 2026-07-28 · exit 0 · clean
- build: `cargo build --workspace` · 2026-07-28 · exit 0 · clean
- tests: `cargo test --workspace` · 2026-07-28 · exit 0 · 95/95 passed

## Blocked

*(none)*

## Gated (carried, not counted as done)

- `X-4` — **approval-gated**. The stronger half (children die even if the engine
  is killed) needs a Windows job object, which needs the `windows-sys` crate.
  Adding a dependency is approval-gated under [`binding.md`](binding.md). The
  dependency-free tree kill is in and tested; this asks only for the upgrade.

## Parked conflicts (asked, unanswered)

- `I-3` — contradicts vision clause 6, *"Never an IDE. JustCode stays a small
  editor."* Asked 2026-07-28. Recommendation: option A.
- `O-6` — contradicts vision clause 10, *"Never instruction-taking from
  content."* Asked 2026-07-28. Recommendation: option A.

## Queued `/btw`

*(none — `C-11` is not built; the operator channel in cycle 1 is the chat
session itself)*

## Cycle history

| Cycle | Batches | Features | Tests added | Version |
|---|---|---|---|---|
| 1 | 3 of 5 delivered | 21 | 95 | 0.1.0 (unreleased) |
