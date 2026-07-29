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

const REFRESH_MS = 4000;

let state = {
  root: null,
  view: null,
  error: null,
  installed: true,
  tab: "timeline",
  timer: null,
};

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
    return;
  }
  await refresh();
  startPolling();
}

export function detach() {
  stopPolling();
  state = { ...state, view: null, error: null, root: null };
  render();
}

function startPolling() {
  stopPolling();
  // Polling rather than watching: the journal is append-only and a few seconds
  // of staleness costs nothing, while a file watcher on a directory the loop is
  // writing is a source of its own bugs.
  state.timer = setInterval(() => {
    if (isOpen()) refresh();
  }, REFRESH_MS);
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

function render() {
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
    header.append(el("span", "perp-flight", `in flight: ${position.in_flight}`));
  }
  return header;
}

function renderTabs() {
  const tabs = el("div", "perp-tabs");
  const view = state.view;
  const entries = [
    ["timeline", `Timeline (${view.timeline.length})`],
    ["chat", `Chat (${view.chat.length})`],
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

function renderChat(body, view) {
  if (!view.chat.length) {
    body.append(
      el("p", "perp-empty", "No conversation yet. `perp chat` writes into this journal."),
    );
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
