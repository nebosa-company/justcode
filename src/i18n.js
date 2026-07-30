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
  "file.minimize": "Minimize",
  "file.exit": "Exit",

  "edit.cut": "Cut",
  "edit.copy": "Copy",
  "edit.paste": "Paste",
  "edit.selectAll": "Select All",
  "edit.deleteLine": "Delete Line",
  "edit.moveLineUp": "Move Line Up",
  "edit.moveLineDown": "Move Line Down",
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
  "view.autismTheme": "Autism Theme",
  "view.autismThemeHint": "Muted, low-contrast colours for sensory sensitivity",
  "view.bionicReading": "Bionic Reading",
  "view.problems": "Problems",
  "file.recentOpen": "already open",
  "file.reopen": "Reopen from Disk",
  "palette.title": "Commands",
  "palette.placeholder": "Type to search every command…",
  "palette.none": "No command matches.",
  "view.commandPalette": "Command Palette…",
  "view.nextProblem": "Next Problem",
  "view.previousProblem": "Previous Problem",
  "view.language": "Language…",
  "menu.harness": "Harness",
  "harness.progress": "Progress",
  "harness.refresh": "Refresh",
  "harness.noApprovals": "No pending approvals.",
  "harness.doneHint": "Journalled steps that finished — not batches or requirements. A requirement is usually one step plus a share of its batch's gate.",
  "harness.spendHint": "{total} spent so far, counted from what the models reported.",
  "harness.startItem": "Start…",
  "harness.startTitle": "Start a cycle",
  "harness.startMessage": "The harness works the backlog unattended, gates every batch, and commits what passes. It spends real money and stops at the budget in the binding.",
  "harness.batches": "Batches",
  "harness.items": "Items per batch",
  "harness.startTotal": "Up to {n} requirements — {b} batches of {i}.",
  "harness.start": "Start",
  "harness.started": "Cycle started. Progress appears in the panel as the journal grows.",
  "harness.noWorkspace": "Open a file in the project first — the harness needs a workspace with a binding.",
  "harness.tab.timeline": "Timeline",
  "harness.tab.chat": "Chat",
  "harness.tab.approvals": "Approvals",
  "harness.tab.diff": "Diff",
  "harness.tab.btw": "/btw",
  "harness.tab.artifacts": "Artifacts",
  "harness.hint.timeline": "Every step, newest first",
  "harness.hint.chat": "The conversation, and a note to the loop",
  "harness.hint.approvals": "Waiting on a person",
  "harness.hint.diff": "What the working tree has changed",
  "harness.hint.btw": "Asides waiting to be picked up",
  "harness.hint.artifacts": "Rendered from the journal",
  "harness.hint.refresh": "Re-read the journal now",
  "tabs.moveLeft": "Move Tab Left",
  "tabs.moveRight": "Move Tab Right",
  "tabs.moveToStart": "Move Tab to Beginning",

  "help.shortcuts": "Shortcuts",
  "help.autismTheme": "Autism Theme",
  "help.bionicReading": "Bionic Reading",
  "help.about": "About JustCode",

  "toolbar.new": "New",
  "toolbar.open": "Open",
  "toolbar.save": "Save",
  "toolbar.run": "Run",
  "toolbar.switchToTheme": "Switch to {theme}",

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
  "terminal.position": "Terminal Position",
  "terminal.dockLeft": "Left",
  "terminal.dockBottom": "Bottom",
  "terminal.dockRight": "Right",
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

  "autismHelp.intro":
    "A muted, low-arousal colour theme for sensory-sensitive users — autism, ADHD, migraine and other visual-stress conditions. It keeps the editor calm to look at over long stretches, without giving up readability.",
  "autismHelp.point1":
    "No pure black or pure white. A warm charcoal background and a soft cream foreground avoid both screen glare and the halation effect stark black-on-white contrast causes.",
  "autismHelp.point2":
    "No red or yellow anywhere — the two hues most consistently reported as overstimulating. Errors use a muted terracotta and search highlights a muted gold instead of the usual alarm red and bright yellow.",
  "autismHelp.point3":
    "Every colour is desaturated well below what the Dark and Light themes use, so nothing \"vibrates\" against its neighbours even though contrast against the background stays the same.",
  "autismHelp.point4":
    "Calming hues lead throughout: dusty blue and sage green for most syntax highlighting, muted lavender and tan for the rest.",
  "autismHelp.footer":
    "Switch to it any time from View ▸ Autism Theme, with the button below, or by pressing Ctrl+Shift+T to cycle Dark ▸ Light ▸ Autism.",
  "autismHelp.switch": "Switch to Autism Theme",
  "autismHelp.active": "This is your current theme.",

  "bionicHelp.intro":
    "A typography trick that bolds the leading part of each word, so the eye has fewer, shorter fixation points to land on per line. It's marketed as a reading aid for ADHD, dyslexia and sometimes autism.",
  "bionicHelp.point1":
    "Roughly the first 40% of each word is bolded and the rest stays regular weight — the idea being that the brain recognises a word from its start and fills in the ending from context.",
  "bionicHelp.point2":
    "The evidence is mixed. Several controlled studies found no measurable gain in reading speed or comprehension over ordinary text, and some readers find it more distracting, not less. Try it rather than assuming it will help.",
  "bionicHelp.point3":
    "Applies to whatever document is on screen, the same as Word Wrap — it isn't limited to prose, so turning it on while editing code bolds the start of identifiers and keywords too.",
  "bionicHelp.footer":
    "Switch it on any time from View ▸ Bionic Reading, with the button below, or by pressing Ctrl+Shift+B.",
  "bionicHelp.turnOn": "Turn On Bionic Reading",
  "bionicHelp.turnOff": "Turn Off Bionic Reading",

  // The Help & How-to centre: one topic per feature area, picked from a
  // sidebar. Autism Theme and Bionic Reading reuse the `autismHelp.*` /
  // `bionicHelp.*` strings above rather than duplicating them.
  "help.center": "Help & How-to",
  "modal.helpCenterTitle": "Help & How-to",
  "help.searchPlaceholder": "Search help…",
  "help.noResults": "No matching topics",

  "help.overviewTitle": "Overview",
  "help.overviewIntro":
    "JustCode is a small, fast editor for HTML, CSS, JavaScript and general text or code — built to open quickly and stay out of the way, not to replace a full IDE.",
  "help.overviewPoint1":
    "Files open as tabs in one window; there's no project or workspace to set up first.",
  "help.overviewPoint2": "Press F1 any time for the full keyboard shortcut reference.",

  "help.filesTitle": "Files",
  "help.filesIntro": "Files open as tabs; JustCode has no project or workspace concept.",
  "help.filesPoint1":
    "New File (Ctrl+N) offers starter templates for many languages, or a blank document; Open… (Ctrl+O) accepts several files at once.",
  "help.filesPoint2":
    "Save (Ctrl+S) and Save As (Ctrl+Shift+S) write straight to disk; Save All (Ctrl+Alt+S) covers every changed tab in one go.",
  "help.filesPoint3": "File ▸ Recent Files remembers the last 15 files across restarts.",
  "help.filesPoint4":
    "File ▸ File Associations… registers JustCode with Windows so it appears as an \"Open with\" option — or the default — for the file types it understands.",

  "help.editingTitle": "Editing",
  "help.editingIntro": "Standard editing, plus a few extras beyond cut, copy, paste, undo and redo.",
  "help.editingPoint1":
    "Ctrl+/ toggles a comment using whatever syntax the current language has — line comments for a single line, block comments across a selection.",
  "help.editingPoint2":
    "Ctrl+Shift+U / Ctrl+Shift+L change the selection's case; Ctrl+Alt+G inserts a fresh GUID.",
  "help.editingPoint3":
    "Alt+↑ / Alt+↓ moves the current line; Alt+Shift+↑ / Alt+Shift+↓ duplicates it up or down.",
  "help.editingPoint4": "Ctrl+click adds another cursor; Alt+drag makes a rectangular (column) selection.",

  "help.searchTitle": "Search",
  "help.searchIntro": "Find and replace inside the current file, or jump straight to a definition.",
  "help.searchPoint1":
    "Ctrl+F opens Find; Ctrl+H opens Find and Replace. F3 / Shift+F3 repeat the last search forward or backward.",
  "help.searchPoint2":
    "Ctrl+Shift+G opens Go to Symbol — a filterable list of the functions, classes and other declarations in the current file.",

  "help.bookmarksTitle": "Bookmarks",
  "help.bookmarksIntro":
    "Three numbered bookmark slots per document, for jumping around a large file without scrolling to find your place.",
  "help.bookmarksPoint1":
    "Ctrl+Shift+1 / 2 / 3 sets or clears a bookmark at the current line; Ctrl+1 / 2 / 3 jumps straight to it.",

  "help.splitTabsTitle": "Split View & Tabs",
  "help.splitTabsIntro": "Tabs can be arranged into up to four panes.",
  "help.splitTabsPoint1":
    "Ctrl+K then an arrow key splits the active pane in that direction; dragging a tab to a screen edge does the same.",
  "help.splitTabsPoint2":
    "Dragging a tab onto another pane's tab bar moves it there; dragging the last tab back out of a pane undoes the split.",
  "help.splitTabsPoint3": "Ctrl+Tab / Ctrl+Shift+Tab cycle through tabs; middle-click a tab to close it.",

  "help.foldingTitle": "Code Folding",
  "help.foldingIntro": "Any block can be collapsed to get it out of the way.",
  "help.foldingPoint1":
    "Click the chevron in the gutter, or press Ctrl+Shift+[ / ] to fold or unfold the block at the cursor.",
  "help.foldingPoint2": "Ctrl+Alt+[ / ] folds or unfolds everything in the file at once.",

  "help.terminalTitle": "Terminal",
  "help.terminalIntro": "An integrated terminal panel, not a separate window.",
  "help.terminalPoint1":
    "Ctrl+` shows or hides it; Ctrl+Shift+` opens a new one alongside any already running.",
  "help.terminalPoint2": "A new terminal starts in the folder of the file currently being edited.",
  "help.terminalPoint3":
    "File ▸ Run in Terminal (Ctrl+F5) runs the current script and leaves the shell open at a prompt afterwards, so a failure can be poked at on the spot.",

  "help.runTitle": "Run",
  "help.runIntro": "F5 shows the current file the way it will actually look or behave, rather than as source text.",
  "help.runPoint1":
    "HTML: opens in the default browser. Running the same file again refreshes that tab instead of opening a second one, as long as it's still open.",
  "help.runPoint2": "Markdown: rendered to a styled HTML document first, then previewed the same way.",
  "help.runPoint3":
    "PowerShell, Batch and Shell scripts: run in their own console window, which stays open after the script finishes.",

  "help.wordWrapIntro":
    "Off by default. Wrapping keeps long lines on screen without side-scrolling, but it breaks the one-row-per-line match between the gutter and the actual line numbers.",
  "help.wordWrapPoint1":
    "Toggle it from View ▸ Word Wrap or Alt+Z. It applies to whichever document is on screen.",

  "help.spellCheckIntro": "Off by default — most identifiers in source code are \"misspelled\" by definition.",
  "help.spellCheckPoint1":
    "Runs as a linter over the real text rather than the webview's built-in checker, so misspellings are counted in the Problems total and come with suggested corrections.",
  "help.spellCheckPoint2": "Toggle it from View ▸ Spell Check.",

  "help.languageTitle": "Interface Language",
  "help.languageIntro":
    "The language of JustCode's own menus and dialogs — independent of any file's content or programming language.",
  "help.languagePoint1":
    "View ▸ Language… lists every translation by its own name rather than its English name, since \"German\" is no help to someone who only reads Deutsch.",
  "help.languagePoint2": "English is built in; every other language downloads the first time it's selected.",

  // The shortcut reference. Key names (Ctrl, F5, Tab) are the same on every
  // Windows keyboard and stay untranslated; the gestures under `sc.k.` are
  // prose and do get translated, as do all the descriptions.
  "sc.g.menus": "Menus",
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

  "sc.openFileMenu": "Open the File menu",
  "sc.openEditMenu": "Open the Edit menu",
  "sc.openViewMenu": "Open the View menu",
  "sc.openHelpMenu": "Open the Help menu",

  "sc.newFile": "New file",
  "sc.openFile": "Open file",
  "sc.save": "Save",
  "sc.saveAs": "Save as",
  "sc.run": "Run — browser for HTML/Markdown, console for scripts",
  "sc.closeTab": "Close tab",
  "sc.closeAll": "Close all tabs",
  "sc.minimize": "Minimize the window",
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
  "sc.toolbar": "Toggle the toolbar",
  "sc.statusBar": "Toggle the status bar",
  "sc.wordWrap": "Toggle word wrap",
  "sc.spellCheck": "Toggle spell check",
  "sc.bionicReading": "Toggle Bionic Reading",
  "sc.cycleTheme": "Cycle themes (Dark ▸ Light ▸ Autism)",
  "sc.split": "Split up / down / left / right",
  "sc.contextMenu": "Context menu",
  "sc.terminal": "Show or hide the terminal",
  "sc.zoomInOut": "Zoom in / out",
  "sc.resetZoom": "Reset zoom",
  "sc.zoom": "Zoom",
  "sc.showProblems": "Show or hide problems",
  "sc.nextPrevProblem": "Next / previous problem",
  "sc.problemsPanel": "Open the problems panel",
  "sc.goToSymbol": "Go to symbol",
  "sc.shortcutList": "Help & How-to",

  "sc.nextPrevTab": "Next / previous tab",
  "sc.moveTab": "Move tab right / left",
  "sc.moveTabStart": "Move tab to the beginning",
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
