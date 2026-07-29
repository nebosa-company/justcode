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
  sending: false,
};

let unlisten = null;

/** The watcher tells us when to re-read, rather than a timer guessing (`I-2`). */
async function startListening() {
  if (unlisten) return;
  try {
    unlisten = await listen("perp:changed", () => {
      if (state.root) refresh();
    });
  } catch {
    // No event bridge is survivable — the interval below still runs.
    unlisten = null;
  }
}

let host = null;
let onOpenArtifact = null;
let onShowTranscript = null;
let onProblems = null;

/** Wire the panel to the editor's own surfaces (`I-4`). */
export function configure({ openArtifact, showTranscript, reportProblems } = {}) {
  onOpenArtifact = openArtifact || null;
  onShowTranscript = showTranscript || null;
  onProblems = reportProblems || null;
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
    host.append(el("p", "perp-empty", "No workspace open."));
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
    host.append(el("p", "perp-empty", "Reading the journal…"));
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

function renderHeader(view) {
  const header = el("div", "perp-header");
  const position = view.position;
  const where =
    position.cycle === null
      ? "nothing recorded yet"
      : `cycle ${position.cycle} · ${position.stage ?? "—"}`;
  header.append(el("span", "perp-where", where));

  const counts = el("span", "perp-counts");
  counts.append(el("span", "perp-ok", `${position.done} done`));
  if (position.blocked > 0) {
    counts.append(el("span", "perp-bad", `${position.blocked} blocked`));
  }
  counts.append(
    el("span", null, `gates ${view.spend.gates_green}/${view.spend.gates_run}`),
  );
  // Money is shown even at zero: a missing figure reads as unknown, a zero
  // reads as free, and a local-only cycle really is free.
  counts.append(el("span", null, `$${Number(view.spend.money).toFixed(4)}`));
  if (view.approvals > 0) {
    counts.append(el("span", "perp-bad", `${view.approvals} awaiting approval`));
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

function renderTabs() {
  const tabs = el("div", "perp-tabs");
  const view = state.view;
  const entries = [
    ["timeline", `Timeline (${view.timeline.length})`],
    ["chat", `Chat (${view.chat.length})`],
    ["approvals", `Approvals (${view.approvals_pending.length})`],
    ["diff", "Diff"],
    ["btw", `/btw (${view.btw.length})`],
    ["artifacts", `Artifacts (${view.artifacts.length})`],
  ];
  for (const [name, label] of entries) {
    const button = el("button", name === state.tab ? "active" : null, label);
    button.type = "button";
    button.addEventListener("click", () => selectTab(name));
    tabs.append(button);
  }

  const refreshButton = el("button", "perp-refresh", "Refresh");
  refreshButton.type = "button";
  refreshButton.addEventListener("click", () => refresh());
  tabs.append(refreshButton);
  return tabs;
}

function renderTimeline(body, view) {
  if (!view.timeline.length) {
    body.append(el("p", "perp-empty", "Nothing in the journal yet."));
    return;
  }
  const list = el("ul", "perp-timeline");
  // Newest first: the thing that just happened is the thing being looked for.
  for (const entry of [...view.timeline].reverse().slice(0, 200)) {
    const row = el("li", entry.ok === false ? "bad" : null);
    row.append(el("code", "perp-step", entry.step));
    row.append(el("span", "perp-summary", entry.summary));
    if (entry.requirements.length) {
      row.append(el("span", "perp-reqs", entry.requirements.join(" ")));
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
      "Notes become `/btw` items. They cannot approve, pause or redirect a run.",
    ),
  );
}

function renderChat(body, view) {
  if (!view.chat.length) {
    body.append(
      el("p", "perp-empty", "No conversation yet. `perp chat` writes into this journal."),
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
    body.append(el("p", "perp-empty", "Nothing waiting on a person."));
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
    body.append(el("p", "perp-empty", "No changes in the working tree."));
    return;
  }
  body.append(el("pre", "perp-diff", view.diff));
}

function renderBtw(body, view) {
  if (!view.btw.length) {
    body.append(el("p", "perp-empty", "Nothing waiting."));
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
    body.append(el("p", "perp-empty", "None rendered. `perp artifact all` writes them."));
    return;
  }
  const list = el("ul", "perp-artifacts");
  for (const name of view.artifacts) {
    const row = el("li");
    const open = el("button", "perp-link", name);
    open.type = "button";
    open.addEventListener("click", () => {
      if (onOpenArtifact) onOpenArtifact(`docs/perpetum/artifacts/${name}`);
    });
    row.append(open);
    list.append(row);
  }
  body.append(list);
}
