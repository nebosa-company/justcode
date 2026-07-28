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
| `link.<name>.kind` | `lmstudio`, `lmlink`, `deepseek`, `openai-compat` |
| `link.<name>.base_url` | required except for `lmlink`, which is addressed by device |
| `link.<name>.device` | `lmlink` only — the name from `lms link set-device-name` |
| `link.<name>.model` | checked against what the link actually offers at startup (`M-14`) |
| `link.<name>.privacy` | `local` or `cloud`; implied by kind where the kind settles it |
| `link.<name>.auth_env` | the **name** of the environment variable holding the key — never the key (`S-2`) |
| `role.<role>` | an ordered chain; the first healthy, eligible link wins (`M-3`) |
| `deprecated.<id>` | a model id known to be dead, and what replaced it |

Privacy is not free-form. An `lmstudio` or `lmlink` link is always `local` — a
rig you own is yours even when it is in another room — and a `deepseek` link is
always `cloud`. Declaring otherwise is a startup error rather than a preference.

```perp-links
# On this machine. Small, always available, and the only link that sees
# anything marked local-only.
link.here.kind      = lmstudio
link.here.base_url  = http://localhost:1234
link.here.model     = qwen3-4b-instruct
link.here.auth_env  = LMSTUDIO_TOKEN
link.here.concurrency = 1

# A DeepSeek link is configured but unreachable until batch 8 gives the harness
# an HTTPS transport. Listed so the router can be exercised against a cloud
# link, and so `local-only` can be seen to skip it rather than fall back to it.
link.ds-fast.kind     = deepseek
link.ds-fast.base_url = https://api.deepseek.com
link.ds-fast.model    = deepseek-v4-flash
link.ds-fast.auth_env = DEEPSEEK_API_KEY

# An LM Link peer goes here once `lms link status` names one. Left out rather
# than invented — a configuration that describes hardware nobody has is worse
# than one that is short.
# link.rig.kind   = lmlink
# link.rig.device = <from lms link set-device-name>
# link.rig.model  = qwen3-coder-30b

role.planner    = ds-fast, here
role.coder      = ds-fast, here
role.gatefixer  = ds-fast, here
role.verifier   = here, ds-fast
role.chat       = ds-fast, here
role.compactor  = here
role.classifier = here
role.summarizer = here
role.embedder   = here

deprecated.deepseek-chat     = deepseek-v4-flash
deprecated.deepseek-reasoner = deepseek-v4-pro
```

## Why the chains look like this

- **`verifier` is `here` first, then `ds-fast`** — the opposite of `coder`.
  `V-5` says the verifier must not be the link that authored the change, and
  ordering the two chains differently is the cheapest way to make that the
  default rather than a rule someone has to remember.
- **`compactor`, `classifier` and `embedder` never leave the machine.** They are
  the highest-volume, lowest-stakes calls in the loop; sending them to a paid
  endpoint would cost the most and prove the least.
- **Every role has a local option**, so `local-only` mode is a supported
  configuration rather than a broken one (`M-5`, vision clause 2). `perp links
  --local-only` reports any role that loses its last option.

## The two deprecations are not decoration

`deepseek-chat` and `deepseek-reasoner` were deprecated on **2026-07-24**, four
days before this design was written. Anything that had hard-coded them would
have needed a release to say so. They live here, in configuration, because
`M-14` was written by that fact rather than in the abstract.
