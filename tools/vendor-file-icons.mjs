// Vendors the Material Icon Theme into `public/file-icons/`, so the explorer
// shows the file icons people already recognise from VS Code.
//
// Source: the `material-icon-theme` npm package (MIT), a devDependency. It is a
// build input and is never imported at runtime — what ships is the output of
// this script.
//
// Three things are written:
//
//   public/file-icons/<id>.svg   only the icons the maps actually reach
//   public/file-icons/map.json   name/extension/folder -> icon id
//   public/file-icons/LICENSE    the upstream MIT notice, shipped in the app
//
// Icon files are resolved through `iconDefinitions[id].iconPath` rather than
// assumed to be `<id>.svg`. 72 of them are not — `latex` lives in
// `latex.clone.svg` — and guessing the name drops them silently, which shows up
// as a broken image in the tree rather than as an error anywhere.
//
// Icons this project owns rather than vendors live in `assets/file-icons/` and
// are copied in alongside, with their extensions folded into the same maps. The
// Material Icon Theme has no icon for a language it has never heard of, and a
// file type the editor highlights should not be the one row in the tree
// wearing the blank default.
//
// `folderNamesExpanded` is dropped: it is exactly `folderNames[k] + "-open"`
// for all 4654 entries, verified, so deriving it halves the largest map.
// `highContrast` is dropped because it is empty. `languageIds` is dropped
// because it is keyed by VS Code language ids, which this app does not have —
// `src/languages.js` maps extensions to CodeMirror modes, so wiring it up would
// mean inventing an extension->languageId table to feed a table that is itself
// keyed by extension.
//
//     node tools/vendor-file-icons.mjs           # regenerate if the version moved
//     node tools/vendor-file-icons.mjs --force   # regenerate regardless
//     node tools/vendor-file-icons.mjs --check   # report drift, change nothing
//
// The generated directory is gitignored. A 1,178-file commit that a version
// bump re-diffs wholesale is churn, and generating it removes the drift class
// rather than policing it. `--check` still earns its place: it catches a
// version bump landing in package-lock.json without a re-vendor.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkgDir = path.join(root, "node_modules", "material-icon-theme");
const outDir = path.join(root, "public", "file-icons");
const ownDir = path.join(root, "assets", "file-icons");
const check = process.argv.includes("--check");

/** Icons this project ships itself, and the file types they claim.
 *
 * Kept in step with `src/languages.js` by hand: it is one entry, and deriving
 * it would mean teaching this script the language registry's shape to save a
 * line. Extensions carry no leading dot, matching the upstream maps. */
const OWN_ICONS = {
  neper: { extensions: ["e"] },
};

if (!fs.existsSync(pkgDir)) {
  console.error("material-icon-theme is not installed. Run `npm install` first.");
  process.exit(1);
}

const version = JSON.parse(fs.readFileSync(path.join(pkgDir, "package.json"), "utf8")).version;

// `predev` and `prebuild` call this on every run. Rewriting 1,178 files each
// time `npm run dev` starts is a second of pointless disk churn, so a tree that
// already records this exact version is left alone. `--check` still does the
// full byte comparison; this is only the "nothing to do" fast path.
if (!check && !process.argv.includes("--force")) {
  const mapPath = path.join(outDir, "map.json");
  if (fs.existsSync(mapPath)) {
    try {
      const built = JSON.parse(fs.readFileSync(mapPath, "utf8"));
      // The version gate is about the vendored set. Our own icons change on
      // their own schedule, so compare them too — otherwise editing one and
      // running `npm run dev` would quietly keep serving the old drawing.
      const ownCurrent = Object.keys(OWN_ICONS).every((id) => {
        const from = path.join(ownDir, `${id}.svg`);
        const to = path.join(outDir, `${id}.svg`);
        return fs.existsSync(to) && fs.readFileSync(to).equals(fs.readFileSync(from));
      });
      if (built.version === version && ownCurrent) {
        process.exit(0);
      }
    } catch {
      // A truncated or hand-edited map falls through and is rebuilt.
    }
  }
}

const theme = JSON.parse(fs.readFileSync(path.join(pkgDir, "dist", "material-icons.json"), "utf8"));

/** Every icon id the maps below can ask for, so nothing unreferenced is copied. */
const wanted = new Set();
const claim = (id) => {
  if (!id) return id;
  wanted.add(id);
  return id;
};

// Folders are stored closed and the open twin is derived at lookup time by
// appending "-open", so both have to be copied.
//
// Three of the light folder icons have no open twin upstream. Rather than teach
// the runtime an exception table for those three, the closed icon is written
// out under the open name as well: the derivation rule then always resolves,
// and the cost is three duplicated files under a kilobyte each. A rule with no
// exceptions is worth more than three files.
const aliased = new Map();
const claimFolder = (id) => {
  claim(id);
  if (theme.iconDefinitions[`${id}-open`]) claim(`${id}-open`);
  else aliased.set(`${id}-open`, id);
  return id;
};

const mapValues = (source, take) =>
  Object.fromEntries(Object.entries(source ?? {}).map(([key, id]) => [key, take(id)]));

const map = {
  // Recorded so the pre-build hook can no-op when it is already current, and so
  // --check can say which version the tree was built from.
  version,
  file: claim(theme.file),
  folder: claimFolder(theme.folder),
  folderRoot: claimFolder(theme.rootFolder),
  fileExtensions: mapValues(theme.fileExtensions, claim),
  fileNames: mapValues(theme.fileNames, claim),
  folderNames: mapValues(theme.folderNames, claimFolder),
  // The light variants are not derivable and are kept whole. They exist because
  // ~50 of these icons are near-white and vanish against a white background.
  light: {
    file: theme.light?.file ? claim(theme.light.file) : undefined,
    folder: theme.light?.folder ? claimFolder(theme.light.folder) : undefined,
    fileExtensions: mapValues(theme.light?.fileExtensions, claim),
    fileNames: mapValues(theme.light?.fileNames, claim),
    folderNames: mapValues(theme.light?.folderNames, claimFolder),
  },
};

// Ours win over anything upstream claims for the same extension: the editor
// highlights these languages, so its own icon is the more specific answer.
for (const [id, { extensions = [], fileNames = [] }] of Object.entries(OWN_ICONS)) {
  const file = path.join(ownDir, `${id}.svg`);
  if (!fs.existsSync(file)) {
    console.error(`assets/file-icons/${id}.svg is missing.`);
    process.exit(1);
  }
  for (const extension of extensions) map.fileExtensions[extension] = id;
  for (const name of fileNames) map.fileNames[name] = id;
}

/** id -> the file on disk it actually lives in. */
const sourceFor = (id) => {
  const defined = theme.iconDefinitions[id]?.iconPath;
  if (!defined) return null;
  return path.join(pkgDir, "dist", defined);
};

const files = new Map();
const missing = [];
for (const id of wanted) {
  const from = sourceFor(id);
  if (!from || !fs.existsSync(from)) {
    missing.push(id);
    continue;
  }
  files.set(`${id}.svg`, fs.readFileSync(from));
}
if (missing.length) {
  console.error(`${missing.length} icon(s) named by the maps have no file: ${missing.slice(0, 5).join(", ")}`);
  process.exit(1);
}

for (const id of Object.keys(OWN_ICONS)) {
  files.set(`${id}.svg`, fs.readFileSync(path.join(ownDir, `${id}.svg`)));
}

for (const [openName, closedId] of aliased) {
  const body = files.get(`${closedId}.svg`);
  if (body) files.set(`${openName}.svg`, body);
}

files.set("LICENSE", fs.readFileSync(path.join(pkgDir, "LICENSE")));
const mapText = `${JSON.stringify(map, null, 0)}\n`;

if (check) {
  const drift = [];
  const onDisk = fs.existsSync(outDir) ? new Set(fs.readdirSync(outDir)) : new Set();
  const current = fs.existsSync(path.join(outDir, "map.json"))
    ? fs.readFileSync(path.join(outDir, "map.json"), "utf8")
    : "";
  if (current !== mapText) drift.push("map.json");
  for (const [name, body] of files) {
    if (!onDisk.has(name)) drift.push(`${name} (missing)`);
    else if (!fs.readFileSync(path.join(outDir, name)).equals(body)) drift.push(name);
  }
  for (const name of onDisk) {
    if (name !== "map.json" && !files.has(name)) drift.push(`${name} (stale)`);
  }
  if (drift.length) {
    console.error(
      `public/file-icons is ${drift.length} file(s) out of step with material-icon-theme ${version}:`
    );
    for (const name of drift.slice(0, 10)) console.error(`  ${name}`);
    console.error("Run `npm run vendor:file-icons`.");
    process.exit(1);
  }
  console.log(`public/file-icons is current: ${files.size - 1} icons, material-icon-theme ${version}`);
  process.exit(0);
}

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });
for (const [name, body] of files) fs.writeFileSync(path.join(outDir, name), body);
fs.writeFileSync(path.join(outDir, "map.json"), mapText);

console.log(
  `Vendored ${files.size - 1} icons from material-icon-theme ${version} into public/file-icons ` +
    `(${(mapText.length / 1024).toFixed(0)} KB map).`
);
