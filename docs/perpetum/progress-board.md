# Progress board

Fields per Perpetum Appendix 2. Regenerated at every feature's step 7; the
rendered view is published as an artifact, and this file is what survives the
session.

**Generated:** 2026-07-28 · **Cycle:** 1 · **Phase:** D · **Version:** 0.1.0 (unreleased)

## Batch

**2 of 10 — gate runner and evidence** (closed)

| Field | Value |
|---|---|
| Current feature | — (batch closed; next is batch 3, recovery and watchdogs) |
| Previous 3 features | `L-16` attempt ceiling · `X-12` declared environment · `T-4` nursery |
| Delivered this batch | 4 of 6 — `T-3` `T-4` `X-12` `L-16` |
| Carried | `V-2` (no commit sha until `G-6`), `X-4` (job object is approval-gated) |
| Blocked | 0 |
| Gated | 1 — `X-4`, approval-gated |

## Gates — last run 2026-07-28 by `perp gate all`, all green

| Gate | Command | Exit | Result |
|---|---|---|---|
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| build | `cargo build --workspace` | 0 | clean |
| test | `cargo test --workspace` | 0 | 71 passed / 0 failed |

Tests: **71 passed, 0 failed** — 66 unit, 5 integration. Eight have been
red-run against mutated implementations across batches 1–2; the other 63 have
not.

## Cycle totals

| | |
|---|---|
| Requirements | 146 |
| Done | 11 |
| In progress | 4 — `L-4` `L-8` `V-2` `X-4` |
| Batched, not started | 70 |
| Unbatched | 61 |
| Conflicting, parked | 2 — `I-3`, `O-6` |
| Gated | 1 — `X-4` |
| Commits | 3, on `perp/c1/{init,b1,b2}`. Nothing pushed. |

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
