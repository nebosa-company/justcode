// Cross-file checks: names one file uses and another file defines.
//
// These are the cheapest tests in the suite and the ones most likely to earn
// their keep. Every failure mode here is a runtime throw or a blank label that
// no compiler catches, in code that only runs when a menu is opened — which is
// how a menu item with a mistyped icon ships, and how a `t()` call outlives the
// key it reads.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";

import { PATHS } from "../src/icons.js";
import { EN } from "../src/i18n.js";

/** Every `src/*.js`, as text, for the source-scanning checks below. */
function sources() {
  return readdirSync("src")
    .filter((name) => name.endsWith(".js"))
    .map((name) => ({ name, text: readFileSync(`src/${name}`, "utf8") }));
}

test("every icon a source file names exists", () => {
  const missing = [];
  for (const { name, text } of sources()) {
    // `icon: "x"`, `iconMarkup("x")` and `iconElement("x")` — the three ways a
    // name reaches the icon table.
    const used = [
      ...text.matchAll(/\bicon:\s*"([a-zA-Z0-9_-]+)"/g),
      ...text.matchAll(/\bicon(?:Markup|Element)\("([a-zA-Z0-9_-]+)"/g),
    ].map((match) => match[1]);
    for (const icon of used) {
      if (!(icon in PATHS)) missing.push(`${name}: ${icon}`);
    }
  }
  assert.deepEqual(missing, [], `iconMarkup throws on an unknown name:\n${missing.join("\n")}`);
});

test("every translation key a source file reads exists", () => {
  const missing = [];
  for (const { name, text } of sources()) {
    if (name === "i18n.js" || name === "locales.js") continue;
    // Only literal keys. A computed `t(someVariable)` cannot be checked here and
    // is deliberately not guessed at.
    for (const match of text.matchAll(/\bt\("([a-zA-Z0-9_.]+)"/g)) {
      if (!(match[1] in EN)) missing.push(`${name}: ${match[1]}`);
    }
  }
  assert.deepEqual(
    missing,
    [],
    `a key with no entry renders as itself or blank:\n${missing.join("\n")}`,
  );
});

test("no translation key is defined twice", () => {
  // `EN` is an object literal, so a duplicate key silently wins and the earlier
  // wording disappears with no error anywhere.
  const text = readFileSync("src/i18n.js", "utf8");
  const seen = new Map();
  const duplicated = [];
  for (const match of text.matchAll(/^\s*"([a-zA-Z0-9_.]+)":/gm)) {
    const key = match[1];
    if (seen.has(key)) duplicated.push(key);
    seen.set(key, true);
  }
  assert.deepEqual(duplicated, [], `later wins, earlier vanishes: ${duplicated.join(", ")}`);
});

test("every harness tab has a label, a hint and an icon", () => {
  // The tab table builds its keys with a template literal — `harness.tab.${name}`
  // — which the literal-key check above cannot see. This is that check for the
  // one place that computes them, and it is why the table is a single list: the
  // panel strip, the Harness menu and this test all read the same names.
  const text = readFileSync("src/perp.js", "utf8");
  const table = text.slice(
    text.indexOf("export const TABS"),
    text.indexOf("export function tabLabel"),
  );
  const tabs = [...table.matchAll(/name:\s*"([a-z]+)",\s*icon:\s*"([a-zA-Z]+)"/g)];

  assert.ok(tabs.length >= 6, `the table parsed: found ${tabs.length}`);
  const missing = [];
  for (const [, name, icon] of tabs) {
    if (!(`harness.tab.${name}` in EN)) missing.push(`label for ${name}`);
    if (!(`harness.hint.${name}` in EN)) missing.push(`hint for ${name}`);
    if (!(icon in PATHS)) missing.push(`icon ${icon} for ${name}`);
  }
  assert.deepEqual(missing, [], `a tab with no name renders blank:\n${missing.join("\n")}`);
});

test("every element id the front-end looks up exists in index.html", () => {
  const html = readFileSync("index.html", "utf8");
  const declared = new Set([...html.matchAll(/\bid="([a-zA-Z0-9_-]+)"/g)].map((m) => m[1]));
  const missing = [];
  for (const { name, text } of sources()) {
    for (const match of text.matchAll(/getElementById\("([a-zA-Z0-9_-]+)"\)/g)) {
      if (!declared.has(match[1])) missing.push(`${name}: ${match[1]}`);
    }
  }
  // A missing id is `null`, and every use of it is a crash or a silent no-op —
  // the resize handle and the status-bar badge both reach for one.
  assert.deepEqual(missing, [], `getElementById returns null for:\n${missing.join("\n")}`);
});

test("a hidden element that is display-something has an explicit [hidden] rule", () => {
  // `button { display: inline-flex }` beats the browser's `[hidden]` rule, so an
  // element that sets `display` needs `[hidden] { display: none }` of its own or
  // the `hidden` attribute does nothing. This has caught four elements in this
  // stylesheet already, each time by someone noticing it on screen.
  const css = readFileSync("src/styles.css", "utf8");
  const html = readFileSync("index.html", "utf8");

  // Ids that are written with `hidden` in the markup, so hiding them matters.
  const hideable = [...html.matchAll(/\bid="([a-zA-Z0-9_-]+)"[^>]*\bhidden\b/g)].map((m) => m[1]);
  const missing = hideable.filter((id) => !css.includes(`#${id}[hidden]`));
  assert.deepEqual(
    missing,
    [],
    `these stay visible when hidden is set:\n${missing.join("\n")}`,
  );
});
