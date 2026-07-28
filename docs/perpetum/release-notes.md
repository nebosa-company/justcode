# perp — release notes

The harness versions independently of JustCode. The editor's own release
process is unchanged and documented in [`../RELEASING.md`](../RELEASING.md).

---

## 0.1.0 — 2026-07-28 · *unreleased, and deliberately so*

The first cycle of [Perpetum](../../../perpetum.md) run against the harness that
will eventually run Perpetum. Five batches, 38 requirements, 128 tests.

**Nothing is published.** Tagging, pushing and merging to `main` are
approval-gated under [`binding.md`](binding.md); this release stops at that
boundary and parks the two steps that cross it. The version exists so the
cycle has something to name, not because anything shipped.

### What works

| | |
|---|---|
| `perp bind` | Loads the binding, resolves every path it names, refuses to run unbound. |
| `perp record` | Appends an intent or outcome to the journal. |
| `perp state` | Replays the journal into the state file. |
| `perp gate` | Runs the project's gates, keeps the transcripts, pins them to a commit sha, and writes them to the journal. |
| `perp resume` | Says what a dead process left in flight, and whether to skip it, redo it, or park it. |
| `perp check ids` | Proves that every requirement id cited anywhere is defined in the requirements source. |

Underneath: an append-only journal, a state file that is a projection of it, a
bounded process runner with a declared environment, three watchdogs, a git
harness that refuses `add -A`, `--no-verify` and force-push before spawning
anything, and the verification layer — reality checks, red-run verdicts, a
tamper guard, and markers derived from evidence rather than asserted.

### API breaking changes

None. This is the first version.

For anyone building on it early, two things are load-bearing and will be
treated as breaking if they change: the journal record shape (`v: 1`, see `N-8`)
and the step-id format `c<cycle>/<stage>/s<seq>` (`L-22`), which the journal,
commit trailers, the board and status markers all cite.

### Dependencies

**Zero.** `cargo tree` lists `perp` → `perp-core` and nothing else; `Cargo.lock`
holds two packages, both local. Std-only is a policy (`N-11`), not an accident:
adding a crate is approval-gated under this project's binding.

### Security review (Perpetum E.5)

- **Dependency surface: empty.** There is no third-party code to audit.
- `cargo-audit` is **not installed** on this machine and was not installed
  unattended. With zero dependencies it would have nothing to report, but the
  honest statement is that the tool did not run.
- The harness makes **no network calls** — there is no HTTP client in the crate.
- No secret is read, written, or logged; nothing in the code reads an
  environment variable other than the named allow-list in `Env::essentials`.
- Process execution is the one real surface: commands come from the binding,
  are split without a shell, run with a declared environment and a timeout, and
  their whole process tree is killed on deadline.

Findings feed cycle 2's Phase B `security/` pass, per Perpetum E.5. There are
none to feed.

### Accessibility, localisation, price book

Marked N/A in [`nfrs.md`](../initiation/nfrs.md) with reasons: the harness is a
CLI with no user interface yet, ships no user-facing strings beyond English
help text, and nothing is sold.

### Running a cycle (Perpetum E.9 — training material)

```bash
perp bind                                   # is the project bound?
perp resume                                 # did a previous run leave anything in flight?
perp gate all --step c1/b6/s01              # run the gates, keep the evidence
perp state --out -                          # what the journal says, rendered
perp check ids                              # nobody invented a requirement id
```

One caveat worth knowing before it wastes an hour: **do not run `perp` from the
build directory it is about to rebuild.** On Windows the running executable
cannot be replaced and the build gate fails with `Access is denied (os error
5)`. Copy the binary elsewhere first. Filed as `T-18`; until it is fixed, this
is the workaround.

### Parked at the approval boundary

| Step | Why |
|---|---|
| Deploy / publish | Perpetum 0.4 — nothing is published without a human. |
| Push, tag, merge to `main` | `G-5` — six branches exist locally and none has left the machine. |

### Known and not fixed

- `T-18` — the harness locks the executable its own build gate rebuilds.
- `X-4` — children do not survive a kill of the engine itself; needs a Windows
  job object, which needs a dependency, which needs an approval.
- `L-8` — an invariant, re-asserted each batch rather than a finished feature.
- End-to-end coverage exists for the spine only. The gate runner, git harness
  and verification layer have unit tests but nothing drives `perp` as a
  subprocess. Perpetum D asks for this; it is outstanding, not done.
- `I-3`, `O-6`, `G-13` — parked as conflicting, awaiting a decision.
