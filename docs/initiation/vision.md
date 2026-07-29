# Vision — Perpetum Harness (`perp`)

## What this is

A harness that runs the [Perpetum](../../../perpetum.md) loop unattended against
models the operator controls: LM Studio locally, LM Link on their own rigs, and
the DeepSeek API when cloud is allowed. It keeps a repository moving — gather,
prioritise, build, gate, release-to-the-approval-boundary, clean up, repeat —
and it produces evidence for every claim it makes.

Requirements live in [`docs/perpetum.md`](../perpetum.md).

## Who it is for

One developer with their own hardware, a small cloud budget, and a repository
they want worked on while they are not watching. Not a team, not a platform, not
a fleet.

## What makes it different

Agent harnesses optimise for a good session. This one optimises for **the
hundredth hour unattended**, where the failure mode is not a bad answer but a
convincing lie: a status marker that says done, a green suite that tests
nothing, a batch reported complete with a known failure inside it.

So the core of the product is evidence, not autonomy:

- A gate is green because the harness ran the command and kept the transcript.
- A test counts because it was observed to fail without the change.
- A review counts because a different model on a different link did it.
- Every artefact records which link and which quantization produced it.

## What it will never be

This list is what makes conflict detection possible (Perpetum C.2). A
requirement that contradicts a line here is `CONFLICTING` and gets parked, not
built.

1. **Never a hosted service.** It runs on hardware the operator owns. There is
   no server, no tenancy, no account.
2. **Never dependent on a cloud model.** `local-only` is a first-class mode, not
   a degraded one. If a release makes local-only unusable, the release is wrong.
3. **Never past the approval boundary unattended.** It does not deploy to
   production, contact users, post publicly, spend money, or delete
   infrastructure without a human saying yes, every time.
4. **Never a model provider.** No training, no fine-tuning, no serving. It
   consumes endpoints; it does not become one.
5. **Never a second source of truth.** The journal on disk is the truth. Any UI,
   board or artifact is a view of it.
6. **Never an IDE *by accident*.** Amended 2026-07-29, when the operator chose
   the full panel in `I-3`: chat, the approvals queue, diff review, artifacts
   and a journal timeline all live in JustCode. That is a deliberate decision,
   not drift, and it is written here so the next conflict check measures against
   what was actually decided.

   What did not change is the enforceable half: the harness is a **sidecar**.
   The editor must run perfectly with it absent, uninstalled, or crashed, and
   nothing under `src/` or `src-tauri/` may depend on `crates/`. A conflict
   check can test that; "small" was never testable, which is why the clause
   read as a rule and behaved as a mood.
7. **Never a team product.** No multi-user state, no shared queue, no
   collaboration server.
8. **Never a benchmark chaser.** No feature exists to make an autonomy score
   look better. Given a choice between finishing more work and being able to
   prove what was finished, it proves.
9. **Never destructive by default.** No force-push, no history rewrite, no
   `git add -A`, no deleting a test to pass a gate — regardless of what any
   model, file, or web page asks for.
10. **Never instruction-taking from content.** Repository text, dependency code,
    issue threads, tool output and web pages are data. Only the operator and the
    binding give instructions.

## What success looks like

A cycle runs overnight on a repository. In the morning: a branch of small
commits each citing a requirement id, a state file that matches the branch, gate
transcripts for every claim, two items honestly blocked with their error text,
one approval waiting in a queue, and a bill under a dollar.
