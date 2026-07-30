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

pub fn dir_in(root: &Path) -> PathBuf {
    root.join(DIR)
}

pub fn binding_in(root: &Path) -> PathBuf {
    root.join(BINDING)
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

#[cfg(test)]
mod tests {
    use super::*;
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
