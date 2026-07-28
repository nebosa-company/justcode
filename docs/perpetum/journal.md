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
