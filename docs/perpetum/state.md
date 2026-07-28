# Perpetum state

Cycle: 1 · Phase: D · Batch: 1 of 5 delivered · Updated: 2026-07-28

Binding: [`binding.md`](binding.md) · Journal: [`journal.md`](journal.md) +
`journal.jsonl` · Requirements: [`../perpetum.md`](../perpetum.md) ·
Batches: [`../prioritization/batches.md`](../prioritization/batches.md) ·
Board: [`progress-board.md`](progress-board.md)

Hand-written this cycle. The engine can render a projection (`perp state`) but
cannot yet represent parked conflicts or gated items, so `out.state` is not
pointed here until batch 3.

## Position

- Batch 1 — **the spine** — closed: 7 of 9 delivered, 2 carried, 0 blocked.
- Next: **batch 2 — gate runner and evidence** (`V-2` `T-3` `L-16` `X-4` `X-12` `T-4`).
- Carried into batch 3, not done: `L-4` (projection needs the loop to call it
  after each outcome), `L-8` (an invariant, re-asserted every batch).

## Last green gates

- lint: `cargo clippy --workspace --all-targets -- -D warnings` · 2026-07-28 · exit 0 · clean
- build: `cargo build --workspace` · 2026-07-28 · exit 0 · clean
- tests: `cargo test --workspace` · 2026-07-28 · exit 0 · 53/53 passed

Transcripts in [`journal.md`](journal.md) at `c1/b1/s11`.

## Blocked

*(none)*

## Gated (carried, not counted as done)

*(none)*

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
| 1 | 1 of 5 delivered | 7 | 53 | 0.1.0 (unreleased) |
