//! The journal (`L-3`).
//!
//! One line per record, append-only, never rewritten. A step writes its
//! **intent** before the side effect and its **outcome** after; a run that dies
//! in between leaves an intent with no outcome, which is exactly the evidence
//! recovery needs (`L-7`) and the reason the two are separate records rather
//! than one written at the end.
//!
//! Unknown fields are preserved on the way through (`N-8`): a journal written
//! by a later version must still be readable, and readable means *unchanged*,
//! not "the parts we recognise".

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::json::{self, Value};
use crate::step::StepId;

/// Bumped when the record shape changes in a way an older reader cannot handle.
pub const FORMAT_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Intent,
    Outcome,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Intent => "intent",
            Kind::Outcome => "outcome",
        }
    }

    fn parse(text: &str) -> Option<Kind> {
        match text {
            "intent" => Some(Kind::Intent),
            "outcome" => Some(Kind::Outcome),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub version: i64,
    pub step: StepId,
    pub kind: Kind,
    /// Epoch seconds, supplied by the caller so tests are not clock-dependent.
    pub at: i64,
    pub summary: String,
    /// Requirement ids this step serves. Cited, never minted (`V-9`).
    pub requirements: Vec<String>,
    /// Outcomes only.
    pub ok: Option<bool>,
    /// Verbatim evidence — a gate transcript, or the actual error text (`L-16`).
    pub detail: Option<String>,
    /// Fields this version does not know about, kept so they survive (`N-8`).
    pub extra: Vec<(String, Value)>,
}

impl Record {
    pub fn intent(step: StepId, at: i64, summary: impl Into<String>) -> Record {
        Record {
            version: FORMAT_VERSION,
            step,
            kind: Kind::Intent,
            at,
            summary: summary.into(),
            requirements: Vec::new(),
            ok: None,
            detail: None,
            extra: Vec::new(),
        }
    }

    pub fn outcome(step: StepId, at: i64, ok: bool, summary: impl Into<String>) -> Record {
        Record {
            version: FORMAT_VERSION,
            step,
            kind: Kind::Outcome,
            at,
            summary: summary.into(),
            requirements: Vec::new(),
            ok: Some(ok),
            detail: None,
            extra: Vec::new(),
        }
    }

    pub fn for_requirements<I, S>(mut self, ids: I) -> Record
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.requirements = ids.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Record {
        self.detail = Some(detail.into());
        self
    }

    /// Declare whether repeating this step is safe (`L-5`).
    ///
    /// Carried as an extra field rather than a new column: a reader that has
    /// never heard of it still round-trips the record (`N-8`), and a record
    /// that does not carry it is treated as unsafe to repeat, which is the
    /// conservative reading.
    pub fn idempotent(mut self, idempotent: bool) -> Record {
        self.extra.retain(|(key, _)| key != "idempotent");
        self.extra.push(("idempotent".to_string(), Value::Bool(idempotent)));
        self
    }

    pub fn is_idempotent(&self) -> Option<bool> {
        self.extra
            .iter()
            .find(|(key, _)| key == "idempotent")
            .and_then(|(_, value)| value.as_bool())
    }

    pub fn to_value(&self) -> Value {
        let mut pairs = vec![
            ("v".to_string(), Value::int(self.version)),
            ("at".to_string(), Value::int(self.at)),
            ("step".to_string(), Value::str(self.step.to_string())),
            ("kind".to_string(), Value::str(self.kind.as_str())),
            ("summary".to_string(), Value::str(self.summary.clone())),
        ];
        if !self.requirements.is_empty() {
            let ids = self.requirements.iter().map(Value::str).collect();
            pairs.push(("requirements".to_string(), Value::Arr(ids)));
        }
        if let Some(ok) = self.ok {
            pairs.push(("ok".to_string(), Value::Bool(ok)));
        }
        if let Some(detail) = &self.detail {
            pairs.push(("detail".to_string(), Value::str(detail.clone())));
        }
        pairs.extend(self.extra.iter().cloned());
        Value::Obj(pairs)
    }

    pub fn from_value(value: &Value, line: usize) -> Result<Record> {
        let fail = |reason: &str| Error::Record { line, reason: reason.to_string() };

        let Value::Obj(pairs) = value else {
            return Err(fail("not a JSON object"));
        };

        let version = value.get("v").and_then(Value::as_i64).ok_or_else(|| fail("no `v`"))?;
        let at = value.get("at").and_then(Value::as_i64).ok_or_else(|| fail("no `at`"))?;
        let step = value
            .get("step")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("no `step`"))
            .and_then(|s| StepId::parse(s).map_err(|e| fail(&e.to_string())))?;
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .and_then(Kind::parse)
            .ok_or_else(|| fail("`kind` is not `intent` or `outcome`"))?;
        let summary = value
            .get("summary")
            .and_then(Value::as_str)
            .ok_or_else(|| fail("no `summary`"))?
            .to_string();

        let requirements = match value.get("requirements") {
            None => Vec::new(),
            Some(v) => v
                .as_arr()
                .ok_or_else(|| fail("`requirements` is not an array"))?
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| fail("a requirement id is not a string"))
                })
                .collect::<Result<Vec<_>>>()?,
        };

        let known = ["v", "at", "step", "kind", "summary", "requirements", "ok", "detail"];
        let extra = pairs
            .iter()
            .filter(|(k, _)| !known.contains(&k.as_str()))
            .cloned()
            .collect();

        Ok(Record {
            version,
            step,
            kind,
            at,
            summary,
            requirements,
            ok: value.get("ok").and_then(Value::as_bool),
            detail: value.get("detail").and_then(Value::as_str).map(str::to_string),
            extra,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn at(path: impl Into<PathBuf>) -> Journal {
        Journal { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one record. Not an atomic rewrite (`N-10` is for projections) —
    /// appending is what makes the journal append-only.
    pub fn append(&self, record: &Record) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut line = json::to_string(&record.to_value());
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| Error::io(&self.path, e))?;
        file.write_all(line.as_bytes()).map_err(|e| Error::io(&self.path, e))?;
        file.sync_all().map_err(|e| Error::io(&self.path, e))
    }

    /// Every record, in the order they were written. A journal that has never
    /// been written is empty, not an error — that is a loop that has not begun.
    pub fn read_all(&self) -> Result<Vec<Record>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(&self.path, e)),
        };

        let mut records = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let line_no = index + 1;
            let value = json::parse(line).map_err(|e| Error::Record {
                line: line_no,
                reason: e.to_string(),
            })?;
            records.push(Record::from_value(&value, line_no)?);
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    fn step(text: &str) -> StepId {
        StepId::parse(text).expect("step id")
    }

    #[test]
    fn appends_and_reads_back_in_order() {
        let dir = tmpdir("journal-order");
        let journal = Journal::at(dir.join("journal.jsonl"));

        journal
            .append(&Record::intent(step("c1/b1/s01"), 1_700_000_000, "load the binding"))
            .expect("append intent");
        journal
            .append(
                &Record::outcome(step("c1/b1/s01"), 1_700_000_005, true, "bound")
                    .for_requirements(["L-21"]),
            )
            .expect("append outcome");

        let records = journal.read_all().expect("read");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].kind, Kind::Intent);
        assert_eq!(records[1].kind, Kind::Outcome);
        assert_eq!(records[1].ok, Some(true));
        assert_eq!(records[1].requirements, vec!["L-21".to_string()]);
    }

    #[test]
    fn an_absent_journal_is_an_empty_one() {
        let dir = tmpdir("journal-absent");
        let records = Journal::at(dir.join("nothing-here.jsonl")).read_all().expect("read");
        assert!(records.is_empty());
    }

    #[test]
    fn appending_never_rewrites_what_is_there() {
        let dir = tmpdir("journal-append-only");
        let path = dir.join("journal.jsonl");
        let journal = Journal::at(&path);
        journal
            .append(&Record::intent(step("c1/b1/s01"), 1, "first"))
            .expect("append");
        let after_first = std::fs::read_to_string(&path).expect("read");
        journal
            .append(&Record::intent(step("c1/b1/s02"), 2, "second"))
            .expect("append");
        let after_second = std::fs::read_to_string(&path).expect("read");
        assert!(
            after_second.starts_with(&after_first),
            "the first record must survive byte-for-byte"
        );
    }

    #[test]
    fn keeps_verbatim_error_text_on_one_line() {
        let dir = tmpdir("journal-verbatim");
        let path = dir.join("journal.jsonl");
        let journal = Journal::at(&path);
        let transcript = "error[E0308]: mismatched types\n  --> src/main.rs:4:9\n   |\n   = note: \"expected\"";
        journal
            .append(
                &Record::outcome(step("c1/b1/s03"), 3, false, "build failed")
                    .with_detail(transcript),
            )
            .expect("append");

        let raw = std::fs::read_to_string(&path).expect("read");
        assert_eq!(raw.lines().count(), 1, "a multi-line error stays one record");

        let records = journal.read_all().expect("read");
        assert_eq!(records[0].detail.as_deref(), Some(transcript), "verbatim, not summarised");
    }

    #[test]
    fn preserves_fields_from_a_later_version() {
        let dir = tmpdir("journal-forward");
        let path = dir.join("journal.jsonl");
        std::fs::write(
            &path,
            "{\"v\":9,\"at\":7,\"step\":\"c1/b1/s01\",\"kind\":\"intent\",\"summary\":\"from the future\",\"link\":\"rig\",\"cost\":{\"in\":12}}\n",
        )
        .expect("write");

        let records = Journal::at(&path).read_all().expect("read");
        assert_eq!(records[0].version, 9);
        assert_eq!(records[0].extra.len(), 2, "unknown fields are kept, not dropped");

        let round_tripped = json::to_string(&records[0].to_value());
        assert!(round_tripped.contains("\"link\":\"rig\""), "{round_tripped}");
        assert!(round_tripped.contains("\"cost\":{\"in\":12}"), "{round_tripped}");
    }

    #[test]
    fn a_broken_line_names_its_line_number() {
        let dir = tmpdir("journal-broken");
        let path = dir.join("journal.jsonl");
        std::fs::write(
            &path,
            "{\"v\":1,\"at\":1,\"step\":\"c1/b1/s01\",\"kind\":\"intent\",\"summary\":\"fine\"}\n\
             not json at all\n",
        )
        .expect("write");
        let err = Journal::at(&path).read_all().expect_err("must fail");
        assert!(format!("{err}").contains("line 2"), "{err}");
    }

    #[test]
    fn a_record_missing_a_field_is_rejected() {
        let dir = tmpdir("journal-incomplete");
        let path = dir.join("journal.jsonl");
        std::fs::write(&path, "{\"v\":1,\"at\":1,\"kind\":\"intent\",\"summary\":\"no step\"}\n")
            .expect("write");
        let err = Journal::at(&path).read_all().expect_err("must fail");
        assert!(format!("{err}").contains("no `step`"), "{err}");
    }
}
