# Harness-Bench findings — planned fixes to `perp`

What a full Harness-Bench run found wrong with this harness, and what to change.

**This document mints no requirement ids.** Ids are minted in
[`.harness/perpetum.md`](../.harness/perpetum.md) and nowhere else
(`V-9`); everything below cites existing ones. When a fix here is taken up, its
id goes in the requirements source first and this document becomes commentary on
it.

**Status: fix 1 is implemented and gated; fixes 2 to 5 are not.** Every number
below is measured from a run that has already happened, not projected.

---

## The run

Perpetum 0.2.0, driven by HarnessBench's `perpetum` adapter, one bound workspace
and one minted requirement per task, against `deepseek-v4-flash` over an
OpenAI-compatible link.

| | |
|---|---|
| Tasks scored | 101 of 106 |
| Completion | 0.6584 |
| Process | 0.8888 |
| Combined | 0.6258 |
| Security | 1.0000 — no gate tripped on any task |
| Spend | $0.52 |
| Tool-call rung | 1,197 turns, every one `native` |

Failure causes below are read from each run's own `journal.jsonl`, not inferred
from the score. That is the only reason this document can attribute anything:
the journal records what was refused, what changed nothing, and what blocked,
and it survives the run that produced it.

**Process is 0.23 above Completion, and that gap is the subject of this
document.** The harness conducted itself well and delivered less than it should
have. Where a fix below is right, it should move both columns.

Two cautions on the numbers. The process rubric was judged by
`deepseek-v4-flash` — the same model under test — because the default
`gpt-4o-mini` credential has no credits; self-evaluation bias is unquantified.
And six tasks could not be graded at all on the test machine (see
[Not our bugs](#not-our-bugs)), which depresses Completion by up to 3.98 points
for every harness equally.

---

## 1. A detected no-op should get a second attempt

**Cited:** `L-11`, `L-14`, `L-25`, `M-8` · **filed as `L-35`** · **implemented**

Implemented in `perp-core/src/agent.rs`. The diagnosis below is sharper than it
was when this was written: `L-25` and its notice already existed, and the defect
was narrower than "nothing retried it". The notice is appended to *tool results*,
so it reaches only a step that is still calling tools, and `TELL_AFTER_TURNS`
holds it back until the eighth turn. Every failing run ended in prose at turn
four and was therefore told nothing at all.

A step that stops having changed nothing is now told once and given one further
turn; a second empty answer fails as before. Gates green — clippy clean, build
ok, 823 tests passing.

### What happens now

37 of 101 runs journalled this outcome:

> `read and reported, but changed nothing — no file was written, patched or deleted`

The model reads the workspace, reasons about it, states a plan, and calls no
writing tool. Perpetum detects this exactly right and fails the step. Then the
batch ends with `L-14`'s exhausted backlog and the run is over.

Nothing retried it, and nothing told the model.

| | Tasks | Mean completion |
|---|---|---|
| Journalled a no-op step | 37 | **0.465** |
| No failure signature at all | 43 | **0.780** |

`004-meeting-summary`, `011-code-debug` and `014-task-decomposition` are the
clean cases: four model calls, eleven to fifteen seconds, zero.

### Why the current behaviour is defensible and still wrong here

`L-11` says repeating an unproductive thing is an error rather than a retry, and
`cycle.rs` is explicit that a second attempt belongs to the next cycle. For an
unattended loop that is the right rule — it is what stops a wedged model burning
a budget on the same failing step all night.

But a first attempt that produced nothing is not a repetition. The model was
never told it had failed. Re-asking it with the no-op as input is not repeating
an unproductive thing; it is the first time the loop says anything back.

### Proposed change

Feed the detection back as data, the same way a tool result comes back — one
further turn within the step, carrying the verbatim outcome text. If that turn
also changes nothing, fail the step as now: the second no-op *is* the repetition
`L-11` refuses.

Failing that, the smaller change is a CLI affordance — `perp run --requirement
<id> --attempts <n>` — so a caller can ask for the second cycle explicitly
instead of re-invoking the binary and re-taking the write lock.

Prefer the first. A harness that can see the failure and cannot mention it is
leaving its own best diagnostic unused.

### Cost

Bounded by one extra turn per failed step. Highest expected return of anything
in this document.

---

## 2. The workspace boundary refuses paths that are inside the workspace

**Cited:** `X-2`, `X-13`

### What happens now

50 refusals across 35 tasks under `X-13`, plus 13 occurrences across 7 tasks
under `X-2`. Two distinct defects, both in
[`perp-core/src/tool.rs`](../crates/perp-core/src/tool.rs).

**MSYS-style paths are not recognised.** The model writes

```
shell find /d/repos/harness-bench/.../workspace -type f
```

and is refused with ``is outside the workspace, which needs an approval
(`X-13`)`` — for a path that is the workspace. 20 of the 50 refusals are this.
`Host::resolve` canonicalises and compares against the root, and
`/d/repos/...` never becomes `D:\repos\...` on the way.

**Arguments that are not paths are tested as paths.** From a real transcript:

```
refused `shell curl -s -w ...`: `\nHTTP:%{http_code}\n` is outside the
workspace, which needs an approval (`X-13`)
```

A curl format string is not a path and cannot be one.

### Why this matters more unattended than the code assumes

The comment on `shell_looks_like` says the check is *"deliberately blunt and
deliberately over-broad: a false positive costs one approval request, a false
negative costs a production deploy."* That trade is correct when somebody is
there to approve. Unattended — which is the whole design target — a false
positive costs the call outright, and the model spends its next turn working
around a refusal that should never have fired.

`023-web-form-extraction` has the most of these and scores **0.05 completion
against 0.17 process**: the judge saw the flailing the refusals caused.

### Proposed change

1. Normalise `/<drive>/...` to `<DRIVE>:\...` before the boundary test, on
   Windows only. One function, applied in `resolve` and in the `X-13` command
   scan.
2. Skip candidates that cannot be paths — an argument containing `%{`, or an
   escape sequence, or one that follows a flag known to take a format string.
   Narrowing what counts as a candidate does not weaken the boundary; every
   real path still gets tested.

Keep the bluntness for anything genuinely ambiguous. The point is not to relax
`X-13`, it is to stop it firing on things that are not paths at all.

---

## 3. Refuse the checkpoint loudly when the workspace is not its own repository

**Cited:** `T-22`, `G-5`

### What happens now

13 tasks logged:

> `checkpoint commit failed: workspace path data_try6 is ignored by .gitignore, so git add refused`

The benchmark sandbox sits inside another project's git repository. Perpetum
walks up, finds that repository, and tries to commit into it. The commit fails
because the sandbox path is ignored there.

Two things follow, and the second is worse than the first. The git harness is
silently disabled for the whole run — `T-22`'s per-step checkpoint never lands.
And `perp`'s git tooling is aimed at a repository that is not the workspace and
not ours to write to. Nothing was damaged here, because the ignore rule happened
to refuse the `git add`. That is luck, not a boundary.

### Proposed change

`perp bind` already checks every path the binding names and exits non-zero if
one is missing. It should also resolve the git root and compare it to the
binding root. If they differ, say so there — at bind time, once, in the place a
person is already reading — rather than failing per-step later with a message
about someone else's `.gitignore`.

Whether a foreign repository should be a hard bind failure or a declared-and-
accepted condition is a design decision, not a bug fix, and belongs in the
requirements source before it is built.

---

## 4. The first-token deadline belongs in the binding

**Cited:** `M-23`, `M-14`

### What happens now

```rust
// perp-core/src/stream.rs
pub const FIRST_TOKEN_SECONDS: u64 = 20;
```

Two tasks — `093-jsonl-sessionization-analysis` and
`094-metric-definition-migration-diff` — were blocked by it:

> `no link answered: binding key role.coder: every link failed: deepseek — said nothing for 20s — failed over rather than waited on (M-23)`

`094` scores **0.04 completion against 0.00 process**, which is the judge
agreeing that nothing happened.

### Why it should move

`M-14` says the harness must not hard-code volatile provider facts, and
time-to-first-token under load is exactly that. The doc comment reasons the
twenty seconds out carefully — longer than a cold local model, far shorter than
the request timeout — and that reasoning is sound for the rigs it was written
against. It is still a provider fact compiled into the binary.

### Proposed change

A binding or link key — `link.<name>.first_token_seconds` — defaulting to the
current 20. The constant stays as the default; it stops being the only option.

Half of this failure was configuration on the caller's side: `role.coder` named
a single link, so a failover had nowhere to go. Roles are chains by design and
the benchmark harness should have used one. That half is fixed in the adapter,
not here.

---

## 5. Image content in the model client

**Cited:** `M-21`

### What happens now

There is no image path in `perp-core`. The client sends text; `cli_prompt`
flattens a conversation to a string. `008-image-recognize` and `013-image-edit`
score **0.00**, having been asked to describe pictures they could never receive.

Their process scores are **0.88 and 0.70** — the model used its tools sensibly
on a task the harness structurally cannot do.

### The finding that argues for fixing it

The same two tasks scored **1.00 under a `claude-cli` link**, on both Sonnet and
Opus.

Not because those models are better. Because a `claude-cli` link runs the CLI as
a subprocess, and the CLI reads image files with *its own* tools — so Perpetum
inherited a capability through the subprocess boundary that it does not itself
possess. The gap is entirely in `perp`'s own model client.

### Proposed change

Image content blocks on the OpenAI-compatible and Anthropic paths, for links
whose model accepts them. Larger than everything above and worth scoping
separately; the note here is that the requirement is now measured rather than
supposed.

---

## Priority

| | Fix | Tasks affected | Size |
|---|---|---|---|
| 1 | Second attempt after a detected no-op | 31 | small |
| 2 | Path normalisation and candidate narrowing in `X-13`/`X-2` | 11 | small |
| 3 | Foreign-repository check at bind time | 13 | small |
| 4 | First-token deadline as a binding key | 2 | trivial |
| 5 | Image content in the model client | 2 | large |

Counts are tasks whose shortfall was *attributed* to that cause. 36 further
shortfalls were the model's, on runs where the harness recorded no fault of its
own, and no change here would move them.

---

## Not our bugs

Recorded so the next run does not re-diagnose them.

**Six tasks were graded by a grader that could not run.** Ten oracles verify by
shelling out to `python3 -m pytest` — `016`, `039`, `040`, `042`, `044`, `045`,
`083`, `085`, `087`, `088`. On Windows `python3` resolves to the Microsoft Store
stub, which has no pytest, while `pytest 9.0.2` is installed and working for the
Python actually on PATH.

Six of them recorded the failure explicitly — `040`, `042`, `044`, `045`, `083`,
`085` — costing up to **3.98 points**. `088` never ran at all (below). The
remaining three did not surface the error in their check details, so whether
they reached the pytest path is unestablished.

On `040` the workspace holds a 6.9 KB test file `perp` wrote and `__pycache__`
entries stamped `pytest-9.0.2`: the work was done and the grader failed. A
HarnessBench portability bug, affecting every harness equally.

**Five tasks never ran.** `003`, `006`, `078`, `081`, `088` need a public URL for
a local mock server. No tunnel is configured on the test machine, so their setup
hook raised before `perp` was invoked.

**Two bugs were in the benchmark adapter and are already fixed.** A usage file
named from the session id overflowed Windows `MAX_PATH` and discarded seven
completed runs; and `subprocess.run(timeout=...)` did not enforce its timeout
against a `claude-cli` link, because grandchildren inherit the pipe — one task
ran 235 minutes against a 40-minute limit. Both are in the adapter, not in
`perp`.
