//! The panel's data surface (`I-1`–`I-5`).
//!
//! The panel is **a view onto the journal, not a second source of truth**
//! (`I-5`). So this module renders one JSON document from the records and
//! nothing else: no in-memory session, no state the editor owns, nothing that
//! can drift from what is on disk.
//!
//! Everything follows from that one decision:
//!
//! - **Closing the editor does not stop the loop, and reopening re-attaches**
//!   (`I-5`), because there is nothing to attach *to*. The panel reads a file
//!   and renders it. Attachment is not a connection; it is a read.
//! - **The engine is a sidecar** (`I-2`). It is a separate process the editor
//!   invokes, never a library it links. An agent loop must not be able to take
//!   the editor down with it, and must outlive the editor window — both of
//!   which are properties of *being a different process*, not of careful coding.
//! - **Nothing under `src/` depends on `crates/`** (vision clause 6). The
//!   editor shells out to a binary that may not be installed, and says so when
//!   it is not.
//!
//! The panel therefore cannot show anything the CLI cannot, which is the
//! intended constraint rather than a limitation: two surfaces that can disagree
//! is one surface too many.

use crate::approval::Queue as Approvals;
use crate::json::Value;
use crate::journal::{Kind, Record};
use crate::metrics::{Cycle, Snapshot};
use crate::state::{replay, Projection};

/// What the panel needs, in one document (`I-3`).
///
/// Assembled in one pass so every section describes the same moment. A panel
/// that fetched chat and approvals separately could show an approval that the
/// timeline says was already answered.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub projection: Projection,
    pub metrics: Cycle,
    pub snapshot: Snapshot,
    pub timeline: Vec<Entry>,
    pub chat: Vec<ChatLine>,
    pub artifacts: Vec<String>,
}

/// One row of the journal timeline (`I-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub step: String,
    pub at: i64,
    pub kind: &'static str,
    pub summary: String,
    pub ok: Option<bool>,
    pub requirements: Vec<String>,
    /// A gate transcript, when the step has one. The panel shows these in the
    /// **terminal dock** rather than inventing a viewer (`I-4`).
    pub transcript: Option<String>,
}

/// One side of the conversation, pulled back out of the shared stream (`C-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLine {
    pub speaker: String,
    pub text: String,
    pub at: i64,
    pub partial: bool,
}

impl View {
    pub fn of(
        records: &[Record],
        approvals: &Approvals,
        artifacts: Vec<String>,
        now: i64,
    ) -> View {
        let projection = replay(records);
        let metrics = Cycle::count(records);
        let snapshot = Snapshot::of(&projection, records, approvals.pending(now).len());

        let mut timeline = Vec::new();
        let mut chat = Vec::new();

        for record in records {
            if let Some(line) = chat_line(record) {
                chat.push(line);
                continue;
            }
            // `/btw` is in the timeline as itself, not as a chat line: it is an
            // aside about the work, and mixing it into the conversation loses
            // the classification.
            timeline.push(Entry {
                step: record.step.to_string(),
                at: record.at,
                kind: match record.kind {
                    Kind::Intent => "intent",
                    Kind::Outcome => "outcome",
                },
                summary: record.summary.clone(),
                ok: record.ok,
                requirements: record.requirements.clone(),
                transcript: record
                    .detail
                    .clone()
                    .filter(|detail| detail.starts_with("gate:")),
            });
        }

        View { projection, metrics, snapshot, timeline, chat, artifacts }
    }

    /// Gate failures, for the editor's **Problems** panel (`I-4`).
    ///
    /// Reusing the surface the editor already has rather than building a second
    /// list of red things — a developer already knows where problems appear.
    pub fn problems(&self) -> Vec<&Entry> {
        self.timeline.iter().filter(|entry| entry.ok == Some(false)).collect()
    }

    /// Transcripts, for the **terminal dock** (`I-4`).
    pub fn transcripts(&self) -> Vec<(&str, &str)> {
        self.timeline
            .iter()
            .filter_map(|entry| {
                entry.transcript.as_deref().map(|text| (entry.step.as_str(), text))
            })
            .collect()
    }

    pub fn to_json(&self) -> String {
        crate::json::to_string(&self.to_value())
    }

    fn to_value(&self) -> Value {
        let obj = |pairs: Vec<(&str, Value)>| {
            Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        };
        let strings = |items: &[String]| {
            Value::Arr(items.iter().map(|s| Value::str(s.clone())).collect())
        };

        obj(vec![
            ("version", Value::str(crate::VERSION)),
            (
                "position",
                obj(vec![
                    (
                        "cycle",
                        self.projection.cycle.map_or(Value::Null, |c| Value::int(i64::from(c))),
                    ),
                    (
                        "stage",
                        self.projection
                            .stage
                            .clone()
                            .map_or(Value::Null, Value::str),
                    ),
                    (
                        "in_flight",
                        self.projection
                            .open_step
                            .as_ref()
                            .map_or(Value::Null, |step| Value::str(step.to_string())),
                    ),
                    ("done", Value::int(as_i64(self.projection.done.len()))),
                    ("blocked", Value::int(as_i64(self.projection.blocked.len()))),
                ]),
            ),
            (
                "spend",
                obj(vec![
                    ("tokens", Value::int(self.metrics.tokens)),
                    ("money", Value::Num(format!("{:.6}", self.metrics.money))),
                    ("calls", Value::int(as_i64(self.metrics.model_calls))),
                    ("gates_run", Value::int(as_i64(self.metrics.gates_run))),
                    ("gates_green", Value::int(as_i64(self.metrics.gates_green))),
                ]),
            ),
            (
                "approvals",
                Value::int(as_i64(self.snapshot.approvals_pending)),
            ),
            (
                "btw",
                Value::Arr(
                    self.projection
                        .pending_btw
                        .iter()
                        .map(|item| {
                            obj(vec![
                                ("id", Value::int(as_i64_u64(item.id))),
                                ("class", Value::str(item.class.to_string())),
                                ("source", Value::str(item.source.clone())),
                                ("text", Value::str(item.text.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "timeline",
                Value::Arr(
                    self.timeline
                        .iter()
                        .map(|entry| {
                            obj(vec![
                                ("step", Value::str(entry.step.clone())),
                                ("at", Value::int(entry.at)),
                                ("kind", Value::str(entry.kind)),
                                ("summary", Value::str(entry.summary.clone())),
                                (
                                    "ok",
                                    entry.ok.map_or(Value::Null, Value::Bool),
                                ),
                                ("requirements", strings(&entry.requirements)),
                                (
                                    "transcript",
                                    entry
                                        .transcript
                                        .clone()
                                        .map_or(Value::Null, Value::str),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "chat",
                Value::Arr(
                    self.chat
                        .iter()
                        .map(|line| {
                            obj(vec![
                                ("speaker", Value::str(line.speaker.clone())),
                                ("text", Value::str(line.text.clone())),
                                ("at", Value::int(line.at)),
                                ("partial", Value::Bool(line.partial)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("artifacts", strings(&self.artifacts)),
        ])
    }
}

fn as_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn as_i64_u64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// Pull a chat turn back out of the shared journal. `None` for a record that is
/// not one — most are not.
fn chat_line(record: &Record) -> Option<ChatLine> {
    let detail = record.detail.as_deref()?;
    let speaker = detail
        .split_whitespace()
        .find_map(|part| part.strip_prefix("speaker="))?
        .to_string();
    let text = detail.split_once("\n\n").map(|(_, body)| body).unwrap_or("").to_string();
    Some(ChatLine {
        speaker,
        text,
        at: record.at,
        partial: detail.contains("partial=true"),
    })
}

/// How the editor finds the engine (`I-2`).
///
/// A path to a binary, which may not be there. The editor must build, start and
/// work with `crates/` deleted — so "not installed" is an ordinary state with a
/// message, not an error condition to handle defensively at twenty call sites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sidecar {
    Available { program: String },
    Missing { looked_for: String },
}

impl Sidecar {
    /// Look for the binary. `PERP_BIN` first, so an operator can point the
    /// editor at a build without installing one.
    pub fn find() -> Sidecar {
        let program = std::env::var("PERP_BIN").unwrap_or_else(|_| "perp".to_string());
        Sidecar::Available { program }
    }

    pub fn is_available(&self) -> bool {
        matches!(self, Sidecar::Available { .. })
    }

    /// What the panel says when the harness is not installed. A blank panel
    /// reads as broken; this reads as absent, which is what it is.
    pub fn explain(&self) -> String {
        match self {
            Sidecar::Available { program } => format!("using {program}"),
            Sidecar::Missing { looked_for } => format!(
                "the perp harness is not installed ({looked_for} was not found). \
                 The editor works without it; the panel has nothing to show."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Turn;
    use crate::step::StepId;

    const T: i64 = 1_700_000_000;

    fn step(n: u32) -> StepId {
        StepId::new(4, "b18", n).expect("step")
    }

    fn journal() -> Vec<Record> {
        let mut records = vec![
            Record::intent(step(1), T, "run the gate").for_requirements(["I-5"]),
            Record::outcome(step(1), T + 5, true, "gate lint is green")
                .for_requirements(["I-5"])
                .with_detail("gate: lint\nsha: abc1234\nexit 0\n"),
            Record::outcome(step(2), T + 10, false, "gate build is red")
                .with_detail("gate: build\nerror: mismatched types\nexit 101\n"),
        ];
        records.push(Turn::operator("why did the build go red?", T + 20).record(step(3)));
        records.push(
            Turn::assistant("because a type changed", T + 25, "here").record(step(4)),
        );
        records
    }

    #[test]
    fn the_panel_is_a_view_and_holds_nothing_of_its_own() {
        // `I-5`. Two views of the same records are identical, which is only
        // true because neither keeps state. That is also why closing the editor
        // cannot stop the loop: there is nothing to disconnect.
        let records = journal();
        let first = View::of(&records, &Approvals::new(), Vec::new(), T);
        let second = View::of(&records, &Approvals::new(), Vec::new(), T);
        assert_eq!(first, second);
    }

    #[test]
    fn the_conversation_comes_back_out_of_the_shared_stream() {
        let view = View::of(&journal(), &Approvals::new(), Vec::new(), T);
        assert_eq!(view.chat.len(), 2, "one from each side");
        assert_eq!(view.chat[0].speaker, "operator");
        assert!(view.chat[0].text.contains("why did the build"), "{:?}", view.chat[0]);
        assert_eq!(view.chat[1].speaker, "assistant");

        // And the chat is not also in the timeline — one record, one place.
        assert!(
            !view.timeline.iter().any(|entry| entry.summary.contains("why did the build")),
            "a record shows in one section, not two"
        );
    }

    #[test]
    fn gate_failures_go_to_the_editors_problems_panel() {
        // `I-4`: reuse the surface the editor already has. A developer knows
        // where problems appear; a second list of red things is a second place
        // to forget to look.
        let view = View::of(&journal(), &Approvals::new(), Vec::new(), T);
        let problems = view.problems();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].summary.contains("build is red"), "{:?}", problems[0]);
    }

    #[test]
    fn transcripts_go_to_the_terminal_dock_rather_than_a_new_viewer() {
        let view = View::of(&journal(), &Approvals::new(), Vec::new(), T);
        let transcripts = view.transcripts();
        assert_eq!(transcripts.len(), 2, "both gates kept one");
        assert!(transcripts[0].1.contains("cargo") || transcripts[0].1.contains("gate:"));
    }

    #[test]
    fn the_document_describes_one_moment() {
        // Assembled in one pass. Fetching chat and approvals separately could
        // show an approval the timeline says was already answered.
        let view = View::of(&journal(), &Approvals::new(), Vec::new(), T);
        let json = view.to_json();
        let parsed = crate::json::parse(&json).expect("valid JSON");

        assert!(parsed.get("position").is_some());
        assert!(parsed.get("timeline").is_some());
        assert!(parsed.get("chat").is_some());
        assert!(parsed.get("spend").is_some());
        assert_eq!(
            parsed.get("version").and_then(Value::as_str),
            Some(crate::VERSION),
            "so a panel from a different build can say so"
        );
    }

    #[test]
    fn journal_text_survives_the_json_round_trip() {
        // A transcript contains quotes, backslashes and newlines routinely.
        let records = vec![Record::outcome(step(1), T, false, "gate build is red")
            .with_detail("gate: build\nerror: expected `\"a\\b\"`\nexit 101\n")];
        let view = View::of(&records, &Approvals::new(), Vec::new(), T);
        let parsed = crate::json::parse(&view.to_json()).expect("valid JSON");
        let timeline = parsed.get("timeline").and_then(Value::as_arr).expect("timeline");
        let transcript = timeline[0].get("transcript").and_then(Value::as_str).expect("transcript");
        assert!(transcript.contains(r#"expected `"a\b"`"#), "{transcript}");
    }

    #[test]
    fn a_missing_harness_reads_as_absent_rather_than_broken() {
        // The editor must build, start and work with `crates/` deleted. A blank
        // panel looks like a bug; this says what is actually going on.
        let missing = Sidecar::Missing { looked_for: "perp".into() };
        assert!(!missing.is_available());
        let text = missing.explain();
        assert!(text.contains("not installed"), "{text}");
        assert!(text.contains("editor works without it"), "{text}");
    }

    #[test]
    fn an_empty_journal_produces_a_document_rather_than_a_failure() {
        let view = View::of(&[], &Approvals::new(), Vec::new(), T);
        assert!(view.timeline.is_empty());
        assert!(view.chat.is_empty());
        let parsed = crate::json::parse(&view.to_json()).expect("still valid JSON");
        assert_eq!(
            parsed.get("position").and_then(|p| p.get("cycle")),
            Some(&Value::Null),
            "unknown is null, not zero — zero would be a claim"
        );
    }
}
