# Harness-Bench findings — planned fixes to `perp`

What a full Harness-Bench run found wrong with this harness, and what to change.

**This document mints no requirement ids.** Ids are minted in
[`.harness/perpetum.md`](../.harness/perpetum.md) and nowhere else
(`V-9`); everything below cites existing ones. When a fix here is taken up, its
id goes in the requirements source first and this document becomes commentary on
it.

**Status: all five fixes are implemented and gated.** Every
number below is measured from a run that has already happened, not projected.

**Two of the numbers below were wrong, and both corrections are recorded in
place.** Fix 2's refusal count was tasks that *recorded* a refusal rather than
tasks a refusal harmed — 11 became 2. Fix 5 named `013-image-edit` as a vision
failure on the strength of its name; its journal said `no-op-step`, and it
scored 1.00 on a text-only link once that was fixed.

**And the largest finding here has a cause underneath it.** The no-op signature
that fix 1 was written from — 37 tasks at mean 0.465 — was substantially an
artifact: the benchmark's sandboxes lived inside another project's checkout,
whose `.gitignore` matched them, so `T-29` discarded every write from `touched`
and `V-13` recorded "changed nothing" for steps that had written plenty. That is
filed as `T-33` and fixed. Fix 1 remains a real gap in a real mechanism; its
evidence was contaminated.

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

**Cited:** `X-2`, `X-13` · **filed as `X-15`** · **implemented**

> **This section overstated the problem by an order of magnitude, and the
> correction is the more useful finding.** It claimed 20 of 50 refusals were
> paths wrongly refused. Resolving every refused token against its own
> workspace root says otherwise: of **59** refusals, **2** were wrong, 1 was a
> format string, and **56 were correct** — ancestors of the workspace, `..`,
> `/`, and fragments like `/s` that name nothing.
>
> The two defects below are real and are now fixed, because refusing a path for
> being spelled differently is wrong however seldom it happens. But `X-13` was
> not costing 11 tasks their scores, and the priority table's count for this row
> was reading "tasks that recorded a refusal" as "tasks a refusal harmed". Most
> of those refusals were the boundary working.

### What happens now

Two distinct defects, both in
[`perp-core/src/tool.rs`](../crates/perp-core/src/tool.rs).

**MSYS-style paths are not recognised.** The model writes

```
shell find /d/repos/harness-bench/.../workspace -type f
```

and is refused with ``is outside the workspace, which needs an approval
(`X-13`)`` — for a path that is the workspace. `Host::resolve` canonicalises and
compares against the root, and `/d/repos/...` never becomes `D:\repos\...` on
the way; Windows reads the leading slash as rooted on the current drive, so the
two spellings of one place land in different ones.

**Two** refusals across the run, on `097-research-claims-batch-evidence-audit`
and `098-three-source-decision-record-synthesis`. Every other `/d/...` token
refused was a genuine ancestor of the workspace and stays refused after the fix.

**Arguments that are not paths are tested as paths.** From a real transcript:

```
refused `shell curl -s -w ...`: `\nHTTP:%{http_code}\n` is outside the
workspace, which needs an approval (`X-13`)
```

A curl format string is not a path and cannot be one. One refusal across the run.

### Why this matters more unattended than the code assumes

The comment on `shell_looks_like` says the check is *"deliberately blunt and
deliberately over-broad: a false positive costs one approval request, a false
negative costs a production deploy."* That trade is correct when somebody is
there to approve. Unattended — which is the whole design target — a false
positive costs the call outright, and the model spends its next turn working
around a refusal that should never have fired.

That argument still holds. What it does not license is the claim this section
originally made next — that `023-web-form-extraction`, at **0.05 completion
against 0.17 process**, was scored down by refusals that should not have fired.
Its refusals were all correct ones. The step was reaching outside the workspace
and the boundary said no; the low process score is the judge watching a step
flail, not the harness causing it.

The two wrong refusals were on `097` and `098`, both of which scored above the
run's mean anyway. Nothing here bought back a score.

### The change

1. `msys_drive_path` reads `/<drive>/...` as `<DRIVE>:\...`, applied in
   `Host::resolve` — which `confined` already routes through, so both `X-2` and
   `X-13` get it from one place. Windows only: on a real POSIX system
   `/d/repos` is an ordinary absolute path and rewriting it would be this same
   bug pointed the other way. Normalising decides nothing about where a token
   goes; it is still resolved and compared exactly as before.
2. `looks_like_path` skips tokens carrying `%{` or `%(`.

**Brace forms only, and deliberately not the backslash escapes in the same
string.** `\n` and `\t` cannot be told apart from the start of `new/` and
`tests/` in a Windows path, so a rule that read them as escapes would let real
paths through. `X-13` erring toward refusal is the trade the whole check is
built on, and the narrowing does not touch it.

Keep the bluntness for anything genuinely ambiguous. The point is not to relax
`X-13`, it is to stop it firing on things that are not paths at all.

---

## 3. Refuse git when the repository is not the workspace

**Cited:** `T-22`, `G-5`, `G-1` · **filed as `G-18`** · **implemented**

> **Verified, and worse than this section claimed.** It said the checkpoint
> "silently failed" 13 times. It also silently *succeeded*: **fourteen commits
> landed on the other project's `main`**, unattended and unapproved, each
> carrying a bench task's output files. The failures were the runs where a
> `.gitignore` happened to refuse the `git add` — luck, not a boundary.
>
> Score impact remains **nil**: the oracle grades files, not git. The severity
> is entirely a safety one, and higher than a bind-time warning answers. So the
> fix refuses rather than warns.

### What happens now

15 tasks logged a failed commit or a gitignore refusal; 13 of them this:

> `checkpoint commit failed: workspace path data_try6 is ignored by .gitignore, so git add refused`

The benchmark sandbox sits inside another project's git repository. Perpetum
walks up, finds that repository, and tries to commit into it. The commit fails
because the sandbox path is ignored there.

Two things follow, and the second is worse than the first. The git harness is
silently disabled for the whole run — `T-22`'s per-step checkpoint never lands.
And `perp`'s git tooling is aimed at a repository that is not the workspace and
not ours to write to. Nothing was damaged here, because the ignore rule happened
to refuse the `git add`. That is luck, not a boundary.

### The change

`Host::own_repository` resolves `git rev-parse --show-toplevel` and refuses
unless it equals the workspace root, naming both paths. Applied at `Tool::Git`
and — because `shell git commit` is a git call wearing a shell — at `Tool::Shell`
whenever `invokes_git` says the program is git. Both were used in the run: a
model refused at `git(args=…)` reached for `shell(command=git -C <path> …)`.

Reads are refused too. A `git status` against the wrong repository answers with
thousands of unrelated files, and a model that believes that answer is worse off
than one told no.

Equality, not ancestry. A workspace that is a subdirectory of its own repository
is refused as well, deliberately: that may be a legitimate layout, and a person
should say so rather than the harness assume it from a path relationship it
cannot tell apart from this one.

---

## 4. The first-token deadline belongs in the binding

**Cited:** `M-23`, `M-14` · **filed as `M-35`** · **implemented**

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

### The change

`link.<name>.first_token_seconds`, defaulting to the same 20. The constant stays
as the default and stops being the only option. Per link rather than global: a
laptop model and a hosted API do not share a sensible number, and the three call
sites in `client.rs` all had the link in hand already.

Refused rather than clamped when it cannot work — a non-numeric value, or zero,
is a typo far more often than an intention.

Half of this failure was configuration on the caller's side: `role.coder` named
a single link, so a failover had nowhere to go. Roles are chains by design and
the benchmark harness should have used one. That half is fixed in the adapter,
not here.

---

## 5. Image content in the model client

**Cited:** `M-21` · **filed as `M-36`** · **implemented, partly**

> **This row named two tasks and only one of them belonged to it.** It read
> `008-image-recognize` and `013-image-edit` as a matched pair of vision
> failures. `013` was not one: its zero carried the `no-op-step` signature —
> `T-30`'s contamination — and once that was fixed it scored **1.00** on a
> text-only link, with no vision configured anywhere. It was filed here on the
> strength of its name rather than its journal.
>
> `008` is the real case and remains open at **0.00**: it asks what is *in* a
> picture, which nothing but a model looking at it can answer.

### What happens now

There was no image path in `perp-core`. The client sent text; `cli_prompt`
flattens a conversation to a string, so a `read` of a PNG failed on invalid
UTF-8 and a model asked to look at a picture got a decoding error.

### What each of the two actually needed

**`013-image-edit` needed nothing from this row.** Its oracle asks for two PNGs
that exist, differ from their originals, and are described — all of which a
model satisfies by driving PIL through the shell, which is exactly what it did:

```
shell :: python -c "from PIL import Image; im = Image.open(...)"
```

Its 0.00 was `touched` being empty, not sight being absent.

**`008-image-recognize` needed sight and still does.** No link in the benchmark
configuration declares `vision = true`, and the model under test is not
vision-capable, so the plumbing this row built has nothing to carry. Reaching it
needs a vision model in the role chain, not another code change.

### The finding that argued for fixing it

Both tasks scored **1.00 under a `claude-cli` link** while scoring 0.00 on an
API link — and that contrast is what made them look like one problem.

For `008` the reading holds: a `claude-cli` link runs the CLI as a subprocess,
and the CLI reads image files with *its own* tools, so Perpetum inherited a
capability through the subprocess boundary that it did not itself possess.

For `013` it was a coincidence. The CLI link happened to succeed at a task the
API link was failing for an unrelated reason, and two zeros side by side read as
a pattern. A signature in the journal would have said otherwise, and did.

---

## Measurement caveats

Things that are true about how these numbers were produced, recorded so a later
run is not compared against them in ignorance.

### Reasoning effort is undeclared on both `claude-cli` backends

A three-way run — `deepseek-v4-flash` over an API link, and Opus and Sonnet over
`claude-cli` links — was made with harness entries that declare **no `effort`**:

```
link.claude.kind = claude-cli
link.claude.model = opus
```

`cli_invocation` passes `--effort` only when a link declares one, so both Claude
backends ran at whatever the CLI defaults to. `M-34` exists precisely to stop
this, and its own test says why:

> Nothing passed `--effort`, so every call ran at whatever the CLI defaulted to
> that week — and two runs a month apart were not comparable without anyone
> being able to see why.

**The asymmetry is the part that matters.** DeepSeek's reasoning level is fixed
by the API and does not float; the two Claude backends' does. So a three-way
comparison holds one leg still and lets two drift, and a re-run can move for a
reason nothing in the results records.

The numbers stand as a snapshot. They are not reproducible in the sense `M-34`
means, and should not be quoted as a stable ranking of the three backends.

Not fixed here, deliberately: setting `effort` would have invalidated a run that
was already hours in. The adapter already supports it — any scalar link key in
the harness config reaches `links.md` — so the fix is one line of configuration
whenever a comparable run is wanted, plus whatever surface the panel grows for
it. `perp` needs nothing new; `M-34` is already built.

---

## Priority

| | Fix | Tasks affected | Size | State |
|---|---|---|---|---|
| 1 | Second attempt after a detected no-op | 31 | small | done, `L-35` |
| 2 | Path normalisation and candidate narrowing in `X-13`/`X-2` | **2** | small | done, `X-15` |
| 3 | Refuse git outside the workspace repository | 15 | small | done, `G-20` |
| 4 | First-token deadline as a link key | 2 | trivial | done, `M-37` |
| 5 | Image content in the model client | **1** | large | built, `M-38`; `008` still needs a vision model, `013` never belonged here |

Row 2 read **11** until its refusals were resolved against their own workspace
roots. That count was tasks which *recorded* a refusal, not tasks a refusal
harmed — 56 of 59 were the boundary working correctly. The others in this table
have not been checked that way and may be softer than they look for the same
reason.

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
