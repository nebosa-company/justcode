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
}

impl Kind {
    pub fn parse(text: &str) -> Result<Kind> {
        match text {
            "lmstudio" => Ok(Kind::LmStudio),
            "lmlink" => Ok(Kind::LmLink),
            "deepseek" => Ok(Kind::DeepSeek),
            "openai-compat" => Ok(Kind::OpenAiCompat),
            other => Err(Error::unbound(
                "link kind",
                format!(
                    "`{other}` is not one of lmstudio, lmlink, deepseek, openai-compat"
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
        }
    }

    /// Does this kind serve LM Studio's native `/api/v0` surface, with its
    /// model facts — quantization, loaded state, real context length (`M-7`)?
    pub fn has_native_api(self) -> bool {
        matches!(self, Kind::LmStudio | Kind::LmLink)
    }

    /// The privacy class this kind can never be configured out of.
    fn implied_privacy(self) -> Option<Privacy> {
        match self {
            Kind::LmStudio | Kind::LmLink => Some(Privacy::Local),
            Kind::DeepSeek => Some(Privacy::Cloud),
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
        format!(
            "{} [{} · {} · {}] {}",
            self.name,
            self.kind.as_str(),
            self.privacy.as_str(),
            self.model,
            where_
        )
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

        let result = Links { links, chains, deprecated };
        result.check_chains()?;
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

        Err(Error::unbound(
            format!("role.{role}"),
            format!(
                "every eligible link is unhealthy: [{}]",
                eligible.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")
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
    if kind != Kind::LmLink && field("base_url").is_none() {
        return Err(Error::unbound(format!("link.{name}.base_url"), "is required"));
    }

    let concurrency = match field("concurrency") {
        Some(text) => text.trim().parse().map_err(|_| {
            Error::unbound(format!("link.{name}.concurrency"), format!("`{text}` is not a number"))
        })?,
        None => 1,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
