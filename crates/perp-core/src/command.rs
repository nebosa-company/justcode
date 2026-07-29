//! Slash commands (`C-6`) and the evidence chain (`C-7`).
//!
//! **Engine-side, never model-interpreted.** The list below is closed: an
//! unknown slash command is an error, not a prompt. That single rule is doing
//! more work than it looks like — if `/deploy` falls through to the model as
//! text, then a typo, a pasted log line, or a web page quoting a command
//! becomes a request the model may try to satisfy. Here it is a parse failure
//! that names the commands that exist.
//!
//! Commands that change something say so in their own type. Nothing in this
//! module runs anything; it decides what was asked for, which is deliberately a
//! separate question from whether it is allowed.

use std::fmt;

use crate::error::{Error, Result};
use crate::journal::{Kind, Record};
use crate::step::StepId;

/// What the operator typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A slash command, already resolved. Never text.
    Command(Command),
    /// Ordinary conversation, for the model.
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Status,
    Pause,
    Resume,
    /// One step, then stop again.
    Step,
    Approve { id: u64 },
    Reject { id: u64 },
    /// Back to a step, for inspection. Approval-gated, because it discards.
    Rewind { to: StepId },
    Links,
    Cost,
    Board,
    Gate { name: Option<String> },
    Explain { subject: Subject },
    Btw { text: String },
}

/// What `/explain` was pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Requirement(String),
    Step(StepId),
    Sha(String),
}

impl Subject {
    /// Work out which of the three it is from its shape. A requirement is
    /// `X-12`, a step is `c3/b13/s04`, and anything else that is long and
    /// hexadecimal is a sha.
    pub fn parse(text: &str) -> Result<Subject> {
        if let Ok(step) = StepId::parse(text) {
            return Ok(Subject::Step(step));
        }
        if is_requirement_id(text) {
            return Ok(Subject::Requirement(text.to_uppercase()));
        }
        if text.len() >= 7 && text.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(Subject::Sha(text.to_lowercase()));
        }
        Err(Error::refused(
            "/explain",
            format!("`{text}` is not a requirement id, a step id, or a commit sha"),
        ))
    }
}

fn is_requirement_id(text: &str) -> bool {
    let Some((prefix, number)) = text.split_once('-') else { return false };
    prefix.len() == 1
        && prefix.chars().all(|c| c.is_ascii_alphabetic())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

/// Every command, for the error message and for `/help`. One list, so a command
/// cannot exist in the parser and not in the message that says what exists.
pub const NAMES: &[&str] = &[
    "status", "pause", "resume", "step", "approve", "reject", "rewind", "links", "cost", "board",
    "gate", "explain", "btw",
];

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Status => "status",
            Command::Pause => "pause",
            Command::Resume => "resume",
            Command::Step => "step",
            Command::Approve { .. } => "approve",
            Command::Reject { .. } => "reject",
            Command::Rewind { .. } => "rewind",
            Command::Links => "links",
            Command::Cost => "cost",
            Command::Board => "board",
            Command::Gate { .. } => "gate",
            Command::Explain { .. } => "explain",
            Command::Btw { .. } => "btw",
        }
    }

    /// Whether running this changes the workspace or the loop's course. Read
    /// commands stay available while the loop holds the write lock (`C-3`).
    pub fn writes(&self) -> bool {
        match self {
            Command::Status
            | Command::Links
            | Command::Cost
            | Command::Board
            | Command::Explain { .. } => false,
            // `/btw` writes a queue entry and nothing else; it is allowed
            // mid-run on purpose (`C-8`) and cannot act (`C-10`).
            Command::Btw { .. } => false,
            Command::Pause
            | Command::Resume
            | Command::Step
            | Command::Approve { .. }
            | Command::Reject { .. }
            | Command::Rewind { .. }
            | Command::Gate { .. } => true,
        }
    }

    /// Commands a person has to confirm even having typed them. `/rewind`
    /// discards work that is on the record; `/approve` is the approval itself
    /// and must never be reachable from anything but a person (`T-7`).
    pub fn needs_confirmation(&self) -> bool {
        matches!(self, Command::Rewind { .. } | Command::Approve { .. })
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}", self.name())
    }
}

/// Read one line of input.
///
/// A leading `/` means a command or an error — never a prompt.
pub fn parse(line: &str) -> Result<Input> {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return Ok(Input::Message(trimmed.to_string()));
    };

    let (name, argument) = match rest.split_once(char::is_whitespace) {
        Some((name, argument)) => (name, argument.trim()),
        None => (rest, ""),
    };

    let need = |what: &str| {
        Error::refused(format!("/{name}"), format!("takes {what}, and none was given"))
    };
    let number = |what: &str| {
        argument
            .parse::<u64>()
            .map_err(|_| Error::refused(format!("/{name}"), format!("takes {what}, not `{argument}`")))
    };

    let command = match name {
        "status" => Command::Status,
        "pause" => Command::Pause,
        "resume" => Command::Resume,
        "step" => Command::Step,
        "approve" => Command::Approve { id: number("an approval number")? },
        "reject" => Command::Reject { id: number("an approval number")? },
        "rewind" => {
            if argument.is_empty() {
                return Err(need("a step id"));
            }
            Command::Rewind { to: StepId::parse(argument)? }
        }
        "links" => Command::Links,
        "cost" => Command::Cost,
        "board" => Command::Board,
        "gate" => Command::Gate {
            name: (!argument.is_empty()).then(|| argument.to_string()),
        },
        "explain" => {
            if argument.is_empty() {
                return Err(need("a requirement id, a step id, or a commit sha"));
            }
            Command::Explain { subject: Subject::parse(argument)? }
        }
        "btw" => {
            if argument.is_empty() {
                return Err(need("something to note"));
            }
            Command::Btw { text: argument.to_string() }
        }
        other => {
            // The load-bearing branch. Not a prompt, not a guess, not a
            // did-you-mean that runs something.
            return Err(Error::refused(
                format!("/{other}"),
                format!("is not a command. There are {}: {}", NAMES.len(), NAMES.join(", ")),
            ));
        }
    };
    Ok(Input::Command(command))
}

/// The evidence for one decision (`C-7`).
///
/// Assembled from the journal and nothing else. Every field is either something
/// that was recorded or absent — there is no field this can fill in from
/// inference, which is why several of them are `Option` and stay that way.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub subject: String,
    /// The steps that cited it, in order.
    pub steps: Vec<Link>,
    /// Requirement ids the steps carried.
    pub requirements: Vec<String>,
    /// Gate transcripts found on those steps.
    pub transcripts: Vec<String>,
    /// The commit sha, if a transcript was pinned to one (`G-6`).
    pub sha: Option<String>,
    /// Which link wrote it, if a call was journalled on the step (`M-10`).
    pub links: Vec<String>,
    /// The reality check that ran before the work (`V-1`), if one did.
    pub reality_check: Option<String>,
    /// The verifier's verdict and whether it was independent (`V-5`).
    pub verdict: Option<String>,
    /// The diff, read from the repository at the pinned commit — not stored in
    /// the journal. A diff in a record would be a second copy of something git
    /// already keeps, and the two would eventually disagree.
    pub diff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub step: StepId,
    pub summary: String,
    pub ok: Option<bool>,
}

impl Chain {
    /// Walk the journal for everything that touches a subject.
    pub fn build(subject: &Subject, records: &[Record]) -> Chain {
        let matches = |record: &Record| match subject {
            Subject::Requirement(id) => record.requirements.iter().any(|r| r == id),
            Subject::Step(step) => &record.step == step,
            Subject::Sha(sha) => record
                .detail
                .as_deref()
                .map(|detail| detail.contains(sha.as_str()))
                .unwrap_or(false),
        };

        let mut chain = Chain {
            subject: match subject {
                Subject::Requirement(id) => id.clone(),
                Subject::Step(step) => step.to_string(),
                Subject::Sha(sha) => sha.clone(),
            },
            steps: Vec::new(),
            requirements: Vec::new(),
            transcripts: Vec::new(),
            sha: None,
            links: Vec::new(),
            reality_check: None,
            verdict: None,
            diff: None,
        };

        for record in records.iter().filter(|r| matches(r)) {
            if record.kind == Kind::Outcome {
                chain.steps.push(Link {
                    step: record.step.clone(),
                    summary: record.summary.clone(),
                    ok: record.ok,
                });
            }
            for id in &record.requirements {
                if !chain.requirements.contains(id) {
                    chain.requirements.push(id.clone());
                }
            }
            if let Some(detail) = &record.detail {
                if detail.starts_with("gate:") {
                    chain.transcripts.push(detail.clone());
                    if chain.sha.is_none() {
                        chain.sha = sha_from(detail);
                    }
                }
            }
            if let Some(detail) = &record.detail {
                // `V-1` and `V-5` write their own prefixes, so the chain can
                // find them without a schema.
                if detail.starts_with("reality:") && chain.reality_check.is_none() {
                    chain.reality_check = Some(detail.clone());
                }
                if detail.starts_with("verdict:") && chain.verdict.is_none() {
                    chain.verdict = Some(detail.clone());
                }
            }
            if let Some(entry) = crate::cost::from_record(record) {
                if !chain.links.contains(&entry.link) {
                    chain.links.push(entry.link);
                }
            }
        }
        chain
    }

    /// Read the diff from the repository at the pinned commit (`C-7`).
    ///
    /// Read rather than stored. A diff in a journal record would be a second
    /// copy of something git already keeps, and the two would eventually
    /// disagree — which is the failure mode this whole design is built against.
    pub fn with_diff(mut self, repo: &crate::git::Repo) -> Chain {
        if let Some(sha) = &self.sha {
            self.diff = repo.plumbing(&["show", "--stat", "--format=%s", sha]).ok();
        }
        self
    }

    /// Whether there is anything here at all. An empty chain is reported as
    /// empty rather than rendered as a decision with no evidence.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.transcripts.is_empty()
    }

    pub fn render(&self) -> String {
        if self.is_empty() {
            return format!(
                "{}: nothing in the journal cites it.\n\
                 That is an answer, not a gap — a requirement with no steps has not been worked on.\n",
                self.subject
            );
        }

        let mut out = format!("{}\n\n", self.subject);
        if !self.requirements.is_empty() {
            out.push_str(&format!("requirements: {}\n", self.requirements.join(", ")));
        }
        if let Some(sha) = &self.sha {
            out.push_str(&format!("pinned to:    {sha}\n"));
        }
        if !self.links.is_empty() {
            out.push_str(&format!("written by:   {}\n", self.links.join(", ")));
        }
        out.push_str("\nsteps\n");
        for link in &self.steps {
            let mark = match link.ok {
                Some(true) => "ok  ",
                Some(false) => "FAIL",
                None => "    ",
            };
            out.push_str(&format!("  {mark} {} — {}\n", link.step, link.summary));
        }
        match &self.reality_check {
            Some(check) => out.push_str(&format!("
reality check
  {}
", check.trim())),
            None => out.push_str("
no reality check recorded (`V-1`)
"),
        }
        match &self.verdict {
            Some(verdict) => out.push_str(&format!("
verdict
  {}
", verdict.trim())),
            // Said out loud rather than left as an absence: an unreviewed change
            // and a change that passed review look identical if the field is
            // simply missing.
            None => out.push_str("
no verifier verdict recorded (`V-5`)
"),
        }
        if let Some(diff) = &self.diff {
            out.push_str(&format!("
diff
{diff}
"));
        }

        if self.transcripts.is_empty() {
            // Said out loud rather than left as an absence: a claim with no
            // transcript is exactly what `V-2` says not to believe.
            out.push_str("\nno gate transcript on any of these steps\n");
        } else {
            out.push_str(&format!("\n{} gate transcripts\n", self.transcripts.len()));
            for transcript in &self.transcripts {
                out.push('\n');
                out.push_str(transcript.trim_end());
                out.push('\n');
            }
        }
        out
    }
}

fn sha_from(transcript: &str) -> Option<String> {
    transcript
        .lines()
        .find_map(|line| line.trim().strip_prefix("sha:"))
        .map(|sha| sha.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chain_names_what_is_missing_rather_than_omitting_it() {
        // `C-7` lists six things. An unreviewed change and one that passed
        // review look identical if the field is simply absent, so the absence
        // is printed.
        let records = vec![Record::outcome(step(1), 200, true, "done").for_requirements(["C-7"])];
        let rendered = Chain::build(&Subject::Requirement("C-7".into()), &records).render();
        assert!(rendered.contains("no reality check recorded"), "{rendered}");
        assert!(rendered.contains("no verifier verdict recorded"), "{rendered}");
    }

    #[test]
    fn a_recorded_reality_check_and_verdict_reach_the_chain() {
        let records = vec![
            Record::outcome(step(1), 100, true, "checked")
                .for_requirements(["C-7"])
                .with_detail("reality: the file exists and the function is not there"),
            Record::outcome(step(2), 200, true, "reviewed")
                .for_requirements(["C-7"])
                .with_detail("verdict: ds-fast reviewed what here wrote — no objection"),
        ];
        let chain = Chain::build(&Subject::Requirement("C-7".into()), &records);
        assert!(chain.reality_check.is_some());
        assert!(chain.verdict.is_some());

        let rendered = chain.render();
        assert!(rendered.contains("ds-fast reviewed what here wrote"), "{rendered}");
        assert!(!rendered.contains("no verifier verdict"), "{rendered}");
    }

    fn step(n: u32) -> StepId {
        StepId::new(3, "b13", n).expect("step")
    }

    #[test]
    fn an_unknown_slash_command_is_an_error_not_a_prompt() {
        let err = parse("/deploy production").expect_err("must not fall through to the model");
        let text = format!("{err}");
        assert!(text.contains("/deploy"), "{text}");
        assert!(text.contains("is not a command"), "{text}");
        assert!(text.contains("status"), "and says what does exist: {text}");
    }

    #[test]
    fn every_documented_command_parses() {
        let lines = [
            "/status",
            "/pause",
            "/resume",
            "/step",
            "/approve 4",
            "/reject 4",
            "/rewind c3/b13/s02",
            "/links",
            "/cost",
            "/board",
            "/gate",
            "/gate lint",
            "/explain L-3",
            "/btw the terminal font is too small",
        ];
        for line in lines {
            assert!(matches!(parse(line), Ok(Input::Command(_))), "did not parse: {line}");
        }
        // And the list in the error message matches the list that parses.
        assert_eq!(NAMES.len(), 13);
    }

    #[test]
    fn text_without_a_slash_is_a_message() {
        assert_eq!(
            parse("what happened in cycle 2?").expect("parse"),
            Input::Message("what happened in cycle 2?".into())
        );
        // Including text that mentions a command.
        assert!(matches!(parse("should I /pause it?").expect("parse"), Input::Message(_)));
    }

    #[test]
    fn a_command_missing_its_argument_says_what_it_wanted() {
        let err = parse("/explain").expect_err("needs a subject");
        assert!(format!("{err}").contains("commit sha"), "{err}");
        let err = parse("/approve").expect_err("needs a number");
        assert!(format!("{err}").contains("approval number"), "{err}");
        let err = parse("/rewind yesterday").expect_err("needs a step id");
        assert!(format!("{err}").contains("step id"), "{err}");
    }

    #[test]
    fn explain_tells_the_three_subjects_apart() {
        assert_eq!(Subject::parse("L-14").expect("id"), Subject::Requirement("L-14".into()));
        assert_eq!(Subject::parse("c3/b13/s04").expect("step"), Subject::Step(step(4)));
        assert_eq!(
            Subject::parse("a7b2092").expect("sha"),
            Subject::Sha("a7b2092".into())
        );
        assert!(Subject::parse("last tuesday").is_err(), "and refuses what it cannot resolve");
    }

    #[test]
    fn read_commands_stay_available_while_the_loop_writes() {
        for line in ["/status", "/cost", "/explain L-3", "/btw a thought"] {
            let Ok(Input::Command(command)) = parse(line) else { panic!("{line}") };
            assert!(!command.writes(), "{line} must be usable mid-run (`C-3`)");
        }
        for line in ["/pause", "/rewind c3/b13/s01", "/approve 1"] {
            let Ok(Input::Command(command)) = parse(line) else { panic!("{line}") };
            assert!(command.writes(), "{line} changes the loop's course");
        }
    }

    #[test]
    fn discarding_and_approving_both_need_a_person_to_confirm() {
        let Ok(Input::Command(rewind)) = parse("/rewind c3/b13/s01") else { panic!() };
        let Ok(Input::Command(approve)) = parse("/approve 2") else { panic!() };
        let Ok(Input::Command(status)) = parse("/status") else { panic!() };
        assert!(rewind.needs_confirmation());
        assert!(approve.needs_confirmation());
        assert!(!status.needs_confirmation());
    }

    #[test]
    fn the_evidence_chain_is_assembled_from_the_journal() {
        let transcript = "gate: test\nsha: a7b2092\n$ cargo test\nexit 0\n";
        let records = vec![
            Record::intent(step(1), 100, "wire the driver")
                .for_requirements(["L-1", "L-2"]),
            Record::outcome(step(1), 200, true, "driver wired")
                .for_requirements(["L-1", "L-2"]),
            Record::outcome(step(2), 300, true, "gate test is green")
                .for_requirements(["L-1"])
                .with_detail(transcript),
            Record::outcome(StepId::new(9, "zz", 1).expect("other"), 400, true, "unrelated"),
        ];

        let chain = Chain::build(&Subject::Requirement("L-1".into()), &records);
        assert_eq!(chain.steps.len(), 2, "both outcomes citing L-1");
        assert_eq!(chain.sha.as_deref(), Some("a7b2092"), "pinned to the commit (`G-6`)");
        assert_eq!(chain.transcripts.len(), 1);

        let rendered = chain.render();
        assert!(rendered.contains("cargo test"), "the transcript is verbatim:\n{rendered}");
        assert!(rendered.contains("L-2"), "and the other ids the steps carried");
    }

    #[test]
    fn a_requirement_nobody_worked_on_says_so_rather_than_looking_thin() {
        let chain = Chain::build(&Subject::Requirement("X-9".into()), &[]);
        assert!(chain.is_empty());
        let rendered = chain.render();
        assert!(rendered.contains("nothing in the journal cites it"), "{rendered}");
    }

    #[test]
    fn a_claim_with_no_transcript_is_called_out() {
        let records =
            vec![Record::outcome(step(1), 200, true, "done").for_requirements(["L-9"])];
        let rendered = Chain::build(&Subject::Requirement("L-9".into()), &records).render();
        assert!(rendered.contains("no gate transcript"), "`V-2` is visible in the output:\n{rendered}");
    }

    #[test]
    fn explaining_a_sha_finds_the_steps_pinned_to_it() {
        let records = vec![
            Record::outcome(step(1), 200, true, "gate lint is green")
                .with_detail("gate: lint\nsha: 567114b\nexit 0\n"),
            Record::outcome(step(2), 300, true, "something else"),
        ];
        let chain = Chain::build(&Subject::Sha("567114b".into()), &records);
        assert_eq!(chain.steps.len(), 1);
        assert_eq!(chain.steps[0].step, step(1));
    }
}
