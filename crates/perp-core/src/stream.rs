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
    /// The provider said it is finished.
    Done { finish_reason: Option<String> },
    /// Usage, which most providers send in the final chunk.
    Usage { prompt: i64, completion: i64 },
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
        if prompt > 0 || completion > 0 {
            return Some(Event::Usage { prompt, completion });
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
    pub stop: Stop,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// How long until the first token arrived. The number that says whether a
    /// link is slow or wedged, and they need different answers.
    pub first_token: Option<Duration>,
}

impl Streamed {
    /// Whether there is anything worth journalling. An interrupted stream with
    /// half a sentence in it is worth keeping (`C-4`); a silent one is not
    /// content, it is a failure.
    pub fn has_content(&self) -> bool {
        !self.content.is_empty()
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
    mut interrupt: impl FnMut() -> bool,
    mut on_event: impl FnMut(&Event),
) -> Result<Streamed> {
    let mut child = Command::new("curl")
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::io("curl", e))?;

    if let (Some(text), Some(pipe)) = (stdin, child.stdin.as_mut()) {
        use std::io::Write;
        // The credential goes here and nowhere else (`S-2`): not argv, not a
        // file, not the journal.
        pipe.write_all(text.as_bytes()).map_err(|e| Error::io("curl stdin", e))?;
    }
    drop(child.stdin.take());

    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        return Err(Error::unbound("curl", "produced no stdout pipe"));
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
    let mut out = Streamed {
        content: String::new(),
        reasoning: String::new(),
        stop: Stop::Complete { finish_reason: None },
        prompt_tokens: 0,
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
        let budget = match out.first_token {
            None => first_token.saturating_sub(started.elapsed()),
            Some(_) => first_token,
        };
        if budget.is_zero() {
            out.stop = Stop::Silent { after: started.elapsed() };
            break;
        }

        match rx.recv_timeout(budget.min(Duration::from_millis(250))) {
            Ok(line) => {
                let Some(event) = parse_line(&line) else { continue };
                if out.first_token.is_none() {
                    out.first_token = Some(started.elapsed());
                }
                on_event(&event);
                match &event {
                    Event::Delta(text) => out.content.push_str(text),
                    Event::Reasoning(text) => out.reasoning.push_str(text),
                    Event::Usage { prompt, completion } => {
                        out.prompt_tokens = *prompt;
                        out.completion_tokens = *completion;
                    }
                    Event::Done { finish_reason } => {
                        out.stop = Stop::Complete { finish_reason: finish_reason.clone() };
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Not necessarily a failure — the 250ms cap above exists so the
                // interrupt is checked often. Only the budget expiring is.
                if out.first_token.is_none() && started.elapsed() >= first_token {
                    out.stop = Stop::Silent { after: started.elapsed() };
                    break;
                }
            }
            // The pipe closed: curl exited.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    Ok(out)
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

    #[test]
    fn usage_arrives_in_its_own_chunk() {
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":3}}"#;
        assert_eq!(parse_line(line), Some(Event::Usage { prompt: 12, completion: 3 }));
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
            stop: Stop::Silent { after: Duration::from_secs(20) },
            prompt_tokens: 0,
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
