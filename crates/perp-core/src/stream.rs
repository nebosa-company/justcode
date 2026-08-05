//! Server-sent events, and the deadline that makes them safe (`M-23`, `C-4`).
//!
//! Streaming is required for the chat surface and optional for the loop, and
//! **a first-token deadline applies either way**. That second half is the
//! requirement doing the work: a link that has said nothing in *t* seconds is
//! failed over, not waited on.
//!
//! The failure it prevents is specific and expensive. A model server that has
//! accepted the connection, allocated the context and then wedged — a GPU that
//! went into a bad state, a queue that will never drain — looks exactly like a
//! model that is thinking hard. Without a first-token deadline the loop waits
//! out the *whole-request* timeout, which is minutes, and does it on every
//! retry. With one, it fails over in seconds and says why.
//!
//! ## Why `curl -N` and a pipe
//!
//! `N-11` still holds: no HTTP crate. `curl -N` disables buffering and writes
//! each SSE chunk to stdout as it arrives, so streaming is a matter of reading
//! the child's stdout line by line rather than waiting for it to exit. That is
//! `std::process` and nothing more.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};
use crate::json::{self, Value};

/// One thing that arrived on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A piece of the assistant's message.
    Delta(String),
    /// A piece of a separate reasoning channel, kept apart from the message
    /// (`M-22`) — journalled, never concatenated, never replayed.
    Reasoning(String),
    /// Fragments of the tool calls a model is asking for.
    ///
    /// Carried as a vector because one chunk may advance several calls at
    /// once, and dropping the ones after the first would silently lose a tool
    /// call — the kind of loss that looks like a model that changed its mind.
    ToolCalls(Vec<ToolCallDelta>),
    /// The provider said it is finished.
    Done { finish_reason: Option<String> },
    /// Usage, which most providers send in the final chunk.
    /// `cached` is the part of `prompt` the provider says it had already,
    /// which is priced at a fraction of the rest. Carried because a streamed
    /// call costs the same as a buffered one and the ledger has to agree
    /// (`M-11`): recording zero here charged every cache hit at miss price.
    Usage { prompt: i64, completion: i64, cached: i64 },
}

/// One chunk's worth of a tool call, as it arrives on the wire.
///
/// Every field except `index` is optional because the provider sends the
/// identity once and then only argument text: the first chunk carries `id` and
/// `function.name`, and the dozen after it carry a few characters of
/// `arguments` each. `index` is what ties them together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: String,
}

/// One tool call, assembled from the fragments it arrived in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON text. Left as text rather than parsed here, because the ladder
    /// owns what a malformed argument object means (`M-8`).
    pub arguments: String,
}

/// Why a stream stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The provider finished.
    Complete { finish_reason: Option<String> },
    /// Nothing arrived within the first-token deadline (`M-23`). Fail over
    /// rather than wait — this is the wedged-server case.
    Silent { after: Duration },
    /// The operator interrupted (`C-4`). Whatever arrived is kept.
    Interrupted,
    /// The transport itself failed.
    Failed { why: String },
}

impl Stop {
    pub fn is_complete(&self) -> bool {
        matches!(self, Stop::Complete { .. })
    }

    /// Whether the next link in the chain should be tried (`M-9`). A silent
    /// link and a broken one both fail over; an interruption is the operator's
    /// decision and must not silently re-ask somewhere else.
    pub fn should_fail_over(&self) -> bool {
        matches!(self, Stop::Silent { .. } | Stop::Failed { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Stop::Complete { finish_reason } => match finish_reason {
                Some(reason) => format!("complete ({reason})"),
                None => "complete".into(),
            },
            Stop::Silent { after } => format!(
                "said nothing for {}s — failed over rather than waited on (`M-23`)",
                after.as_secs()
            ),
            Stop::Interrupted => "interrupted by the operator (`C-4`)".into(),
            Stop::Failed { why } => format!("transport failed: {why}"),
        }
    }
}

/// Parse one SSE line into an event. `None` for the lines that carry no
/// content — comments, blanks, and the `[DONE]` sentinel's own framing.
pub fn parse_line(line: &str) -> Option<Event> {
    let payload = line.strip_prefix("data:")?.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return payload.eq("[DONE]").then_some(Event::Done { finish_reason: None });
    }
    let parsed = json::parse(payload).ok()?;

    // Usage arrives in its own chunk on most providers, with empty choices.
    if let Some(usage) = parsed.get("usage") {
        let int = |name: &str| usage.get(name).and_then(Value::as_i64).unwrap_or_default();
        let (prompt, completion) = (int("prompt_tokens"), int("completion_tokens"));
        // DeepSeek names it `prompt_cache_hit_tokens`; the OpenAI shape nests
        // the same number under `prompt_tokens_details.cached_tokens`. Either
        // is read, and neither being present is an honest zero.
        let cached = {
            let flat = int("prompt_cache_hit_tokens");
            if flat > 0 {
                flat
            } else {
                usage
                    .get("prompt_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or_default()
            }
        };
        if prompt > 0 || completion > 0 {
            return Some(Event::Usage { prompt, completion, cached });
        }
    }

    let choice = parsed.get("choices").and_then(Value::as_arr).and_then(<[Value]>::first)?;
    if let Some(delta) = choice.get("delta") {
        // Reasoning first: a provider that sends both in one chunk must not
        // have its reasoning folded into the message (`M-22`).
        if let Some(text) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
        {
            if !text.is_empty() {
                return Some(Event::Reasoning(text.to_string()));
            }
        }
        // Before content, because a chunk that carries a tool call carries an
        // empty `content` alongside it and would otherwise fall through to the
        // `finish_reason` line below with the call dropped on the floor.
        if let Some(entries) = delta.get("tool_calls").and_then(Value::as_arr) {
            let deltas: Vec<ToolCallDelta> = entries
                .iter()
                .map(|entry| {
                    let function = entry.get("function");
                    fn text(value: Option<&Value>) -> Option<&str> {
                        value.and_then(Value::as_str)
                    }
                    ToolCallDelta {
                        // A provider that omits `index` is sending one call at
                        // a time, which is index zero.
                        index: entry
                            .get("index")
                            .and_then(Value::as_i64)
                            .unwrap_or_default()
                            .max(0) as usize,
                        id: text(entry.get("id")).map(str::to_string),
                        name: text(function.and_then(|f| f.get("name"))).map(str::to_string),
                        arguments: text(function.and_then(|f| f.get("arguments")))
                            .unwrap_or_default()
                            .to_string(),
                    }
                })
                .collect();
            if !deltas.is_empty() {
                return Some(Event::ToolCalls(deltas));
            }
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                return Some(Event::Delta(text.to_string()));
            }
        }
    }
    choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(|reason| Event::Done { finish_reason: Some(reason.to_string()) })
}

/// Parse one line of `claude --output-format stream-json`.
///
/// Not SSE. The CLI writes one JSON object per line with no `data:` framing,
/// wrapping the Messages API's own events in `{"type":"stream_event","event":…}`
/// and emitting several kinds that are not the answer — `system`, `assistant`,
/// `rate_limit_event` — which are the `None` cases.
///
/// Usage is read from `result`, the last line, and not from the `message_delta`
/// that also carries it. Both are correct and taking both would double the
/// ledger.
pub fn parse_cli_line(line: &str) -> Option<Event> {
    let parsed = json::parse(line.trim()).ok()?;
    let kind = parsed.get("type").and_then(Value::as_str).unwrap_or_default();

    // The terminal record, which is the one that knows what the call cost.
    if kind == "result" || parsed.get("total_cost_usd").is_some() {
        let usage = parsed.get("usage")?;
        let int = |name: &str| usage.get(name).and_then(Value::as_i64).unwrap_or_default();
        return Some(Event::Usage {
            prompt: int("input_tokens"),
            completion: int("output_tokens"),
            // A read is a hit. Creation costs *more* than a miss, not less, so
            // counting it here would understate the bill (`M-11`).
            cached: int("cache_read_input_tokens"),
        });
    }

    let event = parsed.get("event")?;
    match event.get("type").and_then(Value::as_str).unwrap_or_default() {
        "content_block_delta" => {
            let delta = event.get("delta")?;
            // Reasoning first and kept apart (`M-22`): folded into the message
            // it would be replayed back to the model as if it had said it.
            if let Some(text) = delta.get("thinking").and_then(Value::as_str) {
                return (!text.is_empty()).then(|| Event::Reasoning(text.to_string()));
            }
            let text = delta.get("text").and_then(Value::as_str)?;
            (!text.is_empty()).then(|| Event::Delta(text.to_string()))
        }
        "message_delta" => {
            let reason = event.get("delta")?.get("stop_reason").and_then(Value::as_str)?;
            Some(Event::Done { finish_reason: Some(reason.to_string()) })
        }
        _ => None,
    }
}

/// How long a link may say nothing before it is failed over (`M-23`).
///
/// Twenty seconds: longer than a cold model's first token on a slow local rig,
/// far shorter than the whole-request timeout, and short enough that a wedged
/// server costs one failover rather than a stalled batch.
pub const FIRST_TOKEN_SECONDS: u64 = 20;

/// What a stream produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Streamed {
    pub content: String,
    /// Kept apart from `content` (`M-22`).
    pub reasoning: String,
    /// The tool calls asked for, assembled in the order the provider indexed
    /// them. Empty for a plain answer, which is not a failure.
    pub tool_calls: Vec<ToolCall>,
    pub stop: Stop,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// The part of the prompt the provider had already, priced at a fraction.
    pub cached_tokens: i64,
    /// How long until the first token arrived. The number that says whether a
    /// link is slow or wedged, and they need different answers.
    pub first_token: Option<Duration>,
}

impl Streamed {
    /// Whether there is anything worth journalling. An interrupted stream with
    /// half a sentence in it is worth keeping (`C-4`); a silent one is not
    /// content, it is a failure.
    ///
    /// A turn that asked for a tool and said nothing else is the loop's most
    /// common turn, so tool calls count: judging it on prose alone would call
    /// the ordinary case empty.
    pub fn has_content(&self) -> bool {
        !self.content.is_empty() || !self.tool_calls.is_empty()
    }
}

/// Read an SSE stream from `curl`, with a first-token deadline.
///
/// `interrupt` is polled between events: when it returns true the child is
/// killed and whatever arrived is kept (`C-4`). Passing `|| false` streams to
/// completion.
pub fn read(
    args: &[String],
    stdin: Option<&str>,
    first_token: Duration,
    interrupt: impl FnMut() -> bool,
    on_event: impl FnMut(&Event),
) -> Result<Streamed> {
    read_from("curl", args, stdin, parse_line, first_token, interrupt, on_event)
}

/// The same reader, over any program that writes one event per line.
///
/// Split out because `claude-cli` is not an address and cannot be reached with
/// `curl`, but everything that makes streaming *safe* — the first-token
/// deadline, the interrupt poll, killing the child and keeping what arrived —
/// has nothing to do with which program produced the lines. Left specialised,
/// the subprocess link had no streaming path at all: it asked for a `base_url`
/// it does not have, failed, and every chat call printed the failure before
/// falling back.
///
/// `parse` turns one line into an event, or `None` for a line that carries
/// nothing — framing, keep-alives, and in the CLI's case the several event
/// kinds that are not the answer.
pub fn read_from(
    program: &str,
    args: &[String],
    stdin: Option<&str>,
    parse: fn(&str) -> Option<Event>,
    first_token: Duration,
    mut interrupt: impl FnMut() -> bool,
    mut on_event: impl FnMut(&Event),
) -> Result<Streamed> {
    let mut child = Command::new(crate::process::program_path(program))
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::io(program, e))?;

    if let (Some(text), Some(pipe)) = (stdin, child.stdin.as_mut()) {
        use std::io::Write;
        // The credential goes here and nowhere else (`S-2`): not argv, not a
        // file, not the journal. For the CLI it is the prompt rather than a
        // credential, and the same reasoning applies for the same reason.
        pipe.write_all(text.as_bytes())
            .map_err(|e| Error::io(format!("{program} stdin"), e))?;
    }
    drop(child.stdin.take());

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err(Error::unbound(program, "produced no stdout pipe"));
    };

    // The reader thread exists only so the deadline can be enforced: a blocking
    // read on a wedged server never returns, and a timeout around it is the
    // whole point of `M-23`.
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    // When something last arrived, which is what the deadline is actually
    // about. Starts with the clock, so before anything has come back it is the
    // same instant as `started` and the first-token case needs no branch of its
    // own.
    let mut last = started;
    let mut out = Streamed {
        content: String::new(),
        reasoning: String::new(),
        tool_calls: Vec::new(),
        stop: Stop::Complete { finish_reason: None },
        prompt_tokens: 0,
        cached_tokens: 0,
        completion_tokens: 0,
        first_token: None,
    };

    loop {
        if interrupt() {
            out.stop = Stop::Interrupted;
            break;
        }
        // Before the first token the deadline is `first_token`; after it, the
        // gap between tokens is allowed to be as long again. A model that
        // started answering has demonstrated it is alive.
        //
        // That was the stated intent and not the behaviour: the gap was never
        // measured. `budget` became a constant `first_token` once anything had
        // arrived, so it never reached zero, and the timeout arm below only
        // breaks while `first_token` is `None` — so a stream that fell silent
        // *after* its first token had nothing left that could end it. Only the
        // pipe closing or an interrupt could, and the loop passes `|| false`
        // for the interrupt.
        //
        // One rule instead of two, keyed on when something last arrived rather
        // than on whether anything ever did. `last` starts equal to `started`,
        // so the first-token case falls out of the same arithmetic.
        let budget = first_token.saturating_sub(last.elapsed());
        if budget.is_zero() {
            out.stop = Stop::Silent { after: last.elapsed() };
            break;
        }

        match rx.recv_timeout(budget.min(Duration::from_millis(250))) {
            Ok(line) => {
                // Before parsing: a keep-alive or a framing line carries no
                // event and is still the link saying it is there. Liveness is
                // about the socket, not about the content.
                last = Instant::now();
                let Some(event) = parse(&line) else { continue };
                if out.first_token.is_none() {
                    out.first_token = Some(started.elapsed());
                }
                on_event(&event);
                match &event {
                    Event::Delta(text) => out.content.push_str(text),
                    Event::Reasoning(text) => out.reasoning.push_str(text),
                    Event::ToolCalls(deltas) => {
                        for delta in deltas {
                            // Grow to fit rather than index blindly: a provider
                            // is entitled to start at index 1, and a panic here
                            // would take the batch down (`N-9`).
                            if out.tool_calls.len() <= delta.index {
                                out.tool_calls.resize(delta.index + 1, ToolCall::default());
                            }
                            let Some(call) = out.tool_calls.get_mut(delta.index) else {
                                continue;
                            };
                            if let Some(id) = &delta.id {
                                call.id.clone_from(id);
                            }
                            if let Some(name) = &delta.name {
                                call.name.clone_from(name);
                            }
                            call.arguments.push_str(&delta.arguments);
                        }
                    }
                    Event::Usage { prompt, completion, cached } => {
                        out.prompt_tokens = *prompt;
                        out.completion_tokens = *completion;
                        out.cached_tokens = *cached;
                    }
                    Event::Done { finish_reason } => {
                        out.stop = Stop::Complete { finish_reason: finish_reason.clone() };
                    }
                }
            }
            // Not a failure on its own — the 250ms cap above exists so the
            // interrupt is polled often, so most timeouts are just that poll.
            // Whether the silence has gone on too long is the budget check at
            // the top of the loop, which sees the same `last` and answers it
            // once rather than in two places that could disagree.
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // The pipe closed: curl exited.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    Ok(out)
}

/// Rebuild the non-streaming response shape from a stream.
///
/// Everything above the transport reads the buffered shape:
/// `ladder::parse_native` wants `choices[0].message.tool_calls`, and
/// `client::parse_chat` wants `message.content`, `message.reasoning_content`
/// and a `usage` object. Reassembling it here rather than teaching each of them
/// a second wire format keeps one parser per format and makes streaming what it
/// actually is — a transport detail (`M-21`).
///
/// Reasoning goes back in its own field and never into `content` (`M-22`).
pub fn buffered_shape(streamed: &Streamed, model: &str) -> String {
    let mut message = vec![
        ("role".to_string(), Value::str("assistant")),
        ("content".to_string(), Value::str(streamed.content.clone())),
    ];
    if !streamed.reasoning.is_empty() {
        message.push(("reasoning_content".to_string(), Value::str(streamed.reasoning.clone())));
    }
    if !streamed.tool_calls.is_empty() {
        let calls = streamed
            .tool_calls
            .iter()
            .map(|call| {
                Value::Obj(vec![
                    ("id".to_string(), Value::str(call.id.clone())),
                    ("type".to_string(), Value::str("function")),
                    (
                        "function".to_string(),
                        Value::Obj(vec![
                            ("name".to_string(), Value::str(call.name.clone())),
                            ("arguments".to_string(), Value::str(call.arguments.clone())),
                        ]),
                    ),
                ])
            })
            .collect();
        message.push(("tool_calls".to_string(), Value::Arr(calls)));
    }
    let finish = match &streamed.stop {
        Stop::Complete { finish_reason } => finish_reason.clone(),
        _ => None,
    };
    // The miss is the remainder. `M-11` prices hit and miss ~50× apart, so a
    // rebuilt body that reported only the total would charge every cache hit at
    // miss price — which is how the ledger came to disagree with the invoice.
    let miss = (streamed.prompt_tokens - streamed.cached_tokens).max(0);
    json::to_string(&Value::Obj(vec![
        ("model".to_string(), Value::str(model)),
        (
            "choices".to_string(),
            Value::Arr(vec![Value::Obj(vec![
                ("index".to_string(), Value::int(0)),
                ("message".to_string(), Value::Obj(message)),
                ("finish_reason".to_string(), finish.map_or(Value::Null, Value::str)),
            ])]),
        ),
        (
            "usage".to_string(),
            Value::Obj(vec![
                ("prompt_tokens".to_string(), Value::int(streamed.prompt_tokens)),
                ("completion_tokens".to_string(), Value::int(streamed.completion_tokens)),
                ("prompt_cache_hit_tokens".to_string(), Value::int(streamed.cached_tokens)),
                ("prompt_cache_miss_tokens".to_string(), Value::int(miss)),
            ]),
        ),
    ]))
}

/// Add the flags that make `curl` stream (`M-23`).
///
/// `-N` is the load-bearing one: without it curl buffers, and the first token
/// arrives at the same time as the last — which passes every test and defeats
/// the entire point.
pub fn streaming_args(base: Vec<String>) -> Vec<String> {
    let mut args = vec!["--no-buffer".to_string()];
    args.extend(base);
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `M-23`: a stream that falls silent **after** its first token is failed
    /// over, not waited on.
    ///
    /// The first test to drive [`read_from`] against a real process rather than
    /// a parser, which is why this went unnoticed. The deadline was documented
    /// as "before the first token … after it, the gap between tokens is allowed
    /// to be as long again" and only the first half was implemented: once
    /// anything had arrived, `budget` became a constant and the timeout arm
    /// stopped checking, so nothing could end the read but the pipe closing.
    ///
    /// The child here emits one chunk and then sleeps far longer than the
    /// deadline without exiting, so the pipe stays open and only a gap
    /// deadline can stop it.
    #[test]
    fn a_stream_that_goes_quiet_after_its_first_token_is_failed_over() {
        let chunk = r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#;
        let (program, args) = if cfg!(windows) {
            (
                "powershell",
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    format!("Write-Output '{chunk}'; Start-Sleep -Seconds 30"),
                ],
            )
        } else {
            ("sh", vec!["-c".to_string(), format!("echo '{chunk}'; sleep 30")])
        };

        let started = Instant::now();
        let streamed = read_from(
            program,
            &args,
            None,
            parse_line,
            Duration::from_secs(2),
            || false,
            |_| {},
        )
        .expect("the reader ran");

        assert_eq!(streamed.content, "hi", "the first token did arrive");
        assert!(streamed.first_token.is_some(), "so this is the gap deadline, not the first-token one");
        assert!(
            matches!(streamed.stop, Stop::Silent { .. }),
            "a stream that stopped speaking is failed over: {:?}",
            streamed.stop
        );
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "and it gave up near the deadline rather than waiting out the child: {:?}",
            started.elapsed()
        );
        assert!(streamed.stop.should_fail_over(), "`M-9` gets to try the next link");
    }

    /// A tool call arrives split across chunks: identity once, then argument
    /// text a few characters at a time.
    ///
    /// The red run for this was the loop itself. Before tool calls were read
    /// off the stream, routing the loop through it (`M-23`) produced turns that
    /// asked for nothing, because `parse_line` returned `None` for every chunk
    /// carrying a call and the ladder saw an empty message.
    #[test]
    fn a_tool_call_is_assembled_from_its_fragments() {
        let chunks = [
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read","arguments":""}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"path\""}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"a.txt\"}"}}]}}]}"#,
        ];

        let mut assembled: Vec<ToolCall> = Vec::new();
        for chunk in chunks {
            let Some(Event::ToolCalls(deltas)) = parse_line(chunk) else {
                panic!("every one of these chunks carries a tool call: {chunk}");
            };
            for delta in deltas {
                if assembled.len() <= delta.index {
                    assembled.resize(delta.index + 1, ToolCall::default());
                }
                let Some(call) = assembled.get_mut(delta.index) else { continue };
                if let Some(id) = &delta.id {
                    call.id.clone_from(id);
                }
                if let Some(name) = &delta.name {
                    call.name.clone_from(name);
                }
                call.arguments.push_str(&delta.arguments);
            }
        }

        assert_eq!(assembled.len(), 1, "one call, not one per chunk");
        assert_eq!(assembled[0].id, "call_1");
        assert_eq!(assembled[0].name, "read");
        assert_eq!(
            assembled[0].arguments, r#"{"path":"a.txt"}"#,
            "the arguments are the concatenation, and must parse as JSON afterwards"
        );
    }

    /// Two calls in one chunk. Returning only the first would lose a tool call
    /// and look like a model that changed its mind.
    #[test]
    fn one_chunk_may_advance_several_calls() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"a"}},{"index":1,"id":"call_2","function":{"name":"glob","arguments":"b"}}]}}]}"#;
        let Some(Event::ToolCalls(deltas)) = parse_line(line) else {
            panic!("a chunk with two calls is still a tool-call chunk");
        };
        assert_eq!(deltas.len(), 2, "both calls survive the parse");
        assert_eq!(deltas[1].index, 1);
        assert_eq!(deltas[1].name.as_deref(), Some("glob"));
    }

    /// A turn that asked for a tool and said nothing else is the loop's most
    /// common turn. Judged on prose alone it would be called empty and dropped.
    #[test]
    fn a_toolonly_turn_has_content() {
        let streamed = Streamed {
            content: String::new(),
            reasoning: String::new(),
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                arguments: "{}".into(),
            }],
            stop: Stop::Complete { finish_reason: Some("tool_calls".into()) },
            prompt_tokens: 10,
            completion_tokens: 5,
            cached_tokens: 0,
            first_token: None,
        };
        assert!(streamed.has_content(), "a tool call is content");
    }

    /// The CLI's stream is not SSE — one JSON object per line, with the
    /// Messages API's own events wrapped a level down.
    #[test]
    fn a_cli_delta_is_read_as_content() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi there"}},"session_id":"x"}"#;
        assert_eq!(parse_cli_line(line), Some(Event::Delta("hi there".into())));
    }

    /// `M-22`: reasoning is journalled apart and never replayed, so it must not
    /// arrive as content on the way in either.
    #[test]
    fn cli_thinking_is_kept_apart_from_the_message() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"let me see"}}}"#;
        assert_eq!(parse_cli_line(line), Some(Event::Reasoning("let me see".into())));
    }

    /// The ledger has to agree with the buffered path (`M-11`). A read is a
    /// hit; creation costs more than a miss and is not counted as one.
    #[test]
    fn the_cli_result_line_carries_the_usage() {
        let line = r#"{"type":"result","is_error":false,"total_cost_usd":0.012,"usage":{"input_tokens":1,"cache_creation_input_tokens":1799,"cache_read_input_tokens":3289,"output_tokens":5}}"#;
        assert_eq!(
            parse_cli_line(line),
            Some(Event::Usage { prompt: 1, completion: 5, cached: 3289 })
        );
    }

    #[test]
    fn a_cli_stop_reason_finishes_the_stream() {
        let line = r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}}"#;
        assert_eq!(
            parse_cli_line(line),
            Some(Event::Done { finish_reason: Some("end_turn".into()) })
        );
    }

    /// The CLI emits several kinds that are not the answer. Reading one as
    /// content would put its own bookkeeping into the reply.
    #[test]
    fn the_cli_lines_that_are_not_the_answer_are_ignored() {
        for line in [
            r#"{"type":"system","subtype":"init","session_id":"x"}"#,
            r#"{"type":"rate_limit_event","rate_limit":{}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0}}"#,
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            "",
            "not json at all",
        ] {
            assert_eq!(parse_cli_line(line), None, "{line}");
        }
    }

    /// The flag that makes the deltas appear. Without it the CLI answers once
    /// at the end, which reads as a working stream and is not one (`M-23`).
    #[test]
    fn the_streaming_invocation_asks_for_partial_messages() {
        let (args, _) =
            crate::anthropic::cli_streaming_invocation("claude", "sonnet", None, "hello");
        assert!(args.iter().any(|a| a == "--include-partial-messages"), "{args:?}");
        assert!(args.iter().any(|a| a == "stream-json"), "{args:?}");
        assert!(!args.iter().any(|a| a == "json"), "and not the buffered form: {args:?}");
        // `stream-json` is refused without it.
        assert!(args.iter().any(|a| a == "--verbose"), "{args:?}");
        // Everything the buffered form established still holds.
        let at = args.iter().position(|a| a == "--tools").expect("{args:?}");
        assert_eq!(args.get(at + 1).map(String::as_str), Some(""), "{args:?}");
        assert!(args.iter().any(|a| a == "--safe-mode"), "{args:?}");
    }

    #[test]
    fn a_content_chunk_becomes_a_delta() {
        let line = r#"data: {"choices":[{"index":0,"delta":{"content":"hel"}}]}"#;
        assert_eq!(parse_line(line), Some(Event::Delta("hel".into())));
    }

    #[test]
    fn reasoning_never_becomes_message_content() {
        // `M-22`. A provider that sends both in one chunk must not have its
        // reasoning folded into the answer — it is journalled, shown behind a
        // fold, and never replayed into the next request.
        let line = r#"data: {"choices":[{"delta":{"reasoning_content":"let me think","content":""}}]}"#;
        assert_eq!(parse_line(line), Some(Event::Reasoning("let me think".into())));
    }

    #[test]
    fn the_framing_lines_carry_nothing() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line(": ping"), None, "a comment is a keepalive");
        assert_eq!(parse_line("event: message"), None);
        assert_eq!(parse_line("data: [DONE]"), Some(Event::Done { finish_reason: None }));

        // The one the red run found. A line without the `data:` prefix is not
        // an event, *even when it is valid JSON that looks exactly like one* —
        // SSE framing is what says a line is data, and a parser that reads any
        // JSON it sees would act on a fragment the protocol says to ignore.
        let unframed = r#"{"choices":[{"delta":{"content":"should be ignored"}}]}"#;
        assert_eq!(parse_line(unframed), None, "the prefix is the framing");
    }


    /// A streamed call has to price the same as a buffered one (`M-11`). The
    /// cached half of a prompt costs a fraction of the rest, and this was being
    /// dropped on the floor — so every hit was billed at miss price and the
    /// chat ledger was wrong in the expensive direction, invisibly.
    #[test]
    fn a_streamed_usage_chunk_keeps_what_the_provider_had_cached() {
        let deepseek = r#"data: {"choices":[],"usage":{"prompt_tokens":2200,"completion_tokens":40,"prompt_cache_hit_tokens":2100,"prompt_cache_miss_tokens":100}}"#;
        assert_eq!(
            parse_line(deepseek),
            Some(Event::Usage { prompt: 2200, completion: 40, cached: 2100 })
        );

        // The OpenAI shape nests the same number one level down.
        let openai = r#"data: {"choices":[],"usage":{"prompt_tokens":900,"completion_tokens":10,"prompt_tokens_details":{"cached_tokens":768}}}"#;
        assert_eq!(
            parse_line(openai),
            Some(Event::Usage { prompt: 900, completion: 10, cached: 768 })
        );

        // Neither present is an honest zero rather than a guess.
        let plain = r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;
        assert_eq!(parse_line(plain), Some(Event::Usage { prompt: 12, completion: 3, cached: 0 }));
    }

    #[test]
    fn usage_arrives_in_its_own_chunk() {
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;
        assert_eq!(parse_line(line), Some(Event::Usage { prompt: 12, completion: 3, cached: 0 }));
    }

    #[test]
    fn a_finish_reason_ends_the_stream() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_line(line), Some(Event::Done { finish_reason: Some("stop".into()) }));
    }

    #[test]
    fn a_silent_link_is_failed_over_and_an_interruption_is_not() {
        // `M-23` and `M-9`: a wedged server is somebody else's turn. An
        // interruption is the operator's decision and must not quietly re-ask
        // the same question somewhere else.
        assert!(Stop::Silent { after: Duration::from_secs(20) }.should_fail_over());
        assert!(Stop::Failed { why: "exit 7".into() }.should_fail_over());
        assert!(!Stop::Interrupted.should_fail_over());
        assert!(!Stop::Complete { finish_reason: None }.should_fail_over());
    }

    #[test]
    fn the_silent_message_says_what_it_did_and_why() {
        let text = Stop::Silent { after: Duration::from_secs(20) }.describe();
        assert!(text.contains("failed over rather than waited on"), "{text}");
        assert!(text.contains("M-23"), "{text}");
    }

    #[test]
    fn the_no_buffer_flag_is_first_and_present() {
        // Without `-N`/`--no-buffer` curl buffers, the first token arrives with
        // the last, every test still passes, and the deadline protects nothing.
        let args = streaming_args(vec!["--silent".into(), "https://x/y".into()]);
        assert_eq!(args[0], "--no-buffer");
        assert!(args.contains(&"https://x/y".to_string()));
    }

    #[test]
    fn a_stream_that_said_nothing_has_nothing_to_journal() {
        // An interrupted stream with half a sentence is worth keeping (`C-4`);
        // a silent one is a failure, not content.
        let silent = Streamed {
            content: String::new(),
            reasoning: String::new(),
            tool_calls: Vec::new(),
            stop: Stop::Silent { after: Duration::from_secs(20) },
            prompt_tokens: 0,
            cached_tokens: 0,
            completion_tokens: 0,
            first_token: None,
        };
        assert!(!silent.has_content());

        let interrupted = Streamed {
            content: "I will start by deleting the failing".into(),
            stop: Stop::Interrupted,
            ..silent
        };
        assert!(interrupted.has_content(), "the half that arrived is the interesting half");
    }

    #[test]
    fn a_real_stream_is_read_to_completion() {
        // Not a mock: `curl` reading a `file://` URL exercises the spawn, the
        // pipe, the reader thread and the parse. What it does not exercise is
        // the network, which is the transport's business.
        let dir = crate::testutil::tmpdir("stream-file");
        let path = dir.join("events.sse");
        std::fs::write(
            &path,
            "data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\
             data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\
             data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\
             data: [DONE]\n",
        )
        .expect("write");

        let url = format!("file:///{}", crate::runtime::normalise(&path));
        let args = streaming_args(vec!["--silent".into(), url]);
        let mut seen = 0;
        let streamed = read(&args, None, Duration::from_secs(10), || false, |_| seen += 1)
            .expect("read");

        assert_eq!(streamed.content, "hello");
        assert!(streamed.stop.is_complete(), "{:?}", streamed.stop);
        assert!(streamed.first_token.is_some(), "the arrival time is measured");
        assert!(seen >= 3, "every event reached the callback: {seen}");
    }

    #[test]
    fn an_interrupt_keeps_what_arrived() {
        // `C-4`. The partial is journalled, not discarded.
        let dir = crate::testutil::tmpdir("stream-interrupt");
        let path = dir.join("events.sse");
        let mut body = String::new();
        for i in 0..200 {
            body.push_str(&format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{i} \"}}}}]}}\n"
            ));
        }
        std::fs::write(&path, body).expect("write");

        let url = format!("file:///{}", crate::runtime::normalise(&path));
        let args = streaming_args(vec!["--silent".into(), url]);
        let mut seen = 0;
        let streamed = read(
            &args,
            None,
            Duration::from_secs(10),
            || {
                seen += 1;
                seen > 3
            },
            |_| {},
        )
        .expect("read");

        assert_eq!(streamed.stop, Stop::Interrupted);
        assert!(!streamed.stop.should_fail_over(), "the operator's decision is not a failure");
    }
}
