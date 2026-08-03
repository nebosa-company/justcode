//! Is it reachable from a real run? (`V-15`)
//!
//! Every other gate asks whether a thing works. None of them asks whether
//! anything calls it, and a function with thorough tests and no caller passes
//! every test it has, forever.
//!
//! This is not hypothetical. `verify::may_implement` and `RealityCheck::run`
//! were written, tested and cited, and the engine never called either — so
//! `V-1`'s mandatory reality check was mandatory in prose only, and every
//! requirement this harness built carried "no reality check recorded" in its
//! evidence chain while reporting itself done. `verify::independence`,
//! `Role::Verifier` and the whole verdict renderer were the same: `V-5`'s
//! independent review had never once run. The status table could not see it,
//! because "done" is scored per requirement against that requirement's own
//! tests.
//!
//! ## What it does
//!
//! Finds every `pub fn` in this crate outside a test module, then looks for a
//! call to it anywhere in the crate or the CLI, also outside test modules.
//! Anything with no caller is unreachable from a real run.
//!
//! ## The allowlist is a ratchet
//!
//! There are unreachable functions today, and failing on all of them would
//! mean either a red build or a hundred rushed edits. So they are listed, and
//! the list may only shrink:
//!
//! - a new unreachable function fails the test — this is the point
//! - an allowlisted function that has *become* reachable **also** fails, with
//!   an instruction to delete the line
//!
//! Without that second half the list rots into a permanent excuse. With it,
//! the list is a debt whose balance is visible and which cannot quietly grow.
//!
//! ## What it will not catch
//!
//! Text matching, not name resolution — a call through a trait object, a macro
//! or a function pointer may read as absent, and two functions sharing a name
//! shadow one another. It under-reports rather than over-reports, which is the
//! right direction: a false alarm here costs an argument about tooling, and a
//! miss costs what `V-1` and `V-5` cost.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// Names whose absence proves nothing: trait methods, conversions and the
    /// constructors every type has. Looking for callers of `new` finds
    /// thousands and means nothing.
    const UNIVERSAL: &[&str] = &[
        "new", "default", "fmt", "from", "into", "clone", "as_str", "parse", "to_string", "eq",
        "hash", "cmp", "next", "drop", "deref", "add", "sub",
    ];

    fn crate_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Everything before the first `#[cfg(test)]`. Crude and sufficient: this
    /// crate puts its tests in one module at the end of each file, and a test
    /// that calls a function is not evidence that the loop does.
    fn production_only(text: &str) -> &str {
        match text.find("#[cfg(test)]") {
            Some(at) => &text[..at],
            None => text,
        }
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// Every `.rs` file the shipped binary is built from: this crate, plus the
    /// CLI, which is where a great many of these are legitimately called.
    fn sources() -> Vec<(String, String)> {
        let root = crate_root();
        let mut out = Vec::new();

        let src = root.join("src");
        if let Ok(entries) = std::fs::read_dir(&src) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "rs") {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    out.push((name, production_only(&read(&path)).to_string()));
                }
            }
        }

        let cli = root.join("../perp/src/main.rs");
        if cli.exists() {
            out.push(("main.rs".to_string(), production_only(&read(&cli)).to_string()));
        }
        out
    }

    /// `(file, name)` for every `pub fn` defined outside a test module.
    fn public_functions(sources: &[(String, String)]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (file, body) in sources {
            if file == "main.rs" {
                continue; // The CLI is the caller, not the surface.
            }
            for line in body.lines() {
                let trimmed = line.trim_start();
                let Some(rest) = trimmed.strip_prefix("pub fn ") else { continue };
                let name: String =
                    rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if name.is_empty() || UNIVERSAL.contains(&name.as_str()) {
                    continue;
                }
                out.push((file.clone(), name));
            }
        }
        out
    }

    /// Whether anything outside the definition itself calls `name`.
    fn is_called(name: &str, sources: &[(String, String)]) -> bool {
        let needle = format!("{name}(");
        let definition = format!("pub fn {name}(");
        for (_, body) in sources {
            for line in body.lines() {
                if line.contains(&definition) {
                    continue;
                }
                let Some(at) = line.find(&needle) else { continue };
                // `foo(` inside `some_other_foo(` is not a call to `foo`.
                let preceded_by_ident = line[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
                if !preceded_by_ident {
                    return true;
                }
            }
        }
        false
    }

    fn allowlist_path() -> PathBuf {
        crate_root().join("unreachable-allow.txt")
    }

    fn allowlist() -> BTreeSet<String> {
        read(&allowlist_path())
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect()
    }

    /// `V-15`, mechanised: a requirement whose code nothing calls is not done.
    #[test]
    fn every_public_function_is_reachable_from_a_real_run() {
        let sources = sources();
        assert!(sources.len() > 10, "the scanner found no sources — it is testing nothing");

        let mut unreachable = BTreeSet::new();
        for (file, name) in public_functions(&sources) {
            if !is_called(&name, &sources) {
                unreachable.insert(format!("{file}::{name}"));
            }
        }

        let allowed = allowlist();
        let fresh: Vec<&String> = unreachable.difference(&allowed).collect();
        let fixed: Vec<&String> = allowed.difference(&unreachable).collect();

        let mut complaint = String::new();
        if !fresh.is_empty() {
            complaint.push_str(&format!(
                "\n{} public function(s) have no caller outside tests.\n\
                 A requirement whose code nothing invokes is not done, however green its own \
                 tests are — that is how `V-1` and `V-5` shipped unrun.\n\n\
                 Call it, delete it, or — if it is genuinely API surface for someone else — add \
                 it to {}:\n",
                fresh.len(),
                allowlist_path().display()
            ));
            for name in &fresh {
                complaint.push_str(&format!("  {name}\n"));
            }
        }
        if !fixed.is_empty() {
            complaint.push_str(&format!(
                "\n{} allowlisted function(s) are now called. Delete these lines from {} — the \
                 list is a debt that may only shrink, and one that never shrinks is an excuse:\n",
                fixed.len(),
                allowlist_path().display()
            ));
            for name in &fixed {
                complaint.push_str(&format!("  {name}\n"));
            }
        }

        assert!(complaint.is_empty(), "{complaint}");
    }
}
