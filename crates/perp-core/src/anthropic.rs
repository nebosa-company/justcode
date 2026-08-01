//! Anthropic's Messages API, and the `claude` command (`M-1`).
//!
//! The two links that are not OpenAI-shaped. Everything else the harness talks to
//! speaks `/v1/chat/completions` closely enough that a name and a default address
//! are the whole integration; these two need their own request and their own
//! reading of the answer.
//!
//! ## Four differences that each break silently
//!
//! - **`system` is a field, not a message.** OpenAI carries the system prompt as
//!   a message with `role: "system"`. Anthropic rejects that role and takes a
//!   top-level `system` string. Send it as a message and the API errors; drop it
//!   and the model simply never sees its instructions, which is worse.
//! - **`max_tokens` is required.** OpenAI treats it as optional. Omit it here and
//!   the request is refused, so there is a default rather than an `Option`.
//! - **Content is a list of blocks.** `content` is an array of typed blocks, not
//!   a string. Reading `content` as a string yields nothing and looks like an
//!   empty answer.
//! - **Usage has different names.** `input_tokens` and `output_tokens`, with
//!   cache counts split into read and creation. Mapping them wrong makes the cost
//!   ledger quietly wrong, which is the one kind of wrong nobody notices.
//!
//! ## The command is not an endpoint
//!
//! `claude-cli` runs a program. The prompt goes on **stdin**, never in argv: a
//! prompt is longer than any command line allows, and a credential must never be
//! an argument (`S-2`) — the CLI reads its own auth from its own configuration,
//! which is the point of using it.
//!
//! **Neither of these is verified against a live endpoint.** There is no Anthropic
//! key and no `claude` on this machine. The shapes below are implemented against
//! the published wire formats and unit-tested against them, which is not the same
//! as tested.

use crate::client::{ChatRequest, Reply, Usage};
use crate::error::{Error, Result};
use crate::json::{self, Value};

/// What `max_tokens` becomes when a request does not say.
///
/// Anthropic refuses a request without it, so something has to be chosen. Large
/// enough that a coding answer is not truncated mid-patch, which is the failure
/// that costs a whole step.
pub const DEFAULT_MAX_TOKENS: i64 = 8192;

/// The Messages API body.
///
/// System messages are hoisted out of the list and joined, because the API takes
/// one `system` string and a conversation may legitimately carry more than one
/// system turn.
pub fn request_body(request: &ChatRequest, model: &str) -> String {
    let mut system = Vec::new();
    let mut messages = Vec::new();
    for message in &request.messages {
        if message.role == "system" {
            system.push(message.content.clone());
            continue;
        }
        messages.push(Value::Obj(vec![
            ("role".into(), Value::str(message.role.clone())),
            ("content".into(), Value::str(message.content.clone())),
        ]));
    }

    let mut fields = vec![
        ("model".to_string(), Value::str(model)),
        ("messages".to_string(), Value::Arr(messages)),
        (
            "max_tokens".to_string(),
            Value::int(request.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS)),
        ),
        ("stream".to_string(), Value::Bool(request.stream)),
    ];
    if !system.is_empty() {
        fields.push(("system".to_string(), Value::str(system.join("\n\n"))));
    }
    // `M-8`'s top rung, same as the OpenAI path: tools that are described but not
    // sent produce a fabricated answer that looks like work.
    if let Some(tools) = &request.tools {
        fields.push(("tools".to_string(), tools.clone()));
    }
    json::to_string(&Value::Obj(fields))
}

/// Read a Messages API response.
pub fn parse(body: &str) -> Result<Reply> {
    let parsed = json::parse(body)?;

    if let Some(error) = parsed.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("no message");
        return Err(Error::unbound("chat", format!("the link returned an error: {message}")));
    }

    // Blocks, joined. A `text` block is the answer; a `thinking` block is kept
    // apart from it (`M-22`) rather than concatenated, because reasoning that
    // leaks into content gets replayed into the next request as if the model had
    // said it out loud.
    let blocks = parsed
        .get("content")
        .and_then(Value::as_arr)
        .ok_or_else(|| Error::unbound("chat", "the response has no content blocks"))?;

    let mut content = String::new();
    let mut reasoning = String::new();
    for block in blocks {
        let kind = block.get("type").and_then(Value::as_str).unwrap_or("");
        let text = block
            .get("text")
            .or_else(|| block.get("thinking"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "text" => content.push_str(text),
            "thinking" | "redacted_thinking" => reasoning.push_str(text),
            // A `tool_use` block carries no text and is read from the raw body by
            // the ladder, which is why it is skipped rather than stringified.
            _ => {}
        }
    }

    let usage = parsed.get("usage");
    let count = |key: &str| {
        usage.and_then(|usage| usage.get(key)).and_then(Value::as_i64).unwrap_or_default()
    };

    Ok(Reply {
        content,
        reasoning: Some(reasoning).filter(|text| !text.is_empty()),
        model: parsed.get("model").and_then(Value::as_str).unwrap_or_default().to_string(),
        // `stop_reason`, not `finish_reason`.
        finish_reason: parsed.get("stop_reason").and_then(Value::as_str).map(str::to_string),
        usage: Usage {
            prompt_tokens: count("input_tokens"),
            completion_tokens: count("output_tokens"),
            // Read is a cache hit. Creation is a write — it costs more than a
            // miss, not less, so counting it as a hit would understate the bill.
            cache_hit_tokens: count("cache_read_input_tokens"),
            cache_miss_tokens: count("cache_creation_input_tokens"),
        },
    })
}

/// The command line for a `claude-cli` link, and what goes on its stdin.
///
/// `-p` is the non-interactive mode: one prompt, one answer, no session. The JSON
/// output carries usage and cost, which the ledger needs — the plain text output
/// carries neither and would make every call look free.
///
/// ## It is an agent, and a link wants a model
///
/// `claude` is not a model endpoint. It is an agent: its own tools, its own
/// permission prompts, its own reading of the operator's `CLAUDE.md`. Run as it
/// comes, it does the work *itself* — which sounds like a feature and is not. Its
/// edits land outside `X-2`'s confinement, no tool call is journalled, and no red
/// run happens, so the loop cannot say what changed or undo it. Two agents, one
/// wheel. Three flags take the agent off and leave the model:
///
/// - **`--system-prompt-file`** — the harness's prompt as an actual system
///   prompt. `system` is a path, not the text: the text is thousands of
///   characters of workspace-derived prompt, longer than a Windows command line
///   allows and visible in a process listing to every other user (`S-2`).
///
///   Folding it into the user turn instead — which is what this used to do — puts
///   a tool protocol and a claim of authority into user text, and that is the
///   exact shape of a prompt injection. A well-behaved agent refuses it. Ours
///   did, in as many words, and then used its own tools.
///
/// - **`--tools ""`** — no tools at all, so there is nothing to use but ours.
///   Denying them by name was tried and does not work: the list is long, it
///   changes between versions, and what it missed the model reported as its
///   *real* toolset — concluding that ours were fake and the tool results it was
///   being shown could not be trusted. Right conclusion, from its side.
///
///   The empty string is load-bearing and is why this returns a list rather than
///   a line: `--tools ""` means none, and `--tools` with the empty argument lost
///   means **all**, silently. See [`crate::process::Spec::argv`].
///
/// - **`--safe-mode`** — no `CLAUDE.md`, skills, plugins, hooks, MCP servers or
///   custom agents. The operator's `CLAUDE.md` is written for their own sessions
///   and says things like which two sections every answer must end with; appended
///   here it corrupts every reply, because a reply is parsed as a tool call and
///   read by no one.
///
///   Not `--bare`, which looks similar and also forces authentication through
///   `ANTHROPIC_API_KEY` — that would take the subscription out of the picture,
///   which is the whole reason this link kind exists.
pub fn cli_invocation(
    program: &str,
    model: &str,
    system: Option<&std::path::Path>,
    prompt: &str,
) -> (Vec<String>, String) {
    let mut args = vec![
        program.to_string(),
        "-p".to_string(),
        "--output-format".to_string(),
        "json".to_string(),
        "--model".to_string(),
        model.to_string(),
        // The agent, off. See above for why each of these and not another.
        "--safe-mode".to_string(),
        "--tools".to_string(),
        String::new(),
    ];
    if let Some(path) = system {
        args.push("--system-prompt-file".to_string());
        args.push(path.display().to_string());
    }
    (args, prompt.to_string())
}

/// The same invocation, asking for the stream rather than the finished answer.
///
/// `--output-format stream-json` needs `--verbose` — the CLI refuses the
/// combination without it — and `--include-partial-messages` is the flag that
/// actually makes the deltas appear. Without it the "stream" is one message at
/// the end, which passes every test and defeats the point (`M-23`).
pub fn cli_streaming_invocation(
    program: &str,
    model: &str,
    system: Option<&std::path::Path>,
    prompt: &str,
) -> (Vec<String>, String) {
    let (mut args, stdin) = cli_invocation(program, model, system, prompt);
    for arg in &mut args {
        if arg == "json" {
            "stream-json".clone_into(arg);
        }
    }
    args.push("--include-partial-messages".to_string());
    args.push("--verbose".to_string());
    (args, stdin)
}

/// The system text of a request, which the CLI takes as a file and not a message.
pub fn cli_system(request: &ChatRequest) -> Option<String> {
    let mut out = String::new();
    for message in request.messages.iter().filter(|m| m.role == "system") {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&message.content);
    }
    Some(out).filter(|text| !text.is_empty())
}

/// Flatten a conversation into the single prompt the CLI takes.
///
/// The CLI has no conversation argument, so a multi-turn request has to be
/// rendered into one prompt. Roles are labelled rather than dropped: a model given
/// an unlabelled wall of text cannot tell its own previous answers from the
/// user's.
///
/// System messages are not here. They go to [`cli_system`] and reach the command
/// as a system prompt, for the reason given on [`cli_invocation`].
pub fn cli_prompt(request: &ChatRequest) -> String {
    let mut out = String::new();
    for message in request.messages.iter().filter(|m| m.role != "system") {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        match message.role.as_str() {
            "user" => {
                out.push_str("Human: ");
                out.push_str(&message.content);
            }
            "assistant" => {
                out.push_str("Assistant: ");
                out.push_str(&message.content);
            }
            other => {
                out.push_str(other);
                out.push_str(": ");
                out.push_str(&message.content);
            }
        }
    }
    out
}

/// Read the CLI's `--output-format json` answer.
pub fn parse_cli(body: &str, model: &str) -> Result<Reply> {
    let parsed = json::parse(body)?;

    // The CLI reports failure in the payload as well as by exit code, and the
    // payload says why.
    let failed = parsed.get("is_error").and_then(Value::as_bool).unwrap_or(false);
    if failed {
        let said = parsed
            .get("result")
            .or_else(|| parsed.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("no message");
        return Err(Error::unbound("chat", format!("claude exited with an error: {said}")));
    }

    let content = parsed
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::unbound("chat", "the CLI answer has no `result`"))?
        .to_string();

    let usage = parsed.get("usage");
    let count = |key: &str| {
        usage.and_then(|usage| usage.get(key)).and_then(Value::as_i64).unwrap_or_default()
    };

    Ok(Reply {
        content,
        reasoning: None,
        // The CLI does not echo the model, so the link's own name for it stands.
        model: parsed
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(model)
            .to_string(),
        finish_reason: parsed
            .get("subtype")
            .and_then(Value::as_str)
            .map(str::to_string),
        usage: Usage {
            prompt_tokens: count("input_tokens"),
            completion_tokens: count("output_tokens"),
            cache_hit_tokens: count("cache_read_input_tokens"),
            cache_miss_tokens: count("cache_creation_input_tokens"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Message;

    fn request() -> ChatRequest {
        ChatRequest {
            messages: vec![
                Message { role: "system".into(), content: "Be terse.".into() },
                Message { role: "user".into(), content: "Add dedupe.".into() },
            ],
            max_tokens: None,
            stream: false,
            tools: None,
        }
    }

    #[test]
    fn the_system_prompt_becomes_a_field_rather_than_a_message() {
        // Anthropic rejects `role: "system"`. Sending it as a message errors, and
        // dropping it means the model never sees its instructions — the quieter
        // and worse of the two failures.
        let body = request_body(&request(), "claude-opus-5");
        let parsed = json::parse(&body).expect("valid json");

        assert_eq!(parsed.get("system").and_then(Value::as_str), Some("Be terse."));
        let messages = parsed.get("messages").and_then(Value::as_arr).expect("messages");
        assert_eq!(messages.len(), 1, "only the user turn is left: {messages:?}");
        assert_eq!(messages[0].get("role").and_then(Value::as_str), Some("user"));
    }

    #[test]
    fn max_tokens_is_always_sent_because_the_api_requires_it() {
        let body = request_body(&request(), "claude-opus-5");
        let parsed = json::parse(&body).expect("valid json");
        assert_eq!(
            parsed.get("max_tokens").and_then(Value::as_i64),
            Some(DEFAULT_MAX_TOKENS),
            "a request that does not ask still gets one"
        );

        let mut asked = request();
        asked.max_tokens = Some(64);
        let parsed = json::parse(&request_body(&asked, "m")).expect("valid json");
        assert_eq!(parsed.get("max_tokens").and_then(Value::as_i64), Some(64), "and one that asks wins");
    }

    #[test]
    fn several_system_turns_are_joined_rather_than_dropped() {
        let mut many = request();
        many.messages.insert(1, Message { role: "system".into(), content: "Cite ids.".into() });
        let parsed = json::parse(&request_body(&many, "m")).expect("valid json");
        let system = parsed.get("system").and_then(Value::as_str).expect("system");
        assert!(system.contains("Be terse."), "{system}");
        assert!(system.contains("Cite ids."), "the second one survives: {system}");
    }

    #[test]
    fn content_blocks_are_joined_and_thinking_is_kept_apart() {
        // `content` is a list of typed blocks. Reading it as a string yields
        // nothing and looks exactly like an empty answer.
        let body = r#"{
          "model": "claude-opus-5",
          "stop_reason": "end_turn",
          "content": [
            {"type": "thinking", "thinking": "consider the order"},
            {"type": "text", "text": "Added "},
            {"type": "tool_use", "id": "t1", "name": "write", "input": {}},
            {"type": "text", "text": "dedupe."}
          ],
          "usage": {
            "input_tokens": 11, "output_tokens": 7,
            "cache_read_input_tokens": 3, "cache_creation_input_tokens": 5
          }
        }"#;
        let reply = parse(body).expect("parses");
        assert_eq!(reply.content, "Added dedupe.", "text blocks join, in order");
        assert_eq!(reply.reasoning.as_deref(), Some("consider the order"), "kept apart (`M-22`)");
        assert_eq!(reply.finish_reason.as_deref(), Some("end_turn"), "stop_reason, not finish_reason");
        assert_eq!(reply.usage.prompt_tokens, 11);
        assert_eq!(reply.usage.completion_tokens, 7);
        // Read is a hit; creation is a write and costs more than a miss, so
        // counting it as a hit would understate the bill.
        assert_eq!(reply.usage.cache_hit_tokens, 3);
        assert_eq!(reply.usage.cache_miss_tokens, 5);
    }

    #[test]
    fn an_error_payload_is_an_error_and_not_an_empty_answer() {
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: required"}}"#;
        let failed = parse(body).expect_err("an error is not a reply");
        assert!(format!("{failed}").contains("max_tokens: required"), "{failed}");
    }

    #[test]
    fn a_response_with_no_blocks_is_refused_rather_than_read_as_silence() {
        assert!(parse(r#"{"model":"m"}"#).is_err(), "no content is not an empty answer");
    }

    #[test]
    fn the_cli_takes_its_prompt_on_stdin_and_never_in_argv() {
        // A prompt is longer than any command line allows, and argv is visible to
        // every other process on the machine (`S-2`).
        let (args, stdin) = cli_invocation("claude", "claude-opus-5", None, "Add dedupe.");
        assert_eq!(stdin, "Add dedupe.");
        assert!(!args.iter().any(|arg| arg.contains("Add dedupe.")), "not in argv: {args:?}");
        assert_eq!(args.first().map(String::as_str), Some("claude"));
        assert!(args.contains(&"-p".to_string()), "non-interactive: {args:?}");
        // JSON, because the plain output carries no usage and would make every
        // call look free.
        assert!(args.contains(&"json".to_string()), "{args:?}");
        assert!(args.contains(&"claude-opus-5".to_string()), "the model is named: {args:?}");
    }

    /// The system prompt is named as a file, and its text is not in argv either —
    /// it is the longest thing in the whole request.
    #[test]
    fn the_system_prompt_reaches_the_cli_as_a_system_prompt() {
        let (args, _) = cli_invocation(
            "claude",
            "claude-opus-5",
            Some(std::path::Path::new("/tmp/sys.txt")),
            "Add dedupe.",
        );
        let at = args.iter().position(|arg| arg == "--system-prompt-file").expect("{args:?}");
        assert!(args[at + 1].contains("sys.txt"), "the path follows the flag: {args:?}");
        // Unquoted, because these go as a list and never through a splitter.
        assert!(!args[at + 1].starts_with('"'), "not quoted: {args:?}");
    }

    /// The operator's own `CLAUDE.md` is written for their sessions — it says
    /// which two sections every answer must end with — and appended to ours it
    /// shapes every reply we then try to parse as a tool call.
    #[test]
    fn the_operators_own_customisations_are_left_out() {
        let (args, _) = cli_invocation("claude", "claude-opus-5", None, "Add dedupe.");
        assert!(args.iter().any(|arg| arg == "--safe-mode"), "{args:?}");
        // `--bare` looks like it would do too, and also forces authentication
        // through `ANTHROPIC_API_KEY`, which is the one thing this must not do.
        assert!(!args.iter().any(|arg| arg == "--bare"), "not this one: {args:?}");
    }

    /// The defect this whole shape exists to fix. With no file to put it in, the
    /// system text used to be folded into the user turn — where it reads as user
    /// text claiming authority over the model, which is an injection, and the
    /// agent on the other end refused it and did the job with its own tools.
    #[test]
    fn the_system_text_is_never_folded_into_the_user_turn() {
        let prompt = cli_prompt(&request());
        assert!(!prompt.contains("Be terse."), "the system text is not in the prompt: {prompt}");
        assert_eq!(cli_system(&request()).as_deref(), Some("Be terse."), "it is here instead");
    }

    /// `claude` is an agent and brings its own tools. Left with them it edits the
    /// workspace itself: outside `X-2`, unjournalled, and with no red run.
    ///
    /// The empty argument is the whole flag. `--tools ""` is no tools; `--tools`
    /// with the empty one lost is *every* tool, which is why this asserts the
    /// pair and not just the flag.
    #[test]
    fn the_commands_own_tools_are_taken_away_so_that_it_is_a_model_and_not_an_agent() {
        let (args, _) = cli_invocation("claude", "claude-opus-5", None, "Add dedupe.");
        let at = args.iter().position(|arg| arg == "--tools").expect("{args:?}");
        assert_eq!(args.get(at + 1).map(String::as_str), Some(""), "empty, not absent: {args:?}");
    }

    /// The reason [`cli_invocation`] returns a list and the caller passes it as
    /// one. Flattening it to a line and splitting it back drops the empty
    /// argument, and the flag that meant *no tools* comes out meaning *all* of
    /// them — the failure this whole shape exists to prevent, and a silent one.
    #[test]
    fn flattening_the_arguments_to_a_line_would_invert_the_tools_flag() {
        let (args, _) = cli_invocation("claude", "claude-opus-5", None, "Add dedupe.");
        let round_tripped = crate::process::split_command(&args.join(" ")).expect("split");
        let at = round_tripped.iter().position(|arg| arg == "--tools").expect("{round_tripped:?}");
        assert_ne!(
            round_tripped.get(at + 1).map(String::as_str),
            Some(""),
            "if this ever round-trips, the list can go back to being a line"
        );
    }

    #[test]
    fn a_conversation_is_flattened_with_its_roles_labelled() {
        // The CLI takes one prompt. A model handed an unlabelled wall of text
        // cannot tell its own previous answers from the user's.
        let mut talk = request();
        talk.messages.push(Message { role: "assistant".into(), content: "Done.".into() });
        let prompt = cli_prompt(&talk);
        assert!(prompt.contains("Human: Add dedupe."), "{prompt}");
        assert!(prompt.contains("Assistant: Done."), "{prompt}");
    }

    #[test]
    #[ignore = "runs a real subprocess; needs a `claude` on PATH"]
    fn the_command_path_reaches_a_real_process() {
        // The one half of this that can be exercised without an Anthropic
        // account: that the plumbing runs a program, hands it the prompt on
        // stdin, and reads its JSON back. Ignored by default because it needs a
        // binary; run with `--ignored` and `PERP_CLAUDE_BIN` pointing at one.
        let links = crate::link::Links::parse(
            "```perp-links
             link.cc.kind = claude-cli
             link.cc.model = claude-opus-5
             role.coder = cc
```
",
        )
        .expect("links");
        let link = links.get("cc").expect("link");

        let transport = crate::net::Curl::new();
        let client = crate::client::Client::new(&transport);
        let reply = client
            .chat(link, &ChatRequest::new(vec![Message::user("Add dedupe to the toolkit.")]))
            .expect("the command answered");

        assert!(reply.content.contains("saw "), "it read our stdin back: {}", reply.content);
        assert!(reply.content.contains("-p"), "and the args were passed: {}", reply.content);
        assert!(reply.usage.prompt_tokens > 0, "usage came through: {:?}", reply.usage);
        assert_eq!(reply.model, "claude-opus-5", "the link named the model");
    }

    #[test]
    fn the_cli_answer_carries_its_usage() {
        let body = r#"{
          "type": "result", "subtype": "success", "is_error": false,
          "result": "Added dedupe.",
          "usage": {"input_tokens": 20, "output_tokens": 4, "cache_read_input_tokens": 2}
        }"#;
        let reply = parse_cli(body, "claude-opus-5").expect("parses");
        assert_eq!(reply.content, "Added dedupe.");
        assert_eq!(reply.usage.prompt_tokens, 20);
        assert_eq!(reply.usage.completion_tokens, 4);
        assert_eq!(reply.usage.cache_hit_tokens, 2);
        // The CLI does not echo the model, so the link's own name stands rather
        // than an empty string reaching the cost ledger.
        assert_eq!(reply.model, "claude-opus-5");
    }

    #[test]
    fn the_cli_reporting_failure_in_its_payload_is_an_error() {
        // It says so in the payload as well as by exit code, and the payload is
        // the half that says why.
        let body = r#"{"type":"result","is_error":true,"result":"credit balance too low"}"#;
        let failed = parse_cli(body, "m").expect_err("an error is not a reply");
        assert!(format!("{failed}").contains("credit balance too low"), "{failed}");
    }
}
