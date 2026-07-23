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

// A low-arousal theme for sensory-sensitive users (autism, ADHD, migraine and
// visual-stress conditions), built from a consistent thread across accessible-
// design research: mute saturation rather than dim brightness, keep contrast
// moderate instead of extreme, and avoid red/yellow in favour of blue/green/
// lavender. Concretely:
//  - Neither pure black nor pure white: both read as harsh/glaring, and pure
//    #000 on #fff causes a halation effect where text seems to bleed. The
//    background is a warm, muted charcoal rather than near-black, and the
//    foreground a soft cream rather than stark white.
//  - Every hue is desaturated well below the vivid syntax colours the other
//    themes use (roughly half the saturation of one-dark's palette at a
//    similar lightness, so contrast against the background is unchanged but
//    nothing "vibrates").
//  - No red or yellow anywhere in the syntax palette, including for errors —
//    both are called out repeatedly as overstimulating/anxiety-inducing for
//    autistic users. Errors use a muted terracotta instead of alarm red;
//    search highlights use a muted gold wash instead of bright yellow.
//  - Warm neutrals (the palette leans brown/tan, not blue-grey) match the
//    "beige/cream/tan are calming" guidance, and functions/strings favour
//    soft sage green and dusty blue, both repeatedly named as calming hues.
const autismPalette = {
  background: "#2a2825",
  foreground: "#ddd6c9",
  caret: "#7fadd1",
  selection: "#3c4c54",
  gutterBackground: "#242220",
  gutterForeground: "#8c8577",
  activeLine: "#302d28",
  activeLineGutter: "#3a372f",
  border: "#46423a",
  panel: "#2f2d29",
};

const autismTheme = EditorView.theme(
  {
    "&": {
      color: autismPalette.foreground,
      backgroundColor: autismPalette.background,
    },
    ".cm-content": { caretColor: autismPalette.caret },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: autismPalette.caret },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: autismPalette.selection },
    ".cm-activeLine": { backgroundColor: autismPalette.activeLine },
    ".cm-selectionMatch": { backgroundColor: "#36423c" },
    "&.cm-focused .cm-matchingBracket": {
      backgroundColor: "#33424a",
      outline: "1px solid #5f93c4",
    },
    ".cm-gutters": {
      backgroundColor: autismPalette.gutterBackground,
      color: autismPalette.gutterForeground,
      border: "none",
      borderRight: `1px solid ${autismPalette.border}`,
    },
    ".cm-activeLineGutter": {
      backgroundColor: autismPalette.activeLineGutter,
      color: autismPalette.foreground,
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "#3a372f",
      border: `1px solid ${autismPalette.border}`,
      color: autismPalette.gutterForeground,
    },
    ".cm-tooltip": {
      backgroundColor: autismPalette.panel,
      border: `1px solid ${autismPalette.border}`,
      color: autismPalette.foreground,
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "#4f89ac",
      // Warm near-black rather than the theme's usual cream foreground —
      // cream-on-blue only reaches ~3.7:1, short of the 4.5:1 WCAG AA text
      // minimum; this pairing clears 4.7:1 while staying off pure black.
      color: "#1a1613",
    },
    ".cm-panels": {
      backgroundColor: autismPalette.panel,
      color: autismPalette.foreground,
      borderTop: `1px solid ${autismPalette.border}`,
    },
    // A muted gold wash rather than the usual bright yellow search highlight —
    // yellow is one of the two hues (with red) the research most consistently
    // flags as overstimulating.
    ".cm-searchMatch": { backgroundColor: "#4a4132" },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "#5f5238" },
  },
  { dark: true },
);

const autismHighlightStyle = HighlightStyle.define([
  { tag: [t.comment, t.blockComment, t.lineComment], color: "#8c8577", fontStyle: "italic" },
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.moduleKeyword], color: "#93a9c9" },
  { tag: [t.name, t.deleted, t.character, t.macroName], color: autismPalette.foreground },
  { tag: [t.variableName, t.propertyName], color: autismPalette.foreground },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "#9cb586" },
  { tag: [t.labelName], color: "#7fadd1" },
  { tag: [t.definition(t.name), t.separator], color: autismPalette.foreground },
  { tag: [t.typeName, t.className, t.namespace], color: "#c3ab82" },
  { tag: [t.number, t.integer, t.float, t.bool, t.null, t.atom], color: "#ab9bc4" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "#93b39c" },
  { tag: [t.escape], color: "#ab9bc4" },
  { tag: [t.operator, t.operatorKeyword, t.punctuation], color: "#a49c8c" },
  { tag: [t.meta, t.documentMeta], color: "#8c8577" },
  { tag: [t.tagName], color: "#7fadd1" },
  { tag: [t.attributeName], color: "#c3ab82" },
  { tag: [t.attributeValue], color: "#93b39c" },
  { tag: [t.heading], color: "#7fadd1", fontWeight: "bold" },
  { tag: [t.link, t.url], color: "#7fadd1", textDecoration: "underline" },
  { tag: [t.emphasis], fontStyle: "italic" },
  { tag: [t.strong], fontWeight: "bold" },
  { tag: [t.strikethrough], textDecoration: "line-through" },
  // Muted terracotta instead of alarm red, for the same reason as the search
  // highlight above — errors still need to read as distinct, not as vivid red.
  { tag: [t.invalid], color: "#bd8672" },
]);

export const THEMES = ["dark", "light", "autism"];

/** The CodeMirror extensions that make up one theme. */
export function themeExtensions(themeId) {
  if (themeId === "light") return [lightTheme, syntaxHighlighting(lightHighlightStyle)];
  if (themeId === "autism") return [autismTheme, syntaxHighlighting(autismHighlightStyle)];
  return [oneDark];
}
