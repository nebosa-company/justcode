// Project metrics: how many lines the open folder holds, split into code,
// comment and blank, grouped by language, and compared with the last scan.
//
// The counting happens in Rust (`project_metrics`) because it is a whole-project
// disk walk. What lives here is the part Rust should not have to know: what a
// language is called, how it writes a comment, and how to say "last 2 hours" in
// thirty-six languages.

import { invoke } from "@tauri-apps/api/core";
import { LANGUAGE_EXTENSIONS, LANGUAGE_LABELS } from "./languages.js";

/** The report, written beside the project. Also hard-coded in lib.rs, which
 *  skips it while walking so the report never counts itself. */
export const METRICS_FILE = ".metrics";

/**
 * How each language writes a comment, keyed by the ids in `languages.js`.
 *
 * A language missing from this table is still counted — its lines simply come
 * out as code and blank, which is the right answer for JSON and for plain text.
 * Markdown is deliberately here as prose-is-code: under a code/comment/blank
 * split there is no third bucket for a paragraph, and calling a README's body
 * "comment" would flatter every project that ships documentation.
 */
const COMMENTS = {
  html: { block: ["<!--", "-->"] },
  xml: { block: ["<!--", "-->"] },
  css: { block: ["/*", "*/"] },
  javascript: { line: ["//"], block: ["/*", "*/"] },
  typescript: { line: ["//"], block: ["/*", "*/"] },
  jsx: { line: ["//"], block: ["/*", "*/"] },
  tsx: { line: ["//"], block: ["/*", "*/"] },
  python: { line: ["#"] },
  cpp: { line: ["//"], block: ["/*", "*/"] },
  java: { line: ["//"], block: ["/*", "*/"] },
  csharp: { line: ["//"], block: ["/*", "*/"] },
  kotlin: { line: ["//"], block: ["/*", "*/"] },
  swift: { line: ["//"], block: ["/*", "*/"] },
  r: { line: ["#"] },
  toml: { line: ["#"] },
  protobuf: { line: ["//"], block: ["/*", "*/"] },
  go: { line: ["//"], block: ["/*", "*/"] },
  rust: { line: ["//"], block: ["/*", "*/"] },
  // GNU as takes `#` for a line comment and C's block form; NASM uses `;`.
  assembly: { line: ["#", "//"], block: ["/*", "*/"] },
  intelasm: { line: [";"] },
  neper: { line: ["//"] },
  yaml: { line: ["#"] },
  sql: { line: ["--"], block: ["/*", "*/"] },
  sqlite: { line: ["--"], block: ["/*", "*/"] },
  mysql: { line: ["--", "#"], block: ["/*", "*/"] },
  postgresql: { line: ["--"], block: ["/*", "*/"] },
  dart: { line: ["//"], block: ["/*", "*/"] },
  // `{ }` rather than `(* *)`: both are Pascal block comments, but `(*` is rare
  // in modern Object Pascal and `{` is what the highlighter here already treats
  // as one — see the commentTokens in object-pascal.js.
  objectpascal: { line: ["//"], block: ["{", "}"] },
  powershell: { line: ["#"], block: ["<#", "#>"] },
  shell: { line: ["#"] },
  terraform: { line: ["#", "//"], block: ["/*", "*/"] },
  batch: { line: ["REM", "::"] },
};

/**
 * The language table the Rust side scans with.
 *
 * Built from `languages.js` on every call rather than cached: it is thirty-odd
 * small objects, built once per scan, and a cache here would be one more thing
 * to invalidate when a language is added.
 */
export function scanSpecs() {
  return Object.entries(LANGUAGE_LABELS).map(([id, label]) => ({
    label,
    extensions: LANGUAGE_EXTENSIONS[id],
    line: COMMENTS[id]?.line || [],
    block: COMMENTS[id]?.block || null,
  }));
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * How far back the deltas reach, in words: "2 hours ago", "yesterday",
 * "3 months ago".
 *
 * `Intl.RelativeTimeFormat` rather than a phrase of our own, because it is
 * already correct in every locale the app ships — including the ones where the
 * plural rule has more than two cases (Russian, Arabic) and the ones that
 * prefer "yesterday" to "1 day ago". A hand-written "last {n} hours" would have
 * needed seven new keys translated thirty-five times to arrive somewhere worse.
 *
 * Past a month the relative form stops carrying information, so the date itself
 * takes over — also from `Intl`, so it lands in the reader's own conventions.
 */
export function windowLabel(baselineIso, locale, now = Date.now()) {
  if (!baselineIso) return null;
  const at = new Date(baselineIso);
  if (Number.isNaN(at.getTime())) return null;

  const ago = Math.max(0, now - at.getTime());
  if (ago >= 30 * DAY) {
    const on = new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "short",
      day: "numeric",
    }).format(at);
    return { text: on, since: true, at };
  }

  // `numeric: "auto"` is what turns -1 day into "yesterday" rather than
  // "1 day ago" in the locales that have a word for it.
  const relative = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const [amount, unit] =
    ago < HOUR
      ? [Math.max(1, Math.round(ago / MINUTE)), "minute"]
      : ago < DAY
        ? [Math.round(ago / HOUR), "hour"]
        : [Math.round(ago / DAY), "day"];
  return { text: relative.format(-amount, unit), since: false, at };
}

/** The exact stamps, for the tooltip behind the relative wording. */
export function exactStamps(scannedIso, baselineIso, locale) {
  const format = (iso) =>
    new Intl.DateTimeFormat(locale, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(iso));
  return baselineIso ? [format(scannedIso), format(baselineIso)] : [format(scannedIso), null];
}

const FIELDS = ["files", "lines", "code", "comment", "blank"];
const ZERO = { files: 0, lines: 0, code: 0, comment: 0, blank: 0, extensions: [] };

/**
 * Joins a scan to the previous one.
 *
 * Rows come back sorted by size, which is the order someone reads a project in.
 * A language that has appeared since the last scan is marked `new` rather than
 * given a delta equal to its whole self, and one that has gone is kept for this
 * single report — a deletion is worth seeing once — with every count at zero.
 */
export function compare(current, baseline) {
  const before = baseline?.languages || {};
  const names = new Set([...Object.keys(current.languages), ...Object.keys(before)]);
  const first = !baseline;

  const rows = [...names].map((label) => {
    const now = current.languages[label] || ZERO;
    const then = before[label] || null;
    const extensions = now.extensions?.length ? now.extensions : then?.extensions || [];
    const delta = first || !then ? null : Object.fromEntries(FIELDS.map((f) => [f, now[f] - then[f]]));
    return {
      label,
      ...now,
      extensions,
      delta,
      state: first ? "first" : !then ? "new" : now.files === 0 ? "gone" : "same",
      lost: then && now.files === 0 ? then.lines : 0,
    };
  });

  rows.sort((a, b) => b.lines - a.lines || a.label.localeCompare(b.label));

  const totals = { ...current.totals };
  totals.delta = first ? null : Object.fromEntries(FIELDS.map((f) => [f, totals[f] - (baseline.totals?.[f] ?? 0)]));
  return { rows, totals };
}

/**
 * Scans the project, diffs it against `.metrics`, and leaves the new report in
 * its place.
 *
 * The report is written *after* the diff, so a scan that fails part-way leaves
 * the previous baseline intact rather than replacing it with nothing.
 */
export async function scanProject(root, locale) {
  const current = await invoke("project_metrics", { root, languages: scanSpecs() });

  let baseline = null;
  try {
    const text = await invoke("read_text_file", { path: `${root}/${METRICS_FILE}` });
    const parsed = JSON.parse(text);
    // A report from a future version may be shaped differently; a missing
    // baseline is a known state and far better than a wrong comparison.
    if (parsed?.version === 1) baseline = parsed;
  } catch (_) {
    // No previous scan, or an unreadable one. Either way this scan is the first.
  }

  const scanned = new Date().toISOString();
  const report = {
    version: 1,
    scanned,
    root: root.replace(/\\/g, "/"),
    truncated: current.truncated,
    totals: current.totals,
    languages: current.languages,
  };
  await invoke("write_text_file", {
    path: `${root}/${METRICS_FILE}`,
    contents: `${JSON.stringify(report, null, 2)}\n`,
  });

  return {
    ...compare(current, baseline),
    scanned,
    baseline: baseline?.scanned || null,
    truncated: current.truncated,
    window: windowLabel(baseline?.scanned, locale),
    stamps: exactStamps(scanned, baseline?.scanned, locale),
  };
}
