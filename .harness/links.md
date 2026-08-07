# LM Links

The links this project can reach, and which role uses which (`M-1`–`M-5`).
Read by the engine from the fenced block below — same convention as
[`binding.md`](binding.md), for the same reason: the document a human reads and
the keys the engine reads cannot drift apart.

Nothing in the harness names a model. It names a **role**, and the router
resolves the role to a link. That is what lets the same loop run against a 4B
on this laptop, a 30B on a rig in the next room, and a cloud API, without a
single call site knowing which.

## Reading the block

| Key | Meaning |
|---|---|
| `link.<name>.kind` | `lmstudio`, `lmlink`, `deepseek`, `claude-cli`, `openai-compat` |
| `link.<name>.base_url` | required except for `lmlink` and `claude-cli`, neither of which is an address |
| `link.<name>.device` | `lmlink` only — the name from `lms link set-device-name` |
| `link.<name>.model` | checked against what the link actually offers at startup (`M-14`) |
| `link.<name>.privacy` | `local` or `cloud`; implied by kind where the kind settles it |
| `link.<name>.auth_env` | the **name** of the environment variable holding the key — never the key (`S-2`) |
| `link.<name>.first_token_seconds` | how long this link may say nothing before it is failed over (`M-23`). Default 20. Raise it for a link that is slow to *begin* answering — a blocked batch and a wedged server look identical until the deadline tells them apart |
| `role.<role>` | an ordered chain; the first healthy, eligible link wins (`M-3`) |
| `deprecated.<id>` | a model id known to be dead, and what replaced it |

Privacy is not free-form. An `lmstudio` or `lmlink` link is always `local` — a
rig you own is yours even when it is in another room — and a `deepseek` or
`claude-cli` link is always `cloud`. Declaring otherwise is a startup error
rather than a preference. A `claude-cli` link runs a command on this machine,
but the machine that answers it is somebody else's, whoever is paying for it.

```perp-links
# On this machine. Small, always available, and the only link that sees
# anything marked local-only.
link.here.kind      = lmstudio
link.here.base_url  = http://localhost:1234
link.here.model     = qwen3-4b-instruct
link.here.concurrency = 1
# No auth_env: LM Studio's server does not require a token by default. Add one
# only if you have enabled auth — a declared variable that is not set makes the
# link fail before it connects, which is `M-24`'s whole point.

# Claude Code, driven as a subprocess. No `base_url` and no `auth_env`: the CLI
# holds its own credential and signs in as you, which is the point — this runs
# against the Max subscription rather than metered API tokens.
#
# Two links rather than one, and not for redundancy. `V-5` says the verifier
# must not be the link that authored the change; two models under one
# subscription is the cheapest way to make that the default rather than a rule
# somebody has to remember.
link.claude.kind  = claude-cli
link.claude.model = sonnet

link.claude-deep.kind  = claude-cli
link.claude-deep.model = opus

# The DeepSeek link is deliberately not declared. It works, the key is set, and
# leaving it in a role chain means a Claude link that fails once quietly hands
# the work to a metered endpoint and the run succeeds — which is exactly what
# happened before `chat_raw` learned to route subprocess links, and the only
# sign was a provenance line naming the wrong model. Commented out rather than
# deleted so the shape is here when a second opinion is wanted on purpose.
#
# link.ds-fast.kind     = deepseek
# link.ds-fast.base_url = https://api.deepseek.com
# link.ds-fast.model    = deepseek-v4-flash
# link.ds-fast.auth_env = DEEPSEEK_API_KEY

# An LM Link peer goes here once `lms link status` names one. Left out rather
# than invented — a configuration that describes hardware nobody has is worse
# than one that is short.
# link.rig.kind   = lmlink
# link.rig.device = <from lms link set-device-name>
# link.rig.model  = qwen3-coder-30b

role.planner    = claude, here
role.coder      = claude, here
role.gatefixer  = claude, here
role.verifier   = claude-deep, here
role.chat       = claude, here
role.compactor  = here
role.classifier = here
role.summarizer = here
role.embedder   = here

# Currency units per million tokens, verified 2026-07-28. In configuration
# because they change — a harness that bakes them in reports yesterday's bill
# with total confidence. A link with no price is free, which is right for a
# local one and a visible zero for a cloud one.
#
# Claude Code is billed by subscription and not by token, so zero is the truth
# about the meter rather than a number nobody filled in. A cycle costing
# $0.000000 in the ledger is the correct report, not a broken one.
price.claude.cache_hit  = 0
price.claude.cache_miss = 0
price.claude.output     = 0

price.claude-deep.cache_hit  = 0
price.claude-deep.cache_miss = 0
price.claude-deep.output     = 0

deprecated.deepseek-chat     = deepseek-v4-flash
deprecated.deepseek-reasoner = deepseek-v4-pro
```

## Why the chains look like this

- **`verifier` is `claude-deep`, and `coder` is `claude`.** `V-5` says the
  verifier must not be the link that authored the change. Two models under one
  subscription is the cheapest way to make that the default rather than a rule
  someone has to remember — and the stronger one reads rather than writes,
  which is the way round that catches things.
- **`compactor`, `classifier` and `embedder` never leave the machine.** They are
  the highest-volume, lowest-stakes calls in the loop; sending them to a paid
  endpoint would cost the most and prove the least. `embedder` in particular is
  not a chat call at all, and `claude-cli` has nothing to answer it with.
- **No metered link is in any chain.** Not because DeepSeek is worse, but
  because a fallback to it is silent: the run succeeds, the work looks done,
  and the only sign is a provenance line naming a model nobody chose. If a
  paid second opinion is wanted, it should be asked for on purpose.
- **Every role has a local option**, so `local-only` mode is a supported
  configuration rather than a broken one (`M-5`, vision clause 2). `perp links
  --local-only` reports any role that loses its last option.

## The two deprecations are not decoration

`deepseek-chat` and `deepseek-reasoner` were deprecated on **2026-07-24**, four
days before this design was written. Anything that had hard-coded them would
have needed a release to say so. They live here, in configuration, because
`M-14` was written by that fact rather than in the abstract.
