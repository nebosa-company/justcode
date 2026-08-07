//! Driving a whole cycle, unattended (`L-1`, `L-14`).
//!
//! Everything before this ran *one* thing: a batch of gates, or one requirement
//! against a model. This is the piece that walks the phase machine — picks work
//! off the backlog, does it, gates it, measures whether the phase is over, and
//! moves on — until one of `L-14`'s three conditions stops it or a budget parks
//! it.
//!
//! ## The backlog is the requirements source
//!
//! Not a separate list. A requirement is work if it is in `.harness/perpetum.md`
//! without a done marker, and it stops being work when the marker changes.
//! There is deliberately no second place to look: `C-2` already says a
//! conversation does not get its own backlog, and the same reasoning applies to
//! the loop.
//!
//! ## What it will not do
//!
//! Mark its own work done. The loop can write code, run gates and journal
//! results; **it cannot set a `✅`**. That marker means "implemented, gates
//! green, transcript in the journal" and a loop that awards it to itself is a
//! loop whose status is worth nothing. A person reads the evidence and marks.

use crate::agent::{Agent, Item};
use crate::budget::Spend;
use crate::engine::{Engine, Gates, Report};
use crate::error::Result;
use crate::phase::{Advance, Machine, Measured, Phase, Stop};

/// Requirements still open, read from the requirements source.
///
/// A row is work when it has an id and no status marker. `🟡` is included —
/// in progress means unfinished — and `✅`, `⛔`, `🔶` and `❌` are not: done,
/// externally gated, conflicting and declined are all "not the loop's to pick
/// up".
/// Requirements this cycle has already attempted, from the journal.
///
/// The loop may not mark its own work done (`V-2`), so the requirements source
/// looks identical after a batch as before it. Without this the second batch
/// picks up the first batch's items again, and an hour-long run redoes the same
/// four things fifteen times — which is exactly what a long run is for finding.
///
/// Attempted, not *succeeded*: an item that failed is not retried inside the
/// same cycle either. `L-11`'s watchdog says repeating an unproductive thing is
/// an error rather than a retry, and the next cycle is where a second attempt
/// belongs.
pub fn attempted(records: &[crate::journal::Record], cycle: u32) -> Vec<String> {
    let mut seen = Vec::new();
    for record in records.iter().filter(|r| r.step.cycle == cycle) {
        for id in &record.requirements {
            // No exception for `V-2` any more. It was here because the gate cited
            // `V-2` on every batch, which would have marked the harness's own
            // `V-2` attempted without anyone working on it — and it would now do
            // the opposite harm, keeping real work on `V-2` from ever counting.
            if !seen.contains(id) {
                seen.push(id.clone());
            }
        }
    }
    seen
}

/// The backlog with everything this cycle has already touched removed.
pub fn remaining(source: &str, records: &[crate::journal::Record], cycle: u32, limit: usize) -> Vec<Item> {
    let done = attempted(records, cycle);
    backlog(source, usize::MAX)
        .into_iter()
        .filter(|item| !done.contains(&item.requirement))
        .take(limit)
        .collect()
}

/// Every requirement's marker, in source order (`V-8`).
///
/// The counting half of `V-8` needs to know what each row *is*, not just which
/// rows are open — `backlog` answers "what is left to work on" and throws the
/// marker away getting there, which is why "8 of 8 delivered" could be printed
/// while three of them waited on a human.
///
/// `❌` (won't do) has no [`Marker`] variant and is skipped rather than
/// mapped onto a neighbour: counting a declined requirement as conflicting
/// would be a wrong number where an absent one is honest. No row in this
/// repository carries it today.
pub fn markers(source: &str) -> Vec<crate::verify::Marker> {
    marked(source).into_iter().map(|(_, marker)| marker).collect()
}

/// The same, with each requirement's id — what a check needs in order to say
/// *which* marker it disbelieves (`V-7`).
pub fn marked(source: &str) -> Vec<(String, crate::verify::Marker)> {
    use crate::verify::Marker;
    let mut out = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let first = first.trim();
        // Read before stripping, then confirm what is left is really an id —
        // a table of contents row starting with the same glyph is not a
        // requirement.
        let marker = if first.starts_with('✅') {
            Marker::Done
        } else if first.starts_with('🟡') {
            Marker::InProgress
        } else if first.starts_with('🚧') {
            Marker::Blocked
        } else if first.starts_with('⛔') {
            Marker::Gated
        } else if first.starts_with('🔶') {
            Marker::Conflicting
        } else if first.starts_with('❌') {
            continue;
        } else {
            Marker::Open
        };
        let id = first
            .trim_start_matches(['✅', '🟡', '⛔', '🔶', '❌', '🚧'])
            .replace('~', "")
            .trim()
            .trim_matches('`')
            .trim()
            .to_string();
        if !is_requirement_id(&id) {
            continue;
        }
        out.push((id, marker));
    }
    out
}

/// Every requirement in the source with its text, done or not.
///
/// `backlog` skips anything already marked, because it answers "what is left to
/// work on". This answers "what does this id mean", which has to include the
/// finished ones: a step citing `T-2` is worth explaining long after `T-2` is
/// ticked.
/// Requirements waiting on a person, with what each is waiting for (`O-16`).
///
/// `backlog` drops these silently — correctly, since a loop must not pick work
/// gated on a decision it is not allowed to make. But dropping them is all that
/// happened: nothing carried them anywhere a person looks, so a loop blocked on
/// somebody and a loop that had finished were indistinguishable.
///
/// Measured on Janitor: twelve requirements gated on a GUI toolkit choice — a
/// third-party dependency, which the binding reserves to a person — and the
/// cycle reported `backlog 0 open requirement(s)`, ran three batches and
/// delivered nothing. The twelve were visible only by opening the file.
///
/// The text is returned whole because the gate's reason lives in it: the row
/// says which kind of gate and what is missing, and a summary that dropped that
/// would surface the fact without the thing a person needs to act on it.
/// What a gated requirement is waiting for, pulled apart (`O-17`).
///
/// The prose already carries this and cannot show it: `J-38` states the ask,
/// the argument for it and a prerequisite in one paragraph, and a reader
/// separates them by hand — twelve times, every time they look.
///
/// **The options are the requirement author's**, read from the row rather than
/// invented here. Ungating edits a source only a person may write (`V-12`), and
/// a loop that authored the choices for a decision reserved from it would have
/// reserved nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    pub id: String,
    /// `**Gated: dependency approval**` yields `dependency approval`. Empty
    /// when the row does not say, which is a row worth improving rather than a
    /// reason to refuse to show it.
    pub kind: String,
    /// The row's text, whole. The kind and the options are lifted out of it and
    /// left in it: a reader who wants the argument still needs the paragraph.
    pub waiting_for: String,
    /// `**Options:** approve `windows`; refuse, drop the Windows shell` — semicolons,
    /// because `|` is the markdown table cell separator and would end the row.
    pub options: Vec<String>,
    /// `**Recommended by the loop:** approve — it has read `R8.9``, as the
    /// advice and the name of whoever is giving it (`O-17`).
    ///
    /// **Never pre-selected, always attributed.** A recommendation from the
    /// party that wants to be unblocked has an interest, and a pre-selected
    /// option is a decision taken by whoever drew the dialog rather than by the
    /// person the gate is reserved for. Carrying the source is what lets a
    /// reader weigh it: *the loop recommends approving* and *you wrote that you
    /// would approve* are different sentences and must not render alike.
    pub recommendation: Option<Advice>,
}

/// Advice on a gate, and who is giving it (`O-17`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    /// `the loop`, `R8.9`, a person's name — whatever the row attributes it to.
    /// Never inferred: an unattributed recommendation is not rendered at all,
    /// because advice whose source a reader cannot weigh is worse than none.
    pub from: String,
    pub says: String,
}

/// Lift `**Key:** …` out of a requirement's prose, to the end of its line.
fn gate_field(text: &str, key: &str) -> Option<String> {
    let open = format!("**{key}:");
    let at = text.find(&open)? + open.len();
    let rest = &text[at..];
    let end = rest.find("**").unwrap_or(rest.len());
    // A marker written `**Gated: dependency approval**` closes after the value;
    // one written `**Options:** a | b` closes before it. Both are in use, so
    // take whichever side has the text.
    let inside = rest[..end].trim();
    let value = if inside.is_empty() { rest[end..].trim_start_matches('*').trim() } else { inside };
    let value = value.split(" — ").next().unwrap_or(value);
    let value = value.split(". ").next().unwrap_or(value);
    let value = value.trim().trim_matches('*').trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}

/// The markers a status cell may open with. Listed once because three readers
/// and one writer all have to agree on them, and a marker missing from one of
/// those lists is a row that parses differently depending on who is looking.
pub const MARKERS: [char; 6] = ['✅', '🟡', '⛔', '🔶', '❌', '🚧'];

/// The requirement id in a row's status cell, or `None` when the cell holds
/// something that is not one.
///
/// The cell is `` `L-3` ``, or `✅ ~~`L-3`~~`, or `⛔ `J-38``. Every reader of
/// this file wants the id out of it and none of them wants the marker, so the
/// stripping happens here rather than three times — [`catalogue`], [`gated`]
/// and [`crate::requirement`] were the three, and the fourth would have been
/// the one that got it subtly wrong.
pub fn row_id(cell: &str) -> Option<String> {
    let id = cell
        .trim()
        .trim_start_matches(MARKERS)
        .replace('~', "")
        .trim()
        .trim_matches('`')
        .trim()
        .to_string();
    is_requirement_id(&id).then_some(id)
}

/// What a row's status cell says about the work: `open`, `in progress`,
/// `done`, `blocked`, `gated`, `conflicting` or `won't do`.
///
/// The **cell**, never the line (`V-23`): a requirement whose prose mentions a
/// marker is not in that state by saying so.
pub fn row_state(cell: &str) -> &'static str {
    let cell = cell.trim();
    if cell.starts_with('✅') {
        "done"
    } else if cell.starts_with('🟡') {
        "in progress"
    } else if cell.starts_with('🚧') {
        "blocked"
    } else if cell.starts_with('⛔') {
        "gated"
    } else if cell.starts_with('🔶') {
        "conflicting"
    } else if cell.starts_with('❌') {
        "won't do"
    } else {
        "open"
    }
}

pub fn gated(source: &str) -> Vec<Gate> {
    let mut waiting: Vec<Gate> = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };
        // The status cell, not the line (`V-23`): a requirement whose prose
        // mentions a marker is not gated by saying so.
        if !first.contains('⛔') {
            continue;
        }
        let (Some(id), false) = (row_id(first), text.trim().is_empty()) else { continue };
        let text = text.trim().to_string();
        let options = gate_field(&text, "Options")
            .map(|list| {
                list.split(';').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect()
            })
            .unwrap_or_default();
        // `**Recommended by <who>:** <advice>`. The attribution is part of the
        // key, so a row cannot carry advice without saying whose it is.
        let recommendation = text
            .find("**Recommended by ")
            .and_then(|at| {
                let rest = &text[at + "**Recommended by ".len()..];
                let (from, tail) = rest.split_once(':')?;
                // Taken whole, unlike the other fields: for advice the reason
                // *is* the value, and truncating at the first dash would leave
                // `approve` with the argument for it cut off — which is the
                // half a reader needs in order to disagree.
                let says = tail
                    .trim_start_matches('*')
                    .split("**")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if says.is_empty() {
                    return None;
                }
                Some(Advice { from: from.trim().to_string(), says })
            });
        waiting.push(Gate {
            id,
            kind: gate_field(&text, "Gated").unwrap_or_default(),
            waiting_for: text,
            options,
            recommendation,
        });
    }
    waiting
}

/// One row of the requirements source, as something a person can look at: what
/// it is called, what it says, and what state it is in (`O-18`).
///
/// Every row, whatever its marker. [`marked`] drops `❌` because a check that
/// disbelieves a marker has nothing to say about a requirement nobody is going
/// to build, and [`backlog`] drops everything that is not workable — so between
/// them there was no way to ask the plain question *what is on this project's
/// list*. A surface that shows only what the loop may pick up next tells a
/// person the ones it may not do not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogued {
    pub id: String,
    /// The opening sentence, for a list. Never the whole paragraph.
    pub name: String,
    /// What the row says, in full — truncating here is what `T-6` forbids.
    pub text: String,
    /// `open`, `in progress`, `done`, `blocked`, `gated`, `conflicting` or
    /// `won't do`. A string rather than [`crate::verify::Marker`] because that
    /// enum has no `❌` and gains nothing from one: it exists so a check can
    /// name the marker it disbelieves, and there is no claim to disbelieve in
    /// a row that says the work will not happen.
    pub state: String,
}

/// Every requirement the source declares, in the order it declares them
/// (`O-18`).
pub fn catalogue(source: &str) -> Vec<Catalogued> {
    let mut all: Vec<Catalogued> = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let state = row_state(first);
        let Some(id) = row_id(first) else { continue };
        if all.iter().any(|seen| seen.id == id) {
            continue;
        }
        let text = text.trim().to_string();
        all.push(Catalogued { id, name: opening_sentence(&text), text, state: state.to_string() });
    }
    all
}

/// The first sentence of a requirement, for a row in a list (`O-18`).
///
/// Requirements here have no name field — the id and the paragraph are all
/// there is — so the name is taken rather than stored, and taking it is the
/// honest option: a name kept beside the text is a second thing to update and
/// the one that goes stale.
fn opening_sentence(text: &str) -> String {
    let plain = text.replace("**", "").replace('`', "");
    let end = plain
        .char_indices()
        .find(|(at, c)| {
            *c == '.'
                && plain[at + c.len_utf8()..].chars().next().is_none_or(char::is_whitespace)
        })
        .map(|(at, _)| at);
    let sentence = match end {
        Some(at) => &plain[..at],
        None => &plain,
    };
    let sentence = sentence.trim();
    if sentence.chars().count() <= 96 {
        return sentence.to_string();
    }
    let cut: String = sentence.chars().take(95).collect();
    format!("{}…", cut.trim_end())
}

pub fn backlog_all(source: &str) -> Vec<(String, String)> {
    let mut all: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };
        let id = first
            .trim()
            .trim_start_matches(['✅', '🟡', '⛔', '🔶', '❌', '🚧'])
            .replace('~', "")
            .trim()
            .trim_matches('`')
            .trim()
            .to_string();
        if !is_requirement_id(&id) || text.trim().is_empty() {
            continue;
        }
        let summary: String = text.trim().chars().take(160).collect();
        if !all.iter().any(|(seen, _)| *seen == id) {
            all.push((id, summary));
        }
    }
    all
}

/// What one requirement says, in full, whatever its marker (`V-9`).
///
/// [`backlog_all`] truncates to 160 characters because it feeds a list a
/// person scrolls; [`backlog`] hands a model the whole text. Reaching for the
/// first when a model needed the second gave `perp run --requirement` a
/// requirement cut off mid-sentence — no ellipsis, nothing to say anything was
/// missing, which is exactly the silent truncation `T-6` forbids and worse than
/// saying nothing: a model that is told nothing goes and reads the file, and a
/// model handed a confident-looking fragment does not.
///
/// Marker-blind on purpose. `backlog` skips what is done, and asking to re-run
/// a finished requirement by name is a reasonable thing to want.
pub fn stated(source: &str, id: &str) -> Option<String> {
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };
        let found = first
            .trim()
            .trim_start_matches(['🟡', '✅', '⛔', '🔶', '❌'])
            .trim()
            .trim_matches('~')
            .trim_matches('`')
            .trim_matches('~')
            .trim_matches('`')
            .trim();
        if found == id {
            let text = text.trim();
            return (!text.is_empty()).then(|| text.to_string());
        }
    }
    None
}

pub fn backlog(source: &str, limit: usize) -> Vec<Item> {
    let mut items = Vec::new();
    // Checked before the loop, not inside it: the earlier version pushed an
    // item and *then* noticed the limit, so a limit of zero returned one.
    if limit == 0 {
        return items;
    }
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        // `V-8`: a gated item is never re-picked without its reason changing,
        // and `🚧` is on that list. It was excluded only by accident before —
        // the id parser strips `🟡` and nothing else, so `🚧 \`L-3\`` failed
        // `is_requirement_id` and fell out one line further down. Correct, and
        // for a reason that had nothing to do with the rule it was keeping;
        // anyone teaching the parser to strip markers would have silently put
        // blocked work back in the backlog.
        let mut cells = line.trim_matches('|').split('|');
        let (Some(first), Some(text)) = (cells.next(), cells.next()) else { continue };

        // `V-23`: the marker is read from the status cell, not from anywhere on
        // the line. This was `line.contains`, so a requirement whose *prose*
        // mentioned a marker excluded itself — silently, and from the backlog
        // rather than from the display, so the row simply stopped existing.
        //
        // Measured on Janitor: `J-29`'s text recorded that it had been briefly
        // gated, and writing `⛔` inside that sentence dropped it out of the
        // backlog entirely. It was not offered as an alternative, nothing said
        // why, and two cycles ran past it before anyone noticed the row had
        // gone quiet.
        if first.contains('✅')
            || first.contains('⛔')
            || first.contains('🔶')
            || first.contains('❌')
            || first.contains('🚧')
        {
            continue;
        }

        let id = first.trim().trim_start_matches('🟡').trim().trim_matches('`').trim();
        if !is_requirement_id(id) {
            continue;
        }
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let summary: String = text.chars().take(70).collect();
        if let Ok(item) = Item::new(id, &summary, text) {
            items.push(item);
        }
        if items.len() >= limit {
            break;
        }
    }
    items
}

/// Whether `text` is a requirement id: one uppercase letter, a dash, digits.
///
/// Public because a write has to refuse an id before it goes looking for a row
/// ([`crate::requirement`]), and "looks like an id" is the same question the
/// readers here ask.
pub fn is_requirement_id(text: &str) -> bool {
    let Some((prefix, number)) = text.split_once('-') else { return false };
    prefix.len() == 1
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Two `Work`s run as one (`L-14`).
///
/// A leg is *work then gates*, and it must produce **one** terminal record. The
/// first unattended cycle ran them as two `Engine::run` calls and the journal
/// came back reading `work · stopped · gate · stopped` — two exhausted-backlog
/// records for one leg, with a gate in between them. `L-14` says each stop
/// writes a distinct terminal record; it does not say a leg may write two.
pub struct Then<A: crate::engine::Work, B: crate::engine::Work> {
    first: A,
    second: B,
    on_second: bool,
}

impl<A: crate::engine::Work, B: crate::engine::Work> Then<A, B> {
    pub fn new(first: A, second: B) -> Then<A, B> {
        Then { first, second, on_second: false }
    }

    pub fn second(&self) -> &B {
        &self.second
    }
}

impl<A: crate::engine::Work, B: crate::engine::Work> crate::engine::Work for Then<A, B> {
    fn next(&mut self) -> Option<crate::engine::Task> {
        if !self.on_second {
            if let Some(task) = self.first.next() {
                return Some(task);
            }
            // The first is spent. Not a stop — the second half of the leg has
            // not run yet.
            self.on_second = true;
            // The hand-over is the only moment both halves are in one place and
            // the first has finished: the gate learns which requirements it may
            // stand as evidence for (`V-14`).
            let delivered = self.first.delivered();
            self.second.covers(delivered);
        }
        self.second.next()
    }

    fn perform(&mut self, task: &crate::engine::Task) -> crate::engine::Done {
        if self.on_second {
            self.second.perform(task)
        } else {
            self.first.perform(task)
        }
    }

    fn spend(&self) -> Spend {
        self.first.spend().plus(self.second.spend())
    }

    fn author(&self) -> Option<String> {
        self.first.author().or_else(|| self.second.author())
    }

    fn touched(&self) -> Vec<String> {
        let mut paths = self.first.touched();
        for path in self.second.touched() {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        paths
    }

    fn delivered(&self) -> Vec<String> {
        let mut ids = self.first.delivered();
        for id in self.second.delivered() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    fn covers(&mut self, delivered: Vec<String>) {
        // A `Then` nested in another passes it on to whichever half has not run.
        self.first.covers(delivered.clone());
        self.second.covers(delivered);
    }

    fn drain_records(&mut self) -> Vec<crate::journal::Record> {
        let mut records = self.first.drain_records();
        records.extend(self.second.drain_records());
        records
    }

    fn at_step(&mut self, step: &crate::step::StepId) {
        self.first.at_step(step);
        self.second.at_step(step);
    }

    /// `V-5`: forward the review to whichever half just ran.
    ///
    /// Without this the trait default answers `None`, and a batch is always a
    /// `Then` — so the engine asked the *wrapper* for a verdict, got nothing,
    /// and no verifier ever ran. `Agent::review` was written, tested, and
    /// unreachable through the only path that reaches an agent.
    ///
    /// That is the second time `V-5` has shipped unrun. Its own doc comment
    /// records the first: `verify::independence` tested, the renderer tested,
    /// `Role::Verifier` in the router, and every one of 129 calls made as
    /// `coder`. Implementing `Agent::review` fixed the half that was missing and
    /// left this one. Janitor's first batch: fifteen calls, all `coder`, no
    /// verdict.
    ///
    /// Keyed on `on_second` rather than trying both, so the agent is reviewed
    /// after each of its own steps and the gates — which have nothing to review
    /// — are not asked on its behalf.
    fn review(&mut self) -> Option<String> {
        if self.on_second {
            self.second.review()
        } else {
            self.first.review()
        }
    }
}

/// What one phase of the cycle did.
#[derive(Debug, Clone, PartialEq)]
pub struct Leg {
    pub phase: Phase,
    pub report: Report,
    /// Why the machine did or did not move on.
    pub advanced: String,
}

/// What a whole cycle did.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub cycle: u32,
    pub legs: Vec<Leg>,
    pub stop: Option<Stop>,
    pub parked: Option<String>,
    pub spend: Spend,
    pub ended_in: Phase,
}

impl Outcome {
    pub fn describe(&self) -> String {
        let mut out = format!("cycle {} — {} legs\n", self.cycle, self.legs.len());
        for leg in &self.legs {
            out.push_str(&format!("  {} · {}\n", leg.phase, leg.report.describe()));
            out.push_str(&format!("       {}\n", leg.advanced));
            // `L-29`: what the leg had to say for itself.
            //
            // `Report::warnings` had seven writers and no reader outside a
            // test. A batch that landed its work said so into a `Vec` nobody
            // printed, and — the half that costs something — so did one that
            // could not commit at all: `land_batch` records "gate was green but
            // nothing was committed" with the real error, and the run reported
            // "stopped: the backlog is exhausted" and nothing else. Measured on
            // Janitor, where `J-19` was written, staged, and never committed,
            // and the reason existed the whole time in a field with no reader.
            for warning in &leg.report.warnings {
                out.push_str(&format!("       · {warning}\n"));
            }
        }
        out.push_str(&format!("ended in phase {}\n", self.ended_in));
        match (&self.stop, &self.parked) {
            (Some(stop), _) => out.push_str(&format!("{}\n", stop.summary())),
            (None, Some(why)) => out.push_str(&format!("parked: {why}\n")),
            (None, None) => out.push_str("still running\n"),
        }
        out.push_str(&format!("spent {}\n", self.spend));
        out
    }
}

/// Measure the workspace for the phase machine (`L-2`).
///
/// Every field comes from a file or the journal. Nothing here asks a model, and
/// nothing a model produced reaches it — which is the point of the type.
pub fn measure(engine: &Engine, batches_done: u32) -> Result<Measured> {
    let binding = engine.session().binding();
    let records = engine.session().journal().read_all()?;
    let projection = crate::state::replay(&records);

    let source = binding
        .resolve("path.requirements")
        .ok()
        .map(|p| crate::layout::requirements_text(&p))
        .unwrap_or_default();
    let open = u32::try_from(backlog(&source, usize::MAX).len()).unwrap_or(u32::MAX);

    let gates_green_at_sha = records.iter().rev().take(20).any(|record| {
        record.ok == Some(true)
            && record.detail.as_deref().is_some_and(|d| d.starts_with("gate:") && d.contains("sha:"))
    });

    Ok(Measured::from_workspace(
        true,
        binding.resolve("path.vision").map(|p| p.exists()).unwrap_or(false),
        open,
        binding.resolve("path.batches").map(|p| p.exists()).unwrap_or(false).into(),
        0,
        batches_done,
        binding.root().join(".harness/release-notes.md").exists(),
        gates_green_at_sha,
        u32::from(projection.open_step.is_some()),
        true,
    ))
}

/// Run a cycle until something stops it.
///
/// `batches` bounds Phase D the way Perpetum does — five per cycle by default —
/// and `items_per_batch` bounds how much work one batch attempts. Both exist
/// because an unattended loop with no bound on its own appetite is a loop that
/// spends a budget discovering it had one.
pub struct Driver<'a> {
    pub root: std::path::PathBuf,
    pub links: &'a crate::link::Links,
    pub health: &'a dyn crate::link::Health,
    pub mode: crate::link::Mode,
    pub batches: u32,
    pub items_per_batch: usize,
    pub transport: &'a dyn crate::net::Transport,
}

impl Driver<'_> {
    /// Walk the machine. Returns when `L-14` stops it, a budget parks it, or
    /// Phase D's batch allowance is used up.
    pub fn run(&self, cycle: u32, from: Phase) -> Result<Outcome> {
        let mut machine = Machine::at(cycle, from);
        let mut outcome = Outcome {
            cycle,
            legs: Vec::new(),
            stop: None,
            parked: None,
            spend: Spend::default(),
            ended_in: from,
        };
        let mut batches_done = 0;

        loop {
            outcome.ended_in = machine.phase();
            let phase = machine.phase();

            // Only D has work a machine can do. B, C, E and F are a person's —
            // gathering requirements, prioritising, writing release notes,
            // reconciling — and pretending otherwise would have the loop
            // asserting it had done them.
            let report = if phase == Phase::D {
                batches_done += 1;
                self.batch(cycle, batches_done)?
            } else {
                self.gates_only(cycle, phase)?
            };

            outcome.spend = outcome.spend.plus(report.spend);
            let stop = report.stop.clone();
            let park = report.park.clone();

            let measured = {
                let engine = Engine::open(&self.root)?;
                measure(&engine, batches_done)?
            };
            let advanced = match machine.advance(&measured, self.batches) {
                Ok(Advance::Stay { because, .. }) => format!("stays in {phase}: {because}"),
                Ok(Advance::Moved { from, to }) => format!("{from} → {to}"),
                Ok(Advance::Cycled { cycle }) => format!("F closed, cycle {cycle} opens at B"),
                Err(e) => format!("cannot advance: {e}"),
            };
            let moved = advanced.contains('→') || advanced.contains("opens at B");

            outcome.legs.push(Leg { phase, report, advanced });

            // A leg reporting `BacklogExhausted` means *that leg's* work ran
            // out, not that the cycle has none left. Conflating the two ended
            // the first hour-long run after 99 seconds and three of twenty-four
            // requirements — the driver asked for a batch, got "exhausted", and
            // stopped.
            //
            // The cycle's backlog is the requirements source minus what this
            // cycle has attempted, and that is what `L-14`'s condition means
            // here.
            if let Some(stop) = stop {
                let cycle_done = match Engine::open(&self.root) {
                    Ok(engine) => {
                        let seen = engine.session().journal().read_all().unwrap_or_default();
                        let source = engine
                            .session()
                            .binding()
                            .resolve("path.requirements")
                            .ok()
                            .map(|p| crate::layout::requirements_text(&p))
                            .unwrap_or_default();
                        remaining(&source, &seen, cycle, 1).is_empty()
                    }
                    Err(_) => true,
                };
                if cycle_done || !matches!(stop, Stop::BacklogExhausted) {
                    outcome.stop = Some(stop);
                    break;
                }
            }

            // A failed step means the batch was not delivered. Starting the next
            // one would be piling work on a tree whose last batch did not land,
            // and the gate that then goes red belongs to nobody.
            //
            // What is said about the gate is what the gate said. A step fails on
            // a tool error, an unparseable reply or a refusal just as readily as
            // on a red gate, and this used to report the last of those whatever
            // had happened — on a cycle whose gates both exited 0.
            if let Some(leg) = outcome.legs.last() {
                if leg.report.failed > 0 && leg.report.stop.is_none() {
                    let why = blocked_why(leg.report.failed, leg.report.gates_green);
                    let blocked = Stop::BatchBlocked { batch: format!("b{batches_done}"), why };
                    outcome.stop = Some(blocked);
                    break;
                }
            }
            if let Some(park) = park {
                outcome.parked = Some(park.reason);
                break;
            }
            if phase == Phase::D && batches_done >= self.batches && !moved {
                outcome.parked = Some(format!(
                    "{} batches attempted and Phase D's exit is still not met",
                    self.batches
                ));
                break;
            }
            // A phase that cannot move and has no work left would spin.
            if !moved && phase != Phase::D {
                outcome.parked =
                    Some(format!("{phase} cannot advance and has no work a loop can do"));
                break;
            }
            if machine.phase() == Phase::B && phase == Phase::F {
                break;
            }
        }
        Ok(outcome)
    }

    /// Phase D: pick work off the backlog, do it, then gate it.
    ///
    /// Integrates L-17 container orchestration if enabled: creates a persistent
    /// container for this batch and routes gates through it.
    /// `V-18`: the red run for a batch that added a test (`V-3`).
    ///
    /// The trigger reads the batch's own change. **Untracked files are read
    /// too**, and that is not an optimisation: a new test usually arrives as a
    /// whole new file, which `git diff HEAD` does not mention at all, so a
    /// tracked-only trigger would miss the commonest case it exists for.
    ///
    /// The test gate only. `V-3` is about a test proving it can fail, and a
    /// lint that passes with and without the change is the expected result
    /// rather than a finding — paying for it would double a gate to learn
    /// nothing.
    ///
    /// Every early return is a skip, never a failure: no test gate declared, no
    /// test added, no repository. A batch is not blocked because the harness
    /// could not decide whether to check it.
    fn red_run(
        &self,
        engine: &mut Engine,
        cycle: u32,
        stage: &str,
        report: &Report,
        base: Option<&str>,
    ) -> Result<()> {
        // No base, no red run. Without a commit to compare against there is no
        // "tree without the change", and running it anyway would compare the
        // work with itself — which is the bug this argument exists to fix.
        let Some(base) = base else { return Ok(()) };
        let Ok(gate) = crate::gate::Gate::named(engine.session().binding(), "test") else {
            return Ok(());
        };
        let repo = crate::git::Repo::at(&self.root);

        let mut change = repo.plumbing_all(&["diff", base]).unwrap_or_default();
        // The untracked half. `status --porcelain` marks these `??`; their whole
        // content is new, so every line of it counts as added.
        for line in repo.plumbing_all(&["status", "--porcelain"]).unwrap_or_default().lines() {
            let Some(path) = line.trim().strip_prefix("?? ") else { continue };
            if let Ok(text) = std::fs::read_to_string(self.root.join(path.trim())) {
                for added in text.lines() {
                    change.push('+');
                    change.push_str(added);
                    change.push('\n');
                }
            }
        }

        if !crate::verify::adds_a_test(&change) {
            return Ok(());
        }

        let red = crate::verify::RedRun::perform(&repo, &gate, base)?;
        let verdict = red.verdict();
        crate::verbose::say("v-18", &format!("red run: {}", verdict.describe()));

        let step = report
            .last_step
            .clone()
            .unwrap_or(crate::step::StepId::new(cycle, stage, 0)?);
        engine.session().journal().append(
            // `ok` is whether the *red run* earned its green, not whether the
            // gate passed — a test that cannot fail is a finding even though
            // every gate around it is green.
            &crate::journal::Record::outcome(
                step,
                crate::time::now(),
                verdict.is_earned(),
                format!("red run: {}", verdict.describe()),
            )
            .with_detail(red.evidence()),
        )?;
        Ok(())
    }

    fn batch(&self, cycle: u32, batch: u32) -> Result<Report> {
        let stage = format!("b{batch}");
        let mut engine = Engine::open(&self.root)?;

        let source = engine
            .session()
            .binding()
            .resolve("path.requirements")
            .ok()
            .map(|p| crate::layout::requirements_text(&p))
            .unwrap_or_default();
        // Everything this cycle already attempted comes off the list first.
        let seen = engine.session().journal().read_all()?;
        let items = remaining(&source, &seen, cycle, self.items_per_batch);
        // `O-10`: what the same slice left on the backlog, so the agent can
        // name it against the item it took instead. Recomputed rather than
        // threaded out of `remaining` itself, which stays pure and unaware of
        // the decision log.
        let done = attempted(&seen, cycle);
        let passed_over: Vec<String> = backlog(&source, usize::MAX)
            .into_iter()
            .filter(|item| !done.contains(&item.requirement))
            .skip(items.len())
            .map(|item| item.requirement)
            .collect();
        // Taken before the items move into the agent: the gate cites these too,
        // so each requirement in the batch can reach the transcript that cleared
        // it (`G-6`).
        let covered: Vec<String> = items.iter().map(|item| item.requirement.clone()).collect();

        // The branch opens before any work happens, so the batch's edits are on
        // it from the first patch rather than being moved onto it afterwards.
        let branch = if covered.is_empty() {
            None
        } else {
            open_branch(&self.root, engine.session().binding(), cycle, batch)
        };

        // `V-18`: where this batch started, for the red run below.
        //
        // Captured **here**, before any step runs, and not read as `HEAD` later.
        // A step commits what it touched as it goes (`T-22`), so by the time the
        // red run happens `HEAD` already contains the work — and a red run whose
        // "without the change" tree *has* the change compiles the same code
        // twice, passes both times, and reports `ProvesNothing` about a test it
        // never actually tried.
        //
        // Measured on Janitor's first batch: the agent checkpointed at
        // `c1/b1/s01`, the red run ran at `s04`, and eleven real tests were
        // written off as proving nothing.
        let base = crate::git::Repo::at(&self.root).head_sha().ok();

        // L-17: Initialize orchestrator and create container if enabled (`L-17`).
        let feature_id = format!("c{cycle}/b{batch}");
        let mut orchestrator = crate::l17::Orchestrator::from_binding(engine.session().binding())?;
        if orchestrator.enabled() {
            let workspace = self.root.clone();
            if let Some((container_id, _mount)) = orchestrator.create_container(&feature_id, &workspace)? {
                crate::verbose::say("l17", &format!("created persistent container {container_id} for {feature_id}"));
            }
        }

        // An empty backlog is `L-14`'s exhausted condition, and the engine
        // reaches it by being handed no work rather than by being told.
        // `S-3`: the operator's own patterns, not just the standing vendor
        // prefixes. This is the client that carries a batch's prompts — repo
        // text, file contents, gate output — to whichever link answers, so it
        // is the one that most needs them.
        let redact = {
            let entries: Vec<(String, String)> = engine
                .session()
                .binding()
                .entries()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            crate::security::patterns_from_entries(&entries)
        };
        // `T-30`: the binding may size the repo map, or turn it off with `0`.
        // Read here as well as on the `run` path — `with_map_budget` was called
        // in exactly one place, `cmd_run`, so every batch that went through a
        // cycle got no map at all whatever the binding said.
        let map_budget = engine
            .session()
            .binding()
            .get("map.budget")
            .ok()
            .and_then(|text| text.trim().parse::<usize>().ok())
            .unwrap_or(crate::agent::DEFAULT_MAP_BUDGET);

        let mut agent = Agent::new(
            crate::client::Client::new(self.transport)
                .with_redaction(redact)
                // `S-4`: a batch's prompts may go to the links the operator
                // declared and nowhere else. This was fail-open — `egress` was
                // `None` and `check_egress` returns `Ok` on `None`, so every
                // check on this path was a no-op.
                .with_egress(
                    crate::security::Egress::new(Vec::new()).allowing_links(self.links.all()),
                ),
            self.links,
            self.health,
            crate::agent::host_for(&self.root),
            items,
        )
        .picked_over(passed_over)
        .with_map_budget(map_budget);
        if self.mode == crate::link::Mode::LocalOnly {
            agent = agent.local_only();
        }
        // Work then gates, as one run, so the leg produces one terminal record
        // and the evidence sits beside the work it is evidence for.
        let target = self.root.join("crates/target");
        let mut gates = Gates::from_binding(engine.session().binding(), &target)?
            .covering(covered.clone());

        // L-17: Route gates through persistent container if enabled (`L-17`).
        if orchestrator.enabled() {
            let default_runtime = gates.default_runtime().cloned().unwrap_or(crate::runtime::Runtime::Host);
            let l17_runtime = orchestrator.runtime_for_feature(&feature_id, &default_runtime);
            gates = gates.with_runtime(l17_runtime);
        }

        let mut leg = Then::new(agent, gates);
        let mut report = engine.run(cycle, &stage, &mut leg, 0)?;
        // Asked of the gates themselves rather than inferred from the step
        // count, which is a different fact about a different thing.
        report.gates_green = leg.second().verdict();

        // `V-18`: if this batch added a test, prove it could fail (`V-3`).
        //
        // Recorded, never blocking. The trigger is a heuristic over a diff, and
        // a heuristic that can halt an unattended run is a heuristic that will
        // halt one for the wrong reason at three in the morning. The verdict and
        // both transcripts go on the journal, where a person and `perp check
        // markers` can read them.
        //
        // Before the commit below, deliberately: the batch's edits are still
        // uncommitted here, so `HEAD` is the tree without them.
        if let Err(e) = self.red_run(&mut engine, cycle, &stage, &report, base.as_deref()) {
            crate::verbose::say("v-18", &format!("red run skipped: {e}"));
        }

        let report = report;

        // L-17: Clean up the batch's container when done (`L-17`).
        if orchestrator.enabled() {
            orchestrator.cleanup_feature(&feature_id)?;
            crate::verbose::say("l17", &format!("cleaned up container for {feature_id}"));
        }

        // Green gate, and only then. A red one leaves the tree exactly as it is
        // for someone to look at — the driver stops the cycle on it anyway.
        //
        // `G-16`: asked of the gates, not of the failure count. Those differ in
        // exactly the case that matters — a leg that never reached its gate
        // steps has nothing failed and nothing checked, and the count cannot
        // tell the two apart.
        if branch.is_some() && report.failed == 0 && leg.second().approved_for_commit() {
            let step = report
                .last_step
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("c{cycle}/{stage}"));
            let subject = format!("Deliver {}", covered.join(", "));
            // The step's own files, plus the evidence the harness wrote about
            // them — a commit with the work and no journal is half a record.
            let mut paths = crate::engine::Work::touched(&leg);
            // `G-18`: what the step declared, plus what the tree says actually
            // changed. `touched` records the paths the *edit tools* named, and a
            // `shell` call can change anything — so a step that edits through a
            // script it wrote is invisible to it.
            //
            // Measured on Janitor's cycle 32: the coder wrote
            // `tools/j26_scan_edit.py` and `tools/j26_plan_edit.py`, ran them,
            // and delivered `J-26` with 150 tests green. `plan.rs`, `scan.rs`
            // and `lib.rs` — 465 insertions — were never staged, and the commit
            // said `Deliver J-26` while carrying only `elevation.rs`. A partial
            // delivery that claims a requirement is worse than no delivery.
            //
            // Tracked files only. An untracked file still has to be declared,
            // so scratch a step leaves behind does not sweep itself in.
            let repo = crate::git::Repo::at(&self.root);
            // `G-19`: and files git does not have yet but does not ignore. A
            // new crate is untracked by definition, so without this a step that
            // creates one has no route into the commit at all.
            for path in
                repo.modified_tracked().into_iter().chain(repo.untracked_unignored())
            {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            for path in evidence_paths(&self.root, engine.session().binding()) {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
            match land_batch(&self.root, &step, &covered, &subject, &paths, crate::engine::Work::author(&leg)) {
                Ok(Some(sha)) => {
                    let mut report = report;
                    // `G-19`: what the commit contains must cover what the step
                    // claims. A commit headed `Deliver X` is the record that X
                    // was done, and a person marks from that record — so a
                    // commit that carries part of the work says something false
                    // in the one place the harness treats as true.
                    //
                    // Measured on Janitor's cycle 32: `Deliver J-26` carried
                    // `elevation.rs` and left 465 insertions across `plan.rs`,
                    // `scan.rs` and `lib.rs` in the tree, because the coder had
                    // edited them through shell scripts `touched` never saw.
                    // Gates green, verifier passed, 150 tests. `G-18` closed
                    // that route; this refuses to trust that it closed every
                    // route, because the next one will be found the same way.
                    let repo = crate::git::Repo::at(&self.root);
                    let left = repo.modified_tracked();
                    if !left.is_empty() {
                        report.warnings.push(format!(
                            "PARTIAL DELIVERY — {} was committed as {sha} but these                              tracked files are still uncommitted: {}. The commit does not                              cover the work it claims; do not mark from it.",
                            covered.join(", "),
                            left.join(", ")
                        ));
                        report.failed += 1;
                    } else {
                        report.warnings.push(format!("committed {} as {sha}", covered.join(", ")));
                    }
                    return Ok(report);
                }
                Ok(None) => {}
                // A batch that ran green and could not be committed is not a
                // failed batch, and pretending otherwise would throw away work
                // that is sitting in the tree. It is said out loud instead.
                Err(e) => {
                    let mut report = report;
                    report.warnings.push(format!("gate was green but nothing was committed: {e}"));
                    return Ok(report);
                }
            }
        }
        Ok(report)
    }

    /// Every other phase: run the gates so the leg produces evidence, and let
    /// the exit predicate decide.
    fn gates_only(&self, cycle: u32, phase: Phase) -> Result<Report> {
        let mut engine = Engine::open(&self.root)?;
        let target = self.root.join("crates/target");
        let mut gates = Gates::from_binding(engine.session().binding(), &target)?;
        let mut report = engine.run(cycle, &phase.letter().to_string(), &mut gates, 0)?;
        report.gates_green = gates.verdict();
        let report = report;

        // A leg that writes evidence and does not commit it leaves the tree
        // dirty, and the next run starts on someone else's mess. A three-batch
        // run landed all three batches and still ended with a modified journal,
        // because phase E ran after the last one.
        if report.failed == 0 {
            let paths = evidence_paths(&self.root, engine.session().binding());
            let step = report
                .last_step
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("c{cycle}/{}", phase.letter()));
            let subject = format!("Record {phase} evidence");
            let mut report = report;
            // No requirement: this commit is the evidence a phase wrote about
            // itself, and there is none in flight. `Perpetum-Step` carries the
            // provenance, which is what a reader of this commit actually needs.
            match land_batch(&self.root, &step, &[], &subject, &paths, None) {
                Ok(Some(sha)) => report.warnings.push(format!("committed evidence as {sha}")),
                Ok(None) => {}
                Err(e) => report.warnings.push(format!("evidence not committed: {e}")),
            }
            return Ok(report);
        }
        Ok(report)
    }
}

/// Why a batch is blocked, saying only what is known.
///
/// A failed step and a red gate are two facts, and this used to report the
/// second whenever it saw the first. Cycle 5 of a real project ended
/// `1 step(s) failed and the gate is red` with both gates at exit 0 — the step
/// had failed on a tool error. `V-2` says only a gate may call something green;
/// the same discipline applies to calling it red, and a stop reason that gets it
/// wrong is worse than one that says less, because it is the one line an
/// operator reads to decide whether to look.
fn blocked_why(failed: u32, gates_green: Option<bool>) -> String {
    let steps = format!("{failed} step(s) failed");
    match gates_green {
        Some(false) => format!("{steps} and the gate is red"),
        Some(true) => format!("{steps}, though the gate is green"),
        None => format!("{steps} and no gate ran"),
    }
}

/// The evidence a leg writes about itself, for staging (`G-3`, `A-2`).
///
/// Named paths, because `git add .` is refused. The artifacts directory is
/// included as a directory: `git add` on it stages the files inside, and the
/// board is regenerated wholesale rather than edited.
///
/// The board is not looked up through `out.board`. That key named a file
/// nothing ever wrote; [`crate::artifact::DIR`] is where artifacts actually
/// go, and it is the authority.
///
/// The journal, and not the state file. `out.state` was staged here too, and
/// `L-4` is the reason it should not be: the state file is a *projection*, and
/// when it and the journal disagree the journal wins. Committing it puts a
/// second copy of the truth in the history that this project's own rule says is
/// not authoritative — and being derived, it rewrites after every outcome, so
/// two batch branches conflict on it every time. `perp state` renders it back
/// byte for byte from the journal, which is what makes leaving it out safe.
///
/// Artifacts went the same way earlier and for the weaker version of the same
/// reason; the state file is the one the rule was actually written about.
/// What a batch commits alongside its work: the journal, and nothing else.
///
/// **Not the artifacts.** They were staged here and `perp init` writes a
/// `.gitignore` that ignores them, for a reason it states plainly — each one is
/// a projection of the journal, regenerable with `perp artifact all`, and
/// committing them means seven files churning on every closed step for no
/// information the journal does not already hold. So the harness was asking git
/// to stage what the harness had told git to ignore, `git add` exited 1, and
/// **every landing failed**: `land_batch` returned the error, `L-29`'s warning
/// carried it, and until that warning had a reader the batch simply reported
/// success with nothing committed. Janitor's `J-19` was written, staged and
/// lost exactly this way.
///
/// The journal is the evidence. A rendering of it is not more evidence.
fn evidence_paths(root: &std::path::Path, binding: &crate::Binding) -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(path) = binding.get("out.journal") {
        let path = path.to_string();
        if root.join(&path).exists() {
            paths.push(path);
        }
    }
    paths
}

/// The branch a batch works on, from `git.branch.batch` (`G-1`).
///
/// Read from the binding rather than hardcoded, because the key has been in
/// every binding since the first one and was read by nothing — the loop did all
/// its work in whatever tree it started in and committed none of it. A pattern
/// that names no placeholders would put every batch on one branch, so a name
/// that does not change with the batch is refused rather than silently reused.
fn branch_for(binding: &crate::Binding, cycle: u32, batch: u32) -> String {
    let pattern = binding.get("git.branch.batch").unwrap_or("perp/c{cycle}/b{batch}");
    let named = pattern.replace("{cycle}", &cycle.to_string()).replace("{batch}", &batch.to_string());
    if named == pattern && pattern != crate::git::batch_branch(cycle, batch) {
        return crate::git::batch_branch(cycle, batch);
    }
    named
}

/// Put the batch's work on its own branch before any of it happens (`G-1`).
///
/// Returns the branch, or `None` when there is no repository to speak of — a
/// scratch directory is a legitimate place to run and not an error.
///
/// There is deliberately no protected-branch check here. `commit` refuses a
/// protected branch outright, which is the guard that matters and the one that
/// cannot be walked past; a second check on the way in was unreachable — every
/// pattern that could name `main` is already sent to the default by
/// [`branch_for`] — and a red run proved it by deleting it without turning
/// anything red.
fn open_branch(root: &std::path::Path, binding: &crate::Binding, cycle: u32, batch: u32) -> Option<String> {
    let repo = crate::git::Repo::at(root);
    repo.head_sha().ok()?;
    let branch = branch_for(binding, cycle, batch);
    match repo.current_branch() {
        Ok(Some(on)) if on == branch => Some(branch),
        _ => repo.create_branch(&branch).ok().map(|()| branch),
    }
}

/// Commit the batch's work, once its gate is green (`G-1`, `G-2`).
///
/// Green first, always. A commit made before the gate is a commit that says
/// work landed when what landed is unknown, and the whole point of `V-2` is
/// that only a gate may call something green.
fn land_batch(
    root: &std::path::Path,
    step: &str,
    requirements: &[String],
    subject: &str,
    paths: &[String],
    author: Option<String>,
) -> Result<Option<String>> {
    let repo = crate::git::Repo::at(root);
    if repo.is_clean().unwrap_or(true) {
        return Ok(None);
    }
    // `G-3`: named, never swept. A batch that touched nothing it can name has
    // nothing to commit, whatever else is lying around in the tree.
    let named: Vec<&str> = paths.iter().map(String::as_str).filter(|p| !p.is_empty()).collect();
    if named.is_empty() {
        return Ok(None);
    }
    repo.stage(&named)?;
    if repo.plumbing(&["diff", "--cached", "--name-only"]).unwrap_or_default().trim().is_empty() {
        return Ok(None);
    }
    let message = crate::git::CommitMessage::new(subject)
        .for_requirements(requirements.to_vec())
        .at_step(step);
    let message = match author {
        Some(who) => message.authored_by(who),
        None => message,
    };
    repo.commit(&message).map(Some)
}

#[cfg(test)]
mod tests {

    /// `O-18`: the catalogue carries every row, including the ones no other
    /// reader keeps.
    ///
    /// `backlog` drops everything that is not workable and `marked` drops `❌`,
    /// so a surface built on either answers "what may the loop do next" when
    /// the question asked was "what is on the list". A person who filed a
    /// requirement and then saw it marked won't-do would find it had vanished.
    #[test]
    fn the_catalogue_keeps_the_rows_the_other_readers_drop() {
        let source = "| id | Requirement |
|---|---|
| `J-1` | A rule is a declarative value, not code. It has an id. |
| ✅ ~~`J-2`~~ | Scanning resolves a rule to the paths that exist. |
| ⛔ `J-3` | The macOS shell is AppKit. **Gated: dependency approval** (`objc2`). |
| ❌ `J-4` | Split into `J-5` and `J-6` after failing twice as one step. |
| 🚧 `J-7` | Blocked on something. |
";
        let all = super::catalogue(source);
        let states: Vec<_> = all.iter().map(|e| (e.id.as_str(), e.state.as_str())).collect();
        assert_eq!(
            states,
            vec![
                ("J-1", "open"),
                ("J-2", "done"),
                ("J-3", "gated"),
                ("J-4", "won't do"),
                ("J-7", "blocked"),
            ],
            "every row, in source order, with the marker it carries"
        );

        // The name is the opening sentence, not the paragraph, and not a
        // truncation at a fixed width that cuts a word in half.
        assert_eq!(all[0].name, "A rule is a declarative value, not code");
        assert!(all[0].text.ends_with("It has an id."), "the text stays whole: {}", all[0].text);

        // Markdown is a rendering detail; a name is read, not rendered.
        assert_eq!(all[2].name, "The macOS shell is AppKit");

        // `marked` is the reader this one is not: it drops the won't-do row,
        // which is exactly why the catalogue exists.
        let marked: Vec<_> = super::marked(source).into_iter().map(|(id, _)| id).collect();
        assert!(!marked.contains(&"J-4".to_string()));
    }

    /// `O-18`: a filed requirement gets an id nothing else is using.
    #[test]
    fn a_new_id_follows_the_highest_the_source_already_uses() {
        // Deliberately out of order, with a done row and a gated row in the
        // middle: the highest is the highest, not the last one written.
        let source = "| id | Requirement |
|---|---|
| `J-7` | Seven. |
| ✅ ~~`J-30`~~ | Thirty, and finished. |
| ⛔ `J-12` | Twelve, and gated. |
";
        let all = super::catalogue(source);
        let highest = all
            .iter()
            .filter_map(|e| e.id.split_once('-'))
            .filter_map(|(_, n)| n.parse::<u32>().ok())
            .max();
        assert_eq!(highest, Some(30), "a gap is a deleted row, not a free id");
    }

    /// `O-16`: a loop blocked on a person does not look like a finished one.
    ///
    /// `backlog` drops gated rows and nothing carried them anywhere else, so
    /// twelve requirements waiting on a GUI toolkit choice produced
    /// `backlog 0 open requirement(s)`, three batches, and no deliveries —
    /// indistinguishable from having nothing to do.
    #[test]
    fn a_requirement_waiting_on_a_person_is_reported_as_waiting() {
        let source = "| id | Requirement |
|---|---|
| ⛔ `J-38` | The Windows shell uses the platform's own controls. Needs a toolkit dependency. |
| `J-39` | Ordinary open work. |
| ✅ ~~`J-40`~~ | Done. |
| `J-41` | Mentions ⛔ in its prose and is not gated by saying so. |
";

        let waiting = gated(source);
        assert_eq!(waiting.len(), 1, "one row is gated: {waiting:?}");
        assert_eq!(waiting[0].id, "J-38");
        assert!(
            waiting[0].waiting_for.contains("toolkit dependency"),
            "and carries what it waits for, not just that it waits: {:?}",
            waiting[0].waiting_for
        );

        let open: Vec<String> =
            backlog(source, 10).iter().map(|i| i.requirement.clone()).collect();
        assert!(!open.contains(&"J-38".to_string()), "still not picked: {open:?}");
        assert!(open.contains(&"J-41".to_string()), "prose is not a marker (`V-23`): {open:?}");
    }

    /// `V-23`: a marker in the prose is not a marker.
    ///
    /// The filter was `line.contains`, so a requirement whose text mentioned a
    /// status character excluded itself — silently, and from the backlog
    /// rather than from a display, so the row stopped existing rather than
    /// looking wrong.
    ///
    /// Measured on Janitor. `J-29`'s text recorded that it had been briefly
    /// gated, and the `⛔` inside that sentence dropped the row out of the
    /// backlog: it was not picked, not offered as an alternative, and nothing
    /// said why. Two cycles ran past it before the silence was noticed.
    #[test]
    fn a_marker_in_the_prose_does_not_gate_the_row() {
        let source = "| id | Requirement |
|---|---|
| `J-29` | A rule declares its root. Was briefly gated (⛔) while a person chose the design. |
| ⛔ `J-30` | Genuinely gated, and the marker is in the status cell. |
| ✅ ~~`J-31`~~ | Done. |
";

        let picked: Vec<String> = backlog(source, 10).iter().map(|i| i.requirement.clone()).collect();
        assert!(
            picked.contains(&"J-29".to_string()),
            "a row that merely talks about gating is still work: {picked:?}"
        );
        assert!(!picked.contains(&"J-30".to_string()), "and a truly gated row stays out: {picked:?}");
        assert!(!picked.contains(&"J-31".to_string()), "as does a done one: {picked:?}");
    }
    use super::*;

    #[allow(clippy::expect_used)]
    fn bound(text: &str) -> (std::path::PathBuf, crate::Binding) {
        let dir = crate::testutil::tmpdir("cycle-branch");
        std::fs::create_dir_all(dir.join(".harness")).expect("dirs");
        std::fs::write(dir.join(".harness/perpetum.md"), "# requirements
").expect("reqs");
        std::fs::write(
            dir.join(".harness/binding.md"),
            format!("```perp-binding
path.requirements = .harness/perpetum.md
{text}```
"),
        )
        .expect("binding");
        let binding = crate::Binding::load(&dir).expect("load");
        (dir, binding)
    }

    /// A transport with nothing behind it.
    ///
    /// `batch` builds an agent whether or not there is anything for it to do,
    /// and an agent needs a transport. With an empty backlog nothing is ever
    /// sent, so a stub that refuses is honest rather than a stand-in for a
    /// model — and a call that *did* happen would fail loudly instead of
    /// quietly passing.
    #[derive(Debug)]
    struct NoTransport;

    impl crate::net::Transport for NoTransport {
        fn send(&self, _request: &crate::net::Request) -> Result<crate::net::Response> {
            Err(crate::Error::unbound("transport", "this test sends nothing"))
        }
    }

    /// A repository with a seed commit, for driving a batch.
    #[allow(clippy::expect_used)]
    fn seeded(dir: &std::path::Path) -> crate::git::Repo {
        let repo = crate::git::Repo::at(dir);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "loop@perpetum.test"],
            vec!["config", "user.name", "Perpetum test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            repo.run_unchecked(&args).expect("git");
        }
        repo.stage(&[".harness"]).expect("stage");
        repo.run_unchecked(&["commit", "-q", "-m", "The starting point"]).expect("seed");
        repo
    }

    /// `L-29`: a leg's warnings reach the reader.
    ///
    /// `Report::warnings` had seven writers and no reader outside a test — and
    /// three tests asserting the warning *arrives*, which was true and not the
    /// question. `land_batch` records "gate was green but nothing was
    /// committed" with the real error; on Janitor that happened, `J-19`'s work
    /// sat staged and uncommitted, and the run said only "stopped: the backlog
    /// is exhausted".
    #[test]
    fn a_legs_warnings_are_printed_and_not_only_stored() {
        let report = Report {
            steps: 4,
            failed: 0,
            stop: None,
            park: None,
            spend: Spend::default(),
            took_over: None,
            first_step: None,
            last_step: None,
            controls: Vec::new(),
            warnings: vec![
                "gate was green but nothing was committed: boom".into(),
                "committed J-19 as abc1234".into(),
            ],
            approvals_raised: 0,
            gates_green: None,
        };

        let outcome = Outcome {
            cycle: 5,
            legs: vec![Leg {
                phase: Phase::D,
                report,
                advanced: "stays in D".into(),
            }],
            stop: None,
            parked: None,
            spend: Spend::default(),
            ended_in: Phase::D,
        };

        let text = outcome.describe();
        assert!(
            text.contains("nothing was committed: boom"),
            "a failed landing must reach the reader, verbatim:
{text}"
        );
        assert!(
            text.contains("committed J-19 as abc1234"),
            "and so must a successful one — it is the only confirmation there is:
{text}"
        );
    }

    /// A transport that plays a scripted coder, for driving a batch to a commit.
    ///
    /// Deliberately adversarial in shape rather than ideal: the script writes a
    /// scratch file and removes it, and repeats a verification command. Both
    /// are things a real coder does and both have taken a whole delivery down
    /// (`G-17`, `L-31`).
    #[derive(Debug)]
    struct ScriptedCoder {
        replies: std::cell::RefCell<Vec<String>>,
    }

    impl ScriptedCoder {
        fn new(replies: Vec<&str>) -> ScriptedCoder {
            ScriptedCoder {
                replies: std::cell::RefCell::new(
                    replies.into_iter().map(str::to_string).collect(),
                ),
            }
        }
    }

    impl crate::net::Transport for ScriptedCoder {
        fn send(&self, request: &crate::net::Request) -> Result<crate::net::Response> {
            if request.url.contains("/models") {
                return Ok(crate::net::Response {
                    status: 200,
                    body: r#"{"data":[{"id":"small","type":"llm","state":"loaded"}]}"#.into(),
                });
            }
            if request.url.contains("/responses") {
                return Ok(crate::net::Response { status: 404, body: "{}".into() });
            }
            let mut replies = self.replies.borrow_mut();
            // An exhausted script means "nothing further to do", which ends the
            // step cleanly rather than failing it — the coder has finished.
            let content = if replies.is_empty() { String::new() } else { replies.remove(0) };
            Ok(crate::net::Response {
                status: 200,
                body: crate::json::to_string(&crate::json::Value::Obj(vec![
                    ("model".into(), crate::json::Value::str("small")),
                    (
                        "choices".into(),
                        crate::json::Value::Arr(vec![crate::json::Value::Obj(vec![(
                            "message".into(),
                            crate::json::Value::Obj(vec![
                                ("role".into(), crate::json::Value::str("assistant")),
                                ("content".into(), crate::json::Value::str(&content)),
                            ]),
                        )])]),
                    ),
                    (
                        "usage".into(),
                        crate::json::Value::Obj(vec![
                            ("prompt_tokens".into(), crate::json::Value::int(10)),
                            ("completion_tokens".into(), crate::json::Value::int(5)),
                        ]),
                    ),
                ])),
            })
        }
    }

    /// `G-19`: a commit must cover the work it claims.
    ///
    /// A commit headed `Deliver X` is the record that X was done, and a person
    /// marks from that record — so one carrying part of the work says something
    /// false in the one place this harness treats as true.
    ///
    /// Measured on Janitor's cycle 32: `Deliver J-26` carried `elevation.rs`
    /// and left 465 insertions across three files in the tree, because the
    /// coder edited them through shell scripts `touched` never saw. Every gate
    /// green, verifier passed, 150 tests. `G-18` closed that route; this
    /// refuses to assume it closed every route.
    ///
    /// Here the gate itself writes a tracked file after the coder has finished,
    /// so the change cannot reach `touched` by any route — the shape of the
    /// cycle-32 failure. The assertion is the invariant itself: after a
    /// delivery, nothing tracked is left over. `G-19` is the backstop that says
    /// so out loud and fails the batch when some future route defeats it.
    #[test]
    fn a_commit_that_leaves_the_work_behind_is_not_a_delivery() {
        // A gate that edits a tracked file and passes.
        let meddling = if cfg!(windows) {
            "cmd /C \"echo meddled>> tracked.txt\""
        } else {
            "sh -c \"echo meddled >> tracked.txt\""
        };
        let (dir, _binding) = bound(&format!(
            "path.links = .harness/links.md
             gate.cwd = .
             gate.timeout = 60
             gate.test = {meddling}
             out.journal = .harness/journal.jsonl
             out.state = .harness/state.md
             git.branch.batch = perp/c{{cycle}}/b{{batch}}
"
        ));
        std::fs::write(
            dir.join(".harness/perpetum.md"),
            "| id | Requirement |
|---|---|
| `W-1` | Write greeting.txt. |
",
        )
        .expect("reqs");
        std::fs::write(
            dir.join(".harness/links.md"),
            "```perp-links
link.here.kind = openai-compat
             link.here.base_url = http://127.0.0.1:1/v1
link.here.privacy = local
             link.here.model = small
role.coder = here
```
",
        )
        .expect("links");
        // Tracked before the seed commit, so the gate's edit to it is a change
        // to a file git already knows — the case `modified_tracked` reports.
        std::fs::write(dir.join("tracked.txt"), "original
").expect("write");
        let _repo = seeded(&dir);

        let coder = ScriptedCoder::new(vec![
            "```perp-call
tool: write
path: greeting.txt
content: hello
```",
            "Done.",
        ]);
        let driver = Driver {
            root: dir.clone(),
            links: &crate::link::Links::parse(
                &std::fs::read_to_string(dir.join(".harness/links.md")).expect("read"),
            )
            .expect("links"),
            health: &crate::link::AssumeHealthy,
            mode: crate::link::Mode::LocalOnly,
            batches: 1,
            items_per_batch: 1,
            transport: &coder,
        };
        let report = driver.batch(1, 1).expect("the batch ran");

        let said = report.warnings.join(" | ");
        assert!(said.contains("committed W-1"), "the batch landed: {said}");
        assert!(
            !said.contains("PARTIAL DELIVERY"),
            "and it landed whole — nothing was left behind to warn about: {said}"
        );

        // The invariant, checked against the tree rather than against the
        // report: after a delivery there is nothing tracked left over. The gate
        // edited `tracked.txt` after the coder finished, by a route `touched`
        // cannot see, and it still went into the commit.
        let left = crate::git::Repo::at(&dir).modified_tracked();
        assert!(
            left.is_empty(),
            "a delivery must leave no tracked work behind, and left: {left:?}"
        );
        assert_eq!(report.failed, 0, "so the batch is clean and a person may mark from it");
    }

    /// **The invariant nothing asserted: green work becomes a commit.**
    ///
    /// `land_batch` was tested in isolation and `Driver::batch` was tested with
    /// an empty backlog — deliberately, to keep that test about the batch path
    /// "rather than about landing". So the sentence the whole harness exists to
    /// make true — *a step that wrote code and gated green produces a commit* —
    /// was asserted nowhere, and every violation of it cost a real cycle to
    /// find, on a project, with a person reading the log.
    ///
    /// Seven did. `G-16` committed a tree that would not compile; `G-17` lost a
    /// delivery to a scratch file the coder had tidied away; `L-31` killed a
    /// step for running its tests twice; `T-31` for saying `bash`; `V-23` hid a
    /// requirement because its prose mentioned a marker. Each was reachable
    /// only once the one before it was fixed, so they arrived one cycle at a
    /// time over a day.
    ///
    /// The script here is adversarial on purpose — it writes a scratch file and
    /// deletes it, and runs its check twice — because an ideal coder is not the
    /// one that finds these.
    #[test]
    fn a_batch_that_wrote_code_and_gated_green_lands_a_commit() {
        // A gate that tidies a temp file and passes — a build step doing what
        // build steps do. The coder wrote `.scratch.tmp`, so `touched` holds it;
        // by staging time it is gone, which is `G-17`'s exact shape.
        let green = if cfg!(windows) {
            "cmd /C \"del .scratch.tmp\""
        } else {
            "sh -c \"rm -f .scratch.tmp\""
        };
        let (dir, _binding) = bound(&format!(
            "path.links = .harness/links.md
             gate.cwd = .
             gate.timeout = 60
             gate.test = {green}
             out.journal = .harness/journal.jsonl
             out.state = .harness/state.md
             git.branch.batch = perp/c{{cycle}}/b{{batch}}
"
        ));
        std::fs::write(
            dir.join(".harness/perpetum.md"),
            "| id | Requirement |
|---|---|
| `W-1` | Write greeting.txt with one line. |
",
        )
        .expect("reqs");
        std::fs::write(
            dir.join(".harness/links.md"),
            "```perp-links
link.here.kind = openai-compat
link.here.base_url = http://127.0.0.1:1/v1
             link.here.privacy = local
\n             link.here.model = small
role.coder = here
```
",
        )
        .expect("links");
        let repo = seeded(&dir);

        // Write the real file; make a scratch file and remove it; check twice.
        let coder = ScriptedCoder::new(vec![
            "```perp-call
tool: write
path: greeting.txt
content: hello
```",
            "```perp-call
tool: write
path: .scratch.tmp
content: temp
```",
            "Done — greeting.txt is written.",
        ]);

        let driver = Driver {
            root: dir.clone(),
            links: &crate::link::Links::parse(
                &std::fs::read_to_string(dir.join(".harness/links.md")).expect("read"),
            )
            .expect("links"),
            health: &crate::link::AssumeHealthy,
            mode: crate::link::Mode::LocalOnly,
            batches: 1,
            items_per_batch: 1,
            transport: &coder,
        };
        let report = driver.batch(1, 1).expect("the batch ran");

        assert!(dir.join("greeting.txt").exists(), "the coder's file is on disk");
        assert!(
            !dir.join(".scratch.tmp").exists(),
            "the scratch file must actually be gone, or this proves nothing"
        );

        let log = repo
            .run_unchecked(&["log", "--oneline", "--all"])
            .expect("git")
            .stdout_tail;
        assert!(
            log.contains("Deliver W-1"),
            "green work must land: gates {:?}, warnings {:?}, log {log}",
            report.gates_green,
            report.warnings
        );
    }

    /// `V-18`: a batch that added a test records a red run.
    ///
    /// **The first test to drive `Driver::batch` at all.** Everything inside it
    /// was covered — `remaining`, `markers`, `Gates`, `Agent`, `RedRun` — and
    /// the function that wires them together was not, which is why the red-run
    /// hook could be added, compile, pass every gate, and never once execute.
    /// The reachability check cannot see a gap like that: `batch` *is* called,
    /// from `run_phase`. It only ever answers "does anything call this", never
    /// "does anything try it".
    ///
    /// The backlog is left empty on purpose. The hook reads the working tree
    /// rather than the agent's result, so no model is needed to prove it fires —
    /// and an empty backlog means no branch and no commit, keeping this about
    /// the batch path rather than about landing.
    #[test]
    fn a_batch_that_added_a_test_records_a_red_run() {
        let green = if cfg!(windows) { "cmd /C \"exit 0\"" } else { "sh -c \"exit 0\"" };
        let (dir, _binding) = bound(&format!(
            "out.journal = .harness/journal.jsonl\n\
             out.state   = .harness/state.md\n\
             gate.test   = {green}\n"
        ));
        seeded(&dir);

        // The change: a new, untracked file carrying a test. Untracked is the
        // realistic case and the one `git diff HEAD` says nothing about.
        std::fs::write(dir.join("added_test.rs"), "#[test]\nfn it_works() {}\n")
            .expect("write");

        let links = crate::link::Links::parse(
            "```perp-links\n\
             link.here.kind = lmstudio\n\
             link.here.base_url = http://localhost:1234\n\
             link.here.model = small\n\
             role.coder = here\n```\n",
        )
        .expect("links");

        let driver = Driver {
            root: dir.clone(),
            links: &links,
            health: &crate::link::AssumeHealthy,
            mode: crate::link::Mode::LocalOnly,
            batches: 1,
            items_per_batch: 1,
            transport: &NoTransport,
        };

        driver.batch(1, 1).expect("the batch ran");

        let records = crate::journal::Journal::at(dir.join(".harness/journal.jsonl"))
            .read_all()
            .expect("journal");
        let red = records
            .iter()
            .find(|r| r.summary.starts_with("red run:"))
            .expect("a batch that added a test records a red run (`V-18`)");

        // Both transcripts, not just the verdict (`V-2`).
        let detail = red.detail.as_deref().unwrap_or_default();
        assert!(detail.contains("without the change"), "{detail}");
        assert!(detail.contains("with the change"), "{detail}");

        // The gate passes either way here — the file is not compiled by
        // `exit 0` — so the honest verdict is that it proves nothing, and
        // `ok` reports the *red run*, not the gate.
        assert_eq!(red.ok, Some(false), "a test that cannot fail is a finding: {}", red.summary);
    }

    /// And a batch that added no test does not pay for a second gate run.
    #[test]
    fn a_batch_with_no_new_test_records_no_red_run() {
        let green = if cfg!(windows) { "cmd /C \"exit 0\"" } else { "sh -c \"exit 0\"" };
        let (dir, _binding) = bound(&format!(
            "out.journal = .harness/journal.jsonl\n\
             out.state   = .harness/state.md\n\
             gate.test   = {green}\n"
        ));
        seeded(&dir);
        std::fs::write(dir.join("notes.md"), "a change with no test in it\n").expect("write");

        let links = crate::link::Links::parse(
            "```perp-links\n\
             link.here.kind = lmstudio\n\
             link.here.base_url = http://localhost:1234\n\
             link.here.model = small\n\
             role.coder = here\n```\n",
        )
        .expect("links");

        Driver {
            root: dir.clone(),
            links: &links,
            health: &crate::link::AssumeHealthy,
            mode: crate::link::Mode::LocalOnly,
            batches: 1,
            items_per_batch: 1,
            transport: &NoTransport,
        }
        .batch(1, 1)
        .expect("the batch ran");

        let records = crate::journal::Journal::at(dir.join(".harness/journal.jsonl"))
            .read_all()
            .expect("journal");
        assert!(
            !records.iter().any(|r| r.summary.starts_with("red run:")),
            "no test added, no second gate run"
        );
    }

    /// `V-14`: the gate is told what the work delivered, not what it was given.
    ///
    /// Cycle 10 filed three green gates against `L-23`, `M-29` and `M-27` on a
    /// cycle that changed no source at all. The gates were real and green; they
    /// had measured a tree none of those requirements had touched.
    /// `V-5`: a `Then` must forward the review to the half that ran.
    ///
    /// The default is `None`, and a batch is always a `Then`, so the engine
    /// asked the wrapper for a verdict and got nothing — no verifier ever ran
    /// through the only path that reaches an agent. Janitor's first batch made
    /// fifteen calls, every one as `coder`, and recorded no verdict.
    ///
    /// The second time this requirement has shipped unrun: `Agent::review`'s
    /// own doc records the first, when 129 calls went out as `coder` and the
    /// configured verifier was asked for nothing.
    #[test]
    fn a_review_is_not_swallowed_by_the_wrapper_around_the_agent() {
        use crate::engine::{Done, Task, Work};

        /// A half that has something to say about its own work.
        #[derive(Default)]
        struct Reviewed {
            done: bool,
        }
        impl Work for Reviewed {
            fn next(&mut self) -> Option<Task> {
                if self.done {
                    return None;
                }
                Some(Task::new("work"))
            }
            fn perform(&mut self, _task: &Task) -> Done {
                self.done = true;
                Done::ok("wrote something")
            }
            fn spend(&self) -> Spend {
                Spend::default()
            }
            fn review(&mut self) -> Option<String> {
                Some("verdict: a second link read it".into())
            }
        }

        /// A half with nothing to review — a gate.
        #[derive(Default)]
        struct Silent;
        impl Work for Silent {
            fn next(&mut self) -> Option<Task> {
                None
            }
            fn perform(&mut self, _task: &Task) -> Done {
                Done::ok("gate")
            }
            fn spend(&self) -> Spend {
                Spend::default()
            }
        }

        let mut leg = Then::new(Reviewed::default(), Silent);

        // While the first half is running, its verdict reaches the engine.
        let task = Work::next(&mut leg).expect("the first half has work");
        leg.perform(&task);
        assert_eq!(
            Work::review(&mut leg).as_deref(),
            Some("verdict: a second link read it"),
            "the agent's review must not be swallowed by the wrapper (`V-5`)"
        );

        // Once it has handed over, the gates are asked instead — and a gate has
        // nothing to review, so the agent is not re-reviewed on its behalf.
        assert!(Work::next(&mut leg).is_none(), "both halves are spent");
        assert_eq!(Work::review(&mut leg), None, "a gate reviews nothing");
    }

    #[test]
    fn a_gate_cites_only_the_requirements_that_delivered() {
        use crate::engine::{Done, Task, Work};

        /// A first half that was handed three requirements and delivered one.
        #[derive(Default)]
        struct Half {
            done: bool,
        }
        impl Work for Half {
            fn next(&mut self) -> Option<Task> {
                if self.done {
                    return None;
                }
                Some(Task::new("work"))
            }
            fn perform(&mut self, _task: &Task) -> Done {
                self.done = true;
                Done::ok("delivered M-29 only")
            }
            fn spend(&self) -> Spend {
                Spend::default()
            }
            fn delivered(&self) -> Vec<String> {
                vec!["M-29".to_string()]
            }
        }

        /// A second half that records what it was told to cover.
        #[derive(Default)]
        struct Recorder {
            covering: Vec<String>,
            asked: bool,
        }
        impl Work for Recorder {
            fn next(&mut self) -> Option<Task> {
                if self.asked {
                    return None;
                }
                Some(Task::new("gate"))
            }
            fn perform(&mut self, _task: &Task) -> Done {
                self.asked = true;
                Done::ok("gate is green")
            }
            fn spend(&self) -> Spend {
                Spend::default()
            }
            fn covers(&mut self, delivered: Vec<String>) {
                self.covering = delivered;
            }
        }

        let mut leg = Then::new(Half::default(), Recorder::default());
        // Drive it the way the engine does: next/perform until it is spent.
        while let Some(task) = leg.next() {
            leg.perform(&task);
        }

        // Not the batch's three. The one that changed something.
        assert_eq!(leg.second().covering, vec!["M-29".to_string()]);
    }

    #[test]
    fn every_cited_id_counts_as_attempted_with_no_exceptions() {
        // `attempted` used to skip `V-2` outright, because the gate cited it on
        // every batch and would otherwise have marked the harness's own `V-2`
        // attempted with nobody working on it.
        //
        // The gate stopped citing it, so the exception stopped protecting
        // anything and started doing the opposite harm: real work on `V-2` would
        // never count as attempted, and the same batch would offer it again for
        // the rest of the cycle. Which is the bug `attempted` exists to prevent.
        use crate::journal::Record;
        use crate::step::StepId;

        let step = |n: u32| StepId::new(4, "b1", n).expect("step");
        let records = vec![
            Record::outcome(step(1), 100, true, "did the work").for_requirements(["V-2"]),
            Record::outcome(step(2), 200, true, "did more").for_requirements(["L-3", "V-9"]),
        ];
        let seen = attempted(&records, 4);
        assert!(seen.contains(&"V-2".to_string()), "no id is exempt: {seen:?}");
        assert_eq!(seen.len(), 3, "and each is counted once: {seen:?}");
    }

    #[test]
    fn the_batch_branch_comes_from_the_binding_key_that_declares_it() {
        // `git.branch.batch` was in every binding written and read by nothing.
        // The loop worked in whatever tree it started in and committed none of
        // it, through four unattended runs that all reported success.
        let (_dir, binding) = bound("git.branch.batch = work/c{cycle}-b{batch}
");
        assert_eq!(branch_for(&binding, 2, 7), "work/c2-b7");

        let (_dir, default) = bound("");
        assert_eq!(branch_for(&default, 1, 3), crate::git::batch_branch(1, 3));
    }

    #[test]
    fn a_pattern_naming_no_batch_is_refused_rather_than_reused() {
        // Every batch on one branch is worse than the hardcoded name: the
        // second batch's commit lands on the first batch's branch and the two
        // can no longer be told apart.
        let (_dir, binding) = bound("git.branch.batch = perp/fixed
");
        assert_eq!(branch_for(&binding, 1, 1), crate::git::batch_branch(1, 1));
        assert_eq!(branch_for(&binding, 1, 2), crate::git::batch_branch(1, 2));
        assert_ne!(branch_for(&binding, 1, 1), branch_for(&binding, 1, 2));
    }

    #[test]
    fn the_evidence_a_leg_writes_is_named_so_it_can_be_staged() {
        // A three-batch run landed all three batches and still ended with a
        // modified journal and an unstaged board, because phase E ran after the
        // last batch and nothing committed what it wrote.
        let (dir, binding) = bound("out.journal = .harness/journal.jsonl
out.state = .harness/state.md
");

        // Nothing written yet: nothing to name.
        assert!(evidence_paths(&dir, &binding).is_empty());

        std::fs::write(dir.join(".harness/journal.jsonl"), "{}
").expect("journal");
        std::fs::create_dir_all(crate::artifact::dir_in(&dir)).expect("artifacts");
        std::fs::write(crate::artifact::Kind::Board.path_in(&dir), "<p>").expect("board");

        let paths = evidence_paths(&dir, &binding);
        assert!(paths.contains(&".harness/journal.jsonl".to_string()), "{paths:?}");
        // And **not** the artifacts, though they are sitting right there.
        // `perp init` writes a `.gitignore` that ignores them, so staging them
        // made `git add` exit 1 and every landing fail — the harness asking git
        // to stage what the harness had told git to ignore. Janitor's `J-19`
        // was written, staged and lost that way, and the reason was invisible
        // until `L-29` gave the warning a reader.
        assert!(
            !paths.contains(&crate::artifact::DIR.to_string()),
            "a rendering of the journal is not more evidence than the journal: {paths:?}"
        );
        assert!(
            !paths.contains(&".harness/state.md".to_string()),
            "a path the binding names but nothing has written is not staged: {paths:?}"
        );
    }

    #[test]
    fn a_directory_that_is_not_a_repository_is_not_an_error() {
        let (dir, binding) = bound("");
        assert_eq!(open_branch(&dir, &binding, 1, 1), None, "a scratch tree is a fine place to run");
    }

    #[test]
    fn a_batch_lands_on_its_own_branch_and_never_on_a_protected_one() {
        let (dir, binding) = bound("");
        let repo = crate::git::Repo::at(&dir);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "loop@perpetum.test"],
            vec!["config", "user.name", "Perpetum test"],
            vec!["config", "commit.gpgsign", "false"],
        ] {
            repo.run_unchecked(&args).expect("git");
        }
        // The harness lives in `.harness` now, so that is what the seed commit has.
        repo.stage(&[".harness"]).expect("stage");
        // Seeded through plumbing: `commit` refuses `main` outright (`G-1`),
        // which is the rule under test further down. This stands for a
        // repository that already existed before the loop ever saw it.
        repo.run_unchecked(&["commit", "-q", "-m", "The starting point"]).expect("seed");
        assert_eq!(repo.current_branch().expect("branch").as_deref(), Some("main"));

        let opened = open_branch(&dir, &binding, 1, 1).expect("a branch");
        assert_eq!(opened, "perp/c1/b1");
        assert_eq!(repo.current_branch().expect("branch").as_deref(), Some("perp/c1/b1"));

        // Nothing to commit yet: a clean tree is not an empty commit.
        assert_eq!(land_batch(&dir, "c1/b1/s01", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None).expect("land"), None);

        std::fs::write(dir.join("src.txt"), "the work
").expect("write");
        let sha = land_batch(&dir, "c1/b1/s01", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None)
            .expect("land")
            .expect("a commit");

        assert!(!sha.is_empty());
        assert!(repo.is_clean().expect("clean"), "the tree is clean once the batch has landed");

        let log = repo.plumbing(&["log", "-1", "--format=%B"]).expect("log");
        assert!(log.contains("Deliver L-3"), "{log}");
        assert!(log.contains("L-3"), "the requirement is a trailer, not prose: {log}");
        assert!(log.contains("c1/b1/s01"), "and so is the step: {log}");
        assert!(
            !log.contains("Co-Authored-By"),
            "no author, no trailer — better than one git cannot parse: {log}"
        );

        // Given an identity, it is a well-formed one.
        std::fs::write(dir.join("src.txt"), "more work
").expect("write");
        land_batch(
            &dir,
            "c1/b1/s02",
            &["L-3".into()],
            "Deliver L-3 again",
            &["src.txt".into()],
            Some("deepseek-v4-flash via perp <ds-fast@perp.invalid>".into()),
        )
        .expect("land")
        .expect("a commit");
        let log = repo.plumbing(&["log", "-1", "--format=%B"]).expect("log");
        let trailer = log
            .lines()
            .find(|line| line.starts_with("Co-Authored-By:"))
            .expect("a co-author trailer");
        assert!(
            trailer.contains('<') && trailer.contains('>') && trailer.contains('@'),
            "a git identity, not a bare word: {trailer}"
        );

        // A file the step never named stays out, even though it is dirty.
        std::fs::write(dir.join("stray.txt"), "not this step's
").expect("write");
        assert_eq!(
            land_batch(&dir, "c1/b1/s02", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None)
                .expect("land"),
            None,
            "`G-3` stages what the step touched, not what happens to be lying around"
        );

        // The guard that matters: committing on a protected branch is refused,
        // whatever else has gone wrong upstream of it (`G-1`).
        repo.run_unchecked(&["switch", "-q", "main"]).expect("switch");
        std::fs::write(dir.join("src.txt"), "on main
").expect("write");
        let refused = land_batch(&dir, "c1/b1/s03", &["L-3".into()], "Deliver L-3", &["src.txt".into()], None);
        assert!(refused.is_err(), "a commit onto main is refused, not made: {refused:?}");

        // And a step that named nothing commits nothing, however dirty the tree.
        repo.run_unchecked(&["switch", "-q", "perp/c1/b1"]).expect("switch");
        assert_eq!(
            land_batch(&dir, "c1/b1/s04", &["L-3".into()], "Deliver L-3", &[], None).expect("land"),
            None,
            "no named paths, no commit"
        );
    }

    const SOURCE: &str = "\
| id | Requirement |\n\
|---|---|\n\
| ✅ ~~`L-1`~~ | already done |\n\
| 🟡 `L-2` | in progress, so still work |\n\
| `L-3` | never started |\n\
| ⛔ `M-25` | external-gated |\n\
| ❌ `X-11` | declined |\n\
| 🔶 `I-3` | conflicting |\n\
| not a row | ignored |\n";

    /// `V-8`: the counting half. A marker is read as what it is, so gated and
    /// blocked can be counted apart from delivered.
    #[test]
    fn every_marker_is_read_as_what_it_is() {
        use crate::verify::{Counts, Marker};
        let source = "| ✅ ~~`L-1`~~ | done |\n\
                      | 🟡 `L-2` | in progress |\n\
                      | `L-3` | open |\n\
                      | 🚧 `L-4` | blocked |\n\
                      | ⛔ `L-5` | gated |\n\
                      | 🔶 `L-6` | conflicting |\n\
                      | not a row | ignored |\n";
        let markers = markers(source);
        assert_eq!(
            markers,
            vec![
                Marker::Done,
                Marker::InProgress,
                Marker::Open,
                Marker::Blocked,
                Marker::Gated,
                Marker::Conflicting
            ]
        );

        let counts = Counts::of(&markers);
        assert_eq!(counts.done, 1);
        assert_eq!(counts.gated, 1);
        assert_eq!(counts.blocked, 1);
        assert_eq!(counts.total(), 6);
        // The lie `V-8` names: never `done / total`.
        assert!(counts.describe().starts_with("1 of 6 delivered"), "{}", counts.describe());
        assert!(counts.describe().contains("1 gated"), "{}", counts.describe());
    }

    /// `V-8`: a blocked requirement is not re-picked.
    ///
    /// It was already excluded, but by accident — the id parser strips `🟡`
    /// and nothing else, so `🚧 \`L-3\`` failed `is_requirement_id` further
    /// down. Correct for a reason unrelated to the rule it was keeping, which
    /// is a correctness that anyone teaching the parser to strip markers would
    /// have removed without noticing. Now it is on the exclusion list and this
    /// says so.
    #[test]
    fn a_blocked_requirement_is_not_picked_up_again() {
        let source = "| 🚧 `L-3` | blocked: it does not compile |\n| `L-4` | open work |\n";
        let picked = backlog(source, 10);
        let ids: Vec<&str> = picked.iter().map(|i| i.requirement.as_str()).collect();
        assert_eq!(ids, vec!["L-4"], "blocked work stays blocked until its reason changes");
    }

    #[test]
    fn the_backlog_is_what_the_requirements_source_says_is_open() {
        let items = backlog(SOURCE, 10);
        let ids: Vec<&str> = items.iter().map(|i| i.requirement.as_str()).collect();
        assert_eq!(ids, ["L-2", "L-3"], "in progress is unfinished; the rest are not the loop's");
    }

    #[test]
    fn a_done_requirement_is_never_picked_up_again() {
        // The marker is the only thing that takes work off the backlog, which
        // is why the loop may not set one.
        let items = backlog(SOURCE, 10);
        assert!(!items.iter().any(|i| i.requirement == "L-1"));
        assert!(!items.iter().any(|i| i.requirement == "M-25"), "external-gated is not blocked-on-us");
    }

    #[test]
    fn the_batch_size_bounds_what_one_batch_attempts() {
        // An unattended loop with no bound on its own appetite is one that
        // spends a budget discovering it had one.
        assert_eq!(backlog(SOURCE, 1).len(), 1);
        assert_eq!(backlog(SOURCE, 0).len(), 0);
    }

    #[test]
    fn an_empty_backlog_is_the_exhausted_condition() {
        // `L-14`. The engine reaches it by being handed no work, rather than by
        // being told there is none.
        assert!(backlog("| ✅ ~~`L-1`~~ | done |\n", 10).is_empty());
        assert!(backlog("", 10).is_empty());
    }

    #[test]
    fn only_development_has_work_a_machine_can_do() {
        // B, C, E and F are a person's — gathering, prioritising, writing
        // release notes, reconciling. A loop that ran them would be asserting
        // it had done them, which is exactly what `L-2` forbids.
        assert_eq!(Phase::D.exit(5), crate::phase::Exit::BatchesDelivered(5));
        for phase in [Phase::B, Phase::C, Phase::E, Phase::F] {
            assert_ne!(phase.exit(5), crate::phase::Exit::BatchesDelivered(5));
        }
    }

    #[test]
    fn a_cycle_does_not_pick_up_what_it_already_attempted() {
        // The loop may not mark its own work done, so the requirements source
        // looks identical after a batch as before it. Without this the second
        // batch redoes the first batch's items, and an hour-long run repeats
        // the same four things fifteen times.
        use crate::journal::Record;
        use crate::step::StepId;

        let step = |n: u32| StepId::new(1, "b1", n).expect("step");
        let records = vec![
            Record::outcome(step(1), 100, true, "did it").for_requirements(["L-2"]),
            // A gate no longer cites `V-2`, so nothing here needs an exception
            // for it. See the test below for why the exception had to go.
            Record::outcome(step(2), 200, true, "gate").for_requirements(["L-2"]),
        ];

        let left = remaining(SOURCE, &records, 1, 10);
        let ids: Vec<&str> = left.iter().map(|i| i.requirement.as_str()).collect();
        assert_eq!(ids, ["L-3"], "L-2 was attempted; L-3 was not");

        // A different cycle starts clean — a second attempt belongs there.
        let next = remaining(SOURCE, &records, 2, 10);
        assert_eq!(next.len(), 2, "cycle 2 may try L-2 again");
    }

    #[test]
    fn a_failed_item_is_not_retried_inside_the_same_cycle() {
        // `L-11`: repeating an unproductive thing is an error, not a retry.
        use crate::journal::Record;
        use crate::step::StepId;
        let records = vec![Record::outcome(
            StepId::new(1, "b1", 1).expect("step"),
            100,
            false,
            "L-2 made no progress",
        )
        .for_requirements(["L-2"])];
        let ids: Vec<String> =
            remaining(SOURCE, &records, 1, 10).into_iter().map(|i| i.requirement).collect();
        assert!(!ids.contains(&"L-2".to_string()), "{ids:?}");
    }

    /// `L-4`: the journal is the record and the state file is a projection of
    /// it. Staging the projection commits a second copy of the truth that the
    /// rule itself says loses any disagreement — and nothing tested that it was
    /// being staged, which is how it stayed that way.
    #[test]
    fn a_leg_commits_the_journal_and_not_the_projection_of_it() {
        let (dir, binding) = bound("out.journal = .harness/journal.jsonl\nout.state = .harness/state.md\n");
        std::fs::write(dir.join(".harness/journal.jsonl"), "{}\n").expect("journal");
        std::fs::write(dir.join(".harness/state.md"), "# state\n").expect("state");

        let paths = evidence_paths(&dir, &binding);
        assert!(paths.iter().any(|p| p.contains("journal")), "the record is staged: {paths:?}");
        assert!(
            !paths.iter().any(|p| p.contains("state.md")),
            "the projection is not: {paths:?}"
        );
    }

    /// The defect verbatim. Cycle 5 of a real project reported this with both
    /// gates at exit 0, because the claim was read off the step count.
    #[test]
    fn a_green_gate_is_never_reported_as_red() {
        let said = blocked_why(1, Some(true));
        assert!(!said.contains("the gate is red"), "{said}");
        assert!(said.contains("green"), "it says what was actually true: {said}");
        assert!(said.contains("1 step(s) failed"), "and still says the step failed: {said}");
    }

    #[test]
    fn a_red_gate_is_still_reported_as_red() {
        let said = blocked_why(2, Some(false));
        assert_eq!(said, "2 step(s) failed and the gate is red");
    }

    /// Not the same as red, and worth saying: a batch that never reached its
    /// gate failed somewhere earlier, which is where the operator should look.
    #[test]
    fn a_gate_that_never_ran_is_not_reported_as_red_either() {
        let said = blocked_why(1, None);
        assert!(!said.contains("red"), "{said}");
        assert!(said.contains("no gate ran"), "{said}");
    }

    /// The tri-state the message is built from. [`Gates::all_green`] answers
    /// `false` for an empty run, which is right for deciding whether to commit
    /// and wrong for saying what happened.
    #[test]
    fn gates_that_never_ran_have_no_verdict_rather_than_a_red_one() {
        let (dir, binding) = bound("gate.test = true\n");
        let gates = crate::engine::Gates::from_binding(&binding, &dir).expect("gates");
        assert!(gates.results.is_empty(), "nothing has run yet");
        assert_eq!(gates.verdict(), None, "no results is not a red verdict");
        assert!(!gates.all_green(), "and all_green still answers false, as its callers expect");
    }

    #[test]
    fn a_leg_is_work_then_gates_and_stops_once() {
        // Found by the first unattended cycle: running the two as separate
        // engine calls produced `work · stopped · gate · stopped` — two
        // terminal records for one leg, with a gate between them.
        use crate::engine::{Done, Task, Work};

        struct Fixed(Vec<&'static str>, usize);
        impl Work for Fixed {
            fn next(&mut self) -> Option<Task> {
                self.0.get(self.1).map(|s| Task::new(*s))
            }
            fn perform(&mut self, _task: &Task) -> Done {
                self.1 += 1;
                Done::ok("did it")
            }
        }

        let mut leg = Then::new(Fixed(vec!["item"], 0), Fixed(vec!["gate"], 0));
        let mut seen = Vec::new();
        while let Some(task) = leg.next() {
            seen.push(task.summary.clone());
            leg.perform(&task);
        }
        assert_eq!(seen, ["item", "gate"], "the first is spent, then the second runs");
        assert!(leg.next().is_none(), "and only then is the leg exhausted — once");
    }

    #[test]
    fn an_outcome_reads_as_a_sequence_of_legs() {
        let outcome = Outcome {
            cycle: 5,
            legs: Vec::new(),
            stop: Some(Stop::BacklogExhausted),
            parked: None,
            spend: Spend { tokens: 1200, seconds: 90, money: 0.0021 },
            ended_in: Phase::D,
        };
        let text = outcome.describe();
        assert!(text.contains("cycle 5"), "{text}");
        assert!(text.contains("backlog is exhausted"), "{text}");
        assert!(text.contains("$0.002100"), "the real number: {text}");
    }
}
