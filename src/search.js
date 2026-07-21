import {
  search,
  SearchQuery,
  getSearchQuery,
  setSearchQuery,
  findNext,
  findPrevious,
  replaceNext,
  replaceAll,
  openSearchPanel,
  closeSearchPanel,
  searchPanelOpen,
} from "@codemirror/search";
import { iconElement } from "./icons.js";

// Counting every match in a huge file would block typing, so stop here and
// display the total as "999+".
const MAX_COUNTED_MATCHES = 999;

const panels = new WeakMap();
let openWithReplace = false;

/**
 * A find/replace panel modelled on the VS Code one: pinned to the top, a
 * collapsible replace row, Aa/.*​/ab toggles and — the part CodeMirror's stock
 * panel has no equivalent for — a live "N of M" match counter.
 */
class SearchPanel {
  constructor(view) {
    this.view = view;
    this.replaceVisible = openWithReplace;
    this.build();
    panels.set(view, this);
  }

  // Rendered above the editor rather than below it.
  get top() {
    return true;
  }

  build() {
    this.dom = document.createElement("div");
    this.dom.className = "jc-search";
    this.dom.addEventListener("keydown", (event) => this.onKeyDown(event));

    this.toggleReplace = button("chevronRight", "Toggle Replace", () =>
      this.setReplaceVisible(!this.replaceVisible),
    );
    this.toggleReplace.classList.add("jc-search-expand");

    this.searchInput = input("Find");
    this.searchInput.addEventListener("input", () => this.commit());

    this.caseButton = toggle("Aa", "Match Case", () => this.commit());
    this.regexpButton = toggle(".*", "Use Regular Expression", () => this.commit());
    this.wordButton = toggle("ab", "Match Whole Word", () => this.commit());

    this.count = document.createElement("span");
    this.count.className = "jc-search-count";

    const findRow = document.createElement("div");
    findRow.className = "jc-search-row";
    const findField = document.createElement("div");
    findField.className = "jc-search-field";
    findField.append(this.searchInput, this.caseButton, this.regexpButton, this.wordButton);
    findRow.append(
      findField,
      this.count,
      button("arrowUp", "Previous Match (Shift+Enter)", () => findPrevious(this.view)),
      button("arrowDown", "Next Match (Enter)", () => findNext(this.view)),
      button("close", "Close (Escape)", () => {
        closeSearchPanel(this.view);
        this.view.focus();
      }),
    );

    this.replaceInput = input("Replace");
    this.replaceInput.addEventListener("input", () => this.commit());

    const replaceButton = textButton("Replace", () => {
      this.commit();
      replaceNext(this.view);
    });
    const replaceAllButton = textButton("Replace All", () => {
      this.commit();
      replaceAll(this.view);
    });

    this.replaceRow = document.createElement("div");
    this.replaceRow.className = "jc-search-row";
    const replaceField = document.createElement("div");
    replaceField.className = "jc-search-field";
    replaceField.append(this.replaceInput);
    this.replaceRow.append(replaceField, replaceButton, replaceAllButton);

    const rows = document.createElement("div");
    rows.className = "jc-search-rows";
    rows.append(findRow, this.replaceRow);

    this.dom.append(this.toggleReplace, rows);
    this.setReplaceVisible(this.replaceVisible);
  }

  setReplaceVisible(visible) {
    this.replaceVisible = visible;
    this.replaceRow.hidden = !visible;
    this.toggleReplace.replaceChildren(iconElement(visible ? "chevronDown" : "chevronRight"));
  }

  focusSearch() {
    this.searchInput.focus();
    this.searchInput.select();
  }

  onKeyDown(event) {
    if (event.key === "Enter") {
      event.preventDefault();
      this.commit();
      if (event.target === this.replaceInput) replaceNext(this.view);
      else if (event.shiftKey) findPrevious(this.view);
      else findNext(this.view);
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeSearchPanel(this.view);
      this.view.focus();
    }
  }

  /** Pushes the panel's fields into the editor's search state. */
  commit() {
    const query = new SearchQuery({
      search: this.searchInput.value,
      caseSensitive: this.caseButton.classList.contains("on"),
      regexp: this.regexpButton.classList.contains("on"),
      wholeWord: this.wordButton.classList.contains("on"),
      replace: this.replaceInput.value,
    });
    if (!query.eq(getSearchQuery(this.view.state))) {
      this.view.dispatch({ effects: setSearchQuery.of(query) });
    }
    this.renderCount();
  }

  renderCount() {
    const query = getSearchQuery(this.view.state);
    const invalid = query.search !== "" && !query.valid;
    this.searchInput.classList.toggle("invalid", invalid);

    if (query.search === "") {
      this.count.textContent = "";
      return;
    }
    if (invalid) {
      this.count.textContent = "Invalid regex";
      return;
    }

    const { total, index, capped } = this.countMatches(query);
    this.count.textContent =
      total === 0
        ? "No results"
        : `${index || "?"} of ${total}${capped ? "+" : ""}`;
  }

  /** Walks the document counting matches and locating the selected one. */
  countMatches(query) {
    const state = this.view.state;
    const selection = state.selection.main;
    let total = 0;
    let index = 0;
    try {
      const cursor = query.getCursor(state);
      for (let step = cursor.next(); !step.done; step = cursor.next()) {
        total++;
        const match = step.value;
        if (match.from === selection.from && match.to === selection.to) index = total;
        if (total >= MAX_COUNTED_MATCHES) return { total, index, capped: true };
      }
    } catch {
      return { total: 0, index: 0, capped: false };
    }
    return { total, index, capped: false };
  }

  mount() {
    // Seed from the selection, the way every editor does. mount() runs inside
    // CodeMirror's update cycle, so commit()'s dispatch has to be deferred —
    // dispatching now throws "update in progress" and the plugin is torn down.
    const selection = this.view.state.selection.main;
    let seededFromSelection = false;
    if (!selection.empty && selection.to - selection.from < 200) {
      const text = this.view.state.sliceDoc(selection.from, selection.to);
      if (!text.includes("\n")) {
        this.searchInput.value = text;
        seededFromSelection = true;
      }
    }
    if (!seededFromSelection) {
      this.searchInput.value = getSearchQuery(this.view.state).search;
    }
    this.renderCount();
    this.focusSearch();
    if (seededFromSelection) queueMicrotask(() => this.commit());
  }

  update(update) {
    if (update.docChanged || update.selectionSet || update.transactions.length) {
      this.renderCount();
    }
  }

  destroy() {
    if (panels.get(this.view) === this) panels.delete(this.view);
  }
}

function input(placeholder) {
  const element = document.createElement("input");
  element.type = "text";
  element.placeholder = placeholder;
  element.className = "jc-search-input";
  element.spellcheck = false;
  return element;
}

function button(icon, title, onClick) {
  const element = document.createElement("button");
  element.type = "button";
  element.className = "jc-search-button";
  element.title = title;
  element.append(iconElement(icon));
  element.addEventListener("click", onClick);
  return element;
}

function textButton(label, onClick) {
  const element = document.createElement("button");
  element.type = "button";
  element.className = "jc-search-text-button";
  element.textContent = label;
  element.addEventListener("click", onClick);
  return element;
}

function toggle(label, title, onChange) {
  const element = document.createElement("button");
  element.type = "button";
  element.className = "jc-search-toggle";
  element.title = title;
  element.textContent = label;
  element.addEventListener("click", () => {
    element.classList.toggle("on");
    element.setAttribute("aria-pressed", String(element.classList.contains("on")));
    onChange();
  });
  return element;
}

function open(view, withReplace) {
  const existing = panels.get(view);
  if (existing && searchPanelOpen(view.state)) {
    if (withReplace) existing.setReplaceVisible(true);
    existing.focusSearch();
    return true;
  }
  openWithReplace = withReplace;
  openSearchPanel(view);
  return true;
}

export function searchExtensions() {
  return search({ top: true, createPanel: (view) => new SearchPanel(view) });
}

export const searchPanelKeymap = [
  { key: "Mod-f", run: (view) => open(view, false), scope: "editor search-panel" },
  { key: "Mod-h", run: (view) => open(view, true), scope: "editor search-panel" },
  { key: "Escape", run: closeSearchPanel, scope: "editor search-panel" },
  { key: "F3", run: findNext, shift: findPrevious, scope: "editor search-panel", preventDefault: true },
  { key: "Mod-g", run: findNext, shift: findPrevious, scope: "editor search-panel", preventDefault: true },
];
