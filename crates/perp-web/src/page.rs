//! The page, as one string (`L-34`).
//!
//! Self-contained: inlined CSS, inlined script, no CDN and no build step —
//! `A-5`'s rule for artifacts, applied here for the same reason. A front end
//! over a harness that is proud of having almost no dependencies should not
//! need a network to draw itself.
//!
//! It renders in the browser rather than on the server because everything it
//! shows comes from one `/api/view` document, and re-fetching that after a
//! write is the whole of its state management. The server has no session, no
//! template and no second copy of the truth — the same decision `I-5` makes for
//! the editor's panel, for the same reason.

pub const HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Perpetum — requirements</title>
<style>
  :root {
    color-scheme: light dark;
    --bg: #fbfbfa; --panel: #ffffff; --ink: #1b1b1a; --dim: #6b6b66;
    --line: #e3e1dc; --accent: #2f6f4f; --warn: #9a5b12; --stop: #9a2b2b;
    --shade: rgba(0,0,0,.05);
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --bg: #16171a; --panel: #1e2024; --ink: #e8e6e1; --dim: #9a9890;
      --line: #2e3137; --accent: #6fbf90; --warn: #d8a25a; --stop: #e08585;
      --shade: rgba(255,255,255,.05);
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; background: var(--bg); color: var(--ink);
    font: 15px/1.55 ui-sans-serif, system-ui, "Segoe UI", sans-serif;
  }
  header {
    padding: 18px 24px 14px; border-bottom: 1px solid var(--line);
    display: flex; align-items: baseline; gap: 14px; flex-wrap: wrap;
  }
  header h1 { font-size: 17px; margin: 0; font-weight: 600; letter-spacing: -.01em; }
  header .path { color: var(--dim); font-size: 12.5px; font-family: ui-monospace, Consolas, monospace; }
  main {
    display: grid; grid-template-columns: minmax(0,1.75fr) minmax(300px,1fr);
    gap: 24px; padding: 22px 24px 60px; align-items: start;
  }
  @media (max-width: 900px) { main { grid-template-columns: 1fr; } }
  section { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; }
  section > h2 {
    font-size: 12px; text-transform: uppercase; letter-spacing: .08em;
    color: var(--dim); margin: 0; padding: 13px 16px; border-bottom: 1px solid var(--line);
  }
  .pad { padding: 16px; }

  /* --- left: the list ------------------------------------------------ */
  form.compose { display: flex; gap: 8px; padding: 14px 16px; border-bottom: 1px solid var(--line); }
  input[type=text] {
    flex: 1; min-width: 0; padding: 9px 11px; border-radius: 7px;
    border: 1px solid var(--line); background: var(--bg); color: var(--ink); font: inherit;
  }
  input[type=text]:focus { outline: 2px solid var(--accent); outline-offset: -1px; }
  button {
    padding: 9px 13px; border-radius: 7px; border: 1px solid var(--line);
    background: var(--shade); color: var(--ink); font: inherit; cursor: pointer;
  }
  button:hover { border-color: var(--accent); }
  button:disabled { opacity: .5; cursor: default; }
  button.quiet { padding: 3px 8px; font-size: 12px; color: var(--dim); background: none; border-color: transparent; }
  button.quiet:hover { color: var(--ink); border-color: var(--line); }
  button.danger:hover { color: var(--stop); border-color: var(--stop); }

  .req { padding: 11px 16px; border-bottom: 1px solid var(--line); }
  .req:last-child { border-bottom: 0; }
  .req .head { display: flex; align-items: center; gap: 9px; }
  .id { font-family: ui-monospace, Consolas, monospace; font-size: 12.5px; color: var(--dim); min-width: 46px; }
  .state {
    font-size: 11px; padding: 1px 7px; border-radius: 99px; border: 1px solid var(--line);
    color: var(--dim); white-space: nowrap;
  }
  .state.done { color: var(--accent); border-color: currentColor; }
  .state.gated, .state.blocked { color: var(--warn); border-color: currentColor; }
  .state.conflicting { color: var(--stop); border-color: currentColor; }
  .name { flex: 1; min-width: 0; }
  .req.is-done .name, .req.is-wont .name { color: var(--dim); }
  .req .tools { display: flex; gap: 2px; opacity: 0; transition: opacity .1s; }
  .req:hover .tools, .req:focus-within .tools { opacity: 1; }
  .req details { margin: 6px 0 0 55px; }
  .req details summary { font-size: 12px; color: var(--dim); cursor: pointer; }
  .req details p { margin: 6px 0 0; color: var(--dim); font-size: 13.5px; }
  .req form.edit { display: flex; gap: 8px; margin: 8px 0 2px 55px; }

  /* --- right: progress and screenshots -------------------------------- */
  .pct { font-size: 40px; font-weight: 600; letter-spacing: -.03em; line-height: 1; }
  .pct small { font-size: 14px; font-weight: 400; color: var(--dim); margin-left: 8px; letter-spacing: 0; }
  .bar { height: 8px; border-radius: 99px; background: var(--shade); margin: 14px 0 16px; overflow: hidden; }
  .bar > i { display: block; height: 100%; background: var(--accent); }
  .states { display: flex; flex-direction: column; gap: 5px; }
  .states div { display: flex; justify-content: space-between; font-size: 13px; }
  .states span:last-child { font-family: ui-monospace, Consolas, monospace; color: var(--dim); }

  figure { margin: 0 0 16px; }
  figure img { width: 100%; border-radius: 7px; border: 1px solid var(--line); display: block; background: var(--shade); }
  figcaption { font-size: 12.5px; color: var(--dim); margin-top: 6px; }
  figcaption b { color: var(--ink); font-weight: 500; }
  .swapped { color: var(--stop); }

  /* --- the ideas dialog ----------------------------------------------- */
  dialog {
    border: 1px solid var(--line); border-radius: 12px; background: var(--panel);
    color: var(--ink); padding: 0; width: min(640px, calc(100vw - 32px)); max-height: 88vh;
    overflow: auto; font: inherit;
  }
  dialog::backdrop { background: rgba(0,0,0,.45); }
  dialog h3 { margin: 0; padding: 16px 18px 12px; font-size: 15px; font-weight: 600; }
  dialog .body { padding: 0 18px 18px; display: flex; flex-direction: column; gap: 12px; }
  dialog textarea {
    width: 100%; min-height: 110px; resize: vertical; padding: 10px 12px; border-radius: 8px;
    border: 1px solid var(--line); background: var(--bg); color: var(--ink); font: inherit;
  }
  dialog textarea:focus { outline: 2px solid var(--accent); outline-offset: -1px; }
  dialog .row { display: flex; gap: 8px; justify-content: flex-end; align-items: center; }
  dialog .row .grow { flex: 1; min-width: 0; }
  .cand { display: flex; gap: 10px; align-items: flex-start; padding: 9px 0; border-bottom: 1px solid var(--line); }
  .cand:last-of-type { border-bottom: 0; }
  .cand input[type=checkbox] { margin-top: 11px; width: 16px; height: 16px; accent-color: var(--accent); flex: none; }
  .cand input[type=text] { flex: 1; min-width: 0; }
  .where { font-size: 12.5px; }
  .where.cloud { color: var(--warn); }
  /* Narrow screens: the dialog is the page, and the buttons are thumb-sized. */
  @media (max-width: 560px) {
    dialog { width: 100vw; max-width: 100vw; max-height: 100vh; height: 100vh; border-radius: 0; border: 0; }
    dialog .row { flex-wrap: wrap; }
    dialog .row button, dialog .row .grow { min-height: 44px; }
    .cand input[type=text] { font-size: 16px; }
  }

  .note { color: var(--dim); font-size: 13px; }
  .say { padding: 10px 16px; border-bottom: 1px solid var(--line); font-size: 13px; white-space: pre-wrap; }
  .say.bad { color: var(--stop); }
  footer { padding: 0 24px 30px; color: var(--dim); font-size: 12.5px; max-width: 70ch; }
</style>
</head>
<body>
<header>
  <h1>Perpetum</h1>
  <span class="path" id="root"></span>
</header>

<main>
  <section>
    <h2>Requirements</h2>
    <div id="say"></div>
    <form class="compose" id="compose">
      <input type="text" id="new" placeholder="What should it do?" required autocomplete="off">
      <button type="submit" id="file">File it</button>
      <button type="button" id="ideas">Ideas</button>
    </form>
    <div id="list"></div>
  </section>

  <div style="display:flex;flex-direction:column;gap:24px">
    <section>
      <h2>Progress</h2>
      <div class="pad">
        <div class="pct"><span id="pct">–</span>%<small id="ratio"></small></div>
        <div class="bar"><i id="fill" style="width:0"></i></div>
        <div class="states" id="states"></div>
      </div>
    </section>

    <section>
      <h2>The product under construction</h2>
      <div class="pad" id="shots"></div>
    </section>
  </div>
</main>

<dialog id="ideabox">
  <h3>An idea, split into requirements</h3>
  <div class="body">
    <p class="note" id="ideawhere"></p>
    <textarea id="ideatext" placeholder="Say the idea in your own words. A paragraph is plenty."></textarea>
    <div class="row">
      <span class="grow note" id="ideasay"></span>
      <button type="button" id="ideaclose">Close</button>
      <button type="button" id="ideago">Derive requirements</button>
    </div>
    <div id="ideacands"></div>
    <div class="row" id="ideafilerow" hidden>
      <span class="grow note">Nothing is on the list until you file it. Edit any line first.</span>
      <button type="button" id="ideafile">File the ticked ones</button>
    </div>
  </div>
</dialog>

<footer id="why"></footer>

<script>
"use strict";

// Every write carries this header. A cross-origin form cannot set one and a
// cross-origin fetch that tries is stopped by a preflight the server refuses —
// which is what keeps another page in this browser from writing this
// workspace's requirements file.
const HEADERS = { "X-Perp-Web": "1", "Content-Type": "application/x-www-form-urlencoded" };

let view = null;
let editing = null;

const el = (tag, cls, text) => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

function say(message, bad) {
  const box = document.getElementById("say");
  box.textContent = "";
  if (!message) return;
  const line = el("div", bad ? "say bad" : "say", message);
  box.append(line);
  if (!bad) setTimeout(() => line.remove(), 6000);
}

async function load() {
  const response = await fetch("/api/view", { headers: { "X-Perp-Web": "1" } });
  if (!response.ok) { say(await response.text(), true); return; }
  view = await response.json();
  render();
}

// One door for every write, mirroring the server's: the page has no more ways
// to change the file than the allowlist has entries.
//
// It returns the message rather than showing it, because the same write is
// made from two places — the box on the list, and the ideas dialog on top of
// it — and a refusal announced behind a modal is a refusal nobody read.
async function write(action, fields) {
  const body = new URLSearchParams({ action, ...fields });
  const response = await fetch("/api/write", { method: "POST", headers: HEADERS, body });
  const text = await response.text();
  if (!response.ok) return { ok: false, message: text };
  const done = JSON.parse(text);
  await load();
  return { ok: true, message: done.note ? done.summary + "\n" + done.note : done.summary };
}

// The common case: make the write and put whatever came back on the list.
async function writeAndSay(action, fields) {
  const done = await write(action, fields);
  say(done.message, !done.ok);
  return done.ok;
}

function render() {
  document.getElementById("root").textContent = view.source;
  document.getElementById("new").placeholder =
    view.next_id ? "What should it do?  (files as " + view.next_id + ")" : "What should it do?";
  renderList();
  renderProgress();
  renderShots();
  document.getElementById("why").textContent =
    "This page writes the requirements source, which only a person may do. It can file, "
    + "reword, delete and ungate a row — " + view.writes.join(", ")
    + " — and nothing else. Marking work done is not on that list: a green marker means "
    + "gates passed with a transcript, and a button that could type one would make every "
    + "green here worth nothing.";
}

function renderList() {
  const list = document.getElementById("list");
  list.textContent = "";
  if (!view.catalogue.length) {
    list.append(el("div", "pad note", "This project's requirements source declares nothing yet."));
    return;
  }
  for (const item of view.catalogue) {
    const slug = item.state.replace(/[^a-z]+/g, "-");
    const row = el("div", "req is-" + (item.state === "won't do" ? "wont" : slug));

    const head = el("div", "head");
    head.append(el("span", "id", item.id));
    head.append(el("span", "state " + slug, item.state));
    head.append(el("span", "name", item.name));

    const tools = el("div", "tools");
    const edit = el("button", "quiet", "edit");
    edit.type = "button";
    edit.onclick = () => { editing = editing === item.id ? null : item.id; renderList(); };
    tools.append(edit);

    if (item.state === "gated") {
      const ungate = el("button", "quiet", "ungate");
      ungate.type = "button";
      ungate.onclick = () => writeAndSay("ungate", { id: item.id });
      tools.append(ungate);
    }

    const remove = el("button", "quiet danger", "delete");
    remove.type = "button";
    remove.onclick = () => {
      // The text, not the id: a person confirming `L-31` is confirming a
      // number they have to go and look up.
      if (window.confirm("Delete " + item.id + "?\n\n" + item.name)) {
        writeAndSay("delete", { id: item.id });
      }
    };
    tools.append(remove);
    head.append(tools);
    row.append(head);

    if (editing === item.id) {
      const form = el("form", "edit");
      const field = el("input");
      field.type = "text";
      field.value = item.text;
      field.required = true;
      const save = el("button", null, "Save");
      save.type = "submit";
      const cancel = el("button", "quiet", "cancel");
      cancel.type = "button";
      cancel.onclick = () => { editing = null; renderList(); };
      form.append(field, save, cancel);
      form.onsubmit = async (event) => {
        event.preventDefault();
        save.disabled = true;
        // Caught here so the message lands beside the field, and caught again
        // in the harness because this is not its only caller.
        if (field.value.includes("|")) {
          say("A requirement may not contain `|` — it ends the table row it is written into.", true);
          save.disabled = false;
          return;
        }
        if (await writeAndSay("edit", { id: item.id, text: field.value })) editing = null;
        save.disabled = false;
      };
      row.append(form);
      setTimeout(() => field.focus(), 0);
    } else if (item.text !== item.name) {
      const more = el("details");
      more.append(el("summary", null, "Read it in full"));
      more.append(el("p", null, item.text));
      row.append(more);
    }
    list.append(row);
  }
}

function renderProgress() {
  const p = view.progress;
  document.getElementById("pct").textContent = p.percent;
  document.getElementById("ratio").textContent = p.done + " of " + p.total + " done";
  document.getElementById("fill").style.width = p.percent + "%";
  const states = document.getElementById("states");
  states.textContent = "";
  for (const row of p.states) {
    const line = el("div");
    line.append(el("span", null, row.state));
    line.append(el("span", null, String(row.n)));
    states.append(line);
  }
}

function renderShots() {
  const box = document.getElementById("shots");
  box.textContent = "";

  if (view.product.declared) {
    const form = el("form");
    form.style.cssText = "display:flex;gap:8px;margin-bottom:16px";
    const claim = el("input");
    claim.type = "text";
    claim.placeholder = "What would this be evidence of?";
    claim.required = true;
    claim.style.cssText = "flex:1;min-width:0";
    const shoot = el("button", null, "Capture");
    shoot.type = "submit";
    form.append(claim, shoot);
    form.onsubmit = async (event) => {
      event.preventDefault();
      shoot.disabled = true;
      shoot.textContent = "Running…";
      const body = new URLSearchParams({ claim: claim.value });
      const response = await fetch("/api/capture", { method: "POST", headers: HEADERS, body });
      const text = await response.text();
      shoot.disabled = false;
      shoot.textContent = "Capture";
      if (!response.ok) { say(text, true); return; }
      claim.value = "";
      // The picture is of the screen, not of a window, and a window opened by
      // a background process does not always come to the front. Said here
      // rather than left for the person to work out from a screenshot with no
      // product in it.
      const done = JSON.parse(text);
      say(done.focused
        ? "Captured. The window was asked to come forward — if it did not, it was behind something."
        : "Captured — the screen as it was. Nothing was brought forward; set `product.window` to try.");
      await load();
    };
    box.append(form);
    const runs = el("p", "note", "Runs `" + view.product.command + "` and photographs the screen "
      + view.product.settle + "s later, then stops it.");
    runs.style.margin = "-8px 0 16px";
    box.append(runs);
  } else {
    box.append(el("p", "note", view.product.why));
  }

  if (!view.evidence.length) {
    box.append(el("p", "note", "No screenshots recorded yet."));
    return;
  }
  for (const shot of view.evidence) {
    const figure = el("figure");
    const img = el("img");
    img.src = "/evidence/" + encodeURIComponent(shot.file);
    img.alt = shot.claim;
    img.loading = "lazy";
    const caption = el("figcaption");
    caption.append(el("b", null, shot.claim));
    caption.append(el("br"));
    caption.append(document.createTextNode(
      new Date(shot.at * 1000).toLocaleString() + " · " + shot.step));
    // Evidence that cannot be checked is decoration. A file whose bytes no
    // longer hash to what was recorded is shown and labelled, never hidden.
    if (!shot.intact) {
      caption.append(el("br"));
      caption.append(el("span", "swapped", "this file no longer matches what was recorded"));
    }
    figure.append(img, caption);
    box.append(figure);
  }
}

/* --- the ideas dialog ------------------------------------------------------
 *
 * A model drafts and a person files, and the two are separate requests on
 * purpose. `/api/ideas` writes nothing; every candidate reaches the file only
 * by being ticked here and posted to `/api/write` as an ordinary
 * `requirement add` — the same door the box above uses. Which is why each
 * candidate is an editable field rather than a label: a person who cannot
 * change the wording before filing it is not the author of it, they are the
 * transport.
 */
const ideaBox = document.getElementById("ideabox");

function ideaSay(message, bad) {
  const line = document.getElementById("ideasay");
  line.textContent = message || "";
  line.style.color = bad ? "var(--stop)" : "";
}

document.getElementById("ideas").onclick = () => {
  const where = document.getElementById("ideawhere");
  where.className = "where note";
  if (!view || !view.ideas.available) {
    where.textContent = view ? view.ideas.why : "";
    where.classList.add("cloud");
  } else if (view.ideas.privacy === "cloud") {
    // Said before the box is typed in, not after it is sent. An idea is a
    // person's unpublished thought about their own product.
    where.classList.add("cloud");
    where.textContent = "This leaves the machine: `" + view.ideas.link + "` is a cloud link ("
      + view.ideas.model + "). Redaction and the egress allowlist apply, and it still leaves.";
  } else {
    where.textContent = "Drafted by `" + view.ideas.link + "` (" + view.ideas.model
      + "), which runs on hardware you own.";
  }
  document.getElementById("ideago").disabled = !(view && view.ideas.available);
  ideaBox.showModal();
};

document.getElementById("ideaclose").onclick = () => ideaBox.close();

document.getElementById("ideago").onclick = async () => {
  const text = document.getElementById("ideatext").value;
  const go = document.getElementById("ideago");
  if (!text.trim()) { ideaSay("Type the idea first.", true); return; }
  go.disabled = true;
  go.textContent = "Thinking…";
  ideaSay("");
  try {
    const response = await fetch("/api/ideas", {
      method: "POST", headers: HEADERS, body: new URLSearchParams({ idea: text }),
    });
    const body = await response.text();
    if (!response.ok) { ideaSay(body, true); return; }
    const draft = JSON.parse(body);
    renderCandidates(draft);
  } catch (e) {
    ideaSay(String(e), true);
  } finally {
    go.disabled = false;
    go.textContent = "Derive requirements";
  }
};

function renderCandidates(draft) {
  const list = document.getElementById("ideacands");
  list.textContent = "";
  for (const candidate of draft.candidates) {
    const row = el("div", "cand");
    const tick = el("input");
    tick.type = "checkbox";
    // Unticked. A dialog that arrives with everything selected is a dialog
    // whose default is "file all of it", and choosing is the whole point.
    tick.checked = false;
    const field = el("input");
    field.type = "text";
    field.value = candidate;
    row.append(tick, field);
    list.append(row);
  }
  document.getElementById("ideafilerow").hidden = draft.candidates.length === 0;
  // Counted, not swallowed: a draft that showed six of nine and said nothing
  // would have quietly edited what the model proposed.
  ideaSay(draft.candidates.length + " drafted by " + draft.via
    + (draft.dropped ? " · " + draft.dropped + " dropped, they carried a marker or a `|`" : ""));
}

document.getElementById("ideafile").onclick = async () => {
  const button = document.getElementById("ideafile");
  const rows = [...document.querySelectorAll("#ideacands .cand")]
    .filter((row) => row.querySelector("input[type=checkbox]").checked);
  if (!rows.length) { ideaSay("Tick the ones you want.", true); return; }
  button.disabled = true;
  let filed = 0;
  // One `requirement add` per pick, each minting its own id — never a batch
  // endpoint, because a batch endpoint is a second way to write the file.
  for (const row of rows) {
    const field = row.querySelector("input[type=text]");
    const done = await write("add", { text: field.value });
    if (!done.ok) {
      // Shown here rather than on the list behind the dialog, and the loop
      // stops: filing the rest would bury the one that failed.
      ideaSay(done.message, true);
      break;
    }
    filed += 1;
    row.remove();
  }
  button.disabled = false;
  if (filed) {
    ideaSay(filed + " filed, with no marker — only gates decide what is done.");
    document.getElementById("ideafilerow").hidden =
      !document.querySelector("#ideacands .cand");
  }
};

document.getElementById("compose").onsubmit = async (event) => {
  event.preventDefault();
  const field = document.getElementById("new");
  const button = document.getElementById("file");
  if (field.value.includes("|")) {
    say("A requirement may not contain `|` — it ends the table row it is written into.", true);
    return;
  }
  button.disabled = true;
  if (await writeAndSay("add", { text: field.value })) field.value = "";
  button.disabled = false;
};

load();
</script>
</body>
</html>
"##;
