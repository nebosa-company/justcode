# Links

Which models this project may use, and for what. Credentials are named here by
environment variable and never written here.

```perp-links
# Pick one or more, uncomment it, and set the variable it names.
#
# `base_url` can be left out for anything with an obvious address — it is only
# needed for a gateway, a proxy, or a local server on another port.

# --- local ---------------------------------------------------------------
# link.local.kind     = lmstudio
# link.local.model    = qwen2.5-coder-32b
# link.local.privacy  = local

# link.ollama.kind    = ollama
# link.ollama.model   = qwen2.5-coder
# link.ollama.privacy = local

# link.vllm.kind      = vllm
# link.vllm.model     = Qwen/Qwen2.5-Coder-32B-Instruct
# link.vllm.privacy   = local

# --- hosted --------------------------------------------------------------
# link.openai.kind      = openai
# link.openai.model     = gpt-5
# link.openai.auth_env  = OPENAI_API_KEY

# link.claude.kind      = anthropic
# link.claude.model     = claude-opus-5
# link.claude.auth_env  = ANTHROPIC_API_KEY

# link.deepseek.kind    = deepseek
# link.deepseek.model   = deepseek-v4
# link.deepseek.auth_env = DEEPSEEK_API_KEY

# link.grok.kind        = grok
# link.grok.model       = grok-4
# link.grok.auth_env    = XAI_API_KEY

# --- a command rather than an endpoint -----------------------------------
# link.claude-code.kind  = claude-cli
# link.claude-code.model = claude-opus-5

# --- roles ---------------------------------------------------------------
# Which link answers for what. A role may name several, tried in order.
# role.coder    = local
# role.chat     = local
# role.verifier = local
```

## Prices

Money is counted from what a link reports, never estimated, so a hosted link
needs its prices in dollars per million tokens:

```
# price.openai.cache_hit  = 0.13
# price.openai.cache_miss = 1.25
# price.openai.output     = 10.00
```

A local link needs none: it reports tokens and time, and spends no money.
