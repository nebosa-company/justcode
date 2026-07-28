# Batches — cycle 1

The next 10 batches (Perpetum C.7). Each is 5–10 active requirements that are
logically related, in dependency order, with the combined effort estimate and
the score that produced it.

## Delivered in cycle 1 (Perpetum F.2)

Batches 1–5 are closed. Phase D's exit needs five, and five is what it got.

| # | Theme | Delivered | Carried or parked |
|---|---|---|---|
| 1 | The spine | 7 of 9 | `L-4` and `L-8` carried; `L-4` closed in batch 3 |
| 2 | Gate runner and evidence | 4 of 6 | `V-2` carried, closed in batch 4; `X-4` approval-gated |
| 3 | Recovery and watchdogs | 10 of 10 | — |
| 4 | The git harness | 7 of 8 | `G-13` parked as conflicting |
| 5 | The honesty machinery | 8 of 8 | — |

**Batches 6–10 below are unchanged and compete again in cycle 2's Phase C**, in
front of a backlog that now has 59 unbatched requirements plus everything cycle
2's Phase B turns up.

Requirements source: [`../perpetum.md`](../perpetum.md) — 146 requirements after
this cycle's minting (`N-9`–`N-11` from the NFR pass, `L-21`–`L-22` from C.1).
Ids are cited here, never defined (Perpetum 0.8, `V-9`).

## Where score and coherence disagree

**They disagree badly this cycle, and coherence wins.** Scored strictly, batch
10 (artifacts and the board — source 60, effort M) outranks everything at 1600,
and batch 1 (the journal spine — source 40, effort L) sits near the bottom at
400. Building in score order would mean rendering a progress board from a
journal that does not exist. Every score in the table is real; the ordering
below is dependency order, and the one-line justification Perpetum asks for is:
**you cannot report on a loop you have not built, and eight of the ten batches
read or write the journal.**

The scores are still doing work — they are why the model layer (batches 6–7,
score 600) comes before the tool host and approvals (batch 8, score 160) once
the spine is done, rather than the other way round.

| # | Theme | Members | Effort | Source | Impact | Score |
|---|---|---|---|---|---|---|
| 1 | The spine — journal, steps, state projection | `L-3` `L-4` `L-8` `L-21` `L-22` `N-8` `N-9` `N-10` `O-1` | L (8) | 40 | 80 | 400 |
| 2 | Gate runner and evidence | `V-2` `T-3` `L-16` `X-4` `X-12` `T-4` | M (3) | 40 | 80 | 1067 |
| 3 | Recovery and watchdogs | `L-5` `L-6` `L-7` `L-11` `L-12` `L-13` `L-15` `N-1` `N-2` | L (8) | 40 | 80 | 400 |
| 4 | Git harness | `G-1` `G-2` `G-3` `G-4` `G-6` `G-10` `G-13` `G-14` | L (8) | 50 | 80 | 500 |
| 5 | The honesty machinery | `V-1` `V-3` `V-4` `V-7` `V-8` `V-9` `G-7` `G-8` | L (8) | 40 | 80 | 400 |
| 6 | Model links — the router | `M-1` `M-2` `M-3` `M-4` `M-5` `M-6` `M-7` `M-14` | L (8) | 60 | 80 | 600 |
| 7 | Model links — talking to them | `M-8` `M-9` `M-10` `M-11` `M-12` `M-21` `M-22` `M-23` | L (8) | 60 | 80 | 600 |
| 8 | Tools, runtime and approvals | `T-1` `T-2` `T-5` `T-6` `T-7` `T-12` `T-13` `T-14` `T-15` `L-19` | XL (20) | 40 | 80 | 160 |
| 9 | Chat, slash commands, `/btw` | `C-1` `C-2` `C-3` `C-4` `C-5` `C-6` `C-8` `C-9` `C-10` `C-12` | L (8) | 50 | 80 | 500 |
| 10 | Artifacts and the board | `A-1` `A-2` `A-3` `A-4` `A-6` `A-7` `O-2` `O-5` `O-7` | M (3) | 60 | 80 | 1600 |

---

## Batch 1 — The spine

**Theme:** the durable record everything else reads and writes. Nothing in the
harness is testable until a step can be written down and read back.

| id | What it means here |
|---|---|
| `L-21` | Load `binding.md`; refuse to run unbound; an unresolved path stops and asks. |
| `L-22` | Stable, ordered, citable step ids — `c1/b1/s07`. |
| `L-3` | Append-only `journal.jsonl`: intent before the side effect, outcome after. |
| `L-4` | `state.md` as a projection of the journal; journal wins on disagreement. |
| `L-8` | Everything a step needs is reconstructible from binding + journal + workspace. |
| `O-1` | The journal is replayable into the state a run believed at any step. |
| `N-8` | Journal records and config carry a version; readers are forward-compatible. |
| `N-9` | Typed errors on every fallible path; no panic mid-batch. |
| `N-10` | Atomic writes — temp file then rename — for every derived file. |

**Ordering inside the batch:** `L-21` → `L-22` → `L-3` → `N-8`/`N-9`/`N-10`
(properties of the above) → `L-4` → `O-1`/`L-8` (which are assertions about the
first six).

**Combined effort:** L (8). **Delivers:** the M0 walking skeleton's foundation.

## Batch 2 — Gate runner and evidence

**Theme:** running a command and being able to prove what happened. Deliberately
second, because `V-2`'s transcript shape has to exist before anything calls a
gate — retrofitting evidence onto a runner that returns a boolean means
rewriting every caller (backlog pass, "shape of the work").

Members: `V-2` `T-3` `L-16` `X-4` `X-12` `T-4`. Effort M (3).

## Batch 3 — Recovery and watchdogs

**Theme:** surviving a dead session. `L-7`'s reconcile is the batch's spine;
the watchdogs (`L-11`–`L-13`) are what stop a loop that survived from spinning.

Members: `L-5` `L-6` `L-7` `L-11` `L-12` `L-13` `L-15` `N-1` `N-2`. Effort L (8).

**Dependency:** requires batch 1 (journal) and batch 2 (a step that can fail).

## Batch 4 — Git harness

**Theme:** the loop's other memory. Branch layout, explicit staging, trailers
that make a commit traceable to a requirement and a model, and the refusals
(`G-4`, `G-10`) that keep an unattended loop from destroying work.

Members: `G-1` `G-2` `G-3` `G-4` `G-6` `G-10` `G-13` `G-14`. Effort L (8).

**Dependency:** `G-6` needs batch 2's transcript; `G-2` needs `L-22`'s step ids.

## Batch 5 — The honesty machinery

**Theme:** the product thesis. The red run (`V-3`) is the expensive one and the
reason the batch is L rather than M.

Members: `V-1` `V-3` `V-4` `V-7` `V-8` `V-9` `G-7` `G-8`. Effort L (8).

**Dependency:** all of batches 1, 2 and 4. `V-3` cannot exist without a gate
runner and a git harness to stash against.

## Batch 6 — Model links: the router

**Theme:** the first batch that talks to a model. Roles resolve to links; links
are probed, not assumed; no model id is hard-coded (`M-14`, the requirement the
market pass justified on its own).

Members: `M-1` `M-2` `M-3` `M-4` `M-5` `M-6` `M-7` `M-14`. Effort L (8).

**Note:** tests for this batch must run without a GPU and without a network. The
link layer is tested against a recorded-response fake; a real LM Studio is an
integration test, not a unit test.

## Batch 7 — Model links: talking to them

Members: `M-8` `M-9` `M-10` `M-11` `M-12` `M-21` `M-22` `M-23`. Effort L (8).

**Dependency:** batch 6.

## Batch 8 — Tools, runtime and approvals

**Theme:** the largest batch and the lowest score, which is correct — it is
enormous and nothing else is blocked on it. The permission classifier (`T-12`,
`T-13`) is the part that must not be got wrong; the tool catalog (`T-1`) is
volume.

Members: `T-1` `T-2` `T-5` `T-6` `T-7` `T-12` `T-13` `T-14` `T-15` `L-19`.
Effort XL (20).

**Candidate to split** in cycle 2 if it stalls: classifier + approvals as one
batch, tool catalog as another.

## Batch 9 — Chat, slash commands, `/btw`

**Theme:** the operator's most recent ask, and the batch that turns a batch job
into something you can talk to. `C-10` — a `/btw` can never cross the approval
boundary — is the requirement to write a test for first, not last.

Members: `C-1` `C-2` `C-3` `C-4` `C-5` `C-6` `C-8` `C-9` `C-10` `C-12`.
Effort L (8).

**Dependency:** batches 6–7 (a model to talk to), batch 8 (`C-3`'s read-only
tool restriction is the classifier).

## Batch 10 — Artifacts and the board

**Theme:** the visible half. Scored first, built last, for the reason at the top
of this file.

Members: `A-1` `A-2` `A-3` `A-4` `A-6` `A-7` `O-2` `O-5` `O-7`. Effort M (3).

**Dependency:** batch 1. Everything here is a projection of the journal.

---

## Not in these batches

61 requirements remain unbatched — 59 active, 2 parked as conflicting. The 85
above account for the rest. Unbatched: the rest of the OS
layer (`X-1`–`X-3`, `X-5`–`X-11`), the runtime plugins (`T-8`–`T-11`), the
remaining model-host realities (`M-13`, `M-15`–`M-20`), security enforcement
(`S-1`–`S-7`), rewind and live control (`O-3`, `O-4`), independent verification
and artefact exercise (`V-5`, `V-6`), and all of JustCode integration
(`I-1`–`I-5`). They compete again in cycle 2's Phase C.

Two are parked as conflicting, not unbatched: see [`conflicts.md`](conflicts.md).
