# Perpetum state

Cycle: 2 · Phase: D · Batch: 7 — 2 of this cycle's five delivered ·
Updated: 2026-07-28

Binding: [`binding.md`](binding.md) · Journal: [`journal.md`](journal.md) +
`journal.jsonl` · Requirements: [`../perpetum.md`](../perpetum.md) ·
Batches: [`../prioritization/batches-cycle2.md`](../prioritization/batches-cycle2.md) ·
Board: [`progress-board.md`](progress-board.md)

## Position

**Cycle 1 closed** — 5 batches, 38 requirements, phases A–F complete.
**Cycle 2** — phases B and C done; Phase D under way.

| Batch | Theme | Delivered |
|---|---|---|
| 6 | Proving it works | 6 of 7, 1 carried |
| 7 | Model links — the router | 5 of 8, 3 carried |

- Next: **batch 8 — transport and protocols** (`M-8`–`M-10`, `M-21`–`M-23`),
  which also closes `M-6`, `M-7` and `M-14`. It opens with a decision, not a
  discovery: see below.

Carried, not done:

| id | Why |
|---|---|
| `N-6` | The harness opens no socket, but nothing stops a *project's* gate command reaching the network. Enforcement needs the sandboxed runtime in `T-9`. |
| `M-6` `M-7` `M-14` | Each has a half that needs a socket — probing a live endpoint, fetching `/api/v0/models`, refreshing the model list. The logic is done and tested against a recorded response; the network half is batch 8. |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — approval-gated. |

## Last green gates

`perp gate all --root .. --step c2/b7/s06`, exit 0, pinned to `95f14d3`.

- lint · exit 0 · clean
- build · exit 0 · clean
- tests · exit 0 · **171/171** — 152 unit, 5 spine integration, 14 end-to-end
- ids · exit 0 · 149 defined, no strays
- links · exit 0 · 2 links, 9 roles, every role has a local option

## Blocked

*(none)*

## Gated (carried, not counted as done)

- `X-4` — **approval-gated**: a Windows job object needs the `windows-sys`
  crate, and a dependency needs an approval under [`binding.md`](binding.md).

## Waiting on a decision

Three conflicts, unanswered since cycle 1 — `I-3`, `O-6`, `G-13`. All parked in
[`../prioritization/conflicts.md`](../prioritization/conflicts.md) with options
and a recommendation. None blocks anything.

**And one approaching**, recorded now rather than at the moment it bites:
batch 8 needs an HTTPS transport for the DeepSeek link. `http://` to LM Studio
works with `std::net::TcpStream` and no dependency; TLS does not. The options
are a TLS crate (approval), shelling out to `curl` (no dependency, but the API
key must not reach a command line — `S-2`), or `local-only` remaining the only
supported mode. Batch 7 builds the router and the transport *trait*, so the
decision lands before the code that needs it.

## Queued `/btw`

*(none — `C-11` is not built)*

## Cycle history

| Cycle | Batches | Features | Tests added | Version |
|---|---|---|---|---|
| 1 | 5 of 5 delivered | 38 | 128 | 0.1.0 (unreleased) |
| 2 | 2 of 5 delivered | 11 | 171 | 0.1.0 (unreleased) |
