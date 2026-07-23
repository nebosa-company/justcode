import { appLogoElement, iconElement, iconMarkup } from "./icons.js";
import { LOCALES, t } from "./i18n.js";

/**
 * Shortcut reference. The "native" groups are CodeMirror's own bindings, which
 * are not defined anywhere in this app's code — they come from its default
 * keymap — so they are listed here by hand to make them discoverable.
 *
 * Both columns are translation keys, except literal key names ("Ctrl+N", "F5"),
 * which are identical on every keyboard and are printed as-is. A left-hand
 * entry beginning with `sc.k.` describes a gesture in prose and is translated.
 */
const SHORTCUTS = [
  {
    group: "sc.g.menus",
    items: [
      ["Alt+F", "sc.openFileMenu"],
      ["Alt+E", "sc.openEditMenu"],
      ["Alt+V", "sc.openViewMenu"],
      ["Alt+H", "sc.openHelpMenu"],
    ],
  },
  {
    group: "sc.g.file",
    items: [
      ["Ctrl+N", "sc.newFile"],
      ["Ctrl+O", "sc.openFile"],
      ["Ctrl+S", "sc.save"],
      ["Ctrl+Shift+S", "sc.saveAs"],
      ["Ctrl+Alt+S", "sc.saveAll"],
      ["F5", "sc.run"],
      ["Ctrl+W", "sc.closeTab"],
      ["Ctrl+Shift+W", "sc.closeAll"],
      ["Alt+F4", "sc.exit"],
    ],
  },
  {
    group: "sc.g.editing",
    items: [
      ["Ctrl+X / Ctrl+C / Ctrl+V", "sc.cutCopyPaste"],
      ["Ctrl+A", "sc.selectAll"],
      ["Ctrl+Z", "sc.undo"],
      ["Ctrl+Y / Ctrl+Shift+Z", "sc.redo"],
      ["sc.k.rightClick", "sc.contextMenu"],
      ["Ctrl+U / Alt+U", "sc.undoSelection"],
      ["Ctrl+Shift+K", "sc.deleteLine"],
      ["Ctrl+/", "sc.toggleComment"],
      ["Ctrl+Shift+U / Ctrl+Shift+L", "sc.upperLower"],
      ["Ctrl+Alt+G", "sc.guid"],
      ["Tab / Shift+Tab", "sc.indentOutdent"],
      ["Ctrl+] / Ctrl+[", "sc.indentMoreLess"],
      ["Enter", "sc.newLineIndent"],
      ["Ctrl+Backspace / Ctrl+Delete", "sc.deleteWord"],
      ["Alt+↑ / Alt+↓", "sc.moveLine"],
      ["Alt+Shift+↑ / Alt+Shift+↓", "sc.copyLine"],
      ["Ctrl+Space", "sc.completion"],
    ],
  },
  {
    group: "sc.g.bookmarks",
    items: [
      ["Ctrl+Shift+1 / 2 / 3", "sc.toggleBookmark"],
      ["Ctrl+1 / 2 / 3", "sc.gotoBookmark"],
    ],
  },
  {
    group: "sc.g.search",
    items: [
      ["Ctrl+F", "sc.find"],
      ["Ctrl+H", "sc.replace"],
      ["Enter / Shift+Enter", "sc.nextPrevMatch"],
      ["F3 / Shift+F3", "sc.nextPrevMatch"],
      ["Ctrl+G", "sc.nextMatchBar"],
      ["Escape", "sc.closeFindBar"],
    ],
  },
  {
    group: "sc.g.moving",
    items: [
      ["← → ↑ ↓", "sc.byCharLine"],
      ["Ctrl+← / Ctrl+→", "sc.byWord"],
      ["Home / End", "sc.lineStartEnd"],
      ["Ctrl+Home / Ctrl+End", "sc.docStartEnd"],
      ["PageUp / PageDown", "sc.byPage"],
      ["Ctrl+Shift+\\", "sc.matchingBracket"],
    ],
  },
  {
    group: "sc.g.selecting",
    items: [
      ["sc.k.shiftMotion", "sc.extendSelection"],
      ["Ctrl+Shift+← / →", "sc.extendByWord"],
      ["Shift+Home / Shift+End", "sc.selectToLineEdge"],
      ["Shift+PageUp / Shift+PageDown", "sc.selectByPage"],
      ["Ctrl+L", "sc.selectLine"],
      ["Ctrl+I", "sc.growSelection"],
      ["Escape", "sc.collapseCursor"],
      ["sc.k.dblTripleClick", "sc.selectWordLine"],
      ["sc.k.altClick", "sc.addCursor"],
      ["sc.k.altDrag", "sc.rectSelection"],
    ],
  },
  {
    group: "sc.g.folding",
    items: [
      ["Ctrl+Shift+[", "sc.fold"],
      ["Ctrl+Shift+]", "sc.unfold"],
      ["Ctrl+Alt+[", "sc.foldAll"],
      ["Ctrl+Alt+]", "sc.unfoldAll"],
      ["sc.k.gutterChevron", "sc.foldUnfold"],
    ],
  },
  {
    group: "sc.g.view",
    items: [
      ["Ctrl++ / Ctrl+-", "sc.zoomInOut"],
      ["Ctrl+0", "sc.resetZoom"],
      ["sc.k.ctrlWheel", "sc.zoom"],
      ["F8", "sc.showProblems"],
      ["Ctrl+Shift+M", "sc.problemsPanel"],
      ["Ctrl+Shift+G", "sc.goToSymbol"],
      ["Alt+T", "sc.toolbar"],
      ["Alt+S", "sc.statusBar"],
      ["Alt+Z", "sc.wordWrap"],
      ["F7", "sc.spellCheck"],
      ["Ctrl+Shift+B", "sc.bionicReading"],
      ["Ctrl+Shift+T", "sc.cycleTheme"],
      ["Ctrl+`", "sc.terminal"],
      ["Ctrl+Shift+`", "terminal.new"],
      ["Ctrl+F5", "file.runInTerminal"],
      ["F1", "sc.shortcutList"],
    ],
  },
  {
    group: "sc.g.tabs",
    items: [
      ["Ctrl+Tab / Ctrl+Shift+Tab", "sc.nextPrevTab"],
      ["Ctrl+PageDown / Ctrl+PageUp", "sc.nextPrevTab"],
      ["Ctrl+Shift+PageDown / PageUp", "sc.moveTab"],
      ["sc.k.middleClickTab", "sc.closeIt"],
      ["Ctrl+K ↑ / ↓ / ← / →", "sc.split"],
      ["sc.k.dragToEdge", "sc.splitView"],
      ["sc.k.dragToTabBar", "sc.moveOrUnsplit"],
      ["sc.k.dblClickTabBar", "sc.newFile"],
    ],
  },
  {
    group: "sc.g.links",
    items: [
      ["sc.k.ctrlClickUrl", "sc.openInBrowser"],
      ["Ctrl+Enter", "sc.openLinkAtCaret"],
      ["sc.k.clickPath", "sc.copyFullPath"],
      ["sc.k.clickLanguage", "sc.changeMode"],
    ],
  },
];

let overlay = null;
// Run when the overlay goes away by any route — button, Escape or backdrop —
// so a dialog that answers a question always settles its promise.
let onOverlayClosed = null;
// Whatever had focus before the dialog opened, so closing it does not dump the
// user on document.body and leave the editor unable to receive keystrokes.
let focusBeforeOverlay = null;

function closeOverlay() {
  if (!overlay) return;
  overlay.remove();
  overlay = null;
  document.removeEventListener("keydown", onOverlayKey, true);
  // Un-inerts the app behind the dialog — see the note in openOverlay.
  const app = document.getElementById("app");
  if (app) app.inert = false;

  const restore = focusBeforeOverlay;
  focusBeforeOverlay = null;
  if (restore && restore.isConnected) restore.focus({ preventScroll: true });

  const finish = onOverlayClosed;
  onOverlayClosed = null;
  finish?.();
}

function onOverlayKey(event) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    closeOverlay();
  }
}

/** Whether a modal is on screen — the app's global shortcuts stand down then. */
export function isOverlayOpen() {
  return overlay !== null;
}

/** Builds the shared modal shell and returns its body for the caller to fill. */
function openOverlay(title, wide, onClosed) {
  const previous = document.activeElement;
  closeOverlay();
  // Captured after closing any previous dialog, so a dialog that replaces
  // another still restores the element from before the *first* one.
  focusBeforeOverlay = previous && previous !== document.body ? previous : focusBeforeOverlay;
  onOverlayClosed = onClosed ?? null;
  overlay = document.createElement("div");
  overlay.className = "modal-backdrop";
  overlay.addEventListener("mousedown", (event) => {
    if (event.target === overlay) closeOverlay();
  });

  const dialog = document.createElement("div");
  dialog.className = `modal${wide ? " modal-wide" : ""}`;
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");

  const header = document.createElement("div");
  header.className = "modal-header";
  const heading = document.createElement("h2");
  heading.id = "modal-heading";
  heading.textContent = title;
  dialog.setAttribute("aria-labelledby", heading.id);
  const close = document.createElement("button");
  close.className = "modal-close";
  close.type = "button";
  close.innerHTML = iconMarkup("close");
  close.title = t("modal.close");
  close.setAttribute("aria-label", t("modal.close"));
  close.addEventListener("click", closeOverlay);
  header.append(heading, close);

  const body = document.createElement("div");
  body.className = "modal-body";

  dialog.append(header, body);
  overlay.append(dialog);
  document.body.append(overlay);
  document.addEventListener("keydown", onOverlayKey, true);
  // Only one dialog is ever open at a time (openOverlay always closes the
  // previous one first), so the rest of the app can be made inert rather
  // than just visually covered — Tab and a screen reader's virtual cursor
  // can no longer wander into it while this is up, and it is undone in
  // closeOverlay.
  const app = document.getElementById("app");
  if (app) app.inert = true;
  close.focus();
  return body;
}

/**
 * The interface-language chooser. Each language is written in its own script,
 * since "German" is no help to someone who only reads Deutsch. Flags are
 * deliberately not used: Windows has no flag emoji glyphs (they render as bare
 * letter pairs), and a language is not a country in any case.
 */
export function showLanguageDialog(current, onPick) {
  const body = openOverlay(t("modal.languageTitle"), true);
  const grid = document.createElement("div");
  grid.className = "language-grid";
  for (const locale of LOCALES) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `language-option${locale.code === current ? " current" : ""}`;
    if (locale.rtl) button.dir = "rtl";

    const code = document.createElement("span");
    code.className = "language-code";
    code.textContent = locale.code.toUpperCase();

    const name = document.createElement("span");
    name.className = "language-name";
    name.textContent = locale.name;

    button.append(code, name);
    button.addEventListener("click", () => {
      closeOverlay();
      onPick(locale.code);
    });
    grid.append(button);
  }
  body.append(grid);
}

/**
 * The go-to-symbol list: a filter box over the declarations in the file, with
 * the result count underneath. Enter (or a click) jumps to the definition.
 */
export function showSymbolPicker(symbols, onPick) {
  const body = openOverlay(t("modal.symbolsTitle"), true);
  body.classList.add("symbol-body");

  const filter = document.createElement("input");
  filter.className = "symbol-filter";
  filter.type = "text";
  filter.placeholder = t("modal.symbolFilter");
  filter.spellcheck = false;

  const list = document.createElement("div");
  list.className = "symbol-list";

  const count = document.createElement("div");
  count.className = "symbol-count";

  body.append(filter, list, count);

  let matches = symbols;
  let highlighted = 0;
  // Set by a genuine pointer movement; cleared whenever the keyboard drives.
  let pointerMoved = false;
  list.addEventListener("mousemove", () => {
    pointerMoved = true;
  });

  const render = () => {
    const query = filter.value.trim().toLowerCase();
    matches = query
      ? symbols.filter((symbol) => symbol.name.toLowerCase().includes(query))
      : symbols;
    if (highlighted >= matches.length) highlighted = Math.max(0, matches.length - 1);

    list.textContent = "";
    matches.forEach((symbol, index) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className = `symbol-row${index === highlighted ? " highlighted" : ""}`;

      const name = document.createElement("span");
      name.className = "symbol-name";
      // Split on the last dot so a qualified name shows its owner in grey.
      const dot = symbol.name.lastIndexOf(".");
      if (dot > 0) {
        const owner = document.createElement("span");
        owner.className = "symbol-owner";
        owner.textContent = `${symbol.name.slice(0, dot)}.`;
        const own = document.createElement("b");
        own.textContent = symbol.name.slice(dot + 1);
        name.append(owner, own);
      } else {
        const own = document.createElement("b");
        own.textContent = symbol.name;
        name.append(own);
      }

      const detail = document.createElement("span");
      detail.className = "symbol-detail";
      detail.textContent = symbol.detail || "";

      row.append(name, detail);
      row.addEventListener("mouseenter", () => {
        // Scrolling the list fires mouseenter on whatever row slides under a
        // resting pointer, which would yank the highlight back from the keys.
        if (!pointerMoved) return;
        highlighted = index;
        for (const el of list.children) el.classList.remove("highlighted");
        row.classList.add("highlighted");
      });
      row.addEventListener("click", () => {
        closeOverlay();
        onPick(symbol);
      });
      list.append(row);
    });
    count.textContent = t("modal.symbolCount", { n: matches.length });
  };

  const move = (delta) => {
    if (!matches.length) return;
    pointerMoved = false;
    highlighted = Math.min(Math.max(highlighted + delta, 0), matches.length - 1);
    // Update in place rather than re-rendering: rebuilding every row on each
    // keypress is what made the pointer race the keyboard in the first place.
    [...list.children].forEach((el, index) =>
      el.classList.toggle("highlighted", index === highlighted),
    );
    list.children[highlighted]?.scrollIntoView({ block: "nearest" });
  };

  filter.addEventListener("input", () => {
    highlighted = 0;
    render();
  });
  filter.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (matches[highlighted]) {
        closeOverlay();
        onPick(matches[highlighted]);
      }
    }
  });

  render();
  filter.focus();
}

/**
 * The prompt shown when closing with unsaved work. Resolves to "save",
 * "discard" or "cancel"; dismissing it any other way counts as cancelling,
 * which is the safe answer when the alternative is losing edits.
 */
export function askSaveChanges(names) {
  return new Promise((resolve) => {
    let answer = "cancel";
    const body = openOverlay(t("dialog.saveTitle"), false, () => resolve(answer));

    const question = document.createElement("p");
    question.textContent = t("dialog.saveMessage", { n: names.length });
    body.append(question);

    const list = document.createElement("ul");
    list.className = "unsaved-list";
    for (const name of names.slice(0, 10)) {
      const row = document.createElement("li");
      row.textContent = name;
      list.append(row);
    }
    if (names.length > 10) {
      const more = document.createElement("li");
      more.className = "muted";
      more.textContent = t("dialog.andMore", { n: names.length - 10 });
      list.append(more);
    }
    body.append(list);

    const buttons = document.createElement("div");
    buttons.className = "dialog-buttons";
    const button = (key, value, primary) => {
      const element = document.createElement("button");
      element.type = "button";
      element.className = `dialog-button${primary ? " primary" : ""}`;
      element.textContent = t(key);
      element.addEventListener("click", () => {
        answer = value;
        closeOverlay();
      });
      return element;
    };
    const save = button("dialog.save", "save", true);
    buttons.append(save, button("dialog.dontSave", "discard"), button("dialog.cancel", "cancel"));
    body.append(buttons);
    save.focus();
  });
}

/**
 * The New File chooser: a blank document, then every type that offers starting
 * content. Starred types sort to the top; both groups are alphabetical.
 *
 * @param {Array<{id: string, label: string, extension: string}>} types
 * @param {Set<string>} favourites  ids the user has starred (mutated in place)
 * @param {(id: string|null) => void} onPick  null means "blank document"
 * @param {() => void} onFavouritesChanged
 */
export function showNewFile(types, favourites, onPick, onFavouritesChanged) {
  const body = openOverlay(t("newFile.title"), true);
  body.classList.add("newfile-body");

  const filter = document.createElement("input");
  filter.className = "newfile-filter";
  filter.type = "text";
  filter.placeholder = t("newFile.filter");
  filter.spellcheck = false;

  const list = document.createElement("div");
  list.className = "newfile-list";

  const count = document.createElement("div");
  count.className = "newfile-count";

  body.append(filter, list, count);

  let rows = [];
  let highlighted = 0;

  /**
   * Types matching the filter, favourites first, each group alphabetical.
   * The label and the extension are both searchable, so "dar" and "dart" both
   * find Dart, and ".ps1" finds PowerShell.
   */
  const ordered = () => {
    const query = filter.value.trim().toLowerCase();
    const matches = query
      ? types.filter(
          (type) =>
            type.label.toLowerCase().includes(query) ||
            type.extension.toLowerCase().includes(query.replace(/^\./, "")),
        )
      : types;
    const starred = matches.filter((type) => favourites.has(type.id));
    const rest = matches.filter((type) => !favourites.has(type.id));
    const byLabel = (a, b) => a.label.localeCompare(b.label);
    return [...starred.sort(byLabel), ...rest.sort(byLabel)];
  };

  const render = () => {
    list.textContent = "";
    rows = [];

    const addRow = (type) => {
      const row = document.createElement("div");
      row.className = "newfile-row";

      const choose = document.createElement("button");
      choose.type = "button";
      choose.className = "newfile-choose";

      const name = document.createElement("span");
      name.className = "newfile-name";
      name.textContent = type ? type.label : t("newFile.blank");

      const detail = document.createElement("span");
      detail.className = "newfile-detail";
      detail.textContent = type ? `.${type.extension}` : t("newFile.blankHint");

      choose.append(name, detail);
      choose.addEventListener("click", () => {
        closeOverlay();
        onPick(type ? type.id : null);
      });
      row.append(choose);

      // The blank document is not a type, so it cannot be starred.
      if (type) {
        const star = document.createElement("button");
        star.type = "button";
        const on = favourites.has(type.id);
        star.className = `newfile-star${on ? " on" : ""}`;
        star.title = t(on ? "newFile.unfavourite" : "newFile.favourite");
        star.setAttribute("aria-pressed", String(on));
        // The glyph itself isn't a meaningful name for a screen reader to read
        // out — it would announce the character, not "favourite"/"unfavourite".
        star.setAttribute("aria-label", star.title);
        star.textContent = on ? "★" : "☆";
        star.addEventListener("click", (event) => {
          event.stopPropagation();
          if (on) favourites.delete(type.id);
          else favourites.add(type.id);
          onFavouritesChanged();
          render();
          // `render()` removed the button that was just clicked, which would
          // otherwise drop focus outside the dialog and kill arrow keys.
          filter.focus();
        });
        row.append(star);
      }

      list.append(row);
      rows.push({ row, choose, id: type ? type.id : null });
    };

    // Starring re-sorts the list, so remember what was highlighted and put the
    // highlight back on the same entry afterwards rather than on whatever row
    // happens to land at that index.
    const wasHighlighted = rows[highlighted]?.id ?? undefined;

    const sorted = ordered();
    // The blank document only belongs at the top of an unfiltered list — once
    // you are searching for a type, the first match should be what Enter takes.
    const searching = filter.value.trim() !== "";
    if (!searching) addRow(null);

    const firstUnstarred = sorted.findIndex((type) => !favourites.has(type.id));
    sorted.forEach((type, index) => {
      // A rule between the starred block and the rest, when both exist.
      if (index === firstUnstarred && index > 0) list.append(document.createElement("hr"));
      addRow(type);
    });

    if (wasHighlighted !== undefined) {
      const moved = rows.findIndex((entry) => entry.id === wasHighlighted);
      if (moved !== -1) highlighted = moved;
    }
    highlighted = Math.max(0, Math.min(highlighted, rows.length - 1));
    rows.forEach(({ row }, index) => row.classList.toggle("highlighted", index === highlighted));
    count.textContent = searching ? t("modal.symbolCount", { n: sorted.length }) : "";
  };

  const move = (delta) => {
    highlighted = Math.min(Math.max(highlighted + delta, 0), rows.length - 1);
    rows.forEach(({ row }, index) => row.classList.toggle("highlighted", index === highlighted));
    rows[highlighted]?.row.scrollIntoView({ block: "nearest" });
  };

  filter.addEventListener("input", () => {
    // Typing restarts at the top match, so "dar" then Enter opens Dart.
    highlighted = 0;
    render();
  });

  body.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      move(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      rows[highlighted]?.choose.click();
    }
  });

  render();
  filter.focus();
}

/**
 * The file-association screen: languages as collapsible groups, each extension
 * a checkbox. `onApply` receives the checked extensions.
 */
export function showAssociations(groups, checkedNow, onApply) {
  const body = openOverlay(t("modal.associationsTitle"), true);
  body.classList.add("assoc-body");

  const intro = document.createElement("p");
  intro.textContent = t("assoc.intro");
  body.append(intro);

  const note = document.createElement("p");
  note.className = "muted assoc-note";
  note.textContent = t("assoc.defaultsNote");
  body.append(note);

  const selected = new Set(checkedNow);
  // Checkboxes are kept here rather than written onto the caller's objects.
  const boxes = new Map();
  const list = document.createElement("div");
  list.className = "assoc-groups";

  /** Reflects a group's header checkbox: on, off, or partly. */
  const refreshGroup = (group, header) => {
    const on = group.extensions.filter((e) => selected.has(e.extension)).length;
    header.checked = on === group.extensions.length;
    header.indeterminate = on > 0 && on < group.extensions.length;
  };

  const groupRefreshers = [];

  for (const group of groups) {
    const section = document.createElement("section");
    section.className = "assoc-group";

    const head = document.createElement("label");
    head.className = "assoc-head";
    const headBox = document.createElement("input");
    headBox.type = "checkbox";
    const headName = document.createElement("span");
    headName.textContent = group.label;
    head.append(headBox, headName);

    const items = document.createElement("div");
    items.className = "assoc-items";

    for (const entry of group.extensions) {
      const row = document.createElement("label");
      row.className = "assoc-item";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = selected.has(entry.extension);
      box.addEventListener("change", () => {
        if (box.checked) selected.add(entry.extension);
        else selected.delete(entry.extension);
        refreshGroup(group, headBox);
      });
      const name = document.createElement("span");
      name.textContent = `${entry.label} (*.${entry.extension})`;
      boxes.set(entry.extension, box);
      row.append(box, name);
      // Say who has it now, so taking it over is a deliberate act.
      if (entry.owner) {
        const owner = document.createElement("em");
        owner.className = "assoc-owner";
        owner.textContent = t("assoc.currently", { owner: entry.owner });
        row.append(owner);
        row.title = t("assoc.currently", { owner: entry.owner });
      }
      items.append(row);
      entry.box = box;
    }

    headBox.addEventListener("change", () => {
      for (const entry of group.extensions) {
        boxes.get(entry.extension).checked = headBox.checked;
        if (headBox.checked) selected.add(entry.extension);
        else selected.delete(entry.extension);
      }
      headBox.indeterminate = false;
    });

    refreshGroup(group, headBox);
    groupRefreshers.push(() => refreshGroup(group, headBox));
    section.append(head, items);
    list.append(section);
  }
  body.append(list);

  const setAll = (on) => {
    for (const group of groups) {
      for (const entry of group.extensions) {
        boxes.get(entry.extension).checked = on;
        if (on) selected.add(entry.extension);
        else selected.delete(entry.extension);
      }
    }
    for (const refresh of groupRefreshers) refresh();
  };

  const buttons = document.createElement("div");
  buttons.className = "dialog-buttons assoc-buttons";
  const plain = (label, run) => {
    const element = document.createElement("button");
    element.type = "button";
    element.className = "dialog-button";
    element.textContent = label;
    element.addEventListener("click", run);
    return element;
  };
  const apply = plain(t("assoc.apply"), () => {
    closeOverlay();
    onApply([...selected]);
  });
  apply.classList.add("primary");
  buttons.append(
    plain(t("assoc.selectAll"), () => setAll(true)),
    plain(t("assoc.unselectAll"), () => setAll(false)),
    apply,
    plain(t("dialog.cancel"), closeOverlay),
  );
  body.append(buttons);
}

/** Builds the shortcut-reference columns into any container — the standalone
 * F1 dialog and the Help Centre's own Shortcuts page both call this. */
function renderShortcutsInto(container) {
  const columns = document.createElement("div");
  columns.className = "shortcut-columns";
  for (const { group, items } of SHORTCUTS) {
    const section = document.createElement("section");
    const heading = document.createElement("h3");
    heading.textContent = t(group);
    const table = document.createElement("table");
    for (const [keys, description] of items) {
      const row = document.createElement("tr");
      const keyCell = document.createElement("td");
      keyCell.className = "shortcut-keys";
      // Literal key names print as written; a `sc.k.` entry is prose to translate.
      keyCell.textContent = keys.startsWith("sc.k.") ? t(keys) : keys;
      const textCell = document.createElement("td");
      textCell.textContent = t(description);
      row.append(keyCell, textCell);
      table.append(row);
    }
    section.append(heading, table);
    columns.append(section);
  }
  container.append(columns);
}

/**
 * One entry per Help Centre sidebar item. Most are plain prose (an intro plus
 * a few bullet points); `feature` marks the two with a live on/off action
 * button, and `custom` marks the one (Shortcuts) with its own layout instead
 * of prose.
 */
const HELP_TOPICS = [
  { id: "overview", icon: "info", titleKey: "help.overviewTitle" },
  { id: "files", icon: "file", titleKey: "help.filesTitle" },
  { id: "editing", icon: "comment", titleKey: "help.editingTitle" },
  { id: "search", icon: "search", titleKey: "help.searchTitle" },
  { id: "bookmarks", icon: "bookmark", titleKey: "help.bookmarksTitle" },
  { id: "splitTabs", icon: "splitRight", titleKey: "help.splitTabsTitle" },
  { id: "folding", icon: "chevronRight", titleKey: "help.foldingTitle" },
  { id: "terminal", icon: "terminal", titleKey: "help.terminalTitle" },
  { id: "run", icon: "play", titleKey: "help.runTitle" },
  { id: "wordWrap", icon: "wordWrap", titleKey: "view.wordWrap" },
  { id: "spellCheck", icon: "spellcheck", titleKey: "view.spellCheck" },
  { id: "autismTheme", icon: "leaf", titleKey: "help.autismTheme", feature: "autismTheme" },
  { id: "bionicReading", icon: "bold", titleKey: "help.bionicReading", feature: "bionicReading" },
  { id: "language", icon: "globe", titleKey: "help.languageTitle" },
  { id: "shortcuts", icon: "keyboard", titleKey: "help.shortcuts", custom: true },
];

/** The intro/points/footer keys for a topic, and — for the two with one — the
 * action button's live state. Kept apart from `HELP_TOPICS` since it needs
 * `ctx` (current theme/Bionic Reading state) to compute the button. */
function topicContent(topic, ctx) {
  switch (topic.id) {
    case "overview":
      return { intro: t("help.overviewIntro"), points: [t("help.overviewPoint1"), t("help.overviewPoint2")] };
    case "files":
      return {
        intro: t("help.filesIntro"),
        points: [
          t("help.filesPoint1"),
          t("help.filesPoint2"),
          t("help.filesPoint3"),
          t("help.filesPoint4"),
        ],
      };
    case "editing":
      return {
        intro: t("help.editingIntro"),
        points: [
          t("help.editingPoint1"),
          t("help.editingPoint2"),
          t("help.editingPoint3"),
          t("help.editingPoint4"),
        ],
      };
    case "search":
      return { intro: t("help.searchIntro"), points: [t("help.searchPoint1"), t("help.searchPoint2")] };
    case "bookmarks":
      return { intro: t("help.bookmarksIntro"), points: [t("help.bookmarksPoint1")] };
    case "splitTabs":
      return {
        intro: t("help.splitTabsIntro"),
        points: [t("help.splitTabsPoint1"), t("help.splitTabsPoint2"), t("help.splitTabsPoint3")],
      };
    case "folding":
      return { intro: t("help.foldingIntro"), points: [t("help.foldingPoint1"), t("help.foldingPoint2")] };
    case "terminal":
      return {
        intro: t("help.terminalIntro"),
        points: [t("help.terminalPoint1"), t("help.terminalPoint2"), t("help.terminalPoint3")],
      };
    case "run":
      return {
        intro: t("help.runIntro"),
        points: [t("help.runPoint1"), t("help.runPoint2"), t("help.runPoint3")],
      };
    case "wordWrap":
      return { intro: t("help.wordWrapIntro"), points: [t("help.wordWrapPoint1")] };
    case "spellCheck":
      return {
        intro: t("help.spellCheckIntro"),
        points: [t("help.spellCheckPoint1"), t("help.spellCheckPoint2")],
      };
    case "language":
      return {
        intro: t("help.languageIntro"),
        points: [t("help.languagePoint1"), t("help.languagePoint2")],
      };
    case "autismTheme":
      return {
        intro: t("autismHelp.intro"),
        points: [
          t("autismHelp.point1"),
          t("autismHelp.point2"),
          t("autismHelp.point3"),
          t("autismHelp.point4"),
        ],
        footer: t("autismHelp.footer"),
        action: {
          label: t(ctx.autismTheme() ? "autismHelp.active" : "autismHelp.switch"),
          disabled: ctx.autismTheme(),
          onClick: ctx.setAutismTheme,
        },
      };
    case "bionicReading":
      return {
        intro: t("bionicHelp.intro"),
        points: [t("bionicHelp.point1"), t("bionicHelp.point2"), t("bionicHelp.point3")],
        footer: t("bionicHelp.footer"),
        action: {
          label: t(ctx.bionicReading() ? "bionicHelp.turnOff" : "bionicHelp.turnOn"),
          onClick: ctx.toggleBionicReading,
        },
      };
    default:
      return {};
  }
}

/**
 * The Help Centre: a search box and a sidebar of topics on the left, the
 * chosen topic's content on the right — one modal instead of a separate
 * dialog per feature. `ctx` supplies the live state (and setters) behind the
 * two pages with an action button; everything else is static prose.
 */
export function showHelpCenter(ctx, initialId = "overview") {
  const body = openOverlay(t("modal.helpCenterTitle"), true);
  body.classList.add("help-body");

  const search = document.createElement("input");
  search.className = "help-search";
  search.type = "text";
  search.placeholder = t("help.searchPlaceholder");
  search.spellcheck = false;
  body.append(search);

  const layout = document.createElement("div");
  layout.className = "help-layout";
  const sidebar = document.createElement("div");
  sidebar.className = "help-sidebar";
  const content = document.createElement("div");
  content.className = "help-content";
  layout.append(sidebar, content);
  body.append(layout);

  let activeId = initialId;

  const renderContent = () => {
    const topic = HELP_TOPICS.find((entry) => entry.id === activeId) ?? HELP_TOPICS[0];
    content.textContent = "";

    const head = document.createElement("div");
    head.className = "about-head";
    const heading = document.createElement("h1");
    heading.textContent = t(topic.titleKey);
    head.append(iconElement(topic.icon, { size: "32px" }), heading);
    content.append(head);

    if (topic.custom) {
      renderShortcutsInto(content);
      return;
    }

    const { intro, points, footer, action } = topicContent(topic, ctx);
    if (intro) {
      const introEl = document.createElement("p");
      introEl.textContent = intro;
      content.append(introEl);
    }
    if (points?.length) {
      const list = document.createElement("ul");
      list.className = "feature-help-list";
      for (const point of points) {
        const item = document.createElement("li");
        item.textContent = point;
        list.append(item);
      }
      content.append(list);
    }
    if (footer) {
      const footerEl = document.createElement("p");
      footerEl.className = "muted";
      footerEl.textContent = footer;
      content.append(footerEl);
    }
    if (action) {
      const buttons = document.createElement("div");
      buttons.className = "dialog-buttons";
      const button = document.createElement("button");
      button.type = "button";
      button.className = "dialog-button primary";
      button.textContent = action.label;
      if (action.disabled) {
        button.disabled = true;
      } else {
        // Re-renders in place rather than closing the Help Centre — flipping
        // a setting from here shouldn't dump the user back out of it.
        button.addEventListener("click", () => {
          action.onClick();
          renderContent();
        });
      }
      buttons.append(button);
      content.append(buttons);
    }
  };

  const renderSidebar = () => {
    const query = search.value.trim().toLowerCase();
    sidebar.textContent = "";
    const matches = HELP_TOPICS.filter((topic) => t(topic.titleKey).toLowerCase().includes(query));
    if (!matches.length) {
      const empty = document.createElement("div");
      empty.className = "help-empty muted";
      empty.textContent = t("help.noResults");
      sidebar.append(empty);
      return;
    }
    for (const topic of matches) {
      const item = document.createElement("button");
      item.type = "button";
      item.className = `help-sidebar-item${topic.id === activeId ? " active" : ""}`;
      item.append(iconElement(topic.icon), document.createTextNode(t(topic.titleKey)));
      item.addEventListener("click", () => {
        activeId = topic.id;
        renderSidebar();
        renderContent();
      });
      sidebar.append(item);
    }
  };

  search.addEventListener("input", renderSidebar);

  renderSidebar();
  renderContent();
  search.focus();
}

export function showAbout(version) {
  const body = openOverlay(t("modal.aboutTitle"), false);
  body.classList.add("about-body");

  // The name sits beside the application mark; every line below it is an icon
  // in a fixed-width column with its label to the right, so the text aligns.
  const head = document.createElement("div");
  head.className = "about-head";
  const name = document.createElement("h1");
  name.textContent = "JustCode";
  head.append(appLogoElement(56), name);
  body.append(head);

  const rows = [
    ["info", t("about.version", { version })],
    ["file", t("about.tagline")],
    ["symbol", t("about.builtWith"), "muted"],
  ];
  for (const [icon, text, className] of rows) {
    const row = document.createElement("p");
    row.className = `about-row${className ? ` ${className}` : ""}`;
    const label = document.createElement("span");
    label.textContent = text;
    row.append(iconElement(icon), label);
    body.append(row);
  }
}
