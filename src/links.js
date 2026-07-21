import { ViewPlugin, Decoration, EditorView } from "@codemirror/view";
import { RangeSetBuilder } from "@codemirror/state";

// Matches http/https/mailto URLs. Trailing punctuation that is usually
// sentence/markup noise rather than part of the link is trimmed afterwards.
const URL_RE = /\b(?:https?:\/\/|mailto:)[^\s<>"'`()[\]{}]+/g;
const TRAILING = /[.,;:!?"'>)\]}]+$/;

// Set once by the app; called with a URL when a link is activated.
let openHandler = () => {};
export function setLinkHandler(fn) {
  openHandler = fn;
}

/** Finds the URL ranges on the lines currently in view. */
function collectLinks(view) {
  const builder = new RangeSetBuilder();
  for (const { from, to } of view.visibleRanges) {
    const text = view.state.sliceDoc(from, to);
    for (const match of text.matchAll(URL_RE)) {
      const trimmed = match[0].replace(TRAILING, "");
      const start = from + match.index;
      const end = start + trimmed.length;
      if (end > start) {
        builder.add(
          start,
          end,
          Decoration.mark({ class: "cm-link", attributes: { "data-url": trimmed } }),
        );
      }
    }
  }
  return builder.finish();
}

/** Returns the URL decoration covering `pos`, or null. */
function linkAt(view, pos) {
  const plugin = view.plugin(linkPlugin);
  if (!plugin) return null;
  let found = null;
  plugin.decorations.between(pos, pos, (from, to, value) => {
    if (pos >= from && pos <= to) {
      found = value.spec.attributes["data-url"];
      return false;
    }
  });
  return found;
}

// A single plugin instance, shared by every editor state.
const linkPlugin = ViewPlugin.fromClass(
  class {
    constructor(view) {
      this.decorations = collectLinks(view);
    }
    update(update) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = collectLinks(update.view);
      }
    }
  },
  {
    decorations: (plugin) => plugin.decorations,
    eventHandlers: {
      mousedown(event, view) {
        if (!event.ctrlKey && !event.metaKey) return false;
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        if (pos == null) return false;
        const url = linkAt(view, pos);
        if (!url) return false;
        event.preventDefault();
        openHandler(url);
        return true;
      },
    },
  },
);

// Toggle a class on the editor while a modifier is held, so the CSS can turn
// links into pointer-cursor affordances only then (matching VS Code).
const modifierWatch = EditorView.domEventHandlers({
  keydown(event, view) {
    if (event.key === "Control" || event.key === "Meta") view.dom.classList.add("cm-mod-held");
  },
  keyup(event, view) {
    if (event.key === "Control" || event.key === "Meta") view.dom.classList.remove("cm-mod-held");
  },
  blur(event, view) {
    view.dom.classList.remove("cm-mod-held");
  },
});

const linkTheme = EditorView.baseTheme({
  ".cm-link": { textDecoration: "underline", textDecorationColor: "rgba(128,128,128,0.6)" },
  ".cm-mod-held .cm-link": { cursor: "pointer", textDecorationColor: "currentColor" },
});

/** The extensions that make URLs detectable and Ctrl/Cmd-clickable. */
export const linkExtensions = [linkPlugin, modifierWatch, linkTheme];

/** Command: open the link under the caret. Returns false when there is none. */
export function openLinkAtCursor(view) {
  const url = linkAt(view, view.state.selection.main.head);
  if (!url) return false;
  openHandler(url);
  return true;
}
