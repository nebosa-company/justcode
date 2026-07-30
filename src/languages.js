// Language registry.
//
// Grammars are pulled in with dynamic imports so only the language actually
// being edited is downloaded and evaluated. Loading all of them eagerly put
// every parser (plus Acorn, via the linters) into the startup bundle, which is
// work the editor does not need to boot. Metadata — labels and extensions — is
// plain data and stays synchronous, so file dialogs, the mode picker and
// extension lookup work before any grammar has loaded.
//
// Linting is deliberately absent for Markdown (no such thing as invalid
// Markdown), for the stream-based modes (they produce no error nodes), and for
// TypeScript/JSX (Acorn would flag valid type annotations and tags).

import { t } from "./i18n.js";
/** @type {Record<string, {label: string, extensions: string[], load?: () => Promise<any[]>}>} */
const LANGUAGES = {
  html: {
    label: "HTML",
    extensions: ["html", "htm", "xhtml"],
    load: async () => {
      const [{ html }, { htmlLinter }] = await Promise.all([
        import("@codemirror/lang-html"),
        import("./linters.js"),
      ]);
      // `html()` already nests the JavaScript and CSS grammars, which brings
      // their own completion of locally-declared names. It does not bring the
      // global scope, so a <script> block would complete `myHelper` but not
      // `document` — the same source the .js path installs is added here too.
      return [
        html({ autoCloseTags: true, matchClosingTags: true }),
        await javascriptGlobals(),
        htmlLinter,
      ];
    },
  },
  css: {
    label: "CSS",
    extensions: ["css"],
    load: async () => {
      const [{ css }, { cssLinter }] = await Promise.all([
        import("@codemirror/lang-css"),
        import("./linters.js"),
      ]);
      return [css(), cssLinter];
    },
  },
  javascript: {
    label: "JavaScript",
    extensions: ["js", "mjs", "cjs"],
    load: async () => {
      const [{ javascript }, { jsLinter }] = await Promise.all([
        import("@codemirror/lang-javascript"),
        import("./linters.js"),
      ]);
      return [javascript(), await javascriptGlobals(), jsLinter];
    },
  },
  // TypeScript and JSX reuse the JavaScript grammar with flags, and skip the
  // Acorn linter, which only understands plain JS.
  typescript: {
    label: "TypeScript",
    extensions: ["ts", "mts", "cts"],
    load: async () => [(await import("@codemirror/lang-javascript")).javascript({ typescript: true })],
  },
  jsx: {
    label: "JavaScript (JSX)",
    extensions: ["jsx"],
    load: async () => [(await import("@codemirror/lang-javascript")).javascript({ jsx: true })],
  },
  tsx: {
    label: "TypeScript (TSX)",
    extensions: ["tsx"],
    load: async () => [
      (await import("@codemirror/lang-javascript")).javascript({ typescript: true, jsx: true }),
    ],
  },
  markdown: {
    label: "Markdown",
    extensions: ["md", "markdown", "mdown", "mkd"],
    load: async () => [(await import("@codemirror/lang-markdown")).markdown()],
  },
  python: {
    label: "Python",
    extensions: ["py", "pyw", "pyi"],
    load: async () => [(await import("@codemirror/lang-python")).python()],
  },
  cpp: {
    label: "C / C++",
    extensions: ["c", "cc", "cpp", "cxx", "h", "hpp", "hh", "hxx"],
    // `.c` comes first so a C file gets the friendlier filter, but a new file
    // from the template is C++ — it opens with <iostream>, not <stdio.h>.
    newFileExtension: "cpp",
    load: async () => [(await import("@codemirror/lang-cpp")).cpp()],
  },
  java: {
    label: "Java",
    extensions: ["java"],
    load: async () => [(await import("@codemirror/lang-java")).java()],
  },
  csharp: {
    label: "C#",
    extensions: ["cs", "csx"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/clike"), "csharp"),
  },
  kotlin: {
    label: "Kotlin",
    extensions: ["kt", "kts"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/clike"), "kotlin"),
  },
  swift: {
    label: "Swift",
    extensions: ["swift"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/swift"), "swift"),
  },
  r: {
    label: "R",
    extensions: ["r"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/r"), "r"),
  },
  toml: {
    label: "TOML",
    extensions: ["toml"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/toml"), "toml"),
  },
  protobuf: {
    label: "Protocol Buffers",
    extensions: ["proto"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/protobuf"), "protobuf"),
  },
  go: {
    label: "Go",
    extensions: ["go"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/go"), "go"),
  },
  rust: {
    label: "Rust",
    extensions: ["rs"],
    load: async () => {
      const [{ rust }, { treeLinter }] = await Promise.all([
        import("@codemirror/lang-rust"),
        import("./linters.js"),
      ]);
      return [rust(), treeLinter];
    },
  },
  json: {
    label: "JSON",
    extensions: ["json", "jsonc", "webmanifest"],
    load: async () => {
      const [{ json, jsonParseLinter }, { linter }] = await Promise.all([
        import("@codemirror/lang-json"),
        import("@codemirror/lint"),
      ]);
      return [json(), linter(jsonParseLinter())];
    },
  },
  yaml: {
    label: "YAML",
    extensions: ["yaml", "yml"],
    load: async () => {
      const [{ yaml }, { treeLinter }] = await Promise.all([
        import("@codemirror/lang-yaml"),
        import("./linters.js"),
      ]);
      return [yaml(), treeLinter];
    },
  },
  // Delphi project files (.dproj) are XML; tag-balance linting applies cleanly.
  xml: {
    label: "XML",
    extensions: ["xml", "dproj", "xsd", "xsl", "xslt"],
    load: async () => {
      const [{ xml }, { xmlLinter }] = await Promise.all([
        import("@codemirror/lang-xml"),
        import("./linters.js"),
      ]);
      return [xml(), xmlLinter];
    },
  },
  // SQL dialects share one package, differing in keyword set and quoting rules.
  // A bare .sql file is ambiguous, so it gets the generic dialect.
  sql: { label: "SQL", extensions: ["sql"], load: () => sqlMode("StandardSQL") },
  sqlite: { label: "SQL (SQLite)", extensions: ["sqlite", "sqlite3"], load: () => sqlMode("SQLite") },
  mysql: { label: "SQL (MySQL)", extensions: ["mysql"], load: () => sqlMode("MySQL") },
  postgresql: {
    label: "SQL (PostgreSQL)",
    extensions: ["pgsql", "psql"],
    load: () => sqlMode("PostgreSQL"),
  },
  dart: {
    label: "Dart",
    extensions: ["dart"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/clike"), "dart"),
  },
  objectpascal: {
    label: "Object Pascal",
    extensions: ["pas", "pp", "dpr", "dpk", "lpr", "inc"],
    load: async () => {
      const [{ StreamLanguage }, { objectPascal }] = await Promise.all([
        import("@codemirror/language"),
        import("./object-pascal.js"),
      ]);
      return [StreamLanguage.define(objectPascal)];
    },
  },
  powershell: {
    label: "PowerShell",
    extensions: ["ps1", "psm1", "psd1"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/powershell"), "powerShell"),
  },
  shell: {
    label: "Shell",
    extensions: ["sh", "bash", "zsh", "ksh"],
    load: async () => streamMode(import("@codemirror/legacy-modes/mode/shell"), "shell"),
  },
  terraform: {
    label: "Terraform",
    extensions: ["tf", "tfvars", "hcl"],
    load: async () => {
      const [{ StreamLanguage }, { terraform }] = await Promise.all([
        import("@codemirror/language"),
        import("./terraform.js"),
      ]);
      return [StreamLanguage.define(terraform)];
    },
  },
  batch: {
    label: "Batch",
    extensions: ["bat", "cmd"],
    load: async () => {
      const [{ StreamLanguage }, { batch }] = await Promise.all([
        import("@codemirror/language"),
        import("./batch.js"),
      ]);
      return [StreamLanguage.define(batch)];
    },
  },
  // No highlighting, but recognised so these get a file filter, an OS
  // association, and a "Plain Text" label rather than falling through silently.
  // There is no dedicated CSV mode worth pulling in, so .csv is plain text too.
  text: { label: "Plain Text", extensions: ["txt", "log", "csv"] },
};

/**
 * Completion for the JavaScript global scope — `document`, `window`, `Math`
 * and everything else reachable from `globalThis`.
 *
 * It attaches to the JavaScript *language*, not to a file type, so registering
 * it once covers plain .js files and `<script>` blocks inside HTML alike.
 */
async function javascriptGlobals() {
  const { javascriptLanguage, scopeCompletionSource } = await import("@codemirror/lang-javascript");
  return javascriptLanguage.data.of({ autocomplete: scopeCompletionSource(globalThis) });
}

/** Wraps a legacy stream mode from `@codemirror/legacy-modes` as a language. */
async function streamMode(modulePromise, exportName) {
  const [{ StreamLanguage }, mode] = await Promise.all([
    import("@codemirror/language"),
    modulePromise,
  ]);
  return [StreamLanguage.define(mode[exportName])];
}

async function sqlMode(dialectName) {
  const mod = await import("@codemirror/lang-sql");
  return [mod.sql({ dialect: mod[dialectName] })];
}

const BY_EXTENSION = new Map();
for (const [id, language] of Object.entries(LANGUAGES)) {
  for (const extension of language.extensions) BY_EXTENSION.set(extension, id);
}

/** Every extension the app claims, for file dialogs and OS file associations. */
export const ALL_EXTENSIONS = [...BY_EXTENSION.keys()];

/**
 * How the file-association screen groups things. A language is one group, and
 * within it each extension is named individually, because a Delphi user may
 * well want .pas and .dpr but not .inc.
 *
 * Only extensions distinctive enough to be worth claiming are listed. The ones
 * held back are those another tool almost certainly owns — .h and .c belong to
 * a C toolchain, .json/.xml/.sql to whatever the user already uses — so taking
 * them by default would be rude. They stay openable from inside the app.
 */
const EXTENSION_LABELS = {
  pas: "Object Pascal unit",
  pp: "Free Pascal unit",
  inc: "Include file",
  dpr: "Delphi project",
  dpk: "Delphi package",
  lpr: "Lazarus project",
  dproj: "Delphi project file",
  html: "HTML document",
  htm: "HTML document",
  xhtml: "XHTML document",
  css: "Style sheet",
  js: "JavaScript",
  mjs: "JavaScript module",
  cjs: "CommonJS module",
  ts: "TypeScript",
  tsx: "TypeScript JSX",
  jsx: "JavaScript JSX",
  md: "Markdown",
  markdown: "Markdown",
  py: "Python script",
  rs: "Rust source",
  go: "Go source",
  dart: "Dart source",
  java: "Java source",
  kt: "Kotlin source",
  swift: "Swift source",
  cs: "C# source",
  cpp: "C++ source",
  c: "C source",
  h: "C/C++ header",
  r: "R script",
  sql: "SQL script",
  sh: "Shell script",
  bash: "Bash script",
  ps1: "PowerShell script",
  psm1: "PowerShell module",
  bat: "Batch file",
  cmd: "Command script",
  json: "JSON document",
  yaml: "YAML document",
  yml: "YAML document",
  toml: "TOML document",
  xml: "XML document",
  proto: "Protocol Buffers",
  tf: "Terraform config",
  tfvars: "Terraform variables",
  hcl: "HCL document",
  txt: "Plain text",
  log: "Log file",
  csv: "Comma-separated values",
};

// Extensions left unticked unless the user asks for them.
const NOT_BY_DEFAULT = new Set([
  "h", "hpp", "hh", "hxx", "c", "cc", "cxx", "json", "jsonc", "webmanifest",
  "xml", "xsd", "xsl", "xslt", "sql", "sqlite", "sqlite3", "mysql", "pgsql",
  "psql", "csv", "log", "txt", "inc",
]);

/**
 * The association tree: one entry per language, each with its extensions.
 * `recommended` marks the ones ticked when nothing has been chosen before.
 */
export function associationGroups() {
  return Object.entries(LANGUAGES)
    .map(([id, language]) => ({
      id,
      label: language.label,
      extensions: language.extensions.map((extension) => ({
        extension,
        label: EXTENSION_LABELS[extension] || t("lang.fileOf", { name: language.label }),
        recommended: !NOT_BY_DEFAULT.has(extension),
      })),
    }))
    .filter((group) => group.extensions.length > 0)
    .sort((a, b) => a.label.localeCompare(b.label));
}

export const LANGUAGE_LABELS = Object.fromEntries(
  Object.entries(LANGUAGES).map(([id, language]) => [id, language.label]),
);

/** Every language as `{ id, label }`, sorted by label for the mode picker. */
export function languageList() {
  return Object.entries(LANGUAGES)
    .map(([id, language]) => ({ id, label: language.label }))
    .sort((a, b) => a.label.localeCompare(b.label));
}

/** The extension a new file of this type should be given. */
export function primaryExtension(languageId) {
  const language = LANGUAGES[languageId];
  return language?.newFileExtension || language?.extensions[0] || "txt";
}

/** Maps a file name to a language id, falling back to plain text. */
export function languageIdFor(fileName) {
  // Reduce to the basename first so a dot in a parent directory can't be
  // mistaken for the file's extension.
  const name = (fileName || "").toLowerCase().split(/[\\/]/).pop();
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "text"; // no extension, or a dotfile like ".gitignore"
  return BY_EXTENSION.get(name.slice(dot + 1)) || "text";
}

const loadedExtensions = new Map();
const inFlight = new Map();

/** Extensions for a language that is already loaded; `[]` until then. */
export function loadedLanguageExtensions(languageId) {
  return loadedExtensions.get(languageId) || [];
}

/**
 * Loads a language's grammar, caching it so switching back to a tab is instant.
 * A failed import degrades to plain text rather than breaking the editor.
 */
export function ensureLanguage(languageId) {
  if (loadedExtensions.has(languageId)) {
    return Promise.resolve(loadedExtensions.get(languageId));
  }
  if (inFlight.has(languageId)) return inFlight.get(languageId);

  const language = LANGUAGES[languageId] || LANGUAGES.text;
  const promise = Promise.resolve(language.load ? language.load() : [])
    .then((extensions) => {
      loadedExtensions.set(languageId, extensions);
      inFlight.delete(languageId);
      return extensions;
    })
    // Only success is cached. Caching the failure too meant one hiccup left
    // that language as plain text for the rest of the session, with no retry.
    .catch(() => {
      inFlight.delete(languageId);
      return [];
    });
  inFlight.set(languageId, promise);
  return promise;
}

/** Filters for the open/save dialogs. */
export function fileFilters() {
  return [
    { name: "All supported files", extensions: ALL_EXTENSIONS },
    ...Object.values(LANGUAGES)
      .filter((language) => language.extensions.length > 0)
      .map((language) => ({ name: language.label, extensions: language.extensions })),
    { name: "All files", extensions: ["*"] },
  ];
}
