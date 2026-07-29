# Parked conflicts — cycle 1

Requirements that contradict the vision, a shipped requirement, or the current
architecture (Perpetum C.2). Parked, not blocking. Asked as a batch (C.6);
unanswered ones carry into the next cycle.

Both of these are real tensions in the design as written, found by reading the
backlog against [`vision.md`](../initiation/vision.md) rather than by looking
for something to report.

---

## CONFLICT-1 — The JustCode panel versus "never an IDE"

**Requirements:** `I-3` (panel hosts chat, the approvals queue, the current
diff, the artifact view and a journal timeline), supported by `I-4`, `A-5`.

**Contradicts vision clause 6:** *"Never an IDE. JustCode stays a small editor.
The harness is a sidecar process with a panel, and the editor must run perfectly
with the harness absent, uninstalled, or crashed."*

**The trade-off.** A panel that hosts chat, diff review, artifact rendering and
a timeline is most of an agent IDE by surface area. Every one of those is
individually justified — approving a diff you cannot see is not approval — but
together they are the thing the vision says this will never become. The clause
also has a second half the panel does satisfy (sidecar, editor runs without it),
so this is a question of degree, not a flat contradiction.

**Options:**

| | Option | Consequence |
|---|---|---|
| A | **Panel shows, terminal does** *(recommended)* — the panel renders board, approvals and timeline read-only; diff review and chat open in the editor's existing surfaces (tabs, Problems panel, terminal dock) rather than in bespoke panel UI. | Keeps `I-4`'s reuse promise, keeps the panel small, costs some polish. Honours clause 6 by construction: the panel adds a view, not an application. |
| B | Full panel as specified in `I-3`. | Best experience, most code, and the editor grows an agent IDE inside it. Would need clause 6 rewritten to say what "small" now means. |
| C | No panel — CLI only, `I-1` alone. | Cheapest and safest; loses the reason this work lives in this repo at all. |

**Recommendation: A.** It is the only option that keeps both halves of clause 6
true, and it defers the expensive UI until there is a loop worth watching. If
the answer is B, clause 6 must be edited in the same breath — a vision that is
quietly violated stops being able to detect the next conflict.

**Blocks nothing this cycle:** `I-*` is in no batch 1–10.

---

## CONFLICT-2 — The notification reply path versus "instructions only from the operator"

**Requirements:** `O-6` (notification sink supports a **reply path** so a human
away from the terminal can approve, reject or `/btw`).

**Contradicts vision clause 10:** *"Never instruction-taking from content.
Repository text, dependency code, issue threads, tool output and web pages are
data. Only the operator and the binding give instructions."* Also in tension
with `S-1`, `T-12` and `C-10`.

**The trade-off.** An inbound reply path is an authentication surface. A webhook
callback or an email reply is *content arriving over the network*, and the
harness cannot tell a real operator from a forged reply without real
authentication. `T-13` says a `never` call cannot be argued past; `O-6` as
written would let an unauthenticated message approve a production deploy —
precisely the class of action the approval boundary exists for.

**Options:**

| | Option | Consequence |
|---|---|---|
| A | **Read-only sink + `/btw` only** *(recommended)* — notifications go out; the only thing that can come back is a `/btw` note, which `C-10` already forbids from crossing the approval boundary. Approvals happen at the CLI or the panel, on the machine. | Removes the authentication problem entirely instead of solving it. Costs remote approval, which is the feature's whole appeal. |
| B | Signed replies — HMAC or a device-bound token, replay-protected, with the approval text hashed into the signature. | Keeps remote approval; adds a key-management surface to a product whose vision says secrets are never stored (`S-2`), and one implementation bug re-opens the boundary. |
| C | `O-6` as written. | Not acceptable — it is the vision's clause 10 with extra steps. |

**Recommendation: A**, with B revisited only once the loop has run unattended
for real cycles and remote approval is demonstrably the bottleneck. Until then
the honest framing is: the phone tells you something needs you; you still walk
to the machine.

**Blocks nothing this cycle:** `O-6` is in no batch 1–10.

---

---

## CONFLICT-3 — `G-13` versus the binding it was written alongside

**Requirement:** `G-13` — *"The harness's own state (journal, scratch,
artifacts) is either outside the repo or ignored by it. The loop never commits
its own noise."*

**Contradicts the current architecture**, specifically
[`binding.md`](../perpetum/binding.md), which declares
`out.journal = docs/perpetum/journal.jsonl` — inside the repository — and every
batch so far has committed it.

Found by implementing batch 4, not by reading. This is the kind of conflict
Perpetum C.2 exists for: a requirement that was reasonable in the abstract and
is wrong against the architecture that grew under it.

**The trade-off.** `G-13` is about churn: build output, caches, scratch files,
a journal that grows a line per tool call. But this loop's journal *is* the
evidence — gate transcripts, verbatim errors, which step produced which commit.
Committing it is what makes a branch reviewable by someone who was not there,
and what makes `perp resume` work on a fresh clone. Ignoring it would mean the
proof of a green gate lives only on the machine that ran it.

**Options:**

| | Option | Consequence |
|---|---|---|
| A | **Split the requirement** *(recommended)* — `G-13` keeps its ban on committing churn (scratch, caches, build output, `crates/target/`), and the journal, state file, board and artifacts are reclassified as **documentation**, deliberately versioned. | Honest about what is happening, keeps the audit trail in git, and still stops the loop committing noise. Needs `G-13`'s text rewritten, not just re-marked. |
| B | Move the journal outside the repository. | Satisfies `G-13` literally. A reviewer of a `perp/**` branch can no longer see why anything was done, and the evidence stops travelling with the work. |
| C | Keep the journal in the repo but gitignored. | Worst of both: it exists, it is not shared, and the next clone starts blind. |

**Recommendation: A.** The requirement was written before the journal had a
reader other than the engine. Now that a human reviews branches by reading it,
"never commit your own state" is the wrong rule for this particular state.

**Blocks nothing.** `G-13` is marked 🔶 and was not implemented in batch 4;
the other seven requirements in that batch were.

---

## Answered — 2026-07-29

All three, after surviving two full cycles of being asked at the end of a phase.
Presented one at a time on the operator's instruction, which is worth recording:
asked as a batch they went unanswered twice; asked singly they were answered in
minutes.

| | Decision | Not the recommendation? |
|---|---|---|
| CONFLICT-1 `I-3` | **Full panel as specified** | Yes — I recommended the read-only panel. The operator chose the fuller one, so **vision clause 6 was rewritten in the same breath**. That was the condition attached to option B and it has been honoured, not skipped: a vision quietly contradicted stops being able to detect the next conflict. |
| CONFLICT-2 `O-6` | **Outbound only, `/btw` the sole return path** | No — recommendation taken. Approvals never arrive over the network. |
| CONFLICT-3 `G-13` | **Split the requirement** | No — recommendation taken. Churn is never committed; the journal and its siblings are documentation, deliberately versioned. |

Each requirement's text in [`../perpetum.md`](../perpetum.md) was **amended to
say what was decided**, rather than left as written with a note attached. A
requirement that still describes the rejected design is a trap for whoever reads
it next.

Separately, `X-4`'s dependency (`windows-sys`) was approved, and it is no longer
gated. `M-25` remains external-gated and is not a conflict — it is a limitation
of LM Studio, measured in `c2/b10/s01`.
