//! Talking to a link (`M-6`–`M-10`, `M-21`, `M-22`).
//!
//! The router picks a link; this is what happens next. Three things it does
//! that a thin HTTP wrapper would not:
//!
//! - **Failover is recorded, never silent** (`M-9`, `M-10`). A call that fell
//!   through to the second link in the chain says so, and the journal record
//!   names the link, the model and the quantization that produced the answer.
//!   "The 4B wrote this migration" has to be discoverable afterwards.
//! - **Reasoning is a separate channel** (`M-22`). It is kept out of the
//!   assistant message, and it is not replayed into the next request — the
//!   providers either reject that or charge for it.
//! - **Model facts come from the link** (`M-7`), and the configured id is
//!   checked against them before anything is sent (`M-14`).

use crate::error::{Error, Result};
use crate::json::{self, Value};
use crate::link::{Health, Link, Links, Mode, Role};
use crate::net::{Method, Request, Response, Secret, Transport};
use crate::probe::{Capabilities, ModelFacts, ProbeCache, ProbeKey};

/// Which wire protocol a link speaks (`M-21`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// `/v1/chat/completions` — every link speaks this.
    ChatCompletions,
    /// `/v1/responses` — LM Studio 0.3.29+ and OpenAI. Not implemented yet;
    /// the enum exists so the choice is explicit rather than assumed.
    Responses,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Message {
        Message { role: "user".into(), content: content.into() }
    }

    pub fn system(content: impl Into<String>) -> Message {
        Message { role: "system".into(), content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Message {
        Message { role: "assistant".into(), content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub max_tokens: Option<i64>,
    pub stream: bool,
}

impl ChatRequest {
    pub fn new(messages: Vec<Message>) -> ChatRequest {
        ChatRequest { messages, max_tokens: None, stream: false }
    }

    /// The JSON body. Stable field order, so a provider's prefix cache sees the
    /// same bytes for the same prefix (`M-12`).
    pub fn to_json(&self, model: &str) -> String {
        let messages: Vec<Value> = self
            .messages
            .iter()
            .map(|message| {
                Value::Obj(vec![
                    ("role".into(), Value::str(message.role.clone())),
                    ("content".into(), Value::str(message.content.clone())),
                ])
            })
            .collect();

        let mut fields = vec![
            ("model".to_string(), Value::str(model)),
            ("messages".to_string(), Value::Arr(messages)),
            ("stream".to_string(), Value::Bool(self.stream)),
        ];
        if let Some(max) = self.max_tokens {
            fields.push(("max_tokens".to_string(), Value::int(max)));
        }
        json::to_string(&Value::Obj(fields))
    }
}

/// What a call cost (`M-11`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// DeepSeek reports these separately, and the difference is ~50× in price.
    pub cache_hit_tokens: i64,
    pub cache_miss_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub content: String,
    /// Kept apart from `content` (`M-22`) — journalled, shown behind a fold,
    /// never concatenated, never replayed into the next request.
    pub reasoning: Option<String>,
    pub model: String,
    pub finish_reason: Option<String>,
    pub usage: Usage,
}

impl Reply {
    /// The message to carry into the next turn.
    ///
    /// Deliberately drops the reasoning: providers reject a replayed reasoning
    /// block or charge for it, and either way it is not what the model said.
    pub fn as_message(&self) -> Message {
        Message::assistant(self.content.clone())
    }
}

/// Parse a chat-completions response.
pub fn parse_chat(body: &str) -> Result<Reply> {
    let parsed = json::parse(body)?;
    if let Some(error) = parsed.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("no message");
        return Err(Error::unbound("chat", format!("the link returned an error: {message}")));
    }

    let choice = parsed
        .get("choices")
        .and_then(Value::as_arr)
        .and_then(|choices| choices.first())
        .ok_or_else(|| Error::unbound("chat", "the response has no choices"))?;
    let message = choice
        .get("message")
        .ok_or_else(|| Error::unbound("chat", "the choice has no message"))?;

    let usage = parsed.get("usage");
    let count = |key: &str| {
        usage
            .and_then(|usage| usage.get(key))
            .and_then(Value::as_i64)
            .unwrap_or_default()
    };

    Ok(Reply {
        content: message.get("content").and_then(Value::as_str).unwrap_or_default().to_string(),
        // `reasoning_content` is DeepSeek's name; `reasoning` is used elsewhere.
        reasoning: message
            .get("reasoning_content")
            .or_else(|| message.get("reasoning"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        model: parsed.get("model").and_then(Value::as_str).unwrap_or_default().to_string(),
        finish_reason: choice.get("finish_reason").and_then(Value::as_str).map(str::to_string),
        usage: Usage {
            prompt_tokens: count("prompt_tokens"),
            completion_tokens: count("completion_tokens"),
            cache_hit_tokens: count("prompt_cache_hit_tokens"),
            cache_miss_tokens: count("prompt_cache_miss_tokens"),
        },
    })
}

/// One link's turn at a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub link: String,
    pub outcome: String,
}

/// A call, and the trail it left (`M-10`).
#[derive(Debug, Clone)]
pub struct Served {
    pub reply: Reply,
    pub link: String,
    pub model: String,
    pub quantization: Option<String>,
    /// Links tried before this one. Empty when the first choice answered.
    pub fell_through: Vec<Attempt>,
}

impl Served {
    pub fn was_substituted(&self) -> bool {
        !self.fell_through.is_empty()
    }

    /// The provenance line that goes in the journal and the commit trailer.
    pub fn provenance(&self) -> String {
        let mut out = format!("link {} · model {}", self.link, self.model);
        if let Some(quantization) = &self.quantization {
            out.push_str(&format!(" · {quantization}"));
        }
        if self.was_substituted() {
            out.push_str(" · after ");
            out.push_str(
                &self
                    .fell_through
                    .iter()
                    .map(|attempt| format!("{} ({})", attempt.link, attempt.outcome))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        out
    }
}

/// Calls a link over a transport.
#[derive(Debug)]
pub struct Client<'a> {
    transport: &'a dyn Transport,
    cache: ProbeCache,
    /// The facts behind the cache key, cached alongside it.
    ///
    /// The probe key includes the quantization, which is only knowable by
    /// asking — so caching the capabilities without caching the facts means
    /// every lookup still costs a round trip, and the TTL protects nothing.
    facts: Vec<(String, ModelFacts, i64)>,
    ttl_secs: i64,
}

impl<'a> Client<'a> {
    pub fn new(transport: &'a dyn Transport) -> Client<'a> {
        Client {
            transport,
            cache: ProbeCache::new(3600),
            facts: Vec::new(),
            ttl_secs: 3600,
        }
    }

    fn base(link: &Link) -> Result<&str> {
        link.base_url.as_deref().ok_or_else(|| {
            Error::unbound(
                format!("link.{}", link.name),
                "has no base_url. An lmlink peer is reached through the local LM Studio server, \
                 which is a separate link — see the open question in the requirements.",
            )
        })
    }

    fn authorise(link: &Link, request: Request) -> Request {
        match &link.auth_env {
            Some(name) => request.bearer(Secret::from_env_var(name)),
            None => request,
        }
    }

    /// What models this link actually offers (`M-7`).
    ///
    /// LM Studio's `/api/v0/models` carries quantization, loaded state and the
    /// real context length; everything else gets the OpenAI-compatible list,
    /// which carries ids and nothing more.
    pub fn models(&self, link: &Link) -> Result<Vec<ModelFacts>> {
        let base = Self::base(link)?.trim_end_matches('/');
        let path = if link.kind.has_native_api() { "/api/v0/models" } else { "/v1/models" };
        let request = Self::authorise(link, Request::get(format!("{base}{path}")));

        let response = self.transport.send(&request)?;
        if !response.is_success() {
            return Err(Error::unbound(
                format!("link.{}", link.name),
                format!("listing models: {}", response.complaint()),
            ));
        }
        crate::probe::parse_models(&response.body)
    }

    /// Check the configured model against what the link offers (`M-14`).
    pub fn verify_model(&self, links: &Links, link: &Link) -> Result<ModelFacts> {
        let facts = self.models(link)?;
        let ids: Vec<String> = facts.iter().map(|f| f.id.clone()).collect();
        links.check_model(link, &ids)?;
        facts
            .into_iter()
            .find(|f| f.id == link.model)
            .ok_or_else(|| Error::unbound(format!("link.{}", link.name), "model vanished mid-check"))
    }

    /// The link's facts, re-fetched only when the cached ones go stale.
    fn cached_facts(&mut self, links: &Links, link: &Link, now: i64) -> Result<ModelFacts> {
        let fresh = now - self.ttl_secs;
        self.facts.retain(|(_, _, at)| *at > fresh);
        if let Some((_, facts, _)) = self.facts.iter().find(|(name, _, _)| name == &link.name) {
            return Ok(facts.clone());
        }
        let facts = self.verify_model(links, link)?;
        self.facts.push((link.name.clone(), facts.clone(), now));
        Ok(facts)
    }

    /// Probe capabilities, cached against link + model + quantization (`M-6`).
    pub fn capabilities(&mut self, links: &Links, link: &Link, now: i64) -> Result<Capabilities> {
        let facts = self.cached_facts(links, link, now)?;
        let key = ProbeKey::of(link, &facts);
        if let Some(cached) = self.cache.get(&key, now) {
            return Ok(cached);
        }
        let capabilities = Capabilities::observed(link.kind, &facts);
        self.cache.put(key, capabilities.clone(), now);
        Ok(capabilities)
    }

    pub fn protocol(link: &Link) -> Protocol {
        // Every link speaks chat-completions. `/v1/responses` is opt-in and not
        // implemented yet — see `M-21`.
        let _ = link;
        Protocol::ChatCompletions
    }

    /// One call to one link.
    pub fn chat(&self, link: &Link, request: &ChatRequest) -> Result<Reply> {
        match Self::protocol(link) {
            Protocol::Responses => Err(Error::unbound(
                format!("link.{}", link.name),
                "the /v1/responses protocol is not implemented yet (`M-21`)",
            )),
            Protocol::ChatCompletions => {
                let base = Self::base(link)?.trim_end_matches('/');
                let body = request.to_json(&link.model);
                let http = Self::authorise(
                    link,
                    Request::post_json(format!("{base}/v1/chat/completions"), body),
                );
                let response = self.transport.send(&http)?;
                Self::interpret(link, &response)
            }
        }
    }

    fn interpret(link: &Link, response: &Response) -> Result<Reply> {
        if !response.is_success() {
            return Err(Error::unbound(
                format!("link.{}", link.name),
                format!("chat: {}", response.complaint()),
            ));
        }
        parse_chat(&response.body)
    }

    /// Resolve a role and call it, falling through the chain on failure
    /// (`M-9`), recording every link that was tried (`M-10`).
    ///
    /// A fall-through never crosses the privacy boundary: the router has
    /// already removed ineligible links, so a `local-only` run cannot reach a
    /// cloud link by failing enough times.
    pub fn call(
        &mut self,
        links: &Links,
        role: Role,
        request: &ChatRequest,
        health: &dyn Health,
        mode: Mode,
        now: i64,
    ) -> Result<Served> {
        let chain = links.chain(role)?;
        let eligible: Vec<&&Link> = chain
            .iter()
            .filter(|link| mode == Mode::Any || link.is_local())
            .filter(|link| health.is_healthy(link))
            .collect();

        if eligible.is_empty() {
            // Reuse the router's message, which explains the privacy rule.
            links.resolve(role, health, mode)?;
        }

        let candidates: Vec<Link> = eligible.iter().map(|link| (**link).clone()).collect();
        let mut fell_through = Vec::new();

        for link in &candidates {
            // `M-14`, enforced rather than merely available: a link whose
            // configured model the server does not offer is not asked. Sending
            // anyway earns a 400 that blames the request instead of the config,
            // and costs a round trip to learn less.
            let facts = match self.cached_facts(links, link, now) {
                Ok(facts) => facts,
                Err(error) => {
                    fell_through
                        .push(Attempt { link: link.name.clone(), outcome: error.to_string() });
                    continue;
                }
            };

            match self.chat(link, request) {
                Ok(reply) => {
                    return Ok(Served {
                        model: if reply.model.is_empty() {
                            link.model.clone()
                        } else {
                            reply.model.clone()
                        },
                        reply,
                        link: link.name.clone(),
                        quantization: facts.quantization.clone(),
                        fell_through,
                    })
                }
                Err(error) => fell_through.push(Attempt {
                    link: link.name.clone(),
                    outcome: error.to_string(),
                }),
            }
        }

        Err(Error::unbound(
            format!("role.{role}"),
            format!(
                "every link failed: {}",
                fell_through
                    .iter()
                    .map(|attempt| format!("{} — {}", attempt.link, attempt.outcome))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        ))
    }
}

/// The HTTP method a link's model listing uses. Exposed for the CLI's
/// explanation of what it is about to do.
pub fn models_method() -> Method {
    Method::Get
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::AssumeHealthy;
    use crate::net::Response;
    use std::cell::RefCell;

    /// A transport with recorded answers, so the client is tested without a
    /// server, a GPU or a key.
    #[derive(Debug)]
    struct Canned {
        answers: RefCell<Vec<Result<Response>>>,
        seen: RefCell<Vec<String>>,
    }

    impl Canned {
        fn new(answers: Vec<Result<Response>>) -> Canned {
            Canned { answers: RefCell::new(answers), seen: RefCell::new(Vec::new()) }
        }

        fn ok(body: &str) -> Result<Response> {
            Ok(Response { status: 200, body: body.to_string() })
        }

        fn status(code: u16, body: &str) -> Result<Response> {
            Ok(Response { status: code, body: body.to_string() })
        }

        fn dead() -> Result<Response> {
            Err(Error::unbound("curl", "exit 7 — could not connect"))
        }
    }

    impl Transport for Canned {
        fn send(&self, request: &Request) -> Result<Response> {
            self.seen.borrow_mut().push(request.url.clone());
            let mut answers = self.answers.borrow_mut();
            if answers.is_empty() {
                return Err(Error::unbound("test", "no answer left"));
            }
            answers.remove(0)
        }
    }

    const MODELS: &str = r#"{"object":"list","data":[
      {"id":"small","type":"llm","quantization":"Q4_K_M","state":"loaded","max_context_length":32768}
    ]}"#;

    const CLOUD_MODELS: &str = r#"{"data":[{"id":"deepseek-v4-flash"}]}"#;

    const CHAT: &str = r#"{
      "id":"chatcmpl-1","model":"small",
      "choices":[{"index":0,"message":{"role":"assistant","content":"the answer"},"finish_reason":"stop"}],
      "usage":{"prompt_tokens":12,"completion_tokens":3}
    }"#;

    const THINKING: &str = r#"{
      "id":"chatcmpl-2","model":"deepseek-v4-flash",
      "choices":[{"index":0,"message":{
        "role":"assistant",
        "reasoning_content":"first I considered the obvious thing",
        "content":"the answer"},"finish_reason":"stop"}],
      "usage":{"prompt_tokens":100,"completion_tokens":5,
               "prompt_cache_hit_tokens":90,"prompt_cache_miss_tokens":10}
    }"#;

    fn links() -> Links {
        Links::parse(
            "```perp-links\n\
             link.here.kind = lmstudio\n\
             link.here.base_url = http://localhost:1234\n\
             link.here.model = small\n\
             link.cloud.kind = deepseek\n\
             link.cloud.base_url = https://api.deepseek.com\n\
             link.cloud.model = deepseek-v4-flash\n\
             link.cloud.auth_env = PERP_FAKE_KEY\n\
             role.coder = cloud, here\n\
             role.compactor = here\n```\n",
        )
        .expect("parse")
    }

    #[test]
    fn model_facts_come_from_the_native_api_for_lm_studio() {
        // `M-7`: quantization and loaded state exist only on `/api/v0`.
        let transport = Canned::new(vec![Canned::ok(MODELS)]);
        let client = Client::new(&transport);
        let links = links();

        let facts = client.models(links.get("here").expect("here")).expect("models");
        assert_eq!(facts[0].quantization.as_deref(), Some("Q4_K_M"));
        assert!(transport.seen.borrow()[0].ends_with("/api/v0/models"), "{:?}", transport.seen);
    }

    #[test]
    fn a_cloud_link_uses_the_openai_compatible_listing() {
        let transport = Canned::new(vec![Canned::ok(r#"{"data":[{"id":"deepseek-v4-flash"}]}"#)]);
        let client = Client::new(&transport);
        let links = links();

        client.models(links.get("cloud").expect("cloud")).expect("models");
        assert!(transport.seen.borrow()[0].ends_with("/v1/models"), "{:?}", transport.seen);
    }

    #[test]
    fn a_configured_model_the_link_does_not_offer_is_caught_before_anything_is_sent() {
        // `M-14`, now against a live listing rather than a hand-written one.
        let transport = Canned::new(vec![Canned::ok(r#"{"data":[{"id":"something-else"}]}"#)]);
        let client = Client::new(&transport);
        let links = links();

        let err = client
            .verify_model(&links, links.get("here").expect("here"))
            .expect_err("must refuse");
        assert!(format!("{err}").contains("Available: something-else"), "{err}");
    }

    #[test]
    fn capabilities_are_probed_once_and_then_cached() {
        // `M-6`: the second call must not hit the transport again.
        let transport = Canned::new(vec![Canned::ok(MODELS)]);
        let mut client = Client::new(&transport);
        let links = links();
        let link = links.get("here").expect("here");

        let first = client.capabilities(&links, link, 1000).expect("probe");
        assert_eq!(first.context_length, Some(32768));
        let second = client.capabilities(&links, link, 1200).expect("cached");
        assert_eq!(first, second);
        assert_eq!(transport.seen.borrow().len(), 1, "the second call was served from the cache");
    }

    #[test]
    fn a_stale_probe_asks_the_link_again() {
        // The other half of `M-6`: cached until the TTL, then re-checked. A
        // model swapped in LM Studio must not be believed to be the old one
        // forever.
        let transport = Canned::new(vec![Canned::ok(MODELS), Canned::ok(MODELS)]);
        let mut client = Client::new(&transport);
        let links = links();
        let link = links.get("here").expect("here");

        client.capabilities(&links, link, 1000).expect("probe");
        client.capabilities(&links, link, 1000 + 3601).expect("re-probe");
        assert_eq!(transport.seen.borrow().len(), 2, "past the ttl, it asks again");
    }

    #[test]
    fn a_reply_is_parsed_with_its_usage() {
        let reply = parse_chat(CHAT).expect("parse");
        assert_eq!(reply.content, "the answer");
        assert_eq!(reply.model, "small");
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(reply.usage.prompt_tokens, 12);
        assert_eq!(reply.reasoning, None);
    }

    #[test]
    fn reasoning_is_a_separate_channel_and_is_not_carried_forward() {
        // `M-22`. Concatenating it into the message is the bug this prevents.
        let reply = parse_chat(THINKING).expect("parse");
        assert_eq!(reply.content, "the answer");
        assert_eq!(reply.reasoning.as_deref(), Some("first I considered the obvious thing"));
        assert!(!reply.content.contains("first I considered"), "never concatenated");

        let carried = reply.as_message();
        assert_eq!(carried.content, "the answer");
        assert!(!carried.content.contains("considered"), "and not replayed into the next request");
    }

    #[test]
    fn cache_hit_and_miss_tokens_are_counted_separately() {
        // `M-11`: the difference between them is roughly fifty-fold in price.
        let reply = parse_chat(THINKING).expect("parse");
        assert_eq!(reply.usage.cache_hit_tokens, 90);
        assert_eq!(reply.usage.cache_miss_tokens, 10);
    }

    #[test]
    fn an_error_body_is_reported_as_an_error_not_as_an_empty_reply() {
        let err = parse_chat(r#"{"error":{"message":"model not loaded"}}"#).expect_err("must fail");
        assert!(format!("{err}").contains("model not loaded"), "{err}");
        assert!(parse_chat(r#"{"choices":[]}"#).is_err(), "no choices is not an empty answer");
    }

    #[test]
    fn a_failed_link_falls_through_and_the_fall_through_is_recorded() {
        // `M-9` and `M-10` together: the substitution happens, and it is not
        // silent.
        let transport = Canned::new(vec![Canned::dead(), Canned::ok(MODELS), Canned::ok(CHAT)]);
        let mut client = Client::new(&transport);
        let links = links();

        let served = client
            .call(
                &links,
                Role::Coder,
                &ChatRequest::new(vec![Message::user("hello")]),
                &AssumeHealthy,
                Mode::Any,
                1000,
            )
            .expect("served");

        assert_eq!(served.link, "here", "the second link answered");
        assert!(served.was_substituted());
        assert_eq!(served.fell_through.len(), 1);
        assert_eq!(served.fell_through[0].link, "cloud");

        let provenance = served.provenance();
        assert!(provenance.contains("link here"), "{provenance}");
        assert!(provenance.contains("after cloud"), "{provenance}");
        assert!(provenance.contains("could not connect"), "the reason survives: {provenance}");
    }

    #[test]
    fn an_http_error_from_the_first_link_also_falls_through() {
        let transport = Canned::new(vec![Canned::ok(CLOUD_MODELS), Canned::status(401, r#"{"error":"bad key"}"#), Canned::ok(MODELS), Canned::ok(CHAT)]);
        let mut client = Client::new(&transport);
        let served = client
            .call(
                &links(),
                Role::Coder,
                &ChatRequest::new(vec![Message::user("hi")]),
                &AssumeHealthy,
                Mode::Any,
                1000,
            )
            .expect("served");
        assert_eq!(served.link, "here");
        assert!(served.fell_through[0].outcome.contains("HTTP 401"), "{:?}", served.fell_through);
    }

    #[test]
    fn local_only_does_not_reach_a_cloud_link_by_failing_enough_times() {
        // The failover path must not become a way around `M-4`.
        let transport = Canned::new(vec![Canned::ok(MODELS), Canned::ok(CHAT)]);
        let mut client = Client::new(&transport);
        let served = client
            .call(
                &links(),
                Role::Coder,
                &ChatRequest::new(vec![Message::user("hi")]),
                &AssumeHealthy,
                Mode::LocalOnly,
                1000,
            )
            .expect("served");
        assert_eq!(served.link, "here", "the cloud link was never a candidate");
        assert!(!served.was_substituted(), "and so this is not a fall-through");
    }

    #[test]
    fn when_everything_fails_the_error_names_every_attempt() {
        let transport = Canned::new(vec![Canned::dead(), Canned::dead()]);
        let mut client = Client::new(&transport);
        let err = client
            .call(
                &links(),
                Role::Coder,
                &ChatRequest::new(vec![Message::user("hi")]),
                &AssumeHealthy,
                Mode::Any,
                1000,
            )
            .expect_err("must fail");
        let text = format!("{err}");
        assert!(text.contains("cloud"), "{text}");
        assert!(text.contains("here"), "{text}");
    }

    #[test]
    fn the_request_body_keeps_a_stable_field_order() {
        // `M-12`: a prefix cache only pays when the same prefix is byte-identical.
        let request = ChatRequest::new(vec![Message::system("rules"), Message::user("do it")]);
        let body = request.to_json("small");
        assert_eq!(body, request.to_json("small"), "same input, same bytes");
        assert!(body.starts_with(r#"{"model":"small","messages":["#), "{body}");
        assert!(body.contains(r#"{"role":"system","content":"rules"}"#), "{body}");
    }
}
