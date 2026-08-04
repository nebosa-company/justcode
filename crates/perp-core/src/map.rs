//! A ranked structural view of the repository (`T-27`).
//!
//! The largest gap between this harness and the others, measured rather than
//! guessed. Given only `grep`, a model spends its turns finding out what is
//! there: running against a Flutter backlog it read for ten turns and wrote
//! nothing, three separate times, and one `grep` for a type name returned
//! 604,886 bytes and blocked the batch outright.
//!
//! The fix is not a better search. It is to stop the search being necessary —
//! a step should *begin* knowing the shape of the code, which is what putting
//! this in the stable prefix does (`M-12`).
//!
//! ## Ranked, because complete is useless
//!
//! Every declaration in a repository is more text than the answer is worth. A
//! file matters to a reader in proportion to how much of the rest of the
//! repository leans on it, so files are scored by how many *other* files use
//! the names they declare, and the map is filled to a byte budget in that
//! order. What falls off the end is what nothing else refers to.
//!
//! ## Off by default, because it was measured and did not pay
//!
//! The reasoning above is sound and the map does not deliver on it. Twelve
//! runs over six paired requirements — each run both ways from the same commit
//! with the arm order alternated, so drift could not land on one side:
//!
//! | | map off | map on |
//! |---|---|---|
//! | turns | 18 | 17 |
//! | tokens | 281,600 | 286,358 |
//! | wall clock | 114 s | 159 s |
//! | cost | $0.0092 | $0.0112 |
//! | succeeded | 2 of 6 | 2 of 6 |
//!
//! It wins two pairs, loses two, ties two: a coin flip that charges 21% more
//! and takes 40% longer. So `map.budget` defaults to `0` and the binding must
//! ask for it.
//!
//! An earlier **single** pair showed 25% fewer turns and 29% fewer tokens. Six
//! pairs say that was noise pointing the wrong way, which is the more useful
//! finding: one run of a stochastic model is not a measurement, however much
//! it agrees with the design.
//!
//! What the same twelve runs did show is that four of six requirements failed
//! in *both* arms. Retrieval was the hypothesis for the exploration stalls;
//! it is not the binding constraint for this model, and the cause of those
//! stalls is still open.
//!
//! Kept rather than deleted for two reasons: it may be that a lexical map is
//! too weak and a parsed one would pay — an experiment this one cannot
//! distinguish from "the idea does not help here" — and six pairs at this
//! variance rule out a large effect, not a small one.
//!
//! ## Lexical, and weaker for it
//!
//! Aider's repo map parses with tree-sitter and ranks with PageRank over the
//! reference graph. That is the right implementation and this is not it:
//! `N-11` puts the standard library first, and a parser per language is a
//! dependency tree in its own right.
//!
//! So this reads declarations with [`crate::tool::symbols`] — line-shaped, not
//! syntax-shaped — and scores with a single pass rather than to convergence.
//! It is wrong at the edges: a name declared in two files is attributed to one
//! of them, a name used only in a macro is invisible, and a comment mentioning
//! a type counts as a use. Each of those costs a line the reader did not need,
//! which is the cheap direction to be wrong in. Nothing here is load-bearing
//! for correctness — being wrong makes the map less useful, never the loop less
//! honest.

use std::collections::BTreeMap;
use std::path::Path;

/// A sensible size **when the binding turns the map on**. It is off by
/// default — see the module note. Big enough to be worth reading, small enough
/// that a prefix-cached prompt is not mostly map (`M-12`).
pub const DEFAULT_BUDGET: usize = 6_000;

/// Files bigger than this are read for their declarations and not scored for
/// their uses. A generated file is enormous and refers to everything.
const MAX_FILE_BYTES: usize = 400_000;

/// Names shorter than this are too common to mean anything. `Doc` survives;
/// `id`, `x` and `new` do not.
const MIN_NAME: usize = 4;

/// Declarations shown per file. A map is for breadth: knowing that fifty files
/// exist beats knowing everything one of them declares.
const PER_FILE: usize = 10;

/// One file's place in the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    /// How many other files use something this one declares.
    pub score: usize,
    pub declarations: Vec<(usize, String)>,
}

/// Build the map, ranked and budgeted.
///
/// `focus` names paths the current work already concerns — they are pulled to
/// the top whatever the graph says, because the file a step is about matters to
/// that step even if nothing else in the repository imports it.
pub fn build(root: &Path, files: &[String], budget: usize, focus: &[String]) -> String {
    let entries = rank(root, files, focus);
    render(&entries, budget)
}

/// Score and order the files. Separated from rendering so the ranking can be
/// tested without asserting on layout.
pub fn rank(root: &Path, files: &[String], focus: &[String]) -> Vec<Entry> {
    let mut sources: Vec<(String, String)> = Vec::new();
    for path in files {
        let full = root.join(path);
        let Ok(meta) = std::fs::metadata(&full) else { continue };
        if meta.len() as usize > MAX_FILE_BYTES {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&full) else { continue };
        sources.push((path.clone(), text));
    }

    // What each file declares, and who declares each name. A name declared in
    // two places is attributed to the first — see the module note; the cost is
    // a misattributed line, not a wrong answer.
    let mut declarations: BTreeMap<String, Vec<(usize, String)>> = BTreeMap::new();
    let mut declared_by: BTreeMap<String, String> = BTreeMap::new();
    for (path, text) in &sources {
        let found = crate::tool::symbols(text);
        for (_, head) in &found {
            if let Some(name) = declared_name(head) {
                if name.len() >= MIN_NAME {
                    declared_by.entry(name).or_insert_with(|| path.clone());
                }
            }
        }
        declarations.insert(path.clone(), found);
    }

    // One pass: every identifier a file mentions that some *other* file
    // declares is a vote for that other file.
    let mut score: BTreeMap<String, usize> = BTreeMap::new();
    for (path, text) in &sources {
        let mut counted: Vec<&str> = Vec::new();
        for token in identifiers(text) {
            if token.len() < MIN_NAME || counted.contains(&token) {
                continue;
            }
            counted.push(token);
            if let Some(owner) = declared_by.get(token) {
                if owner != path {
                    *score.entry(owner.clone()).or_default() += 1;
                }
            }
        }
    }

    let mut entries: Vec<Entry> = sources
        .into_iter()
        .map(|(path, _)| Entry {
            score: score.get(&path).copied().unwrap_or_default(),
            declarations: declarations.remove(&path).unwrap_or_default(),
            path,
        })
        .filter(|entry| !entry.declarations.is_empty())
        .collect();

    // Focus wins outright. A step editing a file needs it in front of it
    // whether or not the rest of the repository has heard of it.
    entries.sort_by(|a, b| {
        let focused = |entry: &Entry| focus.iter().any(|f| entry.path.contains(f.as_str()));
        focused(b)
            .cmp(&focused(a))
            .then(b.score.cmp(&a.score))
            .then(a.path.cmp(&b.path))
    });
    entries
}

/// Fill the budget, most-depended-upon first.
fn render(entries: &[Entry], budget: usize) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "repository map — the files most of the rest depends on, and what they declare.\n\
         Ranked and truncated, so absence here means low rank, not absence from the repo.\n\n",
    );
    let mut shown = 0;
    for entry in entries {
        // Room for the header and at least one declaration, or stop cleanly
        // rather than emitting a path with nothing under it.
        if out.len() + 80 > budget {
            break;
        }
        out.push_str(&format!("{}\n", entry.path));
        shown += 1;
        // Capped per file, so breadth beats depth. `lib.rs` declares fifty
        // modules and would otherwise spend the entire budget listing them —
        // which tells a reader one file exists rather than that fifty do.
        for (line, head) in entry.declarations.iter().take(PER_FILE) {
            if out.len() + head.len() + 12 > budget {
                break;
            }
            out.push_str(&format!("  {line}: {head}\n"));
        }
        if let Some(rest) = entry.declarations.len().checked_sub(PER_FILE).filter(|n| *n > 0) {
            out.push_str(&format!("  … +{rest} more\n"));
        }
        out.push('\n');
    }
    let left = entries.len().saturating_sub(shown);
    if left > 0 {
        // `T-6`: truncation is visible. A map that quietly stops is a map that
        // says a file does not exist.
        out.push_str(&format!(
            "… and {left} more file(s) below the cut. `symbols(path)` reads any of them.\n"
        ));
    }
    out
}

/// The name a declaration introduces, out of the line that introduces it.
///
/// The first word that is not a keyword: `pub fn parse_edits(` is
/// `parse_edits`, `class TraceDialog extends StatefulWidget` is `TraceDialog`.
///
/// The first and not the last. Taking the last word read `class TraceDialog
/// extends StatefulWidget` as declaring `StatefulWidget` — which is not merely
/// a miss but a lie about the graph, crediting every subclass to the framework
/// it inherits from.
///
/// Crude beyond that, and its failure is a name that matches nothing, costing
/// the file its votes and nothing else.
fn declared_name(head: &str) -> Option<String> {
    const KEYWORDS: &[&str] = &[
        "pub", "fn", "struct", "enum", "trait", "impl", "mod", "const", "type", "static",
        "class", "abstract", "sealed", "mixin", "extension", "interface", "func", "function",
        "def", "async", "export", "package", "final", "var", "let",
    ];
    let cut: String = head
        .chars()
        .take_while(|c| *c != '(' && *c != '<' && *c != '=' && *c != ':')
        .collect();
    for word in cut.split_whitespace() {
        if KEYWORDS.contains(&word) {
            continue;
        }
        let name: String = word.chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
        if !name.is_empty() {
            return Some(name);
        }
    }
    None
}

/// Every identifier in a body of text, in order.
fn identifiers(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = None;
    for (index, byte) in bytes.iter().enumerate() {
        let wordish = byte.is_ascii_alphanumeric() || *byte == b'_';
        match (start, wordish) {
            (None, true) => start = Some(index),
            (Some(from), false) => {
                if let Some(token) = text.get(from..index) {
                    out.push(token);
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        if let Some(token) = text.get(from..) {
            out.push(token);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    fn write(root: &Path, path: &str, body: &str) {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).expect("dirs");
        }
        std::fs::write(full, body).expect("write");
    }

    /// The point of the whole module: the file everything leans on comes first.
    #[test]
    fn the_most_depended_upon_file_ranks_first() {
        let dir = tmpdir("map-rank");
        write(&dir, "core.rs", "pub struct Document {}\npub fn render_document() {}\n");
        write(&dir, "lonely.rs", "pub fn nobody_uses_this_at_all() {}\n");
        write(&dir, "a.rs", "fn a() { let d = Document::new(); render_document(); }\n");
        write(&dir, "b.rs", "fn b() { let d = Document::new(); }\n");

        let files = vec![
            "core.rs".to_string(),
            "lonely.rs".to_string(),
            "a.rs".to_string(),
            "b.rs".to_string(),
        ];
        let ranked = rank(&dir, &files, &[]);

        assert_eq!(ranked.first().map(|e| e.path.as_str()), Some("core.rs"));
        assert!(
            ranked.first().map(|e| e.score).unwrap_or_default() >= 2,
            "two other files use what it declares"
        );
        let lonely = ranked.iter().find(|e| e.path == "lonely.rs").expect("present");
        assert_eq!(lonely.score, 0, "nothing refers to it, so it ranks last, not missing");
    }

    /// A file the step is working on beats the graph.
    #[test]
    fn a_focused_file_comes_first_whatever_the_graph_says() {
        let dir = tmpdir("map-focus");
        write(&dir, "core.rs", "pub struct Document {}\n");
        write(&dir, "scratch.rs", "pub fn scratch_helper() {}\n");
        write(&dir, "a.rs", "fn a() { Document::new(); }\n");

        let files =
            vec!["core.rs".to_string(), "scratch.rs".to_string(), "a.rs".to_string()];
        let ranked = rank(&dir, &files, &["scratch.rs".to_string()]);

        assert_eq!(
            ranked.first().map(|e| e.path.as_str()),
            Some("scratch.rs"),
            "the file being edited matters to this step even if nothing imports it"
        );
    }

    /// `T-6`: a map that quietly stops is a map that says a file is not there.
    #[test]
    fn truncation_says_so() {
        let dir = tmpdir("map-budget");
        for n in 0..40 {
            write(&dir, &format!("f{n}.rs"), &format!("pub fn function_number_{n}() {{}}\n"));
        }
        let files: Vec<String> = (0..40).map(|n| format!("f{n}.rs")).collect();

        let rendered = build(&dir, &files, 400, &[]);

        assert!(rendered.len() <= 500, "it respects the budget: {} bytes", rendered.len());
        assert!(
            rendered.contains("more file(s) below the cut"),
            "and says what it left out: {rendered}"
        );
    }

    #[test]
    fn a_declaration_yields_the_name_it_introduces() {
        assert_eq!(declared_name("pub fn parse_edits(raw: &str)").as_deref(), Some("parse_edits"));
        assert_eq!(declared_name("class TraceDialog extends StatefulWidget").as_deref(), Some("TraceDialog"));
        assert_eq!(declared_name("pub struct Doc").as_deref(), Some("Doc"));
    }
}
