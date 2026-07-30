//! Where the harness keeps its things.
//!
//! One directory, and one place that names it. Everything the harness reads by
//! convention and everything it writes lives under [`DIR`]; everything else is a
//! binding key, because a project gets to decide where its own requirements
//! live.
//!
//! It used to be spread across the project's own `docs/`: the binding in one
//! subdirectory, the requirements as a sibling file, the vision in a second
//! subdirectory, the artifacts in a third, and the lock and control files loose
//! at the top of the repository. Five or six locations depending on how you
//! counted, none of them obviously related, inside a directory that belongs to
//! the project's documentation rather than to a tool.
//!
//! A dot-directory says what these files are: machinery, not documents. It also
//! makes the whole thing removable in one gesture, which matters for a tool
//! that asks to be trusted with a repository.

use std::path::{Path, PathBuf};

/// The harness's directory, relative to the workspace root.
pub const DIR: &str = ".harness";

/// The binding, which is how a workspace is recognised at all.
///
/// Not configurable and never will be: it is the file that says where everything
/// else is, so something has to be known before anything can be read.
pub const BINDING: &str = ".harness/binding.md";

/// Rendered artifacts (`A-2`). One stable file per kind.
pub const ARTIFACTS: &str = ".harness/artifacts";

/// What the harness writes that is not evidence of anything — a run's console, a
/// held lock, the control channel, a half-finished atomic write.
///
/// Kept as a real file in the directory rather than a rule in the project's own
/// `.gitignore`, so the harness's mess is the harness's business and removing
/// `.harness/` removes the rule with it.
pub const GITIGNORE: &str = ".harness/.gitignore";

/// The contents of that file.
///
/// Extensions rather than names wherever possible: a new kind of log or lock
/// should be covered by a rule that already exists rather than by an edit here.
///
/// Staging is explicit, so nothing in the harness would commit these anyway
/// (`G-3`). This is for the person who types `git add -A`.
pub const GITIGNORE_BODY: &str = "\
# Written by the harness while it runs. None of it is evidence of anything: the
# journal is the record, and everything here is either scratch or a lock.
#
# Managed by `perp` — it rewrites this file when it is missing, so delete it
# rather than editing it if you want different rules, and keep your own in the
# project's `.gitignore` where they will survive.

# A run's console, kept only so that a refusal to start is not silent.
*.log

# Half-written files, from an atomic write that has not been renamed yet.
*.tmp

# Locks. Whether one is held is a fact about this machine, not about the work.
*.lock

# The control channel: pause, step and stop, read at the next step boundary.
control
";

/// Where requirements live when a project keeps more than one file of them.
///
/// A single `perpetum.md` still works and is what a small project wants. A
/// directory is for the case a big one reaches: one file per area, in folders
/// that mean something, rather than a thousand-line table.
pub const REQUIREMENTS: &str = ".harness/requirements";

/// Files a person edits, as opposed to files the harness writes.
///
/// Named because the editor offers them as a menu and because `init` writes them:
/// the two lists would drift if each kept its own.
pub const VISION: &str = ".harness/vision.md";
pub const LINKS: &str = ".harness/links.md";

pub fn dir_in(root: &Path) -> PathBuf {
    root.join(DIR)
}

pub fn binding_in(root: &Path) -> PathBuf {
    root.join(BINDING)
}

/// The nearest workspace at or above `from`, or `None` if there is none.
///
/// A project is worked on from inside it — from `src/`, from a test directory,
/// from wherever the file being edited lives — and the harness belongs to the
/// project rather than to the directory the shell happens to be sitting in. The
/// editor has always resolved it this way; the command line took whatever
/// `--root` said or else the current directory, so `perp state` one level down
/// reported no workspace while the panel beside it showed the run.
///
/// The nearest [`DIR`] wins, and it is the directory that is looked for rather
/// than the binding inside it. A `.harness` without a binding is a broken
/// workspace and gets said so; searching past it would silently run against
/// whatever unrelated project happened to be further up, which is a worse answer
/// than an error.
pub fn enclosing(from: &Path) -> Option<PathBuf> {
    let mut dir = Some(from);
    // Bounded by the filesystem: `parent()` yields `None` at the root.
    while let Some(candidate) = dir {
        if candidate.join(DIR).is_dir() {
            return Some(candidate.to_path_buf());
        }
        dir = candidate.parent();
    }
    None
}

/// Write the ignore file if it is not there.
///
/// Idempotent and never overwriting: someone who edited it gets to keep their
/// edit, and someone who deleted it gets the default back. Returns whether it
/// wrote anything, so a caller can say so rather than doing it silently.
///
/// A failure here is not an error worth stopping for — the loop's job is not
/// managing a `.gitignore` — so it is reported as `false` and nothing else.
pub fn ensure_gitignore(root: &Path) -> bool {
    let path = root.join(GITIGNORE);
    if path.exists() {
        return false;
    }
    if std::fs::create_dir_all(dir_in(root)).is_err() {
        return false;
    }
    std::fs::write(&path, GITIGNORE_BODY).is_ok()
}

/// Every requirements file, for a `path.requirements` that names either one file
/// or a directory of them.
///
/// Sorted by path, so the order a batch is worked in is the order the files are
/// laid out rather than whatever the filesystem felt like. Recursive, because the
/// point of a directory is subdirectories.
pub fn requirement_sources(resolved: &Path) -> Vec<PathBuf> {
    if resolved.is_file() {
        return vec![resolved.to_path_buf()];
    }
    if !resolved.is_dir() {
        return Vec::new();
    }
    let mut found = Vec::new();
    collect_markdown(resolved, &mut found);
    found.sort();
    found
}

fn collect_markdown(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, into);
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
            into.push(path);
        }
    }
}

/// The text of every requirements file, joined.
///
/// Joined rather than parsed per file because every reader of this is line-based:
/// a table row means the same thing whichever file it came from, and ids are
/// unique across the project by `V-9` regardless.
pub fn requirements_text(resolved: &Path) -> String {
    requirement_sources(resolved)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reason this exists: a project is worked on from inside it.
    #[test]
    fn a_workspace_is_found_from_a_directory_below_it() {
        let root = crate::testutil::tmpdir("enclosing-below");
        std::fs::create_dir_all(root.join(DIR)).expect("harness");
        let deep = root.join("src").join("inner");
        std::fs::create_dir_all(&deep).expect("deep");

        assert_eq!(enclosing(&deep).as_deref(), Some(root.as_path()));
        assert_eq!(enclosing(&root).as_deref(), Some(root.as_path()));
    }

    /// Nothing above it is a workspace, so say so rather than climbing to the
    /// filesystem root and answering with somebody else's project.
    #[test]
    fn no_workspace_above_is_none_rather_than_a_guess() {
        let root = crate::testutil::tmpdir("enclosing-none");
        let deep = root.join("a").join("b");
        std::fs::create_dir_all(&deep).expect("deep");
        assert_eq!(enclosing(&deep), None);
    }

    /// Nested workspaces: the near one is the one being worked in.
    #[test]
    fn the_nearest_workspace_wins() {
        let outer = crate::testutil::tmpdir("enclosing-nested");
        std::fs::create_dir_all(outer.join(DIR)).expect("outer harness");
        let inner = outer.join("packages").join("thing");
        std::fs::create_dir_all(inner.join(DIR)).expect("inner harness");

        assert_eq!(enclosing(&inner).as_deref(), Some(inner.as_path()));
    }

    /// A `.harness` with nothing in it still stops the search. It is a broken
    /// workspace and the binding error says so; walking past it would run the
    /// loop against an unrelated project further up, silently.
    #[test]
    fn a_workspace_missing_its_binding_still_stops_the_search() {
        let outer = crate::testutil::tmpdir("enclosing-broken");
        std::fs::create_dir_all(outer.join(DIR)).expect("outer harness");
        std::fs::write(binding_in(&outer), "# binding
").expect("outer binding");
        let inner = outer.join("half-made");
        std::fs::create_dir_all(inner.join(DIR)).expect("inner harness, no binding");

        assert_eq!(enclosing(&inner).as_deref(), Some(inner.as_path()));
        assert!(!binding_in(&inner).exists(), "the inner one has no binding");
    }

    /// A file is not a directory: `.harness` as a regular file is not a
    /// workspace and must not be mistaken for one.
    #[test]
    fn a_file_called_harness_is_not_a_workspace() {
        let root = crate::testutil::tmpdir("enclosing-file");
        std::fs::write(root.join(DIR), "not a directory").expect("file");
        assert_eq!(enclosing(&root), None);
    }
    use crate::testutil::tmpdir;

    #[test]
    fn every_path_is_under_the_one_directory() {
        // The point of this module. Six locations became one, and a seventh
        // arriving later should not be able to land somewhere else by accident.
        for path in [BINDING, ARTIFACTS, GITIGNORE] {
            assert!(path.starts_with(DIR), "{path} is not under {DIR}");
        }
        let root = Path::new("/w");
        assert_eq!(binding_in(root), root.join(DIR).join("binding.md"));
        assert_eq!(dir_in(root), root.join(DIR));
    }

    #[test]
    fn the_ignore_file_is_written_once_and_never_overwritten() {
        let root = tmpdir("layout-gitignore");

        assert!(ensure_gitignore(&root), "written on a workspace that has none");
        let path = root.join(GITIGNORE);
        assert!(path.is_file());

        // Someone else's rules survive. A tool that rewrites a file it found is a
        // tool that eats an edit somebody made on purpose.
        std::fs::write(&path, "*.mine\n").expect("edit");
        assert!(!ensure_gitignore(&root), "nothing to do the second time");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "*.mine\n");

        // Deleted, and it comes back.
        std::fs::remove_file(&path).expect("remove");
        assert!(ensure_gitignore(&root));
        assert!(std::fs::read_to_string(&path).expect("read").contains("*.log"));
    }

    #[test]
    fn the_ignore_rules_cover_what_the_harness_writes() {
        // Named against the real files rather than as a wish: `cycle.log` from a
        // run started in the editor, `write.lock` and `gate.lock` from the
        // engine, `control` from `perp control`, and the `.tmp` an atomic write
        // leaves if it dies between write and rename.
        for name in ["cycle.log", "write.lock", "gate.lock", "control"] {
            let covered = GITIGNORE_BODY.lines().any(|rule| {
                let rule = rule.trim();
                if rule.is_empty() || rule.starts_with('#') {
                    return false;
                }
                match rule.strip_prefix('*') {
                    Some(extension) => name.ends_with(extension),
                    None => rule == name,
                }
            });
            assert!(covered, "{name} would be committed by `git add -A`");
        }
    }
}
