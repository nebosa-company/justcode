/**
 * Shortcut text, per platform.
 *
 * Shortcuts are declared once, in one spelling — `Ctrl+Shift+K` — and rendered
 * for whichever machine is running. `Ctrl` in a declaration means *the command
 * modifier*, which is Control on Windows and Linux and Command on a Mac; that
 * is already how the key handler reads it (`event.ctrlKey || event.metaKey`),
 * and the declarations were the half still saying Control out loud.
 *
 * A Mac shows the glyphs and no separators, in the order Apple fixes:
 * Control, Option, Shift, Command, then the key. `Ctrl+Shift+K` is `⇧⌘K` and
 * not `⌘⇧K` — the order is not a preference, it is what every other menu on
 * the machine does, and getting it wrong is the kind of small wrongness that
 * reads as a port rather than an application.
 */

/** Whether this is a Mac, from the only thing a renderer can ask. */
export const IS_MAC = /Mac|iPhone|iPad|Darwin/i.test(navigator.userAgent);

/** The glyphs, in Apple's order. */
const MAC_GLYPH = { ctrl: "⌘", control: "⌃", alt: "⌥", shift: "⇧" };
const MAC_ORDER = ["control", "alt", "shift", "ctrl"];

/** What a key with no glyph is called on a Mac menu. */
const MAC_KEY = {
  Backspace: "⌫",
  Delete: "⌦",
  Enter: "↩",
  Escape: "⎋",
  Tab: "⇥",
  Home: "↖",
  End: "↘",
  PageUp: "⇞",
  PageDown: "⇟",
  Left: "←",
  Right: "→",
  Up: "↑",
  Down: "↓",
  Space: "Space",
};

/**
 * Render one declared shortcut for this platform.
 *
 * Passes through anything it does not recognise. The reference list carries
 * prose entries — "click the gutter", "drag a tab" — and mangling those into
 * glyphs would be worse than leaving them alone.
 */
export function accel(spec) {
  return formatAccel(spec, IS_MAC);
}

/**
 * The same, with the platform passed in.
 *
 * Split out so both halves can be tested. `IS_MAC` is read once at module load
 * from the one thing a renderer can ask, which makes the Mac path unreachable
 * from a test runner that is not a Mac — and an untested Mac path is how this
 * whole area got into the state it was in.
 */
export function formatAccel(spec, isMac) {
  if (!spec || typeof spec !== "string") return spec || "";
  // A declaration may list alternatives: `Ctrl+Y / Ctrl+Shift+Z`.
  if (spec.includes(" / ")) {
    return spec
      .split(" / ")
      .map((one) => formatAccel(one, isMac))
      .join(" / ");
  }
  if (!isMac) return spec;

  const parts = spec.split("+").map((part) => part.trim());
  const key = parts.pop();
  const held = new Set(parts.map((part) => part.toLowerCase()));
  // Nothing recognisable to translate — a bare `F5`, or prose.
  if (held.size === 0) return MAC_KEY[key] || key;
  if ([...held].some((part) => !(part in MAC_GLYPH))) return spec;

  const glyphs = MAC_ORDER.filter((name) => held.has(name)).map((name) => MAC_GLYPH[name]);
  return `${glyphs.join("")}${MAC_KEY[key] || key}`;
}

/**
 * The name of the command modifier, for prose that has to say it.
 *
 * Used where a sentence explains a gesture — "Ctrl+click to add a cursor" —
 * rather than naming a shortcut.
 */
export const COMMAND_KEY = IS_MAC ? "⌘" : "Ctrl";

/**
 * A shortcut that is a *different chord* on a Mac, not the same one drawn
 * differently.
 *
 * Most shortcuts need only the glyphs. A few are Windows window-management
 * idioms with a Mac counterpart that shares no keys: Alt+F4 quits on Windows
 * and ⌘Q on a Mac, Alt+M minimises on Windows and ⌘M on a Mac. Rendering
 * `Alt+F4` as `⌥F4` would be a faithful translation of the wrong thing.
 */
export function perPlatform(windows, mac) {
  return IS_MAC ? mac : windows;
}

/**
 * Whether a keydown is the letter a shortcut wants, whatever it typed.
 *
 * `event.key` is the character produced, and on a Mac Option composes: ⌥M is
 * `µ`, ⌥S is `ß`, ⌥T is `†`, ⌥Z is `Ω`. So every `event.altKey && event.key
 * === "m"` binding in this app was dead on a Mac — not mis-drawn in a menu,
 * not awkward, simply never firing, because the comparison could not be true.
 *
 * `event.code` is the physical key and does not compose. It is wrong for a
 * layout where the letter has moved — Dvorak, AZERTY — so the character is
 * still accepted when it matches, and the code is the fallback that makes
 * Option shortcuts work at all.
 */
export function isLetter(event, letter) {
  return (
    event.key.toLowerCase() === letter || event.code === `Key${letter.toUpperCase()}`
  );
}
