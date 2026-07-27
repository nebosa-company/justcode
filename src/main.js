import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getVersion } from "@tauri-apps/api/app";
import { open as openDialog, save as saveDialog, ask, message } from "@tauri-apps/plugin-dialog";
import { readText as clipboardReadText, writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import { openLintPanel, closeLintPanel, nextDiagnostic, previousDiagnostic } from "@codemirror/lint";
import { loadDictionary, requestSpellcheck, refreshSpelling } from "./spellcheck.js";
import { deleteLine, undo, redo, moveLineUp, moveLineDown } from "@codemirror/commands";
import { setLinkHandler } from "./links.js";
import { toggleBookmark, toggleNextBookmark, gotoBookmark, BOOKMARK_SLOTS } from "./bookmarks.js";
import {
  showAbout,
  showHelpCenter,
  showLanguageDialog,
  showSymbolPicker,
  askSaveChanges,
  showAssociations,
  showNewFile,
  isOverlayOpen,
} from "./help.js";
import { findSymbols, supportsSymbols } from "./symbols.js";
import { templateFor, hasTemplate } from "./templates.js";
import { openSearchPanel, findNext, findPrevious } from "@codemirror/search";
import { t, setLocale, currentLocale, onLocaleChange, DEFAULT_LOCALE } from "./i18n.js";
import { EditorView } from "@codemirror/view";
import {
  createState,
  createView,
  applyLanguage,
  applyLanguageToState,
  themeEffect,
  fontSizeEffect,
  spellcheckEffect,
  wordWrapEffect,
  bionicReadingEffect,
  toggleCommentSmart,
  cursorPosition,
  countDiagnostics,
  DEFAULT_FONT_SIZE,
} from "./editor.js";
import { THEMES } from "./theme.js";
import {
  languageIdFor,
  LANGUAGE_LABELS,
  fileFilters,
  languageList,
  associationGroups,
  primaryExtension,
} from "./languages.js";
import { renderMarkdownDocument } from "./markdown.js";
import { createMenuBar, showContextMenu } from "./menu.js";
import {
  initTerminals,
  openTerminal,
  toggleVisible as toggleTerminals,
  isVisible as terminalsVisible,
  openExternalTerminal,
  refreshTheme as refreshTerminalTheme,
  setFontSize as setTerminalFontSize,
  setDock as setTerminalDock,
  dockEdge as terminalDock,
  relayout as relayoutTerminals,
  closeAllTerminals,
  PROFILES as TERMINAL_PROFILES,
  DEFAULT_PROFILE as DEFAULT_TERMINAL,
  runInTerminal,
} from "./terminal.js";
import { iconMarkup } from "./icons.js";

const FILE_FILTERS = fileFilters();

const MIN_FONT_SIZE = 8;
const MAX_FONT_SIZE = 40;
// What a brand-new file gets. Existing files keep whatever they came with.
const DEFAULT_EOL = navigator.userAgent.includes("Windows") ? "\r\n" : "\n";
// Mirrors the version in tauri.conf.json / package.json; used when the Tauri
// API is unavailable (running the frontend in a plain browser).
const APP_VERSION = "0.2.4";

// The file manager is named differently per platform, and the menu should say
// the name people actually know.
const REVEAL_KEY = /Windows|Win32/i.test(navigator.userAgent)
  ? "file.revealWindows"
  : /Mac|Darwin/i.test(navigator.userAgent)
    ? "file.revealMac"
    : "file.revealLinux";

const STORAGE = {
  fontSize: "justcode.fontSize",
  theme: "justcode.theme",
  toolbar: "justcode.toolbar",
  statusbar: "justcode.statusbar",
  spellcheck: "justcode.spellcheck",
  locale: "justcode.locale",
  recent: "justcode.recent",
  wordWrap: "justcode.wordWrap",
  bionicReading: "justcode.bionicReading",
  newFavourites: "justcode.newFavourites",
  session: "justcode.session",
  terminalDock: "justcode.terminalDock",
};

const MAX_RECENT_FILES = 15;

// A cap on the files carried across a restart. Every one of them is read from
// disk before the window is shown, so an enormous session would be paid for as
// a slow launch, every launch.
const MAX_SESSION_FILES = 50;

// Long enough that opening ten files writes once rather than ten times, short
// enough to survive the app being killed a moment later.
const SESSION_SAVE_DELAY = 400;

const dom = {
  app: document.getElementById("app"),
  menubar: document.getElementById("menubar"),
  workspace: document.getElementById("workspace"),
  panes: document.getElementById("panes"),
  zoomLevel: document.getElementById("zoom-level"),
  themeButton: document.getElementById("btn-theme"),
  statusPath: document.getElementById("status-path"),
  statusLang: document.getElementById("status-lang"),
  statusCursor: document.getElementById("status-cursor"),
  statusProblems: document.getElementById("status-problems"),
};

/** @type {Array<{id:number,path:string|null,name:string,state:import("@codemirror/state").EditorState|null,savedText:string,dirty:boolean}>} */
const tabs = [];

/**
 * Split view. Every pane owns an editor and the tabs shown in it; `tabs` above
 * stays the flat list of open documents. Panes sit in one row or one column —
 * the direction is chosen by the first split and flips back when it collapses
 * to a single pane, which covers dragging to any edge without a nested layout.
 *
 * @type {Array<{id:number,root:HTMLElement,tabsEl:HTMLElement,editorEl:HTMLElement,view:import("@codemirror/view").EditorView,tabIds:number[],activeTabId:number|null}>}
 */
const panes = [];
let layoutDirection = "row";
let activePaneId = null;
let nextPaneId = 1;

// `view` and `activeTabId` always mirror the focused pane, so everything that
// acts on "the editor" keeps working without knowing panes exist.
let view = null;
let activeTabId = null;
let nextTabId = 1;
let untitledCount = 0;
let fontSize = DEFAULT_FONT_SIZE;
let theme = "dark";
let showToolbar = true;
let showStatusbar = true;
// Off by default: in source code most identifiers are "misspelled".
let spellcheck = false;
// Off by default: wrapping makes long lines readable but breaks the one-row-
// per-line correspondence that the gutter and column numbers rely on.
let wordWrap = false;
// Off by default: it is a personal reading aid, not something a document
// should suddenly look like for every other file the user happens to open.
let bionicReading = false;

const listeners = {
  // Only the focused pane drives the shared status bar.
  isCurrent: (view_) => {
    const pane = activePane();
    return !pane || pane.view === view_;
  },
  onDocChanged: () => {
    const tab = activeTab();
    if (!tab) return;
    const dirty = view.state.doc.toString() !== tab.savedText;
    if (dirty !== tab.dirty) {
      tab.dirty = dirty;
      renderTabs();
      // The title carries the same unsaved marker as the tab.
      renderStatus();
    }
  },
  onSelection: (position) => {
    dom.statusCursor.textContent = `${position.line}:${position.column}`;
    dom.statusCursor.title = `Line ${position.line}, Column ${position.column}`;
  },
  onDiagnostics: ({ count, errors, spelling }) => {
    const parts = [count === 1 ? t("status.problem") : t("status.problems", { n: count })];
    if (spelling) parts.push(t("status.spelling", { n: spelling }));
    dom.statusProblems.innerHTML = iconMarkup("warning") + `<span>${parts.join(" · ")}</span>`;
    dom.statusProblems.classList.toggle("has-errors", errors > 0);
  },
};

// --------------------------------------------------------------- pane helpers

function activeTab() {
  return tabs.find((tab) => tab.id === activeTabId) || null;
}

/** Whether a real file is open, as opposed to the read-only placeholder. */
const hasTab = () => activeTab() !== null;

/**
 * A tab's text as it stands now — from its pane if it is on screen, from its
 * stashed state if it is not.
 */
function currentTextOf(tab) {
  const owner = paneOfTab(tab.id);
  if (owner && owner.activeTabId === tab.id) return owner.view.state.doc.toString();
  // A tab is only ever stateless mid-construction; treat it as its saved text
  // rather than throwing. This runs on the way out of the application, and an
  // exception here would abandon the close with nothing on screen to explain it.
  return tab.state ? tab.state.doc.toString() : tab.savedText;
}

/** Whether a tab has edits that are not on disk. */
function isModified(tab) {
  return currentTextOf(tab) !== tab.savedText;
}

function activePane() {
  return panes.find((pane) => pane.id === activePaneId) || panes[0] || null;
}

function tabById(id) {
  return tabs.find((tab) => tab.id === id) || null;
}

function paneOfTab(id) {
  return panes.find((pane) => pane.tabIds.includes(id)) || null;
}

/** True when this tab is the one on screen in its pane. */
function isDisplayed(tabId) {
  return panes.some((pane) => pane.activeTabId === tabId);
}

/** Copies the on-screen document of every pane back into its tab. */
function stashAllPanes() {
  for (const pane of panes) {
    const tab = tabById(pane.activeTabId);
    if (tab) tab.state = pane.view.state;
  }
}

/**
 * Applies compartment effects (theme, zoom, spell check) to every document —
 * the one visible in each pane, plus the states parked in background tabs.
 */
function applyEffectsEverywhere(effects) {
  for (const pane of panes) pane.view.dispatch({ effects });
  for (const tab of tabs) {
    if (!isDisplayed(tab.id)) tab.state = tab.state.update({ effects }).state;
  }
  // Dispatching into every pane makes each one report its diagnostics, and the
  // status bar is shared — so without this it ends up showing whichever pane
  // updated last rather than the focused one.
  renderStatus();
}

function createPane(index = panes.length) {
  const root = document.createElement("div");
  root.className = "pane";

  const tabsEl = document.createElement("div");
  tabsEl.className = "pane-tabs";

  const editorEl = document.createElement("div");
  editorEl.className = "pane-editor";

  root.append(tabsEl, editorEl);

  const pane = {
    id: nextPaneId++,
    root,
    tabsEl,
    editorEl,
    view: createView(editorEl),
    tabIds: [],
    activeTabId: null,
    // Explicit pixel size along the split axis, once the user has dragged its
    // grip; null means "share the remaining space equally with the others".
    size: null,
  };
  // Clicking anywhere in a pane makes it the target for menus and shortcuts.
  root.addEventListener("focusin", () => focusPane(pane.id));
  root.addEventListener("mousedown", () => focusPane(pane.id));
  attachPaneDropZones(pane);
  attachPaneGrip(pane);
  panes.splice(index, 0, pane);
  renderPanes();
  return pane;
}

function removePane(pane) {
  if (panes.length <= 1) return;
  const index = panes.indexOf(pane);
  pane.view.destroy();
  pane.root.remove();
  panes.splice(index, 1);
  if (panes.length === 1) setLayoutDirection("row"); // free to re-split any way
  if (activePaneId === pane.id) focusPane(panes[Math.max(0, index - 1)].id);
  renderPanes();
}

/** Changes the split axis, clearing sizes a drag on the other axis wouldn't apply to. */
function setLayoutDirection(direction) {
  layoutDirection = direction;
  for (const pane of panes) pane.size = null;
}

/** Applies each pane's explicit size (if dragged) or lets it share the rest equally. */
function applyPaneSizes() {
  for (const pane of panes) {
    pane.root.style.flex = pane.size != null ? `0 0 ${pane.size}px` : "1 1 0";
  }
}

/**
 * The grip sits on the leading edge of every pane but the first, letting the
 * user drag the boundary between it and its previous sibling — the same
 * mousedown/window-mousemove/window-mouseup shape as the terminal panel's.
 */
function attachPaneGrip(pane) {
  const grip = document.createElement("div");
  grip.className = "pane-grip";
  pane.root.prepend(grip);

  const minSize = 80;
  let dragging = false;
  let prevPane = null;
  let startPos = 0;
  let startPrevSize = 0;
  let startSize = 0;

  grip.addEventListener("mousedown", (event) => {
    const index = panes.indexOf(pane);
    prevPane = panes[index - 1];
    if (!prevPane) return;
    dragging = true;
    const column = layoutDirection === "column";
    startPos = column ? event.clientY : event.clientX;
    startPrevSize = column
      ? prevPane.root.getBoundingClientRect().height
      : prevPane.root.getBoundingClientRect().width;
    startSize = column ? pane.root.getBoundingClientRect().height : pane.root.getBoundingClientRect().width;
    event.preventDefault();
  });
  window.addEventListener("mousemove", (event) => {
    if (!dragging) return;
    // Releasing outside the webview delivers no mouseup, which used to leave
    // the pane resizing on every later mouse move with no button held.
    if (!(event.buttons & 1)) {
      dragging = false;
      return;
    }
    const column = layoutDirection === "column";
    const pos = column ? event.clientY : event.clientX;
    const total = startPrevSize + startSize;
    const newPrevSize = Math.min(Math.max(startPrevSize + (pos - startPos), minSize), total - minSize);
    prevPane.size = newPrevSize;
    pane.size = total - newPrevSize;
    applyPaneSizes();
    prevPane.view.requestMeasure();
    pane.view.requestMeasure();
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
  });
}

function renderPanes() {
  dom.panes.classList.toggle("column", layoutDirection === "column");
  // `append` on a node that is already a child moves it, which detaches the
  // editor and puts it back — losing the scroll position and invalidating
  // CodeMirror's cached measurements. Only touch the ones actually out of order.
  panes.forEach((pane, index) => {
    if (dom.panes.children[index] !== pane.root) dom.panes.append(pane.root);
    pane.root.classList.toggle("active", pane.id === activePaneId);
  });
  applyPaneSizes();
  for (const pane of panes) {
    renderTabsFor(pane);
    // The panes have just been resized by the flex layout; re-read heights so
    // gutters line up rather than keeping the measurements from the old width.
    pane.view.requestMeasure();
  }
}

/** Points `view`/`activeTabId` at a pane and refreshes the chrome. */
function focusPane(id) {
  const pane = panes.find((entry) => entry.id === id);
  if (!pane || activePaneId === id) return;
  stashAllPanes();
  activePaneId = id;
  view = pane.view;
  activeTabId = pane.activeTabId;
  for (const other of panes) other.root.classList.toggle("active", other.id === id);
  renderStatus();
}

function baseName(path) {
  return path.split(/[\\/]/).pop();
}

/**
 * The line ending a file uses. CodeMirror stores documents with "\n" only, so
 * the original convention is remembered here and restored on save — otherwise
 * opening and saving a Windows file would silently rewrite every line ending.
 */
function detectEol(text) {
  if (/\r\n/.test(text)) return "\r\n";
  // An LF-only file must stay LF. Falling back to the platform default here
  // rewrote every line of every Unix-style file the moment it was saved on
  // Windows — the exact thing this field exists to prevent.
  if (/\n/.test(text)) return "\n";
  return DEFAULT_EOL;
}

function openTab({ path, name, text }) {
  const tab = {
    id: nextTabId++,
    path: path ?? null,
    name,
    state: null,
    eol: detectEol(text),
    // Set from the document below, not from `text`: CodeMirror normalises CRLF
    // to LF when it builds the state, so comparing against the raw file text
    // reported every Windows file as modified the instant it was opened.
    savedText: "",
    dirty: false,
    // The effective language. Derived from the name until the user overrides it
    // from the status bar, after which `languageManual` stops a Save As from
    // silently switching it back.
    language: languageIdFor(name),
    languageManual: false,
  };
  tab.state = createState(text, tab.language, listeners, theme);
  tab.savedText = tab.state.doc.toString();
  tabs.push(tab);
  const pane = activePane() || createPane();
  pane.tabIds.push(tab.id);
  activateTab(tab.id);
  return tab;
}

/** Shows a tab, switching focus to whichever pane holds it. */
function activateTab(id) {
  const pane = paneOfTab(id);
  if (!pane) return;
  // Nothing to do — and re-rendering anyway would tear down the tab elements
  // mid-click, which is how the close button used to lose its own click event.
  if (activePaneId === pane.id && pane.activeTabId === id && activeTabId === id) {
    pane.view.focus();
    return;
  }
  if (activePaneId !== pane.id) {
    stashAllPanes();
    activePaneId = pane.id;
    view = pane.view;
    for (const other of panes) other.root.classList.toggle("active", other.id === pane.id);
  }
  if (pane.activeTabId !== id) {
    const current = tabById(pane.activeTabId);
    if (current) current.state = pane.view.state; // scroll, selection, undo history
    pane.activeTabId = id;
    const tab = tabById(id);
    if (tab) {
      pane.view.setState(tab.state);
      // Grammars load on demand; this resolves at once for an already-used one.
      applyLanguage(pane.view, tab.language, () => pane.activeTabId === tab.id);
      // Same reason as in setSpellcheck: a freshly shown document has not
      // changed, so the spell checker needs to be asked for a pass — when off
      // as well as on, since that pass is also what clears stale markers.
      if (spellcheck) loadDictionary().then(() => requestSpellcheck(pane.view));
      else requestSpellcheck(pane.view);
    }
  }
  activeTabId = id;
  renderTabs();
  renderStatus();
  pane.view.focus();
}

/** Activates the tab `delta` steps away within the focused pane, wrapping. */
function switchTab(delta) {
  const pane = activePane();
  if (!pane || pane.tabIds.length < 2) return;
  const index = pane.tabIds.indexOf(pane.activeTabId);
  const next = (index + delta + pane.tabIds.length) % pane.tabIds.length;
  activateTab(pane.tabIds[next]);
}

/** Moves the active tab to the front of its pane's strip. */
function moveTabToStart() {
  const pane = activePane();
  if (!pane) return;
  const index = pane.tabIds.indexOf(pane.activeTabId);
  if (index <= 0) return;
  const [id] = pane.tabIds.splice(index, 1);
  pane.tabIds.unshift(id);
  renderTabs();
}

/** Reorders the active tab within its pane. */
function moveTab(delta) {
  const pane = activePane();
  if (!pane) return;
  const index = pane.tabIds.indexOf(pane.activeTabId);
  const next = index + delta;
  if (index === -1 || next < 0 || next >= pane.tabIds.length) return;
  [pane.tabIds[index], pane.tabIds[next]] = [pane.tabIds[next], pane.tabIds[index]];
  renderTabs();
}

/**
 * Moves a tab into `target`. `edge` splits: dropping on a side creates a new
 * pane there, dropping in the middle (or on a tab bar) joins the existing one.
 * `atIndex`, when given, places it at that position in the destination's tabs
 * instead of at the end — how dropping directly on another tab reorders it.
 */
function moveTabToPane(tabId, target, edge = "center", atIndex = null) {
  const from = paneOfTab(tabId);
  if (!from) return;
  // A lone tab dragged out of its own pane would just swap places with itself,
  // unless a specific slot was requested — reordering within one pane goes
  // through this same "center" path.
  if (from === target && edge === "center") {
    if (atIndex == null) return;
    const currentIndex = from.tabIds.indexOf(tabId);
    if (currentIndex === -1 || currentIndex === atIndex) return;
    from.tabIds.splice(currentIndex, 1);
    from.tabIds.splice(currentIndex < atIndex ? atIndex - 1 : atIndex, 0, tabId);
    renderTabsFor(from);
    return;
  }

  const wantsColumn = edge === "top" || edge === "bottom";
  const after = edge === "right" || edge === "bottom";
  // The tab is the only one in its pane, so that pane goes away as soon as the
  // tab leaves — the move is really a request to re-place the existing split.
  const solo = from.tabIds.length === 1;

  if (edge !== "center" && solo && from === target) {
    // Dropping a pane's only tab on one of its own edges: nothing to create,
    // but with two panes it is an unambiguous request to flip the split from
    // side-by-side to stacked (or back), which previously did nothing at all.
    if (panes.length !== 2) return;
    setLayoutDirection(wantsColumn ? "column" : "row");
    const other = panes.find((pane) => pane !== target);
    panes.splice(panes.indexOf(target), 1);
    panes.splice(panes.indexOf(other) + (after ? 1 : 0), 0, target);
    renderPanes();
    return;
  }
  if (solo && from === target) return;

  let destination = target;
  if (edge !== "center") {
    // The direction may change whenever the result is a plain two-pane split —
    // `from` disappears if the tab was its last one, so count that in.
    if (panes.length + (solo ? 0 : 1) <= 2) setLayoutDirection(wantsColumn ? "column" : "row");
    const at = panes.indexOf(target) + (after ? 1 : 0);
    destination = createPane(at);
  }

  stashAllPanes();
  from.tabIds = from.tabIds.filter((id) => id !== tabId);
  if (from.activeTabId === tabId) {
    from.activeTabId = from.tabIds[from.tabIds.length - 1] ?? null;
    if (from.activeTabId != null) {
      from.view.setState(tabById(from.activeTabId).state);
    } else {
      from.view.setState(createState("", "text", listeners, theme, { readOnly: true }));
    }
  }
  destination.tabIds.splice(atIndex ?? destination.tabIds.length, 0, tabId);
  if (from.tabIds.length === 0) removePane(from);
  renderPanes();
  activateTab(tabId);
}

async function closeTab(id) {
  const tab = tabs.find((entry) => entry.id === id);
  if (!tab) return;
  const owner = paneOfTab(id);
  if (isModified(tab)) {
    const discard = await ask(t("dialog.unsaved", { name: tab.name }), {
      title: "JustCode",
      kind: "warning",
      okLabel: t("dialog.discard"),
      cancelLabel: t("dialog.cancel"),
    });
    if (!discard) return;
  }
  // The dialog above yields to the event loop; a forwarded open-files event can
  // reorder tabs meanwhile, so positions are recomputed here rather than reused.
  const index = tabs.indexOf(tab);
  if (index === -1) return;
  tabs.splice(index, 1);

  const pane = paneOfTab(id) || owner;
  if (pane) {
    const at = pane.tabIds.indexOf(id);
    pane.tabIds = pane.tabIds.filter((entry) => entry !== id);
    if (pane.activeTabId === id) {
      pane.activeTabId = null;
      const next = pane.tabIds[at] ?? pane.tabIds[at - 1] ?? null;
      if (next != null && pane.id !== activePaneId) {
        // A background pane shows its neighbour without taking the caret: you
        // may be typing in another pane and merely closing a tab over here.
        pane.activeTabId = next;
        const nextTab = tabById(next);
        if (nextTab?.state) pane.view.setState(nextTab.state);
        renderTabs();
      } else if (next != null) {
        activateTab(next);
      } else if (panes.length > 1) {
        removePane(pane); // a pane that has run out of tabs closes with them
      } else {
        activeTabId = null;
        pane.view.setState(createState("", "html", listeners, theme, { readOnly: true }));
      }
    }
  }
  renderTabs();
  renderStatus();
}

/**
 * Closes every tab matching `predicate`. Each close may raise its own
 * unsaved-changes prompt; cancelling one abandons the whole batch rather than
 * carrying on and closing the files behind it.
 */
async function closeMany(predicate) {
  for (const tab of [...tabs]) {
    if (!predicate(tab)) continue;
    const remaining = tabs.length;
    await closeTab(tab.id);
    if (tabs.length === remaining) return false;
  }
  return true;
}

const closeAllTabs = () => closeMany(() => true);
// The id is captured up front: closing a tab that another pane is displaying
// makes that pane activate a neighbour, which moves `activeTabId` mid-loop and
// would otherwise spare whichever tab it happened to land on.
const closeOtherTabs = () => {
  const keep = activeTabId;
  return closeMany((tab) => tab.id !== keep);
};

function renderTabs() {
  for (const pane of panes) renderTabsFor(pane);
  // Every change to what is open — opening, closing, reordering, splitting,
  // Save As — ends up here, which makes it the one place the session has to be
  // written from.
  queueSessionSave();
}

/** Draws one pane's tab bar, including the drag handles that drive splitting. */
function renderTabsFor(pane) {
  pane.tabsEl.textContent = "";
  pane.tabsEl.title = t("tabs.newFileHint");
  if (pane.tabIds.length === 0) {
    const empty = document.createElement("span");
    empty.className = "tabs-empty";
    empty.textContent = t("tabs.empty");
    pane.tabsEl.append(empty);
    return;
  }
  for (const id of pane.tabIds) {
    const tab = tabById(id);
    if (!tab) continue;
    const element = document.createElement("div");
    const isActive = pane.activeTabId === tab.id;
    element.className = `tab${isActive ? " active" : ""}${tab.dirty ? " dirty" : ""}`;
    element.title = tab.path || tab.name;
    element.draggable = true;
    element.addEventListener("mousedown", (event) => {
      if (event.button === 1) {
        event.preventDefault();
        closeTab(tab.id);
      }
    });
    // Left-click activates on `click`, not `mousedown`: activating re-renders
    // this bar, and replacing the element under the pointer before the drag
    // threshold is reached means `dragstart` never fires — so an inactive tab
    // could not be dragged at all. A drag suppresses `click`, which is exactly
    // the behaviour wanted here.
    element.addEventListener("click", (event) => {
      if (event.button === 0) activateTab(tab.id);
    });
    element.addEventListener("dragstart", (event) => {
      draggedTabId = tab.id;
      dom.app.classList.add("dragging-tab");
      event.dataTransfer.effectAllowed = "move";
      // Firefox refuses to start a drag without payload; the id above is what
      // the drop handlers actually read.
      event.dataTransfer.setData("text/plain", String(tab.id));
    });
    element.addEventListener("dragend", endTabDrag);
    // Dropping on a tab (rather than empty tab-bar space) reorders: which half
    // of it the pointer is over decides whether the dragged tab lands before
    // or after it, in this pane or a different one.
    element.addEventListener("dragover", (event) => {
      if (draggedTabId == null || draggedTabId === tab.id) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      const before = event.clientX - element.getBoundingClientRect().left < element.offsetWidth / 2;
      element.classList.toggle("drop-before", before);
      element.classList.toggle("drop-after", !before);
    });
    element.addEventListener("dragleave", () => {
      element.classList.remove("drop-before", "drop-after");
    });
    element.addEventListener("drop", (event) => {
      if (draggedTabId == null) return;
      event.preventDefault();
      const before = event.clientX - element.getBoundingClientRect().left < element.offsetWidth / 2;
      element.classList.remove("drop-before", "drop-after");
      const targetIndex = pane.tabIds.indexOf(tab.id) + (before ? 0 : 1);
      const id = draggedTabId;
      endTabDrag();
      moveTabToPane(id, pane, "center", targetIndex);
    });

    const label = document.createElement("span");
    label.textContent = tab.name;

    const dot = document.createElement("span");
    dot.className = "dot";
    dot.textContent = "•";

    const close = document.createElement("button");
    close.className = "close";
    close.innerHTML = iconMarkup("close");
    close.title = `${t("file.closeTab")} (Ctrl+W)`;
    close.setAttribute("aria-label", t("file.closeTab"));
    // Acts on mousedown, not click: closing re-renders this bar, so by mouseup
    // this button may no longer exist and the click would never fire.
    close.addEventListener("mousedown", (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      closeTab(tab.id);
    });

    element.append(label, dot, close);
    pane.tabsEl.append(element);
  }
}

/** Whether the Tauri backend is there — false when served to a plain browser. */
const underTauri = () => "__TAURI_INTERNALS__" in window;

/** Shows a short confirmation where the file path normally sits. */
let flashTimer = null;

function flashStatus(text) {
  // Restoring through renderStatus rather than a captured string: two flashes
  // overlapping used to leave the first flash's text sitting in the status bar.
  if (flashTimer !== null) clearTimeout(flashTimer);
  dom.statusPath.textContent = text;
  dom.statusPath.classList.add("copied");
  flashTimer = setTimeout(() => {
    flashTimer = null;
    dom.statusPath.classList.remove("copied");
    renderStatus();
  }, 1600);
}

/** Copies the active file's full path, confirming briefly in the status bar. */
async function copyPathToClipboard() {
  const tab = activeTab();
  if (!tab || !tab.path) return;
  if (!(await writeClipboard(tab.path))) return;
  flashStatus(t("status.pathCopied"));
}

function renderStatus() {
  const tab = activeTab();
  // The window title names the file being edited, with the usual bullet for
  // unsaved changes — it is what the taskbar and Alt+Tab show. `document.title`
  // alone only renames the webview document, not the native window, so the
  // window is told separately.
  const title = tab ? `${tab.dirty ? "• " : ""}${tab.name} — JustCode` : "JustCode";
  if (document.title !== title) {
    document.title = title;
    if (underTauri()) getCurrentWindow().setTitle(title).catch(() => {});
  }
  dom.statusPath.textContent = tab
    ? tab.path || `${tab.name} (${t("status.unsaved")})`
    : t("status.noFile");
  dom.statusPath.title = tab && tab.path ? `${tab.path}\n(click to copy)` : "";
  dom.statusPath.classList.toggle("clickable", Boolean(tab && tab.path));
  dom.statusLang.textContent = tab ? LANGUAGE_LABELS[tab.language] : "";
  dom.statusLang.disabled = !tab;
  if (!view) return;
  listeners.onSelection(cursorPosition(view.state));
  listeners.onDiagnostics(countDiagnostics(view.state));
}

// ------------------------------------------------------- split-view drag & drop

let draggedTabId = null;

function endTabDrag() {
  draggedTabId = null;
  dom.app.classList.remove("dragging-tab");
  for (const pane of panes) pane.editorEl.dataset.dropEdge = "";
}

/**
 * Which part of a pane the pointer is over. The outer quarter of each side
 * splits toward that side; anywhere else joins the pane as another tab.
 */
function dropEdgeAt(pane, event) {
  const rect = pane.editorEl.getBoundingClientRect();
  const x = (event.clientX - rect.left) / rect.width;
  const y = (event.clientY - rect.top) / rect.height;
  const edge = Math.min(x, 1 - x, y, 1 - y);
  if (edge > 0.25) return "center";
  if (edge === x) return "left";
  if (edge === 1 - x) return "right";
  return edge === y ? "top" : "bottom";
}

function attachPaneDropZones(pane) {
  const over = (event) => {
    if (draggedTabId == null) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    pane.editorEl.dataset.dropEdge = dropEdgeAt(pane, event);
  };
  pane.editorEl.addEventListener("dragover", over);
  pane.editorEl.addEventListener("dragleave", () => {
    pane.editorEl.dataset.dropEdge = "";
  });
  pane.editorEl.addEventListener("drop", (event) => {
    if (draggedTabId == null) return;
    event.preventDefault();
    const edge = dropEdgeAt(pane, event);
    const id = draggedTabId;
    endTabDrag();
    moveTabToPane(id, pane, edge);
  });

  // Dropping on a tab bar always joins that pane — this is how a split is
  // undone: drag the last tab back onto another pane's tabs.
  pane.tabsEl.addEventListener("dragover", (event) => {
    if (draggedTabId == null) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    pane.tabsEl.classList.add("drop-target");
  });
  pane.tabsEl.addEventListener("dragleave", () => pane.tabsEl.classList.remove("drop-target"));
  pane.tabsEl.addEventListener("drop", (event) => {
    if (draggedTabId == null) return;
    event.preventDefault();
    pane.tabsEl.classList.remove("drop-target");
    const id = draggedTabId;
    endTabDrag();
    moveTabToPane(id, pane, "center");
  });

  pane.tabsEl.addEventListener("dblclick", (event) => {
    if (event.target !== pane.tabsEl && !event.target.classList.contains("tabs-empty")) return;
    focusPane(pane.id);
    setTimeout(newFile, 0);
  });

  // A plain wheel over the strip scrolls it horizontally, so tabs that have
  // scrolled out of view are reachable without the thin scrollbar.
  pane.tabsEl.addEventListener(
    "wheel",
    (event) => {
      if (event.ctrlKey || event.deltaY === 0) return;
      if (pane.tabsEl.scrollWidth <= pane.tabsEl.clientWidth) return;
      event.preventDefault();
      pane.tabsEl.scrollLeft += event.deltaY;
    },
    { passive: false },
  );
}

// --------------------------------------------------------------- language mode

/** Applies a language chosen from the status bar to the active tab. */
function setTabLanguage(languageId) {
  const tab = activeTab();
  if (!tab || tab.language === languageId) return;
  tab.language = languageId;
  tab.languageManual = true;
  applyLanguage(view, languageId, () => activeTab() === tab);
  renderStatus();
  view.focus();
}

let langPicker = null;

function closeLanguagePicker() {
  if (!langPicker) return;
  langPicker.remove();
  langPicker = null;
  document.removeEventListener("mousedown", onLanguagePickerOutside, true);
}

function onLanguagePickerOutside(event) {
  if (langPicker && !langPicker.contains(event.target) && event.target !== dom.statusLang) {
    closeLanguagePicker();
  }
}

/**
 * A VS Code-style "Select Language Mode" popup: a filter box over a scrollable
 * list, opening upward from the status bar. Arrow keys move the highlight,
 * Enter commits it, Escape (or an outside click) dismisses.
 */
function showLanguagePicker() {
  if (langPicker) {
    closeLanguagePicker();
    return;
  }
  const tab = activeTab();
  if (!tab) return;
  const items = languageList();

  const picker = document.createElement("div");
  picker.className = "lang-picker";

  const filter = document.createElement("input");
  filter.className = "lang-picker-filter";
  filter.type = "text";
  filter.placeholder = "Select Language Mode";
  filter.spellcheck = false;

  const list = document.createElement("div");
  list.className = "lang-picker-list";

  picker.append(filter, list);
  document.body.append(picker);
  langPicker = picker;

  let highlighted = 0;

  const render = () => {
    const query = filter.value.trim().toLowerCase();
    const matches = items.filter((item) => item.label.toLowerCase().includes(query));
    if (highlighted >= matches.length) highlighted = Math.max(0, matches.length - 1);
    list.textContent = "";
    matches.forEach((item, index) => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "lang-picker-item";
      if (index === highlighted) button.classList.add("highlighted");
      if (item.id === tab.language) button.classList.add("current");
      button.textContent = item.label;
      button.addEventListener("mouseenter", () => {
        highlighted = index;
        for (const el of list.children) el.classList.remove("highlighted");
        button.classList.add("highlighted");
      });
      button.addEventListener("click", () => {
        setTabLanguage(item.id);
        closeLanguagePicker();
      });
      list.append(button);
    });
    return matches;
  };

  filter.addEventListener("input", () => {
    highlighted = 0;
    render();
  });
  filter.addEventListener("keydown", (event) => {
    const matches = items.filter((item) =>
      item.label.toLowerCase().includes(filter.value.trim().toLowerCase()),
    );
    if (event.key === "ArrowDown") {
      event.preventDefault();
      highlighted = Math.min(highlighted + 1, matches.length - 1);
      render();
      list.children[highlighted]?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      highlighted = Math.max(highlighted - 1, 0);
      render();
      list.children[highlighted]?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (matches[highlighted]) {
        setTabLanguage(matches[highlighted].id);
        closeLanguagePicker();
      }
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeLanguagePicker();
      view.focus();
    }
  });

  render();

  // Anchor above the language label, staying inside the viewport.
  const rect = dom.statusLang.getBoundingClientRect();
  picker.style.left = `${Math.max(6, Math.min(rect.left, window.innerWidth - picker.offsetWidth - 6))}px`;
  picker.style.bottom = `${window.innerHeight - rect.top + 4}px`;

  document.addEventListener("mousedown", onLanguagePickerOutside, true);
  filter.focus();
}

// ------------------------------------------------------------------- commands

/** Creates an untitled file of `languageId`, or a blank plain-text one when null. */
function createFile(languageId) {
  untitledCount++;
  const extension = primaryExtension(languageId || "text");
  const name = `untitled-${untitledCount}.${extension}`;
  openTab({ path: null, name, text: languageId ? templateFor(languageId, name) : "" });
  renderStatus();
}

function readFavouriteTypes() {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE.newFavourites) || "[]");
    return new Set(Array.isArray(stored) ? stored.filter((id) => typeof id === "string") : []);
  } catch {
    return new Set();
  }
}

/**
 * File ▸ New. Offers a blank document or a starting point for any type that has
 * one; the blank row is highlighted first, so Enter still gives the old
 * behaviour without reading the list.
 */
function newFile() {
  const favourites = readFavouriteTypes();
  const types = languageList()
    .filter((entry) => hasTemplate(entry.id))
    .map((entry) => ({ ...entry, extension: primaryExtension(entry.id) }));

  showNewFile(
    types,
    favourites,
    (languageId) => createFile(languageId),
    () => localStorage.setItem(STORAGE.newFavourites, JSON.stringify([...favourites])),
  );
}

// ------------------------------------------------------------- recent files

function readRecentFiles() {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE.recent) || "[]");
    return Array.isArray(stored) ? stored.filter((entry) => typeof entry === "string") : [];
  } catch {
    return [];
  }
}

/** Records a path as most-recent, keeping the list unique and capped. */
function rememberRecentFile(path) {
  if (!path) return;
  const next = [path, ...readRecentFiles().filter((entry) => entry !== path)];
  localStorage.setItem(STORAGE.recent, JSON.stringify(next.slice(0, MAX_RECENT_FILES)));
}

// ----------------------------------------------------------------- session
//
// The files that were open last time, reopened at the next launch. Only saved
// documents are listed: an untitled buffer has nothing on disk to reopen, and
// the unsaved-changes prompt on the way out has already settled what happens
// to it. Panes are recorded too, so a split layout comes back as a split.

// Saves are suppressed until startup has finished deciding what is open —
// otherwise the empty editor that exists for the first moments of a launch
// would overwrite the very session being restored.
let sessionReady = false;
let sessionSaveTimer = null;

function readSession() {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE.session) || "null");
    return stored && Array.isArray(stored.panes) ? stored : null;
  } catch {
    return null;
  }
}

/** A tab's caret as the 1-based line/column `revealLine` takes. */
function caretOf(tab) {
  const owner = paneOfTab(tab.id);
  // On screen the live view is authoritative; the stashed state is only current
  // for tabs sitting in the background.
  const state = owner && owner.activeTabId === tab.id ? owner.view.state : tab.state;
  if (!state) return {};
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);
  return { line: line.number, column: head - line.from + 1 };
}

/**
 * Writes the open files to storage. Called on every tab change rather than only
 * on the way out, so a session that ends in a crash or a kill is still there at
 * the next launch.
 */
function rememberSession() {
  if (!sessionReady) return;
  let budget = MAX_SESSION_FILES;
  const entries = [];
  for (const pane of panes) {
    const files = [];
    for (const id of pane.tabIds) {
      const tab = tabById(id);
      if (!tab?.path) continue;
      if (budget-- <= 0) break;
      files.push({ path: tab.path, ...caretOf(tab) });
    }
    // A pane holding nothing but untitled buffers has nothing to reopen, and
    // restoring it as an empty split would be worse than not restoring it.
    if (files.length) {
      entries.push({ paneId: pane.id, active: tabById(pane.activeTabId)?.path || null, files });
    }
  }
  localStorage.setItem(
    STORAGE.session,
    JSON.stringify({
      version: 1,
      layout: layoutDirection,
      // An index rather than an id: pane ids are handed out fresh every launch.
      activePane: Math.max(
        entries.findIndex((entry) => entry.paneId === activePaneId),
        0,
      ),
      panes: entries.map(({ active, files }) => ({ active, files })),
    }),
  );
}

/** Coalesces the burst of tab changes that one command can produce. */
function queueSessionSave() {
  clearTimeout(sessionSaveTimer);
  sessionSaveTimer = setTimeout(rememberSession, SESSION_SAVE_DELAY);
}

/** Writes the session immediately — for the way out, where a timer would not fire. */
function flushSessionSave() {
  clearTimeout(sessionSaveTimer);
  rememberSession();
}

/**
 * Reopens the last session, rebuilding the panes it was spread across and the
 * caret each file was left at. Returns whether anything was actually opened, so
 * a first run — or a session whose files have all since been deleted — still
 * falls back to the usual blank document.
 *
 * Files that no longer exist are skipped without a word: a launch is the wrong
 * moment to be told about a file deleted days ago.
 */
async function restoreSession() {
  const session = readSession();
  if (!session?.panes?.length) return false;
  if (session.layout === "column") setLayoutDirection("column");

  let opened = false;
  for (const [index, entry] of session.panes.entries()) {
    if (!Array.isArray(entry?.files)) continue;
    // The first pane is the one the app always starts with; the rest are the
    // split the user left behind. `focusPane` is what makes new tabs land here,
    // since `openTab` appends to whichever pane is focused.
    const pane = index === 0 ? panes[0] : createPane();
    focusPane(pane.id);
    for (const file of entry.files) {
      if (!file?.path) continue;
      if (await openPath(file.path, { quiet: true, line: file.line, column: file.column })) {
        opened = true;
      }
    }
    // Each file activates as it opens, so the pane ends up showing the last one
    // rather than the one that was on screen.
    const wanted = entry.active && findTabByPath(entry.active);
    if (wanted && paneOfTab(wanted.id) === pane) activateTab(wanted.id);
  }

  // Panes whose files have all gone missing would otherwise linger as empty
  // splits with no way to tell what they were for.
  for (const pane of [...panes]) {
    if (!pane.tabIds.length) removePane(pane);
  }

  const target = panes[Math.min(session.activePane ?? 0, panes.length - 1)];
  if (target) {
    focusPane(target.id);
    if (target.activeTabId != null) activateTab(target.activeTabId);
  }
  return opened;
}

/**
 * A comparable form of a path. Windows treats paths case-insensitively and
 * accepts either separator, so the raw string is not a safe identity.
 */
function samePathKey(path) {
  return path.replace(/\//g, "\\").toLowerCase();
}

/** The open tab for a path, if there is one. */
function findTabByPath(path) {
  const key = samePathKey(path);
  return tabs.find((tab) => tab.path && samePathKey(tab.path) === key) || null;
}

/** Paths with a read in flight, so two opens cannot both create a tab. */
const opening = new Set();

/**
 * Moves the caret to `line` (1-based, `column` optional and also 1-based) and
 * centres it — the `path:line[:column]` half of what a command-line argument
 * or an "Open with" launch can ask for.
 */
function revealLine(view, line, column) {
  if (!view || !line) return;
  const doc = view.state.doc;
  const clamped = Math.min(Math.max(line, 1), doc.lines);
  const lineInfo = doc.line(clamped);
  const col = column ? Math.min(Math.max(column - 1, 0), lineInfo.length) : 0;
  const pos = lineInfo.from + col;
  view.dispatch({
    selection: { anchor: pos },
    effects: EditorView.scrollIntoView(pos, { y: "center" }),
  });
  view.focus();
}

/** Opens one path, focusing it instead if it is already open. */
async function openPath(path, { quiet = false, line = null, column = null } = {}) {
  // Windows paths differ only by case and separator; compare them normalised so
  // C:/a.txt and c:\a.txt do not open as two tabs on the same file.
  const key = samePathKey(path);
  const existing = findTabByPath(path);
  if (existing) {
    activateTab(existing.id);
    // Re-opening should still bump the file up the recent list.
    rememberRecentFile(existing.path);
    revealLine(paneOfTab(existing.id)?.view, line, column);
    return true;
  }
  // Two overlapping opens (a forwarded open-files event arriving while
  // startup_files is still resolving) would both pass the check above.
  if (opening.has(key)) return true;
  opening.add(key);
  try {
    const text = await invoke("read_text_file", { path });
    const tab = openTab({ path, name: baseName(path), text });
    rememberRecentFile(path);
    revealLine(paneOfTab(tab.id)?.view, line, column);
    return true;
  } catch (error) {
    if (!quiet) {
      await message(`Could not open ${path}\n\n${error}`, { title: "JustCode", kind: "error" });
    }
    return false;
  } finally {
    opening.delete(key);
  }
}

async function openFile() {
  const selection = await openDialog({ multiple: true, filters: FILE_FILTERS });
  if (!selection) return;
  for (const path of Array.isArray(selection) ? selection : [selection]) {
    await openPath(path);
  }
  renderStatus();
}

/**
 * Opens files handed over by the OS — either on the command line at launch or
 * forwarded by the single-instance plugin when Explorer opens another document
 * while the app is already running. Each target is `{ path, line, column }`;
 * `line`/`column` come from a `path:line[:column]` argument and are `null`
 * otherwise.
 */
async function openExternalFiles(targets) {
  let opened = false;
  for (const { path, line, column } of targets) {
    if (await openPath(path, { quiet: true, line, column })) opened = true;
  }
  if (opened) {
    // A file arriving from Explorer replaces the placeholder blank document.
    const blank = tabs.find((tab) => !tab.path && !tab.dirty && tab.savedText === "");
    if (blank && tabs.length > 1) await closeTab(blank.id);
    renderStatus();
  }
}

/** Writes a tab to disk, asking for a location when it has none. */
async function saveTab(tab, { forcePrompt = false } = {}) {
  if (!tab) return false;
  const text = tab.id === activeTabId ? view.state.doc.toString() : tab.state.doc.toString();
  let path = tab.path;

  if (!path || forcePrompt) {
    path = await saveDialog({ defaultPath: path || tab.name, filters: FILE_FILTERS });
    if (!path) return false;
  }

  try {
    // Written back with the line ending the file arrived with, so saving does
    // not turn a CRLF file into an LF one (and show up as a whole-file diff).
    const contents = tab.eol === "\n" ? text : text.replaceAll("\n", tab.eol);
    await invoke("write_text_file", { path, contents });
  } catch (error) {
    await message(`Could not save ${path}\n\n${error}`, { title: "JustCode", kind: "error" });
    return false;
  }

  rememberRecentFile(path);
  const renamed = baseName(path) !== tab.name;
  tab.path = path;
  tab.name = baseName(path);
  tab.savedText = text;
  tab.dirty = false;
  // A rename re-derives the language from the new extension, unless the user
  // pinned it by hand from the status bar.
  if (renamed && !tab.languageManual) {
    const next = languageIdFor(tab.name);
    if (next !== tab.language) {
      tab.language = next;
      // The tab being saved is not necessarily the focused one — Save All walks
      // every pane. Using the global `view` here applied the new grammar to
      // whatever happened to be focused, or (when the guard failed) to nothing
      // at all, leaving a renamed file highlighted as its old type.
      const owner = paneOfTab(tab.id);
      if (owner && owner.activeTabId === tab.id) {
        applyLanguage(owner.view, next, () => paneOfTab(tab.id)?.activeTabId === tab.id);
      } else if (tab.state) {
        applyLanguageToState(tab.state, next).then((updated) => {
          // The tab may have been closed or renamed again while the grammar
          // loaded, so only adopt the result if it is still wanted.
          if (tab.language === next && tabs.includes(tab)) tab.state = updated;
        });
      }
    }
  }
  renderTabs();
  renderStatus();
  return true;
}

/**
 * Languages a terminal can execute, mapped to the interpreter that runs them.
 * Anything not listed here is a document, not a program.
 */
const SCRIPT_KINDS = { powershell: "powershell", batch: "batch", shell: "shell" };

/** Whether the active file is something a shell could run. */
function runnableTab() {
  const tab = activeTab();
  return tab && SCRIPT_KINDS[tab.language] ? tab : null;
}

/**
 * Runs the current script inside the integrated terminal, rather than in the
 * separate console window `run()` opens. Output stays in the app and the shell
 * is left at a prompt afterwards, so a failing script can be poked at on the
 * spot instead of vanishing with its window.
 */
async function runInTerminalCommand() {
  const current = runnableTab();
  if (!current) return;
  // The interpreter reads the file, so unsaved edits would not be run.
  if (current.dirty || !current.path) {
    if (!(await saveTab(current))) return;
  }
  try {
    await runInTerminal(current.path, SCRIPT_KINDS[current.language], current.name);
  } catch (error) {
    flashStatus(String(error?.message || error));
  }
}

/**
 * Saves everything that needs saving, then hands the HTML file to the OS so it
 * opens in the default browser. Relative <link>/<script> paths resolve because
 * the file is opened from its own directory.
 */
async function run() {
  stashAllPanes();
  const current = activeTab();

  // A Markdown file is previewed rather than opened directly: it is converted
  // to a styled HTML document in the temp folder, which is what reaches the
  // browser. The .md file itself is still saved first.
  if (current && current.language === "markdown") {
    if (current.dirty || !current.path) {
      if (!(await saveTab(current))) return;
    }
    const html = renderMarkdownDocument(view.state.doc.toString(), current.name);
    try {
      const path = await invoke("write_preview", { name: current.name, html });
      await invoke("open_in_browser", { path });
    } catch (error) {
      await message(`Could not preview ${current.name}\n\n${error}`, {
        title: "JustCode",
        kind: "error",
      });
    }
    return;
  }

  // Scripts are executed by their interpreter in a console window rather than
  // shown in a browser. They must be on disk first — an interpreter reads the
  // file, not the buffer.
  if (current && SCRIPT_KINDS[current.language]) {
    if (current.dirty || !current.path) {
      if (!(await saveTab(current))) return;
    }
    try {
      await invoke("run_script", { path: current.path, kind: SCRIPT_KINDS[current.language] });
    } catch (error) {
      await message(`Could not run ${current.name}\n\n${error}`, {
        title: "JustCode",
        kind: "error",
      });
    }
    return;
  }

  const target =
    current && current.language === "html"
      ? current
      : [...tabs].reverse().find((tab) => tab.language === "html");

  if (!target) {
    await message("Open or create an .html or .md file first — Run needs a page to show.", {
      title: "JustCode",
      kind: "warning",
    });
    return;
  }

  // Flush every file that already lives on disk — an unsaved companion .css or
  // .js would otherwise load a stale version — then the target itself, which
  // may still need a location.
  for (const tab of tabs) {
    if (tab === target || !tab.path || !tab.dirty) continue;
    if (!(await saveTab(tab))) return;
  }
  if (!(await saveTab(target))) return;

  try {
    await invoke("open_in_browser", { path: target.path });
  } catch (error) {
    await message(`Could not open the browser\n\n${error}`, { title: "JustCode", kind: "error" });
  }
}

/** Whether the lint panel is showing under the focused editor. */
function problemsOpen() {
  return Boolean(view?.dom.querySelector(".cm-panel-lint"));
}

/**
 * F8 both opens and closes the panel. It takes a fixed slice off the bottom of
 * the editor, so the key that summons it is also the obvious way to get the
 * space back — the alternative is reaching for the panel's own × with the mouse.
 */
function showProblems() {
  if (problemsOpen()) closeLintPanel(view);
  else openLintPanel(view);
  view.focus();
}

/**
 * Walks the caret through the problems, wrapping at the ends. Independent of
 * the panel: stepping between two warnings does not need a list of them open,
 * though the panel does follow along when it is.
 */
function goToProblem(direction) {
  if (!view) return;
  if (direction < 0) previousDiagnostic(view);
  else nextDiagnostic(view);
  view.focus();
}

/** Lists the declarations in the active file and jumps to the chosen one. */
function goToSymbol() {
  const tab = activeTab();
  if (!tab || !view) return;
  const symbols = findSymbols(view.state.doc.toString(), tab.language);
  if (!symbols.length) {
    dom.statusPath.textContent = t("modal.noSymbols");
    setTimeout(renderStatus, 1600);
    return;
  }
  showSymbolPicker(symbols, (symbol) => {
    const line = view.state.doc.lineAt(Math.min(symbol.pos, view.state.doc.length));
    view.dispatch({
      selection: { anchor: line.from },
      // Put the definition near the top rather than just barely on screen.
      effects: EditorView.scrollIntoView(line.from, { y: "start", yMargin: 40 }),
    });
    view.focus();
  });
}

const openFind = () => {
  openSearchPanel(view);
};
const openReplace = () => {
  // The panel's own Ctrl+H path expands the replace row; go through the keymap
  // so both routes behave identically.
  view.focus();
  view.contentDOM.dispatchEvent(
    new KeyboardEvent("keydown", { key: "h", ctrlKey: true, bubbles: true, cancelable: true }),
  );
};

// ------------------------------------------------------------------ edit menu

/** The cursor's whole line plus its trailing newline — the target when nothing
 * is selected, matching how editors cut/copy a line. */
function cursorLineRange() {
  const head = view.state.selection.main.head;
  const line = view.state.doc.lineAt(head);
  const to = Math.min(line.to + 1, view.state.doc.length);
  return { from: line.from, to };
}

// Clipboard goes through the Tauri plugin (backed by the OS clipboard in Rust),
// which — unlike the browser's navigator.clipboard — is not gated by web
// permission policy. The navigator fallback only matters when running the
// frontend in a plain browser during development.
async function writeClipboard(text) {
  try {
    await clipboardWriteText(text);
    return true;
  } catch {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }
}

async function readClipboard() {
  try {
    return await clipboardReadText();
  } catch {
    return await navigator.clipboard.readText(); // may throw; caller handles it
  }
}

async function editCopy() {
  const selection = view.state.selection.main;
  const range = selection.empty ? cursorLineRange() : selection;
  if (!(await writeClipboard(view.state.sliceDoc(range.from, range.to)))) {
    await message("Copy needs clipboard access, which the system declined.", {
      title: "JustCode",
      kind: "warning",
    });
  }
  view.focus();
}

async function editCut() {
  const selection = view.state.selection.main;
  const range = selection.empty ? cursorLineRange() : selection;
  if (!(await writeClipboard(view.state.sliceDoc(range.from, range.to)))) {
    view.focus();
    return;
  }
  view.dispatch({ changes: { from: range.from, to: range.to, insert: "" }, selection: { anchor: range.from } });
  view.focus();
}

async function editPaste() {
  let text;
  try {
    text = await readClipboard();
  } catch {
    await message("Paste needs clipboard access, which the system declined.", {
      title: "JustCode",
      kind: "warning",
    });
    return;
  }
  if (text == null) return;
  const selection = view.state.selection.main;
  view.dispatch({
    changes: { from: selection.from, to: selection.to, insert: text },
    selection: { anchor: selection.from + text.length },
    scrollIntoView: true,
  });
  view.focus();
}

function editSelectAll() {
  view.dispatch({ selection: { anchor: 0, head: view.state.doc.length } });
  view.focus();
}

function toggleComment() {
  toggleCommentSmart(view);
  view.focus();
}

function deleteCurrentLine() {
  deleteLine(view);
  view.focus();
}

/**
 * Sorts the selected lines, comparing character by character so that case
 * counts: "Zebra" comes before "apple", the way `sort` does on the command
 * line, rather than being folded together as a dictionary would.
 *
 * The selection is widened to whole lines, so a partial selection still sorts
 * the lines it touches. With nothing selected the whole document is sorted.
 */
function sortSelectedLines() {
  const { state } = view;

  // A bare cursor means "sort everything"; otherwise each selected range is
  // sorted on its own, which is what multiple cursors imply.
  let targets;
  if (state.selection.ranges.length === 1 && state.selection.main.empty) {
    // Stop at the end of the last non-empty line. Taking `doc.length` on a file
    // that ends with a newline puts an empty string in the split, which sorts
    // to the top as a blank first line and costs the trailing newline.
    const last = state.doc.line(state.doc.lines);
    const to = last.length === 0 && state.doc.lines > 1
      ? state.doc.line(state.doc.lines - 1).to
      : last.to;
    targets = [{ from: 0, to }];
  } else {
    targets = [];
    for (const range of state.selection.ranges) {
      const from = state.doc.lineAt(range.from).from;
      // A selection dragged to the start of the next line stops there visually,
      // so that line is not part of it — without this, selecting three lines
      // downwards would quietly sort a fourth.
      const endsAtLineStart = range.to > range.from && state.doc.lineAt(range.to).from === range.to;
      const to = state.doc.lineAt(endsAtLineStart ? range.to - 1 : range.to).to;
      const previous = targets[targets.length - 1];
      // Two cursors on the same or neighbouring lines widen to overlapping
      // spans, and CodeMirror rejects overlapping changes — so merge them.
      if (previous && from <= previous.to) previous.to = Math.max(previous.to, to);
      else targets.push({ from, to });
    }
  }

  const changes = [];
  for (const { from, to } of targets) {
    const text = state.doc.sliceString(from, to);
    const lines = text.split("\n");
    if (lines.length < 2) continue;
    const insert = [...lines].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)).join("\n");
    if (insert !== text) changes.push({ from, to, insert });
  }

  if (changes.length) view.dispatch({ changes });
  view.focus();
}

function editUndo() {
  undo(view);
  view.focus();
}

function editRedo() {
  redo(view);
  view.focus();
}

// Alt+↑/↓ come from CodeMirror's own keymap; these exist so the commands are
// also reachable from the Edit menu, which is where someone who does not know
// the shortcut will look for them.
function editMoveLineUp() {
  moveLineUp(view);
  view.focus();
}

function editMoveLineDown() {
  moveLineDown(view);
  view.focus();
}

/** Saves every file that has unsaved changes. Returns false if one was cancelled. */
async function saveAllTabs() {
  stashAllPanes();
  for (const tab of [...tabs]) {
    if (!isModified(tab)) continue;
    if (!(await saveTab(tab))) return false;
  }
  return true;
}

/**
 * Splits the view by moving the active tab into a new pane on `edge`, the same
 * operation dragging a tab to that edge performs — without the drag.
 */
function splitActive(edge) {
  if (!canSplit()) return;
  stashAllPanes();
  moveTabToPane(activeTabId, activePane(), edge);
}

/**
 * Whether splitting would do anything. A tab lives in exactly one pane, so a
 * pane holding a single tab has nothing to give away — the only meaningful
 * move there is re-orienting an existing two-pane split.
 */
function canSplit() {
  const pane = activePane();
  if (!pane || activeTabId === null) return false;
  return pane.tabIds.length > 1 || panes.length === 2;
}

/**
 * Applies `transform` to each selected range; where a range is empty, the word
 * under the caret is used instead so the command is useful without a selection.
 */
function transformSelection(transform) {
  const state = view.state;
  // Spans are collected first and then de-overlapped: an empty range widens to
  // the whole word under the caret, which can straddle another selected range,
  // and CodeMirror silently *merges* overlapping changes rather than throwing —
  // turning `getUserName` into `GETUSERNAMEUSER`.
  const seen = new Set();
  const changes = [];
  for (const range of state.selection.ranges) {
    let { from, to } = range;
    if (from === to) {
      const word = state.wordAt(from);
      if (!word) continue;
      from = word.from;
      to = word.to;
    }
    // Two carets inside the same word expand to the same span; applying both
    // would compose into duplicated text, so each span is transformed once.
    const key = `${from}:${to}`;
    if (from === to || seen.has(key)) continue;
    seen.add(key);
    changes.push({ from, to, insert: transform(state.sliceDoc(from, to)) });
  }
  if (changes.length) view.dispatch({ changes });
  view.focus();
}

const toUpperCase = () => transformSelection((text) => text.toUpperCase());
const toLowerCase = () => transformSelection((text) => text.toLowerCase());

/** RFC 4122 version-4 UUID, lowercase, e.g. 57873991-a75b-4e56-adde-1cb82e93ab0f. */
function generateGuid() {
  if (crypto.randomUUID) return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
  bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
  const hex = [...bytes].map((b) => b.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10, 16).join("")}`;
}

function insertGuid() {
  const selection = view.state.selection.main;
  const guid = generateGuid();
  view.dispatch({
    changes: { from: selection.from, to: selection.to, insert: guid },
    selection: { anchor: selection.from + guid.length },
    scrollIntoView: true,
  });
  view.focus();
}

/** Reveals the active file in the OS file manager. */
async function revealActiveFile() {
  const tab = activeTab();
  if (!tab || !tab.path) return;
  try {
    await invoke("reveal_in_file_manager", { path: tab.path });
  } catch (error) {
    await message(`Could not show ${tab.name}\n\n${error}`, { title: "JustCode", kind: "error" });
  }
}

async function openExternalUrl(url) {
  try {
    await invoke("open_url", { url });
  } catch (error) {
    await message(`Could not open ${url}\n\n${error}`, { title: "JustCode", kind: "error" });
  }
}
setLinkHandler(openExternalUrl);

/**
 * Offers to save before the window goes away. Returns false to call the whole
 * thing off. Anything other than an explicit "don't save" keeps the files.
 */
// True while a close is being decided, so a second attempt (Alt+F4 during the
// prompt, or the window button while File ▸ Exit is mid-flight) cannot start a
// second `saveAllTabs()` racing the first over the same files.
let confirmingClose = false;

async function confirmClose() {
  if (confirmingClose) return false;
  confirmingClose = true;
  try {
    const modified = tabs.filter(isModified);
    if (!modified.length) return true;
    const answer = await askSaveChanges(modified.map((tab) => tab.name));
    if (answer === "cancel") return false;
    if (answer === "save") return await saveAllTabs();
    return true;
  } catch (error) {
    // Never leave the window un-closable because a dialog failed; say what
    // happened and treat it as "do not close".
    await message(`Could not check for unsaved changes.\n\n${error}`, {
      title: "JustCode",
      kind: "error",
    });
    return false;
  } finally {
    confirmingClose = false;
  }
}

// Set once the user has agreed to close, so the close-requested handler lets the
// second attempt through instead of asking all over again.
let closingWindow = false;

/**
 * Shuts the window, having already settled any unsaved work.
 *
 * `destroy` is preferred because it does not come back through the
 * close-requested handler. It needs `core:window:allow-destroy`, so if the
 * capability is ever missing this falls back to `close` rather than leaving the
 * user unable to quit — and if both fail it says so instead of failing silently.
 */
async function closeWindow() {
  if (!underTauri()) return;
  // Belt and braces — the close paths below flush before their dialogs, but a
  // route that reaches here another way still gets one last write in.
  flushSessionSave();
  const window_ = getCurrentWindow();
  try {
    await window_.destroy();
  } catch (destroyError) {
    closingWindow = true;
    try {
      await window_.close();
    } catch (closeError) {
      closingWindow = false;
      await message(`Could not close the window.\n\n${destroyError}\n${closeError}`, {
        title: "JustCode",
        kind: "error",
      });
    }
  }
}

/**
 * Drops the window to the taskbar. Needs `core:window:allow-minimize`; a
 * failure is reported in the status bar rather than swallowed, since nothing
 * visible happens on success either and a silent no-op reads as a dead key.
 */
function minimizeWindow() {
  // Nothing to minimise when the page is served to a plain browser.
  if (!underTauri()) return;
  getCurrentWindow()
    .minimize()
    .catch((error) => flashStatus(String(error?.message || error)));
}

/** Offers to save unsaved work, then closes the window. */
async function exitApp() {
  try {
    // Before anything else: `destroy()` tears the webview down the instant it
    // is called, and a localStorage write made in that last moment can be lost
    // before the engine has put it on disk. Writing here leaves the whole
    // prompt-and-shutdown sequence for it to be persisted in.
    flushSessionSave();
    if (!(await confirmClose())) return;
    await closeAllTerminals();
    await closeWindow();
  } catch (error) {
    await message(`${error}`, { title: "JustCode", kind: "error" });
  }
}

// The window's own close button bypasses File ▸ Exit entirely, so it gets the
// same prompt. `preventDefault` has to happen before the first await, or the
// window is already on its way out by the time the dialog appears.
if (underTauri()) {
  getCurrentWindow().onCloseRequested(async (event) => {
    // Already agreed — this is the fallback `close` coming back round.
    if (closingWindow) return;
    event.preventDefault();
    try {
      // Same reasoning as in exitApp: get the session onto disk while there is
      // still a shutdown's worth of time for the write to land.
      flushSessionSave();
      if (!(await confirmClose())) return;
      // Kill the shells before the window goes, or they linger as orphans.
      await closeAllTerminals();
      await closeWindow();
    } catch (error) {
      // An unhandled rejection here would leave the window permanently
      // un-closable, since the close has already been prevented.
      await message(`${error}`, { title: "JustCode", kind: "error" });
    }
  });
}

// ------------------------------------------------------------ view preferences

/**
 * Applies a zoom level to every open document. Going through a transaction (not
 * raw CSS) is what keeps the gutter aligned: CodeMirror re-measures line heights
 * as part of the update, so line numbers and lint markers stay level with their
 * lines. Background tabs have no view to dispatch through, so their stored
 * states are advanced directly, exactly as the theme does.
 */
function setFontSize(size) {
  fontSize = Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, size));
  applyEffectsEverywhere(fontSizeEffect(fontSize));
  // The transactions above are what make CodeMirror aware of the change; this
  // asks it to re-read line heights once the new styles have been applied, so
  // the gutter rows are rebuilt at the new spacing rather than the old.
  for (const pane of panes) pane.view.requestMeasure();
  // The terminals are part of the same window and read at the same distance, so
  // they zoom with the editor rather than staying at a fixed size.
  setTerminalFontSize(fontSize);
  dom.zoomLevel.textContent = `${fontSize}px`;
  localStorage.setItem(STORAGE.fontSize, String(fontSize));
}

// One icon and one label key per theme, in menu/toolbar-facing order — used
// both for the toolbar button and for computing what it cycles to next.
const THEME_ICONS = { dark: "moon", light: "sun", autism: "leaf" };
const THEME_LABEL_KEYS = { dark: "view.darkTheme", light: "view.lightTheme", autism: "view.autismTheme" };

/**
 * Applies a theme to the app chrome and to every open document. Tabs that are
 * not on screen have no view to dispatch through, so their stored states are
 * advanced directly.
 */
function setTheme(next) {
  theme = THEMES.includes(next) ? next : "dark";
  document.documentElement.dataset.theme = theme;

  applyEffectsEverywhere(themeEffect(theme));

  // The toolbar button shows the *current* theme's icon — with three themes,
  // "which one would clicking give me" is no longer obvious from a binary
  // sun/moon, but "which one am I looking at right now" always is. The
  // tooltip instead names what a click switches *to*, so the predictive
  // half of the old design survives in the one place text can carry it.
  const nextTheme = THEMES[(THEMES.indexOf(theme) + 1) % THEMES.length];
  dom.themeButton.innerHTML = iconMarkup(THEME_ICONS[theme] ?? "sun");
  dom.themeButton.title = t("toolbar.switchToTheme", { theme: t(THEME_LABEL_KEYS[nextTheme]) });
  // The button has no visible text of its own — icon only — so the
  // accessible name has to be set explicitly rather than relying on the
  // `title` fallback, which is what most screen reader/browser pairings
  // read reliably.
  dom.themeButton.setAttribute("aria-label", dom.themeButton.title);
  localStorage.setItem(STORAGE.theme, theme);
  // The terminals read their colours from the same CSS variables, but xterm
  // caches them, so it has to be told the palette changed.
  refreshTerminalTheme();
}

/** Rotates Dark ▸ Light ▸ Autism ▸ Dark — the toolbar button and Ctrl+Shift+T. */
function cycleTheme() {
  setTheme(THEMES[(THEMES.indexOf(theme) + 1) % THEMES.length]);
}

function setToolbarVisible(visible) {
  showToolbar = visible;
  dom.app.classList.toggle("hide-toolbar", !visible);
  for (const pane of panes) pane.view.requestMeasure();
  localStorage.setItem(STORAGE.toolbar, String(visible));
}

function setStatusbarVisible(visible) {
  showStatusbar = visible;
  dom.app.classList.toggle("hide-statusbar", !visible);
  for (const pane of panes) pane.view.requestMeasure();
  localStorage.setItem(STORAGE.statusbar, String(visible));
}

/** Turns the webview's spell checker on or off for every open document. */
function setSpellcheck(enabled) {
  spellcheck = enabled;
  applyEffectsEverywhere(spellcheckEffect(enabled));
  localStorage.setItem(STORAGE.spellcheck, String(enabled));
  // A linter only runs itself when the document changes, so a toggle needs an
  // explicit pass — to add the markers when switching on, and just as
  // importantly to clear them when switching off.
  // Background tabs hold their own state and are refreshed the same way, or a
  // tab parked while checking was on would still be carrying its markers when
  // it is next shown.
  const refreshAll = () => applyEffectsEverywhere(refreshSpelling.of(null));
  if (enabled) loadDictionary().then(refreshAll);
  else refreshAll();
  if (view) view.focus();
}

// ---------------------------------------------------------------- terminals

initTerminals({
  workspace: dom.workspace,
  dock: localStorage.getItem(STORAGE.terminalDock),
  onVisibility: () => {
    for (const pane of panes) pane.view.requestMeasure();
  },
  onDock: (edge) => {
    localStorage.setItem(STORAGE.terminalDock, edge);
    // The panes have just been given more or less room; their gutters are
    // measured from the old width until they are asked to look again.
    for (const pane of panes) pane.view.requestMeasure();
    createMenuBar(dom.menubar, buildMenus());
  },
  // New terminals start beside the file being edited, which is nearly always
  // where a build or a git command wants to run.
  currentDirectory: () => {
    const path = activeTab()?.path;
    return path ? path.replace(/[\\/][^\\/]*$/, "") : null;
  },
});

window.addEventListener("resize", () => relayoutTerminals());

/** Opens a shell in its own window; elevated ones cannot be embedded. */
async function externalTerminal(profile, elevated) {
  try {
    await openExternalTerminal(profile, elevated);
  } catch (error) {
    await message(`${error}`, { title: "JustCode", kind: "error" });
  }
}

/** Turns line wrapping on or off for every open document. */
function setWordWrap(enabled) {
  wordWrap = enabled;
  applyEffectsEverywhere(wordWrapEffect(enabled));
  localStorage.setItem(STORAGE.wordWrap, String(enabled));
  if (view) view.focus();
}

/** Turns Bionic Reading on or off for every open document. */
function setBionicReading(enabled) {
  bionicReading = enabled;
  applyEffectsEverywhere(bionicReadingEffect(enabled));
  localStorage.setItem(STORAGE.bionicReading, String(enabled));
  if (view) view.focus();
}

/**
 * The file-association screen. Reads what is registered now so the boxes show
 * the real state, then writes back only the difference.
 */
async function chooseFileAssociations() {
  const groups = associationGroups();
  const every = groups.flatMap((group) => group.extensions.map((entry) => entry.extension));

  let registered = [];
  let foreign = [];
  try {
    // Settled, not all: one failing call must not discard the other's result.
    // Losing `registered` made the dialog offer the recommended set instead of
    // the real state, and then de-registered nothing on apply.
    const [gotRegistered, gotForeign] = await Promise.allSettled([
      invoke("associated_extensions", { extensions: every }),
      invoke("foreign_extensions", { extensions: every }),
    ]);
    registered = gotRegistered.status === "fulfilled" ? gotRegistered.value : [];
    foreign = gotForeign.status === "fulfilled" ? gotForeign.value : [];
  } catch {
    // Not running under Tauri, or not on Windows — start from the recommended set.
    registered = [];
    foreign = [];
  }

  // Anything another program already handles is labelled and left unticked, so
  // clicking through this screen cannot quietly take .pas away from Delphi.
  const owners = new Map(foreign);
  for (const group of groups) {
    for (const entry of group.extensions) {
      entry.owner = owners.get(entry.extension) || null;
      if (entry.owner) entry.recommended = false;
    }
  }

  // First run: nothing registered yet, so offer the sensible defaults ticked.
  const initial = registered.length
    ? registered
    : groups.flatMap((group) =>
        group.extensions.filter((entry) => entry.recommended).map((entry) => entry.extension),
      );

  showAssociations(groups, initial, async (chosen) => {
    const wanted = new Set(chosen);
    const labels = new Map();
    for (const group of groups) {
      for (const entry of group.extensions) labels.set(entry.extension, entry.label);
    }
    const associate = [...wanted].map((extension) => [extension, labels.get(extension) || extension]);
    const remove = registered.filter((extension) => !wanted.has(extension));

    try {
      const results = await invoke("set_file_associations", { associate, remove });
      // Windows refusing to hand over a type is not the same as failing to
      // write it, so the two are reported separately.
      const blocked = results
        .filter((entry) => entry.startsWith("userchoice:"))
        .map((entry) => `.${entry.slice("userchoice:".length)}`);
      const failures = results.filter((entry) => !entry.startsWith("userchoice:"));

      if (failures.length) {
        await message(`${t("assoc.someFailed")}\n\n${failures.join("\n")}`, {
          title: "JustCode",
          kind: "warning",
        });
      }
      if (blocked.length) {
        // These have a default recorded by the user that applications are not
        // permitted to overwrite — offer the one place it can be changed.
        const open = await ask(
          `${t("assoc.userChoiceBlocked", { list: blocked.join(", ") })}\n\n${t("assoc.openSettings")}`,
          {
            title: "JustCode",
            kind: "info",
            okLabel: t("assoc.openSettingsOk"),
            cancelLabel: t("dialog.cancel"),
          },
        );
        if (open) {
          try {
            await invoke("open_default_apps_settings");
          } catch (error) {
            await message(`${error}`, { title: "JustCode", kind: "error" });
          }
        }
      } else if (!failures.length) {
        flashStatus(t("assoc.applied", { n: wanted.size }));
      }
    } catch (error) {
      await message(`${error}`, { title: "JustCode", kind: "error" });
    }
  });
}

// ------------------------------------------------------------- context menu

/** The right-click menu inside the editor. */
function editorContextMenu(event) {
  event.preventDefault();
  // Right-clicking in an unfocused pane should act on that pane, and outside
  // any selection it should still work on the caret's line.
  const pane = panes.find((entry) => entry.root.contains(event.target));
  if (pane && pane.id !== activePaneId) focusPane(pane.id);

  showContextMenu(event.clientX, event.clientY, [
    { label: t("edit.undo"), icon: "undo", accel: "Ctrl+Z", run: editUndo, enabled: hasTab },
    { label: t("edit.redo"), icon: "redo", accel: "Ctrl+Y", run: editRedo, enabled: hasTab },
    { separator: true },
    { label: t("edit.cut"), icon: "cut", accel: "Ctrl+X", run: editCut, enabled: hasTab },
    { label: t("edit.copy"), icon: "copy", accel: "Ctrl+C", run: editCopy, enabled: hasTab },
    { label: t("edit.paste"), icon: "paste", accel: "Ctrl+V", run: editPaste, enabled: hasTab },
    { separator: true },
    { label: t("edit.selectAll"), icon: "selectAll", accel: "Ctrl+A", run: editSelectAll, enabled: hasTab },
    { label: t("edit.toggleComment"), icon: "comment", accel: "Ctrl+/", run: toggleComment, enabled: hasTab },
    { label: t("edit.deleteLine"), icon: "deleteLine", accel: "Ctrl+Shift+K", run: deleteCurrentLine, enabled: hasTab },
    { separator: true },
    {
      label: t("edit.goToSymbol"),
      icon: "symbol",
      accel: "Ctrl+Shift+G",
      run: goToSymbol,
      enabled: () => hasTab() && supportsSymbols(activeTab().language),
    },
    { separator: true },
    { label: t("edit.find"), icon: "search", accel: "Ctrl+F", run: openFind, enabled: hasTab },
    { label: t("edit.replace"), icon: "search", accel: "Ctrl+H", run: openReplace, enabled: hasTab },
  ]);
}

// ---------------------------------------------------------------- the menu bar

/**
 * The menus are built from scratch on every language change, so labels come
 * from `t()` at build time rather than being baked in once at startup.
 */
function buildMenus() {
  const nextTheme = THEMES[(THEMES.indexOf(theme) + 1) % THEMES.length];
  return [
  {
    label: t("menu.file"),
    mnemonic: "f",
    items: [
      { label: t("file.new"), icon: "file", accel: "Ctrl+N", run: newFile },
      { label: t("file.open"), icon: "folder", accel: "Ctrl+O", run: openFile },
      {
        label: t("file.recent"),
        icon: "clock",
        // Built fresh each time the menu opens, so it is never stale.
        submenu: () => {
          const recent = readRecentFiles();
          if (!recent.length) return [{ label: t("file.recentEmpty"), enabled: () => false, run() {} }];
          return recent.map((path) => ({
            label: baseName(path),
            hint: path,
            run: () => openPath(path).then(renderStatus),
          }));
        },
      },
      { separator: true },
      {
        label: t("file.save"),
        icon: "save",
        accel: "Ctrl+S",
        enabled: () => activeTab() !== null,
        run: () => saveTab(activeTab()),
      },
      {
        label: t("file.saveAs"),
        icon: "saveAs",
        accel: "Ctrl+Shift+S",
        enabled: () => activeTab() !== null,
        run: () => saveTab(activeTab(), { forcePrompt: true }),
      },
      {
        label: t("file.saveAll"),
        icon: "save",
        accel: "Ctrl+Alt+S",
        enabled: () => tabs.some(isModified),
        run: saveAllTabs,
      },
      {
        label: t(REVEAL_KEY),
        icon: "folder",
        enabled: () => Boolean(activeTab()?.path),
        run: revealActiveFile,
      },
      {
        label: t("file.copyPath"),
        icon: "copy",
        enabled: () => Boolean(activeTab()?.path),
        run: copyPathToClipboard,
      },
      { separator: true },
      { label: t("file.associations"), icon: "link", run: chooseFileAssociations },
      { separator: true },
      { label: t("file.run"), icon: "play", accel: "F5", run },
      {
        label: t("file.runInTerminal"),
        icon: "terminal",
        accel: "Ctrl+F5",
        // Only scripts; there is nothing to run for a .txt or a .json.
        enabled: () => runnableTab() !== null,
        run: runInTerminalCommand,
      },
      { separator: true },
      {
        label: t("file.closeTab"),
        icon: "close",
        accel: "Ctrl+W",
        enabled: () => activeTab() !== null,
        run: () => activeTabId !== null && closeTab(activeTabId),
      },
      {
        label: t("file.closeOthers"),
        icon: "close",
        enabled: () => tabs.length > 1,
        run: closeOtherTabs,
      },
      {
        label: t("file.closeAll"),
        icon: "close",
        accel: "Ctrl+Shift+W",
        enabled: () => tabs.length > 0,
        run: closeAllTabs,
      },
      { separator: true },
      { label: t("file.minimize"), icon: "minimize", accel: "Alt+M", run: minimizeWindow },
      { label: t("file.exit"), icon: "exit", accel: "Alt+F4", run: exitApp },
    ],
  },
  {
    label: t("menu.edit"),
    mnemonic: "e",
    items: [
      // `enabled` on everything that writes: with no tab open the editor shows
      // a read-only placeholder belonging to no file, and a direct dispatch
      // bypasses EditorState.readOnly — the text would land there and be lost.
      { label: t("edit.undo"), icon: "undo", accel: "Ctrl+Z", run: editUndo, enabled: hasTab },
      { label: t("edit.redo"), icon: "redo", accel: "Ctrl+Y", run: editRedo, enabled: hasTab },
      { separator: true },
      { label: t("edit.cut"), icon: "cut", accel: "Ctrl+X", run: editCut, enabled: hasTab },
      { label: t("edit.copy"), icon: "copy", accel: "Ctrl+C", run: editCopy, enabled: hasTab },
      { label: t("edit.paste"), icon: "paste", accel: "Ctrl+V", run: editPaste, enabled: hasTab },
      { separator: true },
      { label: t("edit.selectAll"), icon: "selectAll", accel: "Ctrl+A", run: editSelectAll, enabled: hasTab },
      { label: t("edit.deleteLine"), icon: "deleteLine", accel: "Ctrl+Shift+K", run: deleteCurrentLine, enabled: hasTab },
      {
        label: t("edit.moveLineUp"),
        icon: "arrowUp",
        accel: "Alt+↑",
        run: editMoveLineUp,
        enabled: hasTab,
      },
      {
        label: t("edit.moveLineDown"),
        icon: "arrowDown",
        accel: "Alt+↓",
        run: editMoveLineDown,
        enabled: hasTab,
      },
      { separator: true },
      { label: t("edit.find"), icon: "search", accel: "Ctrl+F", run: openFind, enabled: hasTab },
      { label: t("edit.replace"), icon: "search", accel: "Ctrl+H", run: openReplace, enabled: hasTab },
      { label: t("edit.findNext"), icon: "arrowDown", accel: "F3", run: () => findNext(view), enabled: hasTab },
      { label: t("edit.findPrevious"), icon: "arrowUp", accel: "Shift+F3", run: () => findPrevious(view), enabled: hasTab },
      { separator: true },
      {
        label: t("edit.goToSymbol"),
        icon: "symbol",
        accel: "Ctrl+Shift+G",
        run: goToSymbol,
        // Greyed out for languages with no symbol rules, rather than opening
        // and immediately reporting that nothing was found.
        enabled: () => hasTab() && supportsSymbols(activeTab().language),
      },
      { separator: true },
      { label: t("edit.toggleComment"), icon: "comment", accel: "Ctrl+/", run: toggleComment, enabled: hasTab },
      { label: t("edit.uppercase"), icon: "upper", accel: "Ctrl+Shift+U", run: toUpperCase, enabled: hasTab },
      { label: t("edit.lowercase"), icon: "lower", accel: "Ctrl+Shift+L", run: toLowerCase, enabled: hasTab },
      { label: t("edit.sortCaseSensitive"), icon: "sort", run: sortSelectedLines, enabled: hasTab },
      { label: t("edit.guid"), icon: "guid", accel: "Ctrl+Alt+G", run: insertGuid, enabled: hasTab },
      { separator: true },
      {
        label: t("edit.toggleBookmark"),
        icon: "bookmark",
        accel: "Ctrl+Shift+1…3",
        run: () => {
          toggleNextBookmark(view);
          view.focus();
        },
      },
      ...BOOKMARK_SLOTS.map((slot) => ({
        label: t("edit.gotoBookmark", { n: slot }),
        icon: "bookmark",
        accel: `Ctrl+${slot}`,
        run: () => gotoBookmark(view, slot),
      })),
    ],
  },
  {
    label: t("menu.view"),
    mnemonic: "v",
    items: [
      { label: t("view.zoomIn"), icon: "zoomIn", accel: "Ctrl++", run: () => setFontSize(fontSize + 1) },
      { label: t("view.zoomOut"), icon: "zoomOut", accel: "Ctrl+-", run: () => setFontSize(fontSize - 1) },
      {
        label: t("view.resetZoom"),
        icon: "zoomReset",
        accel: "Ctrl+0",
        run: () => setFontSize(DEFAULT_FONT_SIZE),
      },
      { separator: true },
      {
        label: t("view.toolbar"),
        icon: "toolbar",
        accel: "Alt+T",
        checked: () => showToolbar,
        run: () => setToolbarVisible(!showToolbar),
      },
      {
        label: t("view.statusBar"),
        icon: "statusbar",
        accel: "Alt+S",
        checked: () => showStatusbar,
        run: () => setStatusbarVisible(!showStatusbar),
      },
      { separator: true },
      {
        label: t("view.wordWrap"),
        icon: "wordWrap",
        accel: "Alt+Z",
        checked: () => wordWrap,
        run: () => setWordWrap(!wordWrap),
      },
      {
        label: t("view.spellCheck"),
        icon: "spellcheck",
        accel: "F7",
        checked: () => spellcheck,
        run: () => setSpellcheck(!spellcheck),
      },
      {
        label: t("view.bionicReading"),
        icon: "bold",
        accel: "Ctrl+Shift+B",
        checked: () => bionicReading,
        run: () => setBionicReading(!bionicReading),
      },
      { separator: true },
      {
        label: t("tabs.moveLeft"),
        icon: "arrowLeft",
        accel: "Ctrl+Shift+PageUp",
        enabled: hasTab,
        run: () => moveTab(-1),
      },
      {
        label: t("tabs.moveRight"),
        icon: "arrowRight",
        accel: "Ctrl+Shift+PageDown",
        enabled: hasTab,
        run: () => moveTab(1),
      },
      {
        label: t("tabs.moveToStart"),
        icon: "moveToStart",
        accel: "Alt+Home",
        enabled: hasTab,
        run: moveTabToStart,
      },
      { separator: true },
      {
        label: t("view.splitUp"),
        icon: "splitUp",
        accel: "Ctrl+K ↑",
        enabled: canSplit,
        run: () => splitActive("top"),
      },
      {
        label: t("view.splitDown"),
        icon: "splitDown",
        accel: "Ctrl+K ↓",
        enabled: canSplit,
        run: () => splitActive("bottom"),
      },
      {
        label: t("view.splitLeft"),
        icon: "splitLeft",
        accel: "Ctrl+K ←",
        enabled: canSplit,
        run: () => splitActive("left"),
      },
      {
        label: t("view.splitRight"),
        icon: "splitRight",
        accel: "Ctrl+K →",
        enabled: canSplit,
        run: () => splitActive("right"),
      },
      { separator: true },
      // Ctrl+Shift+T rotates dark ▸ light ▸ autism ▸ dark; shown only on
      // whichever of the three is next in that rotation, so the hint stays
      // accurate to what pressing it right now actually does.
      {
        label: t("view.darkTheme"),
        icon: "moon",
        accel: nextTheme === "dark" ? "Ctrl+Shift+T" : undefined,
        checked: () => theme === "dark",
        run: () => setTheme("dark"),
      },
      {
        label: t("view.lightTheme"),
        icon: "sun",
        accel: nextTheme === "light" ? "Ctrl+Shift+T" : undefined,
        checked: () => theme === "light",
        run: () => setTheme("light"),
      },
      {
        label: t("view.autismTheme"),
        icon: "leaf",
        hint: t("view.autismThemeHint"),
        accel: nextTheme === "autism" ? "Ctrl+Shift+T" : undefined,
        checked: () => theme === "autism",
        run: () => setTheme("autism"),
      },
      { separator: true },
      {
        label: t("view.terminal"),
        icon: "terminal",
        accel: "Ctrl+`",
        checked: () => terminalsVisible(),
        run: () => toggleTerminals(),
      },
      {
        // Dragging the panel's header to an edge does the same thing; this is
        // for finding out that it can be moved at all.
        label: t("terminal.position"),
        icon: "terminal",
        submenu: () => [
          {
            label: t("terminal.dockLeft"),
            icon: "splitLeft",
            checked: () => terminalDock() === "left",
            run: () => setTerminalDock("left"),
          },
          {
            label: t("terminal.dockBottom"),
            icon: "splitDown",
            checked: () => terminalDock() === "bottom",
            run: () => setTerminalDock("bottom"),
          },
          {
            label: t("terminal.dockRight"),
            icon: "splitRight",
            checked: () => terminalDock() === "right",
            run: () => setTerminalDock("right"),
          },
        ],
      },
      {
        label: t("terminal.newIn"),
        icon: "terminalAdd",
        submenu: () => [
          ...TERMINAL_PROFILES.map((profile) => ({
            label: profile.label,
            icon: "terminal",
            // The shortcut opens whichever shell is first for this platform, so
            // it is shown against that one rather than on the parent item —
            // "Ctrl+Shift+`" next to "PowerShell" says what it does; next to
            // "New Terminal" it would not.
            accel: profile.id === DEFAULT_TERMINAL.id ? "Ctrl+Shift+`" : undefined,
            run: () => openTerminal(profile.id),
          })),
          { separator: true },
          // Elevated shells run in their own window: a process that is not
          // elevated cannot read an elevated child's pipes, so there is no way
          // to show one inside the app.
          ...TERMINAL_PROFILES.map((profile) => ({
            label: t("terminal.asAdmin", { name: profile.label }),
            icon: "shield",
            run: () => externalTerminal(profile.id, true),
          })),
        ],
      },
      { separator: true },
      { label: t("view.language"), icon: "globe", run: chooseAppLanguage },
      { separator: true },
      { label: t("view.problems"), icon: "warning", accel: "F8", run: showProblems },
      {
        label: t("view.nextProblem"),
        icon: "arrowDown",
        accel: "F4",
        enabled: hasTab,
        run: () => goToProblem(1),
      },
      {
        label: t("view.previousProblem"),
        icon: "arrowUp",
        accel: "Shift+F4",
        enabled: hasTab,
        run: () => goToProblem(-1),
      },
    ],
  },
  {
    label: t("menu.help"),
    mnemonic: "h",
    items: [
      { label: t("help.center"), icon: "help", accel: "F1", run: showHelp },
      { separator: true },
      { label: t("help.about"), icon: "info", run: showAboutDialog },
    ],
  },
  ];
}

/** Opens the interface-language chooser and applies the pick. */
function chooseAppLanguage() {
  showLanguageDialog(currentLocale(), async (code) => {
    await setLocale(code);
    localStorage.setItem(STORAGE.locale, code);
  });
}

/** Redraws everything whose text comes from a translation. */
function applyTranslations() {
  createMenuBar(dom.menubar, buildMenus());
  const label = (id, key) => {
    const span = document.querySelector(`#${id} span`);
    if (span) span.textContent = t(key);
  };
  label("btn-new", "toolbar.new");
  label("btn-open", "toolbar.open");
  label("btn-save", "toolbar.save");
  label("btn-run", "toolbar.run");
  dom.statusLang.title = t("status.selectLanguage");
  dom.statusCursor.title = t("status.lineColumn");
  renderTabs();
  renderStatus();
}

onLocaleChange(applyTranslations);
createMenuBar(dom.menubar, buildMenus());

/** Reads the version from Tauri, falling back when running in a browser. */
async function showAboutDialog() {
  let version = APP_VERSION;
  try {
    version = await getVersion();
  } catch {
    // Not running under Tauri — the bundled constant is right anyway.
  }
  showAbout(version);
}

/**
 * Live state for the two Help Centre pages with an action button. Getters
 * rather than plain values, since the Help Centre re-reads them after its
 * own button clicks instead of closing and reopening.
 */
function showHelp() {
  showHelpCenter({
    autismTheme: () => theme === "autism",
    setAutismTheme: () => setTheme("autism"),
    bionicReading: () => bionicReading,
    toggleBionicReading: () => setBionicReading(!bionicReading),
  });
}

// ------------------------------------------------------------------- wiring

function decorate(id, icon) {
  const button = document.getElementById(id);
  button.insertAdjacentHTML("afterbegin", iconMarkup(icon));
  return button;
}

decorate("btn-new", "file").addEventListener("click", newFile);
decorate("btn-open", "folder").addEventListener("click", openFile);
decorate("btn-save", "save").addEventListener("click", () => saveTab(activeTab()));
decorate("btn-run", "play").addEventListener("click", run);
decorate("btn-zoom-in", "zoomIn").addEventListener("click", () => setFontSize(fontSize + 1));
decorate("btn-zoom-out", "zoomOut").addEventListener("click", () => setFontSize(fontSize - 1));
dom.themeButton.addEventListener("click", cycleTheme);
dom.statusProblems.addEventListener("click", showProblems);
dom.statusLang.addEventListener("click", showLanguagePicker);
dom.statusPath.addEventListener("click", copyPathToClipboard);

// Double-clicking the blank strip to the right of the tabs opens a new file,
// the way browsers do. Double rather than single click so a stray click aimed
// at nothing does not litter the bar with untitled files. The target check
// keeps double-clicks on a tab itself out of it.
//
// The work is deferred: opening a file swaps the editor state and rebuilds this
// very tab bar, and doing that synchronously re-enters CodeMirror while it is
// still settling focus from the same click, which throws "setState ... while an
// update is in progress" and wedges the view.
// (attached per pane in attachPaneDropZones — each pane has its own tab bar)

// Ctrl+K followed by an arrow splits the view, the VS Code chord. `false` until
// Ctrl+K is pressed, and cleared by the very next key whatever it turns out
// to be, so a stray Ctrl+K cannot leave the next keystroke swallowed.
let splitChordArmed = false;
const SPLIT_CHORD_EDGES = {
  ArrowUp: "top",
  ArrowDown: "bottom",
  ArrowLeft: "left",
  ArrowRight: "right",
};

// Captured before CodeMirror sees the event so the app shortcuts always win.
window.addEventListener(
  "keydown",
  (event) => {
    const ctrl = event.ctrlKey || event.metaKey;

    // A modal owns the keyboard while it is up. Without this, F1 during the
    // save-changes prompt opened the shortcut list, which closed the prompt,
    // which resolved it as "cancel" — silently abandoning the window close.
    // Escape still reaches the overlay's own handler.
    if (isOverlayOpen() && event.key !== "Escape") return;

    // AltGr on Windows arrives as Ctrl+Alt. Treating it as a shortcut swallows
    // the characters it is there to type — `@` and `#` on a Turkish layout.
    // The two deliberate Ctrl+Alt bindings are handled explicitly further down.
    const altGr = ctrl && event.altKey;

    // Second half of the Ctrl+K chord: an arrow key splits, anything else just
    // cancels it. Checked before everything so the chord cannot be hijacked.
    if (splitChordArmed) {
      // A lone modifier press is part of the chord, not the end of it.
      if (!["Control", "Shift", "Alt", "Meta"].includes(event.key)) splitChordArmed = false;
      const edge = SPLIT_CHORD_EDGES[event.key];
      if (edge) {
        event.preventDefault();
        event.stopPropagation();
        splitActive(edge);
        return;
      }
    }

    if (event.key === "F5") {
      event.preventDefault();
      // Ctrl+F5 keeps the run inside the app; plain F5 opens a console or a
      // browser, depending on the file.
      if (ctrl) runInTerminalCommand();
      else run();
      return;
    }
    // stopPropagation as well as preventDefault: CodeMirror's lint keymap also
    // claims F8, for "go to next problem". Left to run, it moved the caret on
    // every toggle — including the press that closes the panel again.
    if (event.key === "F8" && !ctrl) {
      event.preventDefault();
      event.stopPropagation();
      showProblems();
      return;
    }
    // Walking the problems is F4, Shift+F4 backwards — the Visual Studio
    // convention, and it leaves F8 free to mean one thing.
    if (event.key === "F4" && !ctrl) {
      event.preventDefault();
      event.stopPropagation();
      goToProblem(event.shiftKey ? -1 : 1);
      return;
    }
    if (event.key === "F1" && !ctrl) {
      event.preventDefault();
      showHelp();
      return;
    }
    // Alt+Z for word wrap, the same binding VS Code uses. Plain Alt rather
    // than a Ctrl combo: every Ctrl+Alt+<letter> is indistinguishable from
    // AltGr typing a special character (see the `altGr` note below), so it
    // is not safe ground for a new binding.
    if (!ctrl && event.altKey && !event.shiftKey && event.key.toLowerCase() === "z") {
      event.preventDefault();
      setWordWrap(!wordWrap);
      return;
    }
    // F7 for spell check: the Word/Outlook convention, close enough even
    // though those run a one-off pass rather than toggling a live checker.
    if (event.key === "F7" && !ctrl) {
      event.preventDefault();
      setSpellcheck(!spellcheck);
      return;
    }
    // Alt+T / Alt+S for the toolbar and status bar. Same reasoning as Alt+Z
    // above — plain Alt avoids the Ctrl+Alt/AltGr ambiguity — chosen to match
    // Word Wrap and Bionic Reading already having a binding, so every View
    // toggle behaves consistently rather than some being mouse/menu-only.
    if (!ctrl && event.altKey && !event.shiftKey && event.key.toLowerCase() === "t") {
      event.preventDefault();
      setToolbarVisible(!showToolbar);
      return;
    }
    if (!ctrl && event.altKey && !event.shiftKey && event.key.toLowerCase() === "s") {
      event.preventDefault();
      setStatusbarVisible(!showStatusbar);
      return;
    }
    // Alt+M drops the window to the taskbar — plain Alt for the same reason as
    // the toggles above, and M for the word every platform uses. Windows' own
    // Win+Down needs two presses from a maximised window (restore, then
    // minimise), which is exactly the case this is for. stopPropagation on top
    // of preventDefault: a focused terminal would otherwise still forward the
    // key to the shell as an escape sequence, so restoring the window would
    // show a stray `m` on the prompt.
    if (!ctrl && event.altKey && !event.shiftKey && event.key.toLowerCase() === "m") {
      event.preventDefault();
      event.stopPropagation();
      minimizeWindow();
      return;
    }
    // Alt+Home sends the tab to the front of the strip. Ctrl+Shift+Home, the
    // obvious pairing with the Ctrl+Shift+PageUp/PageDown that move it one
    // step, is already "select to the start of the document" — a binding worth
    // far more than this one.
    if (!ctrl && event.altKey && !event.shiftKey && event.key === "Home") {
      event.preventDefault();
      event.stopPropagation();
      moveTabToStart();
      return;
    }
    if (!ctrl) return;

    // Ctrl+Tab / Ctrl+Shift+Tab cycle through the pane's tabs. Handled here so
    // the editor never sees it as an indent.
    if (event.key === "Tab" && !altGr) {
      event.preventDefault();
      event.stopPropagation();
      switchTab(event.shiftKey ? -1 : 1);
      return;
    }

    // Ctrl+1..3 jump to a bookmark, Ctrl+Shift+1..3 set or clear one. Matched on
    // `code` rather than `key`: with Shift held the digits arrive as !, @ and #.
    const digit = altGr ? null : /^Digit([123])$/.exec(event.code);
    if (digit) {
      event.preventDefault();
      event.stopPropagation();
      const slot = Number(digit[1]);
      if (event.shiftKey) toggleBookmark(view, slot);
      else gotoBookmark(view, slot);
      return;
    }

    // Ctrl+PageUp/Down switch tabs; adding Shift moves the current tab — the
    // same tab bindings VS Code uses. Handled here rather than in the editor
    // keymap because they act on the tab model, not the document.
    if (!altGr && (event.key === "PageUp" || event.key === "PageDown")) {
      event.preventDefault();
      const direction = event.key === "PageDown" ? 1 : -1;
      if (event.shiftKey) moveTab(direction);
      else switchTab(direction);
      return;
    }

    // Ctrl+` toggles the terminal and Ctrl+Shift+` opens another one, as in
    // VS Code. Shift+` is `~` on most layouts, so this already matched on
    // `event.code` — which meant Ctrl+Shift+` silently toggled instead of
    // being free to bind.
    if (!altGr && (event.key === "`" || event.key === "~" || event.code === "Backquote")) {
      event.preventDefault();
      event.stopPropagation();
      if (event.shiftKey) {
        // Which shell this starts depends on the platform; see PROFILES.
        openTerminal().catch((error) => flashStatus(String(error?.message || error)));
      } else {
        toggleTerminals();
      }
      return;
    }

    const key = event.key.toLowerCase();
    // Commands that write need a real file; the placeholder shown when nothing
    // is open is read-only, and a direct dispatch would bypass that and lose
    // the text on the next tab switch.
    const writes = () => hasTab();

    if (key === "k" && !event.shiftKey && !event.altKey) {
      // Arm the chord; the arrow that finishes it is handled at the top.
      event.preventDefault();
      event.stopPropagation();
      splitChordArmed = true;
    } else if (altGr) {
      // Nothing below is a real Ctrl shortcut when AltGr produced the key.
    } else if (key === "o") {
      event.preventDefault();
      openFile();
    } else if (key === "s") {
      event.preventDefault();
      if (event.altKey) saveAllTabs();
      else saveTab(activeTab(), { forcePrompt: event.shiftKey });
    } else if (key === "n") {
      event.preventDefault();
      newFile();
    } else if (key === "w") {
      event.preventDefault();
      if (event.shiftKey) closeAllTabs();
      else if (activeTabId !== null) closeTab(activeTabId);
    } else if (!altGr && (key === "+" || key === "=" || event.code === "NumpadAdd")) {
      event.preventDefault();
      setFontSize(fontSize + 1);
    } else if (!altGr && (key === "-" || key === "_" || event.code === "NumpadSubtract")) {
      event.preventDefault();
      setFontSize(fontSize - 1);
    } else if (!altGr && key === "0") {
      event.preventDefault();
      setFontSize(DEFAULT_FONT_SIZE);
    } else if (event.altKey && key === "g") {
      event.preventDefault();
      event.stopPropagation();
      if (writes()) insertGuid();
    } else if (!altGr && event.shiftKey && (key === "u" || key === "l" || key === "g")) {
      // stopPropagation as well: Ctrl+Shift+G is otherwise "find previous" in
      // the editor keymap, and we are deliberately overriding it.
      event.preventDefault();
      event.stopPropagation();
      if (key === "u") {
        if (writes()) toUpperCase();
      } else if (key === "l") {
        if (writes()) toLowerCase();
      } else {
        goToSymbol();
      }
    } else if (!altGr && event.shiftKey && key === "b") {
      event.preventDefault();
      setBionicReading(!bionicReading);
    } else if (!altGr && event.shiftKey && key === "t") {
      event.preventDefault();
      cycleTheme();
    }
  },
  true,
);

// Ctrl+wheel zoom, the same gesture every editor uses.
window.addEventListener(
  "wheel",
  (event) => {
    if (!event.ctrlKey) return;
    event.preventDefault();
    setFontSize(fontSize + (event.deltaY < 0 ? 1 : -1));
  },
  { passive: false },
);

// The webview's own context menu offers nothing useful for an editor, so inside
// a pane it is replaced with ours and suppressed everywhere else. Spelling
// suggestions used to be the one reason to keep it; they now live in the lint
// tooltip, which works whether or not the platform menu carries them.
window.addEventListener("contextmenu", (event) => {
  const inEditor = panes.some((pane) => pane.editorEl.contains(event.target));
  if (inEditor) editorContextMenu(event);
  else event.preventDefault();
});

// The stored theme has to be known before the first state is built, because
// setTheme dispatches into a compartment that only exists once one is.
const storedTheme = localStorage.getItem(STORAGE.theme);
theme = THEMES.includes(storedTheme) ? storedTheme : "dark";
const firstPane = createPane();
activePaneId = firstPane.id;
view = firstPane.view;
firstPane.root.classList.add("active");
view.setState(createState("", "html", listeners, theme, { readOnly: true }));
setTheme(theme);
setFontSize(Number(localStorage.getItem(STORAGE.fontSize)) || DEFAULT_FONT_SIZE);
setToolbarVisible(localStorage.getItem(STORAGE.toolbar) !== "false");
setStatusbarVisible(localStorage.getItem(STORAGE.statusbar) !== "false");
// Opt-in: only a stored "true" turns these on.
setSpellcheck(localStorage.getItem(STORAGE.spellcheck) === "true");
setWordWrap(localStorage.getItem(STORAGE.wordWrap) === "true");
setBionicReading(localStorage.getItem(STORAGE.bionicReading) === "true");
renderTabs();
renderStatus();

// Files double-clicked in Explorer arrive either as launch arguments or, if a
// window is already open, as an event from the single-instance plugin. The
// listener is registered before any awaiting below, so nothing is missed.
listen("open-files", (event) => openExternalFiles(event.payload || [])).catch(() => {});

/**
 * Everything that changes what is on screen, finished before the window is
 * revealed. Each of these used to run after the window appeared, so the first
 * second of the app was visibly unsettled: English chrome flipping to the
 * chosen language, a blank untitled tab being replaced by the file you
 * double-clicked, then highlighting arriving a moment later.
 */
async function loadStartupState() {
  // The interface language. English needs no download, so only a stored
  // non-default choice costs anything here.
  const storedLocale = localStorage.getItem(STORAGE.locale);
  const localeReady =
    storedLocale && storedLocale !== DEFAULT_LOCALE ? setLocale(storedLocale) : null;

  let paths = [];
  try {
    paths = (await invoke("startup_files")) || [];
  } catch {
    // Not running under Tauri.
  }

  if (paths.length) {
    // The session comes back first, so a file opened from Explorer joins the
    // workspace it interrupted — and lands on top of it — rather than replacing
    // it. Only a cold start reaches here: while the app is running, the
    // single-instance plugin forwards the file to the open window instead.
    await restoreSession();
    await openExternalFiles(paths);
  } else if (!(await restoreSession())) {
    // A blank document directly, not through the New File chooser — that
    // dialog is for an explicit File ▸ New, not a greeting at every launch.
    createFile(null);
  }

  // What is on screen is now settled, so saving it can no longer overwrite the
  // session being restored. The immediate write records anything that arrived
  // on the command line as part of this session.
  sessionReady = true;
  rememberSession();

  if (localeReady) await localeReady;

  // Syntax highlighting for whatever ended up on screen, so the first paint is
  // already coloured rather than plain text that lights up a moment later.
  const tab = activeTab();
  const pane = activePane();
  if (tab && pane) {
    try {
      await applyLanguage(pane.view, tab.language, () => pane.activeTabId === tab.id);
    } catch {
      // A grammar that will not load is not a reason to stay hidden.
    }
  }

  // Web fonts, so no text reflows just after the window appears.
  try {
    await document.fonts?.ready;
  } catch {
    // Font loading is best-effort.
  }
}

// A slow step must never leave the user staring at nothing, so readiness is
// capped. Rust has its own 4s backstop behind this one.
const startupReady = Promise.race([
  loadStartupState().catch(() => {}),
  new Promise((resolve) => setTimeout(resolve, 2500)),
]).then(() => {
  // A backstop for the two ways `loadStartupState` can end without having set
  // this itself — the 2.5s cap winning the race, or a throw on the way through.
  // Without it a launch that went wrong would also stop remembering anything.
  sessionReady = true;
});

// The window is created hidden so the first thing on screen is the finished
// editor, not an empty frame. `report_ready` shows it and records the boot time.
startupReady
  .then(
    () =>
      new Promise((resolve) =>
        // Two frames: the first schedules the layout everything above produced,
        // the second runs only once that layout has actually been painted.
        requestAnimationFrame(() => requestAnimationFrame(resolve)),
      ),
  )
  .then(() => {
    const nav = performance.getEntriesByType("navigation")[0];
    const detail = [
      `inPage=${Math.round(performance.now())}`,
      nav ? `domInteractive=${Math.round(nav.domInteractive)}` : "",
      nav ? `domComplete=${Math.round(nav.domComplete)}` : "",
    ]
      .filter(Boolean)
      .join(" ");
    invoke("report_ready", { detail }).catch(() => {});
  });
