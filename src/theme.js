import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { oneDark } from "@codemirror/theme-one-dark";
import { tags as t } from "@lezer/highlight";

// A light counterpart to one-dark. Written out rather than relying on
// CodeMirror's bare defaults so the gutters, selection and panels look
// deliberate instead of inheriting browser chrome.
const lightPalette = {
  background: "#ffffff",
  foreground: "#1f2328",
  caret: "#0969da",
  selection: "#b6dcff",
  gutterBackground: "#f6f8fa",
  gutterForeground: "#8c959f",
  activeLine: "#f6f8fa",
  activeLineGutter: "#eaeef2",
  border: "#d0d7de",
  panel: "#f6f8fa",
};

const lightTheme = EditorView.theme(
  {
    "&": {
      color: lightPalette.foreground,
      backgroundColor: lightPalette.background,
    },
    ".cm-content": { caretColor: lightPalette.caret },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: lightPalette.caret },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: lightPalette.selection },
    ".cm-activeLine": { backgroundColor: lightPalette.activeLine },
    ".cm-selectionMatch": { backgroundColor: "#e4ebf3" },
    "&.cm-focused .cm-matchingBracket": {
      backgroundColor: "#dbeafe",
      outline: "1px solid #93c5fd",
    },
    ".cm-gutters": {
      backgroundColor: lightPalette.gutterBackground,
      color: lightPalette.gutterForeground,
      border: "none",
      borderRight: `1px solid ${lightPalette.border}`,
    },
    ".cm-activeLineGutter": {
      backgroundColor: lightPalette.activeLineGutter,
      color: lightPalette.foreground,
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "#eaeef2",
      border: `1px solid ${lightPalette.border}`,
      color: "#57606a",
    },
    ".cm-tooltip": {
      backgroundColor: lightPalette.background,
      border: `1px solid ${lightPalette.border}`,
      color: lightPalette.foreground,
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "#0969da",
      color: "#ffffff",
    },
    ".cm-panels": {
      backgroundColor: lightPalette.panel,
      color: lightPalette.foreground,
      borderTop: `1px solid ${lightPalette.border}`,
    },
    ".cm-searchMatch": { backgroundColor: "#fff3c4" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "#ffd33d" },
  },
  { dark: false },
);

const lightHighlightStyle = HighlightStyle.define([
  { tag: [t.comment, t.blockComment, t.lineComment], color: "#6e7781", fontStyle: "italic" },
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.moduleKeyword], color: "#cf222e" },
  { tag: [t.name, t.deleted, t.character, t.macroName], color: "#1f2328" },
  { tag: [t.variableName, t.propertyName], color: "#1f2328" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "#8250df" },
  { tag: [t.labelName], color: "#0969da" },
  { tag: [t.definition(t.name), t.separator], color: "#1f2328" },
  { tag: [t.typeName, t.className, t.namespace], color: "#953800" },
  { tag: [t.number, t.integer, t.float, t.bool, t.null, t.atom], color: "#0550ae" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "#0a3069" },
  { tag: [t.escape], color: "#0550ae" },
  { tag: [t.operator, t.operatorKeyword, t.punctuation], color: "#0550ae" },
  { tag: [t.meta, t.documentMeta], color: "#6e7781" },
  { tag: [t.tagName], color: "#116329" },
  { tag: [t.attributeName], color: "#0550ae" },
  { tag: [t.attributeValue], color: "#0a3069" },
  { tag: [t.heading], color: "#0550ae", fontWeight: "bold" },
  { tag: [t.link, t.url], color: "#0969da", textDecoration: "underline" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.strong], fontWeight: "bold" },
  { tag: [t.strikethrough], textDecoration: "line-through" },
  { tag: [t.invalid], color: "#cf222e" },
]);

export const THEMES = ["dark", "light"];

/** The CodeMirror extensions that make up one theme. */
export function themeExtensions(themeId) {
  return themeId === "light"
    ? [lightTheme, syntaxHighlighting(lightHighlightStyle)]
    : [oneDark];
}
