# Batches — cycle 3

**The goal this cycle is stated in one line: stop being the thing that types
`perp gate`.** Everything below is ordered by how close it gets the harness to
running Perpetum itself.

88 requirements remained at the start of this cycle. This plan covers all of
them, in fifteen batches — three cycles' worth at Perpetum's five-per-cycle.
Batches 11–15 are this cycle; 16–25 are named here so the shape is visible, and
compete again in cycles 4 and 5.

Requirements source: [`../perpetum.md`](../perpetum.md) — 152, of which 64 done.

## What the operator decided, 2026-07-29

Three conflicts parked since cycle 1, and one dependency, all answered:

| | Decision | Consequence |
|---|---|---|
| `I-3` | **Full panel as specified** | Chat, approvals, diff review, artifacts and timeline all live in JustCode. Vision clause 6 was rewritten in the same breath rather than quietly violated. |
| `O-6` | **Outbound only; `/btw` the sole return path** | Approvals never arrive over the network. The authentication problem is removed rather than solved. |
| `G-13` | **Split** | Churn is never committed; the journal, state, board and artifacts are documentation and deliberately versioned. |
| `X-4` | **`windows-sys` approved** | A real Windows job object, so children die even if the engine is killed. Nothing else may add a crate without another approval. |

`M-25` stays **⛔ external-gated** and is in no batch. Inference on an LM Link
peer is unreachable from outside LM Studio — measured, not assumed, in
`c2/b10/s01`. It needs the LM Studio SDK or a change on their side.

## The critical path to self-running

Two batches. Everything else in the backlog is either downstream of them or
independent of them.

| # | Theme | Members | Why here |
|---|---|---|---|
| 11 | **The tool host and the permission classifier** | `T-1` `T-2` `T-5` `T-6` `T-7` `T-12` `T-13` `T-14` `T-15` `T-16` `T-17` `L-19` | A loop that cannot read, edit or run anything is a scheduler. The classifier comes with it, not after it: an unattended loop that can act before it can refuse is the one shape this design must never ship. |
| 12 | **The loop driver** | `L-1` `L-2` `L-9` `L-10` `L-14` `L-17` `L-18` `L-20` `M-8` | The phase machine, budgets and the step scheduler. At the end of this batch `perp run` executes a batch on its own. `M-8` joins it because the degradation ladder is meaningless until something issues tool calls. |

**After batch 12 the harness runs itself**, and cycles 4–5 are the loop building
the rest of itself under its own gates — which is the first point at which any
of this is worth the name.

## The rest of cycle 3

| # | Theme | Members | Effort |
|---|---|---|---|
| 13 | Chat, slash commands and `/btw` | `C-1`–`C-12` | L |
| 14 | Artifacts and the board | `A-1`–`A-7` `O-2` `O-5` `O-7` | M |
| 15 | Security enforcement | `S-1` `S-3` `S-4` `S-5` `S-6` `S-7` `S-8` `N-6` | L |

Batch 15 is where `S-8` — the finding from cycle 2's release review — gets
fixed, alongside the egress allowlist and redaction that were designed in cycle
1 and never built. Security enforcement waits until batch 15 deliberately: it is
enforcement *of* the tool host and the transport, and neither existed until now.

## Cycles 4 and 5

Named so the plan is complete rather than open-ended.

| # | Theme | Members |
|---|---|---|
| 16 | OS integration | `X-1` `X-2` `X-3` `X-5` `X-6` `X-7` `X-8` `X-9` `X-10` |
| 17 | Live control and rewind | `O-3` `O-4` `O-6` |
| 18 | The JustCode panel | `I-1` `I-2` `I-3` `I-4` `I-5` |
| 19 | Runtime plugins | `T-8` `T-9` `T-10` `T-11` `T-18` |
| 20 | The remaining model layer | `M-21` `M-23` `M-24` `M-13` |
| 21 | Independent verification | `V-5` `V-6` |
| 22 | Git completion | `G-5` `G-9` `G-11` `G-12` |
| 23 | Non-functional closure | `N-4` `N-7` `N-8` `L-8` |
| 24 | Local-host realities | `X-4` `X-11` `X-12` |
| 25 | Whatever cycles 3–4 mint | — |

Batch 25 exists because **eleven requirements were minted while building** across
two cycles, and none of them were foreseeable from a document. Planning fifteen
batches and pretending nothing new appears would be the same optimism Perpetum
C.4 warns about in effort estimates.

## Where score and coherence disagree

They barely do this cycle, because the operator's answer settled it: *make it
self-running first*. Scored strictly, artifacts (batch 14, effort M) outranks
the tool host (batch 11, effort XL) by an order of magnitude — and building a
progress board for a loop that cannot run is the same mistake cycle 1 declined
to make with the journal.

The one place the score is followed against instinct: **security enforcement is
batch 15, not batch 11.** It is tempting to put `S-1`–`S-8` first because they
are about safety. They are enforcement of surfaces that do not exist yet, and a
policy written against an imaginary tool host is a policy that will be wrong.
