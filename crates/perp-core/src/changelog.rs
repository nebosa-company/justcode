//! Release notes, derived from the commits themselves (`O-14`).
//!
//! `.harness/release-notes.md` has been hand-written prose since cycle 1 —
//! true, but the same self-report `V-2` refuses everywhere else: a person or
//! a model summarising what it did is not evidence that it did it. `O-13`
//! already solved this once for the decision log, by deriving it from the
//! journal instead of asserting it. This is the same move over commits: a
//! range of git history, grouped by the `Requirement:` trailer (`G-2`) each
//! commit already carries, and nothing else. Nobody narrates; the trailer
//! either names a requirement or it does not.
//!
//! A commit with no trailer is not dropped for tidiness. It is listed apart,
//! under its own heading — `T-6`'s reasoning about truncation applies just as
//! well to an omission dressed as a clean list: a changelog that quietly
//! folds untracked work into whichever requirement precedes it would read as
//! complete and be wrong.

use crate::cycle::backlog_all;
use std::collections::BTreeMap;

/// Between commits, in [`crate::git::Repo::log_between`]'s raw output.
pub const RECORD_SEP: char = '\u{1e}';
/// Between `sha`, `subject` and `body` within one commit's record.
pub const FIELD_SEP: char = '\u{1f}';

/// One commit's worth of what a changelog needs, and nothing it would have to
/// re-derive later — the subject for the reader, the requirements for the
/// grouping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub sha: String,
    pub subject: String,
    pub requirements: Vec<String>,
}

/// Parse [`crate::git::Repo::log_between`]'s output into one entry per commit.
///
/// The `Requirement:` trailer is read from the body's own text rather than a
/// second `git log --grep`, so a commit that cites nothing is a body with no
/// such line — a fact already in the data, not a second query that could
/// disagree with the first.
pub fn parse(raw: &str) -> Vec<Entry> {
    raw.split(RECORD_SEP)
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let mut fields = record.splitn(3, FIELD_SEP);
            let sha = fields.next().unwrap_or_default().trim().to_string();
            let subject = fields.next().unwrap_or_default().trim().to_string();
            let body = fields.next().unwrap_or_default();
            let requirements = body
                .lines()
                .find_map(|line| line.trim().strip_prefix("Requirement:"))
                .map(|ids| {
                    ids.split(',').map(str::trim).filter(|id| !id.is_empty()).map(String::from).collect()
                })
                .unwrap_or_default();
            Entry { sha, subject, requirements }
        })
        .collect()
}

/// Group `entries` by the requirement(s) each cites, with the requirement's
/// own text from `source` (`.harness/perpetum.md`) — the same lookup `perp
/// explain` uses, so a reader sees what `O-10`'s fork was *for* and not just
/// its id. A commit citing more than one requirement appears under each: it
/// is not a partial answer to any of them.
pub fn render(entries: &[Entry], source: &str) -> String {
    let descriptions: BTreeMap<String, String> = backlog_all(source).into_iter().collect();

    let mut by_requirement: BTreeMap<String, Vec<&Entry>> = BTreeMap::new();
    let mut unattributed: Vec<&Entry> = Vec::new();

    for entry in entries {
        if entry.requirements.is_empty() {
            unattributed.push(entry);
            continue;
        }
        for id in &entry.requirements {
            by_requirement.entry(id.clone()).or_default().push(entry);
        }
    }

    let mut out = String::new();
    for (id, commits) in &by_requirement {
        match descriptions.get(id) {
            Some(summary) => out.push_str(&format!("### `{id}` — {summary}\n")),
            None => out.push_str(&format!("### `{id}`\n")),
        }
        for commit in commits {
            out.push_str(&format!("- {} (`{}`)\n", commit.subject, short(&commit.sha)));
        }
        out.push('\n');
    }

    if !unattributed.is_empty() {
        out.push_str("### Not attributed to a requirement\n");
        for commit in &unattributed {
            out.push_str(&format!("- {} (`{}`)\n", commit.subject, short(&commit.sha)));
        }
        out.push('\n');
    }

    out
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "| ✅ ~~`L-3`~~ | The journal is append-only. |\n\
                           | ✅ ~~`L-4`~~ | State is a projection, rewritten each step. |\n";

    fn record(sha: &str, subject: &str, body: &str) -> String {
        format!("{sha}{FIELD_SEP}{subject}{FIELD_SEP}{body}{RECORD_SEP}")
    }

    /// `G-2`'s own example: one commit citing two requirements.
    #[test]
    fn a_commit_can_cite_more_than_one_requirement() {
        let raw = record(
            "abc123",
            "Add the journal writer",
            "some body\n\nRequirement: L-3, L-4\nPerpetum-Step: c1/b1/s07\n",
        );
        let entries = parse(&raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].requirements, vec!["L-3".to_string(), "L-4".to_string()]);
    }

    /// A trailer-less commit parses to an empty list, not an error — the
    /// absence is the fact, and `render` is what does something with it.
    #[test]
    fn a_commit_with_no_trailer_cites_nothing() {
        let raw = record("def456", "Fix a typo in a comment", "just a typo\n");
        let entries = parse(&raw);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].requirements.is_empty());
    }

    #[test]
    fn multiple_commits_in_one_range_all_parse() {
        let raw = record("aaa", "First", "Requirement: L-3\n")
            + &record("bbb", "Second", "Requirement: L-4\n");
        let entries = parse(&raw);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sha, "aaa");
        assert_eq!(entries[1].sha, "bbb");
    }

    /// The requirement's own text renders beside its id, same lookup `perp
    /// explain` already uses.
    #[test]
    fn render_groups_by_requirement_and_shows_its_text() {
        let entries = vec![Entry {
            sha: "1234567890".into(),
            subject: "Add the journal writer".into(),
            requirements: vec!["L-3".into()],
        }];
        let out = render(&entries, SOURCE);
        assert!(out.contains("`L-3` — The journal is append-only."), "{out}");
        assert!(out.contains("Add the journal writer"), "{out}");
        assert!(out.contains("`1234567`"), "sha is shortened: {out}");
    }

    /// A commit naming two requirements is not split between them — it
    /// appears under both, in full.
    #[test]
    fn a_commit_citing_two_requirements_appears_under_both() {
        let entries = vec![Entry {
            sha: "abc".into(),
            subject: "Both at once".into(),
            requirements: vec!["L-3".into(), "L-4".into()],
        }];
        let out = render(&entries, SOURCE);
        assert!(out.contains("`L-3`"), "{out}");
        assert!(out.contains("`L-4`"), "{out}");
        assert_eq!(out.matches("Both at once").count(), 2, "{out}");
    }

    /// `T-6`: an untracked commit is not silently absorbed into a
    /// neighbouring requirement's section — it gets its own, so it stays
    /// visible.
    #[test]
    fn a_commit_with_no_requirement_is_listed_apart_not_dropped() {
        let entries = vec![
            Entry { sha: "a".into(), subject: "Tracked".into(), requirements: vec!["L-3".into()] },
            Entry { sha: "b".into(), subject: "Untracked cleanup".into(), requirements: vec![] },
        ];
        let out = render(&entries, SOURCE);
        assert!(out.contains("Not attributed to a requirement"), "{out}");
        assert!(out.contains("Untracked cleanup"), "{out}");
    }

    /// A requirement id the source no longer explains (renumbered, or from a
    /// document this changelog was not given) still renders — with its id and
    /// no summary, rather than being dropped for want of a description.
    #[test]
    fn an_unknown_requirement_id_still_renders() {
        let entries = vec![Entry { sha: "a".into(), subject: "x".into(), requirements: vec!["Z-99".into()] }];
        let out = render(&entries, SOURCE);
        assert!(out.contains("`Z-99`"), "{out}");
    }
}
