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

/// What opens a value that spans lines.
///
/// The block format is one `key: value` per line, which cannot express a file.
/// Asked to write one anyway, a model does the only thing left and escapes the
/// newlines — and the escapes were written through verbatim, so nine files came
/// out as a single line each and the project stopped building. The model worked
/// out what was happening and left a `_escape_test.dart` behind containing
/// `line one\nline two`, which is how it was found.
///
/// Unescaping the value instead would be the smaller change and the wrong one:
/// source code is full of legitimate `\n` inside string literals, and there is
/// no way to tell one the model meant from one it escaped. A marker has no such
/// ambiguity — the text between is nobody's business but the file's.
pub const HEREDOC: &str = "<<";

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
                "Reply with one JSON object: {\"calls\":[{\"tool\":\"read\",\"args\":{\"path\":\"…\"}}]}
                 When you are finished, reply with {\"calls\":[]} and put your answer in                  \"summary\"."
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
                 One key per line, `key: value`. One block per call. Blocks may repeat.\n\
                 \n\
                 A value that spans lines — a file you are writing — opens with \
                 `{HEREDOC}` and a marker of your choosing, and runs to a line that is \
                 just that marker. Everything between is taken exactly as typed, so \
                 write real newlines and do not escape them:\n\
                 \n\
                 ```{FENCE}\n\
                 tool: write\n\
                 path: src/hello.rs\n\
                 content: {HEREDOC}EOF\n\
                 fn main() {{\n\
                 \x20   println!(\"hi\");\n\
                 }}\n\
                 EOF\n\
                 ```"
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
    // A well-formed object with no `calls` is the model saying it is finished
    // and answering in prose. Refusing it cost four wasted turns and real money
    // the first time this ran against DeepSeek: the model replied `{"answer":97}`
    // — it *had* the answer — and the harness said "that is not a call" and made
    // it try again. A model that emitted a JSON object understood the format; it
    // just had nothing left to call.
    let Some(entries) = parsed.get("calls").and_then(Value::as_arr) else {
        return Ok(Vec::new());
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

/// How deep the brackets are after reading `line`, starting from `depth`.
///
/// Only `[` and `{` count, and only outside a string — `"a]b"` closes nothing.
/// A backslash escapes whatever follows it, so `"\\""` is a quote in a string
/// rather than the end of one.
fn bracket_depth(line: &str, depth: i32) -> i32 {
    let mut depth = depth;
    let mut in_string = false;
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '[' | '{' if !in_string => depth += 1,
            ']' | '}' if !in_string => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn parse_lines(body: &str) -> Result<Call> {
    let bad = |reason: String| Error::refused("tool block", reason);
    let mut tool: Option<Tool> = None;
    let mut args: Vec<(String, String)> = Vec::new();
    let mut requirement = None;

    let mut lines = body.lines();
    while let Some(raw) = lines.next() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(bad(format!("`{line}` is not `key: value`")));
        };
        let (key, value) = (key.trim(), value.trim());

        // A value that opens a JSON bracket runs until the brackets balance.
        //
        // `apply`'s `edits` is a JSON array, and a model handed "a JSON array"
        // pretty-prints one — which every line-oriented reading of this format
        // refuses on the second line, with `` `]` is not `key: value` ``. The
        // heredoc below could carry it and nothing told the model to reach for
        // one, so the natural output was rejected and the natural repair was to
        // produce the same shape again.
        //
        // Measured on Janitor: `J-13` and `J-17` each burned two attempts and
        // sixteen turns apiece. The model had composed the *correct* edit both
        // times; only the framing was refused. The same call written on one
        // line had worked in an earlier batch, so this failed intermittently on
        // formatting rather than on anything about the work.
        //
        // Counting brackets, not parsing JSON: this crate has no JSON reader
        // for arbitrary text (`N-11`), and the question here is only where the
        // value ends. Quotes are tracked so a bracket inside a string does not
        // close the value, and a backslash escapes the next character.
        if value.starts_with('[') || value.starts_with('{') {
            let mut collected = vec![value.to_string()];
            let mut depth = bracket_depth(value, 0);
            while depth > 0 {
                let Some(next) = lines.next() else {
                    return Err(bad(format!(
                        "`{key}` opened a bracket that never closed — a JSON value must \
                         balance, or use `{HEREDOC}END`"
                    )));
                };
                depth = bracket_depth(next, depth);
                collected.push(next.trim().to_string());
            }
            let text = collected.join("");
            match key {
                "tool" => tool = Some(Tool::parse(text.trim())?),
                "requirement" => requirement = Some(text.trim().to_string()),
                _ => args.push((key.to_string(), text)),
            }
            continue;
        }

        // `key: <<END` takes everything up to a line that is just `END`, kept
        // exactly as written. See [`HEREDOC`].
        if let Some(marker) = value.strip_prefix(HEREDOC) {
            let marker = marker.trim();
            if marker.is_empty() {
                return Err(bad(format!("`{key}: {HEREDOC}` has no end marker")));
            }
            let mut collected: Vec<&str> = Vec::new();
            let mut closed = false;
            for line in lines.by_ref() {
                if line.trim() == marker {
                    closed = true;
                    break;
                }
                collected.push(line);
            }
            if !closed {
                return Err(bad(format!(
                    "`{key}` opened with `{HEREDOC}{marker}` and never reached a line saying `{marker}`"
                )));
            }
            let text = collected.join("\n");
            match key {
                "tool" => tool = Some(Tool::parse(text.trim())?),
                "requirement" => requirement = Some(text.trim().to_string()),
                _ => args.push((key.to_string(), text)),
            }
            continue;
        }

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

    /// A pretty-printed JSON value is a value, not a parse error.
    ///
    /// `apply`'s `edits` is a JSON array, and a model told "a JSON array"
    /// pretty-prints one. Every line after the first then failed the
    /// `key: value` reading with `` `]` is not `key: value` ``, so the natural
    /// output was refused and the natural repair produced the same shape again.
    ///
    /// Janitor's `J-13` and `J-17` each burned two attempts and sixteen turns
    /// on this. The model had composed the correct edit both times; only the
    /// framing was rejected — and the same call written on one line had worked
    /// in an earlier batch, so it failed on formatting rather than on anything
    /// about the work.
    #[test]
    fn a_json_value_may_be_pretty_printed_across_lines() {
        let reply = "```perp-call\n\
                     tool: apply\n\
                     path: src/scan.rs\n\
                     edits: [\n\
                       {\"expect\": \"rule.id\", \"replace\": \"rule.id()\"},\n\
                       {\"expect\": \"rule.mode\", \"replace\": \"rule.mode()\"}\n\
                     ]\n\
                     ```";
        let calls = parse_block(reply).expect("a pretty-printed array is a value");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool, crate::tool::Tool::Apply);
        assert_eq!(calls[0].get("path"), Some("src/scan.rs"));

        let edits = calls[0].get("edits").expect("the edits survived");
        assert!(edits.starts_with('[') && edits.ends_with(']'), "{edits}");
        assert!(edits.contains("rule.id()"), "{edits}");
        assert!(edits.contains("rule.mode()"), "{edits}");
        // One line, so whatever reads it next sees a plain JSON array.
        assert!(!edits.contains('\n'), "the value is joined: {edits}");
    }

    /// A bracket inside a string is text, not structure — otherwise an edit
    /// that replaces `]` would end the value early.
    #[test]
    fn a_bracket_inside_a_string_does_not_close_the_value() {
        let reply = "```perp-call\n\
                     tool: apply\n\
                     path: f.rs\n\
                     edits: [\n\
                       {\"expect\": \"a[0]\", \"replace\": \"a.first()\"}\n\
                     ]\n\
                     ```";
        let calls = parse_block(reply).expect("brackets in strings are text");
        assert!(calls[0].get("edits").expect("edits").contains("a[0]"));
    }

    /// And one that never closes says so, rather than swallowing the rest of
    /// the block.
    #[test]
    fn an_unclosed_bracket_is_refused_with_the_reason() {
        let reply = "```perp-call\n\
                     tool: apply\n\
                     path: f.rs\n\
                     edits: [\n\
                       {\"expect\": \"x\"}\n\
                     ```";
        let err = parse_block(reply).expect_err("an unbalanced value is not a value");
        let text = format!("{err}");
        assert!(text.contains("never closed"), "{text}");
    }

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

    /// The defect this exists for. A file has newlines in it; a format of one
    /// `key: value` per line has nowhere to put them.
    #[test]
    fn a_value_that_spans_lines_survives_with_its_newlines() {
        // Joined rather than one literal: a `\` line continuation in Rust eats
        // the indentation that this test is about.
        let block = [
            "```perp-call",
            "tool: write",
            "path: lib/main.dart",
            "content: <<EOF",
            "void main() {",
            "  runApp(const App());",
            "}",
            "EOF",
            "```",
        ]
        .join("\n");
        let calls = parse_block(&block).expect("parses");
        assert_eq!(calls.len(), 1, "{calls:?}");
        let content = calls[0]
            .args
            .iter()
            .find(|(key, _)| key == "content")
            .map(|(_, value)| value.as_str())
            .expect("a content argument");
        assert_eq!(content, "void main() {\n  runApp(const App());\n}", "{content:?}");
        assert_eq!(content.lines().count(), 3, "three lines, not one");
    }

    /// Indentation is a file's own business. Trimming it would reformat every
    /// Python file the loop ever writes into one that does not run.
    #[test]
    fn the_lines_of_a_spanning_value_are_not_trimmed() {
        let block = [
            "```perp-call",
            "tool: write",
            "path: a.py",
            "content: <<END",
            "def f():",
            "    return 1",
            "END",
            "```",
        ]
        .join("\n");
        let calls = parse_block(&block).expect("parses");
        let (_, content) = calls[0].args.iter().find(|(k, _)| k == "content").expect("content");
        assert!(content.contains("\n    return 1"), "the indent is kept: {content:?}");
    }

    /// An unterminated one is a mistake worth naming, not a file that quietly
    /// swallows the rest of the reply.
    #[test]
    fn a_spanning_value_that_never_closes_is_refused_by_name() {
        let block = "```perp-call\n\
                     tool: write\n\
                     path: a.txt\n\
                     content: <<EOF\n\
                     one\n\
                     two\n\
                     ```";
        let failed = parse_block(block).expect_err("an unclosed value is not a call");
        let said = format!("{failed}");
        assert!(said.contains("EOF"), "it names the marker it wanted: {said}");
    }

    /// The escaping a model falls back to when the format gives it no choice.
    /// This is what the nine broken files looked like, and it must not be what
    /// gets written now.
    #[test]
    fn the_single_line_form_still_works_for_values_that_have_no_newlines() {
        let block = "```perp-call\ntool: read\npath: src/main.rs\n```";
        let calls = parse_block(block).expect("parses");
        assert_eq!(calls[0].args, vec![("path".to_string(), "src/main.rs".to_string())]);
    }

    /// The prompt has to say the form exists, or no model will use it.
    #[test]
    fn the_prompted_rung_explains_how_to_write_a_file() {
        let said = Rung::Prompted.instructions();
        assert!(said.contains(HEREDOC), "it shows the marker: {said}");
        assert!(said.contains("do not escape them"), "and says why: {said}");
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
    fn a_json_object_with_no_calls_is_a_finished_answer() {
        // Found by running the loop against DeepSeek. The model replied
        // `{"answer":97}` — it had the answer — and the harness said "that is
        // not a call" and made it try again, four times, for real money. A
        // model that emitted a JSON object understood the format; it just had
        // nothing left to call.
        assert!(parse_json_object(r#"{"answer":97}"#).expect("parse").is_empty());
        assert!(parse_json_object(r#"{"calls":[]}"#).expect("parse").is_empty());

        // And genuinely broken JSON is still a parse failure.
        assert!(parse_json_object("{not json").is_err());
    }

    #[test]
    fn the_constrained_rung_says_how_to_finish() {
        // The instruction the previous test's failure was caused by not having.
        let instructions = Rung::JsonSchema.instructions();
        assert!(instructions.contains(r#"{"calls":[]}"#), "{instructions}");
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
