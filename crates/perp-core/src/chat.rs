//! Conversation (`C-1`–`C-5`).
//!
//! **One binary, two modes.** Chat is not a second application with its own
//! rules: same tools, same permission classifier, same journal. The alternative
//! — a chat mode that talks to the model directly and a loop mode that goes
//! through the harness — is how a project ends up with two permission models,
//! one of which is the one nobody audited.
//!
//! Three things follow from that, and they are the whole module:
//!
//! - **Chat writes only when the loop is not** (`C-3`). While the loop holds the
//!   write lock, conversation answers from the journal, the state file and read
//!   tools. A write needs a pause, or a target outside the feature in flight.
//! - **An interrupted response is journalled, not discarded** (`C-4`). The half
//!   a model produced before someone hit escape is evidence of what it was
//!   about to do, and it is exactly the half that is interesting when the answer
//!   was going wrong.
//! - **The conversation is in the same stream as the loop** (`C-5`), interleaved
//!   by time. "Why did it do that in cycle 3" is one journal to read, not two to
//!   correlate.

use std::path::Path;

use crate::error::{Error, Result};
use crate::journal::Record;
use crate::lock::ChatMode;
use crate::step::StepId;

/// Which mode the binary is in (`C-1`). The modes differ in what they are for,
/// not in what they are allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Conversation,
    Loop,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Conversation => "chat",
            Mode::Loop => "loop",
        }
    }
}

/// Who said something. Recorded on every turn so the journal reads back as a
/// conversation rather than as a list of strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Operator,
    Assistant,
}

impl Speaker {
    pub fn as_str(self) -> &'static str {
        match self {
            Speaker::Operator => "operator",
            Speaker::Assistant => "assistant",
        }
    }
}

/// One thing said, on its way to the journal (`C-5`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub speaker: Speaker,
    pub text: String,
    pub at: i64,
    /// True when the operator interrupted mid-generation (`C-4`).
    pub partial: bool,
    /// The link that produced it, for an assistant turn (`M-10`).
    pub link: Option<String>,
}

impl Turn {
    pub fn operator(text: impl Into<String>, at: i64) -> Turn {
        Turn { speaker: Speaker::Operator, text: text.into(), at, partial: false, link: None }
    }

    pub fn assistant(text: impl Into<String>, at: i64, link: impl Into<String>) -> Turn {
        Turn {
            speaker: Speaker::Assistant,
            text: text.into(),
            at,
            partial: false,
            link: Some(link.into()),
        }
    }

    /// The same record with the interrupted flag set. Kept as a constructor
    /// rather than a mutation so an interruption cannot be lost by forgetting
    /// to set it.
    pub fn interrupted(mut self) -> Turn {
        self.partial = true;
        self
    }

    /// Into the loop's own journal, not a second one (`C-5`).
    pub fn record(&self, step: StepId) -> Record {
        let mut detail = format!("speaker={}", self.speaker.as_str());
        if let Some(link) = &self.link {
            detail.push_str(&format!(" link={link}"));
        }
        if self.partial {
            detail.push_str(" partial=true");
        }
        let summary = match (self.speaker, self.partial) {
            (Speaker::Operator, _) => format!("chat: {}", first_line(&self.text)),
            (Speaker::Assistant, false) => format!("chat reply: {}", first_line(&self.text)),
            (Speaker::Assistant, true) => {
                format!("chat reply (interrupted): {}", first_line(&self.text))
            }
        };
        // The text goes in the detail verbatim. The summary is for scanning; a
        // journal that only kept the summary would be a journal that answered
        // "what was said" with a paraphrase.
        Record::outcome(step, self.at, true, summary)
            .with_detail(format!("{detail}\n\n{}", self.text))
    }
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.chars().count() <= 72 {
        return line.to_string();
    }
    let cut: String = line.chars().take(69).collect();
    format!("{cut}...")
}

/// A response as it arrives (`C-4`).
///
/// The accumulated text is kept as it streams so an interruption at any point
/// has something to journal. Discarding it would throw away the only record of
/// what the model was in the middle of doing.
#[derive(Debug, Clone, Default)]
pub struct Streaming {
    text: String,
    chunks: usize,
    interrupted: bool,
}

impl Streaming {
    pub fn new() -> Streaming {
        Streaming::default()
    }

    pub fn push(&mut self, delta: &str) {
        if self.interrupted {
            return;
        }
        self.text.push_str(delta);
        self.chunks += 1;
    }

    /// Stop taking deltas. Idempotent, because an interrupt arriving twice —
    /// two escapes, or an escape and a closed pipe — is normal.
    pub fn interrupt(&mut self) {
        self.interrupted = true;
    }

    pub fn was_interrupted(&self) -> bool {
        self.interrupted
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn chunks(&self) -> usize {
        self.chunks
    }

    /// The turn to journal, whether it finished or not.
    pub fn finish(self, at: i64, link: &str) -> Turn {
        let turn = Turn::assistant(self.text, at, link);
        if self.interrupted {
            turn.interrupted()
        } else {
            turn
        }
    }
}

/// Why a write from chat was refused, or that it was allowed (`C-3`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    Allowed,
    ReadOnly { because: String },
}

impl Access {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Access::Allowed)
    }
}

/// May chat write to `target` right now?
///
/// `feature_paths` is what the loop's current feature is touching. A write
/// outside it is allowed while the loop runs, because it cannot collide — which
/// is the point of tracking it rather than blanket-refusing. A write inside it
/// needs a pause.
pub fn may_write(mode: ChatMode, target: &Path, feature_paths: &[&Path]) -> Access {
    if mode == ChatMode::Interactive {
        return Access::Allowed;
    }
    let clashes = feature_paths.iter().any(|owned| target.starts_with(owned));
    if clashes {
        Access::ReadOnly {
            because: format!(
                "the loop is writing {} — pause it, or edit something outside the feature (`C-3`)",
                target.display()
            ),
        }
    } else {
        Access::Allowed
    }
}

/// Something the conversation decided should be built (`C-2`).
///
/// It does not become work by being agreed to. It becomes work by being a
/// requirement in the requirements source, with an id, like everything else —
/// otherwise a conversation quietly grows a second backlog that no prioritisation
/// ever sees and no status marker ever covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub text: String,
    /// Set once it is in the requirements source.
    pub filed_as: Option<String>,
}

impl Proposal {
    pub fn new(text: impl Into<String>) -> Proposal {
        Proposal { text: text.into(), filed_as: None }
    }

    pub fn filed(mut self, id: impl Into<String>) -> Proposal {
        self.filed_as = Some(id.into());
        self
    }

    /// The check the engine runs before building anything a conversation
    /// produced (`C-2`).
    pub fn ready_to_build(&self) -> Result<&str> {
        self.filed_as.as_deref().ok_or_else(|| {
            Error::refused(
                "building from a conversation",
                "this has no requirement id. File it in the requirements source first (`C-2`, \
                 `V-9`) — chat does not get its own backlog",
            )
        })
    }
}

/// The next id for a prefix, given the ids already defined. Filing a proposal
/// mints one, which is legal in exactly one place — the requirements source
/// (`V-9`) — and this is how that place decides what number is next.
pub fn next_id(prefix: char, existing: &[String]) -> String {
    let highest = existing
        .iter()
        .filter_map(|id| {
            let (p, n) = id.split_once('-')?;
            (p.eq_ignore_ascii_case(&prefix.to_string())).then(|| n.parse::<u32>().ok())?
        })
        .max()
        .unwrap_or(0);
    format!("{}-{}", prefix.to_ascii_uppercase(), highest + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const T: i64 = 1_700_000_000;

    fn step() -> StepId {
        StepId::new(3, "b13", 4).expect("step")
    }

    #[test]
    fn the_conversation_goes_in_the_loops_journal() {
        let record = Turn::operator("why did the build gate go red in cycle 2?", T).record(step());
        assert_eq!(record.step, step(), "the same stream, interleaved by time (`C-5`)");
        let detail = record.detail.expect("detail");
        assert!(detail.contains("speaker=operator"));
        assert!(detail.contains("why did the build gate"), "the text is verbatim, not a paraphrase");
    }

    #[test]
    fn an_interrupted_reply_is_journalled_rather_than_discarded() {
        let mut streaming = Streaming::new();
        streaming.push("I will start by deleting the failing ");
        streaming.push("test so the gate ");
        streaming.interrupt();
        streaming.push("passes.");

        assert!(streaming.was_interrupted());
        assert_eq!(streaming.chunks(), 2, "deltas after the interrupt are not taken");
        let turn = streaming.finish(T, "here");
        assert!(turn.partial);

        let record = turn.record(step());
        assert!(record.summary.contains("interrupted"), "{}", record.summary);
        let detail = record.detail.expect("detail");
        assert!(
            detail.contains("deleting the failing"),
            "the half that was produced is exactly the interesting half:\n{detail}"
        );
        assert!(detail.contains("partial=true"));
    }

    #[test]
    fn interrupting_twice_is_not_an_error() {
        let mut streaming = Streaming::new();
        streaming.push("hello");
        streaming.interrupt();
        streaming.interrupt();
        assert_eq!(streaming.text(), "hello");
    }

    #[test]
    fn chat_is_read_only_where_the_loop_is_working() {
        let owned = PathBuf::from("crates/perp-core/src");
        let inside = PathBuf::from("crates/perp-core/src/engine.rs");
        let outside = PathBuf::from("docs/notes.md");

        let refused = may_write(ChatMode::ReadOnly, &inside, &[owned.as_path()]);
        let Access::ReadOnly { because } = &refused else { panic!("must refuse: {refused:?}") };
        assert!(because.contains("pause"), "and says how to proceed: {because}");

        assert!(
            may_write(ChatMode::ReadOnly, &outside, &[owned.as_path()]).is_allowed(),
            "a write that cannot collide is not blocked (`C-3`)"
        );
        assert!(
            may_write(ChatMode::Interactive, &inside, &[owned.as_path()]).is_allowed(),
            "pausing is the handover"
        );
    }

    #[test]
    fn a_conversation_does_not_get_its_own_backlog() {
        let proposal = Proposal::new("support vertical splits");
        let err = proposal.ready_to_build().expect_err("it is not work until it has an id");
        assert!(format!("{err}").contains("C-2"), "{err}");

        let filed = proposal.filed("I-6");
        assert_eq!(filed.ready_to_build().expect("filed"), "I-6");
    }

    #[test]
    fn filing_continues_the_numbering_rather_than_colliding() {
        let existing: Vec<String> =
            ["I-1", "I-2", "I-5", "C-12", "L-22"].iter().map(|s| s.to_string()).collect();
        assert_eq!(next_id('I', &existing), "I-6");
        assert_eq!(next_id('C', &existing), "C-13");
        assert_eq!(next_id('X', &existing), "X-1", "a prefix with nothing yet starts at one");
    }

    #[test]
    fn both_modes_are_the_same_application() {
        // `C-1` is a claim about structure rather than a function, so what is
        // testable is that the mode is a label the rest of the crate does not
        // branch on for permission. This pins the label; the classifier having
        // no mode parameter at all is what enforces it.
        assert_eq!(Mode::Conversation.as_str(), "chat");
        assert_eq!(Mode::Loop.as_str(), "loop");
        let call = crate::tool::Call::new(crate::tool::Tool::Shell).arg("command", "kubectl apply");
        assert!(
            matches!(crate::tool::classify(&call), crate::approval::Policy::Never { .. }),
            "the same classifier, with nowhere to say which mode is asking"
        );
    }

    #[test]
    fn a_long_first_line_is_shortened_for_the_summary_and_kept_in_full() {
        let long = "a".repeat(200);
        let record = Turn::operator(&long, T).record(step());
        assert!(record.summary.len() < 100, "the summary is scannable");
        assert!(record.summary.ends_with("..."));
        assert!(record.detail.expect("detail").contains(&long), "and the text survives whole");
    }
}

/// What the model is told about the workspace before the question (`M-12`).
///
/// A turn used to carry the message and nothing else, so "what does T-2 ask
/// for?" was answered from thin air — confidently, and wrong. The requirements
/// are the answer to most questions anyone types here, and the harness already
/// has them.
///
/// Assembled in a fixed order and returned as one string, so it can be a stable
/// prompt segment: a provider's prefix cache only pays when the prefix is
/// byte-identical, and this is identical between turns until the workspace
/// itself changes.
///
/// Bounded, because a big backlog would otherwise crowd out the conversation.
/// What is dropped is said so in the text rather than silently — a model told
/// nine of forty requirements should know there are forty.
pub struct Context<'a> {
    pub vision: Option<&'a str>,
    pub requirements: Option<&'a str>,
    pub projection: Option<&'a crate::state::Projection>,
}

/// Roughly four characters to a token, the same estimate the prompt module uses.
const CONTEXT_BUDGET: usize = 12_000;

impl Context<'_> {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "You are answering questions about a software project driven by the Perpetum \
harness. Everything below is what the harness knows about it, taken from the \
project's own files and its journal. Answer from it. If it does not say, say \
that it does not say rather than guessing: a confident wrong answer about a \
requirement is worse here than admitting the workspace does not tell you.\n",
        );

        if let Some(vision) = self.vision.map(str::trim).filter(|text| !text.is_empty()) {
            out.push_str("\n## What the project is\n\n");
            out.push_str(&clamp(vision, 2_000));
            out.push('\n');
        }

        if let Some(text) = self.requirements.map(str::trim).filter(|text| !text.is_empty()) {
            out.push_str("\n## Requirements\n\n");
            out.push_str(&clamp(text, CONTEXT_BUDGET));
            out.push('\n');
        }

        if let Some(projection) = self.projection {
            out.push_str("\n## Where the work is\n\n");
            let cycle = projection.cycle.unwrap_or(1);
            let stage = projection.stage.clone().unwrap_or_else(|| "b1".to_string());
            out.push_str(&format!(
                "cycle {cycle}, stage {stage}: {} steps closed, {} blocked.\n",
                projection.done.len(),
                projection.blocked.len()
            ));
            if let Some(open) = &projection.open_step {
                out.push_str(&format!("step {open} is open and has not closed.\n"));
            }
            // The last handful, newest first. What someone asks about is nearly
            // always what just happened.
            let recent: Vec<&crate::state::Done> = projection.done.iter().rev().take(8).collect();
            if !recent.is_empty() {
                out.push_str("\nmost recently closed:\n");
                for done in recent {
                    out.push_str(&format!("- {} — {}\n", done.step, clamp(&done.summary, 200)));
                }
            }
            for blocked in projection.blocked.iter().take(5) {
                out.push_str(&format!(
                    "- BLOCKED {} — {}\n",
                    blocked.step,
                    clamp(&blocked.summary, 200)
                ));
            }
        }
        out
    }
}

/// Cut to a byte budget on a character boundary, and say that it was cut.
fn clamp(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_string();
    }
    let mut cut = budget;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n[... {} more characters, not shown]", &text[..cut], text.len() - cut)
}

#[cfg(test)]
mod context_tests {
    use super::*;

    #[test]
    fn the_requirements_are_in_it() {
        let context = Context {
            vision: Some("A toolkit of small string functions."),
            requirements: Some("| T-2 | `truncate(text, limit)` shortens text. |"),
            projection: None,
        };
        let rendered = context.render();
        assert!(rendered.contains("truncate(text, limit)"), "{rendered}");
        assert!(rendered.contains("A toolkit"), "{rendered}");
        // The instruction not to invent an answer is the point of the whole
        // thing: without the requirements a model answered anyway.
        assert!(rendered.contains("does not say"), "{rendered}");
    }

    #[test]
    fn what_is_dropped_is_declared() {
        let long = "x".repeat(CONTEXT_BUDGET + 500);
        let context = Context { vision: None, requirements: Some(&long), projection: None };
        let rendered = context.render();
        assert!(rendered.contains("more characters, not shown"), "silent truncation");
        assert!(rendered.len() < long.len(), "and it actually got shorter");
    }

    #[test]
    fn a_multibyte_boundary_is_not_split() {
        // Cutting mid-character would panic on the slice.
        let text = "e\u{0301}".repeat(CONTEXT_BUDGET);
        let context = Context { vision: None, requirements: Some(&text), projection: None };
        let rendered = context.render();
        assert!(rendered.contains("not shown"), "it was cut");
    }

    #[test]
    fn an_empty_workspace_still_renders_the_instruction() {
        let context = Context { vision: None, requirements: None, projection: None };
        let rendered = context.render();
        assert!(rendered.contains("Perpetum"), "{rendered}");
    }
}
