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
  state.tab = name;
  render();
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

function render() {
  // The status bar carries the signal too, so a run is visible with the panel
  // shut. `refresh` runs on the watcher's events whether or not anything is
  // showing, which is what makes this true rather than decorative.
  const badge = document.getElementById("status-perp");
  if (badge) {
    const step = state.view?.position?.in_flight;
    badge.hidden = !step;
    if (step) badge.textContent = `thinking · ${step}`;
  }

  if (!host) return;
  host.replaceChildren();

  if (!state.root) {
    host.append(el("p", "perp-empty", t("panel.noWorkspace")));
    return;
  }
  if (!state.installed) {
    // Absent, not broken. A blank panel reads as a bug.
    host.append(el("p", "perp-empty", state.error));
    return;
  }
  if (state.error) {
    host.append(el("p", "perp-error", state.error));
    return;
  }
  if (!state.view) {
    host.append(el("p", "perp-empty", t("panel.reading")));
    return;
  }

  host.append(renderHeader(state.view));
  host.append(renderTabs());

  const body = el("div", "perp-body");
  const view = state.view;
  if (state.tab === "timeline") renderTimeline(body, view);
  else if (state.tab === "chat") renderChat(body, view);
  else if (state.tab === "approvals") renderApprovals(body, view);
  else if (state.tab === "diff") renderDiff(body, view);
  else if (state.tab === "btw") renderBtw(body, view);
  else if (state.tab === "artifacts") renderArtifacts(body, view);
  host.append(body);
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
    const problem = el("span", "perp-setup-problem", "binding");
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
    flight.append(el("span", null, `thinking · ${position.in_flight}`));
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
      const open = el("button", "perp-link", "transcript");
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

function renderComposer(body) {
  const form = el("form", "perp-composer");
  const input = el("input", "perp-input");
  input.type = "text";
  input.placeholder = "Leave a note for the loop…";
  input.disabled = state.sending;
  const button = el("button", "perp-send", state.sending ? "Sending…" : "Send");
  button.type = "submit";
  button.disabled = state.sending;
  form.append(input, button);
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    const text = input.value;
    input.value = "";
    await send(text);
  });
  body.append(form);

  // What the harness said back, verbatim. A note that was reclassified or
  // refused says so here rather than looking like it was accepted.
  if (state.sent) body.append(el("p", "perp-sent", state.sent));

  // The boundary, stated where someone might expect more of it. This is not a
  // way to steer a run: `/btw` is the only thing that can arrive from outside,
  // and it cannot approve, pause or redirect.
  body.append(
    el(
      "p",
      "perp-note",
      t("panel.btwLimit"),
    ),
  );
}

function renderChat(body, view) {
  if (!view.chat.length) {
    body.append(
      el("p", "perp-empty", t("panel.emptyChat")),
    );
    renderComposer(body);
    return;
  }
  const list = el("div", "perp-chat");
  for (const line of view.chat) {
    const turn = el("div", `perp-turn ${line.speaker}`);
    turn.append(el("span", "perp-speaker", line.speaker));
    turn.append(el("p", null, line.text));
    if (line.partial) {
      // The half a model produced before someone stopped it. Kept, and
      // labelled — it is exactly the interesting half when an answer was
      // going wrong.
      turn.append(el("span", "perp-partial", "interrupted"));
    }
    list.append(turn);
  }
  body.append(list);
  renderComposer(body);
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
      const approve = el("button", "perp-approve", `Approve #${pending.id}`);
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
    row.append(el("span", "perp-reqs", `from ${item.source}`));
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
