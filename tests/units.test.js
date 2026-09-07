// Unit tests for the modules that need neither a DOM nor Tauri.
//
// The choice of what to test here is the interesting part. Each case below is a
// rule with a stated reason in the source — a dot in a parent directory, a
// dotfile with no extension, `$&` in a replacement — rather than a restatement
// of whatever the code happens to do.

import { test } from "node:test";
import assert from "node:assert/strict";

import { languageIdFor, primaryExtension, languageList } from "../src/languages.js";
import { findSymbols, supportsSymbols } from "../src/symbols.js";
import { t, EN } from "../src/i18n.js";
import { iconMarkup, PATHS } from "../src/icons.js";
import { blocks, spans } from "../src/replytext.js";
import { decideReload } from "../src/ondisk.js";
import { renderMarkdownDocument } from "../src/markdown.js";
import { formatAccel, isLetter } from "../src/shortcuts.js";
import { StringStream } from "@codemirror/language";
import { neper } from "../src/neper.js";
import { intelAsm } from "../src/intel-asm.js";
import { isNewer, installerFor } from "../src/update.js";
import {
  fileIconId,
  isInside,
  nextTypeAhead,
  pathKey,
  relativePath,
  sortEntries,
  uniqueName,
  validateName,
  visibleRows,
} from "../src/filetree.js";
import { readFileSync, readdirSync } from "node:fs";

test("a file's language comes from its own extension, not its path", () => {
  assert.equal(languageIdFor("a.py"), "python");
  assert.equal(languageIdFor("C:\\Users\\me\\a.py"), "python");
  assert.equal(languageIdFor("/home/me/a.py"), "python");
  assert.equal(languageIdFor("A.PY"), "python", "extensions are matched case-insensitively");
  assert.equal(languageIdFor("boot.S"), "assembly", "preprocessed assembly is spelled .S");

  // The reason the source reduces to a basename first: a dot in a directory
  // name must not be read as the file's extension.
  assert.equal(languageIdFor("/home/me/project.rs/notes"), "text");
  assert.equal(languageIdFor("v1.2/README"), "text");
});

test("a name with no extension is text, and so is a dotfile", () => {
  assert.equal(languageIdFor("Makefile"), "text");
  assert.equal(languageIdFor(".gitignore"), "text", "a leading dot is not an extension");
  assert.equal(languageIdFor(""), "text");
  assert.equal(languageIdFor(null), "text", "a tab with no path must not throw");
});

test("every language round-trips through its primary extension", () => {
  const broken = [];
  for (const { id } of languageList()) {
    const extension = primaryExtension(id);
    const back = languageIdFor(`file.${extension}`);
    if (back !== id) broken.push(`${id} -> .${extension} -> ${back}`);
  }
  // New File offers a language, builds a name from this extension, and the tab
  // then re-detects its language from that name. A mismatch means the file you
  // asked for opens as something else.
  assert.deepEqual(broken, [], `New File would mis-detect:\n${broken.join("\n")}`);
});

test("symbols come back in document order", () => {
  const source = ["def second():", "    pass", "", "def first():", "    pass", ""].join("\n");
  const found = findSymbols(source, "python");
  assert.deepEqual(
    found.map((s) => s.name),
    ["second", "first"],
    "sorted by position, not by name or by rule",
  );
  assert.ok(found[0].pos < found[1].pos);
});

test("a language with no rules returns nothing rather than throwing", () => {
  assert.equal(supportsSymbols("yaml"), false);
  assert.deepEqual(findSymbols("anything: here", "yaml"), []);
  assert.deepEqual(findSymbols("", "a-language-that-does-not-exist"), []);
});

test("a translation falls back to the key rather than to blank", () => {
  assert.equal(t("menu.file"), EN["menu.file"]);
  // Better a visible key than an empty menu item: the label is how you find the
  // missing entry.
  assert.equal(t("no.such.key"), "no.such.key");
});

test("interpolation inserts a value literally, patterns and all", () => {
  const key = Object.keys(EN).find((name) => EN[name].includes("{name}"));
  assert.ok(key, "the fixture needs a real key that interpolates a name");

  // `$&` and `` $` `` are legal in a Windows file name and are replacement
  // patterns to `replaceAll` — the source uses a function replacement for
  // exactly this reason.
  const rendered = t(key, { name: "a$&b$`c" });
  assert.ok(rendered.includes("a$&b$`c"), `inserted literally: ${rendered}`);
  assert.ok(!rendered.includes("{name}"), "and the placeholder is gone");
});

test("an icon is an svg, and an unknown one fails loudly", () => {
  const svg = iconMarkup("brain");
  assert.match(svg, /^<svg /);
  assert.match(svg, /viewBox="0 0 24 24"/);
  assert.match(svg, /aria-hidden="true"/, "decorative: the button's own text names it");
  assert.throws(() => iconMarkup("no-such-icon"), /Unknown icon/);
});

test("every icon defines path data", () => {
  const empty = Object.keys(PATHS).filter((name) => !PATHS[name].trim().startsWith("<"));
  assert.deepEqual(empty, [], `an icon with no geometry renders as a blank box: ${empty}`);
});

test("markdown output escapes the document it was given", () => {
  const html = renderMarkdownDocument("A < B & C", "notes & more");
  assert.ok(!html.includes("A < B & C"), "raw angle brackets would open a tag");
  assert.ok(html.includes("&lt;") || html.includes("&amp;"), `something was escaped: ${html}`);
  assert.match(html, /<\/html>/, "and it is a whole document");
});

// ---------------------------------------------------------------- reply text

test("a fenced block keeps its text and is never read as markup", () => {
  const out = blocks("before\n```py\n**not bold** <img onerror=x>\n```\nafter");
  const code = out.find((b) => b.kind === "code");
  assert.equal(code.language, "py");
  assert.equal(code.text, "**not bold** <img onerror=x>");
  assert.equal(out.filter((b) => b.kind === "para").length, 2, "the prose either side survives");
});

test("headings, lists and quotes come back as what they are", () => {
  const out = blocks("### Key fields\n- one\n- two\n\n> a remark\n\n---");
  assert.deepEqual(out.map((b) => b.kind), ["heading", "list", "quote", "rule"]);
  assert.equal(out[0].level, 3);
  assert.equal(out[1].ordered, false);
  assert.equal(out[1].items.length, 2);
});

test("a numbered list is ordered and keeps its items", () => {
  const out = blocks("1. first\n2. second");
  assert.equal(out[0].kind, "list");
  assert.equal(out[0].ordered, true);
  assert.equal(out[0].items.length, 2);
});

test("inline spans split at the earliest mark, not the first pattern listed", () => {
  // `em` is checked after `strong`; without ordering by position, `a *b* c
  // **d**` would split at the bold and lose the italic in the leading text.
  const out = spans("a *b* c **d**");
  assert.deepEqual(
    out.map((s) => s.kind),
    ["text", "em", "text", "strong"],
  );
  assert.equal(out[1].text, "b");
  assert.equal(out[3].text, "d");
});

test("what is inside backticks is taken literally", () => {
  const out = spans("use `**stars**` for bold");
  const code = out.find((s) => s.kind === "code");
  assert.equal(code.text, "**stars**", "the asterisks are content, not markup");
  assert.ok(!out.some((s) => s.kind === "strong"), "and nothing in there was made bold");
});

test("a link keeps its address apart from its text", () => {
  const [span] = spans("[the docs](https://example.com/x)");
  assert.equal(span.kind, "link");
  assert.equal(span.text, "the docs");
  assert.equal(span.href, "https://example.com/x");

  // An address with a balanced pair in it survives whole. Stopping at the
  // first bracket cut a Wikipedia link in half and left the rest in the
  // sentence.
  const [wiki] = spans("[Foo](https://en.wikipedia.org/wiki/Foo_(bar))");
  assert.equal(wiki.href, "https://en.wikipedia.org/wiki/Foo_(bar)");
});

test("nothing in a reply ever becomes markup", () => {
  // The whole reason this module exists rather than a Markdown-to-HTML one.
  const nasty = "<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[x](javascript:alert(1))";
  const out = blocks(nasty);
  const flat = JSON.stringify(out);
  // The dangerous text is still present — it is displayed, not executed —
  // but it is only ever in a `text` or `href` field, never a markup string.
  assert.ok(flat.includes("script"), "the characters survive to be shown");
  for (const block of out) {
    assert.ok(["para", "code", "heading", "list", "quote", "rule"].includes(block.kind));
  }
  const link = out.flatMap((b) => b.spans || []).find((s) => s.kind === "link");
  assert.equal(link.href, "javascript:alert(1)", "kept as data, and rendered as text only");
});

test("an empty or absent reply is no blocks rather than a crash", () => {
  assert.deepEqual(blocks(""), []);
  assert.deepEqual(blocks(null), []);
  assert.deepEqual(spans(""), []);
});

// ------------------------------------------------ a file changing under a tab

test("an untouched buffer takes what is on disk, without asking", () => {
  assert.equal(
    decideReload({ diskText: "new", savedText: "old", modified: false }),
    "reload",
  );
});

test("a buffer with edits in it asks before anything is lost", () => {
  assert.equal(
    decideReload({ diskText: "new", savedText: "old", modified: true }),
    "ask",
  );
});

test("the editor's own save is not a change to react to", () => {
  // The watcher reports size and modified time, both of which a save moves.
  // Without this every save would reload the tab it just came from.
  assert.equal(
    decideReload({ diskText: "same", savedText: "same", modified: false }),
    "ignore",
  );
  // And with edits on top of a save, it is still not an external change.
  assert.equal(
    decideReload({ diskText: "same", savedText: "same", modified: true }),
    "ignore",
  );
});

test("declining once is not answered by being asked again", () => {
  const state = { diskText: "theirs", savedText: "old", modified: true };
  assert.equal(decideReload(state), "ask");
  assert.equal(decideReload({ ...state, declined: "theirs" }), "ignore");
});

test("a further change is a new question, not the one already declined", () => {
  // Something wrote again, not knowing about the buffer either. Staying quiet
  // because a *previous* version was declined would hide the second write.
  assert.equal(
    decideReload({
      diskText: "theirs, again",
      savedText: "old",
      modified: true,
      declined: "theirs",
    }),
    "ask",
  );
});

test("a declined version that later matches the buffer's save stops mattering", () => {
  // Saving over it makes the disk agree with the tab, which outranks any
  // memory of having declined something.
  assert.equal(
    decideReload({
      diskText: "mine",
      savedText: "mine",
      modified: false,
      declined: "theirs",
    }),
    "ignore",
  );
});

test("an emptied file is still a change worth reacting to", () => {
  // Truncation is a real edit and must not be mistaken for nothing.
  assert.equal(
    decideReload({ diskText: "", savedText: "something", modified: false }),
    "reload",
  );
  assert.equal(
    decideReload({ diskText: "", savedText: "something", modified: true }),
    "ask",
  );
});

test("a Mac draws a shortcut in glyphs, in the order every other Mac menu uses", () => {
  // Not a preference. Control, Option, Shift, Command, then the key — a menu
  // that prints ⌘⇧S among applications printing ⇧⌘S reads as a port.
  assert.equal(formatAccel("Ctrl+Shift+S", true), "⇧⌘S");
  assert.equal(formatAccel("Ctrl+Alt+G", true), "⌥⌘G");
  assert.equal(formatAccel("Ctrl+N", true), "⌘N");
  // `Ctrl` in a declaration means the command modifier, which is what the key
  // handler has always read it as (`event.ctrlKey || event.metaKey`).
  assert.equal(formatAccel("Ctrl+Backspace", true), "⌘⌫");
  // Alternatives keep their separator.
  assert.equal(formatAccel("Ctrl+Y / Ctrl+Shift+Z", true), "⌘Y / ⇧⌘Z");
});

test("everywhere else the same declaration is left exactly as written", () => {
  for (const spec of ["Ctrl+N", "Ctrl+Shift+S", "Alt+F", "Ctrl+Backspace", "F5"]) {
    assert.equal(formatAccel(spec, false), spec);
  }
});

test("a shortcut with nothing to translate survives both platforms", () => {
  // Function keys, and the prose entries the reference list carries.
  assert.equal(formatAccel("F5", true), "F5");
  assert.equal(formatAccel("", true), "");
  assert.equal(formatAccel("Shift+F4", true), "⇧F4");
});

test("both halves of a chord are translated, not just the first", () => {
  // `Ctrl+K Ctrl+O` split on "+" puts "K Ctrl" in the middle, which is not a
  // modifier, so the formatter used to give up and hand back the Windows
  // spelling — leaving one item on the File menu saying "Ctrl" out loud on a
  // Mac while every other item showed glyphs.
  assert.equal(formatAccel("Ctrl+K Ctrl+O", true), "⌘K ⌘O");
  assert.equal(formatAccel("Ctrl+K Ctrl+O", false), "Ctrl+K Ctrl+O");
  // The chords that already worked must keep working: a modifier followed by a
  // bare key, and the same with alternatives after it.
  assert.equal(formatAccel("Ctrl+K ←", true), "⌘K ←");
  assert.equal(formatAccel("Ctrl+K ↑ / ↓", true), "⌘K ↑ / ↓");
  // A row of bare keys is not a chord and must come back untouched.
  assert.equal(formatAccel("← → ↑ ↓", true), "← → ↑ ↓");
});

test("a shortcut whose key is the separator still translates", () => {
  // `Ctrl++` splits into ["Ctrl", "", ""] because "+" is both the separator and
  // the key, so the formatter bailed and Zoom In was the one item left in the
  // View menu reading "Ctrl++" on a Mac while its neighbours showed glyphs.
  assert.equal(formatAccel("Ctrl++", true), "⌘+");
  assert.equal(formatAccel("Ctrl++", false), "Ctrl++");
  assert.equal(formatAccel("Ctrl++ / Ctrl+-", true), "⌘+ / ⌘-");
  // The ordinary case must not have moved.
  assert.equal(formatAccel("Ctrl+-", true), "⌘-");
  assert.equal(formatAccel("Ctrl+0", true), "⌘0");
});

test("a Mac is offered the delete key it actually has", () => {
  // The key a Mac labels "delete" is the one Windows calls Backspace. `Delete`
  // is forward-delete, which most Mac laptops cannot press without Fn — so a
  // tree that named it would advertise a key half its users do not have.
  assert.equal(formatAccel("Backspace", true), "⌫");
  assert.equal(formatAccel("Delete", true), "⌦");
  assert.equal(formatAccel("Del", false), "Del");
});

test("an Option shortcut matches the key that was pressed, not the character it typed", () => {
  // The bug this replaced: on a Mac ⌥M types `µ`, so `event.key === "m"` was
  // never true and six shortcuts in this app simply did not exist there.
  assert.equal(isLetter({ key: "µ", code: "KeyM" }, "m"), true);
  assert.equal(isLetter({ key: "ß", code: "KeyS" }, "s"), true);
  // The character still counts, for a layout where the letter has moved.
  assert.equal(isLetter({ key: "m", code: "Semicolon" }, "m"), true);
  // And a different key is still a different key.
  assert.equal(isLetter({ key: "x", code: "KeyX" }, "m"), false);
});

test("a version is compared as numbers, not as text", () => {
  // The one that matters: 0.2.10 is newer than 0.2.9, which a string compare
  // reads backwards and would leave everyone stuck on .9 forever.
  assert.equal(isNewer("0.2.10", "0.2.9"), true);
  assert.equal(isNewer("v0.3.0", "0.2.9"), true, "the tag's leading v is not part of the number");
  assert.equal(isNewer("0.2.6", "0.2.6"), false);
  assert.equal(isNewer("0.2.5", "0.2.6"), false);
  assert.equal(isNewer("0.2.6-beta", "0.2.6"), false, "a suffix is not an upgrade");
});

test("each platform is offered the installer it can actually run", () => {
  // Asset names as the release workflow publishes them.
  const assets = [
    { name: "JustCode_0.2.6_amd64.AppImage" },
    { name: "JustCode_0.2.6_amd64.deb" },
    { name: "JustCode_0.2.6_universal.dmg" },
    { name: "JustCode_0.2.6_x64-setup.exe" },
    { name: "JustCode_0.2.6_x64_en-US.msi" },
  ];
  const pick = (ua) => installerFor(assets, ua)?.name;
  // Windows takes the NSIS setup over the .msi sitting next to it.
  assert.equal(pick("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"), "JustCode_0.2.6_x64-setup.exe");
  assert.equal(pick("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15)"), "JustCode_0.2.6_universal.dmg");
  assert.equal(pick("Mozilla/5.0 (X11; Linux x86_64)"), "JustCode_0.2.6_amd64.AppImage");
  assert.equal(pick("Mozilla/5.0 (PlayStation 5)"), undefined, "an unknown platform offers nothing");
});

// ------------------------------------------------------------------ explorer

test("folders come before files, and names sort the way a person counts", () => {
  const sorted = sortEntries([
    { name: "file10.txt", isDir: false },
    { name: "zebra", isDir: true },
    { name: "file2.txt", isDir: false },
    { name: "Apple.txt", isDir: false },
    { name: "apples", isDir: true },
  ]).map((entry) => entry.name);

  assert.deepEqual(sorted.slice(0, 2), ["apples", "zebra"], "every folder comes first");
  // The one that matters: a plain string compare puts file10 before file2,
  // which is how every hand-rolled file list gets numbering wrong.
  assert.deepEqual(sorted.slice(2), ["Apple.txt", "file2.txt", "file10.txt"]);
});

test("a folder is not inside a sibling whose name merely starts the same way", () => {
  // The boundary a bare startsWith gets wrong, and the reason drag-and-drop
  // needs a real check: C:\ab starts with C:\a and is nowhere near it.
  assert.equal(isInside("C:\\a", "C:\\ab"), false);
  assert.equal(isInside("C:\\a", "C:\\a\\b"), true);
  // Separator and case both vary on Windows and neither changes the answer.
  assert.equal(isInside("C:/a", "c:\\A\\b"), true);
  assert.equal(isInside("C:\\a", "C:\\a"), false, "a folder is not inside itself");
  assert.equal(isInside("C:\\a\\", "C:\\a\\b"), true, "a trailing separator is not a difference");
});

test("a path under the root loses the root and keeps its separator", () => {
  assert.equal(relativePath("C:\\p", "C:\\p\\src\\main.js"), "src\\main.js");
  assert.equal(relativePath("C:\\p", "C:\\p"), "", "the root itself is empty, not a dot");
  assert.equal(
    relativePath("C:\\p", "D:\\elsewhere\\x.txt"),
    "D:\\elsewhere\\x.txt",
    "something outside the root is returned whole rather than mangled"
  );
});

test("a name Windows refuses is refused before the write is attempted", () => {
  const siblings = ["taken.txt", "Folder"];
  const key = (name, options) => validateName(name, siblings, options)?.key ?? null;

  assert.equal(key("ordinary.txt"), null);
  assert.equal(key(""), "explorer.nameEmpty");
  assert.equal(key("   "), "explorer.nameEmpty");
  assert.equal(key("a/b"), "explorer.nameSeparator");
  assert.equal(key("a\\b"), "explorer.nameSeparator");
  assert.equal(key("a:b"), "explorer.nameInvalidChars");
  assert.equal(key("a?b"), "explorer.nameInvalidChars");
  // Reserved device names are refused with an extension too: CON.txt is as
  // unopenable as CON, which is why the stem is what gets checked.
  assert.equal(key("CON"), "explorer.nameReserved");
  assert.equal(key("com1.txt"), "explorer.nameReserved");
  assert.equal(key("console.txt"), null, "only the exact device names, not anything starting with one");
  // Windows silently strips these, so the file you get is not the one you asked
  // for.
  assert.equal(key("x."), "explorer.nameTrailing");
  assert.equal(key("x "), "explorer.nameTrailing");
  // A duplicate that differs only in case is still a duplicate on NTFS.
  assert.equal(key("TAKEN.TXT"), "explorer.nameExists");
  // Except when renaming a file to a different casing of its own name, which is
  // a rename people actually do and which the naive check refuses.
  assert.equal(key("Taken.TXT", { self: "taken.txt" }), null);
});

test("a duplicate keeps its extension and takes the first free number", () => {
  assert.equal(uniqueName("a.txt", ["a.txt"]), "a copy.txt");
  assert.equal(uniqueName("a.txt", ["a.txt", "a copy.txt"]), "a copy 2.txt");
  assert.equal(uniqueName("a.txt", ["a.txt", "a copy.txt", "a copy 2.txt"]), "a copy 3.txt");
  // A leading dot is a name, not an extension — the same rule the language
  // registry states about .gitignore.
  assert.equal(uniqueName(".gitignore", [".gitignore"]), ".gitignore copy");
  // Asserted so the compound-extension behaviour is a decision rather than an
  // accident: only the last segment is treated as the extension.
  assert.equal(uniqueName("a.tar.gz", ["a.tar.gz"]), "a.tar copy.gz");
  assert.equal(uniqueName("plain", ["plain"]), "plain copy");
  assert.equal(uniqueName("free.txt", ["other.txt"]), "free copy.txt");
});

test("only expanded folders contribute rows, and depth counts from the root", () => {
  const node = (path, name, isDir, children) => [
    pathKey(path),
    { path, name, isDir, children },
  ];
  const nodes = new Map([
    node("C:\\p", "p", true, ["C:\\p\\src", "C:\\p\\readme.md"]),
    node("C:\\p\\src", "src", true, ["C:\\p\\src\\main.js"]),
    node("C:\\p\\src\\main.js", "main.js", false, null),
    node("C:\\p\\readme.md", "readme.md", false, null),
  ]);

  const collapsed = visibleRows("C:\\p", nodes, new Set());
  assert.deepEqual(
    collapsed.map((row) => row.node.name),
    ["src", "readme.md"],
    "a collapsed folder costs one row whatever is under it"
  );

  const open = visibleRows("C:\\p", nodes, new Set([pathKey("C:\\p\\src")]));
  assert.deepEqual(
    open.map((row) => [row.node.name, row.depth]),
    [["src", 0], ["main.js", 1], ["readme.md", 0]],
    "an expanded folder's children sit one level deeper, in place"
  );

  // A folder that has never been read has children === null, which is not the
  // same as a folder that was read and is empty.
  const unread = new Map([node("C:\\q", "q", true, null)]);
  assert.deepEqual(visibleRows("C:\\q", unread, new Set()), []);
});

test("a folder with more children than the clamp offers to show the rest", () => {
  const children = Array.from({ length: 5 }, (_, n) => `C:\\p\\f${n}.txt`);
  const nodes = new Map([
    [pathKey("C:\\p"), { path: "C:\\p", name: "p", isDir: true, children }],
    ...children.map((path) => [
      pathKey(path),
      { path, name: path.split("\\").pop(), isDir: false, children: null },
    ]),
  ]);

  const rows = visibleRows("C:\\p", nodes, new Set(), { clamp: 2 });
  assert.equal(rows.length, 3, "two rows plus the one that offers the rest");
  assert.equal(rows.at(-1).more, 3, "and it says how many are left");
});

test("type-ahead lands on the next match after the current row, and wraps", () => {
  const rows = ["alpha", "beta", "bravo", "charlie"].map((name) => ({ node: { name } }));

  assert.equal(nextTypeAhead(rows, 0, "b"), 1);
  // Typing the same letter again steps to the next match rather than sitting
  // on the one already under the cursor.
  assert.equal(nextTypeAhead(rows, 1, "b"), 2);
  assert.equal(nextTypeAhead(rows, 2, "b"), 1, "and wraps past the end");
  assert.equal(nextTypeAhead(rows, 0, "A"), 0, "matching ignores case");
  assert.equal(nextTypeAhead(rows, 0, "z"), -1, "no match is -1, not 0");
});

test("a file's icon comes from the most specific rule that matches it", () => {
  const map = {
    file: "file",
    folder: "folder",
    folderRoot: "folder-root",
    fileExtensions: { js: "javascript", ts: "typescript", "d.ts": "typescript-def" },
    fileNames: { "package.json": "nodejs", ".config/stylelintrc": "stylelint" },
    folderNames: { src: "folder-src", "META-INF": "folder-java" },
    light: { fileExtensions: { js: "javascript-light" }, fileNames: {}, folderNames: {} },
  };

  assert.equal(fileIconId(map, "app.js"), "javascript");
  assert.equal(fileIconId(map, "package.json"), "nodejs", "an exact name beats its extension");
  // A compound extension beats its own suffix, which is the whole reason the
  // lookup walks the segments instead of taking the last one.
  assert.equal(fileIconId(map, "types.d.ts"), "typescript-def");
  assert.equal(fileIconId(map, "plain.ts"), "typescript");
  assert.equal(fileIconId(map, "thing.qqq"), "file", "an unknown extension still gets an icon");
  assert.equal(fileIconId(map, "README"), "file", "so does a file with no extension at all");
  // 204 of the theme's file-name keys carry a directory; without the parent
  // probe they are dead weight in the map.
  assert.equal(fileIconId(map, "stylelintrc", { parent: ".config" }), "stylelint");
  // 60 keys across the maps are not lowercase, so exact case is tried first.
  assert.equal(fileIconId(map, "META-INF", { isDir: true }), "folder-java");
  assert.equal(fileIconId(map, "src", { isDir: true }), "folder-src");
  assert.equal(fileIconId(map, "src", { isDir: true, expanded: true }), "folder-src-open");
  assert.equal(fileIconId(map, "whatever", { isDir: true }), "folder");
  assert.equal(fileIconId(map, "app.js", { light: true }), "javascript-light");
});

test("a neper source file gets neper's own mark, not the blank default", () => {
  // The Material Icon Theme has never heard of neper, so without an icon of our
  // own every .e file wore the default page while the language registry was
  // busy highlighting it. This asserts against the *vendored* map rather than a
  // synthetic one, so it also fails if a future material-icon-theme starts
  // claiming .e for something else and quietly wins.
  const dir = new URL("../public/file-icons/", import.meta.url);
  const map = JSON.parse(readFileSync(new URL("map.json", dir), "utf8"));

  assert.equal(fileIconId(map, "main.e"), "neper");
  assert.equal(fileIconId(map, "MAIN.E"), "neper", "the extension is matched case-insensitively");
  // The tile carries its own ground, which is the whole reason it was chosen
  // over the bare mark, so it is used unchanged on a light background too.
  assert.equal(fileIconId(map, "main.e", { light: true }), "neper");
  assert.ok(readdirSync(dir).includes("neper.svg"), "and the drawing is on disk");
});

test("every icon the vendored map names was actually vendored", () => {
  // A missing SVG is a silent broken image in the tree, not an exception, so
  // nothing at runtime would ever report a half-finished vendoring. This is the
  // file-icon counterpart of the "every icon a source file names exists" check.
  const dir = new URL("../public/file-icons/", import.meta.url);
  const map = JSON.parse(readFileSync(new URL("map.json", dir), "utf8"));
  const onDisk = new Set(readdirSync(dir));

  const named = new Set();
  const claimFolder = (id) => {
    named.add(`${id}.svg`);
    named.add(`${id}-open.svg`);
  };
  for (const table of [map, map.light]) {
    if (table.file) named.add(`${table.file}.svg`);
    if (table.folder) claimFolder(table.folder);
    if (table.folderRoot) claimFolder(table.folderRoot);
    for (const id of Object.values(table.fileExtensions ?? {})) named.add(`${id}.svg`);
    for (const id of Object.values(table.fileNames ?? {})) named.add(`${id}.svg`);
    for (const id of Object.values(table.folderNames ?? {})) claimFolder(id);
  }

  const missing = [...named].filter((file) => !onDisk.has(file));
  assert.deepEqual(missing, [], `${missing.length} icon(s) named by map.json are not on disk`);
});

// ----------------------------------------------------------------- intel asm

/** Runs the Intel-syntax tokeniser over one line, dropping whitespace. */
function asmTokens(line, state = intelAsm.startState()) {
  const stream = new StringStream(line, 4, 4);
  const out = [];
  while (!stream.eol()) {
    stream.start = stream.pos;
    const tag = intelAsm.token(stream, state);
    assert.notEqual(stream.pos, stream.start, `tokeniser made no progress at ${stream.pos}`);
    if (tag) out.push([stream.current(), tag]);
  }
  return out;
}

test("the Intel mode gets the two things GNU as gets wrong here", () => {
  // `;` is the comment, not `#`, and a register is bare rather than %-prefixed.
  assert.deepEqual(asmTokens("    mov eax, 1 ; set it"), [
    ["mov", "keyword"],
    ["eax", "variableName.special"],
    [",", "punctuation"],
    ["1", "number"],
    ["; set it", "comment"],
  ]);
});

test("a radix suffix stays part of its number", () => {
  // `1Fh` and `1010b` are whole numbers. Reading the letter separately would
  // leave `1F` a number and `h` a name — and `0b1010` must not become `0` `b1010`.
  // `1.5` is one token too, but `.686` is a directive: neither assembler allows
  // a float to start with the point, so nothing legal is lost by that.
  const numbers = ["1Fh", "1010b", "0x1F", "0b1010", "17q", "42", "1.5", "1.0e3"];
  for (const literal of numbers) {
    assert.deepEqual(asmTokens(literal), [[literal, "number"]], literal);
  }
});

test("directives, size specifiers and labels are told apart from names", () => {
  assert.deepEqual(asmTokens("main:  mov dword ptr [count], offset msg"), [
    ["main:", "labelName"],
    ["mov", "keyword"],
    ["dword", "typeName"],
    ["ptr", "modifier"],
    ["[", "bracket"],
    ["count", "variableName"],
    ["]", "bracket"],
    [",", "punctuation"],
    ["offset", "modifier"],
    ["msg", "variableName"],
  ]);
  assert.deepEqual(asmTokens(".686"), [[".686", "typeName"]], "a CPU directive is not a fraction");
  assert.deepEqual(asmTokens("%define BUF 64"), [
    ["%define", "meta"],
    ["BUF", "variableName"],
    ["64", "number"],
  ]);
});

// --------------------------------------------------------------------- neper

/**
 * Runs the neper stream tokeniser over one line and returns `[text, tag]` for
 * every token it produces, dropping whitespace. `state` carries across lines so
 * a raw string can be followed through them.
 */
function neperTokens(line, state) {
  const stream = new StringStream(line, 4, 4);
  const out = [];
  while (!stream.eol()) {
    stream.start = stream.pos;
    const tag = neper.token(stream, state);
    assert.notEqual(stream.pos, stream.start, `tokeniser made no progress at ${stream.pos}`);
    if (tag) out.push([stream.current(), tag]);
  }
  return out;
}

test("a range operator is not eaten by the number in front of it", () => {
  // `0..8` is `0`, `..`, `8`. A float rule that accepted a trailing dot would
  // take `0.` and leave a stray `.`, which is why FLOAT requires a digit after
  // the point.
  assert.deepEqual(neperTokens("for i in 0..8 {", neper.startState()), [
    ["for", "controlKeyword"],
    ["i", "variableName"],
    ["in", "controlKeyword"],
    ["0", "number"],
    ["..", "operator"],
    ["8", "number"],
    ["{", "operator"],
  ]);
});

test("a radix prefix stays part of its number", () => {
  // The decimal branch would match the leading `0` and leave the rest as an
  // identifier, so the prefixed forms are tried first.
  for (const literal of ["0xFFu8", "0o755", "0b1010_1010", "1_000_000usize", "1.5e-3f32"]) {
    assert.deepEqual(
      neperTokens(literal, neper.startState()),
      [[literal, "number"]],
      `${literal} should lex as one number`,
    );
  }
});

test("casing decides what an unknown name is, because neper makes it decide", () => {
  const tagOf = (source) => neperTokens(source, neper.startState())[0][1];
  assert.equal(tagOf("Vec3"), "typeName", "PascalCase is a type");
  assert.equal(tagOf("NotFound"), "typeName", "so is an error name");
  assert.equal(tagOf("MAX_NODES"), "variableName.constant", "SCREAMING_SNAKE is a constant");
  assert.equal(tagOf("node_count"), "variableName", "snake_case is a value");
  assert.equal(tagOf("parse_expr("), "variableName.function", "…unless it is being called");
  assert.equal(tagOf("T"), "typeName", "a lone capital is read as a comptime type parameter");
});

test("a raw string runs to a delimiter with the same number of hashes", () => {
  const state = neper.startState();
  // The `"` inside is not the end: only `"##` is.
  assert.deepEqual(neperTokens('let s = r##"a "quoted" line', state), [
    ["let", "definitionKeyword"],
    ["s", "variableName"],
    ["=", "operator"],
    ['r##"a "quoted" line', "string"],
  ]);
  assert.equal(state.rawHashes, 2, "still open at end of line");
  assert.deepEqual(neperTokens('and another"## ret', state), [
    ['and another"##', "string"],
    ["ret", "controlKeyword"],
  ]);
  assert.equal(state.rawHashes, -1, "closed");
});

test("neper declarations are found at column 0 and nowhere else", () => {
  const source = [
    "use e.mem",
    "",
    "type Vec3 = struct {",
    "    x: f32,",
    "}",
    "",
    "error NotFound",
    "",
    "const MAX_NODES: u32 = 1024",
    "",
    "fn apply[T: type](f: fn(T) -> T, v: T) -> T {",
    "    ret f(v)",
    "}",
  ].join("\n");
  const found = findSymbols(source, "neper");
  assert.deepEqual(
    found.map((s) => `${s.kind} ${s.name}${s.detail}`),
    [
      "struct Vec3 = struct",
      "error NotFound",
      "const MAX_NODES: u32",
      // The parameter list keeps the inner parentheses of the `fn` type, and
      // the indented `ret f(v)` is not mistaken for a declaration.
      "function apply[T: type](f: fn(T) -> T, v: T) -> T",
    ],
    "in document order, with signatures intact",
  );
});
