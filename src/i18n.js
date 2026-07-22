// Interface localisation.
//
// English is the source language and lives here, so the app always has a full
// string set even if a translation is missing a key or fails to load. Other
// locales are one lazily-imported module, keeping ~35 translations out of the
// startup bundle.

export const DEFAULT_LOCALE = "en";

/**
 * The languages offered, with their endonyms — a language picker that lists
 * "German" is no use to someone who only reads Deutsch.
 *
 * `rtl` marks the right-to-left scripts, which flip the whole layout.
 */
export const LOCALES = [
  { code: "en", name: "English" },
  { code: "ar", name: "العربية", rtl: true },
  { code: "bg", name: "Български" },
  { code: "zh-CN", name: "简体中文" },
  { code: "zh-TW", name: "繁體中文" },
  { code: "hr", name: "Hrvatski" },
  { code: "cs", name: "Čeština" },
  { code: "da", name: "Dansk" },
  { code: "nl", name: "Nederlands" },
  { code: "fi", name: "Suomi" },
  { code: "fr", name: "Français" },
  { code: "de", name: "Deutsch" },
  { code: "el", name: "Ελληνικά" },
  { code: "he", name: "עברית", rtl: true },
  { code: "hi", name: "हिन्दी" },
  { code: "hu", name: "Magyar" },
  { code: "id", name: "Indonesia" },
  { code: "it", name: "Italiano" },
  { code: "ja", name: "日本語" },
  { code: "ko", name: "한국어" },
  { code: "ms", name: "Melayu" },
  { code: "no", name: "Norsk" },
  { code: "fa", name: "فارسی", rtl: true },
  { code: "pl", name: "Polski" },
  { code: "pt", name: "Português" },
  { code: "ro", name: "Română" },
  { code: "ru", name: "Русский" },
  { code: "sr", name: "Српски" },
  { code: "sk", name: "Slovenčina" },
  { code: "es", name: "Español" },
  { code: "sv", name: "Svenska" },
  { code: "th", name: "ไทย" },
  { code: "tr", name: "Türkçe" },
  { code: "uk", name: "Українська" },
  { code: "ur", name: "اردو", rtl: true },
  { code: "vi", name: "Tiếng Việt" },
];

/** English strings — the keys every other locale is measured against. */
export const EN = {
  "menu.file": "File",
  "menu.edit": "Edit",
  "menu.view": "View",
  "menu.help": "Help",

  "file.new": "New File",
  "file.open": "Open…",
  "file.save": "Save",
  "file.saveAs": "Save As…",
  "file.revealWindows": "Show in Explorer",
  "file.revealMac": "Show in Finder",
  "file.revealLinux": "Show in File Manager",
  "file.copyPath": "Copy File Path",
  "file.run": "Run",
  "file.runInTerminal": "Run in Terminal",
  "file.closeTab": "Close Tab",
  "file.closeOthers": "Close All But Current",
  "file.closeAll": "Close All",
  "file.exit": "Exit",

  "edit.cut": "Cut",
  "edit.copy": "Copy",
  "edit.paste": "Paste",
  "edit.selectAll": "Select All",
  "edit.deleteLine": "Delete Line",
  "edit.toggleComment": "Toggle Comment",
  "edit.uppercase": "Uppercase",
  "edit.lowercase": "Lowercase",
  "edit.guid": "Generate GUID",
  "edit.sortCaseSensitive": "Sort Case-Sensitive",
  "edit.toggleBookmark": "Toggle Bookmark",
  "edit.gotoBookmark": "Go to Bookmark {n}",

  "view.zoomIn": "Zoom In",
  "view.zoomOut": "Zoom Out",
  "view.resetZoom": "Reset Zoom",
  "view.toolbar": "Toolbar",
  "view.statusBar": "Status Bar",
  "view.spellCheck": "Spell Check",
  "view.darkTheme": "Dark Theme",
  "view.lightTheme": "Light Theme",
  "view.problems": "Problems",
  "view.language": "Language…",

  "help.shortcuts": "Shortcuts",
  "help.about": "About JustCode",

  "toolbar.new": "New",
  "toolbar.open": "Open",
  "toolbar.save": "Save",
  "toolbar.run": "Run",

  "status.noFile": "No file",
  "status.unsaved": "unsaved",
  "status.problems": "{n} problems",
  "status.problem": "1 problem",
  "status.spelling": "{n} spelling",
  "status.pathCopied": "Path copied",
  "status.selectLanguage": "Select language mode",
  "status.lineColumn": "Line, Column",

  "tabs.empty": "No files open — press Ctrl+O to open, Ctrl+N for a new file",
  "tabs.newFileHint": "Double-click for a new file",

  "dialog.unsaved": '"{name}" has unsaved changes. Close without saving?',
  "dialog.discard": "Discard",
  "dialog.cancel": "Cancel",
  "dialog.saveTitle": "Unsaved changes",
  "dialog.saveMessage": "{n} file(s) have unsaved changes. Save them before closing?",
  "dialog.save": "Save",
  "dialog.dontSave": "Don't Save",
  "dialog.andMore": "and {n} more",

  "newFile.title": "New file",
  "newFile.blank": "Blank document",
  "newFile.blankHint": "Empty, no template",
  "newFile.filter": "Type to search file types…",
  "newFile.favourite": "Add to favourites",
  "newFile.unfavourite": "Remove from favourites",

  "file.saveAll": "Save All",
  "edit.undo": "Undo",
  "edit.redo": "Redo",
  "view.wordWrap": "Word Wrap",
  "view.terminal": "Terminal",
  "terminal.new": "New terminal",
  "terminal.newIn": "New Terminal",
  "terminal.close": "Close terminal",
  "terminal.hide": "Hide panel",
  "terminal.exited": "[process exited]",
  "terminal.asAdmin": "{name} as Administrator",
  "view.splitUp": "Split Up",
  "view.splitDown": "Split Down",
  "view.splitLeft": "Split Left",
  "view.splitRight": "Split Right",

  "edit.find": "Find…",
  "edit.replace": "Replace…",
  "edit.findNext": "Find Next",
  "edit.findPrevious": "Find Previous",
  "edit.goToSymbol": "Go to Symbol…",
  "file.recent": "Recent Files",
  "file.recentEmpty": "No recent files",
  "file.associations": "File Associations…",

  "modal.symbolsTitle": "Go to symbol",
  "modal.symbolFilter": "Filter symbols…",
  "modal.symbolCount": "{n} result(s) found",
  "modal.noSymbols": "No symbols found in this file",
  "modal.associationsTitle": "File associations",
  "assoc.selectAll": "Select All",
  "assoc.unselectAll": "Unselect All",
  "assoc.apply": "Apply",
  "assoc.intro": "Choose which file types open in JustCode.",
  "assoc.defaultsNote":
    "Ticked types will open in JustCode. Types another program already handles are marked and left unticked. Where Windows has recorded your own choice for a type, that choice stays and JustCode is only added to the \"Open with\" list.",
  "assoc.currently": "— currently {owner}",
  "assoc.applied": "{n} file type(s) associated",
  "assoc.someFailed": "Some file types could not be registered:",
  "assoc.userChoiceBlocked":
    "Windows keeps its own default app for {list}, and only you can change that — an application is not allowed to.",
  "assoc.openSettings": "Open Windows default-app settings now?",
  "assoc.openSettingsOk": "Open Settings",
  "modal.shortcutsTitle": "Keyboard shortcuts",
  "modal.aboutTitle": "About JustCode",
  "modal.languageTitle": "Language",
  "modal.close": "Close (Escape)",
  "about.tagline": "A small, fast code editor.",
  "about.builtWith": "Built with Tauri 2 and CodeMirror 6.",
  "about.version": "Version {version}",

  // The shortcut reference. Key names (Ctrl, F5, Tab) are the same on every
  // Windows keyboard and stay untranslated; the gestures under `sc.k.` are
  // prose and do get translated, as do all the descriptions.
  "sc.g.file": "File",
  "sc.g.editing": "Editing",
  "sc.g.bookmarks": "Bookmarks",
  "sc.g.search": "Search",
  "sc.g.moving": "Moving around",
  "sc.g.selecting": "Selecting",
  "sc.g.folding": "Folding",
  "sc.g.view": "View",
  "sc.g.tabs": "Tabs and panes",
  "sc.g.links": "Links and files",

  "sc.k.shiftMotion": "Shift + any motion",
  "sc.k.dblTripleClick": "Double-click / triple-click",
  "sc.k.altClick": "Alt+click",
  "sc.k.altDrag": "Alt+drag",
  "sc.k.gutterChevron": "Click the gutter chevron",
  "sc.k.ctrlWheel": "Ctrl+Wheel",
  "sc.k.middleClickTab": "Middle-click a tab",
  "sc.k.dragToEdge": "Drag a tab to an edge",
  "sc.k.dragToTabBar": "Drag a tab to a tab bar",
  "sc.k.dblClickTabBar": "Double-click empty tab bar",
  "sc.k.rightClick": "Right-click in the editor",
  "sc.k.wordWrap": "View ▸ Word Wrap",
  "sc.k.ctrlClickUrl": "Ctrl+click a URL",
  "sc.k.clickPath": "Click the path in the status bar",
  "sc.k.clickLanguage": "Click the language in the status bar",

  "sc.newFile": "New file",
  "sc.openFile": "Open file",
  "sc.save": "Save",
  "sc.saveAs": "Save as",
  "sc.run": "Run — browser for HTML/Markdown, console for scripts",
  "sc.closeTab": "Close tab",
  "sc.closeAll": "Close all tabs",
  "sc.exit": "Exit",

  "sc.cutCopyPaste": "Cut / copy / paste",
  "sc.selectAll": "Select all",
  "sc.undo": "Undo",
  "sc.redo": "Redo",
  "sc.undoSelection": "Undo / redo a selection change",
  "sc.deleteLine": "Delete line",
  "sc.toggleComment": "Toggle comment",
  "sc.upperLower": "Uppercase / lowercase",
  "sc.guid": "Generate GUID",
  "sc.indentOutdent": "Indent / outdent",
  "sc.indentMoreLess": "Indent more / less",
  "sc.newLineIndent": "New line, keeping indentation",
  "sc.deleteWord": "Delete word before / after",
  "sc.moveLine": "Move line up / down",
  "sc.copyLine": "Copy line up / down",
  "sc.completion": "Trigger completion",

  "sc.toggleBookmark": "Toggle bookmark 1 / 2 / 3",
  "sc.gotoBookmark": "Go to bookmark 1 / 2 / 3",

  "sc.find": "Find",
  "sc.replace": "Find and replace",
  "sc.nextPrevMatch": "Next / previous match",
  "sc.nextMatchBar": "Next match (from the find bar)",
  "sc.closeFindBar": "Close the find bar",

  "sc.byCharLine": "By character / line",
  "sc.byWord": "By word",
  "sc.lineStartEnd": "Line start / end",
  "sc.docStartEnd": "Document start / end",
  "sc.byPage": "By page",
  "sc.matchingBracket": "Jump to the matching bracket",

  "sc.extendSelection": "Extend the selection",
  "sc.extendByWord": "Extend by word",
  "sc.selectToLineEdge": "Select to line start / end",
  "sc.selectByPage": "Select by page",
  "sc.selectLine": "Select the current line",
  "sc.growSelection": "Grow the selection to the enclosing syntax",
  "sc.collapseCursor": "Collapse to a single cursor",
  "sc.selectWordLine": "Select word / line",
  "sc.addCursor": "Add another cursor",
  "sc.rectSelection": "Rectangular (column) selection",

  "sc.fold": "Fold",
  "sc.unfold": "Unfold",
  "sc.foldAll": "Fold all",
  "sc.unfoldAll": "Unfold all",
  "sc.foldUnfold": "Fold / unfold",

  "sc.saveAll": "Save all",
  "sc.wordWrap": "Toggle word wrap",
  "sc.split": "Split up / down / left / right",
  "sc.contextMenu": "Context menu",
  "sc.terminal": "Show or hide the terminal",
  "sc.zoomInOut": "Zoom in / out",
  "sc.resetZoom": "Reset zoom",
  "sc.zoom": "Zoom",
  "sc.showProblems": "Show problems",
  "sc.problemsPanel": "Open the problems panel",
  "sc.goToSymbol": "Go to symbol",
  "sc.shortcutList": "This shortcut list",

  "sc.nextPrevTab": "Next / previous tab",
  "sc.moveTab": "Move tab right / left",
  "sc.closeIt": "Close it",
  "sc.splitView": "Split the view",
  "sc.moveOrUnsplit": "Move it there / undo the split",

  "sc.openInBrowser": "Open it in the browser",
  "sc.openLinkAtCaret": "Open the link under the caret",
  "sc.copyFullPath": "Copy the full path",
  "sc.changeMode": "Change the language mode",
};

let current = DEFAULT_LOCALE;
let strings = EN;
let generation = 0;
const listeners = new Set();

/** Translate `key`, filling `{placeholders}`; falls back to English. */
export function t(key, params) {
  let text = strings[key] ?? EN[key] ?? key;
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      // A function replacement, so `$&` or `$\`` in the value — both legal in a
      // file name — are inserted literally instead of being treated as
      // replacement patterns.
      text = text.replaceAll(`{${name}}`, () => String(value));
    }
  }
  return text;
}

export function currentLocale() {
  return current;
}

export function isRtl(code = current) {
  return Boolean(LOCALES.find((locale) => locale.code === code)?.rtl);
}

/** Runs `fn` whenever the language changes, so the UI can redraw itself. */
export function onLocaleChange(fn) {
  listeners.add(fn);
}

/**
 * Switches language. English needs no download; everything else comes from one
 * lazily-loaded module. A failed load keeps the current language rather than
 * leaving the interface half-translated.
 */
export async function setLocale(code) {
  const known = LOCALES.some((locale) => locale.code === code);
  const target = known ? code : DEFAULT_LOCALE;

  // English applies synchronously while every other locale waits on an import,
  // so a later choice can otherwise be overtaken by an earlier one still in
  // flight. Each call takes a ticket and stands down if it is no longer newest.
  const ticket = ++generation;

  if (target === DEFAULT_LOCALE) {
    strings = EN;
  } else {
    try {
      const { TRANSLATIONS } = await import("./locales.js");
      if (ticket !== generation) return current;
      // Missing keys fall through to English rather than showing raw key names.
      strings = { ...EN, ...(TRANSLATIONS[target] || {}) };
    } catch {
      return current;
    }
  }

  current = target;
  document.documentElement.lang = target;
  document.documentElement.dir = isRtl(target) ? "rtl" : "ltr";
  for (const fn of listeners) fn();
  return current;
}
