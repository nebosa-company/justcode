# Source-trust weights

How much the *origin* of a signal counts (Perpetum B). Impact weights are a
different thing and live in
[`../prioritization/weights.md`](../prioritization/weights.md).

| Source | Folder | Weight | Cycle 1 status |
|---|---|---|---|
| Crash analytics | `crashlytics/` | 100 | unavailable — nothing has ever run |
| Security scanners | `security/` | 95 | empty — no harness code, no dependencies yet |
| Support | `support/` | 45 | unavailable — no users, no channel |
| User voice | `user-voice/` | 40 | **partial** — operator session read; GitHub issues unavailable |
| Product backlog | `backlog/` | 30 | **read in full** — 141 requirements |
| NFRs | `../initiation/nfrs.md` | 20 | **read** — minted `N-9`, `N-10`, `N-11` |
| Analytics | `analytics/` | 10 | unavailable — no telemetry, and the vision forbids a hosted service |
| Market signals | `market/` | 5 | **read** — dated 2026-07-28 |

No project overrides this cycle. The defaults stand.

A note on what these weights mean here: with crash, support and analytics all
silent, the backlog (30) and the operator's own voice (40) carry the cycle. That
is expected for a product with no users and no deployment, and it is the reason
Phase C's coherence rule (C.7) matters more than the raw score right now — the
scores will cluster tightly and the ordering has to come from what makes a
buildable, testable unit.
