# What four benchmark rounds are worth improving

Perpetum was run over Harness-Bench's 106 tasks four times, against four model
backends. This is what the evidence supports doing next, and — more usefully —
what it says about the evidence that came before it.

**Read the first section before the recommendations.** Three of the four largest
findings from the first round were artifacts of how the benchmark was set up, not
properties of the harness, and one of them motivated a change that has already
been made. That pattern is the most transferable thing here.

---

## The rounds, and what each one actually measured

| | Backend | Scored | Completion | What it turned out to be measuring |
|---|---|---|---|---|
| 1 | deepseek-v4-flash | 101 / 106 | 65.84% | mostly the benchmark's own configuration |
| 2 | deepseek-v4-flash | 106 / 106 | 74.91% | the harness, for the first time |
| 3 | claude-cli sonnet | 106 / 106 | 61.67% | one failure mode, 14 times |
| 3 | claude-cli opus | 106 / 106 | 78.36% | the same failure mode, once |
| 4 | deepseek-v4-pro | 106 / 106 | 73.91% | that a larger model bought nothing |

Round 1 reported **46 failures attributable to the harness**. After the setup was
corrected the same analysis over round 2 reported **four**. Nothing about
Perpetum changed in between that accounts for the difference.

### What was wrong with round 1

**The workspaces lived inside another project's git checkout.** Its `.gitignore`
matched them, so `T-29`'s "a write git will not keep is not work" check asked git
whether each path was ignored, got yes for everything, and left `touched` empty.
Everything downstream then went wrong at once: `V-13` recorded *"read and
reported, but changed nothing"* for steps that had written plenty, `G-3` staged
nothing, and the quiet-turn and give-up rules saw no progress. **59 of 133 runs**
carried the signature. `001-file` scored full marks from the oracle while being
told it had changed nothing, and spent fourteen turns where four sufficed.

That is filed as `T-33` and fixed. But it had already been written up as a
harness defect — *37 tasks averaging 46.5% against 78.0%* — and `L-35` was built
from it. `L-35` is a real gap in a real mechanism, and its evidence was
contaminated.

**Five tasks never ran** because their setup wanted a public URL for a local mock
server. They needed one environment variable, not a tunnel: the server is on
loopback and Perpetum runs on the same machine. Three of the five then scored
100%.

**Six tasks were graded by a grader that could not run.** Their oracles shell out
to `python3 -m pytest`, which on Windows resolves to a Store stub with no pytest.
Installing it moved `040` from 31% to 54% **on the same workspace, with no
re-run**.

### The one that has not been fixed

**Two tasks are still scored on a tenth of themselves.** `008-image-recognize`
and `013-image-edit` carry `outcome_llm_weight = 0.9`: nine tenths of their score
is a vision-capable model's judgement of the answer, and the remainder is
programmatic. That judge returns HTTP 429, and the blend silently falls back to
the programmatic part alone.

`008`'s programmatic checks are *both answer files exist and are non-empty*. One
backend scored **100%** on it having written `dog with brown and tan fur` for an
image whose reference reads *orange-and-white long-haired kitten*. It called a
kitten a dog and the oracle could not tell.

Both are flagged in the report. Neither is evidence about any backend.

---

## Where Perpetum actually stands

Four complete runs, ranked by combined score:

| Backend | Combined | Completion | Process | Zeros | Blocked | Native rung | Cost |
|---|---|---|---|---|---|---|---|
| claude-cli opus | **73.03%** | 78.36% | 91.53% | 1 | 1 | 0% | $165.84 |
| deepseek-v4-flash | 70.10% | 74.91% | 91.33% | 3 | 0 | **100%** | **$0.64** |
| deepseek-v4-pro | 68.71% | 73.91% | 90.18% | 1 | 0 | **100%** | $1.19 |
| claude-cli sonnet | 57.21% | 61.67% | 76.75% | 15 | 0 | 0% | $62.11 |

Two things worth stating plainly before any improvement work is prioritised.

**Security never moved.** 100% on every backend, every task, all four rounds.
Nothing tripped the gate. The refusals, the egress allowlist and the approval
boundary did their job and produced no false positives that cost a score.

**Process is high everywhere it is measured.** 90–92% on three of four backends.
Perpetum conducts itself well. Where it loses, it loses on delivery, not on
conduct — and that gap is where the remaining work is.

---

## 1. Sonnet rejects the tool protocol as a prompt injection — resolved

**The largest remaining loss. The transcripts have now been read; neither of the
two readings this section originally offered survived.** The full diagnosis is
[`harness-bench-three-turn-diagnosis.md`](harness-bench-three-turn-diagnosis.md);
what follows is the summary.

Both `claude-cli` backends parse **0%** of their turns at the native rung, so
every reply goes through the prompted rung's text parsing. On that shared path,
zeros with the exact signature — one tool call, three turns, ending on *"changed
nothing"*:

| | Zeros with the signature | Process on those | Process elsewhere |
|---|---|---|---|
| claude-cli sonnet | **14** of 106 | **11.15%** | 86.78% |
| claude-cli opus | **1** of 106 | 26.67% | 92.19% |

The two hypotheses were: the parser is model-sensitive, or Sonnet stops early on
its own. **Both are wrong.** In all 14 Sonnet runs the prompted rung parsed the
turn-1 tool call correctly and executed it. The loop dies on turn 2 — the first
turn that carries tool results back — when Sonnet declares Perpetum's `perp-call`
protocol *"a prompt injection attempt"* and refuses to use it further, in those
words, in 13 of the 14 transcripts. When `L-25`'s nudge arrives on turn 3 it
rejects that too, as *"fake labels and pressure tactics"*. In the worst case
(`008`) it then hallucinates a Claude Code UI render — `⎿ Wrote 1 line` — and
reports success over an empty workspace.

The partition is cleaner than the headline numbers:

- **Opus's single signature (`003-browser`) is `T-34`'s fetch gate**, not the
  parser — its journal shows the fetch queued for an approval nobody would give,
  and a lucid refusal to route around it. The real asymmetry is **13 vs 0**.
- Across all 106 tasks, Opus mentions injection only on the three tasks that are
  *about* injection defense. Sonnet does on 15 — 13 of them accusing the harness.
- The refusal is probabilistic, not a wall: Sonnet trips and still recovers on
  `085` and `104`. But the loop grants one further turn after an empty answer,
  so a turn-2 trip has no room.

The mechanism is two code facts together. `ladder.rs:329` treats a reply with no
fence at all as a completed answer, so the refusal parses as "done" and the
repair loop never fires. And `agent.rs:795` sends tool results plus the carry-on
instruction in a **user turn** — the exact shape Perpetum's own comments flagged
and fixed for the system prompt, with the tool-results half left undone. Timing
is 14/14 consistent: turn 1, system prompt only, correct call every time; turn 2,
first results-in-user-turn, protocol abandoned every time.

One caveat, the diagnosis's own: the mechanism is observational. Proving it
needs an A/B — rerun the 13 tasks with the carry-on moved into the system
prompt.

## 2. What to fix, now that the cause is known

**Not the parser.** Of the post-abandonment turns, only one names a real
Perpetum tool; the rest are `powershell(...)` calls to a tool that does not
exist, bare JSON, or `008`'s hallucinated write — which a more tolerant parser
would score as a write that happened. Widening tolerance recovers at most one
run and is actively dangerous. In order:

1. **An empty parse on the prompted rung goes loud** when the step has already
   issued a call and nothing was touched — spend a repair at `ladder.rs:329`
   quoting the format, instead of accepting silence as an answer. Cheapest
   change; all 13 runs would have received a corrective they never got.
2. **Move the carry-on instruction out of the user turn** (`agent.rs:795`) into
   the system prompt, leaving tool results as pure data. This attacks the cause.
3. **Native tool use is not the config switch it looks like.** `Kind::Anthropic`
   exists, but `probe.rs:151` sets `native_tool_calls` from
   `matches!(kind, Kind::DeepSeek)` alone — the `anthropic` kind lands on the
   same prompted rung as `claude-cli`. Widening that predicate is the long-term
   answer for API links; the `claude-cli` kind itself can never take it.
4. **`glob` fails in a non-git workspace** — `tool.rs:1005` implements it as
   `git ls-files`, exit 128 outside a repository. It poisoned the first tool
   result in 5 of the 14 and roughly triples Sonnet's trip rate (46% vs 15%
   with a clean first result). Opus eats the same error and recovers, so it is
   an amplifier, not the cause — and a defect regardless.

Two side defects surfaced in passing: `047`'s read path came back mangled by a
redactor mid-string and was then correctly refused by `X-2`, and `022`'s
`echo $MOCK_API_BASE` returned the literal string — POSIX expansion assumed
under a Windows shell.

## 2. `fetch` is a tool the loop does not have

Filed as `T-34`, not implemented.

`Tool::Fetch` is classified `Approve`, and `L-19` says the loop does not wait —
so unattended the call never runs and the request joins a queue nobody reads.
Measured across three rounds: **9 steps reached for `fetch`, all 9 refused**, on
five tasks. Two models then tried one shell workaround each and stopped at three
turns; a third found a `curl` that worked and scored 100% on the same task.

Whether a URL task succeeds is therefore decided by whether the model thinks to
route around its own harness.

The policy's stated reason is answered twice over elsewhere — `S-4`'s egress
allowlist decides which machines may be reached and applies to an approved fetch
anyway, and `S-1` decides what returning content may do, which is nothing. Making
`fetch` `Auto` for a host already on the egress allowlist costs nothing the other
two rules do not already cover.

## 3. Vision has plumbing and nothing to carry

`M-38` built image content blocks on both wire formats and a `vision` link key.
No configured link declares it, and the models under test cannot see, so
`008-image-recognize` scores zero on the API backends.

This is **configuration, not code** — and it is currently unmeasurable anyway,
because `008` is one of the two tasks whose semantic grading does not run. Fixing
the rubric key comes first; there is no point pointing a vision model at a task
that cannot score it.

## 4. `L-35`'s second attempt is worth keeping and was oversold

A step that stops having written nothing is now told once and given one further
turn. The mechanism is right and the gap was real — `L-25`'s notice rides back
with tool results, so it could never reach a step that stopped before its eighth
turn.

But its measured motivation was `T-33`'s contamination. In the clean round the
no-op cause accounts for **2 tasks**, not 37. Keep it; do not expect it to move a
score.

## 5. What is already done

`X-15` (path spelling on Windows), `G-20` (git refused outside the workspace
repository), `M-37` (first-token deadline as a link key), `T-33` (gitignore
scoped to the workspace's own repository). All gated, all committed. `G-20` is
the one worth remembering: the benchmark produced **fourteen commits on another
project's `main`**, unattended and unapproved, before it existed.

---

## Priority

| | Change | Evidence | Size |
|---|---|---|---|
| 1 | ~~Read a Sonnet three-turn transcript against an Opus one~~ **done** | verdict: protocol rejected as injection, 13 vs 0 | — |
| 2a | Empty parse goes loud on the prompted rung (`ladder.rs:329`) | 13 refusals accepted as answers | small |
| 2b | Carry-on instruction out of the user turn (`agent.rs:795`) | 14/14 break on the first results turn | small |
| 2c | Fix `glob` outside a git repository (`tool.rs:1005`) | poisoned 5 of 14 first results, 3× trip rate | small |
| 3 | `fetch` auto for an allowlisted host (`T-34`) | 9 of 9 refused, 5 tasks | small |
| 4 | A vision-capable rubric key | 2 tasks scored on a tenth of themselves | environment |
| 5 | ~~Declare `effort` on subprocess links~~ **done** | `M-34` was already built; the benchmark config now declares `medium` | — |

Nothing above is a correctness defect in Perpetum's parser — the one thing item
1 was expected to indict. 2a–2c are loop and tool defects the diagnosis
surfaced; 3 is a policy that does not fit unattended operation; 4 is the
benchmark's environment. After 2a/2b land, the A/B that proves the mechanism is
a rerun of the 13 tasks on Sonnet.

---

## What still cannot be measured, and should be said out loud

- **Run-to-run variance is the same size as the effects being claimed.** Between
  two runs of identical code, 53 of 106 tasks moved and 25 got *worse*, one from
  100% to 0%. Single runs cannot resolve differences under a few points. Flash
  and Pro differ by 1.0 point of completion, which is nothing.
- **Process for the subprocess backends is reconstructed** from Perpetum's
  journal rather than observed on the wire. The method validates — Opus's
  reconstructed 91.53% sits within 0.2 points of DeepSeek's wire-derived 91.33% —
  but it is not the same instrument.
- **Reasoning effort was never declared** on either Claude backend, so they ran
  at whatever the CLI defaults to while the API backends' level was fixed. `M-34`
  exists to prevent exactly this. The benchmark config now declares
  `effort = medium` on both `claude-cli` links — which fixes future runs and
  means the existing Opus and Sonnet numbers are not cleanly comparable to the
  next ones.
- **The process judge is `deepseek-v4-flash` judging DeepSeek's own runs.**
  Self-evaluation bias is unquantified.
- **Two tasks are not scored.** See above.

The honest summary of four rounds: Perpetum's conduct is strong and its security
boundary held completely; its losses are concentrated in one link kind and one
parsing rung; and most of what the first round appeared to find was the benchmark
measuring its own setup.
