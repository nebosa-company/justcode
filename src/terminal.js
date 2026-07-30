// The integrated terminal panel.
//
// Each terminal is a real shell running in a pseudo-terminal on the Rust side;
// this module owns the xterm.js views and the panel chrome around them. Several
// terminals can be open at once, picked from a strip along the top of the panel.
//
// xterm.js and its stylesheet are imported dynamically, so the ~250KB they cost
// is only downloaded the first time a terminal is actually opened.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { iconMarkup, iconElement } from "./icons.js";
import { t } from "./i18n.js";
import { DEFAULT_FONT_SIZE } from "./editor.js";

/**
 * Profiles offered in the "new terminal" menu, most idiomatic first — the
 * first entry is what the Ctrl+Shift+` shortcut opens, so the order decides
 * which shell that shortcut means on each platform.
 *
 * macOS has defaulted to zsh since Catalina; listing bash first there gave a
 * shell most users have not configured, and on newer machines an old 3.2 build
 * kept only for licensing reasons.
 */
function profilesFor(agent) {
  if (agent.includes("Windows")) {
    return [
      { id: "powershell", label: "PowerShell" },
      { id: "cmd", label: "Command Prompt" },
    ];
  }
  if (agent.includes("Macintosh") || agent.includes("Mac OS")) {
    return [
      { id: "zsh", label: "zsh" },
      { id: "bash", label: "bash" },
      { id: "sh", label: "sh" },
    ];
  }
  return [
    { id: "bash", label: "bash" },
    { id: "sh", label: "sh" },
  ];
}

export const PROFILES = profilesFor(navigator.userAgent);

/** The shell Ctrl+Shift+` opens — named in the menu so the binding is obvious. */
export const DEFAULT_PROFILE = PROFILES[0];

// Kept in step with the editor's zoom by setFontSize below; the default only
// applies until the app has read the stored zoom level at startup.
let fontSize = DEFAULT_FONT_SIZE;

/** The edges the panel can be docked to; the first is the default. */
export const DOCKS = ["bottom", "left", "right"];

let xterm = null;
let panel = null;
let workspace = null;
let dock = DOCKS[0];
let tabsEl = null;
let viewsEl = null;
let onVisibilityChange = () => {};
let onDockChange = () => {};
let cwdProvider = () => null;
// Injected rather than imported, like every other outside dependency here, so
// this module stays testable and knows nothing about Tauri's clipboard.
let copyText = async () => false;

const terminals = new Map(); // id -> { id, title, term, fit, element, exited }
let activeId = null;
let nextId = 1;
// The tab currently being dragged for reordering, if any.
let draggedId = null;
// The in-flight (or completed) listener registration. A boolean flag here meant
// a single failed `listen()` left it permanently "done", so every later
// terminal ran with no output listener — alive but silent, with no way back.
let listenerSetup = null;

/** Loads xterm.js on first use. */
async function loadXterm() {
  if (xterm) return xterm;
  const [{ Terminal }, { FitAddon }] = await Promise.all([
    import("@xterm/xterm"),
    import("@xterm/addon-fit"),
    import("@xterm/xterm/css/xterm.css"),
  ]);
  xterm = { Terminal, FitAddon };
  return xterm;
}

/** The colours xterm should use, read from the app's own theme variables. */
function themeColours() {
  const styles = getComputedStyle(document.documentElement);
  const read = (name, fallback) => styles.getPropertyValue(name).trim() || fallback;
  return {
    background: read("--bg", "#1e1f26"),
    foreground: read("--fg", "#d7d7db"),
    cursor: read("--fg", "#d7d7db"),
    selectionBackground: read("--selection", "rgba(120,150,255,0.35)"),
  };
}

function buildPanel() {
  panel = document.createElement("div");
  panel.className = "terminal-panel";
  panel.hidden = true;

  const header = document.createElement("div");
  header.className = "terminal-header";

  tabsEl = document.createElement("div");
  tabsEl.className = "terminal-tabs";

  const actions = document.createElement("div");
  actions.className = "terminal-actions";

  const button = (icon, title, run) => {
    const element = document.createElement("button");
    element.className = "terminal-action";
    element.type = "button";
    element.title = title;
    // Icon only, no visible text — the accessible name has to come from
    // here rather than relying on the `title` fallback.
    element.setAttribute("aria-label", title);
    element.innerHTML = iconMarkup(icon);
    element.addEventListener("click", run);
    return element;
  };

  actions.append(
    button("terminalAdd", t("terminal.new"), () => openTerminal()),
    button("close", t("terminal.hide"), () => setVisible(false)),
  );

  // Double-clicking empty space in the strip opens a new terminal, mirroring
  // the editor pane tab bar's double-click-for-new-file gesture.
  tabsEl.addEventListener("dblclick", (event) => {
    if (event.target !== tabsEl) return;
    openTerminal();
  });
  // A plain wheel over the strip scrolls it horizontally, so tabs that have
  // scrolled out of view are reachable without the thin scrollbar.
  tabsEl.addEventListener(
    "wheel",
    (event) => {
      if (event.ctrlKey || event.deltaY === 0) return;
      if (tabsEl.scrollWidth <= tabsEl.clientWidth) return;
      event.preventDefault();
      tabsEl.scrollLeft += event.deltaY;
    },
    { passive: false },
  );

  header.append(tabsEl, actions);

  viewsEl = document.createElement("div");
  viewsEl.className = "terminal-views";

  panel.append(header, viewsEl);
  // Shares the workspace with the editor panes; which side of them it lands on
  // is the dock, applied below.
  workspace.append(panel);
  applyDock();

  // The panel is resizable by dragging the edge that faces the code — its top
  // when docked along the bottom, its inner side when docked to left or right.
  const grip = document.createElement("div");
  grip.className = "terminal-grip";
  panel.prepend(grip);
  let dragging = false;
  grip.addEventListener("mousedown", (event) => {
    dragging = true;
    event.preventDefault();
  });
  window.addEventListener("mousemove", (event) => {
    if (!dragging) return;
    // Releasing outside the webview delivers no mouseup, which used to leave
    // the panel resizing on every later mouse move with no button held.
    if (!(event.buttons & 1)) {
      dragging = false;
      return;
    }
    if (dock === "bottom") {
      const height = Math.min(Math.max(window.innerHeight - event.clientY, 80), window.innerHeight - 160);
      panel.style.height = `${height}px`;
    } else {
      const from = dock === "left" ? event.clientX : window.innerWidth - event.clientX;
      panel.style.width = `${Math.min(Math.max(from, 140), window.innerWidth - 240)}px`;
    }
    fitActive();
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
  });

  attachDockDrag(header);
}

/**
 * Dragging the header moves the panel to another edge. A plain mouse drag
 * rather than HTML5 drag-and-drop: the tabs inside the same header are already
 * draggable for reordering, and a second draggable wrapped around them makes
 * every tab drag ambiguous.
 */
function attachDockDrag(header) {
  let moving = false;

  header.addEventListener("mousedown", (event) => {
    // Only the empty parts of the header are a handle. A tab or a button in it
    // has its own job, and stealing their press would break both.
    if (event.button !== 0) return;
    if (event.target !== header && event.target !== tabsEl) return;
    moving = true;
    panel.classList.add("docking");
    event.preventDefault();
  });

  window.addEventListener("mousemove", (event) => {
    if (!moving) return;
    if (!(event.buttons & 1)) {
      moving = false;
      panel.classList.remove("docking");
      delete workspace.dataset.dockHint;
      return;
    }
    workspace.dataset.dockHint = edgeAt(event);
  });

  window.addEventListener("mouseup", (event) => {
    if (!moving) return;
    moving = false;
    panel.classList.remove("docking");
    delete workspace.dataset.dockHint;
    setDock(edgeAt(event));
  });
}

/**
 * The edge a pointer is asking for: whichever side of the workspace it is
 * nearest, measured as a fraction of the box so it behaves the same in a small
 * window as in a maximised one. The middle keeps the current dock, so a drag
 * that ends nowhere in particular changes nothing.
 */
function edgeAt(event) {
  const box = workspace.getBoundingClientRect();
  const x = (event.clientX - box.left) / box.width;
  const y = (event.clientY - box.top) / box.height;
  if (y > 0.7) return "bottom";
  if (x < 0.3) return "left";
  if (x > 0.7) return "right";
  return dock;
}

/** Puts the dock class on the workspace and clears sizes from the other one. */
function applyDock() {
  if (!workspace) return;
  workspace.classList.toggle("dock-left", dock === "left");
  workspace.classList.toggle("dock-right", dock === "right");
  if (!panel) return;
  // A height dragged while docked at the bottom would otherwise survive as an
  // inline style and fix the column's height once it moved to a side.
  panel.style.height = "";
  panel.style.width = "";
}

/** Which edge the panel is docked to: "bottom", "left" or "right". */
export function dockEdge() {
  return dock;
}

/** Moves the panel to `edge`, reporting whether that was a change. */
export function setDock(edge) {
  if (!DOCKS.includes(edge) || edge === dock) return false;
  dock = edge;
  applyDock();
  // The panel has changed shape, so xterm needs to be re-measured and the shell
  // told its new size — the same reason a resize drag calls this.
  relayout();
  onDockChange(dock);
  return true;
}

/** Attaches the panel to the layout. Call once, at startup. */
export function initTerminals({
  workspace: host,
  dock: initialDock,
  onVisibility,
  onDock,
  currentDirectory,
  copy,
}) {
  workspace = host;
  if (DOCKS.includes(initialDock)) dock = initialDock;
  buildPanel();
  onVisibilityChange = onVisibility || (() => {});
  onDockChange = onDock || (() => {});
  cwdProvider = currentDirectory || (() => null);
  copyText = copy || (async () => false);
}

/** Whether a node lives inside the terminal panel — used to tell which surface
 *  a copy was meant for when the editor also has a selection. */
export function containsNode(node) {
  return panel !== null && node instanceof Node && panel.contains(node);
}

/** The active terminal's selected text, or "" when nothing is selected. */
export function selectedText() {
  const term = terminals.get(activeId)?.term;
  return term?.hasSelection() ? term.getSelection() : "";
}

/** Copies the active terminal's selection. Resolves false when there was
 *  nothing selected or the clipboard refused. */
export async function copySelection() {
  const text = selectedText();
  if (!text) return false;
  return await copyText(text);
}

/** Send text to the active terminal as if it had been typed.
 *
 * Straight to the pty rather than through xterm's own input handling, which is
 * the same path `onData` uses — a paste is keystrokes as far as the shell is
 * concerned.
 *
 * Newlines are left alone. Trimming a trailing one would be second-guessing what
 * was copied, and a shell that receives a command without its newline simply
 * waits, which is recoverable; one that receives an extra newline has already run
 * something, which is not.
 */
export async function paste(text) {
  if (!text) return false;
  const record = terminals.get(activeId);
  if (!record || record.exited) return false;
  await invoke("terminal_write", { id: activeId, data: text });
  return true;
}

/** Clear the active terminal's screen and scrollback. */
export function clear() {
  terminals.get(activeId)?.term.clear();
  return true;
}

/** Select everything in the active terminal, so it can be copied. */
export function selectAll() {
  const term = terminals.get(activeId)?.term;
  if (!term) return false;
  term.selectAll();
  return true;
}

/** Whether the active terminal has a live shell behind it. */
export function isLive() {
  const record = terminals.get(activeId);
  return Boolean(record) && !record.exited;
}

export function focusActive() {
  terminals.get(activeId)?.term.focus();
}

export function isVisible() {
  return panel !== null && !panel.hidden;
}

export function setVisible(visible) {
  if (!panel) return;
  panel.hidden = !visible;
  onVisibilityChange(visible);
  if (visible) {
    if (!terminals.size) {
      openTerminal().catch((error) => {
        // Nothing to write into yet, so this is the one case that has to go to
        // the console rather than to a terminal.
        console.error("Could not open a terminal:", error);
      });
    } else {
      fitActive();
      terminals.get(activeId)?.term.focus();
    }
  }
}

export function toggleVisible() {
  setVisible(!isVisible());
}

function renderTabs() {
  tabsEl.textContent = "";
  for (const terminal of terminals.values()) {
    const tab = document.createElement("div");
    tab.className = `terminal-tab${terminal.id === activeId ? " active" : ""}${
      terminal.exited ? " exited" : ""
    }`;
    tab.draggable = true;

    const label = document.createElement("span");
    label.textContent = terminal.title;
    tab.append(label);

    const close = document.createElement("button");
    close.className = "close";
    close.type = "button";
    close.title = t("terminal.close");
    close.setAttribute("aria-label", t("terminal.close"));
    close.append(iconElement("close"));
    close.addEventListener("mousedown", (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      closeTerminal(terminal.id);
    });
    tab.append(close);

    tab.addEventListener("mousedown", (event) => {
      if (event.button === 1) {
        event.preventDefault();
        closeTerminal(terminal.id);
      } else if (event.button === 0) {
        activate(terminal.id);
      }
    });

    tab.addEventListener("dragstart", (event) => {
      draggedId = terminal.id;
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", String(terminal.id));
    });
    tab.addEventListener("dragend", () => {
      draggedId = null;
    });
    // Which half of the tab the pointer is over decides whether the dragged
    // tab lands before or after it, the same gesture the editor tabs use.
    tab.addEventListener("dragover", (event) => {
      if (draggedId == null || draggedId === terminal.id) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      const before = event.clientX - tab.getBoundingClientRect().left < tab.offsetWidth / 2;
      tab.classList.toggle("drop-before", before);
      tab.classList.toggle("drop-after", !before);
    });
    tab.addEventListener("dragleave", () => {
      tab.classList.remove("drop-before", "drop-after");
    });
    tab.addEventListener("drop", (event) => {
      if (draggedId == null) return;
      event.preventDefault();
      const before = event.clientX - tab.getBoundingClientRect().left < tab.offsetWidth / 2;
      const targetIndex = [...terminals.keys()].indexOf(terminal.id) + (before ? 0 : 1);
      const id = draggedId;
      draggedId = null;
      reorderTerminal(id, targetIndex);
    });

    tabsEl.append(tab);
  }
}

/** Moves a terminal's tab to `index` among the others, reordering in place. */
function reorderTerminal(id, index) {
  const keys = [...terminals.keys()];
  const currentIndex = keys.indexOf(id);
  if (currentIndex === -1 || currentIndex === index) return;
  keys.splice(currentIndex, 1);
  keys.splice(currentIndex < index ? index - 1 : index, 0, id);
  const reordered = keys.map((key) => [key, terminals.get(key)]);
  terminals.clear();
  for (const [key, value] of reordered) terminals.set(key, value);
  renderTabs();
}

function activate(id) {
  activeId = id;
  for (const terminal of terminals.values()) {
    terminal.element.hidden = terminal.id !== id;
  }
  renderTabs();
  fitActive();
  terminals.get(id)?.term.focus();
}

function fitActive() {
  const terminal = terminals.get(activeId);
  if (!terminal || terminal.element.hidden) return;
  try {
    terminal.fit.fit();
    if (!terminal.exited) {
      invoke("terminal_resize", {
        id: terminal.id,
        cols: terminal.term.cols,
        rows: terminal.term.rows,
      }).catch(() => {});
    }
  } catch {
    // The panel can be measured mid-layout; the next fit will catch up.
  }
}

/**
 * Routes shell output to the terminal it belongs to. Errors are swallowed on
 * purpose: without the backend a terminal is useless, but the panel should
 * still open and say so rather than silently never appearing.
 */
function ensureListeners() {
  if (!listenerSetup) {
    listenerSetup = (async () => {
      await listen("terminal-output", ({ payload }) => {
        const [id, text] = payload;
        const terminal = terminals.get(id);
        // Output can arrive for a terminal the user just closed.
        if (terminal && !terminal.disposed) terminal.term.write(text);
      });
      await listen("terminal-closed", ({ payload: id }) => {
        const terminal = terminals.get(id);
        if (!terminal || terminal.disposed) return;
        terminal.exited = true;
        terminal.term.write(`\r\n\x1b[2m${t("terminal.exited")}\x1b[0m\r\n`);
        renderTabs();
      });
    })().catch((error) => {
      // Allow a later terminal to try again rather than being born deaf.
      listenerSetup = null;
      throw error;
    });
  }
  return listenerSetup;
}

/**
 * Runs a script in its own terminal tab, named after the file.
 *
 * `kind` is the interpreter (`powershell`, `batch`, `shell`) rather than a
 * profile id — the backend starts that interpreter on the file directly, so
 * the path is never text a shell has to parse.
 */
export async function runInTerminal(path, kind, title) {
  return openTerminal(kind, { script: path, title });
}

/** Opens a new terminal running `profile` and shows it. */
export async function openTerminal(profile = PROFILES[0].id, options = {}) {
  const { Terminal, FitAddon } = await loadXterm();

  const id = nextId++;
  const element = document.createElement("div");
  element.className = "terminal-view";
  viewsEl.append(element);

  const term = new Terminal({
    fontFamily: 'Consolas, "Cascadia Mono", "Courier New", monospace',
    fontSize,
    cursorBlink: true,
    theme: themeColours(),
    scrollback: 5000,
    allowProposedApi: true,
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(element);

  // Ctrl+C copies **only when something is selected**. With nothing selected it
  // has to reach the shell as SIGINT, which is the whole reason this is a
  // condition rather than a binding: a terminal that cannot interrupt a runaway
  // process is worse than one that cannot copy.
  term.attachCustomKeyEventHandler((event) => {
    if (event.type !== "keydown") return true;
    const plainCtrl = event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
    if (plainCtrl && event.key === "c" && term.hasSelection()) {
      copySelection();
      return false;
    }
    return true;
  });

  const label =
    options.title || PROFILES.find((entry) => entry.id === profile)?.label || profile;
  const record = { id, title: label, term, fit, element, exited: false };
  terminals.set(id, record);

  if (panel.hidden) {
    panel.hidden = false;
    onVisibilityChange(true);
  }
  activate(id);
  fit.fit();

  term.onData((data) => {
    if (!record.exited) invoke("terminal_write", { id, data }).catch(() => {});
  });
  term.onResize(({ cols, rows }) => {
    if (!record.exited) invoke("terminal_resize", { id, cols, rows }).catch(() => {});
  });

  // The view is already on screen, so anything that goes wrong from here is
  // reported inside the terminal instead of leaving an empty panel. Listeners
  // are attached *before* the shell starts, or its banner is emitted into the
  // void — visible when a second terminal is opened during xterm's lazy load.
  try {
    await ensureListeners();
    await invoke("terminal_open", {
      id,
      profile,
      cwd: cwdProvider(),
      script: options.script ?? null,
      cols: term.cols || 80,
      rows: term.rows || 24,
    });
  } catch (error) {
    record.exited = true;
    term.write(`\x1b[31m${error?.message || error}\x1b[0m\r\n`);
    renderTabs();
  }
  return id;
}

/** Ends a terminal and removes its tab; hides the panel when none are left. */
export async function closeTerminal(id) {
  const terminal = terminals.get(id);
  if (!terminal) return;
  try {
    await invoke("terminal_close", { id });
  } catch {
    // Already gone — nothing to do.
  }
  const order = [...terminals.keys()];
  const position = order.indexOf(id);
  terminal.disposed = true;
  terminal.term.dispose();
  terminal.element.remove();
  terminals.delete(id);

  if (terminals.size === 0) {
    activeId = null;
    setVisible(false);
    renderTabs();
    return;
  }
  // Only move if the closed one was showing; otherwise closing a background tab
  // would yank focus out of the shell being typed in.
  if (id !== activeId) {
    renderTabs();
    return;
  }
  const remaining = [...terminals.keys()];
  activate(remaining[Math.min(position, remaining.length - 1)]);
}

/** Opens a shell in its own window, optionally elevated. */
export async function openExternalTerminal(profile, elevated) {
  return invoke("open_external_terminal", { profile, elevated, cwd: cwdProvider() });
}

/** Re-reads the theme colours — called when the app theme changes. */
export function refreshTheme() {
  const colours = themeColours();
  for (const terminal of terminals.values()) terminal.term.options.theme = colours;
}

/**
 * Matches the terminals to the editor's zoom level, so both halves of the
 * window read at one size and Ctrl+± moves them together. The relayout is not
 * optional: a font change alters the cell size, and without a re-fit the shell
 * keeps being told the old column count and wraps its output at the wrong width.
 */
export function setFontSize(size) {
  fontSize = size;
  for (const terminal of terminals.values()) terminal.term.options.fontSize = size;
  relayout();
}

let relayoutPending = 0;

/**
 * Re-measures the visible terminal after the window or a pane resizes.
 * Coalesced to one animation frame: `fit()` forces a synchronous layout and
 * sends an IPC resize, and resize events arrive in bursts.
 */
export function relayout() {
  if (relayoutPending) return;
  relayoutPending = requestAnimationFrame(() => {
    relayoutPending = 0;
    fitActive();
  });
}

/** Closes every terminal, so the shells do not outlive the window. */
export async function closeAllTerminals() {
  for (const id of [...terminals.keys()]) {
    try {
      await invoke("terminal_close", { id });
    } catch {
      // Ignore — we are shutting down.
    }
    // Dispose the frontend side too: if the close is then cancelled, the tabs
    // must not still look alive while their shells are gone.
    const terminal = terminals.get(id);
    if (terminal) {
      terminal.disposed = true;
      terminal.term.dispose();
      terminal.element.remove();
      terminals.delete(id);
    }
  }
  activeId = null;
  if (tabsEl) renderTabs();
  setVisible(false);
}
