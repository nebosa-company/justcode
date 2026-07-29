//! The degradation ladder for tool calls (`M-8`).
//!
//! Three rungs, best first:
//!
//! 1. **Native tool calling** — the provider parses and validates, and the
//!    reply carries structured calls.
//! 2. **JSON-schema constrained output** — the provider guarantees shape but
//!    not meaning; the call arrives as a JSON object in the message.
//! 3. **A prompted block** — no guarantee at all. `tool: read` on one line,
//!    `path: src/main.rs` on the next, inside a fence. Deliberately not JSON:
//!    the models that need this rung are the ones that cannot reliably close a
//!    brace, and asking them for the format they are worst at is how a harness
//!    ends up with a repair loop that never converges.
//!
//! Two rules keep this honest.
//!
//! **The loop must complete on rung three.** Every capability above it is an
//! optimisation. A model with no tool support and no constrained output still
//! drives the loop, more slowly, and the tests here run the bottom rung
//! end-to-end for exactly that reason.
//!
//! **Repairs are bounded and then it fails.** Two repairs, quoting the actual
//! parse error, and then the step fails with that error rather than with a
//! guess. An unbounded repair loop against a model that cannot produce the
//! format burns a budget to produce nothing, and — worse — the obvious escape
//! is to accept a half-parsed call, which is how a harness runs the wrong
//! command.

use crate::error::{Error, Result};
use crate::json::{self, Value};
use crate::probe::Capabilities;
use crate::tool::{Call, Tool};

/// Per rung, not per step. Two chances to produce a format, then down a rung —
/// or, at the bottom, out.
pub const MAX_REPAIRS: u32 = 2;

/// The fence the bottom rung asks for.
pub const FENCE: &str = "perp-call";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    Native,
    JsonSchema,
    Prompted,
}

impl Rung {
    pub fn as_str(self) -> &'static str {
        match self {
            Rung::Native => "native",
            Rung::JsonSchema => "json-schema",
            Rung::Prompted => "prompted",
        }
    }

    fn below(self) -> Option<Rung> {
        match self {
            Rung::Native => Some(Rung::JsonSchema),
            Rung::JsonSchema => Some(Rung::Prompted),
            Rung::Prompted => None,
        }
    }

    /// What to tell the model, so the ask matches the rung it is on.
    pub fn instructions(self) -> String {
        match self {
            Rung::Native => "Use the tools you have been given.".into(),
            Rung::JsonSchema => {
                "Reply with one JSON object: {\"calls\":[{\"tool\":\"read\",\"args\":{\"path\":\"…\"}}]}"
                    .into()
            }
            Rung::Prompted => format!(
                "To use a tool, reply with a fenced block and nothing else:\n\
                 \n\
                 ```{FENCE}\n\
                 tool: read\n\
                 path: src/main.rs\n\
                 ```\n\
                 \n\
                 One key per line, `key: value`. One block per call. Blocks may repeat."
            ),
        }
    }
}

/// The rungs a link can actually use, best first, always ending at
/// [`Rung::Prompted`].
pub fn rungs_for(caps: &Capabilities) -> Vec<Rung> {
    let mut rungs = Vec::new();
    if caps.native_tool_calls {
        rungs.push(Rung::Native);
    }
    if caps.json_schema {
        rungs.push(Rung::JsonSchema);
    }
    rungs.push(Rung::Prompted);
    rungs
}

/// What the ladder wants to happen next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Calls came out. Zero of them is a legitimate answer — the model said
    /// something without asking for a tool — and is not a parse failure.
    Calls(Vec<Call>),
    /// Ask again at the same rung, quoting what went wrong.
    Repair { rung: Rung, attempt: u32, complaint: String },
    /// Ask again lower down. The prompt changes with the rung.
    Dropped { from: Rung, to: Rung, why: String },
    /// The bottom rung could not be parsed either. The step fails with the real
    /// error, not a substitute for one.
    Failed { reason: String },
}

/// Walks a model down the rungs.
#[derive(Debug, Clone)]
pub struct Ladder {
    rung: Rung,
    repairs: u32,
    history: Vec<String>,
}

impl Ladder {
    /// Start at the best rung a link claims. Capabilities may be a
    /// [`crate::probe::Source::KindDefault`] guess — being wrong costs a drop,
    /// which is the entire point of having a ladder.
    pub fn for_link(caps: &Capabilities) -> Ladder {
        let rung = rungs_for(caps).first().copied().unwrap_or(Rung::Prompted);
        Ladder { rung, repairs: 0, history: Vec::new() }
    }

    pub fn at(rung: Rung) -> Ladder {
        Ladder { rung, repairs: 0, history: Vec::new() }
    }

    pub fn rung(&self) -> Rung {
        self.rung
    }

    pub fn repairs_used(&self) -> u32 {
        self.repairs
    }

    /// Everything that went wrong on the way down, for the journal. A step that
    /// took four attempts should say so rather than reporting the last one.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// The link itself rejected this rung — a 400 on the `tools` parameter, a
    /// server that ignores `response_format`. Not a parse failure, so it does
    /// not spend a repair.
    pub fn unsupported(&mut self, why: impl Into<String>) -> Next {
        let why = why.into();
        self.history.push(format!("{}: {why}", self.rung.as_str()));
        self.drop_rung(why)
    }

    /// Feed a reply in. `native` is the raw response body when the current rung
    /// is [`Rung::Native`]; the other rungs read `content`.
    pub fn feed(&mut self, content: &str, native: Option<&str>) -> Next {
        let parsed = match self.rung {
            Rung::Native => match native {
                Some(body) => parse_native(body),
                None => Err(Error::refused(
                    "native tool calls",
                    "the transport did not hand over the response body",
                )),
            },
            Rung::JsonSchema => parse_json_object(content),
            Rung::Prompted => parse_block(content),
        };

        match parsed {
            Ok(calls) => Next::Calls(calls),
            Err(e) => {
                let complaint = format!("{e}");
                self.history.push(format!("{}: {complaint}", self.rung.as_str()));
                if self.repairs < MAX_REPAIRS {
                    self.repairs += 1;
                    Next::Repair { rung: self.rung, attempt: self.repairs, complaint }
                } else {
                    self.drop_rung(complaint)
                }
            }
        }
    }

    fn drop_rung(&mut self, why: String) -> Next {
        match self.rung.below() {
            Some(to) => {
                let from = self.rung;
                self.rung = to;
                self.repairs = 0;
                Next::Dropped { from, to, why }
            }
            None => Next::Failed {
                reason: format!(
                    "the bottom rung could not be parsed after {MAX_REPAIRS} repairs: {why}"
                ),
            },
        }
    }
}

/// The OpenAI-shaped `tool_calls` array, straight off the response.
pub fn parse_native(body: &str) -> Result<Vec<Call>> {
    let parsed = json::parse(body)?;
    let bad = |reason: &str| Error::refused("native tool calls", reason);

    let Some(choices) = parsed.get("choices").and_then(Value::as_arr) else {
        return Err(bad("the response has no `choices`"));
    };
    let Some(message) = choices.first().and_then(|c| c.get("message")) else {
        return Err(bad("the first choice has no `message`"));
    };
    // No `tool_calls` at all is a plain answer, not a malformed one.
    let Some(entries) = message.get("tool_calls").and_then(Value::as_arr) else {
        return Ok(Vec::new());
    };

    let mut calls = Vec::new();
    for entry in entries {
        let function = entry.get("function").ok_or_else(|| bad("a tool call has no `function`"))?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a tool call has no name"))?;
        let arguments = function.get("arguments").and_then(Value::as_str).unwrap_or("{}");
        // Providers send this as a JSON *string*, so it is parsed twice on
        // purpose — and a model that produced broken JSON inside it is exactly
        // the case the repair loop exists for.
        let args = json::parse(arguments)
            .map_err(|e| bad(&format!("the arguments of `{name}` are not JSON: {e}")))?;
        calls.push(call_from(name, &args)?);
    }
    Ok(calls)
}

/// `{"calls":[{"tool":"read","args":{"path":"…"}}]}` — the constrained rung.
pub fn parse_json_object(content: &str) -> Result<Vec<Call>> {
    let bad = |reason: String| Error::refused("constrained output", reason);
    let text = strip_fence(content);
    let parsed = json::parse(text)?;
    let Some(entries) = parsed.get("calls").and_then(Value::as_arr) else {
        return Err(bad("the object has no `calls` array".into()));
    };
    let mut calls = Vec::new();
    for entry in entries {
        let name = entry
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("a call has no `tool`".into()))?;
        let empty = Value::Obj(Vec::new());
        let args = entry.get("args").cloned().unwrap_or(empty);
        calls.push(call_from(name, &args)?);
    }
    Ok(calls)
}

/// The bottom rung: fenced `key: value` lines.
///
/// Text around the blocks is ignored rather than refused. Small models narrate,
/// and rejecting a good call because it came with an apology in front of it is a
/// repair loop over nothing.
pub fn parse_block(content: &str) -> Result<Vec<Call>> {
    let mut calls = Vec::new();
    let mut rest = content;
    let open = format!("```{FENCE}");

    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find("```") else {
            return Err(Error::refused(
                "tool block",
                format!("a ```{FENCE} block was opened and never closed"),
            ));
        };
        calls.push(parse_lines(&after[..end])?);
        rest = &after[end + 3..];
    }

    if calls.is_empty() && content.contains("```") {
        // A fence, but not ours: the model reached for markdown or JSON. Worth
        // a repair, because it is one instruction away from correct.
        return Err(Error::refused(
            "tool block",
            format!("there is a fenced block but it is not ```{FENCE}"),
        ));
    }
    Ok(calls)
}

fn parse_lines(body: &str) -> Result<Call> {
    let bad = |reason: String| Error::refused("tool block", reason);
    let mut tool: Option<Tool> = None;
    let mut args: Vec<(String, String)> = Vec::new();
    let mut requirement = None;

    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(bad(format!("`{line}` is not `key: value`")));
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "tool" => tool = Some(Tool::parse(value)?),
            "requirement" => requirement = Some(value.to_string()),
            _ => args.push((key.to_string(), value.to_string())),
        }
    }

    let tool = tool.ok_or_else(|| bad("the block has no `tool:` line".into()))?;
    let mut call = Call { tool, args, requirement: None };
    if let Some(id) = requirement {
        call = call.for_requirement(id);
    }
    Ok(call)
}

fn call_from(name: &str, args: &Value) -> Result<Call> {
    let tool = Tool::parse(name)?;
    let mut call = Call::new(tool);
    if let Value::Obj(fields) = args {
        for (key, value) in fields {
            let text = match value {
                Value::Str(s) => s.clone(),
                Value::Num(n) => n.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Null => String::new(),
                other => json::to_string(other),
            };
            if key == "requirement" {
                call = call.for_requirement(text);
            } else {
                call = call.arg(key.clone(), text);
            }
        }
    }
    Ok(call)
}

/// Models fence JSON even when told not to. Unwrapping it costs one function
/// and saves a repair round-trip.
fn strip_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else { return trimmed };
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    rest.trim_start_matches(['\r', '\n']).trim_end().trim_end_matches("```").trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::{Capabilities, Source};

    fn caps(native: bool, schema: bool) -> Capabilities {
        Capabilities {
            native_tool_calls: native,
            json_schema: schema,
            streaming: false,
            vision: false,
            embeddings: false,
            context_length: Some(8192),
            reasoning_channel: false,
            prefix_cache: false,
            responses: false,
            source: Source::Observed,
        }
    }

    #[test]
    fn the_ladder_starts_at_the_best_rung_the_link_has() {
        assert_eq!(Ladder::for_link(&caps(true, true)).rung(), Rung::Native);
        assert_eq!(Ladder::for_link(&caps(false, true)).rung(), Rung::JsonSchema);
        assert_eq!(Ladder::for_link(&caps(false, false)).rung(), Rung::Prompted);
        assert_eq!(
            *rungs_for(&caps(true, true)).last().expect("a ladder always has a bottom"),
            Rung::Prompted,
        );
    }

    #[test]
    fn the_loop_completes_on_the_bottom_rung() {
        // The whole requirement in one test: a model with no tool support and
        // no constrained output still drives the harness.
        let mut ladder = Ladder::for_link(&caps(false, false));
        let reply = "I will look at the file first.\n\n\
                     ```perp-call\n\
                     tool: read\n\
                     path: src/main.rs\n\
                     requirement: M-8\n\
                     ```\n";
        let Next::Calls(calls) = ladder.feed(reply, None) else {
            panic!("the bottom rung must parse");
        };
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, Tool::Read);
        assert_eq!(calls[0].get("path"), Some("src/main.rs"));
        assert_eq!(calls[0].requirement.as_deref(), Some("M-8"));
    }

    #[test]
    fn two_repairs_then_the_step_fails_with_the_real_error() {
        let mut ladder = Ladder::at(Rung::Prompted);
        let junk = "```\nsome yaml, probably\n```";

        let Next::Repair { attempt, complaint, .. } = ladder.feed(junk, None) else {
            panic!("first failure is a repair");
        };
        assert_eq!(attempt, 1);
        assert!(complaint.contains("perp-call"), "the complaint is actionable: {complaint}");

        let Next::Repair { attempt, .. } = ladder.feed(junk, None) else {
            panic!("second failure is a repair");
        };
        assert_eq!(attempt, 2);

        let Next::Failed { reason } = ladder.feed(junk, None) else {
            panic!("the third must fail, not repair forever");
        };
        assert!(reason.contains("perp-call"), "carries the parse error, not a substitute: {reason}");
    }

    #[test]
    fn a_rung_the_link_rejects_costs_no_repair() {
        let mut ladder = Ladder::at(Rung::Native);
        let Next::Dropped { from, to, .. } = ladder.unsupported("400: unknown parameter `tools`")
        else {
            panic!("an unsupported rung drops immediately");
        };
        assert_eq!((from, to), (Rung::Native, Rung::JsonSchema));
        assert_eq!(ladder.repairs_used(), 0, "the model did not fail — the link did");
    }

    #[test]
    fn failing_at_a_rung_walks_down_rather_than_giving_up() {
        let mut ladder = Ladder::at(Rung::JsonSchema);
        let junk = "sorry, I cannot do that";
        for _ in 0..MAX_REPAIRS {
            assert!(matches!(ladder.feed(junk, None), Next::Repair { .. }));
        }
        let Next::Dropped { to, .. } = ladder.feed(junk, None) else {
            panic!("out of repairs at a rung means down a rung");
        };
        assert_eq!(to, Rung::Prompted);
        assert_eq!(ladder.repairs_used(), 0, "the budget resets for the new format");
        assert_eq!(ladder.history().len(), 3, "and every attempt is on the record");
    }

    #[test]
    fn native_calls_are_read_off_the_response() {
        let body = r#"{"choices":[{"message":{"role":"assistant","tool_calls":[
            {"id":"c1","type":"function","function":{"name":"patch",
             "arguments":"{\"path\":\"a.rs\",\"expect\":\"x\",\"replace\":\"y\"}"}}]}}]}"#;
        let calls = parse_native(body).expect("parse");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, Tool::Patch);
        assert_eq!(calls[0].get("expect"), Some("x"));
    }

    #[test]
    fn an_answer_with_no_tool_call_is_not_a_parse_failure() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"the file is fine"}}]}"#;
        assert!(parse_native(body).expect("parse").is_empty());
        assert!(parse_block("just some prose").expect("parse").is_empty());
    }

    #[test]
    fn broken_arguments_inside_a_native_call_are_repairable_not_silent() {
        let body = r#"{"choices":[{"message":{"tool_calls":[
            {"function":{"name":"read","arguments":"{\"path\": "}}]}}]}"#;
        let err = parse_native(body).expect_err("half a JSON object is not a call");
        assert!(format!("{err}").contains("read"), "names the call: {err}");
    }

    #[test]
    fn the_constrained_rung_tolerates_a_fence_around_its_json() {
        let content = "```json\n{\"calls\":[{\"tool\":\"glob\",\"args\":{\"pattern\":\"**/*.rs\"}}]}\n```";
        let calls = parse_json_object(content).expect("parse");
        assert_eq!(calls[0].tool, Tool::Glob);
        assert_eq!(calls[0].get("pattern"), Some("**/*.rs"));
    }

    #[test]
    fn several_blocks_in_one_reply_are_several_calls() {
        let reply = "```perp-call\ntool: read\npath: a\n```\nthen\n```perp-call\ntool: read\npath: b\n```";
        let calls = parse_block(reply).expect("parse");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].get("path"), Some("b"));
    }

    #[test]
    fn an_unclosed_block_is_refused_rather_than_guessed_at() {
        let err = parse_block("```perp-call\ntool: shell\ncommand: rm -rf /").expect_err("refuse");
        assert!(format!("{err}").contains("never closed"), "{err}");
    }

    #[test]
    fn an_unknown_tool_name_is_refused_at_every_rung() {
        assert!(parse_block("```perp-call\ntool: deploy\n```").is_err());
        assert!(parse_json_object("{\"calls\":[{\"tool\":\"deploy\"}]}").is_err());
    }

    #[test]
    fn each_rung_asks_for_the_format_it_can_parse() {
        let prompted = Rung::Prompted.instructions();
        assert!(prompted.contains(FENCE), "the instruction names the fence it parses");
        assert!(parse_block(&prompted).is_ok(), "and the example it gives is parseable");
    }
}
