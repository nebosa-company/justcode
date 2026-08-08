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

## 1. The prompted rung fails on some models and not others

**The largest remaining loss, and the newest evidence.**

Both `claude-cli` backends parse **0%** of their turns at the native rung — the
CLI has no native tool-call API, so every reply goes through the prompted rung's
text parsing. Both API backends parse **100%** natively.

On that shared prompted path:

| | Zeros with the signature | Process on those | Process elsewhere |
|---|---|---|---|
| claude-cli sonnet | **14** of 106 | **11.15%** | 86.78% |
| claude-cli opus | **1** of 106 | 26.67% | 92.19% |

The signature is exact and identical: one tool call, three turns, the step ending
on *"changed nothing"*. It never occurs on either API backend.

Two readings survive the data, and they have different fixes:

- **The parser is model-sensitive.** Something in how Sonnet phrases a tool call
  is read by the prompted rung 13% of the time and by Opus's phrasing 1% of the
  time. If so, the rung's tolerance is the defect.
- **Sonnet stops early on its own**, and the rung is innocent.

The rubric cannot separate them: it reads the same transcript either way, and
scores it near zero in both. **Reading a Sonnet three-turn transcript against an
Opus one is the next thing to do**, and it is cheap — the transcripts are on
disk. Until then this is the largest unexplained loss in the suite: those 14
tasks hold effectively all of Sonnet's deficit to DeepSeek.

The structural fix, if the parser is at fault, is not to widen the parser. It is
that a `claude-cli` link never reaches the native rung at all — so a link kind
that can use the Anthropic Messages API for tool use would remove the whole class
rather than making the fallback more forgiving.

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
| 1 | Read a Sonnet three-turn transcript against an Opus one | 14 tasks vs 1 on an identical code path | one hour |
| 2 | Native tool use for subprocess links, or a more tolerant prompted rung | follows from 1 | large |
| 3 | `fetch` auto for an allowlisted host (`T-34`) | 9 of 9 refused, 5 tasks | small |
| 4 | A vision-capable rubric key | 2 tasks scored on a tenth of themselves | environment |
| 5 | Declare `effort` on subprocess links | `M-34`; two backends ran at an unrecorded default | trivial |

Nothing above is a correctness defect in Perpetum. Items 1 and 2 are a capability
gap on one link kind; 3 is a policy that does not fit unattended operation; 4 and
5 are the benchmark's environment and the run's own configuration.

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
  exists to prevent exactly this.
- **The process judge is `deepseek-v4-flash` judging DeepSeek's own runs.**
  Self-evaluation bias is unquantified.
- **Two tasks are not scored.** See above.

The honest summary of four rounds: Perpetum's conduct is strong and its security
boundary held completely; its losses are concentrated in one link kind and one
parsing rung; and most of what the first round appeared to find was the benchmark
measuring its own setup.
