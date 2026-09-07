// The explorer's rules, and nothing else: no DOM, no Tauri, no `t()`.
//
// Here rather than inside explorer.js for the reason ondisk.js gives about
// itself — these are the parts that are only checkable by arranging a real
// filesystem and a real window, which is exactly what nobody reproduces on
// purpose. Sorting, containment, name validation and the flattening of a tree
// into rows are all decidable from strings alone, so they are decided here and
// asserted in tests/units.test.js.
//
// One rule the whole module depends on: a path is identified by `pathKey(path)`,
// never by object identity. That is what lets a refresh replace every node in
// the tree without losing which folders were expanded or which row was
// selected — the sets hold keys, and keys survive a re-read.

/**
 * How two paths are compared for "the same file".
 *
 * Windows paths differ only by case and by separator, so `C:/a.txt` and
 * `c:\a.txt` are one file and must not become two rows. Matches
 * `samePathKey` in main.js deliberately: the explorer hands paths to
 * `openPath`, and the two disagreeing about identity would open a second tab
 * on a file that is already open.
 */
export function pathKey(path) {
  return String(path).replace(/\//g, "\\").toLowerCase();
}

/** The last segment of a path, whichever separator it uses. */
export function baseName(path) {
  return String(path).split(/[\\/]/).pop();
}

/** Everything but the last segment, or "" when there is nothing above it. */
export function parentOf(path) {
  const cut = String(path).replace(/[\\/][^\\/]*$/, "");
  return cut === String(path) ? "" : cut;
}

/**
 * Join a name onto a directory using the separator that directory already
 * uses, so a path built here looks like the ones around it rather than mixing
 * `C:\project/src\main.js`.
 */
export function joinPath(dir, name) {
  const text = String(dir);
  const separator = text.includes("\\") && !text.includes("/") ? "\\" : text.includes("\\") ? "\\" : "/";
  return `${text.replace(/[\\/]+$/, "")}${separator}${name}`;
}

/**
 * Whether `child` is inside `parent` — the guard that stops a folder being
 * dropped or copied into its own subtree, which otherwise recurses until the
 * disk is full.
 *
 * A bare `startsWith` gets the boundary wrong: `C:\ab` starts with `C:\a` and
 * is not inside it. The separator has to be part of the comparison.
 */
export function isInside(parent, child) {
  const from = pathKey(parent).replace(/\\+$/, "");
  const to = pathKey(child);
  return to.startsWith(`${from}\\`);
}

/** `path` relative to `root`, or `path` unchanged when it is not under it. */
export function relativePath(root, path) {
  if (pathKey(root) === pathKey(path)) return "";
  if (!isInside(root, path)) return String(path);
  return String(path)
    .slice(String(root).replace(/[\\/]+$/, "").length + 1);
}

/**
 * Folders first, then by name the way a person counts.
 *
 * `Intl.Collator` with `numeric: true` is natural sort, case-insensitive and
 * locale-correct, and the webview already ships it — so `file2` lands before
 * `file10` and `Apple` beside `apple` without a hand-rolled comparator and a
 * test to keep it honest.
 */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export function sortEntries(entries) {
  return [...entries].sort(
    (a, b) => Number(Boolean(b.isDir)) - Number(Boolean(a.isDir)) || collator.compare(a.name, b.name)
  );
}

/**
 * Names Windows refuses whatever the extension. `CON.txt` is as unopenable as
 * `CON`, which is why the check is against the stem rather than the whole name.
 */
const RESERVED = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/i;

/** Characters no Windows file name may contain. Listed so the message can say. */
const FORBIDDEN = '<>:"|?*';

/**
 * Whether `name` may be used for a new or renamed entry in a folder that
 * already holds `siblings`.
 *
 * Returns `null` when it is fine, otherwise `{ key, vars }` — an i18n key and
 * its substitutions, never a finished string. Keeping `t()` out of this module
 * is what lets it be unit-tested without a locale loaded, and what keeps the
 * "no string is written in English at the point it is shown" check true.
 *
 * `self` is the name being renamed, which is allowed to collide with itself —
 * otherwise renaming `Foo.txt` to `foo.txt` is refused for clashing with the
 * very file it is.
 */
export function validateName(name, siblings = [], { self = null } = {}) {
  const text = String(name);
  if (!text.trim()) return { key: "explorer.nameEmpty", vars: {} };
  if (/[\\/]/.test(text)) return { key: "explorer.nameSeparator", vars: {} };

  const bad = [...FORBIDDEN].filter((char) => text.includes(char));
  if (bad.length) return { key: "explorer.nameInvalidChars", vars: { chars: FORBIDDEN } };

  // Windows silently strips a trailing dot or space, so the file you get is not
  // the one you asked for — better to refuse than to create `a` when `a.` was
  // typed.
  if (/[. ]$/.test(text)) return { key: "explorer.nameTrailing", vars: {} };

  const stem = text.replace(/\..*$/, "");
  if (RESERVED.test(stem)) return { key: "explorer.nameReserved", vars: { name: text } };

  const taken = siblings.some(
    (sibling) =>
      sibling.toLowerCase() === text.toLowerCase() &&
      (self === null || sibling.toLowerCase() !== String(self).toLowerCase())
  );
  if (taken) return { key: "explorer.nameExists", vars: { name: text } };

  return null;
}

/**
 * A free name for a copy, keeping the extension so the file stays openable.
 *
 * A leading dot is not an extension: `.gitignore` is a name, so its copy is
 * `.gitignore copy` rather than `.gitignore copy` with the "extension"
 * `gitignore` moved to the end. Same rule the language registry uses.
 */
export function uniqueName(name, siblings = []) {
  const text = String(name);
  const dot = text.lastIndexOf(".");
  const hasExtension = dot > 0;
  const stem = hasExtension ? text.slice(0, dot) : text;
  const extension = hasExtension ? text.slice(dot) : "";
  const taken = new Set(siblings.map((sibling) => sibling.toLowerCase()));

  let candidate = `${stem} copy${extension}`;
  for (let n = 2; taken.has(candidate.toLowerCase()); n += 1) {
    candidate = `${stem} copy ${n}${extension}`;
  }
  return candidate;
}

/**
 * Flatten the tree into the rows that are actually on screen.
 *
 * Only expanded folders contribute children, which is what keeps a root holding
 * `node_modules` cheap: an unexpanded folder costs one row whatever is under it.
 *
 * `clamp` caps how many children one folder contributes. A directory with
 * 40,000 entries in it is a real thing and rendering all of them is the only
 * unbounded case in the panel; the caller shows a "N more" row and raises the
 * cap for that folder when it is clicked.
 *
 * @param {string} root
 * @param {Map<string, {path: string, name: string, isDir: boolean, children: string[]|null}>} nodes
 * @param {Set<string>} expanded
 */
export function visibleRows(root, nodes, expanded, { clamp = 1000, clamps = new Map() } = {}) {
  const rows = [];
  const walk = (path, depth) => {
    const node = nodes.get(pathKey(path));
    if (!node || !node.children) return;
    const limit = clamps.get(pathKey(path)) ?? clamp;
    const children = node.children.slice(0, limit);
    children.forEach((childPath, index) => {
      const child = nodes.get(pathKey(childPath));
      if (!child) return;
      rows.push({
        key: pathKey(childPath),
        path: childPath,
        depth,
        index,
        setSize: children.length,
        node: child,
      });
      if (child.isDir && expanded.has(pathKey(childPath))) walk(childPath, depth + 1);
    });
    if (node.children.length > limit) {
      rows.push({
        key: `${pathKey(path)}\u0000more`,
        path,
        depth,
        more: node.children.length - limit,
        setSize: children.length,
        index: children.length,
      });
    }
  };
  walk(root, 0);
  return rows;
}

/**
 * Where type-ahead should land: the next row after `from` whose name starts
 * with `prefix`, wrapping past the end.
 *
 * Starts at `from + 1` rather than `from` so typing the same letter twice steps
 * through the matches instead of sitting on the first one.
 */
export function nextTypeAhead(rows, from, prefix) {
  const needle = String(prefix).toLowerCase();
  if (!needle) return -1;
  for (let step = 1; step <= rows.length; step += 1) {
    const index = (from + step + rows.length) % rows.length;
    const name = rows[index]?.node?.name;
    if (name && name.toLowerCase().startsWith(needle)) return index;
  }
  return -1;
}

/**
 * Which Material Icon Theme icon a row gets.
 *
 * The order is the upstream theme's own: an exact file name beats an extension,
 * and a longer compound extension beats its suffix — `a.test.js` is the test
 * icon, not the JavaScript one, and `x.d.ts` is the TypeScript-definition icon.
 *
 * `parent` is checked as `folder/name` because 204 of the theme's file-name
 * keys carry a directory (`.config/stylelintrc`); without that probe they are
 * dead weight in the map. Exact case is tried before lowercase because 60 keys
 * across the maps are not lowercase (`META-INF`, `PKGBUILD`, `tmLanguage`).
 */
export function fileIconId(map, name, { isDir = false, expanded = false, parent = "", light = false } = {}) {
  if (!map) return null;
  const text = String(name);
  const lower = text.toLowerCase();

  // The light maps are overrides, not replacements: `light.fileExtensions`
  // carries the ~30 icons that need a different treatment on white, and
  // swapping the whole table in for it would lose the other 1,347.
  const overlay = light ? map.light ?? {} : {};
  const table = (key) => ({ ...map[key], ...overlay[key] });
  const fileNames = table("fileNames");
  const fileExtensions = table("fileExtensions");
  const folderNames = table("folderNames");

  if (isDir) {
    const folder = folderNames[text] ?? folderNames[lower] ?? overlay.folder ?? map.folder;
    return expanded ? `${folder}-open` : folder;
  }

  // `??` only falls through on null and undefined, so a `parent && …` guard
  // here would stop the chain dead on an empty string — the row would lose its
  // icon for the ordinary case of a file at the root. Hence the ternary.
  const byName =
    (parent ? fileNames[`${String(parent).toLowerCase()}/${lower}`] : undefined) ??
    fileNames[text] ??
    fileNames[lower];
  if (byName) return byName;

  // "a.test.js" -> try "test.js", then "js". Segment 0 is the stem and is never
  // part of an extension, which is what keeps ".gitignore" a name rather than a
  // file with the extension "gitignore".
  const segments = lower.split(".");
  for (let start = 1; start < segments.length; start += 1) {
    const byExtension = fileExtensions[segments.slice(start).join(".")];
    if (byExtension) return byExtension;
  }

  return overlay.file ?? map.file;
}
