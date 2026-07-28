//! Capability probing (`M-6`, `M-7`).
//!
//! Local models vary wildly. One will emit clean tool calls; the next, same
//! family, one quantization down, will not. So the harness *discovers* what a
//! link can do rather than assuming an OpenAI feature set — and caches the
//! answer against the link, the model **and the quantization**, because a Q4
//! and a Q8 of the same model are not the same reviewer.
//!
//! `M-7`: for LM Studio and LM Link the facts come from `/api/v0/models`, which
//! reports loaded state, real context length, architecture and quantization.
//! Guessing any of those is how a batch dies three hours in on a context
//! overflow nobody predicted.

use crate::error::{Error, Result};
use crate::json::{self, Value};
use crate::link::{Kind, Link};

/// Whether a model is in memory right now. Loading a 30B is minutes, which is
/// scheduling information, not trivia (`M-16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Loaded,
    NotLoaded,
    Unknown,
}

impl LoadState {
    fn parse(text: &str) -> LoadState {
        match text {
            "loaded" => LoadState::Loaded,
            "not-loaded" => LoadState::NotLoaded,
            _ => LoadState::Unknown,
        }
    }
}

/// What a link says about one model (`M-7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFacts {
    pub id: String,
    /// `llm`, `vlm` or `embeddings`.
    pub kind: String,
    pub publisher: Option<String>,
    pub arch: Option<String>,
    /// `Q4_K_M`, `4bit`, … Recorded in the journal with everything the model
    /// produced, so "the Q4 wrote this migration" is discoverable later.
    pub quantization: Option<String>,
    pub state: LoadState,
    pub max_context_length: Option<i64>,
}

impl ModelFacts {
    pub fn is_embedding_model(&self) -> bool {
        self.kind == "embeddings"
    }

    pub fn sees_images(&self) -> bool {
        self.kind == "vlm"
    }
}

/// Parse an `/api/v0/models` response.
///
/// Tolerant on purpose: an unknown field is ignored, a missing optional field
/// is `None`. A schema this version has never seen is not a reason to refuse to
/// run — but a response that is not a model list at all is.
pub fn parse_models(body: &str) -> Result<Vec<ModelFacts>> {
    let parsed = json::parse(body)?;
    let data = parsed
        .get("data")
        .and_then(Value::as_arr)
        .ok_or_else(|| Error::unbound("models", "the response has no `data` array"))?;

    let mut facts = Vec::new();
    for entry in data {
        let Some(id) = entry.get("id").and_then(Value::as_str) else {
            return Err(Error::unbound("models", "an entry has no `id`"));
        };
        facts.push(ModelFacts {
            id: id.to_string(),
            kind: entry.get("type").and_then(Value::as_str).unwrap_or("llm").to_string(),
            publisher: entry.get("publisher").and_then(Value::as_str).map(str::to_string),
            arch: entry.get("arch").and_then(Value::as_str).map(str::to_string),
            quantization: entry.get("quantization").and_then(Value::as_str).map(str::to_string),
            state: entry
                .get("state")
                .and_then(Value::as_str)
                .map_or(LoadState::Unknown, LoadState::parse),
            max_context_length: entry.get("max_context_length").and_then(Value::as_i64),
        });
    }
    Ok(facts)
}

/// What a link turned out to be able to do (`M-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub native_tool_calls: bool,
    pub json_schema: bool,
    pub streaming: bool,
    pub vision: bool,
    pub embeddings: bool,
    pub context_length: Option<i64>,
    /// A separate reasoning channel, kept out of the assistant message (`M-22`).
    pub reasoning_channel: bool,
    /// A prefix cache worth shaping prompts for (`M-12`).
    pub prefix_cache: bool,
    /// How this was decided, so a wrong answer is traceable to its source
    /// rather than to a vibe.
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Read off a real response from the link.
    Observed,
    /// Derived from the link kind because nothing better was available. Honest
    /// about being a default rather than a measurement.
    KindDefault,
}

impl Capabilities {
    /// The starting point for a kind, before anything has been observed.
    ///
    /// These are *defaults, not claims* — `source` says so — and the degradation
    /// ladder in `M-8` means being wrong here costs a slower path, not a crash.
    pub fn expected_for(kind: Kind) -> Capabilities {
        let cloud = matches!(kind, Kind::DeepSeek);
        Capabilities {
            native_tool_calls: cloud,
            json_schema: cloud,
            streaming: true,
            vision: false,
            embeddings: matches!(kind, Kind::LmStudio | Kind::LmLink),
            context_length: None,
            reasoning_channel: cloud,
            prefix_cache: cloud,
            source: Source::KindDefault,
        }
    }

    /// Refine with what the link actually reported about the model (`M-7`).
    pub fn observed(kind: Kind, facts: &ModelFacts) -> Capabilities {
        let mut caps = Capabilities::expected_for(kind);
        caps.context_length = facts.max_context_length;
        caps.vision = facts.sees_images();
        caps.embeddings = facts.is_embedding_model() || caps.embeddings;
        caps.source = Source::Observed;
        caps
    }

    /// Would a prompt of this size fit?
    pub fn fits(&self, tokens: i64) -> bool {
        self.context_length.is_none_or(|limit| tokens <= limit)
    }
}

/// What a cached probe is keyed on (`M-6`).
///
/// The quantization is part of the key deliberately: swapping a Q8 for a Q4 in
/// LM Studio changes what the link can do while every other identifier stays
/// the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeKey {
    pub link: String,
    pub model: String,
    pub quantization: Option<String>,
}

impl ProbeKey {
    pub fn of(link: &Link, facts: &ModelFacts) -> ProbeKey {
        ProbeKey {
            link: link.name.clone(),
            model: facts.id.clone(),
            quantization: facts.quantization.clone(),
        }
    }
}

/// Probe results with a time to live.
#[derive(Debug, Clone)]
pub struct ProbeCache {
    ttl_secs: i64,
    entries: Vec<(ProbeKey, Capabilities, i64)>,
}

impl ProbeCache {
    pub fn new(ttl_secs: i64) -> ProbeCache {
        ProbeCache { ttl_secs, entries: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn put(&mut self, key: ProbeKey, capabilities: Capabilities, now: i64) {
        self.entries.retain(|(existing, _, _)| existing != &key);
        self.entries.push((key, capabilities, now));
    }

    /// `None` when absent or stale. A stale entry is dropped rather than
    /// returned with a warning, because a warning nobody reads is a lie with
    /// extra steps.
    pub fn get(&mut self, key: &ProbeKey, now: i64) -> Option<Capabilities> {
        let fresh = now - self.ttl_secs;
        self.entries.retain(|(_, _, at)| *at > fresh);
        self.entries
            .iter()
            .find(|(existing, _, _)| existing == key)
            .map(|(_, capabilities, _)| capabilities.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Links, Role};

    /// A recorded `/api/v0/models` response, in the shape LM Studio documents:
    /// id, type, publisher, arch, compatibility_type, quantization, state,
    /// max_context_length. Recorded rather than live, so the suite runs on a
    /// machine with no LM Studio and no GPU.
    const MODELS: &str = r#"{
      "object": "list",
      "data": [
        {
          "id": "qwen3-coder-30b",
          "object": "model",
          "type": "llm",
          "publisher": "qwen",
          "arch": "qwen3",
          "compatibility_type": "gguf",
          "quantization": "Q4_K_M",
          "state": "loaded",
          "max_context_length": 262144
        },
        {
          "id": "qwen3-4b-instruct",
          "object": "model",
          "type": "llm",
          "publisher": "qwen",
          "arch": "qwen3",
          "compatibility_type": "gguf",
          "quantization": "Q8_0",
          "state": "not-loaded",
          "max_context_length": 32768
        },
        {
          "id": "text-embedding-nomic",
          "object": "model",
          "type": "embeddings",
          "publisher": "nomic",
          "arch": "nomic-bert",
          "quantization": "F16",
          "state": "not-loaded",
          "max_context_length": 2048,
          "an_unknown_future_field": {"nested": true}
        }
      ]
    }"#;

    fn links() -> Links {
        Links::parse(
            "```perp-links\n\
             link.rig.kind = lmlink\n\
             link.rig.device = workshop-4090\n\
             link.rig.model = qwen3-coder-30b\n\
             role.coder = rig\n```\n",
        )
        .expect("parse")
    }

    #[test]
    fn reads_the_facts_lm_studio_actually_reports() {
        let facts = parse_models(MODELS).expect("parse");
        assert_eq!(facts.len(), 3);

        let big = &facts[0];
        assert_eq!(big.id, "qwen3-coder-30b");
        assert_eq!(big.quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(big.state, LoadState::Loaded);
        assert_eq!(big.max_context_length, Some(262_144));
        assert_eq!(big.arch.as_deref(), Some("qwen3"));
    }

    #[test]
    fn a_field_from_a_later_version_does_not_break_the_parse() {
        // `N-8` again, at the other end of the wire.
        let facts = parse_models(MODELS).expect("parse");
        assert!(facts[2].is_embedding_model());
        assert_eq!(facts[2].max_context_length, Some(2048));
    }

    #[test]
    fn a_response_that_is_not_a_model_list_is_refused() {
        assert!(parse_models(r#"{"error":"unauthorized"}"#).is_err());
        assert!(parse_models(r#"{"data":[{"no_id":true}]}"#).is_err());
        assert!(parse_models("not json").is_err());
    }

    #[test]
    fn an_empty_list_is_an_answer_not_an_error() {
        // A server with no models loaded is a real state, and `M-14`'s check
        // reports it usefully.
        let facts = parse_models(r#"{"object":"list","data":[]}"#).expect("parse");
        assert!(facts.is_empty());
    }

    #[test]
    fn capabilities_start_as_defaults_and_say_so() {
        let caps = Capabilities::expected_for(Kind::LmStudio);
        assert_eq!(caps.source, Source::KindDefault);
        assert!(!caps.native_tool_calls, "a local model is not assumed to have them");
        assert!(caps.embeddings, "LM Studio serves /v1/embeddings");

        let cloud = Capabilities::expected_for(Kind::DeepSeek);
        assert!(cloud.native_tool_calls && cloud.json_schema && cloud.prefix_cache);
    }

    #[test]
    fn observing_a_model_replaces_the_guess_with_the_report() {
        let facts = parse_models(MODELS).expect("parse");
        let caps = Capabilities::observed(Kind::LmLink, &facts[0]);
        assert_eq!(caps.source, Source::Observed);
        assert_eq!(caps.context_length, Some(262_144));
        assert!(caps.fits(200_000));
        assert!(!caps.fits(300_000), "the limit is the reported one, not a guess");
    }

    #[test]
    fn an_unknown_context_length_does_not_pretend_to_be_a_limit() {
        let caps = Capabilities::expected_for(Kind::OpenAiCompat);
        assert_eq!(caps.context_length, None);
        assert!(caps.fits(10_000_000), "unknown is not zero");
    }

    #[test]
    fn the_cache_is_keyed_on_the_quantization_too() {
        // `M-6`: a Q4 and a Q8 of the same model are not the same reviewer.
        let links = links();
        let link = links.resolve(Role::Coder, &crate::link::AssumeHealthy, Default::default())
            .expect("resolve");
        let facts = parse_models(MODELS).expect("parse");

        let mut cache = ProbeCache::new(3600);
        let q4 = ProbeKey::of(link, &facts[0]);
        let mut q8_facts = facts[0].clone();
        q8_facts.quantization = Some("Q8_0".into());
        let q8 = ProbeKey::of(link, &q8_facts);
        assert_ne!(q4, q8);

        cache.put(q4.clone(), Capabilities::observed(Kind::LmLink, &facts[0]), 1000);
        assert!(cache.get(&q4, 1000).is_some());
        assert!(cache.get(&q8, 1000).is_none(), "the other quantization is a different entry");
    }

    #[test]
    fn a_stale_entry_is_dropped_rather_than_returned() {
        let links = links();
        let link = links.get("rig").expect("rig");
        let facts = parse_models(MODELS).expect("parse");
        let key = ProbeKey::of(link, &facts[0]);

        let mut cache = ProbeCache::new(60);
        cache.put(key.clone(), Capabilities::expected_for(Kind::LmLink), 1000);
        assert!(cache.get(&key, 1030).is_some(), "still fresh");
        assert!(cache.get(&key, 1100).is_none(), "past its ttl");
        assert!(cache.is_empty(), "and gone, not lingering");
    }

    #[test]
    fn re_probing_replaces_rather_than_accumulates() {
        let links = links();
        let link = links.get("rig").expect("rig");
        let facts = parse_models(MODELS).expect("parse");
        let key = ProbeKey::of(link, &facts[0]);

        let mut cache = ProbeCache::new(3600);
        cache.put(key.clone(), Capabilities::expected_for(Kind::LmLink), 100);
        cache.put(key.clone(), Capabilities::observed(Kind::LmLink, &facts[0]), 200);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&key, 200).expect("hit").source, Source::Observed);
    }

    #[test]
    fn the_model_check_uses_what_the_link_actually_offers() {
        // `M-14` end to end: the ids come from the parsed response, not a list
        // in the source.
        let links = links();
        let facts = parse_models(MODELS).expect("parse");
        let available: Vec<String> = facts.iter().map(|f| f.id.clone()).collect();

        links.check_model(links.get("rig").expect("rig"), &available).expect("configured model is there");

        let mut missing = links.get("rig").expect("rig").clone();
        missing.model = "qwen3-coder-70b".into();
        assert!(links.check_model(&missing, &available).is_err());
    }
}
