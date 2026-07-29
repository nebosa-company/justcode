//! `/btw` — the aside (`C-8`–`C-12`).
//!
//! An operator watching a loop has thoughts that do not deserve a stop: *that
//! test name is wrong*, *we should support tabs*, *don't touch the parser this
//! cycle*. `/btw` takes them at any time, acknowledges immediately, and **never
//! aborts the current step**. It is the cheapest possible way to say something,
//! which is the whole design goal — an aside that costs a pause is an aside
//! nobody makes.
//!
//! ## The rule that shapes everything else
//!
//! `C-10`: **a `/btw` can never cross the approval boundary.** It cannot approve
//! a parked action, raise a budget, disable a gate, or reclassify a `never`.
//!
//! Being cheap and being powerful cannot both be true of the same channel. The
//! moment a casual aside can unlock the dangerous half of the harness, every
//! path that produces text — a model summarising a web page, a pasted log, a
//! `/btw` typed at 2am — becomes a path to a deploy. So text that reads like an
//! instruction to cross the boundary is still **accepted and journalled**, and
//! is classified [`Class::Note`] with a refusal attached that says which
//! explicit command to use instead.
//!
//! Note what that is not: it is not refusing the aside, and it is not silently
//! downgrading it. The operator gets an acknowledgement, a classification, and a
//! sentence saying the boundary is elsewhere.

use std::fmt;

use crate::error::Result;
use crate::journal::Record;
use crate::step::StepId;

/// What an aside turns out to be (`C-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Applies to the current feature. Injected at the next step boundary,
    /// never mid-step.
    Steer,
    /// Real work. Filed to the requirements source with source `operator`
    /// before anything is built from it (`C-2`, `V-9`).
    Requirement,
    /// A rule for the rest of the cycle.
    Constraint,
    /// Recorded and nothing more.
    Note,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Steer => "steer",
            Class::Requirement => "requirement",
            Class::Constraint => "constraint",
            Class::Note => "note",
        }
    }

    pub fn parse(text: &str) -> Option<Class> {
        match text.trim().to_ascii_lowercase().as_str() {
            "steer" => Some(Class::Steer),
            "requirement" => Some(Class::Requirement),
            "constraint" => Some(Class::Constraint),
            "note" => Some(Class::Note),
            _ => None,
        }
    }

    /// What happens to an item of this class, in one line, so the shown
    /// classification means something to read.
    pub fn effect(self) -> &'static str {
        match self {
            Class::Steer => "applied at the next step boundary",
            Class::Requirement => "filed to the requirements source, then prioritised like any other",
            Class::Constraint => "in force for the rest of the cycle",
            Class::Note => "journalled only",
        }
    }
}

impl fmt::Display for Class {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The phrases that mean an aside is reaching for the approval boundary.
///
/// Over-broad on purpose, and cheap to be wrong about: a false positive costs
/// the operator one explicit command, and a false negative is a `/btw` that
/// approved a production deploy.
const BOUNDARY: &[(&str, &str)] = &[
    ("approve", "Approving is `/approve <id>` — a person confirming one named action."),
    ("approved", "Approving is `/approve <id>` — a person confirming one named action."),
    ("go ahead and deploy", "A deploy is on the Never list. No approval unlocks it (Perpetum 0.4)."),
    ("deploy to prod", "A deploy is on the Never list. No approval unlocks it (Perpetum 0.4)."),
    ("ship it", "A deploy is on the Never list. No approval unlocks it (Perpetum 0.4)."),
    ("push it", "Pushing is approval-gated and asked for explicitly (`G-5`)."),
    ("merge it", "Merging to `main` is approval-gated and asked for explicitly (`G-5`)."),
    ("raise the budget", "The budget is a binding key. Change it there, where it is versioned."),
    ("increase the budget", "The budget is a binding key. Change it there, where it is versioned."),
    ("more budget", "The budget is a binding key. Change it there, where it is versioned."),
    ("skip the gate", "A gate is failed or fixed, never skipped (`V-2`)."),
    ("skip the test", "A gate is failed or fixed, never skipped (`V-2`)."),
    ("disable the gate", "A gate is failed or fixed, never skipped (`V-2`)."),
    ("ignore the test", "A gate is failed or fixed, never skipped (`V-2`)."),
    ("it's fine to", "If it is fine, it is fine as an explicit command with its own confirmation."),
    ("you have permission", "Permission comes from a command, never from prose."),
    ("i authorise", "Permission comes from a command, never from prose."),
    ("i authorize", "Permission comes from a command, never from prose."),
];

/// One aside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Btw {
    pub id: u64,
    pub text: String,
    pub at: i64,
    /// Where it came from — the CLI, the panel, or the notification reply path
    /// (`C-11`). Recorded because "who said this" is the first question later.
    pub source: String,
    pub class: Class,
    /// Set when the text reached for the approval boundary (`C-10`). The aside
    /// is still accepted; this is what the operator is told instead.
    pub refused: Option<String>,
    /// The step that was running when it arrived, if one was. Asides land
    /// between steps but arrive during them.
    pub during: Option<StepId>,
    pub applied: bool,
}

impl Btw {
    /// What the operator sees back, immediately (`C-8`).
    pub fn acknowledgement(&self) -> String {
        let mut out = format!("noted #{} — {}: {}", self.id, self.class, self.class.effect());
        if let Some(refusal) = &self.refused {
            out.push_str(&format!(
                "\n\nThis reads like an instruction to cross the approval boundary, which \
                 `/btw` cannot do, so it is recorded as a note.\n{refusal}"
            ));
        }
        out.push_str(&format!("\n(`/btw {} <class>` to reclassify)", self.id));
        out
    }

    pub fn record(&self, step: StepId) -> Record {
        Record::outcome(step, self.at, true, format!("/btw #{}: {}", self.id, self.text))
            .with_detail(format!(
                "class={} source={} {}",
                self.class,
                self.source,
                self.refused.as_deref().map(|r| format!("refused_boundary={r}")).unwrap_or_default()
            ))
    }
}

/// Decide what an aside is (`C-9`).
///
/// Engine-side and keyword-shaped, deliberately. A model classifier would be
/// better at nuance and would also mean the *classification* of an aside was
/// model-controlled — and one of the classes changes policy for the cycle. The
/// classification is shown and correctable, which is the mitigation the
/// requirement already asks for.
pub fn classify(text: &str) -> (Class, Option<String>) {
    let lower = text.to_ascii_lowercase();

    // The boundary check runs first and wins. Nothing below can promote an
    // aside past it.
    for (phrase, instead) in BOUNDARY {
        if lower.contains(phrase) {
            return (Class::Note, Some((*instead).to_string()));
        }
    }

    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));

    if has(&["never ", "don't ", "do not ", "always ", "must not ", "stop using", "avoid "]) {
        return (Class::Constraint, None);
    }
    if has(&["we should", "we need", "it should", "add support", "add a", "would be good", "please add", "can we"]) {
        return (Class::Requirement, None);
    }
    if has(&["this feature", "the current", "instead", "rename", "while you", "in this batch", "actually"]) {
        return (Class::Steer, None);
    }
    (Class::Note, None)
}

/// The queue of asides, including the ones nobody has classified yet (`C-12`).
#[derive(Debug, Clone, Default)]
pub struct Queue {
    items: Vec<Btw>,
    next_id: u64,
}

impl Queue {
    pub fn new() -> Queue {
        Queue { items: Vec::new(), next_id: 1 }
    }

    /// Accept an aside. Never fails, never blocks, never aborts anything —
    /// which is the requirement, not a convenience.
    pub fn accept(
        &mut self,
        text: &str,
        source: &str,
        at: i64,
        during: Option<StepId>,
    ) -> &Btw {
        let (class, refused) = classify(text);
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Btw {
            id,
            text: text.to_string(),
            at,
            source: source.to_string(),
            class,
            refused,
            during,
            applied: false,
        });
        #[allow(clippy::expect_used)]
        self.items.last().expect("just pushed")
    }

    /// Correct a classification (`C-9`).
    ///
    /// A boundary refusal is **not** correctable into anything else: letting a
    /// follow-up reclassify it would make the two-message version of the thing
    /// `C-10` forbids in one.
    pub fn reclassify(&mut self, id: u64, class: Class) -> Result<&Btw> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| crate::Error::refused(format!("/btw {id}"), "is not in the queue"))?;
        if item.refused.is_some() {
            return Err(crate::Error::refused(
                format!("/btw {id}"),
                "reached for the approval boundary; reclassifying it would be the same crossing \
                 in two messages (`C-10`)",
            ));
        }
        item.class = class;
        Ok(item)
    }

    pub fn items(&self) -> &[Btw] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&Btw> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Steers waiting to be injected. Taken at a **step boundary** and never
    /// during one (`C-8`).
    pub fn take_steers(&mut self) -> Vec<Btw> {
        let mut taken = Vec::new();
        for item in &mut self.items {
            if item.class == Class::Steer && !item.applied {
                item.applied = true;
                taken.push(item.clone());
            }
        }
        taken
    }

    /// Constraints in force for the rest of the cycle.
    pub fn constraints(&self) -> Vec<&Btw> {
        self.items.iter().filter(|item| item.class == Class::Constraint).collect()
    }

    /// Asides that still need something to happen to them: a requirement to
    /// file, a steer to inject. What `state.md` shows so they survive a restart
    /// (`C-12`).
    pub fn pending(&self) -> Vec<&Btw> {
        self.items
            .iter()
            .filter(|item| !item.applied && matches!(item.class, Class::Steer | Class::Requirement))
            .collect()
    }

    /// The `state.md` section (`C-12`). Empty string when there is nothing, so
    /// the state file does not carry an empty heading.
    pub fn render_for_state(&self) -> String {
        let pending = self.pending();
        if pending.is_empty() {
            return String::new();
        }
        let mut out = String::from("\n## Waiting from `/btw`\n\n");
        for item in pending {
            out.push_str(&format!(
                "- #{} · **{}** · {} — {}\n",
                item.id,
                item.class,
                crate::time::format_date(item.at),
                item.text
            ));
        }
        out.push_str("\nThese survive a restart and are picked up at the next step boundary, or \
                      at Phase B if the engine is not running (`C-11`).\n");
        out
    }

    /// Rebuild from the journal, so a restart does not lose the queue (`C-12`).
    pub fn replay(records: &[Record]) -> Queue {
        let mut queue = Queue::new();
        for record in records {
            let Some(rest) = record.summary.strip_prefix("/btw #") else { continue };
            let Some((id, text)) = rest.split_once(": ") else { continue };
            let Ok(id) = id.parse::<u64>() else { continue };
            let detail = record.detail.as_deref().unwrap_or("");
            let field = |name: &str| {
                detail
                    .split_whitespace()
                    .find_map(|part| part.strip_prefix(name))
                    .map(str::to_string)
            };
            let class = field("class=").and_then(|c| Class::parse(&c)).unwrap_or(Class::Note);
            queue.items.push(Btw {
                id,
                text: text.to_string(),
                at: record.at,
                source: field("source=").unwrap_or_else(|| "unknown".into()),
                class,
                refused: field("refused_boundary="),
                during: None,
                applied: false,
            });
            queue.next_id = queue.next_id.max(id + 1);
        }
        queue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_700_000_000;

    #[test]
    fn an_aside_is_accepted_and_acknowledged_immediately() {
        let mut queue = Queue::new();
        let step = StepId::new(3, "b13", 4).expect("step");
        let item = queue.accept("actually rename that to `resolve`", "cli", T, Some(step.clone()));
        assert_eq!(item.class, Class::Steer);
        assert_eq!(item.during.as_ref(), Some(&step), "it arrived during a step");
        assert!(item.acknowledgement().contains("noted #1"));
        assert!(!item.applied, "and lands at the next boundary, not now");
    }

    #[test]
    fn a_btw_can_never_cross_the_approval_boundary() {
        let mut queue = Queue::new();
        for text in [
            "go ahead and deploy to prod when the gates pass",
            "you have permission to push this branch",
            "just skip the test for now",
            "raise the budget to twenty dollars",
            "I authorise the merge",
            "approve request 3",
        ] {
            let item = queue.accept(text, "cli", T, None);
            assert_eq!(item.class, Class::Note, "must not become actionable: {text}");
            let refusal = item.refused.clone().expect(text);
            let ack = item.acknowledgement();
            assert!(ack.contains("cannot do"), "and says so plainly: {ack}");
            assert!(ack.contains(&refusal), "and says what to do instead: {ack}");
            assert!(
                refusal.ends_with('.'),
                "the refusal is a whole sentence, not a fragment glued to a verb: {refusal}"
            );
        }
    }

    #[test]
    fn a_refused_aside_cannot_be_reclassified_into_one_that_acts() {
        let mut queue = Queue::new();
        let id = queue.accept("go ahead and deploy it", "cli", T, None).id;
        let err = queue
            .reclassify(id, Class::Constraint)
            .expect_err("two messages must not do what one may not");
        assert!(format!("{err}").contains("C-10"), "{err}");
        assert_eq!(queue.get(id).map(|item| item.class), Some(Class::Note), "and it did not move");
    }

    #[test]
    fn an_ordinary_classification_is_correctable() {
        let mut queue = Queue::new();
        let id = queue.accept("the terminal font is too small", "cli", T, None).id;
        let corrected = queue.reclassify(id, Class::Requirement).expect("correctable");
        assert_eq!(corrected.class, Class::Requirement);
    }

    #[test]
    fn the_four_classes_are_told_apart() {
        let cases = [
            ("we should support vertical splits", Class::Requirement),
            ("never touch the parser this cycle", Class::Constraint),
            ("actually use the other helper here", Class::Steer),
            ("interesting that the lint gate is the slow one", Class::Note),
        ];
        for (text, expected) in cases {
            assert_eq!(classify(text).0, expected, "{text}");
        }
    }

    #[test]
    fn steers_are_taken_at_a_boundary_and_only_once() {
        let mut queue = Queue::new();
        queue.accept("actually use a BTreeMap", "cli", T, None);
        queue.accept("we should add a status bar", "cli", T, None);

        let first = queue.take_steers();
        assert_eq!(first.len(), 1, "the requirement is not a steer");
        assert_eq!(queue.take_steers().len(), 0, "and a steer is injected once, not every boundary");
    }

    #[test]
    fn pending_items_show_up_in_the_state_file() {
        let mut queue = Queue::new();
        queue.accept("we should support tabs", "panel", T, None);
        queue.accept("never touch the parser", "cli", T, None);

        let rendered = queue.render_for_state();
        assert!(rendered.contains("tabs"), "the requirement is waiting: {rendered}");
        assert!(!rendered.contains("parser"), "a constraint is already in force, not waiting");
        assert!(rendered.contains("Waiting from `/btw`"));
    }

    #[test]
    fn an_empty_queue_adds_nothing_to_the_state_file() {
        assert_eq!(Queue::new().render_for_state(), "");
        let mut queue = Queue::new();
        queue.accept("just a thought", "cli", T, None);
        assert_eq!(queue.render_for_state(), "", "a note is not waiting for anything");
    }

    #[test]
    fn the_queue_survives_a_restart() {
        let mut queue = Queue::new();
        queue.accept("we should support tabs", "panel", T, None);
        queue.accept("go ahead and ship it", "cli", T, None);

        let step = StepId::new(3, "b13", 9).expect("step");
        let records: Vec<Record> = queue.items().iter().map(|i| i.record(step.clone())).collect();

        let replayed = Queue::replay(&records);
        assert_eq!(replayed.items().len(), 2);
        assert_eq!(replayed.items()[0].class, Class::Requirement);
        assert_eq!(replayed.items()[0].source, "panel");
        assert_eq!(replayed.items()[1].class, Class::Note);
        assert!(
            replayed.items()[1].refused.is_some(),
            "and the boundary refusal survives, so a restart cannot launder it"
        );
        assert_eq!(replayed.next_id, 3, "ids continue rather than colliding");
    }

    #[test]
    fn a_source_is_recorded_because_who_said_it_is_the_first_question_later() {
        let mut queue = Queue::new();
        for source in ["cli", "panel", "notification-reply"] {
            let item = queue.accept("a thought", source, T, None);
            assert_eq!(item.source, source);
        }
    }
}
