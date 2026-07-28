# Progress board

Fields per Perpetum Appendix 2. Regenerated at every feature's step 7; the
rendered view is published as an artifact, and this file is what survives the
session.

**Generated:** 2026-07-28 · **Cycle:** 1 · **Phase:** D · **Version:** 0.1.0 (unreleased)

## Batch

**3 of 10 — recovery and watchdogs** (closed)

| Field | Value |
|---|---|
| Current feature | — (batch closed; next is batch 4, the git harness) |
| Previous 3 features | `L-4` projection after each outcome · `L-15` clean stop · `L-7` reconcile |
| Delivered this batch | 10 of 10 — `L-5` `L-6` `L-7` `L-11` `L-12` `L-13` `L-15` `N-1` `N-2` `L-4` |
| Carried | `L-8` (invariant), `V-2` (needs `G-6`), `X-4` (approval-gated) |
| Blocked | 0 |
| Gated | 1 — `X-4`, approval-gated |

## Gates — last run 2026-07-28 by `perp gate all`, all green

| Gate | Command | Exit | Result |
|---|---|---|---|
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| build | `cargo build --workspace` | 0 | clean |
| test | `cargo test --workspace` | 0 | 95 passed / 0 failed |

Tests: **95 passed, 0 failed** — 90 unit, 5 integration. Fifteen have been
red-run across batches 1–3. Two of them survived their first mutation and were
strengthened; one mutation turned out not to have applied at all, which is now
filed as `V-10`.

## Cycle totals

| | |
|---|---|
| Requirements | 147 (146 + `V-10`, minted in batch 3) |
| Done | 21 |
| In progress | 3 — `L-8` `V-2` `X-4` |
| Batched, not started | 61 |
| Unbatched | 62 |
| Conflicting, parked | 2 — `I-3`, `O-6` |
| Gated | 1 — `X-4` |
| Commits | 4, on `perp/c1/{init,b1,b2,b3}`. Nothing pushed. |

## Two failures worth keeping

Perpetum 0.5 says never fake a green. Both of this cycle's gate failures are in
the journal with their verbatim output:

- `c1/b1/s10` — lint, exit 101: clippy's in-test exemption does not reach helper
  functions in `tests/`. Fixed on attempt 1 of 2.
- `c1/b3/s07` — test, exit 101: the thrash watchdog stopped a step earlier than
  the test expected. The **test** was wrong; `A→B→A→B` is two reverts, not one.

## Sources, this pass

| Source | Weight | Status |
|---|---|---|
| Crash analytics | 100 | unavailable — nothing has ever run |
| Security scanners | 95 | empty — the harness still has zero dependencies |
| Support | 45 | unavailable — no users |
| User voice | 40 | partial — operator read; GitHub issues unreachable |
| Product backlog | 30 | read in full |
| NFRs | 20 | read — minted `N-9`, `N-10`, `N-11` |
| Analytics | 10 | unavailable |
| Market | 5 | read |
