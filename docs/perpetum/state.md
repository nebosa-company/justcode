# Perpetum state

Cycle: 2 · Phase: **D complete** · Batch: 10 — 5 of 5 delivered ·
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
| 8 | Transport and protocols | 6 of 8, 2 carried, `M-8` untouched |
| 9 | Cost, caching and context | 4 of 4 |
| 10 | Local-server realities | 5 of 5 |

**Phase D's exit condition is met for cycle 2**: five batches delivered, suite
green. Next is **Phase E — release**, at 0.2.0, and then F closes the cycle.

Perpetum D also asks for end-to-end coverage to be extended over the five
batches. It was: the CLI suite grew from 11 tests to 15, including a full
`ask` → `cost` round trip over a real socket. That obligation is met this time
rather than carried.

Carried, not done:

| id | Why |
|---|---|
| `N-6` | The harness opens no socket, but nothing stops a *project's* gate command reaching the network. Enforcement needs the sandboxed runtime in `T-9`. |
| `M-21` | Chat completions works; `/v1/responses` is an explicit protocol variant that refuses rather than silently falling back. |
| `M-23` | Every request is bounded by connect and total deadlines, but a true first-token deadline needs streaming, which one-shot `curl` does not give. |
| `M-8` | Untouched — the degradation ladder needs tool calling, which needs the tool host in batch 11. |
| `M-24` | Filed, not built: declared credentials must be checked at bind time, not at the eleventh call. |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — approval-gated. |

## Last green gates

`perp gate all --root .. --step c2/b10/s07`, exit 0, pinned to `535b625`.

- lint · exit 0 · clean
- build · exit 0 · clean
- tests · exit 0 · **235/235** — 215 unit, 5 spine integration, 15 end-to-end
- ids · exit 0 · 149 defined, no strays
- links · exit 0 · 2 links, 9 roles, every role has a local option

## Blocked

*(none)*

## Gated (carried, not counted as done)

- `X-4` — **approval-gated**: a Windows job object needs the `windows-sys`
  crate, and a dependency needs an approval under [`binding.md`](binding.md).
- `M-25` — **external-gated**: inference on an LM Link peer cannot be reached
  from outside LM Studio. Measured, not assumed: a peer's models are absent
  from the local REST listing, and `lms` picks the device from a global setting
  rather than a per-call argument. Needs the LM Studio SDK, or a change on
  their side. The `lmlink` link kind is configurable and routable today; only
  the inference call is out of reach.

## Waiting on a decision

Three conflicts, unanswered since cycle 1 — `I-3`, `O-6`, `G-13`. All parked in
[`../prioritization/conflicts.md`](../prioritization/conflicts.md) with options
and a recommendation. None blocks anything.

**Answered:** the HTTPS question. The operator chose `curl`, so there is still
no dependency, and the credential goes to `curl -K -` on stdin rather than into
argv where any process listing would show it.

**A real model answered** in `c2/b10/s02`: the one model on this machine was
loaded (7.69s for 80 MiB), asked for an embedding, returned a real vector, and
was unloaded again. Chat is still unproven — this machine has no chat model, and
downloading one onto it is not the loop's decision. DeepSeek has no key.

## Queued `/btw`

*(none — `C-11` is not built)*

## Cycle history

| Cycle | Batches | Features | Tests added | Version |
|---|---|---|---|---|
| 1 | 5 of 5 delivered | 38 | 128 | 0.1.0 (unreleased) |
| 2 | 5 of 5 delivered | 26 | 235 | 0.1.0 (unreleased) |
