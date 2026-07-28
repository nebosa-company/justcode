# Batches — cycle 2

## Delivered (Perpetum F.2)

Batches 6–10 are closed. Phase D's exit needs five; five is what it got, and
this time the end-to-end obligation was met rather than carried.

| # | Theme | Delivered | Carried or parked |
|---|---|---|---|
| 6 | Proving it works | 6 of 7 | `N-6` carried — the *gate runner* makes no network call, but nothing stops a project's gate command |
| 7 | Model links — the router | 5 of 8 | `M-6` `M-7` `M-14` carried to batch 8 and closed there |
| 8 | Transport and protocols | 6 of 8 | `M-21` `M-23` carried; `M-8` untouched, waiting on the tool host |
| 9 | Cost, caching and context | 4 of 4 | — |
| 10 | Local-server realities | 5 of 5 | `M-25` discovered and gated |

**Batches 11–15 below are unchanged and compete again in cycle 3's Phase C**, in
front of a backlog that now carries `M-24`, `S-8` and `T-18` — three defects
found by running the thing rather than by planning it.

The next 10 batches (Perpetum C.7), renumbered 6–15. Cycle 1's unbuilt batches
6–10 competed again from scratch rather than being inherited; four of them
survive, in a different order, and two new ones join.

Requirements source: [`../perpetum.md`](../perpetum.md) — 149 after `N-12`.

## Where score and coherence agree this time

Last cycle they disagreed and coherence won. This cycle they mostly agree, and
where they differ the score wins:

**Proving it works (batch 6) outranks the model layer, and goes first.** It
scores highest (source 40, effort M → 1067), it closes cycle 1's one
outstanding Phase D obligation, and it fixes `T-18` — a defect that costs an
operator an hour before they work out what happened. Building the model layer
on top of an unproven CLI would mean the first end-to-end test ever written has
to cover twice as much.

That reasoning is worth stating because the *interesting* work is batch 7, and
"do the boring thing first" is exactly the call an unattended loop is tempted to
skip.

| # | Theme | Members | Effort | Source | Impact | Score |
|---|---|---|---|---|---|---|
| 6 | Proving it works | `N-12` `T-18` `V-6` `L-8` `N-3` `N-5` `N-6` | M (3) | 40 | 80 | 1067 |
| 7 | Model links — the router | `M-1` `M-2` `M-3` `M-4` `M-5` `M-6` `M-7` `M-14` | L (8) | 60 | 80 | 600 |
| 8 | Model links — transport and protocols | `M-8` `M-9` `M-10` `M-21` `M-22` `M-23` | L (8) | 60 | 80 | 600 |
| 9 | Cost, caching and context | `M-11` `M-12` `M-13` `M-15` | M (3) | 60 | 80 | 1600 |
| 10 | Local-server realities | `M-16` `M-17` `M-18` `M-19` `M-20` | L (8) | 50 | 80 | 500 |
| 11 | Tools and the permission classifier | `T-1` `T-2` `T-5` `T-6` `T-7` `T-12` `T-13` `T-14` `T-15` `L-19` | XL (20) | 40 | 80 | 160 |
| 12 | The loop driver | `L-1` `L-2` `L-9` `L-10` `L-14` `L-17` `L-18` `L-20` | L (8) | 40 | 80 | 400 |
| 13 | Chat, slash commands, `/btw` | `C-1`–`C-6` `C-8`–`C-10` `C-12` | L (8) | 50 | 80 | 500 |
| 14 | Artifacts and the board | `A-1`–`A-7` `O-2` `O-5` `O-7` | M (3) | 60 | 80 | 1600 |
| 15 | OS integration and egress | `X-1` `X-2` `X-3` `X-5` `X-7` `S-1` `S-3` `S-4` | L (8) | 40 | 80 | 400 |

Batch 9 and 14 score 1600 and are ninth and fourteenth for the same reason as
last cycle: there is nothing to account the cost of until something calls a
model, and nothing to render a board from until the loop drives itself.

---

## Batch 6 — Proving it works

**Theme:** the claims this harness makes about itself, asserted by a test rather
than by a paragraph.

| id | What it means here |
|---|---|
| `N-12` | End-to-end tests run the real `perp` binary as a subprocess against a real fixture repository. |
| `T-18` | The engine must not lock artefacts its own gates rebuild — the bug that failed `c1/b5/s07`. |
| `V-6` | Exercise the real artefact once per batch; the E2E suite *is* that, automated. |
| `L-8` | Context is disposable — asserted by driving two separate processes over one journal. |
| `N-3` | Single binary, no daemon: the E2E suite starts nothing but `perp`. |
| `N-5` | Deterministic replay: the same journal renders the same state twice. |
| `N-6` | Gates run without network — asserted by running one with none available. |

**Ordering:** `T-18` first (the harness cannot test itself reliably while it
locks its own binary), then the E2E harness, then the assertions.

## Batch 7 — Model links: the router

**Theme:** the first batch that knows what a model is. Deliberately stops at the
network boundary — everything here is testable with a recorded fixture and no
socket, which is what keeps the suite runnable on a machine with no GPU.

| id | What it means here |
|---|---|
| `M-1` | Four link kinds: `lmstudio`, `lmlink`, `deepseek`, `openai-compat`. |
| `M-2` | Every call is issued for a role, never for a named link. |
| `M-3` | A role resolves to an ordered chain; the first healthy link wins. |
| `M-4` | Links carry a privacy class; `local` never leaves hardware the operator owns. |
| `M-5` | `local-only` disables every cloud link and the loop still runs. |
| `M-6` | Capabilities are probed and cached, never assumed. |
| `M-7` | Model facts — context length, quantization, state — come from `/api/v0/models`, not from a guess. |
| `M-14` | No hard-coded model ids; a dead id is a startup error naming its replacement. |

**The decision this batch forces, recorded before it becomes a surprise:**
`http://` to LM Studio is reachable with `std::net::TcpStream` and no
dependency. `https://` to DeepSeek needs TLS, which needs a crate, which is
**approval-gated** under [`binding.md`](../perpetum/binding.md). Batch 7 builds
the router and the transport *trait*; batch 8 implements transports, and the
DeepSeek one waits on that approval or on a decision to shell out to `curl`.
Recording it here means batch 8 starts with an answer rather than a discovery.
