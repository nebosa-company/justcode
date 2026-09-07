// The File > Open Folder tree.
//
// Mirrors the panel contract perp.js already uses — configure/mount/attach/
// detach/isOpen/redraw — so main.js drives both side panels the same way.
//
// The invariant everything else rests on: `expanded`, `selected` and
// `focusedKey` hold *path keys*, never node objects. A refresh can therefore
// replace every node in the map and none of them notice, which is what makes
// re-reading the tree non-destructive. Cache a node object in one of those sets
// and refresh starts silently losing expansion and selection, in a way that
// looks random rather than like a bug in the thing that caused it.
//
// The tree is read one level at a time. A root with `node_modules` in it makes
// eager scanning indefensible, and lazily reading a level means a symlink loop
// costs one listing per click rather than running away on its own.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import { t } from "./i18n.js";
import { iconElement } from "./icons.js";
import { showContextMenu } from "./menu.js";
import { IS_MAC } from "./shortcuts.js";
import {
  baseName,
  fileIconId,
  isInside,
  joinPath,
  nextTypeAhead,
  parentOf,
  pathKey,
  relativePath,
  sortEntries,
  uniqueName,
  validateName,
  visibleRows,
} from "./filetree.js";

/** The dataTransfer type a tree drag carries.
 *
 * Deliberately the only one it sets. A `text/plain` payload would be accepted
 * by CodeMirror, so dropping a file on the editor would paste its path into the
 * document. The tab bar's own drags are told apart by this type being absent.
 */
const DRAG_TYPE = "application/x-justcode-path";

/** How many children one folder renders before offering the rest.
 *
 * ponytail: a clamp rather than a windowing scroller. Lazy expansion already
 * bounds everything except a single enormous directory, and this removes that
 * last unbounded case in ten lines instead of two hundred. Add virtualization
 * if a real folder ever makes scrolling hurt.
 */
const ROW_CLAMP = 1000;

let host = null;
let hooks = { openPath: null, activePath: () => null, onPathRenamed: null };

let rootPath = null;
/** pathKey -> { path, name, isDir, isSymlink, hidden, ignored, children, error } */
const nodes = new Map();
const expanded = new Set();
const selected = new Set();
const clamps = new Map();
let focusedKey = null;
let anchorKey = null;
let clip = null;
let iconMap = null;
let rows = [];
let typed = { text: "", at: 0 };
let editing = null;
let watchTimer = null;

const key = pathKey;
const nodeAt = (path) => nodes.get(key(path));

/* ------------------------------------------------------------------ wiring */

export function configure(options) {
  hooks = { ...hooks, ...options };
}

export function mount(element) {
  host = element;
  render();
}

export function isOpen() {
  return Boolean(host) && !host.hidden;
}

export function root() {
  return rootPath;
}

export function contains(target) {
  return Boolean(host) && host.contains(target);
}

/** Re-render in the new language. Called from applyTranslations(). */
export function redraw() {
  render();
}

/* -------------------------------------------------------------- icon theme */

/**
 * The Material Icon Theme map, fetched once and only when a folder is first
 * opened — an editor that never opens one never pays the 218 KB.
 *
 * `new URL(..., document.baseURI)` rather than a leading-slash path: that is
 * the form spellcheck.js already proves works for `public/` assets both under
 * the dev server and inside the packaged app.
 */
async function loadIconMap() {
  if (iconMap) return iconMap;
  try {
    const response = await fetch(new URL("file-icons/map.json", document.baseURI));
    iconMap = await response.json();
  } catch {
    // A tree with no icons is still a usable tree, so this is not fatal.
    iconMap = null;
  }
  return iconMap;
}

function iconUrl(node, isExpanded) {
  if (!iconMap) return null;
  const light = document.documentElement.dataset.theme === "light";
  const id = fileIconId(iconMap, node.name, {
    isDir: node.isDir,
    expanded: isExpanded,
    parent: baseName(parentOf(node.path)),
    light,
  });
  return id ? new URL(`file-icons/${id}.svg`, document.baseURI).href : null;
}

/* ------------------------------------------------------------ reading disk */

/** Read one level and fold it into the map, sorted. */
async function readDir(path) {
  const entries = await invoke("list_dir", { root: rootPath, dir: path });
  const sorted = sortEntries(entries);
  for (const entry of sorted) {
    const existing = nodeAt(entry.path);
    nodes.set(key(entry.path), { ...entry, children: existing?.children ?? null });
  }
  const parent = nodeAt(path);
  if (parent) {
    parent.children = sorted.map((entry) => entry.path);
    parent.error = null;
  }
  return sorted;
}

/** Open a folder as the tree's root. */
export async function attach(path, { restore = [] } = {}) {
  rootPath = path;
  nodes.clear();
  expanded.clear();
  selected.clear();
  clamps.clear();
  focusedKey = null;
  anchorKey = null;
  nodes.set(key(path), {
    path,
    name: baseName(path) || path,
    isDir: true,
    isSymlink: false,
    hidden: false,
    ignored: false,
    children: null,
  });

  await loadIconMap();
  try {
    await readDir(path);
  } catch (error) {
    const node = nodeAt(path);
    if (node) node.error = String(error);
  }

  // Reopen what was open last time, parents first so each level exists before
  // its children are asked for. A folder that has since been deleted is skipped
  // rather than reported: it is ordinary for a tree to have moved on.
  for (const relative of [...restore].sort((a, b) => a.length - b.length)) {
    const target = joinPath(path, relative.replace(/\//g, "\\"));
    if (!nodeAt(target)) continue;
    try {
      await readDir(target);
      expanded.add(key(target));
    } catch {
      /* gone since last time */
    }
  }

  render();
  syncWatch();
  return rootPath;
}

export function detach() {
  rootPath = null;
  nodes.clear();
  expanded.clear();
  selected.clear();
  clamps.clear();
  focusedKey = null;
  render();
  syncWatch();
}

/**
 * Re-read the root and every expanded folder, keeping expansion, selection,
 * scroll and focus.
 *
 * All four survive because the sets hold path keys: replacing a node object
 * changes nothing they refer to. Focus is only restored when it was already
 * inside the panel — a refresh that fires while you are typing in the editor
 * must not steal the caret.
 */
export async function refresh() {
  if (!rootPath) return;
  const targets = [rootPath, ...[...expanded].map((k) => nodes.get(k)?.path).filter(Boolean)];
  for (const path of targets) {
    try {
      await readDir(path);
    } catch {
      // The folder has gone. Drop it, and let the render fall back to the
      // nearest ancestor that is still there.
      expanded.delete(key(path));
      const node = nodeAt(path);
      if (node) node.children = null;
    }
  }
  for (const set of [expanded, selected]) {
    for (const k of [...set]) if (!nodes.has(k)) set.delete(k);
  }
  if (focusedKey && !nodes.has(focusedKey)) focusedKey = null;
  render();
  syncWatch();
}

/**
 * Tell the backend which folders to watch.
 *
 * Debounced because expanding five folders in a rush should start one watcher
 * thread rather than five; the generation counter retires the losers anyway,
 * but there is no reason to make it.
 */
function syncWatch() {
  clearTimeout(watchTimer);
  watchTimer = setTimeout(() => {
    const dirs = rootPath
      ? [rootPath, ...[...expanded].map((k) => nodes.get(k)?.path).filter(Boolean)]
      : [];
    invoke("explorer_watch", { dirs }).catch(() => {});
    // The watched set changes at exactly the moments the tree's shape does, so
    // one hook covers expand, collapse, attach and refresh.
    hooks.onChanged?.();
  }, 200);
}

listen("explorer:changed", () => {
  // The event names which folders moved, but a re-read of the expanded set is
  // idempotent and cheap, and acting on the whole set avoids a second code path
  // that only runs when someone else touches the disk — which is exactly the
  // path that would rot untested.
  if (rootPath && !editing) refresh();
}).catch(() => {});

/* --------------------------------------------------------------- rendering */

function selectionPaths() {
  return [...selected].map((k) => nodes.get(k)?.path).filter(Boolean);
}

/** The folder a New File / Paste should land in, given what is selected. */
function targetFolder() {
  if (!rootPath) return null;
  const node = selected.size === 1 ? nodes.get([...selected][0]) : null;
  if (!node) return rootPath;
  return node.isDir ? node.path : parentOf(node.path);
}

function render() {
  if (!host) return;
  const hadFocus = host.contains(document.activeElement);
  const scroller = host.querySelector(".explorer-tree");
  const scrollTop = scroller ? scroller.scrollTop : 0;

  host.textContent = "";
  if (!rootPath) {
    host.append(emptyState());
    return;
  }

  host.append(header());

  const tree = document.createElement("div");
  tree.className = "explorer-tree";
  tree.setAttribute("role", "tree");
  tree.setAttribute("aria-multiselectable", "true");
  tree.setAttribute("aria-label", t("explorer.tree"));
  tree.tabIndex = -1;

  rows = visibleRows(rootPath, nodes, expanded, { clamp: ROW_CLAMP, clamps });
  if (focusedKey && !rows.some((row) => row.key === focusedKey)) focusedKey = null;
  if (!focusedKey && rows.length) focusedKey = rows[0].key;

  const rootNode = nodeAt(rootPath);
  if (rootNode?.error) {
    tree.append(notice(t("explorer.readError", { path: rootPath })));
  } else if (!rows.length) {
    tree.append(notice(t("explorer.folderEmpty")));
  }
  for (const row of rows) tree.append(row.more ? moreRow(row) : rowElement(row));

  host.append(tree);
  tree.scrollTop = scrollTop;
  if (hadFocus) focusRow(focusedKey, { scroll: false });
  paintActive();
}

function notice(text) {
  const el = document.createElement("p");
  el.className = "explorer-notice";
  el.textContent = text;
  return el;
}

function emptyState() {
  const box = document.createElement("div");
  box.className = "explorer-empty";
  const title = document.createElement("p");
  title.className = "explorer-empty-title";
  title.textContent = t("explorer.empty");
  const hint = document.createElement("p");
  hint.textContent = t("explorer.emptyHint");
  const button = document.createElement("button");
  button.type = "button";
  button.className = "explorer-open";
  button.textContent = t("explorer.openFolder");
  button.addEventListener("click", () => hooks.openFolder?.());
  box.append(title, hint, button);
  return box;
}

function header() {
  const bar = document.createElement("div");
  bar.className = "explorer-header";

  const title = document.createElement("span");
  title.className = "explorer-title";
  title.textContent = baseName(rootPath) || rootPath;
  title.title = rootPath;
  bar.append(title);

  const spacer = document.createElement("span");
  spacer.className = "spacer";
  bar.append(spacer);

  const action = (icon, label, run) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "explorer-action";
    button.title = label;
    button.setAttribute("aria-label", label);
    button.append(iconElement(icon));
    button.addEventListener("click", run);
    return button;
  };
  bar.append(
    action("file", t("explorer.newFile"), () => beginCreate(false)),
    action("folder", t("explorer.newFolder"), () => beginCreate(true)),
    action("refresh", t("explorer.refresh"), () => refresh()),
    action("chevronDown", t("explorer.collapseAll"), collapseAll)
  );
  return bar;
}

function rowElement(row) {
  const node = row.node;
  const isExpanded = node.isDir && expanded.has(row.key);

  const el = document.createElement("div");
  el.className = "explorer-row";
  el.dataset.key = row.key;
  el.setAttribute("role", "treeitem");
  el.style.setProperty("--depth", String(row.depth));
  el.setAttribute("aria-level", String(row.depth + 1));
  el.setAttribute("aria-posinset", String(row.index + 1));
  el.setAttribute("aria-setsize", String(row.setSize));
  el.setAttribute("aria-selected", String(selected.has(row.key)));
  // Only a directory carries aria-expanded: a file that has it is announced as
  // a collapsible node that never opens.
  if (node.isDir) el.setAttribute("aria-expanded", String(isExpanded));
  el.tabIndex = row.key === focusedKey ? 0 : -1;
  el.draggable = true;
  if (node.hidden || node.ignored) {
    el.classList.add("dim");
    el.title = node.ignored ? t("explorer.ignoredFile") : t("explorer.hiddenFile");
  }
  if (clip?.mode === "cut" && clip.keys.has(row.key)) el.classList.add("cut");

  const body = document.createElement("span");
  body.className = "explorer-body";

  const twisty = document.createElement("span");
  twisty.className = "explorer-twisty";
  if (node.isDir) twisty.append(iconElement(isExpanded ? "chevronDown" : "chevronRight"));
  body.append(twisty);

  const url = iconUrl(node, isExpanded);
  if (url) {
    const img = document.createElement("img");
    img.className = "explorer-icon";
    img.src = url;
    img.width = 16;
    img.height = 16;
    img.alt = "";
    img.decoding = "async";
    img.draggable = false;
    body.append(img);
  }

  if (editing && editing.key === row.key && editing.mode === "rename") {
    body.append(nameInput(node.name, row));
  } else {
    const label = document.createElement("span");
    label.className = "explorer-label";
    label.textContent = node.name;
    body.append(label);
  }

  el.append(body);
  wireRow(el, row);
  return el;
}

function moreRow(row) {
  const el = document.createElement("button");
  el.type = "button";
  el.className = "explorer-row explorer-more";
  el.style.setProperty("--depth", String(row.depth));
  el.textContent = t("explorer.more", { n: row.more });
  el.addEventListener("click", () => {
    clamps.set(key(row.path), (clamps.get(key(row.path)) ?? ROW_CLAMP) + ROW_CLAMP);
    render();
  });
  return el;
}

/** Highlight the row showing the file that is active in the editor. */
export function paintActive() {
  if (!host) return;
  const active = hooks.activePath?.();
  const wanted = active ? key(active) : null;
  for (const el of host.querySelectorAll(".explorer-row")) {
    el.classList.toggle("active", Boolean(wanted) && el.dataset.key === wanted);
  }
}

/* ------------------------------------------------------ selection and keys */

function setSelection(keys, { anchor = null } = {}) {
  selected.clear();
  for (const k of keys) selected.add(k);
  if (anchor) anchorKey = anchor;
  for (const el of host.querySelectorAll(".explorer-row")) {
    el.setAttribute("aria-selected", String(selected.has(el.dataset.key)));
  }
}

function focusRow(k, { scroll = true } = {}) {
  if (!k) return;
  focusedKey = k;
  for (const el of host.querySelectorAll(".explorer-row")) {
    el.tabIndex = el.dataset.key === k ? 0 : -1;
  }
  const el = host.querySelector(`.explorer-row[data-key="${cssEscape(k)}"]`);
  if (!el) return;
  el.focus({ preventScroll: !scroll });
  if (scroll) el.scrollIntoView({ block: "nearest" });
}

const cssEscape = (value) =>
  window.CSS?.escape ? window.CSS.escape(value) : String(value).replace(/["\\]/g, "\\$&");

function rowIndex(k) {
  return rows.findIndex((row) => row.key === k);
}

async function toggle(row, force = null) {
  const node = row.node;
  if (!node?.isDir) return;
  const open = force ?? !expanded.has(row.key);
  if (open) {
    if (!node.children) {
      try {
        await readDir(node.path);
      } catch (error) {
        node.error = String(error);
      }
    }
    expanded.add(row.key);
  } else {
    expanded.delete(row.key);
  }
  render();
  focusRow(row.key);
  syncWatch();
}

function collapseAll() {
  expanded.clear();
  render();
  syncWatch();
}

function wireRow(el, row) {
  el.addEventListener("mousedown", (event) => {
    // Right-click on a row outside the selection replaces it, so a context
    // menu can never act on rows that are not the one under the pointer.
    if (event.button === 2) {
      if (!selected.has(row.key)) setSelection([row.key], { anchor: row.key });
      focusRow(row.key);
      return;
    }
    if (event.button !== 0) return;
    if (event.ctrlKey || event.metaKey) {
      if (selected.has(row.key)) selected.delete(row.key);
      else selected.add(row.key);
      setSelection([...selected], { anchor: row.key });
    } else if (event.shiftKey && anchorKey) {
      const from = rowIndex(anchorKey);
      const to = rowIndex(row.key);
      if (from >= 0 && to >= 0) {
        const [lo, hi] = from < to ? [from, to] : [to, from];
        setSelection(rows.slice(lo, hi + 1).map((r) => r.key));
      }
    } else {
      setSelection([row.key], { anchor: row.key });
    }
    focusRow(row.key);
  });

  el.addEventListener("click", (event) => {
    if (event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (row.node.isDir) toggle(row);
    else hooks.openPath?.(row.node.path);
  });

  el.addEventListener("dragstart", (event) => {
    const dragging = selected.has(row.key) ? selectionPaths() : [row.node.path];
    if (!selected.has(row.key)) setSelection([row.key], { anchor: row.key });
    event.dataTransfer.effectAllowed = "copyMove";
    event.dataTransfer.setData(DRAG_TYPE, dragging.join("\n"));
  });

  el.addEventListener("dragover", (event) => onDragOver(event, row));
  el.addEventListener("drop", (event) => onDrop(event, row));
}

/* ------------------------------------------------------- drag against drop */

const copyModifier = (event) => (IS_MAC ? event.altKey : event.ctrlKey);

/** Where a drop on this row lands: a folder takes it, a file's parent does. */
function dropFolder(row) {
  if (!row) return rootPath;
  return row.node.isDir ? row.node.path : parentOf(row.node.path);
}

function clearDropMark() {
  for (const el of host.querySelectorAll(".drop-into")) el.classList.remove("drop-into");
}

function onDragOver(event, row) {
  // A tab drag carries no such type. Not calling preventDefault leaves it to
  // the tab bar, and leaves an illegal target showing the no-entry cursor
  // rather than needing an error path of its own.
  if (!event.dataTransfer.types.includes(DRAG_TYPE)) return;
  const folder = dropFolder(row);
  if (!folder) return;
  const dragged = (event.dataTransfer.getData(DRAG_TYPE) || "").split("\n").filter(Boolean);
  // getData is empty during dragover in most engines, so the authoritative
  // check happens on drop; this only suppresses the obvious self-drop.
  if (dragged.some((path) => key(path) === key(folder) || isInside(path, folder))) return;

  event.preventDefault();
  event.dataTransfer.dropEffect = copyModifier(event) ? "copy" : "move";
  clearDropMark();
  const target = row && row.node.isDir ? row.key : null;
  const el = target
    ? host.querySelector(`.explorer-row[data-key="${cssEscape(target)}"]`)
    : host.querySelector(".explorer-tree");
  el?.classList.add("drop-into");
}

async function onDrop(event, row) {
  if (!event.dataTransfer.types.includes(DRAG_TYPE)) return;
  event.preventDefault();
  clearDropMark();
  const folder = dropFolder(row);
  const paths = (event.dataTransfer.getData(DRAG_TYPE) || "").split("\n").filter(Boolean);
  if (!folder || !paths.length) return;
  await transfer(paths, folder, copyModifier(event) ? "copy" : "cut");
}

/**
 * Move or copy `paths` into `folder`. Shared by drag-and-drop and by Paste, so
 * the two cannot drift on collisions or on the descendant guard.
 */
async function transfer(paths, folder, mode) {
  const siblings = (nodeAt(folder)?.children ?? []).map((path) => baseName(path));
  const landed = [];
  for (const from of paths) {
    if (key(from) === key(folder) || isInside(from, folder)) continue;
    const name = baseName(from);
    // A copy invents a free name the way VS Code does; a move asks, because
    // silently renaming a file someone dragged is not what they meant.
    let target = joinPath(folder, name);
    if (nodeAt(target) && key(parentOf(from)) !== key(folder)) {
      if (mode === "copy") {
        target = joinPath(folder, uniqueName(name, siblings));
      } else if (!(await ask(t("explorer.overwriteConfirm", { name }), { title: t("explorer.title") }))) {
        continue;
      }
    } else if (nodeAt(target) && mode === "copy") {
      target = joinPath(folder, uniqueName(name, siblings));
    }
    try {
      await invoke(mode === "copy" ? "copy_entry" : "rename_entry", {
        root: rootPath,
        from,
        to: target,
      });
      if (mode !== "copy") hooks.onPathRenamed?.(from, target);
      siblings.push(baseName(target));
      landed.push(key(target));
    } catch (error) {
      await message(`${t(mode === "copy" ? "explorer.copyFailed" : "explorer.moveFailed", { name })}\n\n${error}`, {
        title: t("explorer.title"),
        kind: "error",
      });
    }
  }
  if (clip?.mode === "cut") clip = null;
  await refresh();
  if (landed.length) setSelection(landed, { anchor: landed[0] });
}

/* -------------------------------------------------- inline create / rename */

function nameInput(initial, row) {
  const input = document.createElement("input");
  input.className = "explorer-input";
  input.type = "text";
  input.value = initial;
  input.spellcheck = false;
  input.setAttribute("aria-label", t("explorer.rename"));

  const error = document.createElement("span");
  error.className = "explorer-input-error";
  error.id = `explorer-error-${Math.random().toString(36).slice(2)}`;
  error.hidden = true;
  input.setAttribute("aria-describedby", error.id);

  const siblings = editing.siblings;
  const check = () => {
    const problem = validateName(input.value, siblings, { self: editing.self });
    input.classList.toggle("invalid", Boolean(problem));
    input.setAttribute("aria-invalid", String(Boolean(problem)));
    error.hidden = !problem;
    if (problem) error.textContent = t(problem.key, problem.vars);
    return !problem;
  };

  input.addEventListener("input", check);
  input.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Enter") {
      event.preventDefault();
      if (check()) commitEdit(input.value);
    } else if (event.key === "Escape") {
      event.preventDefault();
      cancelEdit();
    }
  });
  // Committing on blur matches VS Code; cancelling an invalid one rather than
  // holding focus avoids a trap you cannot click out of.
  input.addEventListener("blur", () => {
    if (!editing) return;
    if (validateName(input.value, siblings, { self: editing.self })) cancelEdit();
    else commitEdit(input.value);
  });

  queueMicrotask(() => {
    input.focus();
    // Select the stem only, so renaming report-final.md is two keystrokes.
    const dot = initial.lastIndexOf(".");
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);
  });

  const wrap = document.createDocumentFragment();
  wrap.append(input, error);
  return wrap;
}

function siblingNames(folder) {
  return (nodeAt(folder)?.children ?? []).map((path) => baseName(path));
}

export function beginRename() {
  if (selected.size !== 1) return;
  const node = nodes.get([...selected][0]);
  if (!node || key(node.path) === key(rootPath)) return;
  editing = {
    mode: "rename",
    key: key(node.path),
    folder: parentOf(node.path),
    self: node.name,
    siblings: siblingNames(parentOf(node.path)),
  };
  render();
}

export async function beginCreate(directory) {
  const folder = targetFolder();
  if (!folder) return;
  if (key(folder) !== key(rootPath) && !expanded.has(key(folder))) {
    await toggle({ key: key(folder), node: nodeAt(folder) }, true);
  }
  editing = {
    mode: "create",
    directory,
    folder,
    self: null,
    siblings: siblingNames(folder),
  };
  render();
  const tree = host.querySelector(".explorer-tree");
  const placeholder = document.createElement("div");
  placeholder.className = "explorer-row explorer-new";
  placeholder.style.setProperty("--depth", String(depthOf(folder) + 1));
  const body = document.createElement("span");
  body.className = "explorer-body";
  const twisty = document.createElement("span");
  twisty.className = "explorer-twisty";
  body.append(twisty, nameInput("", null));
  placeholder.append(body);
  const anchor = host.querySelector(`.explorer-row[data-key="${cssEscape(key(folder))}"]`);
  if (anchor) anchor.after(placeholder);
  else tree?.prepend(placeholder);
  placeholder.scrollIntoView({ block: "nearest" });
}

function depthOf(path) {
  const row = rows.find((r) => r.key === key(path));
  return row ? row.depth : -1;
}

function cancelEdit() {
  editing = null;
  render();
}

async function commitEdit(value) {
  const state = editing;
  editing = null;
  if (!state) return;
  const name = value.trim();
  try {
    if (state.mode === "create") {
      const created = await invoke("create_entry", {
        root: rootPath,
        dir: state.folder,
        name,
        directory: state.directory,
      });
      await refresh();
      setSelection([key(created)], { anchor: key(created) });
      focusRow(key(created));
      return;
    }
    const from = nodes.get(state.key)?.path;
    const to = joinPath(state.folder, name);
    if (!from || key(from) === key(to)) return void render();
    await invoke("rename_entry", { root: rootPath, from, to });
    // The tab showing this file has to learn its new path, or Save writes to a
    // name that no longer exists.
    hooks.onPathRenamed?.(from, to);
    await refresh();
    setSelection([key(to)], { anchor: key(to) });
    focusRow(key(to));
  } catch (error) {
    await message(
      `${t(state.mode === "create" ? "explorer.createFailed" : "explorer.renameFailed", { name })}\n\n${error}`,
      { title: t("explorer.title"), kind: "error" }
    );
    render();
  }
}

/* ------------------------------------------------------------- operations */

export async function deleteSelection() {
  const paths = selectionPaths().filter((path) => key(path) !== key(rootPath));
  if (!paths.length) return;
  const bin = t(IS_MAC ? "explorer.binMac" : navigator.userAgent.includes("Windows") ? "explorer.binWindows" : "explorer.binLinux");
  const question =
    paths.length === 1
      ? t("explorer.deleteConfirm", { name: baseName(paths[0]), bin })
      : t("explorer.deleteConfirmMany", { n: paths.length, bin });
  if (!(await ask(question, { title: t("explorer.delete"), kind: "warning" }))) return;
  try {
    await invoke("delete_entry", { root: rootPath, paths });
  } catch (error) {
    await message(`${t("explorer.deleteFailed", { name: baseName(paths[0]) })}\n\n${error}`, {
      title: t("explorer.title"),
      kind: "error",
    });
  }
  await refresh();
}

async function duplicate() {
  if (selected.size !== 1) return;
  const node = nodes.get([...selected][0]);
  if (!node) return;
  const folder = parentOf(node.path);
  const target = joinPath(folder, uniqueName(node.name, siblingNames(folder)));
  try {
    await invoke("copy_entry", { root: rootPath, from: node.path, to: target });
  } catch (error) {
    await message(`${t("explorer.copyFailed", { name: node.name })}\n\n${error}`, {
      title: t("explorer.title"),
      kind: "error",
    });
  }
  await refresh();
}

function setClip(mode) {
  if (!selected.size) return;
  clip = { mode, paths: selectionPaths(), keys: new Set(selected) };
  render();
}

async function paste() {
  const folder = targetFolder();
  if (!clip || !folder) return;
  await transfer(clip.paths, folder, clip.mode);
}

/* ------------------------------------------------------------ context menu */

export function contextMenu(event) {
  if (!rootPath) return;
  event.preventDefault();
  const one = () => selected.size === 1;
  const any = () => selected.size > 0;
  const single = () => nodes.get([...selected][0]);
  const revealKey = IS_MAC
    ? "file.revealMac"
    : navigator.userAgent.includes("Windows")
      ? "file.revealWindows"
      : "file.revealLinux";

  showContextMenu(event.clientX, event.clientY, [
    { label: t("explorer.newFile"), icon: "file", enabled: () => Boolean(targetFolder()), run: () => beginCreate(false) },
    { label: t("explorer.newFolder"), icon: "folder", enabled: () => Boolean(targetFolder()), run: () => beginCreate(true) },
    { separator: true },
    { label: t("edit.cut"), icon: "cut", accel: "Ctrl+X", enabled: any, run: () => setClip("cut") },
    { label: t("edit.copy"), icon: "copy", accel: "Ctrl+C", enabled: any, run: () => setClip("copy") },
    {
      label: t("edit.paste"),
      icon: "paste",
      accel: "Ctrl+V",
      enabled: () => Boolean(clip) && Boolean(targetFolder()),
      run: paste,
    },
    { separator: true },
    {
      label: t("explorer.copyPath"),
      icon: "copy",
      enabled: one,
      run: () => clipboardWriteText(single()?.path ?? ""),
    },
    {
      label: t("explorer.copyRelativePath"),
      icon: "copy",
      enabled: one,
      run: () => clipboardWriteText(relativePath(rootPath, single()?.path ?? "")),
    },
    { separator: true },
    { label: t("explorer.rename"), icon: "comment", accel: "F2", enabled: one, run: beginRename },
    { label: t("explorer.duplicate"), icon: "copy", enabled: one, run: duplicate },
    { label: t("explorer.delete"), icon: "deleteLine", accel: "Del", enabled: any, run: deleteSelection },
    { separator: true },
    {
      label: t(revealKey),
      icon: "folder",
      enabled: one,
      run: () => invoke("reveal_in_file_manager", { path: single()?.path }).catch(() => {}),
    },
    {
      label: t("explorer.openInTerminal"),
      icon: "terminal",
      enabled: () => Boolean(targetFolder()),
      run: () => hooks.openInTerminal?.(targetFolder()),
    },
    { separator: true },
    { label: t("explorer.refresh"), icon: "refresh", run: () => refresh() },
    { label: t("explorer.collapseAll"), icon: "chevronDown", enabled: () => expanded.size > 0, run: collapseAll },
  ]);
}

/* ------------------------------------------------------------- keyboard */

export function handleKey(event) {
  if (!rootPath || editing) return false;
  const index = rowIndex(focusedKey);
  const row = rows[index];
  const move = (to) => {
    const next = rows[Math.max(0, Math.min(to, rows.length - 1))];
    if (!next) return;
    if (event.shiftKey && anchorKey) {
      const from = rowIndex(anchorKey);
      const [lo, hi] = from < rowIndex(next.key) ? [from, rowIndex(next.key)] : [rowIndex(next.key), from];
      setSelection(rows.slice(lo, hi + 1).map((r) => r.key));
    } else {
      setSelection([next.key], { anchor: next.key });
    }
    focusRow(next.key);
  };

  switch (event.key) {
    case "ArrowDown":
      move(index + 1);
      return true;
    case "ArrowUp":
      move(index - 1);
      return true;
    case "ArrowRight":
      if (row?.node?.isDir && !expanded.has(row.key)) toggle(row, true);
      else if (row?.node?.isDir) move(index + 1);
      return true;
    case "ArrowLeft":
      if (row?.node?.isDir && expanded.has(row.key)) toggle(row, false);
      else if (row) {
        const parent = rows.find((r) => r.key === key(parentOf(row.node.path)));
        if (parent) {
          setSelection([parent.key], { anchor: parent.key });
          focusRow(parent.key);
        }
      }
      return true;
    case "Home":
      move(0);
      return true;
    case "End":
      move(rows.length - 1);
      return true;
    case "Enter":
      if (row?.node?.isDir) toggle(row);
      else if (row) hooks.openPath?.(row.node.path);
      return true;
    case " ":
      if (row) {
        if (selected.has(row.key)) selected.delete(row.key);
        else selected.add(row.key);
        setSelection([...selected], { anchor: row.key });
      }
      return true;
    case "F2":
      beginRename();
      return true;
    case "Delete":
      deleteSelection();
      return true;
    default:
      break;
  }

  if (event.ctrlKey || event.metaKey) {
    const letter = event.key.toLowerCase();
    if (letter === "x") return setClip("cut"), true;
    if (letter === "c") return setClip("copy"), true;
    if (letter === "v") return paste(), true;
    if (letter === "a") {
      setSelection(rows.filter((r) => !r.more).map((r) => r.key));
      return true;
    }
    return false;
  }

  // Type-ahead. One printable character, buffered for half a second so typing
  // "re" lands on "readme" rather than on the next thing starting with "e".
  if (event.key.length === 1 && !event.altKey) {
    const now = Date.now();
    typed = { text: now - typed.at < 500 ? typed.text + event.key : event.key, at: now };
    const found = nextTypeAhead(rows, typed.text.length > 1 ? index - 1 : index, typed.text);
    if (found >= 0) {
      setSelection([rows[found].key], { anchor: rows[found].key });
      focusRow(rows[found].key);
    }
    return true;
  }
  return false;
}

/** What to persist: the root, and which folders are open, relative to it. */
export function state() {
  if (!rootPath) return null;
  return {
    version: 1,
    root: rootPath,
    expanded: [...expanded]
      .map((k) => nodes.get(k)?.path)
      .filter(Boolean)
      .map((path) => relativePath(rootPath, path).replace(/\\/g, "/"))
      .filter(Boolean)
      .slice(0, 200),
  };
}
