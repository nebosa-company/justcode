//! Artifacts (`A-1`–`A-7`, `O-2`).
//!
//! Everything here is **generated from the journal**. Nothing an artifact says
//! is typed by hand or asserted by a model — if a number appears on the board,
//! it was counted from records, and if it cannot be counted it does not appear.
//!
//! Three rules give the module its shape:
//!
//! - **Self-contained** (`A-2`, `A-5`). One file, inline styles, no fetches, no
//!   CDN, no build step. It has to open from a USB stick in six months. A
//!   dashboard that needs a server is a dashboard that stops working exactly
//!   when someone is trying to find out what went wrong.
//! - **Provenance or it is decoration** (`A-6`). Cycle, batch, commit, time and
//!   the links used. [`Artifact`] cannot be constructed without it, so the rule
//!   is enforced by the type rather than by remembering.
//! - **Never on the critical path** (`A-7`). A render that fails produces a
//!   warning. It does not fail a gate, block a feature, or stop a batch —
//!   losing a batch because a diagram would not draw is absurd, and a harness
//!   that can do it will eventually do it at 3am.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::state::Projection;
use crate::time;

/// The seven kinds (`A-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Board,
    CycleReport,
    BatchPlan,
    ReleaseNotesDraft,
    GateEvidence,
    Diagram,
    ConflictRegister,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Board,
        Kind::CycleReport,
        Kind::BatchPlan,
        Kind::ReleaseNotesDraft,
        Kind::GateEvidence,
        Kind::Diagram,
        Kind::ConflictRegister,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Board => "board",
            Kind::CycleReport => "cycle-report",
            Kind::BatchPlan => "batch-plan",
            Kind::ReleaseNotesDraft => "release-notes-draft",
            Kind::GateEvidence => "gate-evidence",
            Kind::Diagram => "diagram",
            Kind::ConflictRegister => "conflict-register",
        }
    }

    pub fn parse(text: &str) -> Result<Kind> {
        Kind::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str() == text)
            .ok_or_else(|| {
                Error::refused(
                    text,
                    format!(
                        "is not an artifact kind. There are {}: {}",
                        Kind::ALL.len(),
                        Kind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
                    ),
                )
            })
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Board => "Progress board",
            Kind::CycleReport => "Cycle report",
            Kind::BatchPlan => "Batch plan",
            Kind::ReleaseNotesDraft => "Release notes — draft",
            Kind::GateEvidence => "Gate evidence",
            Kind::Diagram => "Dependency diagram",
            Kind::ConflictRegister => "Conflict register",
        }
    }

    /// One stable name per kind, so a re-render **replaces** rather than
    /// accumulates (`A-2`). Two hundred timestamped boards is not a history —
    /// the journal is the history — it is two hundred files nobody deletes.
    pub fn file_name(self) -> String {
        format!("{}.html", self.as_str())
    }
}

/// Where every artifact lives, and the only place that says so (`A-2`).
///
/// The path used to be spelled out in five places, and `out.board` in the
/// binding named a sixth that nothing wrote — `/board` resolved
/// `.harness/progress-board.md` while the engine wrote
/// `.harness/artifacts/board.html`, so the command read a file that had
/// never existed. The artifact path is the authority; the binding key is gone.
pub const DIR: &str = crate::layout::ARTIFACTS;

/// The artifacts directory inside a workspace.
pub fn dir_in(root: &Path) -> PathBuf {
    root.join(DIR)
}

impl Kind {
    /// Where this kind's one file lives in a workspace.
    pub fn path_in(self, root: &Path) -> PathBuf {
        dir_in(root).join(self.file_name())
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where an artifact came from (`A-6`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub cycle: u32,
    pub batch: Option<String>,
    /// The commit the workspace was at. `None` when the repository could not
    /// say — reported as unknown rather than guessed.
    pub sha: Option<String>,
    pub at: i64,
    /// Every link that contributed to the work being reported on.
    pub links: Vec<String>,
    pub version: String,
}

impl Provenance {
    pub fn new(cycle: u32, at: i64) -> Provenance {
        Provenance {
            cycle,
            batch: None,
            sha: None,
            at,
            links: Vec::new(),
            version: crate::VERSION.to_string(),
        }
    }

    pub fn batch(mut self, batch: impl Into<String>) -> Provenance {
        self.batch = Some(batch.into());
        self
    }

    pub fn sha(mut self, sha: Option<String>) -> Provenance {
        self.sha = sha;
        self
    }

    pub fn links(mut self, links: Vec<String>) -> Provenance {
        self.links = links;
        self
    }

    /// Read the links out of the journal rather than being told them, so the
    /// list is what actually answered rather than what was configured.
    pub fn from_journal(cycle: u32, at: i64, records: &[crate::Record]) -> Provenance {
        let mut links: Vec<String> = Vec::new();
        for record in records {
            if let Some(entry) = crate::cost::from_record(record) {
                if !links.contains(&entry.link) {
                    links.push(entry.link);
                }
            }
        }
        Provenance::new(cycle, at).links(links)
    }

    fn render(&self) -> String {
        let batch = self.batch.clone().unwrap_or_else(|| "—".into());
        let sha = self.sha.clone().unwrap_or_else(|| "unknown".into());
        let links = if self.links.is_empty() {
            "none — nothing in this report came from a model".to_string()
        } else {
            self.links.join(", ")
        };
        format!(
            "<footer><h2>Provenance</h2><dl>\
             <dt>cycle</dt><dd>{}</dd>\
             <dt>batch</dt><dd>{}</dd>\
             <dt>commit</dt><dd><code>{}</code></dd>\
             <dt>generated</dt><dd>{}</dd>\
             <dt>links</dt><dd>{}</dd>\
             <dt>perp</dt><dd>{}</dd>\
             </dl><p>Generated from <code>journal.jsonl</code>. \
             Every number here was counted from a record.</p></footer>",
            self.cycle,
            escape(&batch),
            escape(&sha),
            time::format_utc(self.at),
            escape(&links),
            escape(&self.version),
        )
    }
}

/// A rendered artifact, on its way to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub kind: Kind,
    pub html: String,
    pub provenance: Provenance,
}

impl Artifact {
    pub fn file_name(&self) -> String {
        self.kind.file_name()
    }

    /// Written under the artifacts directory and nowhere else (`A-2`).
    pub fn write(&self, dir: &Path) -> Result<PathBuf> {
        let path = dir.join(self.file_name());
        crate::atomic::write_atomic(&path, &self.html)?;
        Ok(path)
    }

    /// Whether the page reaches outside itself. Checked rather than asserted,
    /// because "self-contained" is the property that quietly stops being true
    /// the first time someone adds a font (`A-2`).
    pub fn is_self_contained(&self) -> bool {
        external_references(&self.html).is_empty()
    }
}

/// Every external reference in a page, so a failure can name what it found.
///
/// What counts is a construct that **fetches**, not a URL that appears. The
/// first version looked for a bare `http://` or `https://` anywhere in the
/// page, which cannot tell a stylesheet the browser will go and get from an
/// error message that happens to quote an address — and the harness quotes
/// them faithfully, because `L-16` keeps a blocked batch's error verbatim.
///
/// Measured on Janitor, where the board and the conflict register failed to
/// render on every run: the journal held `https://status.claude.com` eight
/// times, from Anthropic's own 529 text, and `https://www.gnu.org` twice from a
/// gate transcript. Nothing on the page fetched anything.
///
/// An `<a href>` is deliberately not a reference. `A-2` is about a page that
/// renders without the network, and a link the reader may choose to follow
/// costs nothing until they do.
pub fn external_references(html: &str) -> Vec<String> {
    // Where a browser goes and gets something: an attribute it loads, a
    // stylesheet import, or a CSS `url()`.
    const FETCHERS: &[&str] = &["src=", "url(", "@import"];
    const SCHEMES: &[&str] = &["http://", "https://", "//"];

    let lower = html.to_ascii_lowercase();
    let mut found: Vec<String> = Vec::new();
    let mut note = |what: String| {
        if !found.contains(&what) {
            found.push(what);
        }
    };

    // A stylesheet is fetched whatever its URL looks like, so the tag itself is
    // the finding — this is the one case where the construct alone is enough.
    if lower.contains("<link rel=\"stylesheet\"") || lower.contains("<link rel=stylesheet") {
        note("<link rel=\"stylesheet\"".to_string());
    }

    for fetcher in FETCHERS {
        let mut from = 0;
        while let Some(at) = lower[from..].find(fetcher) {
            let start = from + at + fetcher.len();
            // Past the quote or whitespace the value may open with.
            let value = lower[start..].trim_start_matches(['"', '\'', ' ']);
            if let Some(scheme) = SCHEMES.iter().find(|s| value.starts_with(**s)) {
                note(format!("{fetcher}{scheme}"));
            }
            from = start;
        }
    }
    found
}

/// Where an artifact is going.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// Under the workspace. Rendering locally is `auto` (`A-4`).
    Workspace(PathBuf),
    /// Anywhere else — a gist, a bucket, a wiki, a "private" share link.
    Outside(String),
}

/// What a destination costs (`A-4`).
///
/// Publishing outside the workspace is Perpetum 0.4's *posting publicly*
/// **regardless of how private the destination claims to be**. A private gist
/// is a URL, and a URL is a thing that gets forwarded.
pub fn publish_policy(destination: &Destination) -> crate::approval::Policy {
    match destination {
        Destination::Workspace(_) => crate::approval::Policy::Auto,
        Destination::Outside(where_to) => crate::approval::Policy::Approve {
            reason: format!(
                "publishing to {where_to} is outside the workspace — that is posting publicly \
                 (Perpetum 0.4), however private the destination claims to be"
            ),
        },
    }
}

/// A render that did not work (`A-7`).
///
/// Deliberately not an [`Error`] at the call site: the engine turns this into a
/// warning on the step, and the step still succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub kind: Kind,
    pub why: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "artifact {} did not render: {} (not a failure — `A-7`)", self.kind, self.why)
    }
}

/// Render, or warn. Never fails (`A-7`).
pub fn try_render(
    kind: Kind,
    projection: &Projection,
    records: &[crate::Record],
    provenance: &Provenance,
) -> std::result::Result<Artifact, Warning> {
    let body = match kind {
        Kind::Board => board(projection),
        Kind::CycleReport => cycle_report(projection, records),
        Kind::BatchPlan => batch_plan(projection),
        Kind::ReleaseNotesDraft => release_notes(projection),
        Kind::GateEvidence => gate_evidence(records),
        Kind::Diagram => diagram(projection),
        Kind::ConflictRegister => conflict_register(projection),
    };

    let html = page(kind, &body, provenance);
    let artifact = Artifact { kind, html, provenance: provenance.clone() };
    if !artifact.is_self_contained() {
        return Err(Warning {
            kind,
            why: format!(
                "it reaches outside itself: {}",
                external_references(&artifact.html).join(", ")
            ),
        });
    }
    Ok(artifact)
}

/// One page, one file. The style is inline and small on purpose: it has to
/// survive being emailed, and it has to read in a terminal browser.
fn page(kind: Kind, body: &str, provenance: &Provenance) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title} — perp</title><style>{STYLE}</style></head>\
         <body><header><h1>{title}</h1><p class=\"sub\">cycle {cycle}</p></header>\
         <main>{body}</main>{provenance}</body></html>\n",
        title = escape(kind.title()),
        cycle = provenance.cycle,
        body = body,
        provenance = provenance.render(),
    )
}

const STYLE: &str = "\
:root{color-scheme:light dark;--ink:#16181d;--dim:#5b6472;--line:#d8dde5;--bg:#fbfbfd;--ok:#1f7a44;--bad:#a3272f}\
@media(prefers-color-scheme:dark){:root{--ink:#e6e8ec;--dim:#9aa3b2;--line:#2b313b;--bg:#14161a;--ok:#4ec27f;--bad:#e2696f}}\
*{box-sizing:border-box}\
body{margin:0;padding:2rem 1.25rem;background:var(--bg);color:var(--ink);\
font:15px/1.6 ui-sans-serif,system-ui,-apple-system,Segoe UI,Roboto,sans-serif;max-width:64rem;margin-inline:auto}\
h1{font-size:1.6rem;margin:0 0 .2rem}h2{font-size:1.05rem;margin:2rem 0 .6rem;letter-spacing:.02em}\
.sub{color:var(--dim);margin:0 0 1.5rem}\
table{border-collapse:collapse;width:100%;font-size:.9rem}\
th,td{text-align:left;padding:.4rem .6rem;border-bottom:1px solid var(--line);vertical-align:top}\
th{color:var(--dim);font-weight:600}\
td.num{text-align:right;font-variant-numeric:tabular-nums}\
code,pre{font-family:ui-monospace,SFMono-Regular,Consolas,monospace;font-size:.85em}\
pre{overflow-x:auto;padding:.75rem;border:1px solid var(--line);border-radius:4px;background:transparent}\
.ok{color:var(--ok)}.bad{color:var(--bad)}\
.wrap{overflow-x:auto}\
dl{display:grid;grid-template-columns:auto 1fr;gap:.2rem 1rem;font-size:.85rem}\
dt{color:var(--dim)}dd{margin:0}\
footer{margin-top:3rem;padding-top:1rem;border-top:1px solid var(--line);color:var(--dim);font-size:.85rem}\
";

fn board(projection: &Projection) -> String {
    let mut out = String::from("<h2>Position</h2><dl>");
    out.push_str(&format!(
        "<dt>cycle</dt><dd>{}</dd><dt>stage</dt><dd>{}</dd>",
        projection.cycle.map(|c| c.to_string()).unwrap_or_else(|| "—".into()),
        escape(projection.stage.as_deref().unwrap_or("—")),
    ));
    out.push_str(&format!(
        "<dt>steps</dt><dd>{} done, {} blocked</dd>",
        projection.done.len(),
        projection.blocked.len()
    ));
    match &projection.open_step {
        Some(step) => out.push_str(&format!("<dt>in flight</dt><dd><code>{step}</code></dd>")),
        None => out.push_str("<dt>in flight</dt><dd>nothing</dd>"),
    }
    out.push_str("</dl>");

    if !projection.pending_btw.is_empty() {
        out.push_str("<h2>Waiting from /btw</h2><div class=\"wrap\"><table>");
        out.push_str("<tr><th>#</th><th>class</th><th>from</th><th>said</th></tr>");
        for item in &projection.pending_btw {
            out.push_str(&format!(
                "<tr><td class=\"num\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                item.id,
                item.class,
                escape(&item.source),
                escape(&item.text)
            ));
        }
        out.push_str("</table></div>");
    }

    if !projection.blocked.is_empty() {
        out.push_str("<h2>Blocked</h2><div class=\"wrap\"><table>");
        out.push_str("<tr><th>step</th><th>what</th></tr>");
        for entry in &projection.blocked {
            out.push_str(&format!(
                "<tr><td><code>{}</code></td><td class=\"bad\">{}</td></tr>",
                entry.step,
                escape(&entry.summary)
            ));
        }
        out.push_str("</table></div>");
    }

    out.push_str("<h2>Requirements touched</h2><p>");
    if projection.requirements_touched.is_empty() {
        out.push_str("<em>none</em>");
    } else {
        out.push_str(
            &projection
                .requirements_touched
                .iter()
                .map(|id| format!("<code>{}</code>", escape(id)))
                .collect::<Vec<_>>()
                .join(" · "),
        );
    }
    out.push_str("</p>");
    out
}

fn cycle_report(projection: &Projection, records: &[crate::Record]) -> String {
    let metrics = crate::metrics::Cycle::count(records);
    let mut out = String::from("<h2>Counted</h2><div class=\"wrap\"><table>");
    out.push_str("<tr><th>measure</th><th>value</th></tr>");
    for (name, value) in metrics.rows() {
        out.push_str(&format!("<tr><td>{}</td><td class=\"num\">{}</td></tr>", escape(&name), escape(&value)));
    }
    out.push_str("</table></div>");
    out.push_str(&format!(
        "<h2>Steps</h2><p>{} closed, {} blocked.</p>",
        projection.done.len(),
        projection.blocked.len()
    ));
    out
}

fn batch_plan(projection: &Projection) -> String {
    format!(
        "<h2>Stage</h2><p>{}</p><p class=\"sub\">The plan itself lives in \
         <code>docs/prioritization/</code> and is written by a person. This is what the \
         journal says has happened to it: {} steps closed, {} blocked.</p>",
        escape(projection.stage.as_deref().unwrap_or("—")),
        projection.done.len(),
        projection.blocked.len()
    )
}

fn release_notes(projection: &Projection) -> String {
    let mut out = String::from(
        "<p class=\"sub\">A draft, from the journal. Every line is a step that closed \
         green — nothing here is a claim about intent.</p><ul>",
    );
    for entry in &projection.done {
        if entry.requirements.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "<li>{} <code>{}</code></li>",
            escape(&entry.summary),
            escape(&entry.requirements.join(", "))
        ));
    }
    out.push_str("</ul>");
    out
}

fn gate_evidence(records: &[crate::Record]) -> String {
    let mut out = String::new();
    let mut found = 0;
    for record in records {
        let Some(detail) = &record.detail else { continue };
        if !detail.starts_with("gate:") {
            continue;
        }
        found += 1;
        let green = record.ok == Some(true);
        out.push_str(&format!(
            "<h2><code>{}</code> <span class=\"{}\">{}</span></h2><pre>{}</pre>",
            record.step,
            if green { "ok" } else { "bad" },
            if green { "green" } else { "red" },
            escape(detail.trim_end())
        ));
    }
    if found == 0 {
        // Said out loud. An evidence bundle with nothing in it must not look
        // like an evidence bundle.
        out.push_str("<p class=\"bad\">No gate transcript in the journal. There is no evidence to bundle.</p>");
    }
    out
}

fn diagram(projection: &Projection) -> String {
    // Inline SVG, because a diagram that needs a rendering library is a
    // diagram that does not open (`A-2`).
    let done = projection.done.len();
    let blocked = projection.blocked.len();
    let total = (done + blocked).max(1);
    let width = 600.0;
    let green = width * (done as f64) / (total as f64);
    format!(
        "<h2>Steps</h2><div class=\"wrap\"><svg viewBox=\"0 0 {w} 60\" width=\"100%\" height=\"60\" \
         role=\"img\" aria-label=\"{done} closed, {blocked} blocked\">\
         <rect x=\"0\" y=\"14\" width=\"{w}\" height=\"22\" fill=\"currentColor\" opacity=\"0.12\"/>\
         <rect x=\"0\" y=\"14\" width=\"{green:.1}\" height=\"22\" fill=\"currentColor\" opacity=\"0.45\"/>\
         <text x=\"0\" y=\"52\" font-size=\"12\" fill=\"currentColor\">{done} closed · {blocked} blocked</text>\
         </svg></div>",
        w = width,
        green = green,
        done = done,
        blocked = blocked
    )
}

fn conflict_register(projection: &Projection) -> String {
    let mut out = String::from(
        "<p class=\"sub\">Conflicts are parked by a person in \
         <code>docs/prioritization/conflicts.md</code>. What the journal can say is which \
         steps ended blocked and why.</p>",
    );
    if projection.blocked.is_empty() {
        out.push_str("<p class=\"ok\">Nothing blocked.</p>");
        return out;
    }
    out.push_str("<div class=\"wrap\"><table><tr><th>step</th><th>what</th><th>error</th></tr>");
    for entry in &projection.blocked {
        let detail = entry.detail.as_deref().unwrap_or("").lines().next().unwrap_or("");
        out.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td><code>{}</code></td></tr>",
            entry.step,
            escape(&entry.summary),
            escape(detail)
        ));
    }
    out.push_str("</table></div>");
    out
}

/// HTML-escape. Journal text is arbitrary — a gate transcript contains `<` and
/// `&` routinely, and a `/btw` contains whatever the operator typed.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::Record;
    use crate::state::replay;
    use crate::step::StepId;
    use crate::testutil::tmpdir;

    const T: i64 = 1_700_000_000;

    fn step(n: u32) -> StepId {
        StepId::new(3, "b14", n).expect("step")
    }

    fn journal() -> Vec<Record> {
        vec![
            Record::intent(step(1), T, "run the gate").for_requirements(["V-2"]),
            Record::outcome(step(1), T + 5, true, "gate test is green")
                .for_requirements(["V-2"])
                .with_detail("gate: test\nsha: abc1234\n$ cargo test\nexit 0\n"),
            Record::intent(step(2), T + 6, "run the build"),
            Record::outcome(step(2), T + 9, false, "gate build is red")
                .with_detail("gate: build\n$ cargo build\nerror: mismatched <types> & such\nexit 101\n"),
        ]
    }

    fn provenance() -> Provenance {
        Provenance::new(3, T).batch("b14").sha(Some("abc1234".into()))
    }

    #[test]
    fn one_place_says_where_an_artifact_lives() {
        // `/board` resolved `out.board`, which every binding set to
        // `.harness/progress-board.md`, while the engine wrote
        // `.harness/artifacts/board.html`. The command read a file that had
        // never existed, and the path was spelled out in five other places
        // besides. The artifact path is the authority and this is it.
        let root = std::path::Path::new("/w");
        assert_eq!(dir_in(root), root.join(DIR));
        assert_eq!(Kind::Board.path_in(root), root.join(DIR).join("board.html"));

        // Every kind resolves under the same directory, one file each (`A-2`).
        for kind in Kind::ALL {
            let path = kind.path_in(root);
            assert_eq!(path.parent(), Some(dir_in(root).as_path()), "{kind}");
            assert!(path.to_string_lossy().ends_with(".html"), "{kind}");
        }
    }

    /// `A-2` is about a page that renders without the network, and a URL
    /// **quoted in text** is not one the browser will go and get.
    ///
    /// The check scanned for a bare `https://` anywhere in the page, so a
    /// journal entry that quoted an address failed it. Measured on Janitor: the
    /// board and the conflict register failed to render on every run because
    /// the journal held `https://status.claude.com` eight times, from
    /// Anthropic's own 529 text — which `L-16` keeps verbatim on purpose.
    #[test]
    fn a_url_a_page_only_mentions_is_not_a_reference_it_fetches() {
        let quoted = "<!doctype html><p>API Error: 529 Overloaded.                       If it persists, check https://status.claude.com.</p>";
        assert!(
            external_references(quoted).is_empty(),
            "quoting an address is not fetching it: {:?}",
            external_references(quoted)
        );

        // And the constructs that do fetch are still refused.
        for fetches in [
            r#"<script src="https://cdn.example/x.js"></script>"#,
            r#"<img src="http://example/x.png">"#,
            r#"<link rel="stylesheet" href="https://example/x.css">"#,
            r#"<style>@import "https://example/x.css";</style>"#,
            r#"<style>body{background:url(https://example/x.png)}</style>"#,
            r#"<script src="//cdn.example/x.js"></script>"#,
        ] {
            assert!(
                !external_references(fetches).is_empty(),
                "this one really does reach out: {fetches}"
            );
        }

        // A link the reader may choose to follow costs nothing until they do.
        let linked = r#"<a href="https://example/docs">the requirement</a>"#;
        assert!(
            external_references(linked).is_empty(),
            "an anchor is not a fetch: {:?}",
            external_references(linked)
        );
    }

    #[test]
    fn every_kind_renders_and_is_self_contained() {
        let records = journal();
        let projection = replay(&records);
        for kind in Kind::ALL {
            let artifact = try_render(kind, &projection, &records, &provenance())
                .unwrap_or_else(|w| panic!("{kind} must render: {w}"));
            assert!(
                artifact.is_self_contained(),
                "{kind} reaches outside itself: {:?}",
                external_references(&artifact.html)
            );
            assert!(artifact.html.starts_with("<!doctype html>"), "{kind} is a whole page");
            // `A-6` is about the values being on the page, not the heading. An
            // earlier version of this test asserted the word "Provenance" and
            // stayed green when the whole block was commented out.
            let visible = artifact.html.split("<footer>").nth(1).unwrap_or("");
            for expected in ["abc1234", "b14", &time::format_utc(T)] {
                assert!(
                    visible.contains(expected),
                    "{kind} does not carry {expected} (`A-6`): {visible}"
                );
            }
        }
    }

    #[test]
    fn a_page_that_reaches_out_is_caught_rather_than_shipped() {
        // The property that quietly stops being true the first time someone
        // adds a font. So it is checked, not assumed.
        for reaching in [
            "<script src=\"https://cdn.example/x.js\"></script>",
            "<link rel=\"stylesheet\" href=\"/x.css\">",
            "<style>@import url(http://example/x.css)</style>",
        ] {
            assert!(
                !external_references(reaching).is_empty(),
                "should have been caught: {reaching}"
            );
        }
        assert!(external_references("<p>a local <code>path/to/file</code></p>").is_empty());
    }

    #[test]
    fn a_kind_has_one_stable_name_so_a_rerender_replaces() {
        let dir = tmpdir("artifact-stable");
        let records = journal();
        let projection = replay(&records);

        for _ in 0..3 {
            let artifact =
                try_render(Kind::Board, &projection, &records, &provenance()).expect("render");
            artifact.write(&dir).expect("write");
        }
        let files: Vec<String> = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, ["board.html"], "three renders, one file (`A-2`)");
    }

    #[test]
    fn journal_text_is_escaped_rather_than_pasted_into_the_page() {
        let records = journal();
        let projection = replay(&records);
        let artifact =
            try_render(Kind::GateEvidence, &projection, &records, &provenance()).expect("render");
        assert!(
            artifact.html.contains("mismatched &lt;types&gt; &amp; such"),
            "a transcript is arbitrary text: {}",
            &artifact.html[..400.min(artifact.html.len())]
        );
        assert!(!artifact.html.contains("<types>"));
    }

    #[test]
    fn publishing_outside_the_workspace_needs_an_approval() {
        let inside = Destination::Workspace(PathBuf::from(DIR));
        assert_eq!(publish_policy(&inside), crate::approval::Policy::Auto, "rendering is free");

        // Including the ones that call themselves private.
        for outside in ["a secret gist", "an unlisted bucket", "the team wiki"] {
            let policy = publish_policy(&Destination::Outside(outside.into()));
            let crate::approval::Policy::Approve { reason } = policy else {
                panic!("{outside} must need an approval (`A-4`)");
            };
            assert!(reason.contains("posting publicly"), "{reason}");
        }
    }

    #[test]
    fn a_render_that_fails_is_a_warning_and_not_a_failure() {
        // `A-7` as a type: `try_render` cannot return a crate `Error`, so no
        // caller can `?` an artifact failure into a step failure.
        let warning = Warning { kind: Kind::Diagram, why: "no data".into() };
        let text = format!("{warning}");
        assert!(text.contains("not a failure"), "{text}");
    }

    #[test]
    fn an_empty_evidence_bundle_says_it_is_empty() {
        let projection = replay(&[]);
        let artifact = try_render(Kind::GateEvidence, &projection, &[], &provenance()).expect("render");
        assert!(
            artifact.html.contains("no evidence to bundle"),
            "an empty bundle must not look like a full one"
        );
    }

    #[test]
    fn provenance_names_the_links_that_actually_answered() {
        let entry = crate::cost::Entry {
            step: step(1).to_string(),
            role: "build".into(),
            link: "here".into(),
            model: "m".into(),
            usage: crate::cost::Usage::from_reply(10, 5, 0, 0),
            latency_ms: 100,
            charge: 0.0,
        };
        let records = vec![crate::cost::annotate(
            Record::outcome(step(1), T, true, "call"),
            &entry,
        )];
        let provenance = Provenance::from_journal(3, T, &records);
        assert_eq!(provenance.links, ["here"]);

        // And when nothing came from a model, it says so rather than leaving a
        // blank that reads as "unknown".
        let empty = Provenance::from_journal(3, T, &[]);
        assert!(empty.render().contains("nothing in this report came from a model"));
    }

    #[test]
    fn an_unknown_kind_names_the_ones_that_exist() {
        let err = Kind::parse("dashboard").expect_err("must refuse");
        let text = format!("{err}");
        assert!(text.contains("board"), "{text}");
        assert!(text.contains("There are 7"), "{text}");
    }
}
