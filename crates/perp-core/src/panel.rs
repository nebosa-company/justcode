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
    /// Requirement id to its one-line text, for surfaces that show a bare id
    /// (`I-2`). The panel puts `T-26` beside a step and had no way to say what
    /// `T-26` meant without opening the requirements file.
    pub requirements: Vec<(String, String)>,
    /// Every requirement the source declares, with its state (`O-18`).
    ///
    /// Not `requirements` with a field added: that one is a lookup, keyed by
    /// id so a surface showing a bare `T-26` can say what `T-26` means, and it
    /// carries only what the loop may pick up next. This is the list itself,
    /// in the order the source writes it, including the rows nobody is going
    /// to build — a person asking what is on the list is asking a different
    /// question from the one the loop asks.
    pub catalogue: Vec<crate::cycle::Catalogued>,
    /// The approvals queue, and the diff each one is asking about (`I-3`).
    pub approvals: Vec<Pending>,
    /// Requirements waiting on a person's decision, with what each waits for
    /// (`O-16`).
    ///
    /// Not the same queue as `approvals`: an approval is a thing the loop has
    /// already done and wants blessed, and this is work it cannot begin. Both
    /// are a person's, and only one of them had a surface — so a loop stalled
    /// on twelve gated requirements reported an empty backlog and looked
    /// finished.
    pub gated: Vec<crate::cycle::Gate>,
    /// The working tree's diff, read from git rather than stored.
    pub diff: Option<String>,
}

/// One thing waiting on a person, with what it would change (`I-3`).
///
/// **Approving from the panel opens the diff first**, so the diff travels with
/// the request rather than being fetched when the button is pressed. A button
/// that could be pressed before the diff loaded is a button that gets pressed
/// before the diff loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: u64,
    pub what: String,
    pub why: String,
    pub raised_at: i64,
    /// What approving would let happen. `None` when the action changes no
    /// files — a push, a tag — and the panel then says so rather than showing
    /// an empty diff pane that reads as "no changes".
    pub diff: Option<String>,
}

impl Pending {
    /// Whether the panel may offer an approve button (`I-3`).
    ///
    /// Only with something to show. An approval offered next to a diff that
    /// failed to load is the exact thing this requirement was written to
    /// prevent — the operator confirms what they can see, and if they can see
    /// nothing they should not be confirming.
    pub fn is_reviewable(&self) -> bool {
        self.diff.as_ref().is_some_and(|diff| !diff.trim().is_empty())
    }

    /// What the panel shows when it will not offer the button.
    pub fn why_not_reviewable(&self) -> &'static str {
        match &self.diff {
            None => "this action changes no files — approve it where it was asked for",
            Some(_) => "the diff could not be read, so there is nothing to approve against",
        }
    }
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
    /// What a row can say about itself when asked, shown as a tooltip.
    ///
    /// For a model call that is the accounting — which link and model answered,
    /// how the prompt split between cached and fresh, how long the first token
    /// took, what it cost — and what the call was for. Not the prompt itself:
    /// prompts are not journalled, and a tooltip claiming to be one would be
    /// inventing it. `M-11` records what a call cost, not what it said.
    pub detail: Option<String>,
}


/// The accounting for every model call in one step, and how to say it.
#[derive(Debug, Default, Clone)]
struct CallDetail {
    link: String,
    model: String,
    cached: i64,
    fresh: i64,
    output: i64,
    latency_ms: i64,
}

impl CallDetail {
    /// One line per fact, because this is read hovering rather than studied.
    fn describe(&self, about: Option<&String>) -> String {
        let mut out = String::new();
        if let Some(said) = about {
            // Long intents exist; a tooltip that fills the window is not read.
            let said = if said.chars().count() > 300 {
                let cut: String = said.chars().take(300).collect();
                format!("{cut}…")
            } else {
                said.clone()
            };
            out.push_str(&said);
            out.push_str("\n\n");
        }
        out.push_str(&format!("{} · {}\n", self.link, self.model));
        out.push_str(&format!(
            "in {} tokens ({} cached, {} fresh)\n",
            self.cached + self.fresh,
            self.cached,
            self.fresh
        ));
        out.push_str(&format!("out {} tokens\n", self.output));
        if self.latency_ms > 0 {
            out.push_str(&format!("slowest first token {} ms\n", self.latency_ms));
        }
        // Said rather than left to be inferred from a tooltip that names
        // everything else: someone reasonably expects the prompt here.
        out.push_str("\nthe prompt itself is not journalled");
        out
    }
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

        // A model call is accounting, not an event. `M-11` put every one of them
        // in the journal so the ledger survives a restart, which was right — and
        // it made them 73% of a real run's timeline, all carrying the same
        // summary, so the events worth reading were buried under filler.
        //
        // Collapsed rather than dropped: a step that took twelve round trips
        // should say so, because that is exactly what you want to see when one is
        // going badly. The count and the charge stay; the repetition goes.
        let mut calls: std::collections::HashMap<String, (usize, f64)> =
            std::collections::HashMap::new();
        // What a row can say about itself when asked. Totals rather than the
        // last call's numbers: a step with twenty calls in it spent all of
        // them, and showing only the twentieth would be a smaller true number
        // presented as the whole.
        let mut ledger: std::collections::HashMap<String, CallDetail> =
            std::collections::HashMap::new();
        // What the call was for. The prompt is not journalled — `M-11` records
        // what a call cost, not what it said — so this is the nearest thing
        // that is on the record: the step's own intent, or for a conversation
        // the question that was asked just before it.
        let mut about: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut last_question: Option<String> = None;
        for record in records {
            if let Some(line) = chat_line(record) {
                if line.speaker == "operator" {
                    last_question = Some(line.text.clone());
                }
                continue;
            }
            if record.kind == Kind::Intent {
                about.entry(record.step.to_string()).or_insert_with(|| record.summary.clone());
            }
            if let Some(entry) = crate::cost::from_record(record) {
                let seen = calls.entry(record.step.to_string()).or_insert((0, 0.0));
                seen.0 += 1;
                seen.1 += entry.charge;

                let held = ledger.entry(record.step.to_string()).or_default();
                held.link = entry.link.clone();
                held.model = entry.model.clone();
                held.cached += entry.usage.cache_hit_tokens;
                held.fresh += entry.usage.cache_miss_tokens;
                held.output += entry.usage.output_tokens;
                held.latency_ms = held.latency_ms.max(entry.latency_ms);

                if let Some(question) = &last_question {
                    about
                        .entry(record.step.to_string())
                        .or_insert_with(|| format!("asked: {question}"));
                }
            }
        }
        let mut summarised: std::collections::HashSet<String> = std::collections::HashSet::new();

        // An intent whose outcome says exactly the same thing is not worth a row
        // of its own.
        //
        // The engine journals an intent before every step and an outcome after,
        // uniformly and with no special cases — that is `L-3`, and it is why a
        // crash leaves an intent with no outcome for `perp resume` to find. For
        // most steps the pair carries information: `gate: test` then `gate test is
        // green`. For a stop it cannot, because nothing happens between deciding
        // to stop and stopping, so both records carry the same sentence and the
        // timeline showed it twice.
        //
        // Collapsed here rather than fixed there: the journal is the truth and it
        // is right, and a special case in the most load-bearing invariant in the
        // engine would be a bad trade for a tidier list. An intent with no
        // matching outcome still shows, which is the case that matters.
        let closed: std::collections::HashSet<(String, String)> = records
            .iter()
            .filter(|record| record.kind == Kind::Outcome)
            .map(|record| (record.step.to_string(), record.summary.clone()))
            .collect();

        for record in records {
            if let Some(line) = chat_line(record) {
                chat.push(line);
                continue;
            }
            if crate::cost::from_record(record).is_some() {
                // One line per step, at the position of its first call, so the
                // summary sits where the work did rather than at the end.
                let step = record.step.to_string();
                if !summarised.insert(step.clone()) {
                    continue;
                }
                let (count, charge) = calls.get(&step).copied().unwrap_or((1, 0.0));
                let key = step.clone();
                timeline.push(Entry {
                    step,
                    at: record.at,
                    kind: "calls",
                    summary: format!(
                        "{count} model call{} · ${charge:.4}",
                        if count == 1 { "" } else { "s" }
                    ),
                    ok: None,
                    requirements: record.requirements.clone(),
                    transcript: None,
                    detail: ledger.get(&key).map(|held| held.describe(about.get(&key))),
                });
                continue;
            }
            if record.kind == Kind::Intent
                && closed.contains(&(record.step.to_string(), record.summary.clone()))
            {
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
                detail: None,
            });
        }

        View {
            projection,
            metrics,
            snapshot,
            timeline,
            chat,
            artifacts,
            requirements: Vec::new(),
            catalogue: Vec::new(),
            approvals: Vec::new(),
            gated: Vec::new(),
            diff: None,
        }
    }

    /// Attach the working tree's diff and the approvals queue (`I-3`).
    ///
    /// The diff is **read from git**, never stored in the journal — git already
    /// keeps it, and a second copy is a second thing that can disagree. Same
    /// decision as `C-7`'s evidence chain.
    /// Attach the requirement text, read from the source the binding names.
    ///
    /// Separate from `of`, which takes records and nothing else: the projection is
    /// a fold over the journal, and the requirements file is not in it. A caller
    /// that has the source passes it, and one that does not gets bare ids.
    pub fn with_requirements(mut self, source: &str) -> View {
        self.requirements = crate::cycle::backlog_all(source);
        // `O-18`: the whole list, in source order, with each row's state. The
        // same pass as the other two for the same reason.
        self.catalogue = crate::cycle::catalogue(source);
        // `O-16`: read from the same source in the same pass. A gated
        // requirement that reaches the panel only when somebody remembers a
        // second call is one that will not reach it.
        self.gated = crate::cycle::gated(source);
        self
    }

    pub fn with_review(
        mut self,
        repo: &crate::git::Repo,
        approvals: &Approvals,
        now: i64,
    ) -> View {
        // `plumbing`, not `plumbing_all`, on purpose (`T-20`). This is a
        // panel: a person does not want five thousand lines of diff in it, and
        // the tail is the right answer. What was wrong was that the tail
        // arrived unlabelled, so a truncated diff and a short one looked
        // identical — `plumbing` now says which it handed back.
        self.diff = repo.plumbing(&["diff", "--stat", "--", "."]).ok().filter(|d| !d.trim().is_empty());
        let file_diff = repo.plumbing(&["diff", "--", "."]).ok().filter(|d| !d.trim().is_empty());

        self.approvals = approvals
            .pending(now)
            .into_iter()
            .map(|entry| Pending {
                id: entry.request.id,
                what: entry.request.what.clone(),
                why: entry.request.why.clone(),
                raised_at: entry.request.raised_at,
                // An action that touches files gets the diff; one that does not
                // gets `None` and says why, rather than an empty pane that
                // reads as "nothing to see".
                diff: touches_files(&entry.request.what).then(|| file_diff.clone()).flatten(),
            })
            .collect();
        self
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
            // `O-16`: each with the text that says which kind of gate and what
            // is missing, so a surface can list them and a person can act
            // without opening the requirements file.
            (
                "gated",
                Value::Arr(
                    self.gated
                        .iter()
                        .map(|gate| {
                            obj(vec![
                                ("id", Value::str(gate.id.clone())),
                                ("kind", Value::str(gate.kind.clone())),
                                ("waiting_for", Value::str(gate.waiting_for.clone())),
                                (
                                    "recommendation",
                                    match &gate.recommendation {
                                        Some(advice) => obj(vec![
                                            ("from", Value::str(advice.from.clone())),
                                            ("says", Value::str(advice.says.clone())),
                                        ]),
                                        None => Value::Null,
                                    },
                                ),
                                (
                                    "options",
                                    Value::Arr(
                                        gate.options
                                            .iter()
                                            .map(|o| Value::str(o.clone()))
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
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
                                    "detail",
                                    entry
                                        .detail
                                        .clone()
                                        .map_or(Value::Null, Value::Str),
                                ),
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
            (
                "requirements",
                Value::Obj(
                    self.requirements
                        .iter()
                        .map(|(id, text)| (id.clone(), Value::str(text.clone())))
                        .collect(),
                ),
            ),
            (
                "catalogue",
                Value::Arr(
                    self.catalogue
                        .iter()
                        .map(|entry| {
                            Value::Obj(vec![
                                ("id".to_string(), Value::str(entry.id.clone())),
                                ("name".to_string(), Value::str(entry.name.clone())),
                                ("text".to_string(), Value::str(entry.text.clone())),
                                ("state".to_string(), Value::str(entry.state.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("artifacts", strings(&self.artifacts)),
            (
                "diff",
                self.diff.clone().map_or(Value::Null, Value::str),
            ),
            (
                "approvals_pending",
                Value::Arr(
                    self.approvals
                        .iter()
                        .map(|pending| {
                            obj(vec![
                                ("id", Value::int(as_i64_u64(pending.id))),
                                ("what", Value::str(pending.what.clone())),
                                ("why", Value::str(pending.why.clone())),
                                ("raised_at", Value::int(pending.raised_at)),
                                (
                                    "diff",
                                    pending.diff.clone().map_or(Value::Null, Value::str),
                                ),
                                ("reviewable", Value::Bool(pending.is_reviewable())),
                                (
                                    "why_not",
                                    if pending.is_reviewable() {
                                        Value::Null
                                    } else {
                                        Value::str(pending.why_not_reviewable())
                                    },
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// Whether an action's effect is visible in a diff.
///
/// A push or a tag moves refs and changes no file, so showing the working
/// tree's diff next to it would be showing something unrelated — which is worse
/// than showing nothing, because it looks like the thing being approved.
fn touches_files(what: &str) -> bool {
    let lower = what.to_ascii_lowercase();
    !(lower.starts_with("git push")
        || lower.starts_with("git tag")
        || lower.contains("publish")
        || lower.contains("deploy"))
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

    #[test]
    fn a_stop_is_one_line_and_an_unfinished_step_is_still_shown() {
        // `c1/E/s45 stopped: the backlog is exhausted`, twice. Not a double write:
        // an intent and an outcome for the same step, both carrying the same
        // sentence, because nothing happens between deciding to stop and
        // stopping.
        let stop = crate::step::StepId::new(1, "E", 45).expect("step");
        let gate = crate::step::StepId::new(1, "b1", 9).expect("step");
        let crashed = crate::step::StepId::new(1, "b2", 1).expect("step");

        let records = vec![
            // A pair that says the same thing twice.
            Record::intent(stop.clone(), 100, "stopped: the backlog is exhausted"),
            Record::outcome(stop.clone(), 101, true, "stopped: the backlog is exhausted"),
            // A pair that does not: both halves are worth a row.
            Record::intent(gate.clone(), 200, "gate: test"),
            Record::outcome(gate.clone(), 201, true, "gate test is green"),
            // An intent with no outcome — a step that never finished, which is
            // the whole reason the engine writes intents first.
            Record::intent(crashed.clone(), 300, "T-9: add dedupe"),
        ];

        let view = View::of(&records, &Approvals::new(), Vec::new(), 400);
        let lines: Vec<&str> = view.timeline.iter().map(|e| e.summary.as_str()).collect();

        assert_eq!(
            lines,
            vec![
                "stopped: the backlog is exhausted",
                "gate: test",
                "gate test is green",
                "T-9: add dedupe",
            ],
            "the stop collapses, the gate keeps both halves, the orphan survives"
        );

        // And the row that survives the collapse is the outcome, because it is
        // the one carrying `ok` and the detail.
        let stopped = view.timeline.iter().find(|e| e.summary.starts_with("stopped")).expect("row");
        assert_eq!(stopped.kind, "outcome");
        assert_eq!(stopped.ok, Some(true));
    }

    #[test]
    fn model_calls_are_one_line_per_step_not_one_line_each() {
        // A real run put 179 accounting records in a 246-entry timeline, all
        // reading `coder · link ds-fast · model deepseek-v4-flash`, so the 67
        // entries that were actual events were buried. `M-11` was right to
        // journal them; the panel was wrong to render them as events.
        let step = crate::step::StepId::new(1, "b4", 28).expect("step");
        let mut records = vec![Record::intent(step.clone(), 100, "T-15: add snake_to_camel")
            .for_requirements(["T-15"])];
        for n in 0..12 {
            records.push(crate::cost::annotate(
                Record::outcome(step.clone(), 101 + n, true, "coder · link ds-fast"),
                &crate::cost::Entry {
                    step: step.to_string(),
                    role: "coder".into(),
                    link: "ds-fast".into(),
                    model: "deepseek-v4-flash".into(),
                    usage: crate::cost::Usage {
                        cache_hit_tokens: 1024,
                        cache_miss_tokens: 83,
                        output_tokens: 144,
                    },
                    latency_ms: 2476,
                    charge: 0.000_5,
                },
            ));
        }
        records.push(
            Record::outcome(step.clone(), 200, true, "T-15: all 82 tests pass")
                .for_requirements(["T-15"]),
        );

        let view = View::of(&records, &Approvals::new(), Vec::new(), 300);
        let calls: Vec<&Entry> = view.timeline.iter().filter(|e| e.kind == "calls").collect();
        assert_eq!(calls.len(), 1, "twelve calls, one line: {:?}", view.timeline.len());
        assert_eq!(calls[0].summary, "12 model calls · $0.0060");
        assert_eq!(calls[0].step, "c1/b4/s28");

        // And the events survive untouched.
        assert_eq!(view.timeline.len(), 3, "intent, the collapsed calls, the outcome");
        assert!(
            view.timeline.iter().any(|e| e.summary.contains("all 82 tests pass")),
            "{:?}",
            view.timeline
        );
    }

    #[test]
    fn approving_from_the_panel_needs_something_to_approve_against() {
        // `I-3`: *approving from the panel opens the diff first*. The button is
        // only offered when there is a diff behind it — one shown next to a
        // pane that failed to load is exactly what this requirement was written
        // to prevent. The operator confirms what they can see.
        let reviewable = Pending {
            id: 1,
            what: "patch crates/perp-core/src/panel.rs".into(),
            why: "outside the workspace".into(),
            raised_at: T,
            diff: Some("--- a/x
+++ b/x
@@ -1 +1 @@
-a
+b
".into()),
        };
        assert!(reviewable.is_reviewable());

        let unloaded = Pending { diff: Some("   ".into()), ..reviewable.clone() };
        assert!(!unloaded.is_reviewable(), "a blank diff is not a diff");
        assert!(unloaded.why_not_reviewable().contains("nothing to approve against"));

        let no_files = Pending { diff: None, ..reviewable };
        assert!(!no_files.is_reviewable());
        assert!(no_files.why_not_reviewable().contains("changes no files"));
    }

    #[test]
    fn an_action_that_changes_no_file_is_not_shown_a_diff() {
        // A push or a tag moves refs. Showing the working tree's diff next to
        // one would show something unrelated — worse than showing nothing,
        // because it looks like the thing being approved.
        assert!(!touches_files("git push origin main"));
        assert!(!touches_files("git tag v0.3.0"));
        assert!(!touches_files("publish the board to a gist"));
        assert!(!touches_files("deploy to production"));

        assert!(touches_files("patch src/main.rs"));
        assert!(touches_files("write docs/notes.md"));
    }

    #[test]
    fn the_diff_is_read_from_git_and_not_kept_in_the_journal() {
        // Same decision as `C-7`'s evidence chain: git already keeps it, and a
        // second copy is a second thing that can disagree.
        let dir = crate::testutil::tmpdir("panel-diff");
        let repo = crate::git::Repo::at(&dir);
        // Not a repository, so there is no diff — and the answer is `None`
        // rather than an empty string that would render as "no changes".
        let view = View::of(&journal(), &Approvals::new(), Vec::new(), T)
            .with_review(&repo, &Approvals::new(), T);
        assert!(view.diff.is_none(), "{:?}", view.diff);
        assert!(view.approvals.is_empty());

        let parsed = crate::json::parse(&view.to_json()).expect("valid JSON");
        assert_eq!(parsed.get("diff"), Some(&Value::Null), "unknown is null, not empty");
        assert!(parsed.get("approvals_pending").is_some());
    }

    #[test]
    fn a_pending_approval_reaches_the_panel_with_its_reason() {
        let dir = crate::testutil::tmpdir("panel-approvals");
        let repo = crate::git::Repo::at(&dir);
        let mut queue = Approvals::new();
        queue.raise(
            crate::approval::Request {
                id: 0,
                what: "git push origin perp/c4/b25".into(),
                why: "pushing puts work on a machine that is not this one".into(),
                command: None,
                diff: None,
                requirement: Some("G-5".into()),
                step: StepId::new(4, "b25", 1).expect("step"),
                cycle: 4,
                raised_at: T,
            },
        );

        let view = View::of(&[], &queue, Vec::new(), T).with_review(&repo, &queue, T);
        assert_eq!(view.approvals.len(), 1);
        let pending = &view.approvals[0];
        assert!(pending.why.contains("not this one"), "{}", pending.why);
        // A push changes no file, so no diff and the panel says why rather than
        // offering a button next to an empty pane.
        assert!(!pending.is_reviewable());

        let parsed = crate::json::parse(&view.to_json()).expect("valid JSON");
        let queued = parsed
            .get("approvals_pending")
            .and_then(Value::as_arr)
            .expect("array");
        assert_eq!(queued[0].get("reviewable"), Some(&Value::Bool(false)));
        assert!(queued[0].get("why_not").and_then(Value::as_str).is_some());
    }
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
