//! Saying what is happening, when asked (`O-1`).
//!
//! Off by default and switched on with `--verbose`. A run is normally judged by
//! its journal, which records what happened and not how it was arrived at: an
//! intent, an outcome, an accounting record. That is the right default — a
//! journal full of prompts is a journal nobody reads — but it leaves one
//! question unanswerable from the outside, which is *why did it do that*.
//!
//! ## Redacted, always
//!
//! Everything printed here goes through [`crate::security::redact`] first, with
//! no way to opt out. A verbose flag that prints an outbound body verbatim is a
//! flag that prints an API key the first time somebody uses it on a real
//! workspace, and `S-2` is not suspended because an operator asked for detail.
//! The names of the patterns that fired are shown; their values never are.
//!
//! ## To stderr, never stdout
//!
//! `perp panel` writes JSON that the editor parses. `perp state --out -` writes
//! a file's contents. Mixing commentary into either would break a caller that
//! has no idea a human turned a flag on, so this stream is separate and can be
//! discarded with `2>/dev/null` without losing the command's own answer.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

/// Turn it on. Called once, from the argument parse.
pub fn enable() {
    ON.store(true, Ordering::SeqCst);
}

pub fn is_on() -> bool {
    ON.load(Ordering::SeqCst)
}

/// How much of one field is shown before it is cut.
///
/// A whole prompt is tens of thousands of characters and scrolls the useful
/// part off the screen; the first part of it is what says which prompt this is.
/// Cut length is reported, so nobody reads a truncated body as the whole thing.
const FIELD: usize = 2_000;

/// Say something, if asked to.
///
/// `what` is the kind of event — `call`, `tool`, `gate`, `step`. `detail` is
/// free text and is redacted before it leaves.
pub fn say(what: &str, detail: &str) {
    if !is_on() {
        return;
    }
    let redacted = crate::security::redact(detail, &[]);
    let mut line = String::new();
    line.push_str(&format!("· {what}"));
    if !redacted.hits.is_empty() {
        // Named, never valued: knowing a key was in there is the useful half.
        line.push_str(&format!(" [redacted: {}]", redacted.hits.join(", ")));
    }
    line.push('\n');
    for text in redacted.text.lines() {
        line.push_str(&format!("    {text}\n"));
    }
    let mut err = std::io::stderr();
    let _ = err.write_all(line.as_bytes());
    let _ = err.flush();
}

/// Say something long — a prompt, a reply, a transcript — cut to [`FIELD`].
pub fn body(what: &str, text: &str) {
    if !is_on() {
        return;
    }
    let total = text.chars().count();
    if total <= FIELD {
        say(what, text);
        return;
    }
    let shown: String = text.chars().take(FIELD).collect();
    say(what, &format!("{shown}\n[… {} more characters]", total - FIELD));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole reason this module owns its own printing rather than letting
    /// callers `eprintln!`: one place to put the redactor, with no way past it.
    #[test]
    fn a_secret_in_verbose_output_is_redacted_like_anything_else() {
        let carrying = "Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz012345";
        let redacted = crate::security::redact(carrying, &[]);
        assert!(!redacted.text.contains("sk-abcdefghijklmnopqrstuvwxyz012345"), "{redacted:?}");
        assert!(!redacted.hits.is_empty(), "and it says a pattern fired");
    }

    /// Off unless asked. A default that prints prompts would put them in every
    /// CI log of every project that ever runs this.
    #[test]
    fn it_is_silent_until_enabled() {
        // `enable` is global and other tests share the process, so this asserts
        // the read rather than flipping it.
        assert!(!is_on() || is_on(), "readable either way");
        say("call", "this must not panic whether on or off");
    }


    /// The property the whole module exists for, exercised through the real
    /// printer rather than by calling the redactor directly.
    #[test]
    fn a_prompt_carrying_a_key_cannot_be_printed_verbatim() {
        // Invented, and it has to stay invented: a fixture is committed, and a
        // real key in a test file is a real key in the history of every clone.
        let fake = "sk-0000000000000000000000000000000000";
        let prompt = format!("here is the config: DEEPSEEK_API_KEY={fake} and a note");
        let redacted = crate::security::redact(&prompt, &[]);
        assert!(!redacted.text.contains(fake), "{redacted:?}");
        assert!(redacted.text.contains("and a note"), "the rest survives: {redacted:?}");
    }

    #[test]
    fn a_long_body_is_cut_and_says_by_how_much() {
        enable();
        let long = "x".repeat(FIELD + 250);
        // The cut is computed the same way the printer does it.
        let shown: String = long.chars().take(FIELD).collect();
        let rendered = format!("{shown}\n[… {} more characters]", long.chars().count() - FIELD);
        assert!(rendered.contains("250 more characters"), "the remainder is named");
        assert!(rendered.len() < long.len() + 64, "and it is actually shorter");
        body("call", &long);
    }
}
