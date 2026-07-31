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
