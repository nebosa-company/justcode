import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  drawSelection,
  dropCursor,
  rectangularSelection,
  crosshairCursor,
  Decoration,
  ViewPlugin,
} from "@codemirror/view";
import { EditorState, Compartment, RangeSetBuilder } from "@codemirror/state";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
  toggleLineComment,
  toggleBlockComment,
  deleteLine,
} from "@codemirror/commands";
import {
  bracketMatching,
  codeFolding,
  foldGutter,
  foldKeymap,
  indentOnInput,
  indentUnit,
  syntaxHighlighting,
  defaultHighlightStyle,
} from "@codemirror/language";
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import { highlightSelectionMatches } from "@codemirror/search";
import { lintGutter, lintKeymap, forEachDiagnostic } from "@codemirror/lint";
import { loadedLanguageExtensions, ensureLanguage } from "./languages.js";
import { themeExtensions } from "./theme.js";
import { searchExtensions, searchPanelKeymap } from "./search.js";
import { linkExtensions, openLinkAtCursor } from "./links.js";
import { bookmarkExtensions } from "./bookmarks.js";
import { spellcheckLinter, setSpellcheckEnabled } from "./spellcheck.js";
import { iconElement } from "./icons.js";

const languageConf = new Compartment();
const themeConf = new Compartment();
const fontSizeConf = new Compartment();
const spellcheckConf = new Compartment();
const wrapConf = new Compartment();
const bionicConf = new Compartment();

export const DEFAULT_FONT_SIZE = 14;
let currentFontSize = DEFAULT_FONT_SIZE;

/**
 * The zoom level, as a theme. It lives in a compartment and is changed through a
 * transaction rather than by mutating CSS underneath the editor: CodeMirror
 * caches the measured height of every line, and a font change it is not told
 * about leaves those heights stale — the gutter then renders its line numbers
 * and lint markers at the old spacing, drifting out of step with the text.
 */
function fontSizeTheme(px) {
  return EditorView.theme({
    "&": { fontSize: `${px}px` },
    // Inherit so the gutter can never resolve to a different size than the text.
    ".cm-scroller": { fontSize: "inherit" },
    ".cm-gutters": { fontSize: "inherit" },
  });
}

/** Effect that applies a new zoom level; also the size new states are built at. */
export function fontSizeEffect(px) {
  currentFontSize = px;
  return fontSizeConf.reconfigure(fontSizeTheme(px));
}

// Off by default: code is written to a margin, and wrapping it makes the line
// numbers stop matching the rows on screen.
let currentWordWrap = false;

/** Effect that turns line wrapping on or off; also the mode new states start in. */
export function wordWrapEffect(enabled) {
  currentWordWrap = enabled;
  return wrapConf.reconfigure(enabled ? EditorView.lineWrapping : []);
}

/**
 * Bionic Reading: bolds the leading portion of each word so the eye can
 * anchor on fewer fixation points per line. Evidence that this actually
 * speeds up reading is mixed, but it costs nothing to offer as a toggle —
 * see the Help ▸ Bionic Reading page for the caveat shown to users.
 *
 * Applies to whatever document is on screen, the same as word wrap — it is
 * not restricted to prose-like languages, since a code identifier is still
 * made of words a reader is scanning.
 */
const BIONIC_WORD = /[A-Za-z][A-Za-z'’]*/g;

/** How much of a word to bold. Short words end up fully bold; longer words
 * keep a readable tail — the same shape most Bionic Reading implementations
 * use, though the exact ratio is not standardised anywhere. */
function bionicBoldLength(wordLength) {
  return Math.max(1, Math.ceil(wordLength * 0.4));
}

const bionicMark = Decoration.mark({ class: "cm-bionic-bold" });

/** Only the visible ranges are scanned — cheap enough to redo on every
 * viewport change, and correct even for documents too large to decorate
 * in full. */
function bionicDecorations(view) {
  const builder = new RangeSetBuilder();
  for (const { from, to } of view.visibleRanges) {
    const text = view.state.doc.sliceString(from, to);
    BIONIC_WORD.lastIndex = 0;
    let match;
    while ((match = BIONIC_WORD.exec(text))) {
      const start = from + match.index;
      builder.add(start, start + bionicBoldLength(match[0].length), bionicMark);
    }
  }
  return builder.finish();
}

const bionicPlugin = ViewPlugin.fromClass(
  class {
    constructor(view) {
      this.decorations = bionicDecorations(view);
    }
    update(update) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = bionicDecorations(update.view);
      }
    }
  },
  { decorations: (instance) => instance.decorations },
);

let currentBionicReading = false;

/** Effect that turns Bionic Reading on or off; also the mode new states start in. */
export function bionicReadingEffect(enabled) {
  currentBionicReading = enabled;
  return bionicConf.reconfigure(enabled ? [bionicPlugin] : []);
}

let currentSpellcheck = false;

/**
 * Spell checking is a linter over the document text rather than the webview's
 * built-in checker. The native one cannot be counted (it reports nothing to
 * JavaScript) and misses words, because it reads the rendered DOM where syntax
 * highlighting splits words across spans. Ours produces ordinary diagnostics,
 * so misspellings appear in the problems count with suggested corrections.
 *
 * The linter is always installed and simply returns nothing while disabled —
 * see the note in spellcheck.js about why removing it would strand its markers.
 */
export function spellcheckEffect(enabled) {
  currentSpellcheck = enabled;
  setSpellcheckEnabled(enabled);
  return spellcheckConf.reconfigure([]);
}

/**
 * Comment/uncomment toggle that follows the language and the selection shape:
 *
 * - A selection on a single line uses the line-comment syntax when the language
 *   has one (`//`, `--`, `#`), since that is the lighter-weight edit.
 * - A selection spanning several lines uses the block-comment syntax when the
 *   language has one (`/* *​/`, `<!-- -->`), wrapping the whole region in one
 *   pair rather than prefixing every line.
 * - Languages with only one comment style (CSS: block only; PowerShell: line
 *   only) fall back to whichever they have.
 *
 * Each underlying command is itself a toggle, so a second invocation on the
 * same region removes the comment.
 */
export function toggleCommentSmart(target) {
  const { state } = target;
  const range = state.selection.main;
  const multiLine = state.doc.lineAt(range.to).number > state.doc.lineAt(range.from).number;
  const config = state.languageDataAt("commentTokens", range.from)[0] || {};
  const hasLine = typeof config.line === "string";
  const hasBlock = !!(config.block && config.block.open);

  // Line comments for a single line; block comments once several lines are
  // involved. A block-only language (CSS, HTML) uses block for both.
  const useBlock = hasBlock && (multiLine || !hasLine);
  if (useBlock) {
    // With no selection, a block comment would otherwise wrap nothing at the
    // caret and split the line — comment the whole line instead.
    if (range.empty) {
      const line = state.doc.lineAt(range.head);
      target.dispatch({ selection: { anchor: line.from, head: line.to } });
    }
    return toggleBlockComment(target);
  }
  if (hasLine) return toggleLineComment(target);
  if (hasBlock) return toggleBlockComment(target);
  return false;
}

/**
 * The theme lives in a compartment so it can be swapped on documents that are
 * not currently on screen — see `themeEffect`.
 */
export function themeEffect(themeId) {
  return themeConf.reconfigure(themeExtensions(themeId));
}

/** Chevrons in place of CodeMirror's default triangles in the fold gutter. */
function foldMarker(open) {
  const marker = document.createElement("span");
  marker.className = `cm-fold-marker${open ? "" : " cm-fold-marker-folded"}`;
  marker.title = open ? "Collapse" : "Expand";
  marker.append(iconElement(open ? "chevronDown" : "chevronRight"));
  return marker;
}

/** Builds the extension set shared by every tab. */
function baseExtensions(listeners) {
  return [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightSpecialChars(),
    history(),
    codeFolding(),
    foldGutter({ markerDOM: foldMarker }),
    drawSelection(),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    indentOnInput(),
    indentUnit.of("  "),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    bracketMatching(),
    closeBrackets(),
    autocompletion({ activateOnTyping: true, closeOnBlur: false }),
    rectangularSelection(),
    crosshairCursor(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    lintGutter(),
    searchExtensions(),
    linkExtensions,
    bookmarkExtensions,
    spellcheckLinter(),
    keymap.of([
      ...closeBracketsKeymap,
      ...searchPanelKeymap,
      { key: "Mod-/", run: toggleCommentSmart },
      { key: "Mod-Shift-k", run: deleteLine, preventDefault: true },
      { key: "Mod-Enter", run: openLinkAtCursor },
      ...defaultKeymap,
      ...historyKeymap,
      ...foldKeymap,
      ...completionKeymap,
      ...lintKeymap,
      indentWithTab,
    ]),
    EditorView.theme({ "&": { height: "100%" } }),
    EditorView.baseTheme({ ".cm-bionic-bold": { fontWeight: "700" } }),
    EditorView.updateListener.of((update) => {
      // Every pane shares one listener object and one status bar, so a pane
      // that is not focused must stay quiet — otherwise scrolling a background
      // pane, or its linter finishing a second later, overwrites the focused
      // file's problem count and cursor position and nothing puts them back.
      if (listeners.isCurrent && !listeners.isCurrent(update.view)) return;
      if (update.docChanged) listeners.onDocChanged?.();
      if (update.docChanged || update.selectionSet) listeners.onSelection?.(cursorPosition(update.state));
      // Diagnostics only change through a document edit or a lint transaction;
      // recomputing them on every cursor move walked the whole range set.
      if (update.docChanged || update.transactions.some((tr) => tr.effects.length)) {
        listeners.onDiagnostics?.(countDiagnostics(update.state));
      }
    }),
  ];
}

function cursorPosition(state) {
  const head = state.selection.main.head;
  const line = state.doc.lineAt(head);
  return { line: line.number, column: head - line.from + 1 };
}

function countDiagnostics(state) {
  let count = 0;
  let errors = 0;
  let spelling = 0;
  forEachDiagnostic(state, (diagnostic) => {
    // Spelling is reported separately: it is advisory, and lumping it in with
    // syntax errors would make a prose-heavy file look broken.
    if (diagnostic.source === "spelling") {
      spelling++;
      return;
    }
    count++;
    if (diagnostic.severity === "error") errors++;
  });
  return { count, errors, spelling };
}

/** Creates a fresh document state for a tab. */
export function createState(text, languageId, listeners, themeId, { readOnly = false } = {}) {
  return EditorState.create({
    doc: text,
    extensions: [
      baseExtensions(listeners),
      languageConf.of(loadedLanguageExtensions(languageId)),
      themeConf.of(themeExtensions(themeId)),
      fontSizeConf.of(fontSizeTheme(currentFontSize)),
      // Kept only so the toggle has a compartment to dispatch through; the
      // spell linter itself is installed unconditionally in baseExtensions.
      spellcheckConf.of([]),
      wrapConf.of(currentWordWrap ? EditorView.lineWrapping : []),
      bionicConf.of(currentBionicReading ? [bionicPlugin] : []),
      // The placeholder shown when no tab is open must not accept edits — they
      // would belong to no tab and be lost on the next action.
      readOnly ? [EditorState.readOnly.of(true), EditorView.editable.of(false)] : [],
    ],
  });
}

/**
 * Loads `languageId` if needed and applies it to the view. `stillWanted` is
 * checked after the (asynchronous) load so a quick tab switch during the import
 * cannot leave the editor showing another file's grammar.
 */
export async function applyLanguage(view, languageId, stillWanted = () => true) {
  const extensions = await ensureLanguage(languageId);
  if (!stillWanted()) return;
  view.dispatch({ effects: languageConf.reconfigure(extensions) });
}

/**
 * The same reconfiguration as `applyLanguage`, for a document that is not on
 * screen — a tab in a background pane, whose state is held rather than shown.
 */
export async function applyLanguageToState(state, languageId) {
  const extensions = await ensureLanguage(languageId);
  return state.update({ effects: languageConf.reconfigure(extensions) }).state;
}

export function createView(parent) {
  return new EditorView({ parent });
}

export { cursorPosition, countDiagnostics };
