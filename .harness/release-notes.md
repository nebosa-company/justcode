# perp — release notes

The harness versions independently of JustCode. The editor's own release
process is unchanged and documented in [`../RELEASING.md`](../docs/RELEASING.md).

---

## 0.2.0 — 2026-07-28 · *unreleased, and deliberately so*

Cycle 2. Five batches: the end-to-end suite, the model router, the transport,
cost accounting, and the local-server layer. **A real model answered for the
first time.** 237 tests, from 128.

### What changed

| | |
|---|---|
| `perp links` | Lists links and role chains; resolves a role; reports any role with no local option. |
| `perp ask` | Calls a role's link. Prints the reply, the provenance of whichever link answered, and any link tried first and failed. |
| `perp cost` | Replays the journal and reports what was spent, by link and by role. |
| `perp gate` | Now refuses up front when it would rebuild the binary running it. |

Underneath: four link kinds with an enforced privacy boundary, a `curl`-backed
transport with the credential confined to stdin, capability probing keyed on
quantization, reasoning as a separate channel, recorded failover, cost split by
cache-hit and cache-miss, prompt-prefix enforcement, compaction restricted to
local links, per-link concurrency, VRAM leased per host, and rolling throughput.

### API breaking changes

None for users. Two internal changes that will be treated as breaking if they
move again:

- `Client::call` now takes `&mut self` and a clock, because it verifies a
  link's model against the live listing before sending and caches the result.
- `Permits::acquire` takes `&Arc<Permits>` rather than `&self`, so a held
  permit does not borrow the thing that issued it.

The journal record shape (`v: 1`) and the step-id format are unchanged. Records
now carry accounting fields — `role`, `link`, `model`, `cache_hit`,
`cache_miss`, `output_tokens`, `latency_ms`, `charge` — which older readers
preserve and ignore, per `N-8`.

### Security review (Perpetum E.5)

The first pass with a network and a credential to review. **Two checks were
turned into tests rather than assertions**, because a security property nobody
runs is a security property nobody has:

- the credential reaches the server and **never** appears in argv (a process
  listing) or in a run transcript (which is what gets journalled);
- certificate verification is never disabled — no `--insecure`, `-k`,
  `--proxy-insecure` or `--ssl-no-revoke` anywhere in the command line.

**One finding, filed as `S-8`.** A request body is written to a file for the
duration of the call, in the ambient temp directory. On this machine `TMP` is
`D:\Temp` — a *shared* root-level directory, not the per-user one — so a prompt
containing repository content is briefly readable by any other user of the
machine. The key is unaffected; it never touches disk. Feeds cycle 3's Phase B.

Still true: zero dependencies, no `unsafe`, no panics outside tests.

### Price book (Perpetum E.7)

Now genuinely applicable, and reviewed. The prices in
[`links.md`](links.md) — `0.0028` cache-hit, `0.14` cache-miss, `0.28` output
per million tokens for `deepseek-v4-flash` — were verified against the
provider's own documentation on 2026-07-28 and are unchanged. They live in
configuration precisely so this review is an edit rather than a release.

### Documentation the release made false (Perpetum E.6)

One real correction: the state file still said *"the harness opens no socket"*,
which batch 8 made false. `N-6` is about the **gate runner**, which still makes
no network call; the sentence now says which is which.

The editor's [`README.md`](../README.md) was reviewed again and again left
alone — it documents a shipped product, and the harness is still neither.

### Accessibility and localisation

N/A, unchanged from 0.1.0: a CLI with no interface and no user-facing strings
beyond English help text. Recorded rather than skipped.

### Parked at the approval boundary

| Step | Why |
|---|---|
| Deploy / publish | Perpetum 0.4. The runbook now exists — [`../maintain/deploy.md`](../maintain/deploy.md) — and describes what a deploy would be. Nothing was deployed. |
| Push, tag, merge to `main` | `G-5`. Thirteen branches exist locally and none has left the machine. |

### Known and not fixed

- `S-8` — request bodies in a shared temp directory.
- `M-25` — **external-gated**: inference on an LM Link peer is unreachable from
  outside LM Studio. Measured, not assumed.
- `X-4` — **approval-gated**: children surviving a kill of the engine needs a
  Windows job object, which needs a dependency.
- `M-8` — the degradation ladder, waiting on the tool host.
- `M-21`, `M-23` — `/v1/responses` and a true first-token deadline.
- `M-24` — credentials should be checked at bind time, not at first call.
- `N-6` — nothing stops a project's gate command reaching the network.
- `I-3`, `O-6`, `G-13` — parked as conflicting since cycle 1, still unanswered.

### What has still never happened

**No chat model has been called.** An embedding has. This machine holds no chat
model, and downloading one onto it is not the loop's decision; DeepSeek has no
key set. Everything about generation — tokens per second, time to first token,
the reasoning channel, the cache-hit ratio — is proven against recorded
responses and a real socket, which is not the same thing.

---

## 0.1.0 — 2026-07-28 · *unreleased, and deliberately so*

The first cycle of [Perpetum](../../perpetum.md) run against the harness that
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

Marked N/A in [`nfrs.md`](nfrs.md) with reasons: the harness is a
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
