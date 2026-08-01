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

/// The Messages API version Anthropic is asked for.
///
/// Dated, and sent on every request: the API refuses one without it rather than
/// choosing for you, which is the right call — a silently-changing wire format is
/// worse than an error.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

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
    /// The tools to offer, already in the provider's shape. `None` for a plain
    /// conversation — and for the rungs below native, which parse tool calls
    /// out of the message text instead.
    pub tools: Option<Value>,
}

impl ChatRequest {
    /// Offer the harness's tools on the wire (`M-8`, native rung).
    pub fn with_tools(mut self, tools: Value) -> ChatRequest {
        self.tools = Some(tools);
        self
    }

    pub fn new(messages: Vec<Message>) -> ChatRequest {
        ChatRequest { messages, max_tokens: None, stream: false, tools: None }
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
        // `M-8`'s top rung is only real if the tools are actually sent. Asking a
        // model to "use the tools you have been given" without giving it any is
        // how a run comes back with `[grep output from expected tool call]` in
        // it — a fabricated answer that looks like work.
        if let Some(tools) = &self.tools {
            fields.push(("tools".to_string(), tools.clone()));
            fields.push(("tool_choice".to_string(), Value::str("auto")));
        }
        json::to_string(&Value::Obj(fields))
    }

    /// The `/v1/responses` shape (`M-21`).
    ///
    /// Converted **at the link edge**: the engine's own message model is the
    /// same either way, and only this function knows the difference. A
    /// protocol that leaked into the engine would mean every caller had to
    /// know which link it was talking to.
    ///
    /// The differences that matter: `input` rather than `messages`, `content`
    /// as a typed array rather than a bare string, and
    /// `max_output_tokens` rather than `max_tokens`.
    pub fn to_responses_json(&self, model: &str) -> String {
        let input: Vec<Value> = self
            .messages
            .iter()
            .map(|message| {
                let part = Value::Obj(vec![
                    // `input_text` for what we send, `output_text` for what
                    // comes back. Sending the wrong one is a 400 that says
                    // very little.
                    ("type".into(), Value::str("input_text")),
                    ("text".into(), Value::str(message.content.clone())),
                ]);
                Value::Obj(vec![
                    ("role".into(), Value::str(message.role.clone())),
                    ("content".into(), Value::Arr(vec![part])),
                ])
            })
            .collect();

        let mut fields = vec![
            ("model".to_string(), Value::str(model)),
            ("input".to_string(), Value::Arr(input)),
            ("stream".to_string(), Value::Bool(self.stream)),
        ];
        if let Some(max) = self.max_tokens {
            fields.push(("max_output_tokens".to_string(), Value::int(max)));
        }
        json::to_string(&Value::Obj(fields))
    }
}

/// Parse a `/v1/responses` reply (`M-21`).
///
/// The content is nested two levels deeper than chat-completions, and the token
/// counts are named differently. Both are handled here and nowhere else.
pub fn parse_responses(body: &str) -> Result<Reply> {
    let parsed = json::parse(body)?;
    if let Some(error) = parsed.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the provider returned an error with no message");
        return Err(Error::unbound("responses", message.to_string()));
    }

    let mut content = String::new();
    let mut reasoning = String::new();
    if let Some(output) = parsed.get("output").and_then(Value::as_arr) {
        for item in output {
            let kind = item.get("type").and_then(Value::as_str).unwrap_or("message");
            let Some(parts) = item.get("content").and_then(Value::as_arr) else { continue };
            for part in parts {
                let Some(text) = part.get("text").and_then(Value::as_str) else { continue };
                // `M-22`: reasoning stays out of the message.
                if kind == "reasoning" {
                    reasoning.push_str(text);
                } else {
                    content.push_str(text);
                }
            }
        }
    }

    let usage = parsed.get("usage");
    let count = |name: &str| {
        usage.and_then(|u| u.get(name)).and_then(Value::as_i64).unwrap_or_default()
    };
    let prompt = count("input_tokens");
    let completion = count("output_tokens");

    Ok(Reply {
        content,
        reasoning: (!reasoning.is_empty()).then_some(reasoning),
        model: parsed.get("model").and_then(Value::as_str).unwrap_or_default().to_string(),
        finish_reason: parsed.get("status").and_then(Value::as_str).map(str::to_string),
        usage: Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            cache_hit_tokens: usage
                .and_then(|u| u.get("input_tokens_details"))
                .and_then(|d| d.get("cached_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or_default(),
            cache_miss_tokens: 0,
        },
    })
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
    /// The response exactly as it arrived.
    ///
    /// Native tool calls live in a part of the response the `Reply` does not
    /// model, so a caller that wants the top rung of `M-8` needs the bytes. Not
    /// journalled — the `Reply` is what goes on the record.
    pub raw: String,
    pub link: String,
    pub model: String,
    pub quantization: Option<String>,
    pub role: String,
    pub latency_ms: i64,
    /// Links tried before this one. Empty when the first choice answered.
    pub fell_through: Vec<Attempt>,
}

impl Served {
    pub fn was_substituted(&self) -> bool {
        !self.fell_through.is_empty()
    }

    /// The accounting for this call (`M-11`).
    pub fn entry(&self, step: &crate::step::StepId, price: crate::cost::Price) -> crate::cost::Entry {
        let usage = crate::cost::Usage::from_reply(
            self.reply.usage.prompt_tokens,
            self.reply.usage.completion_tokens,
            self.reply.usage.cache_hit_tokens,
            self.reply.usage.cache_miss_tokens,
        );
        crate::cost::Entry {
            step: step.to_string(),
            role: self.role.clone(),
            link: self.link.clone(),
            model: self.model.clone(),
            charge: price.charge(&usage),
            usage,
            latency_ms: self.latency_ms,
        }
    }

    /// The journal record, carrying both the provenance and the bill.
    pub fn to_record(
        &self,
        step: crate::step::StepId,
        at: i64,
        price: crate::cost::Price,
    ) -> crate::journal::Record {
        let entry = self.entry(&step, price);
        let record = crate::journal::Record::outcome(
            step,
            at,
            true,
            format!("{} — {}", self.role, self.provenance()),
        )
        .with_detail(self.provenance());
        crate::cost::annotate(record, &entry)
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
    permits: std::sync::Arc<crate::link::Permits>,
    /// Applied to every outbound body on its way to a `cloud` link (`S-3`).
    redact: Vec<crate::security::Pattern>,
    /// Which hosts may be reached at all (`S-4`). Empty means "the links the
    /// operator configured, and nothing else" — set by [`Client::with_egress`].
    egress: Option<crate::security::Egress>,
}

impl<'a> Client<'a> {
    pub fn new(transport: &'a dyn Transport) -> Client<'a> {
        Client {
            transport,
            cache: ProbeCache::new(3600),
            facts: Vec::new(),
            ttl_secs: 3600,
            permits: std::sync::Arc::new(crate::link::Permits::new()),
            redact: Vec::new(),
            egress: None,
        }
    }

    /// Patterns to strip from anything bound for a cloud link (`S-3`).
    pub fn with_redaction(mut self, patterns: Vec<crate::security::Pattern>) -> Client<'a> {
        self.redact = patterns;
        self
    }

    /// Refuse any host not on the list (`S-4`).
    pub fn with_egress(mut self, egress: crate::security::Egress) -> Client<'a> {
        self.egress = Some(egress);
        self
    }

    /// Every refusal this client made, for the journal. Egress refusals are
    /// recorded rather than returned quietly: a loop that silently declines to
    /// reach a host looks exactly like one that reached it and got nothing.
    pub fn check_egress(&self, url: &str) -> Result<()> {
        let Some(egress) = &self.egress else { return Ok(()) };
        egress.check(url).map_err(|refusal| {
            Error::refused(refusal.host, format!("{} (`S-4`)", refusal.why))
        })
    }

    /// Where to send a request: what the binding said, or the kind's own default.
    ///
    /// The default exists because these addresses are not a choice — there is one
    /// `api.openai.com` — and a binding that must state the obvious is a binding
    /// with one more thing to get wrong. A stated `base_url` still wins, which is
    /// the case that matters: a gateway, a proxy, or Ollama on another port.
    fn base(link: &Link) -> Result<&str> {
        link.base_url
            .as_deref()
            .or_else(|| link.kind.default_base_url())
            .ok_or_else(|| {
                Error::unbound(
                    format!("link.{}", link.name),
                    format!(
                        "has no base_url and `{}` has no default. An lmlink peer is reached \
                         through the local LM Studio server, which is a separate link, and \
                         `claude-cli` is a program rather than an address.",
                        link.kind.as_str()
                    ),
                )
            })
    }

    /// Attach the credential in whatever header this kind reads it from.
    ///
    /// Anthropic wants `x-api-key` with a bare value; everything else wants
    /// `Authorization: Bearer`. Asked of the kind rather than branched on here, so
    /// a kind added later cannot be given a header in one place and forgotten in
    /// another.
    fn authorise(link: &Link, request: Request) -> Request {
        let Some(name) = &link.auth_env else { return request };
        let secret = Secret::from_env_var(name);
        let request = if link.kind.bearer_prefixed() {
            request.bearer(secret)
        } else {
            request.auth_header(link.kind.auth_header(), secret)
        };
        if link.kind == crate::link::Kind::Anthropic {
            return request.header("anthropic-version", ANTHROPIC_VERSION);
        }
        request
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
        // `M-14` asks a server which models it serves, so that a link naming one
        // it does not have is skipped rather than earning a 400. A subprocess
        // has no server to ask: there is no address, no `/v1/models`, and the
        // command decides its own model from its own configuration.
        //
        // Probing it anyway is what made a `claude-cli` link unusable. The probe
        // needed a `base_url`, a subprocess has none, so the link failed here —
        // *before* it was ever run — and the chain quietly fell through to the
        // paid link behind it. The run worked, which is what made it hard to
        // notice: the only sign was the provenance line naming the wrong model.
        if link.kind.is_subprocess() {
            return Ok(ModelFacts::of(&link.model));
        }
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
        // `M-21`: whether the link serves `/v1/responses` is asked, not
        // configured. Cached with everything else the probe learned.
        let capabilities = Capabilities::observed(link.kind, &facts)
            .with_responses(self.serves_responses(link));
        self.cache.put(key, capabilities.clone(), now);
        Ok(capabilities)
    }

    /// Which wire protocol a link speaks (`M-21`).
    ///
    /// **From the probe, not from config guesswork.** A binding that declares
    /// `protocol = responses` is a binding that is wrong the day the server is
    /// upgraded, and wrong in a way that produces a 404 rather than a message
    /// about configuration.
    /// Stream a reply, token by token, with a first-token deadline
    /// (`M-23`, `C-4`).
    ///
    /// Returns what arrived even when the stream was interrupted or the link
    /// went silent — the caller decides what that means. A silent link is
    /// failed over (`M-9`); an interruption is the operator's and is journalled
    /// as a partial rather than discarded.
    pub fn stream(
        &self,
        link: &Link,
        request: &ChatRequest,
        interrupt: impl FnMut() -> bool,
        on_event: impl FnMut(&crate::stream::Event),
    ) -> Result<crate::stream::Streamed> {
        // A command is not an address, and asking one for a `base_url` is how
        // every chat call to a `claude-cli` link came to print a failure before
        // falling back to the buffered path.
        if link.kind.is_subprocess() {
            return self.stream_command(link, request, interrupt, on_event);
        }
        let base = Self::base(link)?.trim_end_matches('/');
        let url = format!("{base}/v1/chat/completions");
        self.check_egress(&url)?;

        let mut streaming = request.clone();
        streaming.stream = true;
        let body =
            crate::security::outbound(&streaming.to_json(&link.model), link, &self.redact).text;

        let http = Self::authorise(link, Request::post_json(url, body));
        let (args, stdin) = crate::net::Curl::new().streaming_invocation(&http)?;
        crate::stream::read(
            &crate::stream::streaming_args(args),
            stdin.as_deref(),
            std::time::Duration::from_secs(crate::stream::FIRST_TOKEN_SECONDS),
            interrupt,
            on_event,
        )
    }

    /// Stream from the `claude` command (`M-23`).
    ///
    /// The buffered path's sibling, and it has to exist rather than the caller
    /// falling back: a subprocess link asked for a `base_url` it does not have,
    /// so the failure was printed and the buffered path ran anyway. Correct
    /// output, an error on every call, and no streaming for the one surface
    /// that requires it (`C-4`).
    ///
    /// `--output-format stream-json` writes one JSON object per line, which is
    /// not SSE but is line-oriented, so the deadline and interrupt machinery is
    /// the same and only the parser differs.
    ///
    /// No egress check, for the same reason as the buffered path: there is no
    /// URL to check. The CLI reaches Anthropic on its own account.
    fn stream_command(
        &self,
        link: &Link,
        request: &ChatRequest,
        interrupt: impl FnMut() -> bool,
        on_event: impl FnMut(&crate::stream::Event),
    ) -> Result<crate::stream::Streamed> {
        let program = std::env::var("PERP_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
        let prompt = crate::anthropic::cli_prompt(request);
        let stdin = crate::security::outbound(&prompt, link, &self.redact).text;
        let system = crate::anthropic::cli_system(request)
            .map(|text| crate::security::outbound(&text, link, &self.redact).text);
        let handed = system.as_deref().map(SystemFile::write).transpose()?;

        let (args, stdin) = crate::anthropic::cli_streaming_invocation(
            &program,
            &link.model,
            handed.as_ref().map(SystemFile::path),
            &stdin,
        );

        crate::stream::read_from(
            &program,
            // The program leads the list the buffered path builds; the reader
            // takes it separately, so it is not also an argument to itself.
            &args[1..],
            Some(&stdin),
            crate::stream::parse_cli_line,
            std::time::Duration::from_secs(crate::stream::FIRST_TOKEN_SECONDS),
            interrupt,
            on_event,
        )
    }

    /// Anthropic's Messages API.
    ///
    /// Same shape of call as the OpenAI path — egress checked before the socket,
    /// the body passed through `outbound` so a credential cannot leave in it — and
    /// a different body, a different path and a different reading of the answer.
    fn messages(&self, link: &Link, request: &ChatRequest) -> Result<Reply> {
        let base = Self::base(link)?.trim_end_matches('/');
        let url = format!("{base}/v1/messages");
        self.check_egress(&url)?;
        let body = crate::security::outbound(
            &crate::anthropic::request_body(request, &link.model),
            link,
            &self.redact,
        )
        .text;
        let http = Self::authorise(link, Request::post_json(url, body));
        let response = self.transport.send(&http)?;
        if !response.is_success() {
            // The body first, because Anthropic says what is wrong in it and the
            // status line alone turns "max_tokens: required" into "400". The
            // complaint is the fallback for a failure with nothing readable in it.
            return Err(crate::anthropic::parse(&response.body).err().unwrap_or_else(|| {
                Error::unbound(
                    format!("link.{}", link.name),
                    format!("messages: {}", response.complaint()),
                )
            }));
        }
        crate::anthropic::parse(&response.body)
    }

    /// The `claude` command, run as a subprocess.
    ///
    /// No egress check, and that is not an oversight: there is no URL to check. The
    /// CLI reaches Anthropic on its own account with its own configured
    /// credential, which is why the kind is classified `Cloud` and cannot be
    /// reached for in a `local-only` run (`M-4`).
    ///
    /// The prompt goes on stdin. A conversation is flattened first, because the
    /// command takes one prompt and has no notion of turns.
    ///
    /// The system prompt does not go with it. It is written to a temporary file
    /// and named with `--system-prompt-file`, so that the command receives it as
    /// instructions rather than as user text claiming to be instructions — see
    /// [`crate::anthropic::cli_invocation`] for what happens when it does not.
    fn command(&self, link: &Link, request: &ChatRequest) -> Result<Reply> {
        let program = std::env::var("PERP_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
        let prompt = crate::anthropic::cli_prompt(request);

        // Redacted on the way out, the same as an HTTP body: a prompt assembled
        // from a workspace can carry anything the workspace does. The system text
        // is assembled the same way and gets the same treatment.
        let stdin = crate::security::outbound(&prompt, link, &self.redact).text;
        let system = crate::anthropic::cli_system(request)
            .map(|text| crate::security::outbound(&text, link, &self.redact).text);
        let handed = system.as_deref().map(SystemFile::write).transpose()?;

        let (args, stdin) = crate::anthropic::cli_invocation(
            &program,
            &link.model,
            handed.as_ref().map(SystemFile::path),
            &stdin,
        );

        // The arguments go as a list. Joined into a line and split back apart,
        // `--tools ""` loses its empty argument and becomes `--tools`, which
        // means every tool rather than none — see `Spec::argv`.
        let spec = crate::process::Spec::new(
            args.join(" "),
            std::path::Path::new("."),
            std::time::Duration::from_secs(600),
        )
        .with_env(crate::process::Env::declared())
        .with_argv(args.clone())
        .with_stdin(stdin);
        let run = crate::process::run(&spec)?;
        if !matches!(run.exit, crate::process::Exit::Code(0)) {
            // Its own words where it has any: the payload says why, and the exit
            // code says only that it did not work.
            return Err(crate::anthropic::parse_cli(&run.stdout_tail, &link.model)
                .err()
                .unwrap_or_else(|| {
                    Error::unbound(
                        format!("link.{}", link.name),
                        format!(
                            "{program} exited {}: {}",
                            run.exit.describe(),
                            run.stderr_tail.trim()
                        ),
                    )
                }));
        }
        crate::anthropic::parse_cli(&run.stdout_tail, &link.model)
    }

    /// Ask whether `/v1/responses` exists (`M-21`).
    ///
    /// An empty POST: a server that serves the route answers 4xx-with-a-message
    /// (the body is invalid), and one that does not answers 404 or 405. Both
    /// are cheap, and neither generates a token.
    ///
    /// A transport failure answers **no**. Guessing yes on a failed probe would
    /// route every call to a route that may not exist, and the 404 that follows
    /// says nothing about configuration.
    fn serves_responses(&self, link: &Link) -> bool {
        let Ok(base) = Self::base(link) else { return false };
        let url = format!("{}/v1/responses", base.trim_end_matches('/'));
        if self.check_egress(&url).is_err() {
            return false;
        }
        let probe = Self::authorise(link, Request::post_json(url, "{}".to_string()));
        match self.transport.send(&probe) {
            Ok(response) => !matches!(response.status, 404 | 405 | 501),
            Err(_) => false,
        }
    }

    pub fn protocol_from(capabilities: &Capabilities) -> Protocol {
        if capabilities.responses {
            Protocol::Responses
        } else {
            Protocol::ChatCompletions
        }
    }

    /// Chat-completions, the universal baseline. Used when nothing has been
    /// probed — every link speaks it, so it is the safe assumption rather than
    /// a guess.
    pub fn protocol(link: &Link) -> Protocol {
        let _ = link;
        Protocol::ChatCompletions
    }

    /// One call to one link.
    pub fn chat(&self, link: &Link, request: &ChatRequest) -> Result<Reply> {
        self.speak(link, request, Self::protocol(link))
    }

    /// One call, keeping the raw response so a caller can read native tool
    /// calls out of it (`M-8`).
    pub fn chat_raw(&self, link: &Link, request: &ChatRequest) -> Result<(Reply, String)> {
        // The same two exceptions [`speak`] makes, made here too — because this
        // is the method the chain walker actually calls. Routing them in `speak`
        // alone left both unreachable from the path every real call takes: a
        // `claude-cli` link failed on `base()` needing an address it has no
        // business having, the chain fell through to the paid link behind it,
        // and the run *worked*. The only sign was a provenance line naming the
        // wrong model, which is the kind of wrong that survives a long time.
        if link.kind == crate::link::Kind::Anthropic {
            let reply = self.messages(link, request)?;
            let raw = reply.content.clone();
            return Ok((reply, raw));
        }
        if link.kind.is_subprocess() {
            let reply = self.command(link, request)?;
            let raw = reply.content.clone();
            return Ok((reply, raw));
        }
        let base = Self::base(link)?.trim_end_matches('/');
        let url = format!("{base}/v1/chat/completions");
        self.check_egress(&url)?;
        let body =
            crate::security::outbound(&request.to_json(&link.model), link, &self.redact).text;
        let http = Self::authorise(link, Request::post_json(url, body));
        let response = self.transport.send(&http)?;
        let reply = Self::interpret(link, &response)?;
        Ok((reply, response.body))
    }

    /// One call, on a named protocol (`M-21`).
    ///
    /// Two kinds never reach the protocol match. `M-21` is about choosing between
    /// OpenAI's two surfaces; Anthropic is a third wire format and `claude-cli` is
    /// not a wire at all, so both are decided by kind before that question is
    /// asked.
    pub fn speak(&self, link: &Link, request: &ChatRequest, protocol: Protocol) -> Result<Reply> {
        if link.kind == crate::link::Kind::Anthropic {
            return self.messages(link, request);
        }
        if link.kind.is_subprocess() {
            return self.command(link, request);
        }
        match protocol {
            Protocol::Responses => {
                let base = Self::base(link)?.trim_end_matches('/');
                let url = format!("{base}/v1/responses");
                self.check_egress(&url)?;
                let body = crate::security::outbound(
                    &request.to_responses_json(&link.model),
                    link,
                    &self.redact,
                )
                .text;
                let http = Self::authorise(link, Request::post_json(url, body));
                let response = self.transport.send(&http)?;
                if !response.is_success() {
                    return Err(Error::unbound(
                        format!("link.{}", link.name),
                        format!("responses: {}", response.complaint()),
                    ));
                }
                parse_responses(&response.body)
            }
            Protocol::ChatCompletions => {
                let base = Self::base(link)?.trim_end_matches('/');
                let url = format!("{base}/v1/chat/completions");
                // `S-4` before the socket, not after: an allowlist checked once
                // the connection is open is an allowlist that has already
                // leaked the DNS query and the TLS SNI.
                self.check_egress(&url)?;

                // `S-3`: redaction happens here rather than at the call site,
                // because a call site that has to remember is a call site that
                // will forget. A local link is left alone — redacting a prompt
                // on the way to the operator's own GPU buys nothing.
                let body = crate::security::outbound(
                    &request.to_json(&link.model),
                    link,
                    &self.redact,
                )
                .text;

                let http = Self::authorise(link, Request::post_json(url, body));
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

            // `M-15`: one GPU serving one model does not want four parallel
            // requests. A link at its limit is skipped rather than queued.
            let Some(_permit) = self.permits.acquire(link) else {
                fell_through.push(Attempt {
                    link: link.name.clone(),
                    outcome: format!("at its concurrency limit of {}", link.concurrency),
                });
                continue;
            };

            let started = std::time::Instant::now();
            crate::verbose::say(
                "call",
                &format!("{} · {} · {} message(s)", link.name, link.model, request.messages.len()),
            );
            for message in &request.messages {
                crate::verbose::body(&format!("call/{}", message.role), &message.content);
            }
            match self.chat_raw(link, request) {
                Ok((reply, raw)) => {
                    crate::verbose::body("reply", &reply.content);
                    crate::verbose::say(
                        "reply/usage",
                        &format!(
                            "{} in ({} cached) / {} out · {}ms",
                            reply.usage.prompt_tokens,
                            reply.usage.cache_hit_tokens,
                            reply.usage.completion_tokens,
                            started.elapsed().as_millis()
                        ),
                    );
                    return Ok(Served {
                        model: if reply.model.is_empty() {
                            link.model.clone()
                        } else {
                            reply.model.clone()
                        },
                        reply,
                        raw,
                        link: link.name.clone(),
                        quantization: facts.quantization.clone(),
                        role: role.to_string(),
                        latency_ms: started.elapsed().as_millis() as i64,
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

/// A system prompt on disk for the length of one `claude-cli` call.
///
/// The command takes its system prompt as a file. The file is a temporary one and
/// not a workspace one on purpose: written under the workspace it would be seen by
/// the file watcher, staged by a careless `git add`, and left behind for whoever
/// reads the repository next.
///
/// It is removed on drop rather than after the call, so that an error return, an
/// early `?`, or a panic in between does not leak it. Removal failure is ignored:
/// a leftover file in the temporary directory is not worth failing a call that
/// otherwise succeeded, and there is nothing useful to do about it here.
struct SystemFile(std::path::PathBuf);

impl SystemFile {
    fn write(text: &str) -> Result<SystemFile> {
        // Named by process and by a counter: two calls in one process must not
        // share a path, and two processes must not either.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let ordinal = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("perp-system-{}-{ordinal}.txt", std::process::id()));
        std::fs::write(&path, text).map_err(|failed| {
            Error::unbound(
                "link",
                format!("cannot write the system prompt to {}: {failed}", path.display()),
            )
        })?;
        Ok(SystemFile(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for SystemFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
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

    #[test]
    fn the_tools_array_reaches_the_wire() {
        // Found by running the loop against DeepSeek. The system prompt said
        // "use the tools you have been given" and the request gave it none, so
        // the model invented the output: `[grep output from expected tool
        // call]` and `**X**`. A fabricated answer, shaped like work.
        let plain = ChatRequest::new(vec![Message::user("hi")]).to_json("small");
        assert!(!plain.contains("\"tools\""), "a conversation offers none: {plain}");

        let armed = ChatRequest::new(vec![Message::user("hi")])
            .with_tools(crate::tool::wire_schemas())
            .to_json("small");
        assert!(armed.contains("\"tools\""), "{armed}");
        assert!(armed.contains("\"tool_choice\":\"auto\""), "{armed}");
        for tool in ["read", "grep", "patch", "shell"] {
            assert!(armed.contains(&format!("\"name\":\"{tool}\"")), "missing {tool}: {armed}");
        }
    }

    #[test]
    fn the_two_protocols_differ_only_at_the_link_edge() {
        // `M-21`. The engine's message model is the same either way; only the
        // serialiser knows the difference. A protocol that leaked into the
        // engine would mean every caller had to know which link it was talking
        // to.
        let request = ChatRequest::new(vec![Message::user("hello")]);

        let chat = request.to_json("small");
        assert!(chat.contains("\"messages\""), "{chat}");
        assert!(chat.contains("\"content\":\"hello\""), "a bare string: {chat}");

        let responses = request.to_responses_json("small");
        assert!(responses.contains("\"input\""), "{responses}");
        assert!(responses.contains("\"input_text\""), "a typed array: {responses}");
        assert!(!responses.contains("\"messages\""), "{responses}");
    }

    #[test]
    fn a_responses_reply_is_read_out_of_its_deeper_shape() {
        let body = r#"{
          "model":"small","status":"completed",
          "output":[
            {"type":"reasoning","content":[{"type":"output_text","text":"thinking"}]},
            {"type":"message","content":[{"type":"output_text","text":"the answer"}]}
          ],
          "usage":{"input_tokens":12,"output_tokens":3,
                   "input_tokens_details":{"cached_tokens":8}}
        }"#;
        let reply = parse_responses(body).expect("parse");
        assert_eq!(reply.content, "the answer");
        // `M-22`: reasoning is kept apart, never folded into the message.
        assert_eq!(reply.reasoning.as_deref(), Some("thinking"));
        assert_eq!(reply.usage.prompt_tokens, 12);
        assert_eq!(reply.usage.cache_hit_tokens, 8);
        assert_eq!(reply.finish_reason.as_deref(), Some("completed"));
    }

    #[test]
    fn a_responses_error_is_reported_rather_than_parsed_as_an_empty_answer() {
        let body = r#"{"error":{"message":"model not found","type":"invalid_request"}}"#;
        let err = parse_responses(body).expect_err("must not read as an empty reply");
        assert!(format!("{err}").contains("model not found"), "{err}");
    }

    #[test]
    fn a_link_that_serves_the_route_is_observed_as_serving_it() {
        // The other direction: a 400 means the route exists and the empty body
        // was rejected, which is exactly what an empty POST should produce.
        let transport = Canned::new(vec![
            Canned::ok(MODELS),
            Canned::status(400, r#"{"error":{"message":"input is required"}}"#),
        ]);
        let mut client = Client::new(&transport);
        let links = links();
        let link = links.get("here").expect("here");

        let caps = client.capabilities(&links, link, 1000).expect("probe");
        assert!(caps.responses, "400 means the route is there");
        assert_eq!(Client::protocol_from(&caps), Protocol::Responses);
    }

    #[test]
    fn the_protocol_comes_from_the_probe_and_not_from_config() {
        // `M-21`. A binding declaring `protocol = responses` is wrong the day
        // the server is upgraded, and wrong in a way that produces a 404 rather
        // than a message about configuration.
        let mut caps = crate::probe::Capabilities::expected_for(crate::link::Kind::LmStudio);
        assert_eq!(Client::protocol_from(&caps), Protocol::ChatCompletions, "the baseline");
        caps = caps.with_responses(true);
        assert_eq!(Client::protocol_from(&caps), Protocol::Responses);
    }

    #[test]
    fn a_responses_call_goes_to_the_responses_route() {
        let transport = Canned::new(vec![Canned::ok(
            r#"{"model":"small","output":[{"type":"message","content":[{"type":"output_text","text":"hi"}]}]}"#,
        )]);
        let client = Client::new(&transport);
        let links = links();
        let here = links.get("here").expect("link");

        let reply = client
            .speak(here, &ChatRequest::new(vec![Message::user("x")]), Protocol::Responses)
            .expect("call");
        assert_eq!(reply.content, "hi");
        assert!(
            transport.seen.borrow()[0].ends_with("/v1/responses"),
            "{:?}",
            transport.seen.borrow()
        );
    }
    use crate::link::AssumeHealthy;
    use crate::net::Response;
    use std::cell::RefCell;

    /// A transport with recorded answers, so the client is tested without a
    /// server, a GPU or a key.
    #[derive(Debug)]
    struct Canned {
        answers: RefCell<Vec<Result<Response>>>,
        seen: RefCell<Vec<String>>,
        /// What was actually sent. `S-3` is a claim about the bytes on the
        /// wire, so a test that only checks the return value proves nothing.
        bodies: RefCell<Vec<String>>,
    }

    impl Canned {
        fn new(answers: Vec<Result<Response>>) -> Canned {
            Canned {
                answers: RefCell::new(answers),
                seen: RefCell::new(Vec::new()),
                bodies: RefCell::new(Vec::new()),
            }
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
            if let Some(body) = &request.body {
                self.bodies.borrow_mut().push(body.clone());
            }
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
    fn a_key_shaped_string_is_stripped_before_it_reaches_a_cloud_link() {
        // `S-3`, on the wire rather than in a helper. The redaction lives in
        // `Client::chat` precisely because a call site that has to remember to
        // redact is a call site that will forget.
        let transport = Canned::new(vec![Canned::ok(CHAT)]);
        let client = Client::new(&transport);
        let links = links();
        let cloud = links.get("cloud").expect("the cloud link");

        let leaky = ChatRequest::new(vec![Message::user(
            "the config says DEEPSEEK_API_KEY=sk-abcdef0123456789, is that right?",
        )]);
        client.chat(cloud, &leaky).expect("the call happened");

        let sent = transport.bodies.borrow();
        let body = sent.first().expect("a body went out");
        assert!(!body.contains("sk-abcdef0123456789"), "the key reached the wire: {body}");
        assert!(body.contains("[redacted]"), "{body}");
    }

    #[test]
    fn a_local_link_gets_the_prompt_unredacted() {
        // Redacting on the way to the operator's own GPU buys nothing and makes
        // the local path worse at its job (`M-4`).
        let transport = Canned::new(vec![Canned::ok(CHAT)]);
        let client = Client::new(&transport);
        let links = links();
        let here = links.get("here").expect("the local link");

        let request =
            ChatRequest::new(vec![Message::user("token sk-abcdef0123456789 in a local prompt")]);
        client.chat(here, &request).expect("the call happened");

        let sent = transport.bodies.borrow();
        assert!(
            sent.first().expect("a body").contains("sk-abcdef0123456789"),
            "a local link is not redacted"
        );
    }

    #[test]
    fn a_host_outside_the_allowlist_is_refused_before_the_socket_opens() {
        // `S-4`. Checked before the transport is touched: an allowlist enforced
        // after the connection is open has already leaked the DNS query and the
        // TLS SNI.
        let transport = Canned::new(vec![Canned::ok(CHAT)]);
        let client = Client::new(&transport)
            .with_egress(crate::security::Egress::new(vec!["localhost".into()]));
        let links = links();
        let cloud = links.get("cloud").expect("the cloud link");

        let err = client
            .chat(cloud, &ChatRequest::new(vec![Message::user("hello")]))
            .expect_err("api.deepseek.com is not on the list");
        assert!(format!("{err}").contains("S-4"), "{err}");
        assert!(transport.seen.borrow().is_empty(), "and nothing was sent at all");
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
        //
        // One probe is now two round trips — the model listing (`M-7`) and the
        // `/v1/responses` endpoint (`M-21`) — because both are observed rather
        // than configured. What matters is that the *second* `capabilities`
        // call adds none.
        let transport = Canned::new(vec![Canned::ok(MODELS), Canned::status(404, "{}")]);
        let mut client = Client::new(&transport);
        let links = links();
        let link = links.get("here").expect("here");

        let first = client.capabilities(&links, link, 1000).expect("probe");
        assert_eq!(first.context_length, Some(32768));
        assert!(!first.responses, "a 404 on the route means it is not served");
        let asked = transport.seen.borrow().len();

        let second = client.capabilities(&links, link, 1200).expect("cached");
        assert_eq!(first, second);
        assert_eq!(
            transport.seen.borrow().len(),
            asked,
            "the second call was served entirely from the cache"
        );
    }

    #[test]
    fn a_stale_probe_asks_the_link_again() {
        // The other half of `M-6`: cached until the TTL, then re-checked. A
        // model swapped in LM Studio must not be believed to be the old one
        // forever.
        let transport = Canned::new(vec![
            Canned::ok(MODELS),
            Canned::status(404, "{}"),
            Canned::ok(MODELS),
            Canned::status(404, "{}"),
        ]);
        let mut client = Client::new(&transport);
        let links = links();
        let link = links.get("here").expect("here");

        client.capabilities(&links, link, 1000).expect("probe");
        let after_first = transport.seen.borrow().len();
        client.capabilities(&links, link, 1000 + 3601).expect("re-probe");
        assert!(
            transport.seen.borrow().len() > after_first,
            "past the ttl, it asks again"
        );
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
    fn a_call_records_what_it_cost_and_who_answered() {
        // `M-11` end to end: the journal record carries the bill, and the
        // ledger is rebuilt from it rather than accumulated in memory.
        let transport = Canned::new(vec![Canned::ok(MODELS), Canned::ok(CHAT)]);
        let mut client = Client::new(&transport);
        let links = links();
        let served = client
            .call(
                &links,
                Role::Compactor,
                &ChatRequest::new(vec![Message::user("hi")]),
                &AssumeHealthy,
                Mode::Any,
                1000,
            )
            .expect("served");

        assert_eq!(served.role, "compactor");
        assert_eq!(served.quantization.as_deref(), Some("Q4_K_M"), "recorded, per M-10");

        let price = crate::cost::Price { cache_hit: 0.0028, cache_miss: 0.14, output: 0.28 };
        let step = crate::step::StepId::parse("c2/b9/s01").expect("step");
        let record = served.to_record(step, 1000, price);
        let ledger = crate::cost::Ledger::replay(&[record]);

        assert_eq!(ledger.entries.len(), 1);
        let entry = &ledger.entries[0];
        assert_eq!(entry.link, "here");
        assert_eq!(entry.role, "compactor");
        // The fixture reports no cache split, so all input prices as a miss.
        assert_eq!(entry.usage.cache_miss_tokens, 12);
        assert_eq!(entry.usage.output_tokens, 3);
        assert!(entry.charge > 0.0);
    }

    #[test]
    fn a_link_at_its_limit_is_skipped_and_the_skip_is_recorded() {
        // `M-15` through the call path: not queued, not exceeded, not silent.
        let transport = Canned::new(vec![Canned::ok(MODELS), Canned::ok(CHAT)]);
        let mut client = Client::new(&transport);
        let links = links();
        let permits = std::sync::Arc::clone(&client.permits);
        let held = permits.acquire(links.get("here").expect("here")).expect("hold it");

        let err = client
            .call(
                &links,
                Role::Compactor,
                &ChatRequest::new(vec![Message::user("hi")]),
                &AssumeHealthy,
                Mode::Any,
                1000,
            )
            .expect_err("the only link is busy");
        assert!(format!("{err}").contains("concurrency limit"), "{err}");

        drop(held);
        client
            .call(
                &links,
                Role::Compactor,
                &ChatRequest::new(vec![Message::user("hi")]),
                &AssumeHealthy,
                Mode::Any,
                1000,
            )
            .expect("and once released, it goes through");
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
