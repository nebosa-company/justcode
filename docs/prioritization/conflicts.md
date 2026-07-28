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

## Asked

Both presented to the operator on 2026-07-28 as a batch, per C.6. The cycle
continued without waiting (C.6). No answers recorded yet.
