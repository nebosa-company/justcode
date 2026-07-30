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

test("every menu item's enabled, checked and submenu is a function", () => {
  // `menu.js` calls all three: `item.enabled()`, `item.checked?.()`,
  // `item.submenu()`. Anything else is a `TypeError` at render time, and the
  // render happens on the line *before* the dropdown is unhidden — so the whole
  // menu opens as nothing rather than as a broken row.
  //
  // `enabled: canCopy()` did exactly that. With no file open the result was
  // `false`, the `&&` short-circuited, nothing was called and the menu worked;
  // with a file open it was `true`, and `true()` threw. It survived because the
  // failing case is the useful one — a menu only breaks once there is something
  // to edit.
  //
  // Checking the shape rather than just the `foo()` form, because `enabled: true`
  // and `enabled: someFlag` fail the same way and read as harmless.
  const offenders = [];
  for (const { name, text } of sources()) {
    // Names this file can vouch for: declared as a function, assigned one, or
    // imported.
    //
    // Imports are taken on trust — the check cannot follow one into another
    // module, so an imported *constant* used as `enabled` would slip through.
    // That is the honest limit of reading text, and it costs less than refusing
    // every imported predicate would: the bugs this exists for are
    // `enabled: canCopy()`, `enabled: true` and `enabled: someLocalFlag`, and all
    // three are still caught.
    const imported = [...text.matchAll(/import\s*\{([^}]*)\}\s*from/g)]
      .flatMap((match) => match[1].split(","))
      .map((part) => part.split(/\s+as\s+/).pop().trim())
      .filter(Boolean);
    const functions = new Set([
      ...[...text.matchAll(/\bfunction\s+([A-Za-z_$][\w$]*)/g)].map((m) => m[1]),
      ...[...text.matchAll(/\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?(?:\(|function)/g)].map(
        (m) => m[1],
      ),
      ...imported,
    ]);

    for (const match of text.matchAll(/\b(enabled|checked|submenu):\s*([^,\n]*)/g)) {
      const property = match[1];
      const value = match[2].trim().replace(/[}\s]+$/, "");
      if (!value) continue;
      // An arrow or a function expression is fine however it continues.
      if (/^(\(|async\s*\(|function\b)/.test(value)) continue;
      // A bare identifier is fine when this file declares it as a function.
      if (/^[A-Za-z_$][\w$]*$/.test(value) && functions.has(value)) continue;
      // `.bind(...)` still yields a function.
      if (/\.bind\(/.test(value)) continue;
      offenders.push(`${name}: ${property}: ${value}`);
    }
  }
  assert.deepEqual(offenders, [], `called at render time, so not callable:\n${offenders.join("\n")}`);
});

test("top-level menu mnemonics are lowercase and unique", () => {
  // The bar matches with `menu.mnemonic === key` against a lowercased key, so an
  // uppercase mnemonic never matches anything — and two menus claiming the same
  // letter means the second is unreachable by keyboard. Both were true of the
  // Harness menu the day it was added: `"H"` never matched, and `h` was Help's.
  const text = readFileSync("src/main.js", "utf8");
  const mnemonics = [...text.matchAll(/^\s*mnemonic:\s*"([A-Za-z])",/gm)].map((m) => m[1]);
  assert.ok(mnemonics.length >= 4, `found ${mnemonics.length}`);

  const upper = mnemonics.filter((letter) => letter !== letter.toLowerCase());
  assert.deepEqual(upper, [], `never matches a lowercased key: ${upper.join(", ")}`);

  // `has` then `add`. `Set.add` returns the set, which is always truthy, so
  // `!seen.add(letter)` is always false — a red run caught this assertion
  // agreeing with everything.
  const seen = new Set();
  const clashing = mnemonics.filter((letter) => {
    if (seen.has(letter)) return true;
    seen.add(letter);
    return false;
  });
  assert.deepEqual(clashing, [], `two menus claim the same Alt key: ${clashing.join(", ")}`);
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
