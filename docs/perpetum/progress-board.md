# Progress board

Fields per Perpetum Appendix 2. Regenerated at every feature's step 7; the
rendered view is published as an artifact, and this file is what survives the
session.

**Generated:** 2026-07-28 · **Cycle:** 1 · **Phase:** D complete → E ·
**Version:** 0.1.0 (unreleased)

## Batch

**5 of 10 — the honesty machinery** (closed). Phase D's exit condition is met.

| Field | Value |
|---|---|
| Current feature | — (Phase D complete; next is Phase E, release) |
| Previous 3 features | `V-9` stray-id check · `V-8` gated never counts as done · `V-7` markers from evidence |
| Delivered this batch | 8 of 8 — `V-1` `V-3` `V-4` `V-7` `V-8` `V-9` `G-7` `G-8` |
| Blocked | 0 |
| Gated | 1 — `X-4`, approval-gated |
| Discovered | `T-18` — filed, not built |

## Gates — last run 2026-07-28 by `perp gate all`, all green

Pinned to `ffe5618513a6bb1b3bc4a8e2444276684ee7611a`.

| Gate | Command | Exit | Result |
|---|---|---|---|
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| build | `cargo build --workspace` | 0 | clean |
| test | `cargo test --workspace` | 0 | 128 passed / 0 failed |
| ids | `perp check ids` | 0 | 147 defined, 18 documents, no strays |

Tests: **128 passed, 0 failed** — 123 unit, 5 integration. Twenty-six have been
red-run against mutated implementations across five batches. Three mutations
were inconclusive and were re-run properly rather than counted.

## Cycle totals

| | |
|---|---|
| Requirements | 148 (141 designed, 7 minted while building) |
| Done | 38 |
| In progress | 2 — `L-8`, `X-4` |
| Conflicting, parked | 3 — `I-3`, `O-6`, `G-13` |
| Gated | 1 — `X-4` |
| Batched, not started | 45 |
| Unbatched | 59 |
| Commits | 6, on `perp/c1/{init,b1,b2,b3,b4,b5}`. Nothing pushed. |

## Minted while building, not while designing

Seven requirements exist because the loop ran, not because anyone sat down to
think of them:

| id | Came from |
|---|---|
| `N-9` `N-10` `N-11` | the NFR pass in Phase B |
| `L-21` `L-22` | reconciling the backlog against the code in C.1 |
| `V-10` | a red run that reported green after its mutation silently failed to apply |
| `T-18` | the harness locking the executable its own build gate had to replace |

## Five gate failures, all recorded

Perpetum 0.5 says never fake a green. Every one is in the journal with its
verbatim output:

| Step | Gate | What it was |
|---|---|---|
| `c1/b1/s10` | lint | clippy's in-test exemption misses helpers in `tests/` |
| `c1/b3/s07` | test | the **test** was wrong — A→B→A→B is two reverts |
| `c1/b4/s05` | test | real bug: combined short flags walked past the classifier |
| `c1/b4/s06` | test | `rev-parse HEAD` cannot answer before the first commit |
| `c1/b5/s07` | build | the harness held a lock on the exe it was rebuilding |

Two bugs in code, two wrong expectations in tests, one bootstrap defect. None
blocked; all fixed inside the two attempts Perpetum 0.5 allows.

## Outstanding, and not claimed

Perpetum D also asks for end-to-end tests over the five batches' features.
`tests/spine.rs` covers the spine; the gate runner, git harness and verification
layer have unit coverage but nothing that drives `perp` as a subprocess. Carried
into Phase E as the first thing to fix.

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
