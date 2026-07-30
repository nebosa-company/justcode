//! The binding (`L-21`).
//!
//! Perpetum is generic; a project is not. The binding maps every path and
//! command the method names to what this repository actually uses, and the
//! engine refuses to run without one — an unresolved path stops the loop and
//! asks rather than being guessed at.
//!
//! The binding is read out of a fenced block inside `binding.md` rather than a
//! config file of its own, so the document a human reads and the keys the
//! engine reads cannot drift apart.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// The fence label that marks the machine-readable block.
pub const FENCE: &str = "perp-binding";

/// Where the binding lives, relative to the repository root.
pub const DEFAULT_PATH: &str = crate::layout::BINDING;

#[derive(Debug, Clone)]
pub struct Binding {
    root: PathBuf,
    source: PathBuf,
    entries: Vec<(String, String)>,
}

impl Binding {
    /// Read the binding at the conventional path under `root`.
    pub fn load(root: &Path) -> Result<Binding> {
        Binding::load_from(root, &root.join(DEFAULT_PATH))
    }

    pub fn load_from(root: &Path, source: &Path) -> Result<Binding> {
        let text = std::fs::read_to_string(source).map_err(|e| {
            Error::unbound(
                "binding",
                format!("cannot read {}: {e} — nothing runs unbound", source.display()),
            )
        })?;
        let entries = parse_block(&text)?;
        Ok(Binding {
            root: root.to_path_buf(),
            source: source.to_path_buf(),
            entries,
        })
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn get(&self, key: &str) -> Result<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| Error::unbound(key, "not declared in the binding"))
    }

    /// A declared path, resolved against the repository root.
    pub fn resolve(&self, key: &str) -> Result<PathBuf> {
        Ok(self.root.join(self.get(key)?))
    }

    /// Every `path.*` input must exist, and every `out.*` must have somewhere to
    /// be written. Anything else stops the loop naming the key (`L-21`).
    pub fn verify(&self) -> Result<()> {
        for (key, value) in self.entries() {
            if let Some(rest) = key.strip_prefix("path.") {
                let path = self.root.join(value);
                if !path.exists() {
                    return Err(Error::unbound(
                        key,
                        format!(
                            "`{rest}` resolves to {}, which does not exist",
                            path.display()
                        ),
                    ));
                }
            } else if key.starts_with("out.") {
                let path = self.root.join(value);
                let parent = path.parent().unwrap_or(Path::new("."));
                if !parent.exists() {
                    return Err(Error::unbound(
                        key,
                        format!("its directory {} does not exist", parent.display()),
                    ));
                }
            }
        }
        Ok(())
    }
}

fn parse_block(text: &str) -> Result<Vec<(String, String)>> {
    parse_fenced(text, FENCE, "binding")
}

/// Read `key = value` lines out of a labelled fenced block.
///
/// Shared with the link configuration (`M-1`), so both live inside the document
/// that explains them rather than in a config file that drifts from its prose.
/// Blank lines and `#` comments are ignored; a duplicate key is an error, since
/// silently taking the last one is how a config lies about itself.
pub fn parse_fenced(text: &str, fence: &str, what: &str) -> Result<Vec<(String, String)>> {
    let mut lines = text.lines();
    let opener = format!("```{fence}");
    if !lines.any(|line| line.trim_end() == opener) {
        return Err(Error::unbound(
            what,
            format!("no ```{fence} block — the engine has nothing to read"),
        ));
    }

    let mut entries: Vec<(String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut closed = false;

    for line in lines {
        if line.trim_end().starts_with("```") {
            closed = true;
            break;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(Error::unbound(what, format!("`{line}` is not `key = value`")));
        };
        let (key, value) = (key.trim().to_string(), value.trim().to_string());
        if key.is_empty() || value.is_empty() {
            return Err(Error::unbound(what, format!("`{line}` has an empty key or value")));
        }
        if !seen.insert(key.clone()) {
            return Err(Error::unbound(key, "declared twice"));
        }
        entries.push((key, value));
    }

    if !closed {
        return Err(Error::unbound(what, "the block is never closed"));
    }
    if entries.is_empty() {
        return Err(Error::unbound(what, "the block is empty"));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tmpdir;

    fn write(root: &Path, rel: &str, body: &str) -> PathBuf {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(&path, body).expect("write fixture");
        path
    }

    fn bound(root: &Path) -> Binding {
        write(root, ".harness/perpetum.md", "# requirements");
        write(
            root,
            DEFAULT_PATH,
            "Commentary a human reads.\n\
             \n\
             ```perp-binding\n\
             # a comment, and a blank line, both ignored\n\
             \n\
             path.requirements = .harness/perpetum.md\n\
             out.journal       = .harness/journal.jsonl\n\
             gate.test         = cargo test --workspace\n\
             ```\n\
             \n\
             More commentary.\n",
        );
        Binding::load(root).expect("load")
    }

    #[test]
    fn reads_keys_out_of_the_fenced_block_only() {
        let root = tmpdir("binding-read");
        let binding = bound(&root);
        assert_eq!(binding.get("path.requirements").expect("key"), ".harness/perpetum.md");
        assert_eq!(binding.get("gate.test").expect("key"), "cargo test --workspace");
        assert_eq!(binding.entries().count(), 3);
    }

    #[test]
    fn resolves_against_the_repository_root() {
        let root = tmpdir("binding-resolve");
        let binding = bound(&root);
        assert_eq!(
            binding.resolve("path.requirements").expect("resolve"),
            root.join(".harness/perpetum.md")
        );
    }

    #[test]
    fn refuses_to_run_unbound() {
        let root = tmpdir("binding-missing");
        let err = Binding::load(&root).expect_err("must refuse");
        assert!(format!("{err}").contains("nothing runs unbound"), "{err}");
    }

    #[test]
    fn refuses_a_document_with_no_block() {
        let root = tmpdir("binding-noblock");
        write(&root, DEFAULT_PATH, "# Binding\n\nAll prose, no block.\n");
        let err = Binding::load(&root).expect_err("must refuse");
        assert!(format!("{err}").contains("no ```perp-binding block"), "{err}");
    }

    #[test]
    fn verify_names_the_input_that_is_missing() {
        let root = tmpdir("binding-verify");
        write(
            &root,
            DEFAULT_PATH,
            "```perp-binding\npath.vision = .harness/vision.md\n```\n",
        );
        let binding = Binding::load(&root).expect("load");
        let err = binding.verify().expect_err("must refuse");
        let text = format!("{err}");
        assert!(text.contains("path.vision"), "names the key: {text}");
        assert!(text.contains("does not exist"), "says why: {text}");
    }

    #[test]
    fn verify_accepts_an_output_that_does_not_exist_yet() {
        let root = tmpdir("binding-out");
        let binding = bound(&root);
        // journal.jsonl has never been written; its directory exists because
        // binding.md lives there. An output is not an input.
        binding.verify().expect("outputs may be absent");
    }

    #[test]
    fn rejects_a_key_declared_twice() {
        let root = tmpdir("binding-dupe");
        write(
            &root,
            DEFAULT_PATH,
            "```perp-binding\ngate.test = a\ngate.test = b\n```\n",
        );
        let err = Binding::load(&root).expect_err("must refuse");
        assert!(format!("{err}").contains("declared twice"), "{err}");
    }

    #[test]
    fn rejects_a_line_that_is_not_a_pair() {
        let root = tmpdir("binding-junk");
        write(&root, DEFAULT_PATH, "```perp-binding\njust some words\n```\n");
        assert!(Binding::load(&root).is_err());
    }

    #[test]
    fn rejects_an_unclosed_block() {
        let root = tmpdir("binding-unclosed");
        write(&root, DEFAULT_PATH, "```perp-binding\ngate.test = cargo test\n");
        let err = Binding::load(&root).expect_err("must refuse");
        assert!(format!("{err}").contains("never closed"), "{err}");
    }

    #[test]
    fn get_names_the_key_it_could_not_find() {
        let root = tmpdir("binding-unknown");
        let binding = bound(&root);
        let err = binding.get("path.nowhere").expect_err("must refuse");
        assert!(format!("{err}").contains("path.nowhere"), "{err}");
    }
}
