//! The four ways a **person** may change the requirements source (`O-18`,
//! `V-12`, `L-34`).
//!
//! `V-12` reserves writes to the requirements source to a person: the loop's
//! host refuses a write to the path the binding names, and that refusal is not
//! relaxed here. What this module is for is the other side of that line — the
//! surfaces a person acts through. There are three of them now (the CLI, the
//! editor's panel, the web front end), and every one of them is a door into the
//! same file.
//!
//! **So the allowlist is the type, not a string comparison in each door.** A
//! browser posting file contents to a Rust binary that wrote them through would
//! defeat `V-12` completely — the person would be the loop's proxy rather than
//! its author. Instead every door parses its request into a [`Write`], and a
//! request that does not parse is refused by name:
//!
//! - `requirement add "<text>"` — files a row with **no marker**
//! - `requirement edit <id> "<text>"` — replaces a row's text cell, only
//! - `requirement delete <id>` — removes a row
//! - `ungate <id>` — clears a `⛔`
//!
//! There is no fifth, and in particular there is no *set state*. Nothing in
//! here can write a `✅`: [`Write::Edit`] rebuilds a row from its own status
//! cell taken verbatim, so the marker a row carries is the marker it keeps.
//! That is `V-2` — a green means gates passed with a transcript, and a marker
//! a front end can type is a marker worth nothing.

use std::path::{Path, PathBuf};

use crate::atomic::write_atomic;
use crate::cycle;
use crate::error::{Error, Result};

/// The section a filed requirement lands in, created if it is not there.
///
/// A new row never joins an existing table. Every table in a requirements
/// source sits under a heading that says something about the rows in it —
/// which slice they belong to, what is deliberately excluded, what gates them —
/// and appending to whichever table happened to be last would silently give a
/// filed requirement that heading's meaning.
pub const FILED_SECTION: &str = "## Filed in the app";

/// What every door may ask for, and the whole of it (`L-34`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// File a new requirement. The id is minted, never supplied.
    Add { text: String },
    /// Replace what a row says. Never what state it is in.
    Edit { id: String, text: String },
    /// Remove a row.
    Delete { id: String },
    /// Clear a `⛔`, because a person decided (`O-17`).
    Ungate { id: String, choose: Option<String> },
}

impl Write {
    /// Every write a person may ask for, spelled the way a door names it.
    ///
    /// Down to the verb: `requirement` alone was the editor panel's whole
    /// allowlist entry, which let `requirement delete` through a door opened
    /// for `requirement add`. A subcommand is not a permission.
    pub const ALLOWED: [&'static str; 4] =
        ["requirement add", "requirement edit", "requirement delete", "ungate"];

    /// Parse a door's request, or refuse it naming what is allowed.
    ///
    /// `args` is positional and flagless apart from `--choose`, because this is
    /// the surface a browser reaches: every extra shape here is a shape three
    /// doors have to agree on.
    pub fn parse(subcommand: &str, args: &[&str]) -> Result<Write> {
        match (subcommand, args) {
            ("requirement", ["add", text, ..]) => {
                Ok(Write::Add { text: checked_text(text)? })
            }
            ("requirement", ["edit", id, text, ..]) => {
                Ok(Write::Edit { id: checked_id(id)?, text: checked_text(text)? })
            }
            ("requirement", ["delete", id, ..]) => Ok(Write::Delete { id: checked_id(id)? }),
            ("ungate", [id, rest @ ..]) if !id.starts_with("--") => Ok(Write::Ungate {
                id: checked_id(id)?,
                choose: rest
                    .iter()
                    .position(|arg| *arg == "--choose")
                    .and_then(|at| rest.get(at + 1))
                    .map(|choice| (*choice).to_string()),
            }),
            _ => Err(Error::refused(
                "write",
                format!(
                    "`{subcommand} {}` is not one of the writes a person may make. \
                     Only {} change anything here.",
                    args.join(" "),
                    Write::ALLOWED.join(", ")
                ),
            )),
        }
    }

    /// Do it, against the resolved `path.requirements` — which may name a
    /// directory rather than a file (`O-18`).
    pub fn apply(&self, resolved: &Path) -> Result<Done> {
        match self {
            Write::Add { text } => add(resolved, text),
            Write::Edit { id, text } => edit(resolved, id, text),
            Write::Delete { id } => delete(resolved, id),
            Write::Ungate { id, choose } => ungate(resolved, id, choose.as_deref()),
        }
    }
}

/// What happened, in the words a person reading it wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub id: String,
    /// The file that actually changed. Worth saying: a directory source has
    /// several, and which one a write landed in is not guessable.
    pub source: PathBuf,
    pub summary: String,
    /// The thing a person needs to know and did not ask about — the row that
    /// was deleted was marked done, the gate's prose is now stale. `None` when
    /// there is nothing to say, rather than a reassuring sentence.
    pub note: Option<String>,
}

/// A requirement's text, checked before it can end a table row.
///
/// A `|` closes the cell it is written into and a line break closes the row, so
/// either one turns a requirement into two malformed ones. Caught here because
/// this is the one place all three doors pass through.
fn checked_text(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::refused(
            "requirement",
            "a requirement with no text is not a requirement",
        ));
    }
    if text.contains('|') || text.contains('\n') || text.contains('\r') {
        return Err(Error::refused(
            "requirement",
            "a requirement may not contain `|` or a line break — both end a table row",
        ));
    }
    Ok(text.to_string())
}

/// An id, checked before it is used to find a row.
fn checked_id(id: &str) -> Result<String> {
    let id = id.trim().trim_matches('`').trim();
    if !cycle::is_requirement_id(id) {
        return Err(Error::refused(
            "requirement",
            format!("`{id}` is not a requirement id — they look like `L-34`"),
        ));
    }
    Ok(id.to_string())
}

// --------------------------------------------------------------------- rows

/// One row of a requirements file, located rather than parsed: which file, and
/// which line of it.
struct Row {
    source: PathBuf,
    text: String,
    at: usize,
}

impl Row {
    /// The row's cells, as they are written. `[0]` is empty for a row that
    /// opens with `|`, `[1]` is the status cell and `[2]` is the requirement.
    fn cells(&self) -> Vec<&str> {
        self.line().split('|').collect()
    }

    fn line(&self) -> &str {
        self.text.lines().nth(self.at).unwrap_or_default()
    }

    fn state(&self) -> &'static str {
        cycle::row_state(self.cells().get(1).copied().unwrap_or_default())
    }

    fn says(&self) -> String {
        self.cells().get(2).copied().unwrap_or_default().trim().to_string()
    }

    /// The whole file with line `at` replaced, or removed when `line` is
    /// `None`. Line endings are preserved by rebuilding from the original
    /// split, which `str::lines` cannot do — hence [`rejoin`].
    fn rewritten(&self, line: Option<String>) -> String {
        let mut lines: Vec<String> = self.text.lines().map(str::to_string).collect();
        match line {
            Some(replacement) => lines[self.at] = replacement,
            None => {
                lines.remove(self.at);
            }
        }
        rejoin(&self.text, &lines)
    }
}

/// Put lines back together the way the file had them.
///
/// `str::lines` drops the endings, and writing `\n` back into a file that used
/// `\r\n` rewrites every line of it — a one-word edit arriving as a diff of the
/// whole document, on the one platform this project is tested on first (`N-4`).
fn rejoin(original: &str, lines: &[String]) -> String {
    let ending = if original.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = lines.join(ending);
    if original.ends_with('\n') && !out.is_empty() {
        out.push_str(ending);
    }
    out
}

/// Find the row for `id`, in whichever file of the source holds it.
///
/// By the **status cell**, not by searching for the id: `L-34` appears in the
/// prose of `L-33` and in a heading and in this comment, and a write that
/// matched any of those would edit the wrong requirement or none.
fn locate(resolved: &Path, id: &str) -> Result<Row> {
    for source in sources_of(resolved)? {
        let Ok(text) = std::fs::read_to_string(&source) else { continue };
        for (at, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                continue;
            }
            let mut cells = trimmed.trim_matches('|').split('|');
            let (Some(first), Some(_)) = (cells.next(), cells.next()) else { continue };
            if cycle::row_id(first).as_deref() == Some(id) {
                return Ok(Row { source, text, at });
            }
        }
    }
    Err(Error::refused(
        "requirement",
        format!("`{id}` is not a row in {} — nothing to change", resolved.display()),
    ))
}

/// Every file the source is made of. One, unless it names a directory (`O-18`).
fn sources_of(resolved: &Path) -> Result<Vec<PathBuf>> {
    if resolved.is_file() {
        return Ok(vec![resolved.to_path_buf()]);
    }
    if resolved.is_dir() {
        return Ok(crate::layout::requirement_sources(resolved));
    }
    Err(Error::refused(
        "requirement",
        format!("{}: no requirements source there", resolved.display()),
    ))
}

/// Which file a *new* row lands in, when `path.requirements` may name a
/// directory (`O-18`).
///
/// A file resolves to itself. A directory resolves to whichever file already
/// carries `needle`, so a second write joins the first, and otherwise to
/// `filed.md` — a new file rather than an arbitrary existing one, because every
/// file in there is somebody's chapter and appending to whichever sorted first
/// is a coin toss with a person's document.
pub fn writable_source(resolved: &Path, needle: &str) -> Result<PathBuf> {
    if resolved.is_file() {
        return Ok(resolved.to_path_buf());
    }
    if !resolved.is_dir() {
        return Err(Error::refused(
            "requirement",
            format!("{}: no requirements source there", resolved.display()),
        ));
    }
    for path in crate::layout::requirement_sources(resolved) {
        if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(needle)) {
            return Ok(path);
        }
    }
    Ok(resolved.join("filed.md"))
}

// ------------------------------------------------------------------- writes

/// The next unused id, in the prefix the source already uses (`O-18`).
///
/// Highest number plus one rather than lowest gap: a gap is usually a row
/// somebody deleted, and handing its id to something unrelated makes every
/// older reference to it point at the wrong requirement. Which is exactly why
/// [`Write::Delete`] leaves one.
pub fn next_requirement_id(source: &str) -> Option<String> {
    let mut prefix: Option<String> = None;
    let mut highest = 0u32;
    for entry in cycle::catalogue(source) {
        let (found, number) = entry.id.split_once('-')?;
        let number: u32 = number.parse().ok()?;
        if prefix.is_none() {
            prefix = Some(found.to_string());
        }
        if prefix.as_deref() == Some(found) && number > highest {
            highest = number;
        }
    }
    prefix.map(|prefix| format!("{prefix}-{}", highest + 1))
}

fn add(resolved: &Path, text: &str) -> Result<Done> {
    // The id counts against every file of the source, and the row is written to
    // one of them. A workspace whose requirements are a directory would
    // otherwise mint an id that a sibling file already uses.
    let whole = crate::layout::requirements_text(resolved);
    let id = next_requirement_id(&whole).ok_or_else(|| {
        Error::refused(
            "requirement",
            format!("{}: no requirement id to count from", resolved.display()),
        )
    })?;
    let row = format!("| `{id}` | {text} |\n");

    let source = writable_source(resolved, FILED_SECTION)?;
    let existing = if source.exists() {
        std::fs::read_to_string(&source).map_err(|e| Error::io(&source, e))?
    } else {
        String::new()
    };

    let mut updated = existing.clone();
    if updated.contains(FILED_SECTION) {
        // Append under the heading's table, which is the end of the file only
        // because the section is always written last.
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&row);
    } else {
        // One blank line between the section and whatever came before it, and
        // none at the top of a file this call is creating.
        if !updated.is_empty() {
            if !updated.ends_with('\n') {
                updated.push('\n');
            }
            updated.push('\n');
        }
        updated.push_str(&format!(
            "{FILED_SECTION}\n\nFiled by a person from the panel, not by the loop. These rows \
             carry no slice's gating: they say what should happen and nothing about when.\n\n\
             | id | Requirement |\n|---|---|\n{row}"
        ));
    }
    write_atomic(&source, &updated)?;
    Ok(Done { id: id.clone(), source, summary: format!("filed {id}: {text}"), note: None })
}

/// Replace a row's text cell and **only** its text cell (`L-34`).
///
/// The row is rebuilt from its own cells with index 2 swapped, so the status
/// cell survives byte for byte. That is what keeps `V-2` true through an edit:
/// the marker is not re-emitted from a parsed state, it is never touched, and
/// no path through this function can write a `✅` that was not already there.
fn edit(resolved: &Path, id: &str, text: &str) -> Result<Done> {
    let row = locate(resolved, id)?;
    let cells = row.cells();
    if cells.len() < 3 {
        return Err(Error::refused(
            "requirement",
            format!("`{id}`'s row has no text cell to replace: {}", row.line().trim()),
        ));
    }
    // A row whose own text contains a `|` has more cells than a row has
    // meanings — `**Options:** approve | defer` is one requirement written
    // across two cells, and replacing cell 2 would leave `defer` stranded
    // beside the new text. Refused rather than mangled: this command exists so
    // that a person editing a requirement does not have to think about the
    // table, and the one row where that is not true should say so.
    if cells.iter().skip(3).any(|cell| !cell.trim().is_empty()) {
        return Err(Error::refused(
            "requirement",
            format!(
                "`{id}`'s text contains a `|`, so it spans more than one cell — \
                 this cannot reword it without losing what is after the bar. \
                 Edit {} by hand.",
                row.source.display()
            ),
        ));
    }
    let was = row.says();
    if was == text {
        return Ok(Done {
            id: id.to_string(),
            source: row.source.clone(),
            summary: format!("{id} already said that — nothing written"),
            note: None,
        });
    }

    let indent: String = row.line().chars().take_while(|c| c.is_whitespace()).collect();
    let mut cells: Vec<String> = cells.iter().map(|cell| (*cell).to_string()).collect();
    cells[0] = String::new();
    cells[2] = format!(" {text} ");
    let line = format!("{indent}{}", cells.join("|").trim_start());

    let state = row.state();
    write_atomic(&row.source, &row.rewritten(Some(line)))?;
    Ok(Done {
        id: id.to_string(),
        source: row.source,
        summary: format!("{id} now says: {text}"),
        // Said rather than prevented. Correcting the wording of a delivered
        // requirement is a legitimate thing to do, and it is also how a `✅`
        // ends up over a sentence nobody proved — so the marker is named.
        note: (state != "open")
            .then(|| format!("that row is still marked `{state}` — this changed the text, not the marker")),
    })
}

fn delete(resolved: &Path, id: &str) -> Result<Done> {
    let row = locate(resolved, id)?;
    let was = row.says();
    let state = row.state();
    write_atomic(&row.source, &row.rewritten(None))?;
    Ok(Done {
        id: id.to_string(),
        source: row.source,
        // The text comes back out with it. A deleted requirement is otherwise
        // gone from the only place it was written down, and a person who meant
        // a different row finds out one keystroke too late.
        summary: format!("deleted {id}: {was}"),
        note: match state {
            "done" => Some(format!(
                "{id} was marked done — the record that it was delivered went with the row"
            )),
            "open" => Some(format!("{id} will not be reused; the next id is still counted from the highest")),
            other => Some(format!("{id} was marked `{other}`")),
        },
    })
}

/// Clear a requirement's gate, because a person decided (`O-17`).
///
/// It takes the id and nothing else. `--choose` is carried into what is
/// printed, so the command a card offers can name the option a person picked,
/// but the file only ever loses a `⛔`: encoding which option won into the row
/// is the author's edit, not a flag's.
fn ungate(resolved: &Path, id: &str, choose: Option<&str>) -> Result<Done> {
    let row = locate(resolved, id)?;
    if row.state() != "gated" {
        return Err(Error::refused(
            "ungate",
            format!("`{id}` is not gated in {}", row.source.display()),
        ));
    }
    let cells = row.cells();
    let mut cells: Vec<String> = cells.iter().map(|cell| (*cell).to_string()).collect();
    let Some(status) = cells.get_mut(1) else {
        return Err(Error::refused("ungate", format!("`{id}`'s row has no status cell")));
    };
    *status = format!(" {} ", status.trim().trim_start_matches('⛔').trim());

    let indent: String = row.line().chars().take_while(|c| c.is_whitespace()).collect();
    cells[0] = String::new();
    let line = format!("{indent}{}", cells.join("|").trim_start());
    write_atomic(&row.source, &row.rewritten(Some(line)))?;

    Ok(Done {
        id: id.to_string(),
        source: row.source,
        summary: match choose {
            Some(choice) => format!("{id} is no longer gated — you chose: {choice}"),
            None => format!("{id} is no longer gated"),
        },
        note: Some(
            "the row still says what it was gated on; edit it if that is now wrong".to_string(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    const SOURCE: &str = "# requirements\n\n\
        | id | Requirement |\n|---|---|\n\
        | ✅ ~~`L-1`~~ | the journal is append-only. |\n\
        | ⛔ `L-2` | a thing. **Gated: dependency approval** |\n\
        | `L-3` | another thing. |\n\
        | ❌ `L-4` | not doing this one. |\n";

    fn fixture(tag: &str) -> PathBuf {
        let root = tmpdir(tag);
        let path = root.join("perpetum.md");
        std::fs::write(&path, SOURCE).expect("fixture");
        path
    }

    fn text_of(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn parse_is_the_allowlist() {
        assert!(Write::parse("requirement", &["add", "a thing"]).is_ok());
        assert!(Write::parse("requirement", &["edit", "L-3", "a thing"]).is_ok());
        assert!(Write::parse("requirement", &["delete", "L-3"]).is_ok());
        assert!(Write::parse("ungate", &["L-2"]).is_ok());

        // The verb is part of the permission, so a subcommand alone opens
        // nothing — and nothing outside the four is a write at all.
        for (subcommand, args) in [
            ("requirement", &["mark", "L-3", "done"][..]),
            ("requirement", &["done", "L-3"][..]),
            ("requirement", &["add"][..]),
            ("run", &["--once"][..]),
            ("record", &["c1/b1/s01", "outcome", "did it"][..]),
        ] {
            let refused = Write::parse(subcommand, args);
            assert!(refused.is_err(), "`{subcommand} {args:?}` must not be a write");
        }
    }

    #[test]
    fn a_row_may_not_be_ended_by_its_own_text() {
        assert!(Write::parse("requirement", &["add", "a | b"]).is_err());
        assert!(Write::parse("requirement", &["add", "a\nb"]).is_err());
        assert!(Write::parse("requirement", &["add", "   "]).is_err());
        assert!(Write::parse("requirement", &["edit", "L-3", "a | b"]).is_err());
    }

    #[test]
    fn add_mints_the_next_id_and_no_marker() {
        let path = fixture("req-add");
        let done = Write::Add { text: "a new thing".into() }.apply(&path).expect("add");
        assert_eq!(done.id, "L-5");

        let text = text_of(&path);
        assert!(text.contains("| `L-5` | a new thing |"), "{text}");
        assert!(text.contains(FILED_SECTION), "{text}");
        // `V-2`: the row arrives with nothing claimed about it.
        let entry = crate::cycle::catalogue(&text).into_iter().find(|e| e.id == "L-5");
        assert_eq!(entry.map(|e| e.state), Some("open".to_string()));
    }

    /// `L-34`'s whole reason for having its own command rather than a file
    /// write: an edit changes what a row says and can never change what it
    /// claims.
    #[test]
    fn edit_replaces_the_text_and_never_the_marker() {
        let path = fixture("req-edit");
        let done =
            Write::Edit { id: "L-1".into(), text: "the journal is append-only, always.".into() }
                .apply(&path)
                .expect("edit");
        assert!(done.note.is_some(), "an edit over a marked row says so");

        let text = text_of(&path);
        assert!(text.contains("| ✅ ~~`L-1`~~ | the journal is append-only, always. |"), "{text}");
        let entry = crate::cycle::catalogue(&text).into_iter().find(|e| e.id == "L-1");
        assert_eq!(entry.map(|e| e.state), Some("done".to_string()), "the marker survived");
    }

    #[test]
    fn edit_cannot_promote_an_open_row() {
        let path = fixture("req-edit-open");
        // The text is a marker, spelled out. It goes in the text cell, which is
        // not the status cell — `V-23` — so the row stays open.
        Write::Edit { id: "L-3".into(), text: "✅ done now, honestly".into() }
            .apply(&path)
            .expect("edit");
        let entry = crate::cycle::catalogue(&text_of(&path)).into_iter().find(|e| e.id == "L-3");
        assert_eq!(entry.map(|e| e.state), Some("open".to_string()));
    }

    /// A row whose text contains a `|` is one requirement written across two
    /// cells. Rewording it here would strand whatever is after the bar.
    #[test]
    fn a_row_whose_text_spans_two_cells_is_refused_rather_than_mangled() {
        let root = tmpdir("req-multicell");
        let path = root.join("perpetum.md");
        let source = "| id | Requirement |\n|---|---|\n\
             | ⛔ `L-2` | a thing. **Options:** approve | defer |\n";
        std::fs::write(&path, source).expect("fixture");

        let refused = Write::Edit { id: "L-2".into(), text: "reworded".into() }
            .apply(&path)
            .expect_err("more cells than meanings");
        assert!(format!("{refused}").contains('|'), "{refused}");
        assert_eq!(text_of(&path), source, "and nothing was written");

        // Ungating one is fine: it rebuilds the status cell and joins the rest
        // back exactly as it found them.
        Write::Ungate { id: "L-2".into(), choose: None }.apply(&path).expect("ungate");
        assert!(
            text_of(&path).contains("| `L-2` | a thing. **Options:** approve | defer |"),
            "{}",
            text_of(&path)
        );
    }

    #[test]
    fn edit_finds_the_row_by_its_status_cell_not_by_the_id_anywhere() {
        let root = tmpdir("req-edit-prose");
        let path = root.join("perpetum.md");
        std::fs::write(
            &path,
            "| id | Requirement |\n|---|---|\n\
             | `L-1` | supersedes `L-2`, which said the other thing. |\n\
             | `L-2` | the other thing. |\n",
        )
        .expect("fixture");

        Write::Edit { id: "L-2".into(), text: "replaced".into() }.apply(&path).expect("edit");
        let text = text_of(&path);
        assert!(text.contains("| `L-1` | supersedes `L-2`, which said the other thing. |"), "{text}");
        assert!(text.contains("| `L-2` | replaced |"), "{text}");
    }

    #[test]
    fn delete_removes_the_row_and_hands_back_what_it_said() {
        let path = fixture("req-delete");
        let done = Write::Delete { id: "L-3".into() }.apply(&path).expect("delete");
        assert!(done.summary.contains("another thing."), "{}", done.summary);

        let text = text_of(&path);
        assert!(!crate::cycle::catalogue(&text).iter().any(|e| e.id == "L-3"), "{text}");
        assert_eq!(crate::cycle::catalogue(&text).len(), 3);
        // The neighbours are untouched, endings and all.
        assert!(text.contains("| ✅ ~~`L-1`~~ | the journal is append-only. |"), "{text}");
        assert!(text.contains("| ❌ `L-4` | not doing this one. |"), "{text}");
    }

    #[test]
    fn ungate_clears_the_marker_and_keeps_the_prose() {
        let path = fixture("req-ungate");
        Write::Ungate { id: "L-2".into(), choose: Some("approve".into()) }
            .apply(&path)
            .expect("ungate");
        let text = text_of(&path);
        assert!(text.contains("| `L-2` | a thing. **Gated: dependency approval** |"), "{text}");
        assert!(crate::cycle::gated(&text).is_empty(), "{text}");

        // And a second one refuses rather than reporting success.
        let again = Write::Ungate { id: "L-2".into(), choose: None }.apply(&path);
        assert!(again.is_err());
    }

    #[test]
    fn a_missing_row_is_refused_rather_than_created() {
        let path = fixture("req-missing");
        for write in [
            Write::Edit { id: "L-9".into(), text: "x".into() },
            Write::Delete { id: "L-9".into() },
            Write::Ungate { id: "L-9".into(), choose: None },
        ] {
            let refused = write.apply(&path).expect_err("no such row");
            assert!(format!("{refused}").contains("L-9"), "{refused}");
        }
        assert_eq!(text_of(&path), SOURCE, "and nothing was written");
    }

    /// `N-4`: this project is tested on Windows first, and a file that used
    /// `\r\n` must not come back as a whole-document diff after a one-word
    /// edit.
    #[test]
    fn line_endings_survive_an_edit() {
        let root = tmpdir("req-crlf");
        let path = root.join("perpetum.md");
        std::fs::write(&path, SOURCE.replace('\n', "\r\n")).expect("fixture");

        Write::Edit { id: "L-3".into(), text: "replaced".into() }.apply(&path).expect("edit");
        let text = text_of(&path);
        assert!(!text.contains("\n\n"), "no bare newline survived: {text:?}");
        assert_eq!(text.matches("\r\n").count(), SOURCE.matches('\n').count());
    }

    /// `O-18`: a `path.requirements` that names a directory. The row lives in
    /// one file of it, and the write has to find that file.
    #[test]
    fn a_directory_source_is_written_where_the_row_is() {
        let root = tmpdir("req-dir");
        let dir = root.join("requirements");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("01-spine.md"), "| `L-1` | the spine. |\n").expect("a");
        std::fs::write(dir.join("02-panel.md"), "| `L-2` | the panel. |\n").expect("b");

        let done = Write::Edit { id: "L-2".into(), text: "the panel, rewritten.".into() }
            .apply(&dir)
            .expect("edit");
        assert!(done.source.ends_with("02-panel.md"), "{}", done.source.display());
        assert_eq!(text_of(&dir.join("01-spine.md")), "| `L-1` | the spine. |\n");

        // And a new row lands in `filed.md` rather than in somebody's chapter.
        let done = Write::Add { text: "something new".into() }.apply(&dir).expect("add");
        assert_eq!(done.id, "L-3");
        assert!(done.source.ends_with("filed.md"), "{}", done.source.display());
    }
}
