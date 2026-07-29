# Cycle notes — the operator's running account

**This file is written by hand. It is not the state file.** `state.md` is the
engine's projection of the journal (`L-4`) and is overwritten on every step —
which is how this file came to exist: `perp run` took `out.state` over in
`c3/b12/s19` and rewrote the narrative that had been living there since cycle 1.
That was the binding working as designed, and the narrative needed its own path.

Position, counts and blocked steps: read [`state.md`](state.md), which is
generated. Reasoning, decisions and what comes next: here.

Cycle: 3 · Phase: D **complete** — five batches · Updated: 2026-07-29

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

**Phase E** shipped 0.2.0 to the approval boundary. **Phase F** reconciled the
markers against the file, moved batches 6–10 out of the active list, and
recorded the numbers.

**F.1 found the progress board six batches stale** — the published artifact was
updated every batch, the file on disk was not. Regenerated, and it says so at
the top. `A-2`/`A-3` would generate both from the journal; neither is built,
which is now evidenced rather than merely argued.

**Cycle 3** — phases B and C done. All four pending decisions answered, and the
**conflicting count is zero** for the first time since cycle 1. Batch 11
delivered: the tool host and the permission classifier.

**Batch 12 landed the thing the cycle was for.** `perp run` takes the write
lock, walks steps, journals each, checks budgets at every boundary and stops for
one of exactly three named reasons. It has run a batch on this repository —
three gates, three steps, three transcripts, one terminal record.

**Batch 13 landed the way in.** `perp chat`, `perp btw`, `perp explain`. An
unknown slash command is an error rather than a prompt, an aside can never cross
the approval boundary, and the evidence chain for a decision is one command.

Running it found two defects no unit test would have: the outcome record was
dropping the requirement ids it was evidence for, and `perp run` was not pinning
its transcripts to a commit while `perp gate` was. Both fixed, both now tested.

**Cycle 3's Phase D is complete.** Five batches: the tool host, the loop driver,
chat and `/btw`, artifacts and the board, security enforcement. 103 requirements
done, 15 in progress, 387 tests, 67 red-run mutations all red.

- Next: **Phase E** (release) and **Phase F** (clean-up) to close cycle 3, then
  cycle 4's batches 16–20: OS integration, live control and rewind, the
  JustCode panel, runtime plugins, and the remaining model layer.

Perpetum D also asks for end-to-end coverage to be extended over the five
batches. It was: the CLI suite grew from 11 tests to 15, including a full
`ask` → `cost` round trip over a real socket. That obligation is met this time
rather than carried.

Carried, not done:

| id | Why |
|---|---|
| `N-6` | The **gate runner** still makes no network call, which is what this requirement is about. The harness as a whole now does — batch 8 gave it a transport — and nothing stops a *project's* gate command reaching the network either. Enforcement needs the sandboxed runtime in `T-9`. |
| `M-21` | Chat completions works; `/v1/responses` is an explicit protocol variant that refuses rather than silently falling back. |
| `M-23` | Every request is bounded by connect and total deadlines, but a true first-token deadline needs streaming, which one-shot `curl` does not give. |
| `M-8` | Untouched — the degradation ladder needs tool calling, which needs the tool host in batch 11. |
| `M-24` | Filed, not built: declared credentials must be checked at bind time, not at the eleventh call. |
| `S-8` | Filed in the Phase E security review, not built: request bodies land in a shared temp directory (`D:\Temp` here) while a call is in flight. |
| `X-4` | Tree kill works; surviving a kill of the engine itself needs a job object — approval-gated. |

## Last green gates

`perp gate all --root . --step c3/b12/s23`, exit 0, pinned to `567114b`.

- lint · exit 0 · clean
- build · exit 0 · clean
- tests · exit 0 · **387/387** — 367 unit, 5 spine integration, 15 end-to-end
- ids · exit 0 · 152 defined across 29 documents, no strays
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
| 2 | 5 of 5 delivered | 26 | 237 | 0.2.0 (unreleased) |

Two cycles, fifteen commits, fourteen branches, `main` unmoved at `e579167`.
One model call, an embedding, against a real local model. £0 spent.
