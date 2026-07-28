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
