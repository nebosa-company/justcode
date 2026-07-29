# Progress board

Fields per Perpetum Appendix 2. Regenerated at every feature's step 7; the
rendered view is published as an artifact, and this file is what survives the
session.

**Generated:** 2026-07-29 · **Cycle:** 2 · **Phase:** F · **Version:** 0.2.0 (unreleased)

> **This file went stale and the reconcile caught it.** It was last written at
> `c1/b5`, six batches ago. The published artifact was updated every batch; this
> was not. That is the exact failure Perpetum 0.7 describes — the visible thing
> gets maintained and the recorded thing rots — and it happened because both are
> hand-written until `A-2`/`A-3` are built and generate them from the journal.
> Recorded rather than quietly fixed, because a board that silently caught up
> would teach nobody anything.

## Where the loop is

| | |
|---|---|
| Cycle 1 | closed — phases A–F, 5 batches, 38 requirements, 128 tests |
| Cycle 2 | phases B–E done, F in progress — 5 batches, 26 requirements, 237 tests |
| Current feature | — (cycle closing; next is Phase B for cycle 3) |
| Previous 3 features | `S-8` filed in the security review · `M-25` measured and gated · `M-20` park-not-promote |
| Blocked | 0 |
| Gated | 2 — `X-4` approval-gated, `M-25` external-gated |

## Gates — last run 2026-07-28 by `perp gate all`, all green

Pinned to `a2319be4bac515a11c5954921180c0520be8e3f9`.

| Gate | Command | Exit | Result |
|---|---|---|---|
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| build | `cargo build --workspace` | 0 | clean |
| test | `cargo test --workspace` | 0 | 237 passed / 0 failed |
| ids | `perp check ids` | 0 | 152 defined, 29 documents, no strays |

Tests: **237 passed, 0 failed** — 217 unit, 5 spine integration, 15 end-to-end.
Fifty-eight have been red-run against mutated implementations across ten
batches.

## Requirements

| State | Count |
|---|---|
| ✅ done | 64 |
| 🟡 in progress | 4 — `N-6` `M-21` `M-23` `X-4` |
| ⛔ gated | 1 — `M-25` external-gated |
| 🔶 conflicting | 3 — `I-3` `O-6` `G-13` |
| open | 80 |
| **total** | **152** |

Counted from the file, not from the last report.

## Eleven requirements that only exist because the loop ran

| id | Came from |
|---|---|
| `N-9` `N-10` `N-11` | the NFR pass, cycle 1 Phase B |
| `L-21` `L-22` | reconciling the backlog against the code, cycle 1 C.1 |
| `V-10` | a red run that reported green after its mutation silently failed to apply |
| `T-18` | the harness locking the executable its own build gate had to rebuild |
| `N-12` | cycle 1's Phase D exit obligation, filed rather than carried as a footnote |
| `M-24` | a declared credential failing at call time instead of bind time |
| `M-25` | measuring that LM Link peers are unreachable from outside LM Studio |
| `S-8` | the Phase E security review: request bodies in a shared temp directory |

## Eight gate failures across two cycles, none hidden

| Step | Gate | What it was |
|---|---|---|
| `c1/b1/s10` | lint | clippy's in-test exemption misses helpers in `tests/` |
| `c1/b3/s07` | test | the test was wrong — A→B→A→B is two reverts |
| `c1/b4/s05` | test | real bug: combined short flags walked past the git classifier |
| `c1/b4/s06` | test | `rev-parse HEAD` cannot answer before the first commit |
| `c1/b5/s07` | build | the harness held a lock on the exe it was rebuilding |
| `c2/b8/s06` | test | the probe cache was consulted *after* the fetch it was meant to avoid |
| `c2/b9/s05` | test | the cost report rounded every real amount to `0.0000` |
| `c2/b10/s06` | test | a multi-line literal carried its indentation into an error message |

Batches 6 and 7 had none. Five in cycle 1, three in cycle 2, all fixed inside
the two attempts Perpetum 0.5 allows.

## Sources, cycle 2

| Source | Weight | Status |
|---|---|---|
| Crash analytics | 100 | unavailable — nothing has ever run |
| Security scanners | 95 | **read** — zero deps, zero unsafe, zero panics; one finding at Phase E (`S-8`) |
| Support | 45 | unavailable — no users |
| User voice | 40 | partial — operator read; GitHub issues still unreachable |
| Product backlog | 30 | read in full |
| NFRs | 20 | read — minted `N-12` |
| Analytics | 10 | unavailable |
| Market | 5 | not re-read — same day as cycle 1's pass. **Due now**, the day having changed |
