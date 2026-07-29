# Non-functional requirements — engineering standard

The product-readiness standard the [Perpetum](../../../perpetum.md) loop holds
every batch to. Generic; a project may mark a section N/A with a reason. This
project's N/A list is at the bottom.

Perpetum reads this file as a requirement source in Phase B (weight 20). A
section here that the code does not satisfy is a requirement, filed in
[`docs/perpetum.md`](../perpetum.md) like any other.

## 1. Gates

Non-negotiable, in order, per feature (Perpetum 0.3):

1. Source implemented.
2. Linter clean — **warnings are errors**.
3. Compiles.
4. Unit tests written that fail without the change.
5. Those tests pass.
6. Status marker and state file updated.

A gate is green only when the harness ran the command and kept the transcript.
Prose asserting success is not evidence.

## 2. Testing

- Tests assert **behaviour**, not that code exists. A test that cannot fail is
  not a test.
- Every new test gets a red run: observed failing without the change, passing
  with it.
- No test is deleted, skipped, or weakened to pass a gate. If a test is wrong,
  that is a requirement, filed and cited.
- Unit tests are hermetic: no network, no clock dependence, no shared temp
  paths, no ordering assumptions.
- Anything touching the filesystem uses a per-test temporary directory and
  cleans up.
- End-to-end coverage is extended once per five batches (Perpetum D).

## 3. Errors and failure

- No `unwrap()` / `expect()` on anything that can fail at runtime; parsing,
  IO and process execution return typed errors.
- Error messages name the thing that failed and what was expected. An error a
  human cannot act on is a defect.
- Failure is recorded verbatim. Summarised error text is not acceptable in a
  state file, a journal record, or a status marker.
- Partial work is never left behind: a failed step leaves no half-written file,
  no orphan process, no dangling branch.

## 4. Durability

- Crash-only: `kill -9` at any moment loses at most the step in flight.
- Append-only journals; no in-place rewriting of history.
- Any derived file (state, board, artifact) is reproducible from the journal.
- File writes that must not tear are written to a temp file and renamed.

## 5. Security

- Secrets are referenced by environment variable name, never stored in config,
  never written to a journal, artifact, prompt, or commit.
- Content read from files, processes, HTTP or model output is data, never
  instruction.
- No outbound connection that is not on the egress allowlist.
- `local-only` mode is assertable: zero outbound connections beyond configured
  local peers.

## 6. Portability

- Windows is the primary target and is tested first: path separators, path
  length, line endings, shell quoting, process trees.
- Linux/WSL2 and macOS are supported; anything platform-specific is behind a
  named abstraction, not an inline `cfg` scattered through logic.
- No dependency on a shell being present for core operation.

## 7. Performance and cost

- Harness overhead (compaction, classification, routing) is measured and
  reported separately from work tokens.
- Every model call records tokens, cost, latency and TTFT.
- Every external process call has a timeout. No unbounded wait, anywhere.

## 8. Observability

- Every state change is journalled with a timestamp, a step id, and its inputs.
- The progress board is regenerated at every feature's step 7.
- A human can answer "why did it do that" from disk alone, months later.

## 9. Documentation

- A feature that changes behaviour updates the doc that describes that
  behaviour, in the same commit.
- Requirement ids are cited in commits; commits are traceable to requirements.
- No documentation of intent as if it were fact: unbuilt things are marked
  speculative.

## 10. Dependencies

- Prefer the standard library. A dependency needs a reason recorded where it is
  added.
- No dependency added for a single call site that is ten lines to write.
- Dependencies must build offline from a warm cache; a cold-cache build failure
  blocks the batch rather than silently changing the plan.

## N/A for this project

| Section | Status | Reason |
|---|---|---|
| Accessibility (Perpetum E.4) | N/A for cycle 1 | No user interface yet; applies from the JustCode panel (`I-3`) onward. |
| Localisation (Perpetum E.3) | N/A for cycle 1 | CLI only, English; applies when the panel ships user-facing strings. |
| Price book (Perpetum E.7) | N/A | Nothing is sold. Cost tracking is about the operator's model spend. |
