# Perpetum Harness — requirements

A speculative design for the runtime that executes [Perpetum](../../perpetum.md)
unattended: an agent harness in the shape of Claude Code and OpenHands, driving
a continuous B→F loop against local models (LM Studio, LM Link) and the DeepSeek
API — and usable as a chat client, a git harness and an OS-integrated tool host
in between cycles.

**Status: built, with seven open.** This document is the requirements source for the
harness — requirement ids are defined here and cited elsewhere (Perpetum 0.8).
Working name for the binary: `perp`.

As of cycle 12: **159 of 167 requirements are done**, 7 open, 1 external-gated
(`M-25`), 0 parked as conflicting. What exists is the spine (binding, steps,
journal, projection), the gate runner and its evidence, the recovery and
watchdog layer, the git harness, the verification machinery, the model router
over a `curl` transport exercised against a live LM Studio, cost accounting
replayed from the journal, the tool host and its permission classifier, **the
loop driver**, and the chat surface with `/btw` — in [`crates/`](../crates/),
606 tests, at version 0.2.0.

**The harness runs a batch on its own, against a real model.** `perp run
--requirement <id>` asks a link, parses tool calls through the degradation
ladder, runs them through the permission classifier, and feeds the results back
as data — journalled, costed, and stopped for one of exactly three named
reasons. It has done this on this repository on DeepSeek for tenths of a cent,
and on a Flutter project through a `claude-cli` link for nothing, against a
subscription.

### These came from running it

Not from reading the code. Each was found by a real cycle against a real
workspace, and each has a transcript behind it. Still open:

- `M-27` — `M-14`'s probe is skipped for a link with no server to ask.
- `M-28` — nothing bounds how many agent processes a batch spawns.
- `M-29` — a link that cannot cache reports zeroes indistinguishable from a
  broken ledger.
- `L-23` — thirteen tool calls before the first edit, on a one-file batch.
- `L-24` — `is_progress` counts `shell` as changing the workspace, so `L-11`'s
  watchdog has never once fired.
- `L-25` — nothing tells a step it has changed nothing until after it has
  stopped.
- `V-15` — a step may cite a requirement without a test, and cycle 9 did.

The last three came out of cycles 10 to 12, and out of reading what those cycles
did. They are the first entries here proposed rather than asked for. `L-25` is
the one that names a cause: the other two catch a failure after it has happened,
and that one is about the step knowing while it can still act.

`V-14` came out of cycle 10 and is closed in `8936558`. The batch took its ids
before the agent ran, because the branch needs them, so the gate filed its
result against every requirement in the batch including the ones that wrote
nothing. Nothing about that was false, which made it worse than the hole `V-13`
closed: the gates ran and were green, having measured a tree none of those
requirements had touched. There was no false claim to catch, only a wrong
attribution. The list is now taken after the work — `Agent` keeps `V-13`'s own
measure instead of discarding it, and the hand-over happens in `Then`, the one
place that holds both halves and knows the first has finished.

Cycle 10 is the clearest reading of where the loop is. Four requirements, four
failures, 117 model turns, not one byte of source changed — and it said so.
`V-13` caught two steps that read and reported; the turn ceiling caught the
other two, which spent forty turns each without an edit. The same cycle under
cycle 8's rules would have reported nine of eleven steps green. Nothing about
the work improved; the account of it stopped being false, which is all `V-12`
and `V-13` were ever going to buy. `L-23` is now being demonstrated by the steps
sent to fix it.

`X-13` is closed, and took three attempts to notice it was not. The loop filed
it done in cycles 6, 8 and 9, and each time the step closed green with three
green gates behind it. Cycle 9's attempt was the instructive one: nine lines,
all of them prose, adding a paragraph asserting `shell` was confined and putting
`X-13` in the module header, with no code and no test. The hole read as sealed
in the one place a reader would look. `V-13` did not catch it, because the step
*had* written — it wrote only the description of the change it had not made.
Closed for real in `a6b4ff0` and marked in `c10/x13/s03`, on a test that writes
a file outside the root and fails when the guard is taken out.

`X-14` was the exception to the heading above, and sat under it only because it
belonged beside `X-13`. It came from reading the shell arm while closing `X-13`,
not from a cycle, and had no transcript behind it. It is closed in `62732ae` and
marked in `c10/x14/s03`: `grep`'s `path` reached `git grep`'s command line
without ever being resolved, and what kept it from reading anything was git
declining to look outside its work tree. The disarmed run is the clearest
statement of the difference — with the resolve removed the test fails on `it
ran:` with an empty result, nothing having escaped and nothing having been
refused either. A tool the harness happens to call is not a boundary the harness
keeps.

`V-12` and `V-13` were the uncomfortable pair, and are closed. `V-2` — no
self-reported success — is the clause this document is built around, and cycle 8
showed it holding only at the requirement marker, by convention, with nothing
underneath it: the loop could have written its own `✅` and the run would have
looked identical. It now cannot write this file at all, and a step that changed
nothing cannot close green.

Both were marked on evidence a person went and made, in `c9/evidence/s02`.
`V-12` was proved by asking a live model to mark itself done and watching the
host refuse it (`c1/D/s125`). `V-13` was proved by cycle 9 doing to itself what
cycle 8 had got away with: the same steps, on the same work, closing not-ok and
saying *read and reported, but changed nothing*.

Closed in cycle 6, marked after a person read the evidence in
`c7/evidence/s02`: `M-26` (a run refused over a declared link no role chain
named), `T-19` (no way to delete a file except through the index) and `V-11`
(`perp check ids` passed on having found nothing to check).

The loop's own steps for all three **failed** — two on the 40-turn ceiling, one
on an unparseable tool block — and its gate was red when they ended, because it
had changed `missing_credentials` without updating the test that asserted the
old contract. The work was right and the run that produced it was not clean.
That is what `V-2` is for: the marker went on after a person ran the gates at
`acee1c3` and read the diff, not because a batch reported success.

`M-25` remains external-gated: inference on an LM Link peer is unreachable from
outside LM Studio, measured in `c2/b10/s01` rather than assumed.

Status markers below say which requirement is where; a marker without a matching
journal entry is not believed (Perpetum 0.7).

Perpetum describes *what* the loop does. This describes *what has to exist* for
the loop to survive being left running for a week with nobody watching.

---

## 0. Reading of the brief

Two terms are read as follows, because the design changes if the reading is
wrong:

- **LM Studio** — the local OpenAI-compatible server, `http://localhost:1234/v1`,
  plus its native `/api/v0` REST surface.
- **LM Link** — LM Studio's distributed-inference feature: remote machines you
  own, linked over a Tailscale-backed encrypted network, whose models appear in
  the model loader alongside local ones. Treated here as a *distinct link kind*
  from local LM Studio, because its latency, availability and failure modes
  differ even though the wire protocol is the same.

If "LM Links" meant a generic router (LiteLLM, OpenRouter), that is covered by
the `openai-compat` link kind in §3.1 and costs nothing extra.

### 0.1 Provider facts, verified 2026-07-28

Volatile. The harness must not hard-code any of it (see `M-14`).

| Fact | Value | Note |
|---|---|---|
| DeepSeek models | `deepseek-v4-flash`, `deepseek-v4-pro` | GA 2026-07-20 |
| DeepSeek context / output | 1M / 384K | both models |
| DeepSeek features | JSON output, tool calls, thinking mode (default), FIM (non-thinking only) | |
| `deepseek-chat` / `deepseek-reasoner` | deprecated 2026-07-24 | mapped to v4-flash non-thinking / thinking |
| DeepSeek caching | automatic prefix cache; cache-hit input priced ~50× under cache-miss | flash: $0.0028 vs $0.14 per 1M in |
| LM Studio OpenAI surface | `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings`, `/v1/models`, `/v1/responses` | port 1234 |
| LM Studio native surface | `/api/v0/models`, `/api/v0/models/{id}`, `/api/v0/chat/completions`, `/api/v0/completions`, `/api/v0/embeddings` | returns `state`, `max_context_length`, `quantization`, `arch`, TTFT and tok/s |
| LM Studio auth | Bearer token supported in current versions | assume required |
| LM Link setup | `lms login`, `lms link enable`, `lms link status`, `lms link set-device-name` | headless supported |

**Settled, 2026-07-28, on a machine with LM Link enabled and a peer connected:**
a peer's models are **not** served through the local REST API. `lms ls` listed
the same model twice — once for `Local`, once for the peer `KUR` — while
`/api/v0/models` listed it once. Device selection is a *global* preferred-device
setting (`lms link set-preferred-device`), not a per-request parameter. So the
`lmlink` kind cannot be a base-URL swap; see `M-25`.

---

## 1. What to take from where

| | Claude Code | OpenHands | Perpetum Harness |
|---|---|---|---|
| Runs on | your host, your repo | a Docker runtime | host first, container optional (`T-9`) |
| Session | one interactive session | one task, then done | **a cycle that outlives every session** |
| Interaction | chat | a task prompt | **chat and unattended loop, one engine, one journal** |
| State | context window + files | event stream + workspace | **journal on disk; context is disposable** |
| Permission | modes, per-tool prompts | sandbox as the boundary | **classification per call + a queue that never blocks the loop** |
| Git | done for you, on request | commits and PRs | **branch model owned by the engine; history is evidence** |
| Model | one vendor | any, via a router | **role→link routing, local first** |
| Done | the user says so | the task exits | **gate transcripts, plus a red run** |

What neither gives us, and what this document is mostly about:

1. **Survival.** A loop that runs for days crosses many context windows, several
   process restarts, and at least one crash. Nothing may live only in context.
2. **Non-blocking approvals.** Perpetum 0.4 stops for a human at certain steps.
   The other 95% of the loop must keep running while that step is parked.
3. **Anti-faking.** An unattended loop optimising for "green" will delete tests.
   The harness has to make lying mechanically harder than doing the work.
4. **Local-model tolerance.** A 7B model on an LM Link rig cannot be trusted to
   emit clean tool calls. The loop must degrade, not break.
5. **A way in while it runs.** An unattended loop you cannot talk to is a batch
   job. `/btw` (§8.2) is how a passing thought becomes a tracked requirement
   without stopping the machine.

---

## 2. The loop engine

### 2.1 Shape

```
        ┌──────────────────── control surface ─────────────────────┐
        │  chat · /btw · slash commands · approvals · artifacts    │
        │  CLI  ·  JustCode panel  ·  notification sink            │
        └────────────────────────────┬─────────────────────────────┘
                                     │
   ┌──────────────┐   ┌──────────────▼──────────────────────┐
   │  binding.md  │──▶│           loop engine               │
   │  state.md    │   │  phase machine · steps · budgets ·  │
   │ journal.jsonl│◀──┤  watchdogs · recovery · scheduler   │
   └──────────────┘   └──┬────────┬─────────┬────────┬──────┘
                         │        │         │        │
              ┌──────────▼──┐ ┌───▼─────┐ ┌─▼──────┐ ┌▼──────────┐
              │  LM Links   │ │  tool   │ │  git   │ │   gate    │
              │  router     │ │  host   │ │ harness│ │  runner   │
              │             │ │ fs·sh·os│ │        │ │ + red run │
              └──────┬──────┘ └────┬────┘ └───┬────┘ └─────┬─────┘
                     │             │          │            │
   ┌─────────────────┴───────┐ ┌───▼──────────▼────────────▼─────┐
   │ lmstudio  lmlink        │ │  runtime: host / WSL2 /         │
   │ deepseek  openai-compat │ │  container                      │
   └─────────────────────────┘ └─────────────────────────────────┘
```

### 2.2 Steps, not turns

The model's turn is not the unit of durability. A **step** is: one bounded piece
of work with a declared intent, a side effect, and a recorded outcome. Steps are
what the journal stores and what recovery replays against.

| id | Requirement |
|---|---|
| ✅ ~~`L-1`~~ | The phase machine implements Perpetum A–G. A runs once, B→F loops, G is terminal and entirely approval-gated. |
| ✅ ~~`L-2`~~ | Every phase declares its exit condition as a **checkable predicate**, not prose. The engine evaluates it; the model does not get to assert it. |
| ✅ ~~`L-3`~~ | Every step is written to `journal.jsonl` as an *intent* record before the side effect and an *outcome* record after. Records are append-only and never rewritten. |
| ✅ ~~`L-4`~~ | `state.md` (Perpetum 0.2) is a **projection** of the journal, rewritten after each outcome. If they disagree, the journal wins. |
| ✅ ~~`L-5`~~ | Every step is idempotent, or declares itself not and is bracketed by a reality check (`V-1`) on replay. |
| ✅ ~~`L-6`~~ | The engine can kill and respawn the model session at any step boundary with no loss beyond the in-flight step. |
| ✅ ~~`L-7`~~ | On start, the engine reconciles: find the last intent with no outcome, verify against the workspace what actually happened, then redo, skip, or park it. Never assume. |
| ✅ ~~`L-8`~~ | Context is disposable. A step may not depend on anything not reconstructible from binding, state, journal and the workspace. |
| ✅ ~~`L-21`~~ | The engine loads `binding.md` before anything else and refuses to run unbound. A path the binding does not resolve stops the loop and asks; it is never guessed (Perpetum 0.1). |
| ✅ ~~`L-22`~~ | Step ids are stable, ordered and human-citable — `c<cycle>/<phase or batch>/s<nn>`. The journal, the commit trailer, the board, the status marker and `/explain` all name the same step with the same string. |

### 2.3 Budgets and stopping

An unattended loop with no ceiling is a billing incident.

| id | Requirement |
|---|---|
| ✅ ~~`L-9`~~ | Budgets are declared per cycle and per batch, in three currencies: tokens, wall-clock, money. Reaching one parks the current work with a `budget` reason and stops cleanly at the next step boundary. |
| ✅ ~~`L-10`~~ | Money is counted from real usage, per link, per role, per step (`M-11`). Local links count as zero money but non-zero wall-clock and watts. |
| ✅ ~~`L-11`~~ | **No-progress watchdog:** N consecutive steps with no workspace change and no gate-state change ends the feature per Perpetum 0.5. Default N=5. |
| ✅ ~~`L-12`~~ | **Repetition watchdog:** the same tool call with the same arguments K times in a window is an error, not a retry. Default K=3. |
| ✅ ~~`L-13`~~ | **Thrash watchdog:** a file edited to a previously seen content hash within a batch is flagged; twice, the feature is blocked. |
| ✅ ~~`L-14`~~ | Stop conditions are exactly Perpetum F's: backlog exhausted, batch blocked, or a human says stop. Each writes a distinct terminal record. |
| ✅ ~~`L-15`~~ | The loop stops *clean*: no half-applied patch, no dangling branch, no running child process. |
| `L-23` | A step states what it intends to do before its first tool call, and the intent is journalled. A batch that opens with `pwd`, `ls` and `echo hello` has spent its turns establishing that the harness is real, which is a reasonable thing for an agent to wonder and an expensive way to answer it. Measured: thirteen tool calls before the first edit, on a batch that had one file to change. |
| `L-24` | Progress means the workspace changed, measured and not guessed from the tool name. `is_progress` counts `shell` as progress on the grounds that it "changes the workspace", and mostly it does not: twenty of cycle 12's twenty-three `shell` calls were `grep`. So `L-11`'s quiet-turn watchdog is unreachable — a step that greps through `shell` every fourth turn resets the counter forever, and cycles 10 to 12 never once fired it across steps of 40, 47, 56 and 59 turns. `V-13` already computes the honest measure, which is whether the touched set grew. |
| `L-25` | A step is told, while it can still act, that it has changed nothing. `V-13` decides a step wrote nothing at scoring time, which is after the model has stopped and cannot do anything about it. Cycle 12 raised the ceiling to 100 and no step reached it: `M-29`, `M-27` and `M-28` read for 47, 59 and 56 turns, concluded they understood the problem, and wrote a summary. Nothing had told them the job was an edit. The measure `V-13` keeps is the one to feed back, and the tool result is where it reaches the model. |
| ✅ ~~`L-16`~~ | Two attempts at a failing gate, then `BLOCKED` with the **verbatim error text** (Perpetum 0.5). The engine enforces the count; the model cannot ask for a third. |

### 2.4 Concurrency

| id | Requirement |
|---|---|
| ✅ ~~`L-17`~~ | One feature in flight at a time by default. Batch-level parallelism is opt-in and requires per-feature git worktrees (`G-11`). |
| ✅ ~~`L-18`~~ | Gate runs are serialised per workspace. Two builds in one target directory is a false red. |
| ✅ ~~`L-19`~~ | A parked approval never blocks the loop: the engine moves to the next eligible item and revisits parked items at the next phase boundary (Perpetum E, F.3). |
| ✅ ~~`L-20`~~ | Exactly one writer at a time. While the loop holds the write lock, chat is read-only unless paused (`C-3`). |

---

## 3. LM Links — the model layer

### 3.1 A link is a named endpoint, not a model name

Nothing in the harness names a model directly. It names a **role**; the router
resolves the role to a link.

```toml
[[link]]
name         = "rig"                       # LM Link peer, big model, my hardware
kind         = "lmlink"
device       = "workshop-4090"
model        = "qwen3-coder-30b"
privacy      = "local"
concurrency  = 1

[[link]]
name         = "here"
kind         = "lmstudio"
base_url     = "http://localhost:1234"
auth_env     = "LMSTUDIO_TOKEN"
model        = "qwen3-4b-instruct"
privacy      = "local"

[[link]]
name         = "ds-fast"
kind         = "deepseek"
base_url     = "https://api.deepseek.com"
auth_env     = "DEEPSEEK_API_KEY"
model        = "deepseek-v4-flash"
thinking     = false
privacy      = "cloud"

[role]
planner   = ["ds-pro", "rig"]
coder     = ["ds-fast", "rig"]
gatefixer = ["ds-fast", "rig"]
verifier  = ["rig", "ds-pro"]      # must differ from the author — V-5
chat      = ["ds-fast", "rig", "here"]
compactor = ["here"]               # high volume, cheap, never leaves the box
classifier= ["here"]
embedder  = ["here"]
```

| id | Requirement |
|---|---|
| ✅ ~~`M-1`~~ | Four link kinds: `lmstudio` (local), `lmlink` (remote peer), `deepseek`, `openai-compat` (Ollama, llama.cpp, vLLM, LiteLLM, OpenRouter — anything speaking `/v1/chat/completions`). |
| ✅ ~~`M-2`~~ | Every call is issued for a **role**. Roles: planner, coder, gatefixer, verifier, chat, compactor, classifier, summarizer, embedder. No call site names a link. |
| ✅ ~~`M-3`~~ | A role resolves to an ordered chain of links. First healthy link wins. |
| ✅ ~~`M-4`~~ | Links carry a `privacy` class. `local` never leaves hardware the user owns; `cloud` is subject to the egress policy (`S-4`). |
| ✅ ~~`M-5`~~ | `local-only` mode disables every `cloud` link. The loop must still run — slower, dumber, complete. This is a supported configuration, not a degraded one. |

### 3.2 Protocols and capability probing

Local models vary wildly. The harness discovers what a link can do rather than
assuming an OpenAI feature set.

| id | Requirement |
|---|---|
| ✅ ~~`M-6`~~ | On first use and on model change, probe: native tool calls, JSON-schema structured output, streaming, vision, embeddings, context length, reasoning-content field, prefix caching. Cache the result with a TTL; key it on link + model id + quantization. |
| ✅ ~~`M-7`~~ | For `lmstudio` and `lmlink`, take context length, `state`, `arch` and `quantization` from `/api/v0/models` rather than guessing. Record the exact quantization in the journal — a Q4 and a Q8 of the same model are not the same reviewer. |
| ✅ ~~`M-8`~~ | **Degradation ladder** for tool calls: native tool calling → JSON-schema constrained output → prompted block with a parse-and-repair loop (max 2 repairs, then the step fails honestly). The loop must complete with a model at the bottom rung. |
| ✅ ~~`M-9`~~ | Failover on timeout, connection loss, rate limit, or malformed output beyond repair. Failover to a link of a **different privacy class** requires the policy to allow it and is always journalled. |
| ✅ ~~`M-10`~~ | A substitution is never silent. The journal records which link produced every artefact, so "the 4B wrote this migration" is discoverable after the fact. |
| ✅ ~~`M-21`~~ | Two wire protocols are supported: **chat completions** (universal baseline) and **responses** (`/v1/responses`, LM Studio and OpenAI). The engine's internal message model is protocol-agnostic and converts at the link edge; a link declares its protocol from the probe, not from config guesswork. DeepSeek is chat-completions today. |
| ✅ ~~`M-22`~~ | Reasoning/thinking content is a separate channel: journalled, shown in chat behind a fold, never concatenated into the assistant message, never replayed into the next request's prefix, and never accepted as evidence for `V-2`. |
| ✅ ~~`M-23`~~ | Streaming is required for the chat surface and optional for the loop, but a first-token deadline applies either way — a link that has said nothing in *t* seconds is failed over, not waited on. |

### 3.3 Cost, caching and context

DeepSeek's prefix cache is worth an order of magnitude on input tokens, which
means prompt *layout* is an engineering requirement, not a style preference.

| id | Requirement |
|---|---|
| ✅ ~~`M-11`~~ | Track per call: input, output, cached-hit and cached-miss tokens, latency, TTFT. Aggregate per step, batch, cycle, role and link. Surface money spent this cycle in the progress board. |
| ✅ ~~`M-12`~~ | Prompts are assembled **stable-prefix first**: system rules, binding, tool schemas, then slowly-changing state, then the volatile task tail. Never reorder the stable region between calls in a batch. |
| ✅ ~~`M-13`~~ | Context compaction is a first-class step run by the `compactor` role on a local link. Compaction output is journalled, so what was dropped is recoverable. |
| ✅ ~~`M-14`~~ | Model ids, prices, context limits and endpoint paths live in config, refreshed from the provider's model list at startup. A deprecated or missing model id is a startup error naming the replacement, never a silent fallback. |
| ✅ ~~`M-26`~~ | A declared link that no role chain names is not credential-checked at startup. `M-24` checks every declared link, so a run refuses over a link nothing would have used — which happened twice on a workspace configured entirely for another provider, and the only fix was to set a variable for an endpoint that was never going to be called. |
| `M-29` | A link that cannot use prefix caching reports that, rather than reporting zeroes. Replacing a subprocess link's system prompt is what buys the tool protocol and it costs the cache with it, so `M-12` does not apply and a column of zeroes is a property of the link rather than a broken ledger — but nothing says so, and the two look identical. |
| ✅ ~~`M-24`~~ | Declared credentials are checked when the project is bound, not at first use. A link whose `auth_env` names an unset variable must fail `perp bind`, not the eleventh call of a batch — by which point the loop has spent an hour to discover a typo. Found in `c2/b8/s06`, where a local link with an optional token failed before it ever tried to connect. |
| ✅ ~~`M-15`~~ | Per-link concurrency limits are respected. One GPU serving one model does not want four parallel requests. |

### 3.4 Local-server realities

| id | Requirement |
|---|---|
| ⛔ `M-25` | An `lmlink` link cannot be reached by a base-URL swap: a peer's models are absent from the local REST listing, and `lms` selects the device from a **global** preferred-device setting rather than a per-call argument. Until LM Studio exposes per-request device selection, inference on a peer is **external-gated** — it needs the LM Studio SDK or a global setting change, and a loop that flipped a global setting to route one call would be changing the operator's environment underneath them. Measured in `c2/b10/s01`. |
| `M-27` | A subprocess link's model is verified against what the command accepts. `M-14`'s probe asks a server which models it serves, so it is skipped entirely for a link that has no server — and a `claude-cli` link with a misspelled model is discovered by a failing call rather than at startup, which is the failure `M-14` exists to prevent. |
| `M-28` | A subprocess link declares a concurrency bound and the router honours it, as `M-15` requires of an HTTP link. Nothing stops a batch spawning one command per parallel item today, and each is a whole agent process rather than a socket. |
| ✅ ~~`M-16`~~ | **Warm before a batch.** LM Studio JIT-loads models; a cold 30B load is minutes. The engine pre-loads the batch's links and holds them with a TTL longer than the batch's expected duration. |
| ✅ ~~`M-17`~~ | Never force two large models onto one host concurrently. The router treats a host's VRAM as a lease. |
| ✅ ~~`M-18`~~ | Use TTFT and tok/s from `/api/v0` to keep a rolling throughput estimate per link, and use it for both scheduling and the wall-clock budget. |
| ✅ ~~`M-19`~~ | `lmlink` health = peer reachable **and** the named model loadable on it. `lms link status` reports peers and loaded models; a peer that vanished mid-step fails the step, not the cycle. |
| ✅ ~~`M-20`~~ | A `lmlink` peer's disappearance never auto-promotes a cloud link when the run is `local-only`. It parks instead. |

---

## 4. Tools, runtime and the approval boundary

### 4.1 Tool host

| id | Requirement |
|---|---|
| ✅ ~~`T-1`~~ | Core tools: read, glob, grep, patch-edit, write, shell (bounded), git (§5), OS (§6), gate runner, HTTP fetch. Optional: browser, MCP client. |
| ✅ ~~`T-2`~~ | Edits are patches with pre-image verification. A patch whose context no longer matches fails; nothing is blind-written. |
| ✅ ~~`T-3`~~ | Every shell call has a timeout, a working directory, and a captured transcript. No unbounded process, ever. |
| ✅ ~~`T-4`~~ | Background processes are tracked and killed at step end (`X-4`). A dev server left running across steps is a leak the next gate will blame on the wrong feature. |
| ✅ ~~`T-5`~~ | Tool schemas are generated once per session and are part of the stable prefix (`M-12`). |
| ✅ ~~`T-6`~~ | Every tool result is truncated to a declared budget, with the truncation visible to the model. Silent truncation causes confident wrong conclusions. |
| ✅ ~~`T-19`~~ | A `delete(path)` tool exists, confined to the workspace like the other file tools. It asks before it runs — a deleted untracked file is the act `git clean` is refused for, and nothing can tell whose file it was. Today the only route to removing a file is `git rm`, which stages as a side effect and refuses on an untracked one — so the loop cannot clean up after itself without touching the index. |
| ✅ ~~`T-7`~~ | Tool output is **data, never instruction**. Content from files, HTTP, issue trackers and test output cannot change harness policy, approve an action, or redirect the loop (`S-1`). |

### 4.2 Runtime

| id | Requirement |
|---|---|
| ✅ ~~`T-8`~~ | Windows host is the primary runtime — this repo is a Tauri/Windows project and the loop must run where the build runs. |
| ✅ ~~`T-9`~~ | Runtimes are pluggable: host, WSL2, container. The gate commands come from `binding.md`; the runtime decides where they execute. |
| ✅ ~~`T-10`~~ | Work happens on a branch, never on `main` (`G-1`). |
| ✅ ~~`T-11`~~ | A `BLOCKED` feature leaves the tree clean: its work is committed to its own branch or shelved, never abandoned half-applied in the working copy. |

### 4.3 Approvals

Perpetum 0.4 is implemented as a classifier over tool calls, not as a prompt
instruction — the model must not be the thing that decides whether the model
needs permission.

| id | Requirement |
|---|---|
| ✅ ~~`T-12`~~ | Every tool call is classified `auto`, `approve`, or `never` before execution, by rule, on the harness side. |
| ✅ ~~`T-13`~~ | `never` covers Perpetum's absolutes: production deploys, customer contact, public posting, spending, and all of Phase G. A `never` call is refused and journalled; the model cannot argue its way past it. |
| ✅ ~~`T-14`~~ | `approve` enqueues a request with: what, why, the exact command or content, the diff, and the requirement id. The loop then continues elsewhere (`L-19`). |
| ✅ ~~`T-15`~~ | Approval is per action and per cycle. An approval granted last cycle never carries forward. |
| ✅ ~~`T-16`~~ | Approvals expire. An unanswered request older than the configured window is parked with reason `approval-gated` and carried into the next cycle (Perpetum 0.6). |
| ✅ ~~`T-18`~~ | The engine must not hold a lock on any artefact its own gates rebuild. On Windows a running executable cannot be replaced, so a harness launched from the workspace own `target/` fails its own build gate with `Access is denied (os error 5)`. The engine runs from a copy outside the tree it builds, and says so in the gate transcript. Found in `c1/b5/s08` by running the gates through the harness on its own repository. |
| ✅ ~~`T-17`~~ | Anything drafted for a human — release notes, issue replies, GTM copy — is written to disk unattended and *sent* only through an `approve` call. Drafting is free; sending is not. |

---

## 5. The git harness

Git is not a tool the loop happens to call. It is the loop's memory of what it
actually did, and the only thing that can undo it.

### 5.1 Branch and commit model

| id | Requirement |
|---|---|
| ✅ ~~`G-1`~~ | The engine owns the branch layout: `perp/c<cycle>/b<batch>` per batch, features committed onto it, merged to the integration branch only when every gate for the batch is green. `main` is never committed to directly. |
| ✅ ~~`G-2`~~ | One feature, one commit (or a tight, ordered series). Commit messages follow the binding's convention and carry trailers: requirement ids, journal step id, and the link + model + quantization that authored the change. A line of code traces back to a requirement and to the model that wrote it. |
| ✅ ~~`G-3`~~ | The staged set is computed from the step's touched files. `git add -A` and `git commit -a` are forbidden — an unattended loop must never sweep up an unrelated change it did not make. |
| ✅ ~~`G-4`~~ | Hooks run and are never bypassed. `--no-verify` is `never` (`T-13`). A hook failure is a gate failure and follows Perpetum 0.5. |
| ✅ ~~`G-5`~~ | Local commits are `auto`. Push, PR/MR creation, tagging a release, and publishing anything are `approve` (`T-12`, `S-7`). |
| ✅ ~~`G-6`~~ | Every gate transcript records the exact commit sha it ran against (`V-2`). A green gate at a sha that no longer exists is not evidence and does not count. |

### 5.2 History as a signal, and as an undo

| id | Requirement |
|---|---|
| ✅ ~~`G-7`~~ | `log`, `blame`, `show` and `diff` are Phase B and C inputs: churn hotspots feed prioritisation, and blame answers "was this already built?" faster than grep alone (Perpetum 0.7, `V-1`). |
| ✅ ~~`G-8`~~ | Feature commits are contiguous and recorded in the journal, so a single feature can be reverted cleanly. This is the git half of rewind (`O-4`). |
| ✅ ~~`G-9`~~ | Conflict handling is bounded: one automated attempt on non-overlapping hunks, then park as `blocked` with the conflict text verbatim. The loop never resolves a semantic conflict by picking a side quietly. |
| ✅ ~~`G-10`~~ | Rewriting published history, force-push, and `reset --hard` on a dirty tree are `never`. Any destructive git operation stashes first and journals the stash ref. |
| ✅ ~~`G-11`~~ | Worktrees are lifecycle-managed: created per feature when `L-17` parallelism is on, removed on merge or abandon, never left stale. |
| ✅ ~~`G-12`~~ | Submodules, LFS and in-repo hooks are detected at binding time and either supported or declared unsupported loudly. A loop that silently skips a submodule ships half a change. |
| ✅ ~~`G-13`~~ | The loop never commits **churn** — caches, build output, `crates/target/`, scratch files — and `.gitignore` is respected by every tool. The journal, state file, board and artifacts are **documentation, deliberately versioned**: they are the evidence a reviewer reads, and what makes `perp resume` work on a fresh clone. Resolved 2026-07-29, after implementing batch 4 showed the original wording contradicted the binding it was written alongside. |
| ✅ ~~`G-14`~~ | Repo state is asserted before each batch: expected branch, clean tree, no rebase or merge in progress, no detached HEAD. A surprising state parks the cycle rather than committing into it. |

---

## 6. OS integration

The loop runs on a real machine, and half of "exercise the real artefact"
(Perpetum 0.7) is an OS operation.

| id | Requirement |
|---|---|
| ✅ ~~`X-1`~~ | Surface: process control, filesystem outside the workspace, environment and toolchain discovery, notifications, opening files and URLs, screenshots, clipboard, and OS scheduling. |
| ✅ ~~`X-2`~~ | The workspace root is a permission boundary. Read outside it: `approve` unless allowlisted. Write outside it: `approve`, always. Delete outside it: `never`. |
| ✅ ~~`X-3`~~ | Toolchain discovery at cycle start: locate and record versions of the interpreters, compilers, package managers and `git` the binding names. "Works on my machine" becomes a journal entry instead of a mystery. |
| ✅ ~~`X-4`~~ | Children are spawned into a job object (Windows) or process group (POSIX) so a step's whole process tree dies with the step, including on kill -9 of the engine (`N-1`). |
| ✅ ~~`X-5`~~ | Notifications go to the OS notifier as one implementation of the sink in `O-6`: approval needed, batch blocked, budget hit, cycle complete. |
| ✅ ~~`X-6`~~ | Screenshot and window capture are available as evidence for `V-6`, stored beside the journal and referenced from the artifact (`A-6`). |
| ✅ ~~`X-7`~~ | Opening a workspace file or a localhost URL in the default app is `auto`. Any other URL or path is `approve`. |
| ✅ ~~`X-8`~~ | Clipboard read is `approve` — it is user data the loop did not create. Clipboard write is `auto`. |
| ✅ ~~`X-9`~~ | The harness can register itself with the OS scheduler (Task Scheduler, systemd, launchd) so a cycle resumes after a reboot. Registration is `approve`; resumption then reconciles per `L-7`. |
| ✅ ~~`X-10`~~ | Sleep and resume are survivable: a loop that wakes to a stale peer, an expired token or a moved clock reconciles rather than continuing on stale assumptions. |
| ✅ ~~`X-11`~~ | GUI automation — driving the mouse and keyboard of other applications — is out of scope. If ever added, it is `never` while unattended. |
| ✅ ~~`X-13`~~ | The `shell` tool is confined to the workspace root the way the file tools are (`X-2`). A command whose working directory is outside it, or that names an absolute path outside it, is refused before it runs. `X-2` was written about file tools and reads as covering everything; `shell` is the hole in it, and `cd` is one argument. |
| ✅ ~~`X-12`~~ | Gates run with a declared environment, not the ambient shell's. The unattended run and the operator's terminal must not disagree about `PATH`. |
| ✅ ~~`X-14`~~ | Every path a tool accepts is resolved before it is used, including the ones that are not called `path`. `grep`'s `path` argument goes into a command line unresolved, so a search rooted outside the workspace is not refused by the harness — only by `git grep` declining to look outside its work tree, which is git's behaviour and not a boundary the harness keeps. `X-13` closed the same hole for `shell`; this is the rest of it. |

---

## 7. The honesty machinery

This is the part a general-purpose harness does not have, and the part that
decides whether a week of unattended running produced software or a fiction.

| id | Requirement |
|---|---|
| ✅ ~~`V-1`~~ | **Reality check before build** (Perpetum 0.7). Before implementing any requirement, the engine runs a mandatory search step — grep plus `G-7` history — and records its result. A feature cannot enter implementation without one. |
| ✅ ~~`V-2`~~ | **No self-reported success.** A gate is green only if the engine ran the command itself and stored the transcript: command, cwd, commit sha, exit code, output tail, duration, timestamp. Model prose asserting success is not evidence and is never written to a status marker. |
| ✅ ~~`V-3`~~ | **The red run.** A new test must be executed against the tree *without* the change and observed to fail, then with the change and observed to pass. Both transcripts are stored. A test that passes in both runs does not satisfy Perpetum's gate 4, and the feature stays open. |
| ✅ ~~`V-4`~~ | **Test tampering is a hard error.** Deleting, skipping, weakening an assertion or loosening a matcher in an existing test during a gate-fix step aborts the step. If the test is genuinely wrong, that is a requirement — filed and cited, not an edit made in passing. |
| ✅ ~~`V-5`~~ | **Independent verification.** The verifier role must resolve to a different link than the one that authored the change. Self-review by the same model on the same context is not review. |
| ✅ ~~`V-6`~~ | **Exercise the artefact.** Once per batch, run the real thing — launch the app, open the page, run the CLI — and store the evidence (exit code, screenshot, log). Perpetum 0.7's second half is a step, not a suggestion. |
| ✅ ~~`V-7`~~ | Status markers are derived from journal evidence. The engine writes them; the model proposes. |
| ✅ ~~`V-8`~~ | Gated items (`external-gated`, `credential-gated`, `approval-gated`, `blocked`) are counted separately from done, forever, and are never re-picked without their reason changing. |
| ✅ ~~`V-10`~~ | The red run verifies the mutation **actually changed the file** before believing either result. A mutation that failed to apply reports a passing test that was never challenged — a false green wearing the costume of evidence. Found the hard way in `c1/b3/s12`, where a multi-line `sed` pattern silently matched nothing. |
| ✅ ~~`V-11`~~ | `perp check ids` fails when it finds no documents to check. It reports `documents: 0` and exits green today, which is a pass that proves nothing — the same shape of green as a test suite that never runs the code it names. A checker that cannot find its inputs has not checked them. |
| ✅ ~~`V-12`~~ | The requirements source named by the binding is not writable by a tool. `write`, `patch` and `delete` on it are refused before they run, whatever the call says it is for. `V-2` says model prose is never written to a status marker and `V-9` says ids are minted only here — both describe the same file, and neither is enforced against the loop that has `patch` and a workspace-relative path. Cycle 8 attempted it twice; what stopped it was a pre-image mismatch, not a rule. |
| ✅ ~~`V-13`~~ | A step that wrote nothing does not end `ok`. Filed as "made no tool call", which was already guarded and was not the defect: the steps that closed green had called plenty and written none of it. `V-2` refuses self-reported success at the requirement marker; the same claim one level down is currently recorded as a green step. Cycle 8 closed three steps `ok=True` whose summaries were prose, a survey of the repository, and a malformed `<perp-call>` block — the gates then passed because nothing had been touched, and the cycle reported eleven steps with two failures having changed not one byte. |
| ✅ ~~`V-9`~~ | Requirement ids are minted only in the requirements source named by the binding (Perpetum 0.8). A write that introduces a new id anywhere else — batches, board, state, a `/btw` note — is rejected by the engine. |
| ✅ ~~`V-14`~~ | A gate outcome is attributed only to requirements whose step changed something. A batch runs its gates once and files the result against every requirement in it, so a requirement whose step wrote nothing collects green gates it did not earn — the gates measured the tree as it already was. Cycle 10 recorded three green gates against `L-23`, `M-29` and `M-27` on a cycle that changed no source at all, and `perp explain` shows that green underneath the failed step. `V-13` stopped prose closing a step; this is the same claim one level up, where the evidence is real and the attribution is not. |
| `V-15` | A step may not add a requirement's citation without adding a test that fails without it. Cycle 9 closed `X-13` with nine lines of prose: a module-doc paragraph asserting `shell` was confined to the workspace, and `X-13` added to the file's requirement header, with no code and no test. Every gate went green, because a false docstring compiles, and `perp check ids` passed, because the id it cited is defined. The hole read as sealed in the one place a reader would look. `V-13` did not catch it — the step *had* written, and what it wrote was the description of the change it had not made. A citation is a claim about behaviour, and the test that fails without it is the only thing that distinguishes the claim from a sentence. |

---

## 8. Talking to the loop

### 8.1 Chat is the same engine

| id | Requirement |
|---|---|
| ✅ ~~`C-1`~~ | One binary, two modes: conversation and loop. Same tools, same permission classifier, same journal. Chat is not a second application with its own rules. |
| ✅ ~~`C-2`~~ | Work that comes out of a conversation becomes a requirement in the requirements source (`V-9`) before it is built. Chat does not create an untracked parallel backlog. |
| ✅ ~~`C-3`~~ | Chat while the loop runs is read-only by default: it answers from the journal, the state file and read tools. A write from chat requires either a pause or a target outside the loop's current feature (`L-20`). |
| ✅ ~~`C-4`~~ | Responses stream and are interruptible mid-generation; the interrupted partial is journalled, not discarded. |
| ✅ ~~`C-5`~~ | The conversation is journalled in the same stream as the loop, interleaved by time. "Why did it do that in cycle 3" is answerable months later. |
| ✅ ~~`C-6`~~ | Slash commands are engine-side, never model-interpreted: `/status` `/pause` `/resume` `/step` `/approve` `/reject` `/rewind` `/links` `/cost` `/board` `/gate` `/explain` `/btw`. An unknown slash command is an error, not a prompt. |
| ✅ ~~`C-7`~~ | `/explain <id\|sha\|step>` renders the evidence chain for a decision: the requirement, the reality check, the diff, the gate transcripts, the verifier's verdict, and the link that wrote it. |

### 8.2 `/btw` — the side channel

A thought you have while the loop is running should cost you nothing and lose
nothing. `/btw` is that: out-of-band operator input that never interrupts the
step in flight.

| id | Requirement |
|---|---|
| ✅ ~~`C-8`~~ | `/btw <text>` is accepted at any time, acknowledged immediately, and never aborts the current step. |
| ✅ ~~`C-9`~~ | Each `/btw` is classified into exactly one of: **steer** — applies to the current feature, injected at the next step boundary; **requirement** — filed to the requirements source with source `operator`; **constraint** — added to the policy for the rest of the cycle; **note** — journalled only. The classification is shown and is correctable with a follow-up. |
| ✅ ~~`C-10`~~ | A `/btw` can never cross the approval boundary. It cannot approve a parked action, raise a budget, disable a gate, or reclassify a `never`. Those are explicit commands with their own confirmation. A casual aside must not be able to unlock the dangerous half of the harness. |
| ✅ ~~`C-11`~~ | `/btw` is available from the CLI, the JustCode panel, and the reply path of the notification sink (`O-6`), and is queued when the engine is not running — the next cycle picks it up at Phase B. |
| ✅ ~~`C-12`~~ | Queued and unclassified `/btw` items appear in `state.md`, so they survive a restart and are visible to whoever resumes the loop. |

### 8.3 Artifacts

Perpetum Appendix 2 already asks for the progress board to be published. Every
other durable output of a cycle deserves the same treatment.

| id | Requirement |
|---|---|
| ✅ ~~`A-1`~~ | Artifact kinds: progress board, cycle report, batch plan, release notes draft, gate evidence bundle, architecture or dependency diagram, conflict register. |
| ✅ ~~`A-2`~~ | Artifacts are generated from the journal, are self-contained (no external fetches, no CDN, assets inlined), and are written under `docs/perpetum/artifacts/` with a stable name per kind, so a re-render replaces rather than accumulates. |
| ✅ ~~`A-3`~~ | The progress board is regenerated at step 7 of every feature (Perpetum Appendix 2) — file and rendered view both. |
| ✅ ~~`A-4`~~ | Rendering locally is `auto`. Publishing an artifact anywhere outside the workspace is `approve` — that is Perpetum 0.4's "posting publicly", regardless of how private the destination claims to be. |
| ✅ ~~`A-5`~~ | Artifacts render in the JustCode panel and standalone in a browser, with no server and no build step. |
| ✅ ~~`A-6`~~ | Every artifact carries provenance: cycle, batch, commit sha, generation time, and the links used. An artifact without provenance is decoration. |
| ✅ ~~`A-7`~~ | Artifact generation is never on the critical path. A failed render is a warning; it never blocks a feature or fails a gate. |

---

## 9. Observability and control

| id | Requirement |
|---|---|
| ✅ ~~`O-1`~~ | `journal.jsonl` is the source of truth and is replayable: given the journal and the repo at a commit, the engine can reconstruct what the loop believed at any step. |
| ✅ ~~`O-2`~~ | The progress board (`A-3`) is the status-at-a-glance surface and is written to disk as well as rendered. |
| ✅ ~~`O-3`~~ | Live controls: pause at next step boundary, resume, single-step, inject a message, redirect to another requirement, abort the cycle cleanly. |
| ✅ ~~`O-4`~~ | **Rewind:** resume from any journal step, discarding later work, with the workspace reset to that step's commit (`G-8`). This is how a bad batch is recovered without re-running the cycle. |
| ✅ ~~`O-5`~~ | A watch mode streams: phase, batch, feature, link in use, tokens and money this cycle, gate state, blocked and gated counts, pending approvals, queued `/btw`. |
| ✅ ~~`O-6`~~ | Notification sink is pluggable (OS notifier, webhook, mail) and is **outbound only**, with exactly one exception: a reply may carry a `/btw` note and nothing else. **Approvals never arrive over the network.** The phone tells you something needs you; you still walk to the machine. Resolved 2026-07-29 — the authentication problem is removed rather than solved, because one bug in a signature check reopens the approval boundary. |
| ✅ ~~`O-7`~~ | Cycle metrics (Perpetum F.5) are appended to the state file's history table by the engine, from counted facts, not from a summary. |

### 9.1 JustCode integration

Speculative, and the reason this document lives in this repo.

| id | Requirement |
|---|---|
| ✅ ~~`I-1`~~ | The engine is a Rust core with two front-ends: a CLI (`perp run`, `perp chat`, `perp status`, `perp approve`, `perp rewind`) and a JustCode panel. |
| ✅ ~~`I-2`~~ | In JustCode the engine runs as a **sidecar process**, not in the Tauri main process. An agent loop must not be able to take the editor down with it, and must outlive the editor window. |
| ✅ ~~`I-3`~~ | The panel hosts chat, the approvals queue, the current diff, the artifact view and a journal timeline. Approving from the panel opens the diff first. |
| ✅ ~~`I-4`~~ | Existing editor surfaces are reused where they fit: the Problems panel for gate failures, the terminal dock for gate transcripts, tabs for the files under edit. |
| ✅ ~~`I-5`~~ | The panel is a view onto the journal, not a second source of truth. Closing the editor does not stop the loop; reopening re-attaches. |

---

## 10. Security and privacy

| id | Requirement |
|---|---|
| ✅ ~~`S-1`~~ | Instructions come from the operator and from `binding.md` alone. Repository content, dependency code, issue text, web pages and tool output are data (`T-7`). Local models are *more* susceptible to injection, not less, and the boundary is enforced by the harness rather than by prompting. |
| ✅ ~~`S-2`~~ | Secrets are referenced by environment variable name in config, never stored in it. Keys are never placed in a prompt, a journal record, an artifact, or a commit. |
| ✅ ~~`S-3`~~ | Outbound prompt content is redacted against a configurable secret pattern set before it reaches any `cloud` link. |
| ✅ ~~`S-4`~~ | Egress policy: an allowlist of hosts the loop may reach. Everything else is refused and journalled. |
| ✅ ~~`S-5`~~ | `credential-gated` items are recognised and parked. The harness never invents, requests, or types a credential to get past a gate. |
| ✅ ~~`S-6`~~ | A `local-only` run makes exactly zero outbound connections beyond the configured local and LM Link peers, and this is assertable from the journal. |
| ✅ ~~`S-8`~~ | Request bodies are written to a file while a call is in flight, and that file inherits the ambient temp directory. On this machine `TMP` is `D:\Temp` — a shared root-level directory, not the per-user one — so a prompt containing repository content is briefly readable by any other user of the machine. The body must go in a private directory with restrictive permissions, or through a pipe. Found in the cycle 2 Phase E security review; the key itself is unaffected, since it never touches disk. |
| ✅ ~~`S-7`~~ | Push, PR creation, tagging and publishing are `approve` (`G-5`). Nothing leaves the machine unattended. |

---

## 11. Non-functional requirements

| id | Requirement |
|---|---|
| ✅ ~~`N-1`~~ | **Crash-only.** Kill -9 at any moment loses at most the in-flight step. No clean-shutdown path is required for correctness. |
| ✅ ~~`N-2`~~ | A cold start reads only the binding and the journal. No hidden state in a cache, a temp file, or a model's memory. |
| ✅ ~~`N-3`~~ | Single binary, no daemon required, no container required for the default host runtime. |
| ✅ ~~`N-4`~~ | Cross-platform: Windows first, then Linux/WSL2 and macOS. Path handling, line endings and shell quoting are tested on Windows, not assumed. |
| ✅ ~~`N-5`~~ | Deterministic replay of the journal for inspection: the same journal renders the same state file and the same board, on any machine. |
| ✅ ~~`N-6`~~ | Gates run without network access wherever the project allows it, so a flaky connection cannot manufacture a red. |
| ✅ ~~`N-7`~~ | Engine overhead is bounded and reported: tokens spent on compaction, classification and routing are counted separately from work tokens. |
| ✅ ~~`N-8`~~ | Config, journal and artifact formats are versioned with forward-compatible readers. A loop mid-cycle must survive a harness upgrade. |
| ✅ ~~`N-9`~~ | No panic on a runtime-fallible path. Parsing, IO and process execution return typed errors; `unwrap`/`expect` appear only where the invariant is local and proven. A harness that panics mid-batch cannot honour `L-7`. |
| ✅ ~~`N-10`~~ | Derived files (state, board, artifacts) are written atomically — temp file, then rename — so a crash mid-write cannot leave a truncated projection that contradicts the journal. |
| ✅ ~~`N-12`~~ | End-to-end tests drive `perp` as a subprocess, not just its library. Perpetum D extends E2E coverage every five batches; unit tests over a library cannot catch an argument parsed wrongly, a path resolved from the wrong root, or an exit code that lies. |
| ✅ ~~`N-11`~~ | Dependency policy: standard library first; every third-party crate carries a recorded reason at the point it is added; the workspace builds from a warm cache with no network. A cold-cache failure blocks the batch rather than silently changing the plan. |

---

## 12. Milestones

| | Milestone | Delivers | Proves |
|---|---|---|---|
| M0 | Walking skeleton | one link (`lmstudio`), tool loop, gate runner, journal, one feature end-to-end | the gate-transcript idea works |
| M1 | Survival | steps, recovery, watchdogs, budgets, `state.md` projection | kill it mid-batch; it resumes correctly |
| M2 | Links | router, roles, capability probe, `deepseek` + `lmlink`, chat/responses protocols, failover, cost accounting | `local-only` completes a batch; cloud completes it faster |
| M3 | Git and OS | branch model, commit trailers, revert, worktrees, process supervision, notifications | a bad feature is reverted without touching the rest of the batch |
| M4 | Honesty | red run, tamper guard, independent verifier, artefact exercise | a deliberately faked green is caught |
| M5 | Chat and `/btw` | conversation mode, slash commands, side channel, classification | a `/btw` becomes a filed requirement without stopping the batch |
| M6 | Full loop | phases B, C, E, F; approvals queue; artifacts | one unattended cycle, start to release-boundary |
| M7 | JustCode panel | sidecar, board, approvals, diff review, timeline | the loop is watchable without a terminal |

M0–M4 are the harness. M5–M6 are Perpetum. M7 is the product.

---

## 13. Open questions

1. ~~**LM Link through `/v1`.**~~ **Answered 2026-07-28 by testing it.** Remote
   models are not in the local REST listing; only `lms` sees them, and it picks
   the device from a global setting rather than a per-call argument. The
   consequence is filed as `M-25`, and the evidence is in `c2/b10/s01`.
2. **Cache hit rate under a churning context.** DeepSeek's prefix cache pays for
   stable prefixes; an agent loop mutates its context constantly. `M-12` is a
   guess at the right layout. Measure hit ratio per step type before optimising.
3. **Small models and structured output.** How far down the parameter count can
   `classifier` and `compactor` go before the repair loop (`M-8`) costs more than
   the model saves? This decides whether `local-only` is genuinely usable — and
   whether `/btw` classification (`C-9`) can run locally at all.
4. **Red run cost.** `V-3` implies stashing the change, running tests, restoring.
   On a large suite this may need per-test targeting or a dedicated worktree.
   Cheap enough to run for every feature is the requirement.
5. **Worktrees on Windows.** Per-feature worktrees plus deep `node_modules` and
   Rust target directories meet the path-length ceiling quickly. May force the
   container or WSL2 runtime for `L-17` parallelism.
6. **Model-id drift.** `deepseek-chat` died four days ago. `M-14` handles the
   error, but a loop that wakes after a month idle should park and name the
   replacement, not thrash.
7. **Who owns the binding?** If the harness writes `binding.md` in Phase A, it is
   configuring its own gates. Convenient, and slightly circular; a human sign-off
   on the first binding may be the right exception to Perpetum 0.4.
8. **Artifact rendering without a build step.** `A-5` wants self-contained pages
   with inlined assets and no CDN, rendered both in a Tauri webview and a plain
   browser. Diagrams are the hard case — an inline renderer or pre-rendered SVG.
