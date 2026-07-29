# Deploying `perp`

Created by Perpetum Phase E, which says this file exists before a deploy is
ever approved. **Nothing here has been run.** It describes what a deploy would
be, so that the approval is a decision about a known thing rather than a
signature on a blank page.

Every step below is **[approval]**. The loop drafts this; a human runs it.

## What "deployed" would mean

`perp` is a CLI for one operator on their own machine. There is no server, no
tenancy, no fleet — the vision's first clause forbids all three. So a deploy is
one of exactly two things:

| | What | Blast radius |
|---|---|---|
| A | The branches merge to `main` and the harness lives in this repository | Anyone who clones JustCode gets a `crates/` directory they can ignore |
| B | A tagged release with built binaries | Anyone who downloads one runs it against their own repository |

**Neither has happened.** As of 0.2.0 there are thirteen `perp/**` branches, all
local, and `main` is untouched at the commit it was on before this work began.

## Before either option

1. **The three parked conflicts need answers** — `I-3`, `O-6`, `G-13`. They
   have been open since cycle 1 and two of them shape what the harness *is*.
2. **`S-8` should be fixed first.** Request bodies land in a shared temp
   directory; on a single-user machine that is a shrug, and on a shared one it
   is a leak of whatever the loop was reading.
3. **A chat model has never been called.** Shipping a harness whose central
   feature has only ever been exercised against a recorded response is a claim
   nobody has checked, including this one.

## Option A — merge to `main`

```bash
# from the repository root, with a clean tree
git switch main
git merge --no-ff perp/c1/init      # phases A–C
git merge --no-ff perp/c1/b1        # … through perp/c2/release
```

Then, and only then:

```bash
cd crates && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo build --workspace && cargo test --workspace
```

Merging is `[approval]` under `G-5`. The gates are not: run them anyway, on
`main`, because a merge is the one moment when a green branch and a green trunk
can differ.

**What this does to JustCode:** nothing. `crates/` is a separate cargo
workspace, verified by `cargo metadata` reporting two distinct workspace roots,
so `cargo tauri build` behaves exactly as it did before any of this existed.
`crates/target/` is gitignored.

## Option B — a tagged release

Do not reuse JustCode's release workflow. It builds and publishes the *editor*,
triggered by a `v*.*.*` tag, and a `perp-0.2.0` tag pushed to this repository
would either do nothing or do something surprising. A harness release needs its
own tag pattern and its own workflow, and neither exists.

That work is not filed as a requirement, because nobody has asked for a
distributable binary. If they do, it starts here.

## Rolling back

There is nothing to roll back. Every commit is on a branch; `main` has not
moved. If option A is taken and regretted, the merge commits revert cleanly —
each feature's commits are contiguous and carry a `Perpetum-Step` trailer, which
is what `G-8` is for.

## What would need to change for this to be a real runbook

A deploy target, a way to distribute, and someone other than the author running
the result. Until then this document's honest content is the paragraph at the
top: nothing has been deployed, and the loop will not deploy anything.
