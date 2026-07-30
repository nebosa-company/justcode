// The Perpetum panel (`I-1`–`I-5`).
//
// A view onto the harness's journal and nothing else. It holds no state of its
// own, so closing the editor cannot stop a loop and reopening cannot lose one —
// re-attaching is just reading the file again.
//
// JustCode must build, start and work with `crates/` deleted, so every path
// through here treats a missing harness as an ordinary answer rather than an
// error: the panel says the harness is not installed and gets out of the way.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { iconMarkup } from "./icons.js";
import { t } from "./i18n.js";
import { blocks } from "./replytext.js";

// The safety net, not the mechanism. `perp:changed` from the watcher is what
// normally triggers a re-read; this catches a workspace whose binding moves the
// journal somewhere the watcher does not look.
const REFRESH_MS = 15000;

let state = {
  root: null,
  view: null,
  error: null,
  installed: true,
  tab: "timeline",
  timer: null,
  // What the last `/btw` was answered with, so sending gives a visible result.
  sent: null,
  // What `perp bind` last complained about, if anything.
  setupProblem: null,
  sending: false,
  // A chat turn in flight. The question is held here so it can be shown the
  // moment it is asked: it reaches the journal only when the turn is written,
  // and a chat window that shows nothing until the answer arrives feels broken
  // rather than busy.
  asking: false,
  asked: null,
  askError: null,
};

let unlisten = null;

/** What the panel should do about a file that changed under `.harness`.
 *
 * The watcher names the paths rather than saying "something moved", because the
 * work differs: a journal write is a new step to show, and a binding or links
 * edit means the setup itself is different and worth re-validating before the
 * next run trips over it.
 */
function reactTo(changed) {
  const paths = Array.isArray(changed) ? changed : [];
  const touched = (name) => paths.some((path) => path.endsWith(name));
  return {
    // No names at all still means re-read: an older harness emitted no payload,
    // and refusing to refresh would be worse than refreshing once too often.
    view: paths.length === 0 || touched("journal.jsonl") || touched("state.md") || touched(".html"),
    setup: touched("binding.md") || touched("links.md"),
    // A requirements file appearing or moving changes a menu, not a view. The
    // Harness menu builds its Requirements submenu from a cached list, and adding
    // a file in a subfolder left the menu showing yesterday's tree until the
    // active tab happened to change.
    structure:
      touched("binding.md") ||
      touched("links.md") ||
      paths.some((path) => path.startsWith("requirements/")),
  };
}

/** The watcher tells us when to re-read, rather than a timer guessing (`I-2`). */
async function startListening() {
  if (unlisten) return;
  try {
    unlisten = await listen("perp:changed", (event) => {
      if (!state.root) return;
      const what = reactTo(event.payload);
      if (what.view) refresh();
      if (what.setup) revalidate();
      if (what.structure && onSetupChanged) onSetupChanged();
    });
  } catch {
    // No event bridge is survivable — the interval below still runs.
    unlisten = null;
  }
}

/** Re-run `perp bind` after the binding or links changed.
 *
 * This is the command that answers "is the setup still good": it resolves every
 * path and reports a link whose credential is unset. Running it on the edit means
 * a typo is a line in the panel now, rather than a refusal an hour later when a
 * cycle is started.
 *
 * The result is a warning, never a failure. Editing a binding is a thing people
 * do in several saves, and a panel that turns red halfway through is a panel that
 * cries wolf.
 */
async function revalidate() {
  if (!state.root) return;
  try {
    await invoke("perp_run", { subcommand: "bind", args: [], root: state.root });
    state.setupProblem = null;
  } catch (error) {
    state.setupProblem = `${error}`;
  }
  render();
}

let host = null;

// The two nodes held across renders — the scrolling body and the composer —
// and the composer's own parts. [clearAbove] rebuilds them if the panel is
// mounted somewhere else, so a stale reference cannot survive.
let scroller = null;
let parts = null;

let onOpenArtifact = null;
let onShowTranscript = null;
let onProblems = null;
let onSetupChanged = null;

/** Wire the panel to the editor's own surfaces (`I-4`). */
export function configure({ openArtifact, showTranscript, reportProblems, setupChanged } = {}) {
  onOpenArtifact = openArtifact || null;
  onShowTranscript = showTranscript || null;
  onProblems = reportProblems || null;
  // Called when the workspace's own files move — a binding, a links file, a
  // requirements file. The panel does not own the menus, so it says so and lets
  // the editor decide what to rebuild.
  onSetupChanged = setupChanged || null;
}

export function mount(element) {
  host = element;
  render();
}

export function isOpen() {
  return Boolean(host && !host.hidden);
}

/** Point the panel at a workspace. Null closes it. */
export async function attach(root) {
  state.root = root;
  if (!root) {
    stopPolling();
    await invoke("perp_unwatch").catch(() => {});
    return;
  }
  await startListening();
  // Watching continues while the panel is closed, so reopening it is current
  // and the toggle can show that a step is open.
  await invoke("perp_watch", { root }).catch(() => {});
  await refresh();
  startPolling();
}

/** Hide the panel and keep observing.
 *
 * Deliberately not a stop. The thinking indicator lives in the status bar, so a
 * closed panel is not a reason to stop knowing — and reopening is then instant
 * rather than showing "Reading the journal…" against a run already in progress.
 * [`release`] is the actual stop, for when the workspace goes away.
 */
export function detach() {
  render();
}

/** The workspace is gone: stop watching and forget it. */
export async function release() {
  stopPolling();
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
  await invoke("perp_unwatch").catch(() => {});
  state = { ...state, view: null, error: null, root: null };
  render();
}

function startPolling() {
  stopPolling();
  // A backstop behind the watcher rather than the way the panel learns
  // anything. It runs whether or not the panel is showing, so the in-flight
  // signal on the toggle stays honest.
  state.timer = setInterval(() => refresh(), REFRESH_MS);
}

function stopPolling() {
  if (state.timer) clearInterval(state.timer);
  state.timer = null;
}

export async function refresh() {
  if (!state.root) return;
  try {
    const text = await invoke("perp_run", {
      subcommand: "panel",
      args: [],
      root: state.root,
    });
    state.view = JSON.parse(text);
    state.error = null;
    state.installed = true;
    if (onProblems) onProblems(problemsOf(state.view));
  } catch (e) {
    const message = String(e);
    state.installed = !message.startsWith("not-installed:");
    state.error = message.replace(/^not-installed:\s*/, "");
    state.view = null;
  }
  render();
}

function problemsOf(view) {
  if (!view) return [];
  return view.timeline
    .filter((entry) => entry.ok === false)
    .map((entry) => ({
      step: entry.step,
      message: entry.summary,
      detail: entry.transcript || "",
    }));
}

export function selectTab(name) {
  const moved = state.tab !== name;
  state.tab = name;
  render();
  if (!moved || !scroller) return;
  // Keeping your place is about the tab you are reading. A different one opens
  // where its newest entry is — the top for the lists that read newest first,
  // and the bottom for the conversation, which reads the other way.
  scroller.scrollTop = name === "chat" ? scroller.scrollHeight : 0;
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** Re-render with whatever `t()` now returns.
 *
 * Called when the interface language changes. The panel holds its view and draws
 * its own labels, so nothing needs re-reading — only redrawing.
 */
export function redraw() {
  render();
}

/** Whether a step is open right now, for callers that are not the panel. */
export function inFlight() {
  return Boolean(state.view && state.view.position && state.view.position.in_flight);
}

/** Clear the region a render owns, and leave what holds state where it is.
 *
 * The composers outlive a render, because a text field cannot survive being
 * redrawn — removing a focused element blurs it, so a message being typed
 * during a run lost the cursor and the half-typed text with it. So does the
 * body, because it is the scroll container: rebuilding it put someone reading
 * an earlier step back at the top every time the journal grew.
 *
 * The header and the tabs are pure projections of the view and are rebuilt.
 */
function clearAbove() {
  if (scroller && scroller.parentNode === host) {
    while (host.firstChild !== scroller) host.removeChild(host.firstChild);
    return;
  }
  host.replaceChildren();
  scroller = el("div", "perp-body");
  parts = { btw: buildComposer("btw"), chat: buildComposer("chat") };
  host.append(scroller, parts.btw.foot, parts.chat.foot);
}

/** Refill the body, and leave the reader where they were.
 *
 * The timeline is newest first, so what arrives during a run is prepended and
 * everything below it slides down by however much arrived. Restoring the old
 * offset alone would still move the row being read, so the height the list grew
 * by is added back. At the top the offset is zero and stays zero, which is where
 * someone following a run wants to be.
 */
function fill(paint) {
  const was = { top: scroller.scrollTop, height: scroller.scrollHeight };
  // Within a hair of the bottom counts as at the bottom: a fractional scroll
  // height is normal and would otherwise stop a conversation following itself.
  const wasAtEnd = was.height - scroller.clientHeight - was.top < 4;
  const next = el("div");
  paint(next);
  scroller.replaceChildren(...next.childNodes);

  // The conversation is the one list that reads oldest first, so what arrives
  // lands at the bottom and following it means staying there. Everywhere else
  // the newest is at the top, and what is wanted is to keep the row being read
  // from sliding down as entries are prepended above it.
  if (state.tab === "chat") {
    scroller.scrollTop = wasAtEnd ? scroller.scrollHeight : was.top;
    return;
  }
  scroller.scrollTop = was.top > 0 ? was.top + (scroller.scrollHeight - was.height) : 0;
}

function render() {
  // The status bar carries the signal too, so a run is visible with the panel
  // shut. `refresh` runs on the watcher's events whether or not anything is
  // showing, which is what makes this true rather than decorative.
  const badge = document.getElementById("status-perp");
  if (badge) {
    const step = state.view?.position?.in_flight;
    badge.hidden = !step;
    if (step) badge.textContent = t("panel.thinking", { step });
  }

  if (!host) return;
  clearAbove();
  const add = (node) => host.insertBefore(node, scroller);

  // Every early return still lands on [dressComposer], which is what hides it:
  // there is nothing to leave a note on without a workspace behind it. The
  // message goes in the body so there is one scroll container either way.
  if (!state.root) {
    fill((body) => body.append(el("p", "perp-empty", t("panel.noWorkspace"))));
  } else if (!state.installed) {
    // Absent, not broken. A blank panel reads as a bug.
    fill((body) => body.append(el("p", "perp-empty", state.error)));
  } else if (state.error) {
    fill((body) => body.append(el("p", "perp-error", state.error)));
  } else if (!state.view) {
    fill((body) => body.append(el("p", "perp-empty", t("panel.reading"))));
  } else {
    add(renderHeader(state.view));
    add(renderTabs());
    fill((body) => paintTab(body, state.view));
  }

  dressComposer();
}

function paintTab(body, view) {
  if (state.tab === "timeline") renderTimeline(body, view);
  else if (state.tab === "chat") renderChat(body, view);
  else if (state.tab === "approvals") renderApprovals(body, view);
  else if (state.tab === "diff") renderDiff(body, view);
  else if (state.tab === "btw") renderBtw(body, view);
  else if (state.tab === "artifacts") renderArtifacts(body, view);
}

/** `$0.04` at full strength and the rest of the digits quiet.
 *
 * Four decimal places are needed — a cycle can cost less than a cent — but only
 * the first two are a number anyone reads. Shown rather than rounded, because a
 * run that spent `$0.0004` and one that spent `$0.0400` must not both say
 * `$0.00`.
 */
function money(amount) {
  const text = amount.toFixed(4);
  const wrap = el("span", "perp-money");
  wrap.append(document.createTextNode(`$${text.slice(0, 4)}`));
  wrap.append(el("span", "perp-money-fraction", text.slice(4)));
  wrap.title = t("harness.spendHint", { total: `$${text}` });
  return wrap;
}

function renderHeader(view) {
  const header = el("div", "perp-header");
  const position = view.position;
  const where =
    position.cycle === null
      ? t("panel.nothingRecorded")
      : `cycle ${position.cycle} · ${position.stage ?? "—"}`;
  header.append(el("span", "perp-where", where));

  // Which workspace this is. `cycle 1 · b4` alone is ambiguous the moment two
  // projects' files are open, and the panel follows the active tab.
  if (state.root) {
    const name = state.root.replace(/[\/]+$/, "").split(/[\/]/).pop() || state.root;
    const label = el("span", "perp-root", name);
    label.title = state.root;
    header.append(label);
  }

  // What `perp bind` said the last time the binding or links changed. A warning
  // beside the workspace it is about, not an error that stops anything.
  if (state.setupProblem) {
    const problem = el("span", "perp-setup-problem", t("panel.bindingProblem"));
    problem.title = state.setupProblem;
    header.append(problem);
  }

  const counts = el("span", "perp-counts");
  const done = el("span", "perp-ok", t("panel.done", { n: position.done }));
  // What the number counts, because it is not obvious and was wrong until
  // recently: journalled steps, which is neither batches nor requirements. One
  // requirement is usually one step plus a share of a gate step.
  done.title = t("harness.doneHint");
  counts.append(done);
  if (position.blocked > 0) {
    counts.append(el("span", "perp-bad", t("panel.blocked", { n: position.blocked })));
  }
  counts.append(
    el("span", null, t("panel.gates", {
      green: view.spend.gates_green,
      total: view.spend.gates_run,
    })),
  );
  // Money is shown even at zero: a missing figure reads as unknown, a zero
  // reads as free, and a local-only cycle really is free.
  counts.append(money(Number(view.spend.money)));
  if (view.approvals > 0) {
    counts.append(el("span", "perp-bad", t("panel.awaiting", { n: view.approvals })));
  }
  header.append(counts);

  if (position.in_flight) {
    // A step being open is the one thing worth animating. The old signal was
    // italic text at 75% opacity, which looked the same as idle.
    const flight = el("span", "perp-flight");
    flight.append(el("span", "perp-pulse"));
    flight.append(el("span", null, t("panel.thinking", { step: position.in_flight })));
    header.append(flight);
  }
  return header;
}

/** The tabs, named once. The panel's strip and the Harness menu both read this.
 *
 * Keys rather than strings, resolved at render time: a label baked in at module
 * evaluation would freeze whatever locale had loaded by then.
 *
 * The hint is the second half of the name. "Diff" and "/btw" say nothing to
 * someone meeting the panel for the first time, and neither a tab strip nor a
 * menu row has room to explain itself.
 */
export const TABS = [
  { name: "timeline", icon: "timeline" },
  { name: "chat", icon: "chat" },
  { name: "approvals", icon: "approvals" },
  { name: "diff", icon: "diff" },
  { name: "btw", icon: "btw" },
  { name: "artifacts", icon: "artifacts" },
];

export function tabLabel(name) {
  return t(`harness.tab.${name}`);
}

export function tabHint(name) {
  return t(`harness.hint.${name}`);
}

/** Which tab is showing, so the menu can mark it. */
export function currentTab() {
  return state.tab;
}

/** Show a tab. Called from the Harness menu as well as from the strip. */
export function showTab(name) {
  selectTab(name);
}

function renderTabs() {
  const tabs = el("div", "perp-tabs");
  const view = state.view;
  const counts = {
    timeline: view.timeline.length,
    chat: view.chat.length,
    approvals: view.approvals_pending.length,
    diff: null,
    btw: view.btw.length,
    artifacts: view.artifacts.length,
  };
  for (const { name, icon } of TABS) {
    const label = tabLabel(name);
    const hint = tabHint(name);
    const count = counts[name];
    const button = el("button", name === state.tab ? "active" : null);
    button.type = "button";
    button.innerHTML = iconMarkup(icon);
    button.append(el("span", "perp-tab-label", count === null ? label : `${label} (${count})`));
    // The count belongs in the tooltip too — it is the tab's own state, and the
    // label is the first thing to go when the panel is dragged narrow.
    button.title = count === null ? `${label} — ${hint}` : `${label} (${count}) — ${hint}`;
    button.setAttribute("aria-label", button.title);
    button.addEventListener("click", () => selectTab(name));
    tabs.append(button);
  }

  const refreshButton = el("button", "perp-refresh");
  refreshButton.type = "button";
  refreshButton.innerHTML = iconMarkup("refresh");
  refreshButton.append(el("span", "perp-tab-label", t("harness.refresh")));
  refreshButton.title = t("harness.hint.refresh");
  refreshButton.setAttribute("aria-label", refreshButton.title);
  refreshButton.addEventListener("click", () => refresh());
  tabs.append(refreshButton);
  return tabs;
}

function renderTimeline(body, view) {
  if (!view.timeline.length) {
    body.append(el("p", "perp-empty", t("panel.emptyTimeline")));
    return;
  }
  const list = el("ul", "perp-timeline");
  // Newest first: the thing that just happened is the thing being looked for.
  for (const entry of [...view.timeline].reverse().slice(0, 200)) {
    // Accounting reads quieter than an event: the round-trip count is worth
    // seeing, but it is not something that happened.
    const tone = entry.ok === false ? "bad" : entry.kind === "calls" ? "calls" : null;
    const row = el("li", tone);
    const step = el("code", "perp-step", entry.step);
    // `at` is journalled in seconds; the panel has been showing an id with no
    // sense of when it happened.
    if (entry.at) step.title = new Date(entry.at * 1000).toLocaleString();
    row.append(step);
    row.append(el("span", "perp-summary", entry.summary));
    if (entry.requirements.length) {
      const reqs = el("span", "perp-reqs", entry.requirements.join(" "));
      // What the ids mean. One line each, so a row citing three of them explains
      // all three rather than making you go and look.
      const known = view.requirements || {};
      const said = entry.requirements
        .map((id) => (known[id] ? `${id} — ${known[id]}` : id))
        .join("\n");
      reqs.title = said;
      row.append(reqs);
    }
    if (entry.transcript && onShowTranscript) {
      // `I-4`: transcripts go to the terminal dock rather than a viewer
      // invented for this panel.
      const open = el("button", "perp-link", t("panel.transcript"));
      open.type = "button";
      open.addEventListener("click", () => onShowTranscript(entry.step, entry.transcript));
      row.append(open);
    }
    list.append(row);
  }
  body.append(list);
}

/** Leave a `/btw` on the loop's inbound channel (`O-6`, `C-10`). */
async function send(text) {
  const note = text.trim();
  if (!note || state.sending) return;
  state.sending = true;
  state.sent = null;
  render();
  try {
    state.sent = (
      await invoke("perp_run", {
        subcommand: "btw",
        args: [note, "--source", "panel"],
        root: state.root,
      })
    ).trim();
  } catch (error) {
    state.sent = `${error}`;
  } finally {
    state.sending = false;
  }
  await refresh();
}

/** Ask the model one question, and wait for the answer (`C-1`–`C-5`).
 *
 * The question is shown before the call rather than after it. A turn takes as
 * long as a model takes to think, and the journal has nothing to show for that
 * time — so without this the box would empty, nothing would appear, and the
 * only honest reading of the screen would be that the message was lost.
 *
 * Nothing is invented while waiting: what is drawn is the text just typed and a
 * marker saying an answer is outstanding. When the turn lands, both sides are
 * in the journal and [refresh] replaces the optimistic pair with the real ones.
 */
async function ask(text) {
  const question = text.trim();
  if (!question || state.asking) return;
  state.asking = true;
  state.asked = question;
  state.askError = null;
  render();
  try {
    await invoke("perp_chat", { message: question, root: state.root });
  } catch (error) {
    // The harness's own words: an unset credential (`M-24`) or a link that
    // could not be resolved both say exactly what is wrong.
    state.askError = `${error}`.replace(/^perp:\s*/, "");
  } finally {
    state.asking = false;
    state.asked = null;
  }
  await refresh();
}

/** A composer, built once for the life of the mount.
 *
 * One per tab that has one, because the two do different things with what is
 * typed: `/btw` files an aside the loop may pick up, and `chat` asks a model and
 * waits for an answer. Sharing a box would mean a question typed on one tab
 * could be filed as an aside by switching to the other, which is a way to send
 * the wrong thing to the wrong place by accident.
 *
 * Their nodes are kept in [parts] and written to by [dressComposer] rather than
 * being made again each render. See [clearAbove] for why.
 */
function buildComposer(kind) {
  const foot = el("div", `perp-foot perp-foot-${kind}`);
  const form = el("form", "perp-composer");
  const input = el("input", "perp-input");
  input.type = "text";
  const button = el("button", "perp-send");
  button.type = "submit";
  form.append(input, button);

  // What the harness said back, verbatim. A note that was reclassified or
  // refused says so here rather than looking like it was accepted; a chat that
  // could not reach a link says why here rather than silently doing nothing.
  const sent = el("p", "perp-sent");

  // The boundary, stated where someone might expect more of it.
  const note = el("p", "perp-note");
  foot.append(form, sent, note);

  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const text = input.value;
    input.value = "";
    if (kind === "chat") await ask(text);
    else await send(text);
    // Clicking Send leaves focus on the button, and the next message usually
    // follows the first, so put the cursor back in the field either way.
    input.focus();
  });

  return { foot, input, button, sent, note };
}

/** Put the current state onto the composers without rebuilding them. */
function dressComposer() {
  if (!parts) return;
  const usable = Boolean(state.root && state.installed && !state.error && state.view);

  // `/btw` files an aside; the conversation tab talks to a model. Each box sits
  // under the list it adds to.
  parts.btw.foot.hidden = !(usable && state.tab === "btw");
  parts.chat.foot.hidden = !(usable && state.tab === "chat");

  // Read from `t()` on every pass, not once at build time: the panel is redrawn
  // when the interface language changes, and these are the only strings in it
  // that outlive a render.
  parts.btw.input.placeholder = t("panel.notePlaceholder");
  parts.btw.button.textContent = state.sending ? t("panel.sending") : t("panel.send");
  parts.btw.button.disabled = state.sending;
  parts.btw.note.textContent = t("panel.btwLimit");
  parts.btw.sent.textContent = state.sent || "";
  parts.btw.sent.hidden = !state.sent;

  parts.chat.input.placeholder = t("panel.askPlaceholder");
  parts.chat.button.textContent = state.asking ? t("panel.asking") : t("panel.send");
  parts.chat.button.disabled = state.asking;
  parts.chat.note.textContent = t("panel.chatLimit");
  parts.chat.sent.textContent = state.askError || "";
  parts.chat.sent.hidden = !state.askError;

  // Both fields stay live while something is in flight — [send] and [ask]
  // already refuse a second one, and disabling a focused input blurs it, which
  // is the bug this whole arrangement exists to avoid.
}

// Read-only. The composer used to be drawn here and now belongs to `/btw`; see
// [dressComposer] for why.
/** Who said it, in the reader's language.
 *
 * The journal stores a speaker as an identifier, and an identifier is not a
 * label. Unknown ones are shown as they are rather than swallowed: a speaker
 * this build has no word for is still worth seeing.
 */
/** Draw a reply's blocks as nodes.
 *
 * Every string here goes in through `textContent`, which is what makes this
 * safe: the panel never assembles markup from a model's words, so a reply
 * containing a tag is a reply displaying a tag. See `replytext.js`.
 */
function paintReply(into, text) {
  for (const block of blocks(text)) {
    if (block.kind === "code") {
      const pre = el("pre", "perp-code");
      // The language is shown rather than used: highlighting a model's code
      // would mean parsing it, and reading it does not need that.
      if (block.language) pre.dataset.language = block.language;
      pre.textContent = block.text;
      into.append(pre);
      continue;
    }
    if (block.kind === "rule") {
      into.append(el("hr", "perp-rule"));
      continue;
    }
    if (block.kind === "heading") {
      const level = Math.min(block.level + 2, 6);
      const heading = el(`h${level}`, "perp-heading");
      paintSpans(heading, block.spans);
      into.append(heading);
      continue;
    }
    if (block.kind === "list") {
      const list = el(block.ordered ? "ol" : "ul", "perp-list");
      for (const item of block.items) {
        const row = el("li");
        paintSpans(row, item);
        list.append(row);
      }
      into.append(list);
      continue;
    }
    if (block.kind === "quote") {
      const quote = el("blockquote", "perp-quote");
      paintSpans(quote, block.spans);
      into.append(quote);
      continue;
    }
    const para = el("p");
    paintSpans(para, block.spans);
    into.append(para);
  }
}

function paintSpans(into, list) {
  for (const span of list) {
    if (span.kind === "code") {
      into.append(el("code", "perp-inline-code", span.text));
    } else if (span.kind === "strong") {
      into.append(el("strong", null, span.text));
    } else if (span.kind === "em") {
      into.append(el("em", null, span.text));
    } else if (span.kind === "link") {
      // Shown, never clickable. A reply is untrusted content, and one click
      // from untrusted content to a browser is how a phishing link works.
      into.append(el("span", "perp-link-text", span.text));
      if (span.href && span.href !== span.text) {
        into.append(el("span", "perp-link-href", ` (${span.href})`));
      }
    } else {
      into.append(document.createTextNode(span.text));
    }
  }
}

function speakerLabel(speaker) {
  const known = { operator: "panel.speakerOperator", assistant: "panel.speakerAssistant" };
  return known[speaker] ? t(known[speaker]) : speaker;
}

function renderChat(body, view) {
  if (!view.chat.length && !state.asked) {
    body.append(
      el("p", "perp-empty", t("panel.emptyChat")),
    );
    return;
  }
  const list = el("div", "perp-chat");
  for (const line of view.chat) {
    const turn = el("div", `perp-turn ${line.speaker}`);
    turn.append(el("span", "perp-speaker", speakerLabel(line.speaker)));
    if (line.speaker === "assistant") {
      // A reply arrives as Markdown. The operator's own words are shown as
      // typed: rendering what someone just wrote back at them changes it.
      const said = el("div", "perp-said");
      paintReply(said, line.text);
      turn.append(said);
    } else {
      turn.append(el("p", null, line.text));
    }
    if (line.partial) {
      // The half a model produced before someone stopped it. Kept, and
      // labelled — it is exactly the interesting half when an answer was
      // going wrong.
      turn.append(el("span", "perp-partial", t("panel.interrupted")));
    }
    list.append(turn);
  }

  // The question just asked, and a marker that an answer is outstanding. Held
  // in memory rather than read from the journal because it is not in the
  // journal yet — see [ask]. Both are replaced by the real records the moment
  // the turn lands, so nothing here can disagree with the journal for longer
  // than the call takes.
  if (state.asked) {
    // Only until the journal has it. `perp chat` writes the operator's turn as
    // soon as it is asked and long before the model answers, and the watcher
    // brings it back within the same second — so drawing the held copy as well
    // showed the question twice, once from memory and once from the record.
    const last = view.chat[view.chat.length - 1];
    const journalled = last && last.speaker === "operator" && last.text === state.asked;
    if (!journalled) {
      const mine = el("div", "perp-turn operator pending");
      mine.append(el("span", "perp-speaker", speakerLabel("operator")));
      mine.append(el("p", null, state.asked));
      list.append(mine);
    }

    const waiting = el("div", "perp-turn assistant pending");
    waiting.append(el("span", "perp-speaker", speakerLabel("assistant")));
    const dots = el("p", "perp-typing");
    // Three of them, animated in CSS, so the wait reads as work rather than as
    // nothing happening. `prefers-reduced-motion` stops the animation and
    // leaves the text.
    dots.append(el("span", "perp-dot"), el("span", "perp-dot"), el("span", "perp-dot"));
    dots.append(el("span", "perp-typing-said", t("panel.chatThinking")));
    waiting.append(dots);
    list.append(waiting);
  }

  body.append(list);
}

// `I-3`: approving from the panel opens the diff first. The button only exists
// when there is a diff behind it — one offered next to a pane that failed to
// load is exactly what the requirement was written to prevent. The operator
// confirms what they can see, and if they can see nothing they should not be
// confirming.
function renderApprovals(body, view) {
  if (!view.approvals_pending.length) {
    body.append(el("p", "perp-empty", t("harness.noApprovals")));
    return;
  }
  for (const pending of view.approvals_pending) {
    const card = el("div", "perp-approval");
    card.append(el("div", "perp-what", pending.what));
    card.append(el("p", "perp-why", pending.why));

    if (pending.reviewable) {
      const diff = el("pre", "perp-diff", pending.diff);
      card.append(diff);
      const approve = el("button", "perp-approve", t("panel.approveId", { id: pending.id }));
      approve.type = "button";
      // Deliberately not wired to an action. Approvals never arrive over a
      // channel (`O-6`), and the panel is a view (`I-5`) — this tells the
      // operator the command to type, at the machine, with the diff in front
      // of them.
      approve.addEventListener("click", () => {
        window.alert(
          `Run this at the machine:

  perp approve ${pending.id} --approve "<your name>"

` +
            "Approvals are never sent from a panel.",
        );
      });
      card.append(approve);
    } else {
      card.append(el("p", "perp-empty", pending.why_not));
    }
    body.append(card);
  }
}

function renderDiff(body, view) {
  if (!view.diff) {
    // Null, not empty: "nothing to show" and "no changes" are different, and a
    // blank pane reads as the second.
    body.append(el("p", "perp-empty", t("panel.emptyDiff")));
    return;
  }
  body.append(el("pre", "perp-diff", view.diff));
}

function renderBtw(body, view) {
  if (!view.btw.length) {
    body.append(el("p", "perp-empty", t("panel.emptyBtw")));
    return;
  }
  const list = el("ul", "perp-btw");
  for (const item of view.btw) {
    const row = el("li");
    row.append(el("span", "perp-class", item.class));
    row.append(el("span", "perp-summary", item.text));
    row.append(el("span", "perp-reqs", t("panel.fromSource", { source: item.source })));
    list.append(row);
  }
  body.append(list);
}

function renderArtifacts(body, view) {
  if (!view.artifacts.length) {
    body.append(el("p", "perp-empty", t("panel.emptyArtifacts")));
    return;
  }
  const list = el("ul", "perp-artifacts");
  for (const name of view.artifacts) {
    const row = el("li");
    const open = el("button", "perp-link", name);
    open.type = "button";
    open.addEventListener("click", () => {
      if (onOpenArtifact) onOpenArtifact(`.harness/artifacts/${name}`);
    });
    row.append(open);
    list.append(row);
  }
  body.append(list);
}
