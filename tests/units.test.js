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
import { renderMarkdownDocument } from "../src/markdown.js";

test("a file's language comes from its own extension, not its path", () => {
  assert.equal(languageIdFor("a.py"), "python");
  assert.equal(languageIdFor("C:\\Users\\me\\a.py"), "python");
  assert.equal(languageIdFor("/home/me/a.py"), "python");
  assert.equal(languageIdFor("A.PY"), "python", "extensions are matched case-insensitively");

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
