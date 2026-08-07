//! LM Links — the router (`M-1`–`M-5`, `M-14`).
//!
//! Nothing in the harness names a model. It names a **role**, and the router
//! resolves the role to a link. That indirection is the whole design: it is
//! what lets the same loop run against a 4B model on the operator's laptop, a
//! 30B on a rig two rooms away, and a cloud API, without a single call site
//! knowing which.
//!
//! Four link kinds (`M-1`), because the wire protocol is not the interesting
//! difference — reachability, latency and trust are:
//!
//! | Kind | What it is |
//! |---|---|
//! | `lmstudio` | LM Studio on this machine, `http://localhost:1234` |
//! | `lmlink` | LM Studio on a machine the operator owns, over a Tailscale-backed link |
//! | `deepseek` | the DeepSeek API |
//! | `openai-compat` | anything else speaking `/v1/chat/completions` — Ollama, llama.cpp, vLLM, LiteLLM, OpenRouter |
//!
//! This module deliberately stops at the network boundary. Everything here is
//! decided from configuration and recorded facts, so the tests run on a machine
//! with no GPU, no server and no API key.

use std::fmt;
use std::time::Duration;

use crate::binding::parse_fenced;
use crate::error::{Error, Result};

/// The fence label for the link configuration.
pub const FENCE: &str = "perp-links";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    LmStudio,
    LmLink,
    DeepSeek,
    OpenAiCompat,
    /// OpenAI proper, which is also what ChatGPT's API is.
    OpenAi,
    /// Anthropic's Messages API. The one provider here that is not
    /// OpenAI-shaped: different path, different auth header, different body.
    Anthropic,
    /// The `claude` command, driven as a subprocess rather than over HTTP.
    ClaudeCli,
    /// xAI. OpenAI-compatible.
    Grok,
    /// Ollama's OpenAI-compatible endpoint, not its native `/api/generate`.
    Ollama,
    /// vLLM's OpenAI-compatible server.
    VLlm,
}

impl Kind {
    /// Where this kind lives, when the binding does not say.
    ///
    /// A default is worth having because these addresses are not a choice: there
    /// is one `api.openai.com`, and Ollama is on 11434 unless somebody moved it.
    /// A binding that names a `base_url` still wins — a local gateway or a proxy
    /// is exactly the case where the default is wrong.
    ///
    /// `None` for the two that have no address: an `lmlink` peer is reached
    /// through a separate link, and `claude-cli` is a program rather than a host.
    pub fn default_base_url(self) -> Option<&'static str> {
        match self {
            Kind::LmStudio => Some("http://localhost:1234"),
            Kind::DeepSeek => Some("https://api.deepseek.com"),
            Kind::OpenAi => Some("https://api.openai.com"),
            Kind::Anthropic => Some("https://api.anthropic.com"),
            Kind::Grok => Some("https://api.x.ai"),
            Kind::Ollama => Some("http://localhost:11434"),
            Kind::VLlm => Some("http://localhost:8000"),
            Kind::LmLink | Kind::ClaudeCli | Kind::OpenAiCompat => None,
        }
    }

    /// The header a credential goes in, and any version header the API demands.
    ///
    /// Everything here is `Authorization: Bearer` except Anthropic, which wants
    /// `x-api-key` and a dated `anthropic-version` — omit the latter and the API
    /// refuses the request rather than picking a default.
    pub fn auth_header(self) -> &'static str {
        match self {
            Kind::Anthropic => "x-api-key",
            _ => "Authorization",
        }
    }

    /// Whether the credential is sent as `Bearer <key>` or bare.
    pub fn bearer_prefixed(self) -> bool {
        !matches!(self, Kind::Anthropic)
    }

    /// Whether this kind speaks OpenAI's `/v1/chat/completions`.
    ///
    /// Six of the nine do, which is why adding most providers is a name and a
    /// default rather than a new wire format.
    pub fn is_openai_shaped(self) -> bool {
        !matches!(self, Kind::Anthropic | Kind::ClaudeCli | Kind::LmLink)
    }

    /// Whether this kind is a program rather than an endpoint.
    pub fn is_subprocess(self) -> bool {
        matches!(self, Kind::ClaudeCli)
    }

    /// Whether a prefix cache can help this kind at all (`M-29`).
    ///
    /// `M-12` assembles a prompt stable-prefix first so the provider can charge
    /// the unchanged head at the cached rate, and every column of cache figures
    /// in this harness assumes that is worth doing.
    ///
    /// It cannot be, for a subprocess link. The system prompt is replaced on
    /// every call — that replacement is what buys the tool protocol — so the
    /// stable region is not stable and there is nothing for a cache to hit.
    /// `M-12` does not apply, and a run through such a link reports zero cached
    /// tokens forever.
    ///
    /// That number is correct and reads exactly like a broken ledger, which is
    /// the failure this answers: not the zeroes, but that nothing distinguishes
    /// *this link cannot cache* from *the accounting is wrong*.
    pub fn prefix_caches(self) -> bool {
        !self.is_subprocess()
    }

    pub fn parse(text: &str) -> Result<Kind> {
        match text {
            "lmstudio" => Ok(Kind::LmStudio),
            "lmlink" => Ok(Kind::LmLink),
            "deepseek" => Ok(Kind::DeepSeek),
            "openai-compat" => Ok(Kind::OpenAiCompat),
            "openai" => Ok(Kind::OpenAi),
            "anthropic" => Ok(Kind::Anthropic),
            "claude-cli" => Ok(Kind::ClaudeCli),
            "grok" => Ok(Kind::Grok),
            "ollama" => Ok(Kind::Ollama),
            "vllm" => Ok(Kind::VLlm),
            other => Err(Error::unbound(
                "link kind",
                format!(
                    "`{other}` is not one of lmstudio, lmlink, deepseek, openai, anthropic,                      claude-cli, grok, ollama, vllm, openai-compat"
                ),
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::LmStudio => "lmstudio",
            Kind::LmLink => "lmlink",
            Kind::DeepSeek => "deepseek",
            Kind::OpenAiCompat => "openai-compat",
            Kind::OpenAi => "openai",
            Kind::Anthropic => "anthropic",
            Kind::ClaudeCli => "claude-cli",
            Kind::Grok => "grok",
            Kind::Ollama => "ollama",
            Kind::VLlm => "vllm",
        }
    }

    /// Does this kind serve LM Studio's native `/api/v0` surface, with its
    /// model facts — quantization, loaded state, real context length (`M-7`)?
    pub fn has_native_api(self) -> bool {
        matches!(self, Kind::LmStudio | Kind::LmLink)
    }

    /// The privacy class this kind can never be configured out of.
    ///
    /// `Some` only where the answer cannot be otherwise. Everything else is
    /// `None` and the operator declares it, which is the conservative direction
    /// on a boundary whose whole job is stopping data leaving (`M-4`).
    fn implied_privacy(self) -> Option<Privacy> {
        match self {
            // Local by construction: neither has a hosted API to point at.
            Kind::LmStudio | Kind::LmLink => Some(Privacy::Local),
            // Somebody else's computer, always.
            Kind::DeepSeek | Kind::OpenAi | Kind::Anthropic | Kind::Grok => Some(Privacy::Cloud),
            // A program rather than an endpoint, and one that talks to Anthropic
            // on its own account. `local-only` must not reach for it thinking it
            // is a local model.
            Kind::ClaudeCli => Some(Privacy::Cloud),
            // Self-hosted, and usually on this machine — but the default base_url
            // is only a default, and one pointed at a box across the internet
            // would carry `Local` into a `local-only` run and leak. Declared
            // rather than assumed.
            Kind::Ollama | Kind::VLlm => None,
            // Could be a container on this machine or a proxy on the internet.
            Kind::OpenAiCompat => None,
        }
    }
}

/// Where a link's traffic is allowed to go (`M-4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// Hardware the operator owns. Includes an `lmlink` peer — the rig is
    /// remote, but it is theirs.
    Local,
    Cloud,
}

impl Privacy {
    pub fn parse(text: &str) -> Result<Privacy> {
        match text {
            "local" => Ok(Privacy::Local),
            "cloud" => Ok(Privacy::Cloud),
            other => Err(Error::unbound("privacy", format!("`{other}` is not local or cloud"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Privacy::Local => "local",
            Privacy::Cloud => "cloud",
        }
    }
}

/// What the loop is allowed to reach for on this run (`M-5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Cloud links are disabled. A supported configuration, not a degraded one.
    LocalOnly,
    #[default]
    Any,
}

/// The jobs a call can be for (`M-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Planner,
    Coder,
    GateFixer,
    Verifier,
    Chat,
    Compactor,
    Classifier,
    Summarizer,
    Embedder,
}

impl Role {
    pub const ALL: &'static [Role] = &[
        Role::Planner,
        Role::Coder,
        Role::GateFixer,
        Role::Verifier,
        Role::Chat,
        Role::Compactor,
        Role::Classifier,
        Role::Summarizer,
        Role::Embedder,
    ];

    pub fn parse(text: &str) -> Result<Role> {
        let role = match text {
            "planner" => Role::Planner,
            "coder" => Role::Coder,
            "gatefixer" => Role::GateFixer,
            "verifier" => Role::Verifier,
            "chat" => Role::Chat,
            "compactor" => Role::Compactor,
            "classifier" => Role::Classifier,
            "summarizer" => Role::Summarizer,
            "embedder" => Role::Embedder,
            other => {
                return Err(Error::unbound("role", format!("`{other}` is not a known role")))
            }
        };
        Ok(role)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Planner => "planner",
            Role::Coder => "coder",
            Role::GateFixer => "gatefixer",
            Role::Verifier => "verifier",
            Role::Chat => "chat",
            Role::Compactor => "compactor",
            Role::Classifier => "classifier",
            Role::Summarizer => "summarizer",
            Role::Embedder => "embedder",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad`, not `write_str`: the latter ignores the width in `{role:<11}`,
        // and a column that silently refuses to align is a small lie about
        // whether the formatting was considered.
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    pub kind: Kind,
    /// Absent for `lmlink`, which is addressed by device rather than by URL.
    pub base_url: Option<String>,
    /// `lmlink` only — the peer's name from `lms link set-device-name`.
    pub device: Option<String>,
    pub model: String,
    pub privacy: Privacy,
    /// The **name** of the environment variable holding the key. Never the key
    /// itself: `S-2` says secrets are referenced, not stored.
    pub auth_env: Option<String>,
    /// One GPU serving one model does not want four parallel requests (`M-15`).
    pub concurrency: u32,
    /// How hard the model should think, for links whose kind can be told
    /// (`M-34`). `None` leaves whatever the provider defaults to.
    ///
    /// Declared rather than inherited: a run that reasons at one level today
    /// and another tomorrow, because a CLI changed its default, is a run whose
    /// results cannot be compared with its own past.
    pub effort: Option<String>,
    /// How long this link may say nothing before it is failed over (`M-23`).
    ///
    /// [`crate::stream::FIRST_TOKEN_SECONDS`] is the default and was, until
    /// this line existed, the only value — a provider fact compiled into the
    /// binary, which is the thing `M-14` says not to do. Time to first token
    /// varies by link and by load in a way no constant can know: measured over
    /// a 101-task benchmark run, two batches were blocked because a cloud link
    /// took longer than twenty seconds to begin answering.
    pub first_token: Duration,
}

impl Link {
    /// Is this link reachable at all without leaving the operator's hardware?
    pub fn is_local(&self) -> bool {
        self.privacy == Privacy::Local
    }

    pub fn describe(&self) -> String {
        let where_ = match (&self.base_url, &self.device) {
            (Some(url), _) => url.clone(),
            (None, Some(device)) => format!("device {device}"),
            (None, None) => "unaddressed".to_string(),
        };
        // `M-34`: shown when declared, absent when not. A binding key that
        // changes how every call reasons and appears nowhere a person looks is
        // the same kind of gap it exists to close — an effort you cannot see is
        // an effort you cannot compare a run against.
        let effort = match &self.effort {
            Some(level) => format!(" · {level} effort"),
            None => String::new(),
        };
        format!(
            "{} [{} · {} · {}{}] {}",
            self.name,
            self.kind.as_str(),
            self.privacy.as_str(),
            self.model,
            effort,
            where_
        )
    }
}

/// Per-link concurrency (`M-15`).
///
/// One GPU serving one model does not want four parallel requests: they do not
/// go faster, they queue inside the server, and the timeouts start firing at
/// the wrong layer. The limit is the link's `concurrency`, which defaults to 1.
///
/// Shared between threads by design — the engine is single-threaded today, and
/// this is the thing that has to already be right on the day it is not.
#[derive(Debug, Default)]
pub struct Permits {
    in_flight: std::sync::Mutex<Vec<(String, u32)>>,
}

/// Held for the duration of a call; releases on drop, including on a panic or
/// an early return.
///
/// Owns a handle to the pool rather than borrowing it, so holding a permit does
/// not lock the caller out of the thing that issued it.
#[derive(Debug)]
pub struct Permit {
    permits: std::sync::Arc<Permits>,
    link: String,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Ok(mut in_flight) = self.permits.in_flight.lock() {
            if let Some(entry) = in_flight.iter_mut().find(|(name, _)| name == &self.link) {
                entry.1 = entry.1.saturating_sub(1);
            }
        }
    }
}

impl Permits {
    pub fn new() -> Permits {
        Permits::default()
    }

    /// `None` when the link is already at its limit. The caller waits or picks
    /// another link; it does not get to exceed it.
    pub fn acquire(self: &std::sync::Arc<Self>, link: &Link) -> Option<Permit> {
        let mut in_flight = self.in_flight.lock().ok()?;
        let slot = match in_flight.iter_mut().find(|(name, _)| name == &link.name) {
            Some(entry) => entry,
            None => {
                in_flight.push((link.name.clone(), 0));
                in_flight.last_mut()?
            }
        };
        if slot.1 >= link.concurrency.max(1) {
            return None;
        }
        slot.1 += 1;
        Some(Permit { permits: std::sync::Arc::clone(self), link: link.name.clone() })
    }

    pub fn in_flight(&self, link: &Link) -> u32 {
        self.in_flight
            .lock()
            .ok()
            .and_then(|held| held.iter().find(|(name, _)| name == &link.name).map(|(_, n)| *n))
            .unwrap_or(0)
    }
}

/// Whether a link can be used right now. Implemented against a real endpoint in
/// batch 8; until then the router is testable with a fake.
pub trait Health {
    fn is_healthy(&self, link: &Link) -> bool;
}

/// Everything reachable is healthy. The router's behaviour without a probe.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssumeHealthy;

impl Health for AssumeHealthy {
    fn is_healthy(&self, _link: &Link) -> bool {
        true
    }
}

/// The configured links, the role chains, and the model ids known to be dead.
#[derive(Debug, Clone, Default)]
pub struct Links {
    links: Vec<Link>,
    chains: Vec<(Role, Vec<String>)>,
    /// `M-14`: a dead id and what replaced it, from configuration rather than
    /// baked into the binary. `deepseek-chat` died on 2026-07-24; a harness
    /// that hard-coded it would have needed a release to say so.
    deprecated: Vec<(String, String)>,
    /// Per-link prices, for the same reason: they change (`M-11`).
    prices: Vec<(String, crate::cost::Price)>,
}

impl Links {
    pub fn load(path: &std::path::Path) -> Result<Links> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::unbound("links", format!("cannot read {}: {e}", path.display())))?;
        Links::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Links> {
        let entries = parse_fenced(text, FENCE, "links")?;
        let mut names: Vec<String> = Vec::new();
        let mut fields: Vec<(String, String, String)> = Vec::new();
        let mut chains: Vec<(Role, Vec<String>)> = Vec::new();
        let mut deprecated: Vec<(String, String)> = Vec::new();

        for (key, value) in &entries {
            if let Some(rest) = key.strip_prefix("link.") {
                let Some((name, field)) = rest.split_once('.') else {
                    return Err(Error::unbound(key, "expected `link.<name>.<field>`"));
                };
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
                fields.push((name.to_string(), field.to_string(), value.clone()));
            } else if let Some(role) = key.strip_prefix("role.") {
                let role = Role::parse(role)?;
                let chain: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if chain.is_empty() {
                    return Err(Error::unbound(key, "names no links"));
                }
                chains.push((role, chain));
            } else if let Some(dead) = key.strip_prefix("deprecated.") {
                deprecated.push((dead.to_string(), value.clone()));
            } else if key.starts_with("price.") {
                // Parsed below, in one pass, so a malformed price names itself.
            } else {
                return Err(Error::unbound(
                    key,
                    "expected `link.*`, `role.*` or `deprecated.*`",
                ));
            }
        }

        let mut links = Vec::new();
        for name in names {
            links.push(build_link(&name, &fields)?);
        }

        let prices = crate::cost::prices(&entries)?;
        let result = Links { links, chains, deprecated, prices };
        result.check_chains()?;
        result.check_prices()?;
        Ok(result)
    }

    /// Every link a chain names must exist. A chain pointing at a link that was
    /// renamed fails at load, not at the first call that needs it.
    fn check_chains(&self) -> Result<()> {
        for (role, chain) in &self.chains {
            for name in chain {
                if !self.links.iter().any(|link| &link.name == name) {
                    return Err(Error::unbound(
                        format!("role.{role}"),
                        format!("names `{name}`, which is not a configured link"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Every price must belong to a link that exists, or a typo silently
    /// prices nothing.
    fn check_prices(&self) -> Result<()> {
        for (name, _) in &self.prices {
            if !self.links.iter().any(|link| &link.name == name) {
                return Err(Error::unbound(
                    format!("price.{name}"),
                    "is not a configured link",
                ));
            }
        }
        Ok(())
    }

    /// What this link charges. A link with no configured price is **free**,
    /// which is right for a local one and a visible zero for a cloud one —
    /// a bill of nothing next to a cloud link is a configuration bug you can
    /// see, rather than a guess at what DeepSeek costs this week.
    /// Every link whose `auth_env` names a variable that is not set (`M-24`),
    /// **among the links a role chain could actually reach** (`M-26`).
    ///
    /// Checked when the project is bound, not at first use. A typo in a
    /// variable name that surfaces on the eleventh call of a batch has already
    /// cost an hour, and the 401 it produces then blames the request rather
    /// than the configuration.
    ///
    /// A link nothing names is not in this list. A workspace configured
    /// entirely for one provider commonly declares a link for another that no
    /// chain ever mentions — kept around, unused, maybe for later. `M-24`
    /// checking it anyway refused two runs over a variable that was never
    /// going to be read, and the only fix was to set it for an endpoint that
    /// was never going to be called.
    ///
    /// Returns the **variable names**, never their values (`S-2`).
    pub fn missing_credentials(&self) -> Vec<(String, String)> {
        self.links
            .iter()
            .filter(|link| self.chains.iter().any(|(_, chain)| chain.contains(&link.name)))
            .filter_map(|link| {
                let name = link.auth_env.as_ref()?;
                std::env::var(name).is_err().then(|| (link.name.clone(), name.clone()))
            })
            .collect()
    }

    pub fn price(&self, name: &str) -> crate::cost::Price {
        self.prices
            .iter()
            .find(|(link, _)| link == name)
            .map(|(_, price)| *price)
            .unwrap_or(crate::cost::Price::FREE)
    }

    pub fn all(&self) -> &[Link] {
        &self.links
    }

    pub fn roles(&self) -> impl Iterator<Item = (Role, &[String])> {
        self.chains.iter().map(|(role, chain)| (*role, chain.as_slice()))
    }

    pub fn get(&self, name: &str) -> Result<&Link> {
        self.links
            .iter()
            .find(|link| link.name == name)
            .ok_or_else(|| Error::unbound(format!("link {name}"), "is not configured"))
    }

    /// The links a role would try, in order (`M-3`).
    pub fn chain(&self, role: Role) -> Result<Vec<&Link>> {
        let (_, names) = self
            .chains
            .iter()
            .find(|(candidate, _)| *candidate == role)
            .ok_or_else(|| {
                Error::unbound(format!("role.{role}"), "has no chain — every call needs one")
            })?;
        names.iter().map(|name| self.get(name)).collect()
    }

    /// Resolve a role to the link that will actually serve it (`M-2`, `M-3`).
    ///
    /// The first healthy, eligible link wins. `local-only` removes cloud links
    /// from consideration entirely rather than falling back to them, because a
    /// silent promotion across the privacy boundary is the one failure mode
    /// `M-4` exists to prevent.
    pub fn resolve(&self, role: Role, health: &dyn Health, mode: Mode) -> Result<&Link> {
        let chain = self.chain(role)?;
        let eligible: Vec<&&Link> = chain
            .iter()
            .filter(|link| mode == Mode::Any || link.is_local())
            .collect();

        if eligible.is_empty() {
            return Err(Error::unbound(
                format!("role.{role}"),
                format!(
                    "no local link — the chain is [{}], and this run is local-only. \
                     A cloud link is not substituted; that would cross the privacy boundary silently.",
                    chain
                        .iter()
                        .map(|l| format!("{}({})", l.name, l.privacy.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }

        for link in &eligible {
            if health.is_healthy(link) {
                return Ok(link);
            }
        }

        // `M-20`: in a local-only run this is a **park**, not a failure to be
        // worked around. The chain may well contain a healthy cloud link, and
        // the whole point is that it is not reached for.
        let parked = mode == Mode::LocalOnly && chain.iter().any(|link| !link.is_local());
        Err(Error::unbound(
            format!("role.{role}"),
            format!(
                "every eligible link is unhealthy: [{}].{}",
                eligible.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", "),
                if parked {
                    " This parks. The chain has a cloud link that is not unhealthy, and local-only does not promote it — a peer going away is not consent to send the work somewhere else."
                } else {
                    ""
                }
            ),
        ))
    }

    /// Can this configuration run with no cloud at all (`M-5`)?
    ///
    /// Answered for every role at once, because "local-only works" is a claim
    /// about the whole loop, not about one call.
    pub fn local_only_gaps(&self) -> Vec<Role> {
        let mut gaps = Vec::new();
        for (role, _) in self.roles() {
            let usable = self
                .chain(role)
                .map(|chain| chain.iter().any(|link| link.is_local()))
                .unwrap_or(false);
            if !usable {
                gaps.push(role);
            }
        }
        gaps
    }

    /// Check a link's model id against what the provider actually offers
    /// (`M-14`).
    ///
    /// A missing id is a startup error naming its replacement when the
    /// configuration knows one — never a silent fallback to whatever is loaded.
    pub fn check_model(&self, link: &Link, available: &[String]) -> Result<()> {
        if available.iter().any(|id| id == &link.model) {
            return Ok(());
        }
        if let Some((_, replacement)) = self
            .deprecated
            .iter()
            .find(|(dead, _)| dead == &link.model)
        {
            return Err(Error::unbound(
                format!("link.{}.model", link.name),
                format!(
                    "`{}` is deprecated; use `{replacement}`. Nothing is substituted automatically.",
                    link.model
                ),
            ));
        }
        Err(Error::unbound(
            format!("link.{}.model", link.name),
            format!(
                "`{}` is not offered by this link. Available: {}",
                link.model,
                if available.is_empty() {
                    "nothing — the link returned no models".to_string()
                } else {
                    available.join(", ")
                }
            ),
        ))
    }
}

fn build_link(name: &str, fields: &[(String, String, String)]) -> Result<Link> {
    let field = |wanted: &str| {
        fields
            .iter()
            .find(|(link, field, _)| link == name && field == wanted)
            .map(|(_, _, value)| value.clone())
    };
    let required = |wanted: &str| {
        field(wanted).ok_or_else(|| {
            Error::unbound(format!("link.{name}.{wanted}"), "is required but not declared")
        })
    };

    let kind = Kind::parse(&required("kind")?)?;
    let privacy = match field("privacy") {
        Some(text) => {
            let declared = Privacy::parse(&text)?;
            if let Some(implied) = kind.implied_privacy() {
                if implied != declared {
                    return Err(Error::unbound(
                        format!("link.{name}.privacy"),
                        format!(
                            "declared `{}`, but a `{}` link is always `{}`",
                            declared.as_str(),
                            kind.as_str(),
                            implied.as_str()
                        ),
                    ));
                }
            }
            declared
        }
        None => kind.implied_privacy().ok_or_else(|| {
            Error::unbound(
                format!("link.{name}.privacy"),
                "an openai-compat link could be on this machine or on the internet — say which",
            )
        })?,
    };

    if kind == Kind::LmLink && field("device").is_none() {
        return Err(Error::unbound(
            format!("link.{name}.device"),
            "an lmlink peer is addressed by device name, from `lms link set-device-name`",
        ));
    }
    // Required only where there is nothing to fall back to.
    //
    // A kind with an obvious address carries its own default now, so stating
    // `api.openai.com` in every binding is no longer the price of using OpenAI.
    // `lmlink` is addressed by device, and `claude-cli` is a program rather than a
    // host — asking either for a URL made a `claude-cli` link impossible to
    // configure at all, which a subprocess test found by not being able to build
    // one.
    if field("base_url").is_none()
        && kind != Kind::LmLink
        && !kind.is_subprocess()
        && kind.default_base_url().is_none()
    {
        return Err(Error::unbound(
            format!("link.{name}.base_url"),
            format!("is required — `{}` has no default address", kind.as_str()),
        ));
    }

    let concurrency = match field("concurrency") {
        Some(text) => text.trim().parse().map_err(|_| {
            Error::unbound(format!("link.{name}.concurrency"), format!("`{text}` is not a number"))
        })?,
        None => 1,
    };

    // Refused rather than passed on: an effort the provider does not know is a
    // flag that fails at the far end, after the call has been set up, and the
    // error blames the invocation rather than the binding line that caused it.
    const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
    let effort = match field("effort") {
        Some(text) => {
            let text = text.trim().to_ascii_lowercase();
            if !EFFORTS.contains(&text.as_str()) {
                return Err(Error::unbound(
                    format!("link.{name}.effort"),
                    format!("`{text}` is not one of {}", EFFORTS.join(", ")),
                ));
            }
            Some(text)
        }
        None => None,
    };

    // Refused rather than clamped, and zero refused with it: a deadline of no
    // time fails every link on its first call, and a binding that says so is
    // more likely to be a typo than an intention.
    let first_token = match field("first_token_seconds") {
        Some(text) => {
            let seconds: u64 = text.trim().parse().map_err(|_| {
                Error::unbound(
                    format!("link.{name}.first_token_seconds"),
                    format!("`{text}` is not a number of seconds"),
                )
            })?;
            if seconds == 0 {
                return Err(Error::unbound(
                    format!("link.{name}.first_token_seconds"),
                    "must be at least 1 — a deadline of no time fails every call".to_string(),
                ));
            }
            Duration::from_secs(seconds)
        }
        None => Duration::from_secs(crate::stream::FIRST_TOKEN_SECONDS),
    };

    Ok(Link {
        name: name.to_string(),
        kind,
        base_url: field("base_url"),
        device: field("device"),
        model: required("model")?,
        privacy,
        auth_env: field("auth_env"),
        concurrency,
        effort,
        first_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_parses_back_from_the_name_it_prints() {
        // The catalog and the parser are two lists that have to agree, and the
        // failure is a binding that names a kind the harness prints but cannot
        // read.
        for name in [
            "lmstudio", "lmlink", "deepseek", "openai-compat", "openai", "anthropic",
            "claude-cli", "grok", "ollama", "vllm",
        ] {
            let kind = Kind::parse(name).expect(name);
            assert_eq!(kind.as_str(), name, "round trip for {name}");
        }
        assert!(Kind::parse("chatgpt").is_err(), "a plausible name is still not one of them");
    }

    #[test]
    fn a_hosted_kind_cannot_be_declared_local() {
        // `M-4`. The direction that matters: a mistake here sends work to
        // somebody else's computer in a run the operator asked to keep at home.
        for kind in [Kind::OpenAi, Kind::Anthropic, Kind::Grok, Kind::DeepSeek, Kind::ClaudeCli] {
            assert_eq!(
                kind.implied_privacy(),
                Some(Privacy::Cloud),
                "{} is hosted",
                kind.as_str()
            );
        }
        for kind in [Kind::LmStudio, Kind::LmLink] {
            assert_eq!(kind.implied_privacy(), Some(Privacy::Local), "{}", kind.as_str());
        }
        // Self-hosted but reachable over a network: the operator declares it,
        // because the default base_url is only a default and one pointed across
        // the internet would carry `Local` into a `local-only` run.
        for kind in [Kind::Ollama, Kind::VLlm, Kind::OpenAiCompat] {
            assert_eq!(kind.implied_privacy(), None, "{} is undetermined", kind.as_str());
        }
    }

    #[test]
    fn a_kind_with_one_obvious_address_carries_it() {
        assert_eq!(Kind::OpenAi.default_base_url(), Some("https://api.openai.com"));
        assert_eq!(Kind::Anthropic.default_base_url(), Some("https://api.anthropic.com"));
        assert_eq!(Kind::Grok.default_base_url(), Some("https://api.x.ai"));
        assert_eq!(Kind::Ollama.default_base_url(), Some("http://localhost:11434"));
        assert_eq!(Kind::VLlm.default_base_url(), Some("http://localhost:8000"));
        // Neither of these is an address: one is reached through another link, the
        // other is a program.
        assert_eq!(Kind::LmLink.default_base_url(), None);
        assert_eq!(Kind::ClaudeCli.default_base_url(), None);
    }

    #[test]
    fn only_anthropic_departs_from_bearer() {
        assert_eq!(Kind::Anthropic.auth_header(), "x-api-key");
        assert!(!Kind::Anthropic.bearer_prefixed());
        for kind in [Kind::OpenAi, Kind::Grok, Kind::DeepSeek, Kind::Ollama, Kind::VLlm] {
            assert_eq!(kind.auth_header(), "Authorization", "{}", kind.as_str());
            assert!(kind.bearer_prefixed(), "{}", kind.as_str());
        }
    }

    #[test]
    fn the_openai_shaped_kinds_are_the_ones_that_need_no_new_wire_format() {
        for kind in [Kind::OpenAi, Kind::Grok, Kind::Ollama, Kind::VLlm, Kind::DeepSeek, Kind::LmStudio] {
            assert!(kind.is_openai_shaped(), "{}", kind.as_str());
        }
        assert!(!Kind::Anthropic.is_openai_shaped(), "a different body and path");
        assert!(!Kind::ClaudeCli.is_openai_shaped(), "not HTTP at all");
        assert!(Kind::ClaudeCli.is_subprocess());
        assert!(!Kind::Anthropic.is_subprocess());
    }

    #[test]
    fn a_credential_is_checked_when_the_project_is_bound() {
        // `M-24`. A typo that surfaces on the eleventh call has already cost an
        // hour, and the 401 it produces then blames the request rather than the
        // configuration.
        //
        // `cloud` is in the chain here. It was not when this test was written,
        // because `M-24` checked every declared link and the distinction did
        // not exist; `M-26` made it exist, and the unreachable case moved to
        // the test below rather than being deleted along with the assertion.
        std::env::remove_var("PERP_DEFINITELY_UNSET_KEY");
        let links = Links::parse(
            "```perp-links
             link.here.kind = lmstudio
             link.here.base_url = http://localhost:1234
             link.here.model = small
             link.cloud.kind = deepseek
             link.cloud.base_url = https://api.deepseek.com
             link.cloud.model = m
             link.cloud.auth_env = PERP_DEFINITELY_UNSET_KEY
             role.chat = here, cloud
```
",
        )
        .expect("parse");

        let missing = links.missing_credentials();
        assert_eq!(missing.len(), 1, "{missing:?}");
        assert_eq!(missing[0].0, "cloud");
        assert_eq!(missing[0].1, "PERP_DEFINITELY_UNSET_KEY", "the name, never the value");

        std::env::set_var("PERP_DEFINITELY_UNSET_KEY", "sk-should-never-appear");
        assert!(links.missing_credentials().is_empty(), "set is not missing");
        std::env::remove_var("PERP_DEFINITELY_UNSET_KEY");
    }

    /// `M-26`. The narrowing, stated as its own contract rather than left as
    /// the absence of an assertion somewhere else.
    ///
    /// A workspace configured for one provider commonly declares a link for
    /// another that no chain names — kept for later, or left behind. Refusing
    /// to start over its unset variable stops a run that was never going to
    /// call it, and the only way through is to set a credential for an endpoint
    /// nothing would have reached.
    #[test]
    fn a_link_no_role_chain_names_is_not_credential_checked() {
        std::env::remove_var("PERP_DEFINITELY_UNSET_KEY");
        let links = Links::parse(
            "```perp-links
             link.here.kind = lmstudio
             link.here.base_url = http://localhost:1234
             link.here.model = small
             link.unused.kind = deepseek
             link.unused.base_url = https://api.deepseek.com
             link.unused.model = m
             link.unused.auth_env = PERP_DEFINITELY_UNSET_KEY
             role.chat = here
```
",
        )
        .expect("parse");

        assert!(
            links.missing_credentials().is_empty(),
            "a link nothing can reach is not a reason to refuse the run: {:?}",
            links.missing_credentials()
        );
        // And it is still a declared link — narrowed, not dropped.
        assert!(links.get("unused").is_ok(), "the link is still configured");
    }

    const CONFIG: &str = "\
```perp-links
link.here.kind      = lmstudio
link.here.base_url  = http://localhost:1234
link.here.model     = qwen3-4b-instruct
link.here.auth_env  = LMSTUDIO_TOKEN

link.rig.kind       = lmlink
link.rig.device     = workshop-4090
link.rig.model      = qwen3-coder-30b

link.ds.kind        = deepseek
link.ds.base_url    = https://api.deepseek.com
link.ds.model       = deepseek-v4-flash
link.ds.auth_env    = DEEPSEEK_API_KEY

role.coder     = ds, rig
role.verifier  = rig, ds
role.compactor = here

deprecated.deepseek-chat     = deepseek-v4-flash
deprecated.deepseek-reasoner = deepseek-v4-pro
```
";

    struct Down(&'static str);

    impl Health for Down {
        fn is_healthy(&self, link: &Link) -> bool {
            link.name != self.0
        }
    }

    struct AllDown;

    impl Health for AllDown {
        fn is_healthy(&self, _link: &Link) -> bool {
            false
        }
    }

    fn links() -> Links {
        Links::parse(CONFIG).expect("parse")
    }

    #[test]
    fn reads_links_roles_and_deprecations() {
        let links = links();
        assert_eq!(links.all().len(), 3);
        assert_eq!(links.roles().count(), 3);

        let rig = links.get("rig").expect("rig");
        assert_eq!(rig.kind, Kind::LmLink);
        assert_eq!(rig.device.as_deref(), Some("workshop-4090"));
        assert_eq!(rig.base_url, None, "an lmlink peer has no URL of its own");
    }

    /// `M-23`, `M-14`: the first-token deadline is a fact about a provider
    /// under load, so a link may say what its own is. Twenty seconds is the
    /// default and was, until this key existed, the only value.
    #[test]
    fn a_link_may_name_its_own_first_token_deadline() {
        let links = links();
        assert_eq!(
            links.get("ds").expect("ds").first_token,
            Duration::from_secs(crate::stream::FIRST_TOKEN_SECONDS),
            "unstated is the default, not zero"
        );

        let text = CONFIG.replace(
            "link.ds.model       = deepseek-v4-flash",
            "link.ds.model       = deepseek-v4-flash\nlink.ds.first_token_seconds = 90",
        );
        let links = Links::parse(&text).expect("parse");
        assert_eq!(links.get("ds").expect("ds").first_token, Duration::from_secs(90));
        // And only that link — a deadline is not a global.
        assert_eq!(
            links.get("here").expect("here").first_token,
            Duration::from_secs(crate::stream::FIRST_TOKEN_SECONDS)
        );
    }

    /// Refused rather than clamped. A deadline of no time fails every call on
    /// the link, and a binding that says so is likelier a typo than a choice.
    #[test]
    fn a_first_token_deadline_that_cannot_work_is_refused() {
        for (value, expected) in [("0", "at least 1"), ("soon", "not a number")] {
            let text = CONFIG.replace(
                "link.ds.model       = deepseek-v4-flash",
                &format!("link.ds.model       = deepseek-v4-flash\nlink.ds.first_token_seconds = {value}"),
            );
            let err = Links::parse(&text).expect_err("must refuse");
            assert!(format!("{err}").contains(expected), "{value}: {err}");
        }
    }

    #[test]
    fn privacy_is_implied_by_kind_when_it_can_be() {
        let links = links();
        assert!(links.get("here").expect("here").is_local());
        assert!(links.get("rig").expect("rig").is_local(), "a rig you own is local");
        assert!(!links.get("ds").expect("ds").is_local());
    }

    #[test]
    fn a_link_cannot_declare_a_privacy_its_kind_forbids() {
        let bad = CONFIG.replace(
            "link.ds.model       = deepseek-v4-flash",
            "link.ds.model       = deepseek-v4-flash\nlink.ds.privacy = local",
        );
        let err = Links::parse(&bad).expect_err("must refuse");
        assert!(format!("{err}").contains("always `cloud`"), "{err}");
    }

    #[test]
    fn an_openai_compat_link_must_say_where_it_lives() {
        let text = "```perp-links\n\
                    link.x.kind = openai-compat\n\
                    link.x.base_url = http://example.test\n\
                    link.x.model = whatever\n\
                    role.coder = x\n```\n";
        let err = Links::parse(text).expect_err("must refuse");
        assert!(format!("{err}").contains("say which"), "{err}");
    }

    #[test]
    fn a_role_resolves_to_the_first_healthy_link_in_its_chain() {
        // `M-3`.
        let links = links();
        let chosen = links.resolve(Role::Coder, &AssumeHealthy, Mode::Any).expect("resolve");
        assert_eq!(chosen.name, "ds", "first in the chain");

        let chosen = links.resolve(Role::Coder, &Down("ds"), Mode::Any).expect("resolve");
        assert_eq!(chosen.name, "rig", "the next one when the first is down");
    }

    #[test]
    fn local_only_removes_cloud_links_rather_than_falling_back_to_them() {
        // `M-4`, `M-5`: the failure mode being prevented is a *silent* promotion
        // across the privacy boundary.
        let links = links();
        let chosen = links.resolve(Role::Coder, &AssumeHealthy, Mode::LocalOnly).expect("resolve");
        assert_eq!(chosen.name, "rig", "the cloud link is skipped, not preferred");

        // And when the chain has nothing local, it fails loudly.
        let text = "```perp-links\n\
                    link.ds.kind = deepseek\n\
                    link.ds.base_url = https://api.deepseek.com\n\
                    link.ds.model = deepseek-v4-flash\n\
                    role.coder = ds\n```\n";
        let cloud_only = Links::parse(text).expect("parse");
        let err = cloud_only
            .resolve(Role::Coder, &AssumeHealthy, Mode::LocalOnly)
            .expect_err("must refuse");
        let message = format!("{err}");
        assert!(message.contains("local-only"), "{message}");
        assert!(message.contains("not substituted"), "{message}");
    }

    #[test]
    fn every_link_being_down_is_a_different_error_from_none_being_eligible() {
        let links = links();
        let err = links.resolve(Role::Coder, &AllDown, Mode::Any).expect_err("must fail");
        assert!(format!("{err}").contains("unhealthy"), "{err}");
    }

    #[test]
    fn a_dead_local_peer_parks_rather_than_promoting_the_cloud_link() {
        // `M-20`. The chain is `ds, rig`; in a local-only run with the rig
        // gone, the cloud link is right there, healthy, and must not be used.
        let links = links();
        let err = links
            .resolve(Role::Coder, &Down("rig"), Mode::LocalOnly)
            .expect_err("must not fall back to the cloud");
        let text = format!("{err}");
        assert!(text.contains("This parks"), "{text}");
        assert!(text.contains("not consent to send the work somewhere else"), "{text}");

        // Outside local-only the same failure is an ordinary one.
        let err = links.resolve(Role::Coder, &AllDown, Mode::Any).expect_err("must fail");
        assert!(!format!("{err}").contains("This parks"), "nothing to park for");
    }

    #[test]
    fn local_only_gaps_are_reported_for_the_whole_configuration() {
        // `M-5` is a claim about the loop, not about one call.
        assert!(links().local_only_gaps().is_empty(), "every role has a local option");

        let text = "```perp-links\n\
                    link.ds.kind = deepseek\n\
                    link.ds.base_url = https://api.deepseek.com\n\
                    link.ds.model = deepseek-v4-flash\n\
                    link.here.kind = lmstudio\n\
                    link.here.base_url = http://localhost:1234\n\
                    link.here.model = small\n\
                    role.coder = ds\n\
                    role.compactor = here\n```\n";
        let mixed = Links::parse(text).expect("parse");
        assert_eq!(mixed.local_only_gaps(), vec![Role::Coder]);
    }

    #[test]
    fn a_role_with_no_chain_is_an_error_not_a_default() {
        // `M-2`: there is no "just use whatever" path.
        let err = links().chain(Role::Planner).expect_err("must fail");
        assert!(format!("{err}").contains("every call needs one"), "{err}");
    }

    #[test]
    fn a_chain_naming_a_link_that_does_not_exist_fails_at_load() {
        let bad = CONFIG.replace("role.coder     = ds, rig", "role.coder     = ds, typo");
        let err = Links::parse(&bad).expect_err("must refuse");
        assert!(format!("{err}").contains("not a configured link"), "{err}");
    }

    #[test]
    fn a_deprecated_model_id_names_its_replacement() {
        // `M-14`, and the exact case that made it a requirement: this id died
        // on 2026-07-24, four days before the design was written.
        let links = links();
        let mut link = links.get("ds").expect("ds").clone();
        link.model = "deepseek-chat".to_string();

        let err = links
            .check_model(&link, &["deepseek-v4-flash".into(), "deepseek-v4-pro".into()])
            .expect_err("must refuse");
        let message = format!("{err}");
        assert!(message.contains("deprecated"), "{message}");
        assert!(message.contains("use `deepseek-v4-flash`"), "{message}");
        assert!(message.contains("Nothing is substituted"), "{message}");
    }

    #[test]
    fn an_unknown_model_id_lists_what_is_actually_there() {
        let links = links();
        let mut link = links.get("here").expect("here").clone();
        link.model = "a-model-that-left".to_string();

        let err = links
            .check_model(&link, &["qwen3-4b-instruct".into()])
            .expect_err("must refuse");
        assert!(format!("{err}").contains("Available: qwen3-4b-instruct"), "{err}");
    }

    #[test]
    fn a_configured_model_that_is_present_passes() {
        let links = links();
        let link = links.get("here").expect("here");
        links.check_model(link, &["qwen3-4b-instruct".into(), "other".into()]).expect("present");
    }

    #[test]
    fn secrets_are_referenced_by_variable_name_never_by_value() {
        // `S-2`. The config holds the name of the variable; nothing here can
        // hold a key, because there is no field for one.
        let links = links();
        assert_eq!(links.get("ds").expect("ds").auth_env.as_deref(), Some("DEEPSEEK_API_KEY"));
        let rendered = format!("{:?}", links.get("ds").expect("ds"));
        assert!(!rendered.contains("sk-"), "no key material anywhere: {rendered}");
    }

    #[test]
    fn only_lm_studio_kinds_offer_the_native_model_facts() {
        // `M-7`: quantization and loaded state come from `/api/v0`, which only
        // LM Studio serves.
        assert!(Kind::LmStudio.has_native_api());
        assert!(Kind::LmLink.has_native_api());
        assert!(!Kind::DeepSeek.has_native_api());
        assert!(!Kind::OpenAiCompat.has_native_api());
    }

    #[test]
    fn a_link_is_not_asked_more_than_its_concurrency_allows() {
        // `M-15`.
        let links = links();
        let link = links.get("rig").expect("rig");
        assert_eq!(link.concurrency, 1, "the default, and the right one for a single GPU");

        let permits = std::sync::Arc::new(Permits::new());
        let first = permits.acquire(link).expect("the first is allowed");
        assert!(permits.acquire(link).is_none(), "the second is not");
        assert_eq!(permits.in_flight(link), 1);

        drop(first);
        assert_eq!(permits.in_flight(link), 0, "and releasing frees the slot");
        assert!(permits.acquire(link).is_some());
    }

    #[test]
    fn one_links_limit_does_not_block_another() {
        let links = links();
        let permits = std::sync::Arc::new(Permits::new());
        let _rig = permits.acquire(links.get("rig").expect("rig")).expect("rig");
        assert!(
            permits.acquire(links.get("here").expect("here")).is_some(),
            "the limit is per link, not global"
        );
    }

    #[test]
    fn the_limit_holds_under_real_threads() {
        // The engine is single-threaded today. This is the thing that has to
        // already be right on the day it is not.
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;

        let text = "```perp-links\n\
                    link.busy.kind = lmstudio\n\
                    link.busy.base_url = http://localhost:1234\n\
                    link.busy.model = m\n\
                    link.busy.concurrency = 2\n\
                    role.coder = busy\n```\n";
        let links = Arc::new(Links::parse(text).expect("parse"));
        let permits = Arc::new(Permits::new());
        let granted = Arc::new(AtomicU32::new(0));
        let peak = Arc::new(AtomicU32::new(0));

        let handles: Vec<_> = (0..16)
            .map(|_| {
                let (links, permits, granted, peak) =
                    (links.clone(), permits.clone(), granted.clone(), peak.clone());
                std::thread::spawn(move || {
                    let link = links.get("busy").expect("busy");
                    if let Some(permit) = permits.acquire(link) {
                        let now = granted.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        granted.fetch_sub(1, Ordering::SeqCst);
                        drop(permit);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread");
        }

        assert!(peak.load(Ordering::SeqCst) <= 2, "the limit was exceeded: {peak:?}");
        assert_eq!(permits.in_flight(links.get("busy").expect("busy")), 0, "all released");
    }

    #[test]
    fn junk_in_the_configuration_is_rejected_rather_than_ignored() {
        for bad in [
            "```perp-links\nnonsense = value\n```\n",
            "```perp-links\nlink.x = missing-the-field\n```\n",
            "```perp-links\nlink.x.kind = telepathy\nlink.x.model = m\nlink.x.base_url = u\n```\n",
            "```perp-links\nlink.x.kind = lmstudio\nlink.x.base_url = u\n```\n",
            "```perp-links\nlink.x.kind = lmlink\nlink.x.model = m\n```\n",
        ] {
            assert!(Links::parse(bad).is_err(), "should have rejected: {bad}");
        }
    }
}
