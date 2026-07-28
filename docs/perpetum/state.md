# Perpetum state

Cycle: 1 · Phase: D · Batch: 2 of 5 delivered · Updated: 2026-07-28

Binding: [`binding.md`](binding.md) · Journal: [`journal.md`](journal.md) +
`journal.jsonl` · Requirements: [`../perpetum.md`](../perpetum.md) ·
Batches: [`../prioritization/batches.md`](../prioritization/batches.md) ·
Board: [`progress-board.md`](progress-board.md)

Hand-written this cycle. The engine renders a projection (`perp state`) but
cannot yet represent parked conflicts or gated items, so `out.state` is not
pointed here until batch 3.

## Position

- Batch 1 — **the spine** — closed: 7 of 9 delivered, 2 carried.
- Batch 2 — **gate runner and evidence** — closed: 4 of 6 delivered, 2 carried.
- Next: **batch 3 — recovery and watchdogs** (`L-5` `L-6` `L-7` `L-11` `L-12`
  `L-13` `L-15` `N-1` `N-2`).

Carried, not done:

| id | Why |
|---|---|
| `L-4` | The projection exists; nothing calls it after each outcome until the loop engine lands (batch 3). |
| `L-8` | An invariant to re-assert every batch, not a feature that finishes. |
| `V-2` | Transcript is complete except the commit sha; `GateResult.sha` is `None` until `G-6` (batch 4). |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — see gated, below. |

## Last green gates

Run by the harness itself: `perp gate all --root .. --step c1/b2/s06`, exit 0.

- lint: `cargo clippy --workspace --all-targets -- -D warnings` · 2026-07-28 · exit 0 · clean
- build: `cargo build --workspace` · 2026-07-28 · exit 0 · clean
- tests: `cargo test --workspace` · 2026-07-28 · exit 0 · 71/71 passed

Transcripts in `journal.jsonl` at `c1/b2/s06`, written by `perp gate`.

## Blocked

*(none)*

## Gated (carried, not counted as done)

- `X-4` — **approval-gated**. The stronger half of the requirement (children die
  even if the engine is killed) needs a Windows job object, which needs the
  `windows-sys` crate. Adding a dependency is approval-gated under
  [`binding.md`](binding.md). The dependency-free tree kill is in and tested;
  this asks only for the upgrade.

## Parked conflicts (asked, unanswered)

- `I-3` — contradicts vision clause 6, *"Never an IDE. JustCode stays a small
  editor."* Asked 2026-07-28. Recommendation: option A (read-only panel, reuse
  the editor's existing surfaces).
- `O-6` — contradicts vision clause 10, *"Never instruction-taking from
  content."* Asked 2026-07-28. Recommendation: option A (outbound notifications
  plus `/btw` replies only; approvals stay on the machine).

## Queued `/btw`

*(none — `C-11` is not built; the operator channel in cycle 1 is the chat
session itself)*

## Cycle history

| Cycle | Batches | Features | Tests added | Version |
|---|---|---|---|---|
| 1 | 2 of 5 delivered | 11 | 71 | 0.1.0 (unreleased) |
