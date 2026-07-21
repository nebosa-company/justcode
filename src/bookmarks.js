import { StateField, StateEffect, MapMode } from "@codemirror/state";
import { gutter, GutterMarker, EditorView } from "@codemirror/view";

export const BOOKMARK_SLOTS = [1, 2, 3];

/** `{ slot, pos }` — a null `pos` clears the slot. */
const setBookmark = StateEffect.define();

/**
 * Bookmarks live in the editor state, so each tab keeps its own set and they
 * survive tab switches. Positions are mapped through every document change, so
 * a bookmark follows its line as text is inserted or removed above it. Deleting
 * a bookmarked line leaves the mark on whatever line takes its place — the
 * stored position is the line start, which survives the deletion as a boundary.
 */
const bookmarkField = StateField.define({
  create: () => new Map(),
  update(bookmarks, tr) {
    let next = bookmarks;
    if (tr.docChanged && next.size) {
      const mapped = new Map();
      for (const [slot, pos] of next) {
        const at = tr.changes.mapPos(pos, -1, MapMode.TrackDel);
        if (at != null) mapped.set(slot, at);
      }
      next = mapped;
    }
    for (const effect of tr.effects) {
      if (!effect.is(setBookmark)) continue;
      next = new Map(next);
      if (effect.value.pos == null) next.delete(effect.value.slot);
      else next.set(effect.value.slot, effect.value.pos);
    }
    return next;
  },
});

class BookmarkMarker extends GutterMarker {
  constructor(label) {
    super();
    this.label = label;
  }
  eq(other) {
    return other.label === this.label;
  }
  toDOM() {
    const element = document.createElement("span");
    element.className = "cm-bookmark";
    element.textContent = this.label;
    return element;
  }
}

/** Slots bookmarked on the line containing `pos`, in numeric order. */
function slotsOnLine(state, pos) {
  const lineNumber = state.doc.lineAt(pos).number;
  const hits = [];
  for (const [slot, at] of state.field(bookmarkField)) {
    if (at <= state.doc.length && state.doc.lineAt(at).number === lineNumber) hits.push(slot);
  }
  return hits.sort((a, b) => a - b);
}

const bookmarkGutter = gutter({
  class: "cm-bookmark-gutter",
  lineMarker(view, line) {
    const hits = slotsOnLine(view.state, line.from);
    return hits.length ? new BookmarkMarker(hits.join("")) : null;
  },
  // No initialSpacer: reserving a permanent column widened the gutter for every
  // file, bookmarks or not. The column appears with the first bookmark instead.
  lineMarkerChange: (update) =>
    update.startState.field(bookmarkField) !== update.state.field(bookmarkField),
});

const bookmarkTheme = EditorView.baseTheme({
  ".cm-bookmark-gutter": { paddingLeft: "1px" },
  ".cm-bookmark": {
    display: "block",
    textAlign: "center",
    fontSize: "0.75em",
    fontWeight: "bold",
    lineHeight: "1.6",
    color: "#ffffff",
    background: "#4d9cf5",
    borderRadius: "3px",
  },
});

export const bookmarkExtensions = [bookmarkField, bookmarkGutter, bookmarkTheme];

/** Sets or clears bookmark `slot` on the line holding the caret. */
export function toggleBookmark(view, slot) {
  const line = view.state.doc.lineAt(view.state.selection.main.head);
  const existing = view.state.field(bookmarkField).get(slot);
  const alreadyHere =
    existing != null &&
    existing <= view.state.doc.length &&
    view.state.doc.lineAt(existing).number === line.number;
  view.dispatch({ effects: setBookmark.of({ slot, pos: alreadyHere ? null : line.from }) });
  return true;
}

/**
 * One-key bookmarking for the menu: clears the bookmark on this line if there
 * is one, otherwise claims the lowest free slot. Does nothing once all three
 * are in use and none of them is here.
 */
export function toggleNextBookmark(view) {
  const line = view.state.doc.lineAt(view.state.selection.main.head);
  const here = slotsOnLine(view.state, line.from);
  if (here.length) {
    view.dispatch({ effects: setBookmark.of({ slot: here[0], pos: null }) });
    return true;
  }
  const used = view.state.field(bookmarkField);
  const free = BOOKMARK_SLOTS.find((slot) => !used.has(slot));
  if (free == null) return false;
  view.dispatch({ effects: setBookmark.of({ slot: free, pos: line.from }) });
  return true;
}

/** Moves the caret to bookmark `slot`; false when that slot is unset. */
export function gotoBookmark(view, slot) {
  const pos = view.state.field(bookmarkField).get(slot);
  if (pos == null || pos > view.state.doc.length) return false;
  const line = view.state.doc.lineAt(pos);
  view.dispatch({ selection: { anchor: line.from }, scrollIntoView: true });
  view.focus();
  return true;
}

/** Which slots are currently set, for menu state. */
export function activeBookmarkSlots(state) {
  return [...state.field(bookmarkField).keys()].sort((a, b) => a - b);
}
