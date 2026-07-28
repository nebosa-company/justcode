# Perpetum journal — cycle 1

Append-only. Each record: intent, outcome, evidence. Gate transcripts are
verbatim — command, exit code, output tail (`V-2`). When the state file and this
file disagree, this file wins (`L-4`).

Operator: Claude Code session (Opus 5), per [`binding.md`](binding.md).

---

## Phase A — Initiation

### c1/A/s01 — Bind the project
- **intent:** create the binding so nothing else runs unbound (Perpetum 0.1).
- **outcome:** [`binding.md`](binding.md) written. Branch `perp/c1/init` created
  off `main` (`G-1`); `main` untouched.
- **evidence:** `git switch -c perp/c1/init` → `Switched to a new branch
  'perp/c1/init'`.

### c1/A/s02 — Write the vision
- **intent:** produce `vision.md` including a "will never be" list, without
  which C.2 cannot detect a conflict.
- **outcome:** [`../initiation/vision.md`](../initiation/vision.md) written with
  10 never-clauses.

### c1/A/s03 — Write the NFRs
- **intent:** the engineering standard the loop holds batches to.
- **outcome:** [`../initiation/nfrs.md`](../initiation/nfrs.md) written, 10
  sections + 3 marked N/A with reasons.

### c1/A/s04 — Fix the requirements-source link
- **intent:** the requirements doc was moved to `docs/` by the operator; its
  link to the concept doc pointed at a path that no longer resolves.
- **outcome:** `[Perpetum](../perpetum.md)` → `../../perpetum.md`.

### c1/A/s05 — Toolchain discovery (`X-3`)
- **intent:** record what the gates will actually run on, before depending on it.
- **outcome:**

  ```
  cargo 1.97.1 (c980f4866 2026-06-30)
  rustc 1.97.1 (8bab26f4f 2026-07-14)
  clippy 0.1.97 (8bab26f4f6 2026-07-14)
  node v24.8.0
  npm 11.6.0
  ```

  clippy is present, so the lint gate is runnable rather than blocked.

**Phase A exit:** vision, NFRs, binding and state all exist; the binding
resolves every path Perpetum names. ✅

---

## Phase B — Requirements gathering

### c1/B/s06 — Crash analytics + analytics sources
- **intent:** determine whether either source can be read (Perpetum B).
- **outcome:** both **unavailable**. No crash reporter and no analytics SDK
  exists anywhere in the repo, and the harness has never run.
- **evidence:** case-insensitive search for
  `sentry|crashlytics|bugsnag|rollbar|amplitude|posthog|mixpanel|google-analytics|gtag|telemetry`
  across the repo (excluding `node_modules`) returned 5 files, every hit a false
  positive — `scrollbar` matching `rollbar`, `matchClosingTags` matching `gtag`,
  and a dictionary word list. Zero real integrations.

### c1/B/s07 — User voice
- **intent:** read the issue tracker.
- **outcome:** GitHub issues **unavailable this cycle**; the operator's own
  requests in the originating session **are** available and were captured.
- **evidence:**

  ```
  $ gh issue list --repo nebosa-company/justcode --limit 20
  GraphQL: Could not resolve to a Repository with the name
  'nebosa-company/justcode'. (repository)

  $ gh auth status
  ✓ Logged in to github.com account gaddlord (keyring)  — active
  ✓ Logged in to github.com account nebosa-company (keyring) — inactive
  ```

  The repo is private to `nebosa-company` and the active `gh` account cannot see
  it. Switching accounts is a settings change and is not done unattended —
  recorded as unavailable with the fix, not silently skipped.

### c1/B/s08 — Remaining sources
- **outcome:** support and market read; backlog read in full; NFRs read and
  three new requirements minted from them.
- **evidence:** `docs/requirements/*/pass-2026-07-28.md`.

**Phase B exit:** every source read or marked unavailable, dated. ✅

---

## Phase C — Prioritisation

### c1/C/s09 — Reconcile against reality (C.1, Perpetum 0.7)
- **intent:** check every backlog candidate against the code before letting it
  compete for a slot.
- **outcome:** **nothing in the backlog is built.** `crates/` does not exist; no
  Rust source outside `src-tauri/`; no reference anywhere in the tree to a
  journal, a link router or a gate runner. Markers and reality agree.
- **evidence:** `grep -oE '^\| \`[A-Z]+-[0-9]+\`' docs/perpetum.md | wc -l` → 141
  before minting; repository listing shows no `crates/`.
- **note:** this is the one cycle where "all open" can be believed cheaply. C.1
  in cycle 2 must re-check rather than trust this record.

### c1/C/s10 — Conflict check (C.2)
- **intent:** compare every requirement against the vision.
- **outcome:** 2 conflicts found and parked in
  [`../prioritization/conflicts.md`](../prioritization/conflicts.md):
  `I-3` (panel scope vs "never an IDE") and `O-6` (notification reply path vs
  "never instruction-taking from content"). Both quote the vision line they
  contradict, name the trade-off, and carry a recommended option.
- **outcome:** neither blocks the cycle; neither appears in batches 1–10.

### c1/C/s11 — Mint from C.1 (`L-21`, `L-22`)
- **intent:** two gaps surfaced by reconciling — nothing in the backlog said the
  engine must load the binding, and nothing gave step ids a shape, which `G-2`'s
  commit trailers and `/explain` both depend on.
- **outcome:** `L-21` and `L-22` filed in the requirements source. Total 146.

### c1/C/s12 — Score, classify, batch (C.4–C.7)
- **outcome:** 10 batches written to
  [`../prioritization/batches.md`](../prioritization/batches.md), 85
  requirements batched, 61 left competing in cycle 2.
- **the disagreement, stated per C.7:** strict score order puts batch 10
  (artifacts, 1600) first and batch 1 (the spine, 400) near the bottom. Built in
  that order the loop would render a progress board from a journal that does not
  exist. Dependency order wins; the scores still decide the model layer (600)
  ahead of the tool host (160).

**Phase C exit:** 10 batches written; conflicts parked and asked; state updated. ✅

---

## Phase D — Batch 1, the spine

Branch `perp/c1/b1`. Nine requirements, built in dependency order. From `c1/b1/s13`
onward the records are also in `journal.jsonl`, written by the engine itself.

### c1/b1/s01–s09 — Write the spine
- **intent:** implement batch 1: binding loader, step ids, journal, projection,
  atomic writes, typed errors, versioned records.
- **outcome:** `crates/` created as its **own cargo workspace**, separate from
  `src-tauri/` so the editor's build is untouched. `perp-core` (library) and
  `perp` (CLI), **zero third-party dependencies** — the binding makes adding a
  crate an approval step, and a hand-written JSON writer and scanner cost less
  than the approval would (`N-11`).
- **files:** `error.rs` `json.rs` `time.rs` `atomic.rs` `step.rs` `binding.rs`
  `journal.rs` `state.rs` `lib.rs`, `perp/src/main.rs`, `tests/spine.rs`.

### c1/b1/s10 — Lint gate, first run: FAILED
- **outcome:** `cargo clippy --workspace --all-targets -- -D warnings` → **exit 101**.

  ```
  error: used `expect()` on a `Result` value
    --> perp-core\tests\spine.rs:19:5
     = note: requested on the command line with `-D clippy::expect-used`
  error: could not compile `perp-core` (test "spine") due to 4 previous errors
  ```

- **cause:** clippy's `allow-expect-in-tests` covers `#[test]` functions; the
  fixture helpers in `tests/` are plain functions beside them, so the exemption
  did not reach them.
- **fix:** `#[allow(clippy::expect_used)]` on the two helpers, with the reason in
  a comment. The policy was not weakened — `unwrap_used`/`expect_used` remain
  `deny` for all library code, which is what `N-9` is about.
- **attempt 1 of 2** (Perpetum 0.5). Resolved on the first attempt.

### c1/b1/s11 — Gates, all three
- **outcome:** green.

  ```
  $ cargo clippy --workspace --all-targets -- -D warnings
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.31s
  exit 0

  $ cargo build --workspace
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.90s
  exit 0

  $ cargo test --workspace
  test result: ok. 48 passed; 0 failed   (perp-core lib)
  test result: ok.  5 passed; 0 failed   (tests/spine.rs)
  exit 0
  ```

### c1/b1/s12 — Red run
- **intent:** Perpetum gate 4 says the tests must *fail without the change*.
  `V-3` is batch 5, so this is the manual form: mutate the implementation, run
  that feature's test, confirm red, restore.
- **outcome:** four of four went red.

  ```
  step.rs     drop the seq tiebreak from Ord   orders_by_cycle_then_sequence          101
  journal.rs  .append(true) -> .truncate(true) appending_never_rewrites_what_is_there 101
  time.rs     div_floor -> integer division    handles_before_the_epoch               101
  binding.rs  !path.exists() -> false          verify_names_the_input_that_is_missing 101
  restored                                     53 passed, exit 0
  ```

- **honest scope:** four of the 53 tests were red-run, chosen as the
  highest-risk behaviours. The other 49 were not. Automating this for every
  feature is `V-3`, in batch 5.

### c1/b1/s13 — Exercise the real artefact (Perpetum 0.7)
- **outcome:** `perp bind --root ..` resolved all 7 inputs `[ok]` and both
  outputs `[to be written]`, exit 0. `perp record` appended four records to
  `docs/perpetum/journal.jsonl`. `perp state --out -` replayed them into the
  projection — 2 done, 0 blocked, 9 requirements cited.

### c1/b1/s14 — What is *not* done
- `L-4` and `L-8` are **🟡 in progress**, not done. The projection exists and is
  tested, but "rewritten after each outcome" needs the loop engine (batch 3) to
  call it, and "context is disposable" is an invariant to re-assert every batch
  rather than a feature that finishes. Marking them done would be the exact
  failure this harness exists to prevent.
- `docs/perpetum/state.md` is still the operator's hand-written file. The engine
  can render a projection but cannot yet represent parked conflicts or gated
  items, so it was **not** pointed at that path this cycle.

**Batch 1 status:** 7 of 9 delivered, 2 carried, 0 blocked, 0 gated. All gates
green at the delivered set.

---

## Phase D — Batch 2, gate runner and evidence

Branch `perp/c1/b2`. Six requirements: `V-2` `T-3` `L-16` `X-4` `X-12` `T-4`.

### c1/b2/s01–s04 — Write the runner
- **outcome:** two modules. `process.rs` — bounded runs, declared environment,
  tree kill, and a nursery that kills what a step spawned. `gate.rs` — gates
  read from the binding, a result type that *contains* its transcript, and the
  attempt counter.
- **two design decisions worth recording:**
  - `Spec` has no constructor without a timeout. `T-3` says no unbounded
    process, ever, and a type is a better place to enforce that than a memo.
  - `GateResult` cannot be built without a `Run`. There is no code path that
    records a pass without the transcript that proves it — which is what `V-2`
    actually asks for.
- **output is drained on threads** rather than read after waiting: a child that
  fills a pipe while the parent polls would block forever, and a deadlock is not
  a timeout.

### c1/b2/s05 — Gates
- **outcome:** green on the first run this time.

  ```
  cargo clippy --workspace --all-targets -- -D warnings -> exit 0
  cargo build --workspace -> exit 0
  cargo test --workspace -> exit 0; 66 unit + 5 integration = 71 passed, 0 failed
  ```

  One warning was fixed before the lint gate ran (`unused import: Path`), which
  the build reported.

### c1/b2/s06 — Red run, and the harness running its own gates
- **red run:** four of four went red.

  ```
  process.rs  drop kill_tree in the deadline path  a_command_that_hangs_is_killed_at_its_deadline          101
  process.rs  drop env_clear()                     the_environment_is_declared_not_inherited              101
  gate.rs     never reach the attempt ceiling      two_failures_block_and_the_third_attempt_is_not_offered 101
  gate.rs     do not stop at the first red gate    runs_stop_at_the_first_red                             101
  restored                                         71 passed, exit 0
  ```

  The first mutation is the interesting one: removing the kill still returns
  `TimedOut`, so the test that only checked the verdict would have passed. It
  fails because it also checks the clock — the process really has to die.

- **artefact exercised (Perpetum 0.7):** `perp gate all --root .. --step c1/b2/s06`
  ran clippy, build and test **through the harness**, exit 0, and appended three
  outcome records to `journal.jsonl` carrying the verbatim transcripts —
  command, cwd, `env: declared: kept 16, set 0`, exit code, duration. The
  harness now produces its own evidence.

### c1/b2/s07 — What is *not* done
- `V-2` is **🟡**: the transcript carries command, cwd, env, exit, duration and
  output tail, but not the **commit sha**. `GateResult.sha` exists and is
  `None`; filling it is `G-6`, in batch 4.
- `X-4` is **🟡 ⛔**: the tree kill works — `taskkill /F /T` on Windows, a
  process-group kill on Unix — but the requirement's stronger clause, that
  children die *even if the engine is killed*, needs a Windows **job object**,
  which needs the `windows-sys` crate. Adding a dependency is approval-gated
  under this project's binding, so it is **requested, not taken**. Carried as
  `approval-gated` (Perpetum 0.6).

**Batch 2 status:** 4 of 6 delivered, 2 carried (1 of them approval-gated),
0 blocked. Gates green.

---

## Phase D — Batch 3, recovery and watchdogs

Branch `perp/c1/b3`. Nine requirements plus `L-4`, carried from batch 1.

### c1/b3/s01–s06 — Write the recovery layer
- **outcome:** `session.rs` — a session opens from the binding and the journal
  and nothing else, hands out step guards, reconciles what a dead process left,
  and stops clean. `watchdog.rs` — the three ways a loop burns tokens without
  moving.
- **the decision that matters:** a step whose intent carries no idempotence flag
  is treated as **unsafe to repeat**. Absent evidence, the conservative reading
  is the safe one — repeating a non-idempotent step blind is how a loop sends
  the same email twice.
- `StepGuard` writes the outcome on `close`. Dropping it without closing leaves
  the intent open **on purpose**: that is what a crash looks like, and hiding it
  would defeat `L-7`.

### c1/b3/s07 — Test gate, first run: FAILED
- **outcome:** `cargo test --workspace` → **exit 101**, 1 of 87 failed.

  ```
  ---- watchdog::tests::a_file_edited_back_to_an_earlier_state_is_thrash ----
  assertion `left == right` failed
    left: Stop { reason: "src/main.rs has been edited back to an earlier state 2 times" }
   right: Continue
  ```

- **diagnosis:** the **test** was wrong, not the code. `one → two → one → two` is
  *two* reverts, so the watchdog stops on the fourth write; the test expected
  the fifth. The behaviour under test is unchanged and still asserted — the
  expectation was corrected, and the reasoning is in the test as a comment so
  the next reader does not "fix" it back.
- **attempt 1 of 2.** Resolved on the first attempt.

### c1/b3/s08 — Gates
- **outcome:** green, run through the harness itself.

  ```
  $ perp gate all --root .. --step c1/b3/s08
  gate: lint   exit 0 in 192ms
  gate: build  exit 0 in 129ms
  gate: test   exit 0 in 840ms
  all 3 gates green
  ```

### c1/b3/s09 — Recovery drill on the real journal (Perpetum 0.7)
- **intent:** prove `L-7` against the actual repository, not a fixture.
- **outcome:**

  ```
  $ perp record c1/b3/s09 intent "drill: die between intent and outcome"
  recorded c1/b3/s09

  $ perp resume
  steps:   8 done, 0 blocked
  resume:  park c1/b3/s09: cannot tell whether it landed, and it is not safe to repeat
  exit 1
  ```

  The CLI's probe answers `Unclear` on purpose — from outside the loop there is
  no way to tell whether a step's effect landed, and the rules then park rather
  than guess. The drill step was then closed by the operator and `perp resume`
  returned to `nothing in flight`, exit 0.

### c1/b3/s10–s11 — Close `L-4`
- **outcome:** `StepGuard::close` now rewrites the state file from the journal
  after the outcome is on the record — never before, since a projection
  describing a step the journal has not accepted is exactly the disagreement
  `L-4` exists to prevent. `L-4` moves from 🟡 to ✅.

### c1/b3/s12 — Red run, and a false green in the red run itself
- **outcome:** five mutations, **two survived**, and both survivals were real
  findings rather than noise:

  ```
  session.rs   unsafe default -> safe default    a_step_with_no_recorded_idempotence…  101  red
  session.rs   idempotent branch never taken     an_unclear_step_parks_unless…         101  red
  watchdog.rs  never reset the quiet counter     any_progress_resets_the_count         101  red
  watchdog.rs  never trim the window             the_window_forgets_old_calls            0  SURVIVED
  session.rs   stop kills nothing                stopping_reports_what_it_had_to…        0  SURVIVED
  ```

  - `the_window_forgets_old_calls` passed with the window trimming disabled,
    because its limit was 3 and the scenario only ever produced 2 repeats. The
    test never tested the window. Rewritten with limit 2 in a window of 3, it
    now fails when trimming is removed.
  - `stopping_reports_what_it_had_to_clean_up` only ever ran with an empty
    nursery, so "kill nothing" passed. A second test now spawns a real child
    and asserts it is killed and that the stop reports itself unclean.

  Both re-run after the fix: **red, as they should be.**

- **and one more, about the red run itself:** a sixth mutation reported "still
  green" when in fact `sed` had silently matched nothing — the pattern spanned
  two lines. A no-op mutation is a false green wearing the costume of evidence.
  Filed as **`V-10`**: the red run must verify the mutation actually changed the
  file before believing either result.

**Batch 3 status:** 10 of 10 delivered (`L-5` `L-6` `L-7` `L-11` `L-12` `L-13`
`L-15` `N-1` `N-2`, plus `L-4` carried from batch 1), 0 blocked. Gates green:
90 unit + 5 integration = 95 tests.

---

## Phase D — Batch 4, the git harness

Branch `perp/c1/b4`. Eight requirements: `G-1` `G-2` `G-3` `G-4` `G-6` `G-10`
`G-13` `G-14`.

### c1/b4/s01–s04 — Write the harness
- **outcome:** `git.rs`. The rules are enforced by a classifier that runs
  **before** the process is spawned, so there is no path where a command runs
  and the policy is consulted afterwards. `Never` is not a strong `Approve`:
  an approval does not unlock `add -A`, and there is a test that says so.

### c1/b4/s05 — Test gate, first run: FAILED
- **outcome:** exit 101, 1 of 103.

  ```
  ---- git::tests::the_refusals_are_refusals ----
  git ["clean", "-fdx"] must be refused outright
  ```

- **diagnosis:** a **real bug in the code**, not the test. The classifier looked
  for `-x`, `-fd` and `-a` as whole arguments, so every combined short flag went
  straight past it — `clean -fdx`, and worse, `commit -am`, which is exactly the
  sweep-up-everything case `G-3` exists to stop.
- **fix:** a `short()` helper that looks *inside* a short-flag cluster, and
  `clean` refused in every form except a dry run — there is no combination of
  letters worth allow-listing. Test cases added for `commit -am`, `commit -n`,
  `push -uf`, `clean -f`.
- **attempt 1 of 2.**

### c1/b4/s06 — Test gate, second failure: FAILED
- **outcome:** exit 101, 9 of 104 — every test using the repository fixture.

  ```
  git rev-parse --abbrev-ref HEAD: exit 128:
  fatal: ambiguous argument 'HEAD': unknown revision or path not in the working tree
  ```

- **diagnosis:** adding the `G-1` refusal to `commit` made every commit ask what
  branch it was on, and `rev-parse --abbrev-ref HEAD` cannot answer before the
  first commit exists. The fixture's own first commit broke.
- **fix:** `git branch --show-current`, which answers on an unborn branch.
  "The repository is too new to have a branch" is not the same as "detached",
  and the old command could not tell them apart.
- **attempt 2 of 2** for this feature — one more and `G-1` would have been
  blocked per Perpetum 0.5. Recorded because that is how close it got.

### c1/b4/s07 — Gates
- **outcome:** green. 104 unit + 5 integration = 109 tests.

### c1/b4/s08 — Red run, now with `V-10` enforced
- **outcome:** four mutations red, and the fifth **caught by the new check**:

  ```
  git.rs  short-flag detection always false     the_refusals_are_refusals                101  red
  git.rs  approval check never fires            an_unapproved_push_is_refused…           101  red
  git.rs  drop the Requirement trailer          a_commit_is_traceable_in_the_repo…       101  red
  git.rs  protected-branch check never fires    readiness_refuses_a_protected_branch     101  red
  git.rs  (a pattern that does not exist)       head_sha_is_what_a_gate_would…    MUTATION DID NOT APPLY
  ```

  The last line is `V-10` working: the red-run harness now diffs the file before
  believing the result, so a mutation that silently matched nothing is reported
  as meaningless instead of as a passing test.

### c1/b4/s09 — `G-6` closes `V-2` (Perpetum 0.7)
- **outcome:** `perp gate` now pins every transcript to `git rev-parse HEAD`.

  ```
  gate: lint
  sha: 3c2d8f6336cd3700dffb7fe54957aea4d330aa88
  exit 0 in 167ms
  ```

  `V-2` asked for command, cwd, sha, exit code, output tail, duration and
  timestamp. The sha was the one field missing since batch 2; it moves to ✅.

### c1/b4/s10 — `G-13` conflicts with the binding
- **outcome:** marked **🔶 conflicting** and parked, not implemented.
- `G-13` says the harness never commits its own state. The binding declares
  `out.journal` **inside** the repository, and every batch so far has committed
  it — deliberately, because the journal is the evidence a reviewer reads and
  what makes `perp resume` work on a fresh clone.
- Found by implementing, not by reading. Full trade-off and three options in
  [`../prioritization/conflicts.md`](../prioritization/conflicts.md) as
  CONFLICT-3; the recommendation is to split the requirement rather than move
  the journal out of the repository.

**Batch 4 status:** 7 of 8 delivered, 1 conflicting, 0 blocked. Plus `V-2`
closed from batch 2. Gates green: 109 tests.

---

## Phase D — Batch 5, the honesty machinery

Branch `perp/c1/b5`. Eight requirements: `V-1` `V-3` `V-4` `V-7` `V-8` `V-9`
`G-7` `G-8`. The product thesis, and the last batch before Phase D's exit.

### c1/b5/s01–s05 — Write it
- **outcome:** `verify.rs`, plus four additions to `git.rs` (`history_mentions`,
  `tree_mentions`, `commits_for_step`, `with_stashed`).
- **`V-1`** looks in the working tree *and* the history: a grep answers "is it
  here", `git log -S` answers "was it here and taken out", and the second
  question is why a feature gets rebuilt after someone deliberately removed it.
  `may_implement` refuses to start without a recorded check.
- **`V-3`** returns a verdict, not a boolean: *earned*, *proves nothing*, *still
  broken*, or *the tree was never actually different* — the last one being
  `V-10` from batch 3, now a first-class outcome rather than a lesson.
- **`V-4`** reads a unified diff, because that is the artefact that exists while
  the change is still reversible. Deliberately blunt: a false positive costs a
  sentence, a false negative costs the point of the harness.
- **`V-7`** derives markers from evidence — an outcome that says "green" and
  carries no transcript is a claim, and lands as 🟡, not ✅.
- **`V-8`** counts gated and conflicting in their own columns, forever. There is
  no `done / total` anywhere in the type.
- **`with_stashed`** returns the stash ref even on success, and on a failed pop
  the error *names the ref* rather than leaving the operator to find their work.

### c1/b5/s06 — Gates and red run
- **gates:** green. 123 unit + 5 integration = 128 tests.
- **red run:** seven mutations. Five red first time; two inconclusive, and both
  for reasons worth writing down:
  - one mutation disabled only one of three conditions inside `is_assertion`, so
    the test legitimately still passed. **The mutation was too weak, not the
    test.** Re-run against the counter itself: red.
  - one mutation contained a `|`, which was the `sed` delimiter — it silently
    produced nothing. `V-10`'s diff check caught it and reported *"MUTATION DID
    NOT APPLY"* instead of a false green. Re-run with a different delimiter: red.

  This is the second time the `V-10` check has earned itself in two batches.

### c1/b5/s07 — Build gate FAILED: the harness cannot rebuild itself
- **outcome:** `perp gate all` → lint green, **build exit 101**.

  ```
  $ cargo build --workspace
  cwd: ..\crates
  error: failed to remove file `D:\repos\justcode\crates\target\debug\perp.exe`
  Caused by:
    Access is denied. (os error 5)
  ```

- **diagnosis:** the gates were being run *by* `crates/target/debug/perp.exe`,
  and Windows will not let `cargo` replace a running executable. Nothing was
  wrong with the code; the harness was standing on the file it was told to
  rebuild.
- **fix:** run from a copy outside the tree under build. Confirmed green
  immediately afterwards, same sha, in `c1/b5/s08`.
- **filed as `T-18`**, because a workaround an operator has to remember is a
  defect: the engine must not hold a lock on anything its own gates rebuild.
  This is the kind of requirement that only exists because the loop was run on
  itself.
- **attempt 1 of 2.**

### c1/b5/s08 — Gates, green, and `V-9` against the real documents
- **outcome:**

  ```
  gate: lint   sha: ffe5618…  exit 0 in 189ms
  gate: build  sha: ffe5618…  exit 0 in 578ms
  gate: test   sha: ffe5618…  exit 0 in 1153ms
  all 3 gates green

  $ perp check ids --root ..
  source:    ..\docs/perpetum.md
  defined:   147
  documents: 18
  no stray ids — everything cited is defined
  ```

  Every requirement id cited across eighteen documents in this cycle is defined
  in the requirements source. Perpetum 0.8 held for the whole cycle, and now
  there is a command that proves it rather than a habit that claims it.

**Batch 5 status:** 8 of 8 delivered, 0 blocked. Gates green: 128 tests.
One requirement discovered and filed (`T-18`).

**Phase D exit:** 5 batches delivered. 38 requirements done, 2 in progress,
3 conflicting, 1 approval-gated, 0 blocked.

---

## Phase E — Release

Ordered so the irreversible steps come last. Version **0.1.0**.

### c1/E/s01 — Version and release notes
- **outcome:** [`release-notes.md`](release-notes.md) written, including the two
  formats that will be treated as breaking if they change — the journal record
  shape and the step-id format.

### c1/E/s02 — Localisation and accessibility
- **outcome:** both **N/A**, per [`nfrs.md`](../initiation/nfrs.md): a CLI with
  no interface and no user-facing strings beyond English help text. Recorded
  rather than skipped, because "not applicable" and "not done" look identical
  in a report that omits them.

### c1/E/s03 — Security scan
- **outcome:** the dependency surface is **empty**.

  ```
  $ cargo tree
  perp v0.1.0
  └── perp-core v0.1.0

  $ grep -c '^\[\[package\]\]' Cargo.lock
  2
  ```

- `cargo-audit` is **not installed** and was not installed unattended. With zero
  dependencies it would have nothing to report, but the honest statement is
  that the tool did not run — not that the scan was clean.
- No findings to feed cycle 2's Phase B.

### c1/E/s04 — Review what the release made false
- **outcome:** one real correction. `docs/perpetum.md` opened with *"Status:
  speculation. Nothing here is built."* — true when written this morning, false
  after batch 1. It now carries counts that are checked against the file rather
  than remembered.
- The editor's [`README.md`](../README.md) was reviewed and **deliberately not
  changed**: it documents JustCode, which is a shipped product, and the harness
  is neither shipped nor part of it. Mentioning `crates/` there would advertise
  something a user cannot use.

### c1/E/s05 — Price book, help pages, training material, GTM
- **price book:** N/A — nothing is sold.
- **help:** `perp help` lists all six commands; kept in the binary rather than a
  page, because a CLI whose help is elsewhere is a CLI nobody reads the help of.
- **training material:** the "running a cycle" section of the release notes,
  including the `T-18` workaround, since an operator will otherwise lose an hour
  to it.
- **GTM:** skipped, with reason — there is no market for a harness with one
  user, and `docs/gtm/` would be a folder of aspiration.

### c1/E/s06 — [approval] Deploy, and [approval] notify
- **outcome:** both **parked**, unattended, as designed. Nothing was pushed,
  tagged, or merged. Six branches exist and all six are local.
- A blocked approval does not fail the release (Perpetum E): the cycle
  continues, and the parked steps are named.

**Phase E exit:** version 0.1.0 shipped **to the approval boundary**, with the
two crossing steps parked. ✅

---

## Phase F — Clean-up

### c1/F/s07 — Reconcile the record
- Status markers updated in the requirements source: 38 ✅, 2 🟡, 3 🔶.
  Verified by counting the file, not by remembering:
  `done: 38  wip: 2  conflicting: 3`, against `148` ids total.
- `perp check ids` confirms all 148 are defined in the source and cited
  correctly across 18 documents — Perpetum 0.8 held for the whole cycle.
- Delivered batches moved out of the active list in
  [`../prioritization/batches.md`](../prioritization/batches.md); batches 6–10
  carry into cycle 2's Phase C.

### c1/F/s08 — [approval] Close the loop with whoever asked
- **outcome:** parked. The only requester is the operator, who is in the session
  — there is no issue thread to reply to, and the GitHub tracker was unreachable
  all cycle (`c1/B/s07`).

### c1/F/s09 — The cycle's numbers
| | |
|---|---|
| Batches delivered | 5 of 5 |
| Requirements done | 38 of 148 |
| In progress | 2 · conflicting 3 · gated 1 · **blocked 0** |
| Requirements minted while building | 7 |
| Tests | 128, from 0 |
| Red-run tests | 26, of which 3 mutations were inconclusive and re-run |
| Gate failures | 5, all fixed inside 2 attempts |
| Commits | 6, none pushed |
| Money spent on models | £0 — cycle 1 ran no model calls; the loop was executed by hand against the binding |

**Phase F exit:** statuses reconciled, delivered work moved out, parked items
carried with their reasons, numbers recorded. The loop returns to Phase B for
cycle 2. ✅

---

## Cycle 1 closed

The harness that will run this loop can now: bind a project, refuse to run
unbound, journal its steps, project its state, run gates and keep their
transcripts pinned to a commit, reconcile a killed run, notice itself thrashing,
refuse the destructive half of git, and prove that no requirement id was
invented outside the source.

It cannot yet talk to a model — which is the whole of batches 6 and 7, and the
first thing cycle 2 will pick up.

---
---

# Perpetum journal — cycle 2

Branch `perp/c2/init` for phases B–C.

## Phase B — Requirements gathering

### c2/B/s01 — Sources
- **outcome:** all eight read or recorded unavailable.

| Source | Weight | This cycle |
|---|---|---|
| Crash analytics | 100 | unavailable — nothing deployed; the search from `c1/B/s06` re-run, still only false positives |
| Security | 95 | **read** — there is code now; see below |
| Support | 45 | unavailable — no users |
| User voice | 40 | partial — operator read, GitHub still unreachable |
| Backlog | 30 | **read in full** — 148 → 149 |
| NFRs | 20 | **read** — produced `N-12` |
| Analytics | 10 | unavailable |
| Market | 5 | **not re-read**, same day; recorded with the reason rather than dated as fresh |

### c2/B/s02 — Security, the first pass with something to scan
- **outcome:** no findings, and the checks that produced that are named rather
  than implied.

  ```
  cargo tree            perp -> perp-core, nothing else (2 packages in Cargo.lock)
  cargo audit           NOT RUN — not installed; a deduction is not a scan
  unsafe blocks         0  (5 textual hits, all the English word)
  unwrap/expect in lib  0  outside #[cfg(test)]; clippy's deny is demonstrably live
  network calls         0  no HTTP client exists
  secret reads          0  only the named allow-list in Env::essentials
  ```

- **the honest caveat:** this is the last pass where "no network, no secrets" is
  true. Batches 7 and 8 add an HTTP client and API keys.

### c2/B/s03 — What the loop asked for
- **outcome:** `N-12` minted — end-to-end tests drive `perp` as a subprocess.
  Cycle 1 delivered five batches and only the spine has E2E coverage, which
  Perpetum D asks for. Filed rather than carried as a footnote, so it can
  compete for a slot like anything else.

**Phase B exit:** every source read or marked unavailable, dated. ✅

---

## Phase C — Prioritisation

### c2/C/s04 — Reconcile against reality (C.1, Perpetum 0.7)
- **intent:** cycle 1's markers were written by the same session that wrote the
  code. That is the situation Perpetum 0.7 exists to distrust.
- **outcome:** **no marker rot.** 38 ✅ each map to a module and passing tests;
  16 modules, ~5,450 lines; `perp check ids` clean across 18 documents; the
  `N-9` and `unsafe` claims verified by independent greps rather than by
  re-reading last cycle's report.
- **note:** expected after one cycle, and it will not stay expected.

### c2/C/s05 — Conflicts
- **outcome:** the three from cycle 1 (`I-3`, `O-6`, `G-13`) carry forward
  unanswered. **No new conflicts.** The TLS-dependency question that batch 7
  raises is an *approval*, not a contradiction — `local-only` remains
  first-class either way, so the vision is untouched.

### c2/C/s06 — Score and batch
- **outcome:** ten batches, renumbered 6–15, in
  [`../prioritization/batches-cycle2.md`](../prioritization/batches-cycle2.md).
- **score and coherence agree this cycle**, and where they differ the score
  wins: **proving it works goes before the model layer**. It scores highest,
  closes cycle 1's outstanding Phase D obligation, and fixes `T-18`. Building
  the model layer on an unproven CLI would mean the first end-to-end test ever
  written has to cover twice as much.
- Recorded because the interesting work is batch 7, and "do the boring thing
  first" is exactly the call an unattended loop is tempted to skip.

**Phase C exit:** 10 batches written; conflicts carried and re-asked; state
updated. ✅

---

## Phase D — Batch 6, proving it works

Branch `perp/c2/b6`. Seven requirements: `N-12` `T-18` `V-6` `L-8` `N-3` `N-5`
`N-6`.

### c2/b6/s01 — Fix `T-18` first
- **intent:** the harness cannot reliably test itself while it locks its own
  binary, so this comes before the tests that would trip over it.
- **outcome:** `gate::self_lock_complaint` refuses up front, with the fix in the
  message. Before and after, on the real repository:

  ```
  before:  error: failed to remove file `...\target\debug\perp.exe`
           Caused by: Access is denied. (os error 5)

  after:   perp: binding key `gate`: this binary is running from
           \\?\D:\repos\justcode\crates\target\debug\perp.exe, inside the
           workspace the gates build (\\?\D:\repos\justcode\crates). On Windows
           the build cannot replace a running executable. Copy `perp` somewhere
           outside the tree and run it from there.
  ```

  Same exit code; the difference is whether the operator loses an hour.

### c2/b6/s02–s03 — The end-to-end suite (`N-12`)
- **outcome:** `crates/perp/tests/cli.rs`, 11 tests that run the **real binary
  as a subprocess** against fixture repositories.
- **where it lives matters:** in the `perp` package, not beside the library's
  own tests, because cargo sets `CARGO_BIN_EXE_perp` there and guarantees the
  binary is built first. An E2E suite that tests a stale binary is worse than
  none.
- **what it catches that unit tests cannot:** an argument parsed wrongly, a path
  resolved from the wrong root, and an exit code that lies — the last being what
  an unattended caller actually reads. Two tests assert exit codes specifically:
  an unbound project and a parked resume both exit non-zero.

### c2/b6/s04 — Gates and red run
- **gates:** green. 125 unit + 5 spine + **11 end-to-end** = 141 tests.
- **red run:** five mutations.

  ```
  gate.rs     never detect the self-lock       a_gate_that_would_rebuild…             101  red
  state.rs    corrupt the projection's counts  a_failed_outcome_carries_its_verbatim… 101  red
  main.rs     stop treating stray ids as errors check_ids_fails_when_a_document…      101  red
  session.rs  Unclear -> Continue instead of Park  resume_parks_a_step_that_died…     101  red
  main.rs     change the park message text     resume_parks_a_step_that_died…           0  green
  ```

- **the green one is not a weak test.** It mutated the wording of an error
  message; the test asserts the *decision* and the *exit code*, which is what
  matters and what the fourth mutation proves. Recorded rather than counted as
  a survivor, because "a mutation that changed nothing anyone depends on" and
  "a test that checks nothing" look identical in a summary.

### c2/b6/s05 — Artefact exercised (`V-6`)
- **outcome:** `perp gate all --root .. --step c2/b6/s05` from a copy outside
  the tree: all three gates green, pinned to `b06c7bc`. The E2E suite is now the
  automated form of this — it runs the real artefact on every `cargo test`.

### c2/b6/s06 — What is *not* done
- **`N-6` is 🟡.** The harness opens no socket, which is verified. But nothing
  stops a *project's* gate command from reaching the network — `cargo` will
  happily fetch — and enforcing that needs the sandboxed runtime in `T-9`.
  Claiming `N-6` on the strength of the harness's own behaviour would be
  answering a different question than the one the requirement asks.

**Batch 6 status:** 6 of 7 delivered, 1 carried, 0 blocked. 141 tests.

---

## Phase D — Batch 7, model links: the router

Branch `perp/c2/b7`. Eight requirements: `M-1`–`M-7`, `M-14`. The first batch
that knows what a model is.

### c2/b7/s01 — Share the fenced-block parser
- **outcome:** `binding::parse_fenced` extracted, so the link configuration
  lives inside the document that explains it — the same reason the binding does.
  No second config format, no second place for the same fact to be wrong.

### c2/b7/s02–s04 — The router and the probe
- **outcome:** `link.rs` and `probe.rs`, and the one decision the whole design
  rests on: **nothing in the harness names a model, only a role**.
- **privacy is not free-form.** An `lmstudio` or `lmlink` link is always
  `local` — a rig you own is yours even in another room — and a `deepseek` link
  is always `cloud`. Declaring otherwise is a startup error. An
  `openai-compat` link *must* declare, because it could be a container on this
  machine or a proxy on the internet, and guessing is not available.
- **`local-only` removes cloud links from consideration rather than falling
  back to them**, and the error says so in as many words. A silent promotion
  across the privacy boundary is the single failure `M-4` exists to prevent, so
  the message names the whole chain with each link's class.
- **the probe cache is keyed on the quantization** as well as the link and the
  model. Swapping a Q8 for a Q4 in LM Studio changes what the link can do while
  every other identifier stays identical.
- **capabilities carry their provenance.** `Source::KindDefault` or
  `Source::Observed` — a default is not a measurement, and a wrong answer should
  be traceable to which it was.

### c2/b7/s05 — Gates and red run
- **gates:** green. 152 unit + 5 spine + 14 end-to-end = **171 tests**.
- **red run:** six mutations, six red — including the one that matters most,
  the privacy filter:

  ```
  link.rs   ignore the privacy filter          local_only_removes_cloud_links…       101  red
  link.rs   let a kind's privacy be overridden a_link_cannot_declare_a_privacy…      101  red
  link.rs   treat every link as healthy        a_role_resolves_to_the_first_healthy… 101  red
  link.rs   stop recognising a dead model id   a_deprecated_model_id_names…          101  red
  probe.rs  drop quantization from the key     the_cache_is_keyed_on_the_quantization… 101 red
  probe.rs  never expire a cached probe        a_stale_entry_is_dropped…             101  red
  ```

- **three mutations needed two attempts to apply**, all defeated by `sed`
  delimiters colliding with `|` in the Rust source. Each was reported as
  *"MUTATION DID NOT APPLY"* by the `V-10` check rather than as a passing test,
  and the last was applied with a Python rewrite instead. The check has now paid
  for itself in four separate batches.

### c2/b7/s06 — Exercised against the real configuration
- **outcome:** [`links.md`](links.md) written and bound as `path.links`.

  ```
  $ perp links --role coder --local-only
  mode:  local-only
    here [lmstudio · local · qwen3-4b-instruct] http://localhost:1234
    ds-fast [deepseek · cloud · deepseek-v4-flash] https://api.deepseek.com  (skipped: cloud)
  role coder: ds-fast → here
  resolves to: here [...] (health assumed — nothing was contacted)
  ```

  The output says *health assumed — nothing was contacted*, because a listing
  that implies it pinged something it did not is exactly the quiet lie this
  harness exists to avoid.
- The real `verifier` chain is deliberately the reverse of `coder`'s, so
  `V-5`'s "the verifier is not the author" is the default rather than a rule
  someone has to remember.

### c2/b7/s07 — What is *not* done
- **`M-6`, `M-7` and `M-14` are 🟡.** Each has a half that needs a socket:
  probing a live endpoint, fetching `/api/v0/models`, and refreshing the model
  list at startup. The logic halves are done and tested against a recorded
  response; the network halves are batch 8.
- Marking them ✅ on the strength of the tested half would be claiming the
  harness talks to LM Studio. It does not talk to anything yet.

**Batch 7 status:** 5 of 8 delivered, 3 carried, 0 blocked. 171 tests.

**The decision batch 8 opens with** is already recorded in
[`../prioritization/batches-cycle2.md`](../prioritization/batches-cycle2.md):
`http://` needs no dependency, `https://` needs TLS, and TLS needs an approval.

---

## Phase D — Batch 8, transport and protocols

Branch `perp/c2/b8`. **The operator answered the HTTPS question: use `curl`.**
No TLS crate, no dependency, and `curl` 8.19 is already on this machine.

### c2/b8/s01 — The credential problem, before the transport
- **the constraint that shaped everything:** anyone on a machine can read
  another process's command line. An `Authorization` header passed as an
  argument is a credential leak with a nice interface.
- **outcome:** the header goes to `curl -K -` on **stdin** — not in argv, not in
  a file that outlives the call, not in the journal (`S-2`). The request body
  goes to a temp file, deleted after, because a prompt is longer than any
  command line allows and stdin is spoken for.
- `Secret` holds the **name** of an environment variable and reads the value at
  the moment of use. Its `Debug` prints `Secret($DEEPSEEK_API_KEY)`, so a `{:?}`
  of a request is safe to journal.

### c2/b8/s02 — A real server, in the tests
- **outcome:** the transport tests start a **one-shot HTTP server on an
  ephemeral port** with `std::net::TcpListener` and let `curl` genuinely
  connect. Dependency-free, hermetic, and real — the assertions are about what
  the server actually received.
- The credential test is the one worth reading: it asserts the key is **absent
  from argv** and **present in the request the server saw**. Both halves, or it
  proves nothing.

### c2/b8/s03–s05 — The client
- `client.rs`: model listing (`/api/v0/models` for LM Studio kinds, `/v1/models`
  otherwise), capability probing, chat, failover.
- **`M-22`** — reasoning is parsed into its own field, never concatenated into
  the message, and `Reply::as_message` deliberately drops it, because providers
  reject a replayed reasoning block or charge for it.
- **`M-9`/`M-10`** — a fall-through is recorded with the reason, and
  `Served::provenance()` produces the line that goes in a journal record:
  `link here · model small · Q4_K_M · after cloud (HTTP 401 — bad key)`.
- **the failover path is not a way around `M-4`**: candidates are filtered by
  privacy *before* the loop, so a `local-only` run cannot reach a cloud link by
  failing enough times. There is a test named after exactly that.

### c2/b8/s06 — Test gate FAILED, and it was the design
- **outcome:** exit 101, `capabilities_are_probed_once_and_then_cached`.

  ```
  cached: Unbound { key: "test", reason: "no answer left" }
  ```

- **diagnosis:** `capabilities()` fetched the model list **before** consulting
  the cache, because the cache key includes the quantization and the
  quantization is only knowable by asking. So the cache saved nothing: every
  lookup still cost a round trip, and the TTL protected nothing.
- **fix:** cache the facts alongside the capabilities, with the same TTL. A
  second test now asserts the other half — past the TTL it asks again, so a
  model swapped in LM Studio is not believed to be the old one forever.
- **attempt 1 of 2.** The test found a real defect in the design, not a typo.

### c2/b8/s07 — A live LM Studio, and two findings
- **LM Studio is running on this machine.** The transport reached it, which
  turned this from a fixture exercise into a real one.
- **finding 1 — `M-24`, filed.** The first live call failed with
  *"`LMSTUDIO_TOKEN` is not set"*, because the configuration declared an
  `auth_env` for a server that needs no token by default. The link failed before
  it connected. Worse, it failed *at call time*: on a real batch that is an hour
  in, to discover a typo. Filed as **`M-24`** — declared credentials are checked
  when the project is bound, not at first use. The configuration was corrected.
- **finding 2 — `M-14` was available but not enforced.** With the token fixed,
  the call went out and came back `HTTP 400: No models loaded`. The check that
  would have caught it existed and nothing called it. Now `Client::call`
  verifies the configured model against the link's real listing before sending:

  ```
  $ perp ask "say hello" --role compactor
  perp: role.compactor: every link failed: here — link.here.model:
  `qwen3-4b-instruct` is not offered by this link.
  Available: text-embedding-nomic-embed-text-v1.5
  ```

  That message is generated from the live server's real model list. `M-7` and
  `M-14` are no longer tested only against a recorded fixture.

### c2/b8/s08 — Gates and red run
- **gates:** green, pinned to `f528ae5`. 175 unit + 5 spine + 14 end-to-end =
  **194 tests**.
- **red run:** five mutations, four red, one anchor missing and reported as
  such rather than counted:

  ```
  net.rs     put the credential back in argv     the_credential_reaches_the_server…   101  red
  client.rs  concatenate reasoning into content  reasoning_is_a_separate_channel…     101  red
  client.rs  fail instead of falling through     a_failed_link_falls_through…         101  red
  client.rs  never hit the facts cache           capabilities_are_probed_once…        101  red
  link.rs    (anchor no longer present)          an_unknown_model_id_lists…    DID NOT APPLY
  ```

### c2/b8/s09 — What is *not* done
- **`M-8`** — the degradation ladder for tool calls — is untouched. It needs
  tool calling, which needs the tool host, which is batch 11.
- **`M-21` is 🟡.** Chat completions works; `/v1/responses` is an explicit
  `Protocol` variant that returns "not implemented yet" rather than silently
  falling back. The enum exists so the choice is visible.
- **`M-23` is 🟡.** `--connect-timeout` and `--max-time` bound every request,
  but a true **first-token** deadline needs streaming, and `curl` one-shot
  invocations do not stream into the parser.
- **DeepSeek has still never been called.** The transport can do HTTPS; no key
  is set on this machine, and the loop will not invent one (`S-5`).

**Batch 8 status:** 6 of 8 delivered (`M-9` `M-10` `M-22`, plus `M-6` `M-7`
`M-14` closed from batch 7), 2 carried, `M-8` untouched, 0 blocked. One
requirement discovered and filed (`M-24`). 194 tests.

---

## Phase D — Batch 9, cost, caching and context

Branch `perp/c2/b9`. Four requirements: `M-11` `M-12` `M-13` `M-15`.

### c2/b9/s01 — The ledger is a projection, not a counter
- **outcome:** `cost.rs`. Every call writes its accounting into the journal
  record's extra fields, and `Ledger::replay` rebuilds the totals from them —
  the same rule as `state.md` (`L-4`). A process that dies mid-batch has still
  recorded what it spent.
- **cache-hit and cache-miss input are never added together.** DeepSeek prices
  them fifty to one; a ledger that merges them reports a number that is wrong
  by an order of magnitude and looks perfectly reasonable.
- **an unreported cache split counts as all-miss** — priced as if nothing was
  cached, rather than assuming a discount nobody granted. `cache_ratio()`
  returns `None` for it: unknown, not zero.
- **prices come from configuration**, like model ids and for the same reason.
  A link with no price is free, which is right for a local one and a visible
  zero next to a cloud one.

### c2/b9/s02 — Prompt layout as an engineering requirement
- **outcome:** `prompt.rs`. Stable region first in a fixed order, volatile tail
  last, and a `PrefixGuard` that fingerprints the stable region per batch.
- **the failure this prevents is invisible**: reorder two system segments and
  the model behaves the same, the tests pass, and every call silently pays
  cache-miss rates. Nothing else in the system would ever mention it.

### c2/b9/s03 — Compaction, and where it is allowed to run
- **outcome:** oldest tail first, newest turn always kept, stable prefix never
  touched — compacting the prefix would cost more cache than it saves tokens.
- **refuses to run on a cloud link** (`M-13`). Compaction reads everything the
  loop has seen; it is the most context-rich call in the system, which is
  exactly why it stays on the operator's hardware.
- Token counts are `estimated_tokens`, named for what they are. The provider's
  reported usage is the truth; this is for deciding what to send before there
  is a report to read.

### c2/b9/s04 — Concurrency
- **outcome:** `Permits`, per link, defaulting to 1. A link at its limit is
  **skipped and the skip recorded**, not queued and not exceeded.
- Tested with sixteen real threads against a limit of two, asserting the peak
  never exceeded it. The engine is single-threaded today; this is the thing
  that has to already be right on the day it is not.

### c2/b9/s05 — Test gate FAILED: the report read zero
- **outcome:** the new end-to-end test failed on its last assertion.

  ```
  the charge: 1 calls · 1000 tokens in (800 cached) · 20 out · 0.1s · 0.0000
  ```

- **diagnosis:** the arithmetic was right and the **display** was wrong. One
  call costs tens of millionths; formatted to four decimal places, every real
  amount rounds to `0.0000`. A cost report that always reads zero is
  decoration.
- **fix:** six decimal places. The test now asserts `0.000036`, which is
  800 hits at 0.0028 plus 200 misses at 0.14 plus 20 out at 0.28, per million.
- **attempt 1 of 2.**

### c2/b9/s06 — The whole chain, over a real socket
- **outcome:** an end-to-end test starts an OpenAI-compatible server on an
  ephemeral port, points a fixture configuration at it, and runs the **real
  binary**: `perp ask --step c2/b9/s01` then `perp cost`, as two separate
  processes over one journal.

  ```
  via: link local · model small · Q4_K_M
  tokens: 1000 in (800 cached) / 20 out
  ...
  1 calls · 1000 tokens in (800 cached) · 20 out · 0.1s · 0.000036
  ```

  Real socket, real `curl`, real journal, real replay. Nothing in that path is
  mocked except the model on the other end.

### c2/b9/s07 — Red run, and a design error it found
- **outcome:** nine mutations, eight red. The survivor was the useful one.
- `reordering_the_stable_region_changes_the_fingerprint` still passed with the
  **segment name** removed from the hash — and thinking about why exposed a
  real error: a prefix cache matches **bytes, not labels**. Hashing the names
  meant a pure rename would trip the guard while the cache hit perfectly. That
  is the worst kind of alarm, the one you learn to ignore.
- **fix:** the fingerprint hashes content and order only. Two new tests pin it
  — a rename does not change it, a segment boundary does — and both go red
  under mutation.

### c2/b9/s08 — Still not proven
- **No real model has been called.** LM Studio is running here but has only an
  embeddings model, and downloading a chat model onto the operator's machine is
  not the loop's decision. DeepSeek has no key. Everything in this batch is
  proven against a real socket and a recorded response, which is not the same
  as proven against a model.

**Batch 9 status:** 4 of 4 delivered, 0 blocked. 222 tests.

---

## Phase D — Batch 10, local-server realities

Branch `perp/c2/b10`. Five requirements: `M-16`–`M-20`. The batch where the open
question stopped being a question.

### c2/b10/s01 — The open question, settled by testing it
- **intent:** `M-19` cannot be built without knowing how an `lmlink` peer is
  actually reached. The design has carried this as open since it was written.
- **the machine turned out to have everything needed**: LM Studio running, the
  `lms` CLI installed, LM Link **enabled**, and a peer **connected**.

  ```
  $ lms link status
  This device: ROG Z13 RTX 3080
  Status: Online

  Found 1 device:

    - KUR
      Status: connected
      Identifier: 249f12e9a0ce27e285de21d08ee37ffd
  ```

- **the measurement that answers it:**

  ```
  $ lms ls
  text-embedding-nomic-embed-text-v1.5   Nomic BERT   84.11 MB   Local
  text-embedding-nomic-embed-text-v1.5   Nomic BERT   84.11 MB   KUR

  $ curl -s localhost:1234/api/v0/models | (count ids)
  1
  ```

  **Two devices to `lms`, one model to the REST API.** A peer's models are not
  served through the local OpenAI-compatible server.
- **and the second half:** `lms load --help` has **no `--device` flag**. Its own
  text says the model "will be loaded on the preferred device (if set)" — a
  *global* setting (`lms link set-preferred-device`), not a per-call argument.
- **consequence, filed as `M-25` and marked ⛔ external-gated:** the `lmlink`
  kind cannot be a base-URL swap, and it cannot be routed per call either.
  Reaching a peer needs the LM Studio SDK or a global setting change — and a
  loop that flipped a global setting to route one call would be changing the
  operator's environment underneath them. That is not a thing this harness does.
- The requirements doc's §0.1 and open question 1 are both updated from
  "unverified" to the measurement.

### c2/b10/s02 — A real model finally answered
- **intent:** every batch so far has been proven against a socket and a
  recorded response. Not the same as a model.
- **outcome:** loaded the one model on this machine and called it.

  ```
  $ lms load text-embedding-nomic-embed-text-v1.5 --ttl 600 -y
  Model loaded successfully in 7.69s. (80.21 MiB)

  $ curl -s localhost:1234/api/v0/models
  text-embedding-nomic-embed-text-v1.5 -> loaded  Q4_K_M  2048

  $ curl -s -X POST localhost:1234/v1/embeddings -d '{...}'
  {"object":"list","data":[{"object":"embedding","embedding":[-0.0423694…
  ```

  A real vector, from a real model, on the operator's hardware. **80 MiB took
  7.69 seconds** — which is the entire argument for `M-16`, since a 30B is
  minutes of that.
- The model was **unloaded afterwards** and `lms ps` confirms nothing is
  resident. The machine is as it was found.
- Still not proven: chat. This machine has no chat model, and downloading one
  onto it is not the loop's decision.

### c2/b10/s03–s05 — The batch
- **`M-16`** — warming is decided from the reported `state` and performed with
  `lms load --ttl`, which is how the "holds them with a TTL" half is met. The
  TTL is the harness's to set, which was not obvious until `lms load --help`
  was read.
- **`M-17`** — VRAM as a lease, keyed on the **host**, not the link: two links
  pointing at `localhost` are two models on one GPU however different their
  names are. A second claim is **refused, not queued** — the honest answer is
  "that will not fit", rather than a wait that ends in an out-of-memory error
  minutes later.
- **`M-18`** — a rolling per-link mean of tokens/second and time-to-first-token.
  An unmeasured link returns `None` rather than an optimistic default: a
  wall-clock budget built on a guess is worse than no budget, because it looks
  like a plan.
- **`M-19`** — `lms link status` parsed against **real recorded output**, quoted
  verbatim in the test. A parser written against imagined output is a parser
  that has never been tested.
- **`M-20`** — a dead local peer **parks**; it does not promote the cloud link
  sitting healthy in the same chain. The message says so in as many words: a
  peer going away is not consent to send the work somewhere else.

### c2/b10/s06 — Test gate FAILED
- **outcome:** exit 101 on the new `M-20` test. The behaviour was right; the
  **message** was wrong — a multi-line string literal carried its own source
  indentation into the error, so the text arrived with twenty spaces in the
  middle of a sentence. Fixed with line continuations.
- **attempt 1 of 2.**

### c2/b10/s07 — Red run
- **outcome:** nine mutations, seven red first time, and both stragglers were
  worth the second pass:
  - one anchor did not match the file and was reported as **not applied**
    rather than as a pass (`V-10`, fifth batch running).
  - `a_remote_peer_is_a_different_host_from_this_machine` survived a mutation
    that collapsed every device to one host key — because asserting two strings
    *differ* is nearly impossible to break. Rewritten to assert the exact keys
    (`device:KUR`, `host:localhost:1234`); now red.

**Batch 10 status:** 5 of 5 delivered, 0 blocked. One requirement discovered and
filed (`M-25`, external-gated). 235 tests. **Phase D's exit condition is met for
cycle 2** — five batches delivered.
