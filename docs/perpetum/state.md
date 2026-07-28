# Perpetum state

Cycle: 1 · **closed** · Phase: F complete → B (cycle 2) · Updated: 2026-07-28

Binding: [`binding.md`](binding.md) · Journal: [`journal.md`](journal.md) +
`journal.jsonl` · Requirements: [`../perpetum.md`](../perpetum.md) ·
Batches: [`../prioritization/batches.md`](../prioritization/batches.md) ·
Board: [`progress-board.md`](progress-board.md)

Still hand-written; the engine rewrites its own projection after every outcome
(`L-4`) but cannot yet carry parked conflicts, gated items or cycle history.
Deliberate, and it closes in batch 10.

## Position

| Batch | Theme | Delivered |
|---|---|---|
| 1 | the spine | 7 of 9, 2 carried |
| 2 | gate runner and evidence | 4 of 6, 2 carried |
| 3 | recovery and watchdogs | 10 of 10 |
| 4 | the git harness | 7 of 8, 1 conflicting |
| 5 | the honesty machinery | 8 of 8 |

**Phase D's exit condition is met:** 5 batches delivered, full suite green,
38 requirements done.

Not met, and not claimed: Perpetum D also asks for end-to-end tests covering
what the five batches shipped. `crates/perp-core/tests/spine.rs` covers the
spine end to end; the gate runner, git harness and verification layer have unit
coverage but no end-to-end test that drives `perp` as a subprocess. **That is
outstanding work, carried into Phase E as the first thing to fix.**

**Phase E** shipped 0.1.0 to the approval boundary — release notes written,
security surface reviewed (zero dependencies; `cargo-audit` not installed and
not run), deploy and customer notification parked unattended. Nothing pushed.

**Phase F** reconciled the markers against the file, moved batches 1–5 out of
the active list, and recorded the cycle's numbers.

- Next: **Phase B — cycle 2**, gathering requirements again. Batches 6 and 7
  (the model layer) compete first: the harness still cannot talk to a model.

Carried, not done:

| id | Why |
|---|---|
| `L-8` | An invariant, re-asserted every batch rather than a feature that finishes. |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — approval-gated, below. |
| `T-18` | Filed this batch, not built: the engine must not hold a lock on artefacts its own gates rebuild. |

## Last green gates

`perp gate all --root .. --step c1/b5/s08`, exit 0, pinned to
`ffe5618513a6bb1b3bc4a8e2444276684ee7611a`.

- lint: `cargo clippy --workspace --all-targets -- -D warnings` · exit 0 · clean
- build: `cargo build --workspace` · exit 0 · clean
- tests: `cargo test --workspace` · exit 0 · 128/128 passed
- ids: `perp check ids` · exit 0 · 147 defined, 18 documents, no strays

## Blocked

*(none. Four gate failures this cycle — `c1/b1/s10`, `c1/b3/s07`, `c1/b4/s05`,
`c1/b4/s06`, `c1/b5/s07` — all fixed inside the two attempts Perpetum 0.5
allows. Two were bugs in the code, two were wrong expectations in tests, and one
was the harness standing on the file it was rebuilding.)*

## Gated (carried, not counted as done)

- `X-4` — **approval-gated**. A Windows job object needs the `windows-sys`
  crate; adding a dependency is approval-gated under [`binding.md`](binding.md).

## Parked conflicts (asked, unanswered)

- `I-3` — vision clause 6, *"Never an IDE."* Recommendation: A.
- `O-6` — vision clause 10, *"Never instruction-taking from content."*
  Recommendation: A.
- `G-13` — contradicts the binding: the journal lives inside the repository on
  purpose. Recommendation: split the requirement.

## Queued `/btw`

*(none — `C-11` is not built; the operator channel in cycle 1 is the chat
session itself)*

## Cycle history

| Cycle | Batches | Features | Tests added | Version |
|---|---|---|---|---|
| 1 | 5 of 5 delivered | 38 | 128 | 0.1.0 (unreleased) |
