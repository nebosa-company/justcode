//! An idea, turned into candidate requirements a person can pick from
//! (`L-34`, `O-18`).
//!
//! A person with an idea and a requirements file has a chore in between: the
//! idea is one paragraph and the file wants one row per behaviour, each a
//! sentence about what should happen. This asks a model to do that split.
//!
//! **The model drafts and the person files, and the gap between those two is
//! the whole design.** Nothing here writes anything. [`derive`] returns
//! strings; every one of them reaches the requirements source only by being
//! chosen and passed to [`crate::requirement::Write::Add`], through the same
//! allowlist a typed command goes through. That is `V-12` intact: the person is
//! still the author, and a model that drafted forty rows has filed none of
//! them. A version of this that appended what came back would be the loop
//! filing its own requirements with a person's click as the fig leaf.
//!
//! Two smaller rules follow from the same place:
//!
//! - **No ids.** The model is told not to invent one, and could not use one if
//!   it did: `Write::Add` mints the id. A drafted `L-40` would collide with a
//!   real row or point at nothing.
//! - **No markers.** Nothing here can produce a `✅` — the drafts are text,
//!   they are filed as text, and `Write::Add` writes a row with an empty status
//!   cell (`V-2`).
//!
//! The idea itself is a person's unpublished thought about their own product,
//! and it goes to whichever link serves the role. [`destination`] exists so
//! that a surface can say **where, before it is sent** rather than after: an
//! idea leaving for a third-party model is a decision, and a decision nobody
//! was offered is not one.

use crate::client::{ChatRequest, Client, Message, Served};
use crate::cycle::Catalogued;
use crate::error::{Error, Result};
use crate::link::{AssumeHealthy, Links, Mode, Role};
use crate::net::Curl;

/// Which role drafts. **Planner**, because splitting an intent into the
/// behaviours that would satisfy it is planning — and because the roles are
/// bound to links in the operator's own file, so a project that wants its
/// cheapest model doing this says so there rather than here.
pub const ROLE: Role = Role::Planner;

/// Where an idea would go, read before anything is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub link: String,
    pub model: String,
    /// `local` or `cloud`. The word that matters: `cloud` means the paragraph
    /// leaves this machine for somebody else's, which is a thing to be told
    /// before typing rather than after.
    pub privacy: &'static str,
}

/// Which link would answer, without calling it.
pub fn destination(links: &Links) -> Result<Destination> {
    let link = links.resolve(ROLE, &AssumeHealthy, Mode::Any)?;
    Ok(Destination {
        link: link.name.clone(),
        model: link.model.clone(),
        privacy: link.privacy.as_str(),
    })
}

/// What came back, and what it cost.
#[derive(Debug, Clone)]
pub struct Draft {
    /// One candidate requirement per entry, in the order they were drafted.
    /// Never written anywhere by this module.
    pub candidates: Vec<String>,
    /// Lines that came back and could not be a row — see [`candidates_from`].
    /// Counted rather than swallowed: a surface that shows six of nine drafts
    /// and says nothing has quietly edited what the model proposed.
    pub dropped: usize,
    /// `link X · model Y`, for the person and for the journal.
    pub via: String,
    /// The whole call, so the caller can journal the spend. Model tokens that
    /// no record mentions are tokens `perp cost` cannot report (`N-7`).
    pub served: Served,
}

/// The instruction, and the list it must not repeat.
///
/// The existing catalogue goes in by **name only** — the opening sentence of
/// each row, never the paragraph. It is there to stop the model proposing what
/// the project already decided, and for that a title is enough; sending the
/// whole file would be sending the whole file to a third party to answer a
/// question about one paragraph.
pub fn messages(idea: &str, catalogue: &[Catalogued]) -> Vec<Message> {
    let mut already = String::new();
    for entry in catalogue {
        // Bounded, and it says when it stopped. A project with two thousand
        // rows should not silently send the first hundred and let the model
        // duplicate the rest.
        if already.len() > 8000 {
            already.push_str(&format!(
                "\n… and {} more that did not fit here.\n",
                catalogue.len() - already.lines().count()
            ));
            break;
        }
        already.push_str(&format!("- {}\n", entry.name));
    }

    let system = format!(
        "You are drafting candidate requirements for a software project, from one \
         person's idea. A requirement is one sentence saying what the product should \
         do — a behaviour someone could later check, not a task and not an \
         implementation.\n\n\
         Rules, all of them hard:\n\
         - Output nothing but the candidates, one per line.\n\
         - No numbering, no bullets, no headings, no preamble, no closing remark.\n\
         - Never write an id such as `L-12`. Ids are minted when a person files a row.\n\
         - Never write a status, a tick, a checkbox or the word done. Whether \
           something is built is decided by tests, not by this list.\n\
         - Never use the `|` character.\n\
         - Say what should happen, not how to build it.\n\
         - Between three and eight candidates. Fewer good ones beats more.\n\
         - Do not propose anything this project has already written down.\n\n\
         Already on this project's list, by opening sentence:\n{already}"
    );

    vec![Message::system(system), Message::user(format!("The idea:\n\n{}", idea.trim()))]
}

/// Pull candidates out of whatever the model actually sent.
///
/// Lenient about what it strips and strict about what it keeps. Models add
/// bullets and numbers however firmly they are told not to, and that is a
/// formatting habit rather than a different answer — but a line carrying a `|`
/// would end the table row it was written into, and a line carrying a marker is
/// a status claim nothing here may make (`V-2`). Those are dropped and counted.
pub fn candidates_from(reply: &str) -> (Vec<String>, usize) {
    let mut kept: Vec<String> = Vec::new();
    let mut dropped = 0usize;
    for line in reply.lines() {
        let line = line.trim();
        let line = line
            .trim_start_matches(['-', '*', '•', '–', '—'])
            .trim_start();
        // `1.` / `1)` — the number and its punctuation, not a sentence that
        // happens to start with a digit.
        let line = match line.find(['.', ')']) {
            Some(at) if at < 3 && line[..at].chars().all(|c| c.is_ascii_digit()) && at > 0 => {
                line[at + 1..].trim_start()
            }
            _ => line,
        };
        let line = line.trim().trim_matches('`').trim();
        if line.is_empty() || line.ends_with(':') {
            continue;
        }
        if line.contains('|') || line.contains(crate::cycle::MARKERS) {
            dropped += 1;
            continue;
        }
        if kept.iter().any(|seen| seen == line) {
            continue;
        }
        kept.push(line.to_string());
    }
    (kept, dropped)
}

/// Ask the model, and hand back what it proposed (`L-34`).
///
/// Writes nothing, by construction: there is no path from here to the
/// requirements source. The caller journals [`Draft::served`] and shows
/// [`Draft::candidates`] to a person.
pub fn derive(
    binding: &crate::binding::Binding,
    links: &Links,
    idea: &str,
    catalogue: &[Catalogued],
    now: i64,
) -> Result<Draft> {
    let idea = idea.trim();
    if idea.is_empty() {
        return Err(Error::refused("idea", "there is nothing here to draft from"));
    }

    let transport = Curl::new();
    let mut client = Client::new(&transport)
        // The same two protections a typed `perp ask` gets, and for the same
        // reason: an idea is a person's own prose about their own product, and
        // it may be going to a link that is not theirs (`S-3`, `S-4`).
        .with_redaction(crate::security::patterns_from_entries(
            &binding.entries().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>(),
        ))
        .with_egress(crate::security::Egress::new(Vec::new()).allowing_links(links.all()));

    let served = client.call(
        links,
        ROLE,
        &ChatRequest::new(messages(idea, catalogue)),
        &AssumeHealthy,
        Mode::Any,
        now,
    )?;

    let (candidates, dropped) = candidates_from(&served.reply.content);
    if candidates.is_empty() {
        return Err(Error::refused(
            "idea",
            format!(
                "{} proposed nothing that could be a requirement — it said: {}",
                served.link,
                served.reply.content.trim().chars().take(300).collect::<String>()
            ),
        ));
    }
    Ok(Draft { candidates, dropped, via: served.provenance(), served })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogued(id: &str, name: &str) -> Catalogued {
        Catalogued {
            id: id.into(),
            name: name.into(),
            text: name.into(),
            state: "open".into(),
        }
    }

    #[test]
    fn bullets_and_numbers_are_habits_not_answers() {
        let (kept, dropped) = candidates_from(
            "- The panel lists every requirement.\n\
             * The list shows each row's state.\n\
             1. A person may reword a row.\n\
             2) A person may delete a row.\n\
             \n\
             `Progress is a percentage of the markers.`\n",
        );
        assert_eq!(
            kept,
            vec![
                "The panel lists every requirement.",
                "The list shows each row's state.",
                "A person may reword a row.",
                "A person may delete a row.",
                "Progress is a percentage of the markers.",
            ]
        );
        assert_eq!(dropped, 0);
    }

    #[test]
    fn a_preamble_and_its_headings_are_not_candidates() {
        let (kept, _) = candidates_from(
            "Here are the candidates:\n\
             \n\
             Requirements:\n\
             The page opens in a browser.\n\
             The page opens in a browser.\n",
        );
        // Both heading lines go — a requirement is a sentence about the
        // product and never ends in a colon — and the repeat goes with them: a
        // model that says the same thing twice has proposed it once.
        assert_eq!(kept, vec!["The page opens in a browser.".to_string()]);
    }

    /// `V-2` and the table, at the parse. A drafted line that claims a state
    /// or that would end its own row never becomes a candidate a person can
    /// click.
    #[test]
    fn a_draft_may_not_carry_a_marker_or_a_bar() {
        let (kept, dropped) = candidates_from(
            "✅ The page already lists requirements.\n\
             The page lists requirements | and their state.\n\
             ⛔ Gated on the approvals work.\n\
             The page shows progress as a percentage.\n",
        );
        assert_eq!(kept, vec!["The page shows progress as a percentage.".to_string()]);
        assert_eq!(dropped, 3, "and the count is reported rather than swallowed");
    }

    #[test]
    fn the_prompt_forbids_what_the_parser_would_have_to_drop() {
        let messages = messages("a web front end", &[catalogued("L-1", "the journal is append-only")]);
        let system = &messages[0].content;
        assert!(system.contains("Never write an id"), "{system}");
        assert!(system.contains("Never write a status"), "{system}");
        assert!(system.contains("`|`"), "{system}");
        // The existing list travels as titles, so the model does not repropose
        // what the project already decided.
        assert!(system.contains("the journal is append-only"), "{system}");
        assert!(messages[1].content.contains("a web front end"));
    }

    /// Names, never paragraphs, and it says when it stopped rather than
    /// silently sending a prefix.
    #[test]
    fn a_long_catalogue_is_bounded_and_says_so() {
        let many: Vec<Catalogued> = (1..400)
            .map(|n| catalogued(&format!("L-{n}"), "a requirement with a reasonably long opening sentence"))
            .collect();
        let messages = messages("an idea", &many);
        let system = &messages[0].content;
        assert!(system.len() < 12_000, "bounded: {}", system.len());
        assert!(system.contains("more that did not fit"), "{system}");
    }
}
