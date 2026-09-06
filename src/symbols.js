// Symbol outline: the declarations in the current file, for jump-to-definition.
//
// Extraction is by regular expression per language family rather than from the
// syntax tree, because several supported languages (Object Pascal, Batch,
// PowerShell, Terraform…) are stream-tokenised and have no tree to walk. A
// regex is approximate — it can miss an unusual declaration — but it works the
// same way for every language and costs nothing to run.

/**
 * Each rule turns a match into `{ name, detail, kind }`. `name` is what gets
 * searched, `detail` is the greyed-out signature beside it.
 */
const RULES = {
  objectpascal: [
    // TKicker.CheckForNewVersion(const Configuration: TKickerConfiguration): Boolean
    {
      // Line-bounded throughout. `\s*` next to a character class containing a
      // space let the two quantifiers divide the same whitespace run every
      // possible way, so a declaration with no `;` after it took 14s on a few
      // thousand spaces. A declaration lives on one line, so newlines are out.
      re: /^[ \t]*(?:class[ \t]+)?(procedure|function|constructor|destructor)[ \t]+([\w.]+)[ \t]*(\([^)\n]*\))?[ \t]*(?::[ \t]*([\w<>.,]+(?:[ \t]+[\w<>.,]+)*))?[ \t]*;/gim,
      build: (m) => ({
        name: m[2],
        detail: (m[3] || "") + (m[4] ? `: ${m[4].trim()}` : ""),
        kind: m[1].toLowerCase(),
      }),
    },
    {
      // `[^;\n]` rather than `[^;]`: a declaration with no semicolon after it
      // would otherwise swallow the rest of the file, and `matchAll` resumes at
      // the end of the match, so every later declaration went unseen.
      re: /^[ \t]*([\w]+)\s*=\s*(class|record|interface)\b[^;\n]*/gim,
      build: (m) => ({ name: m[1], detail: ` = ${m[2]}`, kind: "type" }),
    },
  ],
  javascript: [
    {
      re: /^[ \t]*(?:export[ \t]+(?:default[ \t]+)?)?(?:async[ \t]+)?function[ \t]*\*?[ \t]*([\w$]+)[ \t]*(\([^)\n]*\))/gim,
      build: (m) => ({ name: m[1], detail: m[2], kind: "function" }),
    },
    {
      re: /^[ \t]*(?:export\s+)?(?:abstract\s+)?class\s+([\w$]+)(\s+extends\s+[\w$.]+)?/gim,
      build: (m) => ({ name: m[1], detail: m[2] || "", kind: "class" }),
    },
    {
      re: /^[ \t]*(?:export\s+)?(?:const|let|var)\s+([\w$]+)\s*=\s*(?:async\s*)?(\([^)]*\)|[\w$]+)\s*=>/gim,
      build: (m) => ({ name: m[1], detail: `${m[2]} =>`, kind: "function" }),
    },
    // Class methods: an indented `name(args) {`, excluding control keywords.
    {
      re: /^[ \t]+(?:static\s+|async\s+|get\s+|set\s+)*([\w$]+)\s*(\([^)]*\))\s*\{/gim,
      build: (m) =>
        /^(if|for|while|switch|catch|return|function|do|else)$/.test(m[1])
          ? null
          : { name: m[1], detail: m[2], kind: "method" },
    },
  ],
  python: [
    {
      re: /^[ \t]*(?:async\s+)?def\s+([\w]+)\s*(\([^)]*\))/gim,
      build: (m) => ({ name: m[1], detail: m[2], kind: "function" }),
    },
    {
      re: /^[ \t]*class\s+([\w]+)\s*(\([^)]*\))?/gim,
      build: (m) => ({ name: m[1], detail: m[2] || "", kind: "class" }),
    },
  ],
  rust: [
    {
      re: /^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([\w]+)\s*(\([^)]*\))/gim,
      build: (m) => ({ name: m[1], detail: m[2], kind: "function" }),
    },
    {
      re: /^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(struct|enum|trait|impl|mod)\s+([\w]+)/gim,
      build: (m) => ({ name: m[2], detail: ` ${m[1]}`, kind: m[1] }),
    },
  ],
  // Neper declarations start at column 0 (spec §3), so these anchor there
  // rather than allowing indentation: a `fn` further in is a function *type*
  // inside a signature, not a declaration. Case-sensitive, unlike the rules
  // above — every keyword here is lower-case, and `Fn` would be a type name.
  neper: [
    {
      // `[T: type]` comptime parameters sit between the name and the signature.
      // The parameter list excludes `{` rather than `)` so that a `fn` type in
      // a parameter — `f: fn(i32) -> i32` — keeps its own parentheses, while a
      // one-line body cannot be swallowed: the greedy run stops at the brace
      // and backtracks to the last `)` before it.
      re: /^(?:extern[ \t]+)?fn[ \t]+(\w+)[ \t]*(\[[^\]\n]*\])?[ \t]*(\([^\n{]*\))(?:[ \t]*->[ \t]*([^\n{]+))?/gm,
      build: (m) => ({
        name: m[1],
        detail: `${m[2] || ""}${m[3]}${m[4] ? ` -> ${m[4].trim()}` : ""}`,
        kind: "function",
      }),
    },
    {
      // The composite keywords are listed before the catch-all so that
      // `= struct {` reports `struct` and `= u8` reports the alias target.
      re: /^type[ \t]+(\w+)[ \t]*(\[[^\]\n]*\])?[ \t]*=[ \t]*(union[ \t]+enum|struct|union|enum|[^\n{]+)/gm,
      build: (m) => {
        const rhs = m[3].trim().replace(/[ \t]+/g, " ");
        const composite = /^(?:union enum|struct|union|enum)$/.test(rhs);
        return { name: m[1], detail: `${m[2] || ""} = ${rhs}`, kind: composite ? rhs : "type" };
      },
    },
    { re: /^error[ \t]+(\w+)/gm, build: (m) => ({ name: m[1], detail: "", kind: "error" }) },
    {
      // `const` always initialises; a top-level `var` need not, so the `=` is
      // optional and the type annotation carries the detail on its own.
      re: /^(const|var)[ \t]+(\w+)(?:[ \t]*:[ \t]*([^\n=]+?))?[ \t]*(?:=|$)/gm,
      build: (m) => ({ name: m[2], detail: m[3] ? `: ${m[3].trim()}` : "", kind: m[1] }),
    },
  ],
  cpp: [
    {
      re: /^[ \t]*(?:[\w:<>*&~]+[ \t]+)+([\w:~]+)\s*(\([^;{)]*\))\s*(?:const\s*)?\{/gim,
      build: (m) =>
        /^(if|for|while|switch|catch|return|else)$/.test(m[1])
          ? null
          : { name: m[1], detail: m[2], kind: "function" },
    },
    {
      re: /^[ \t]*(class|struct|namespace|enum)\s+([\w]+)/gim,
      build: (m) => ({ name: m[2], detail: ` ${m[1]}`, kind: m[1] }),
    },
  ],
  powershell: [
    {
      re: /^[ \t]*function\s+([\w-]+)/gim,
      build: (m) => ({ name: m[1], detail: "", kind: "function" }),
    },
  ],
  shell: [
    {
      re: /^[ \t]*(?:function\s+)?([\w-]+)\s*\(\)\s*\{/gim,
      build: (m) => ({ name: m[1], detail: "()", kind: "function" }),
    },
  ],
  batch: [
    { re: /^[ \t]*:([\w.-]+)/gim, build: (m) => ({ name: m[1], detail: "", kind: "label" }) },
  ],
  terraform: [
    {
      re: /^[ \t]*(resource|data)\s+"([^"]+)"\s+"([^"]+)"/gim,
      build: (m) => ({ name: `${m[2]}.${m[3]}`, detail: ` ${m[1]}`, kind: m[1] }),
    },
    {
      // `\b` after the group: without it `variables_file = "x"` matches the
      // `variable` prefix and captures `s_file` as a symbol name.
      re: /^[ \t]*(variable|output|module|provider|locals|terraform)\b\s*"?([\w-]*)"?/gim,
      build: (m) => ({ name: m[2] || m[1], detail: ` ${m[1]}`, kind: m[1] }),
    },
  ],
  markdown: [
    {
      re: /^(#{1,6})\s+(.+)$/gim,
      build: (m) => ({ name: m[2].trim(), detail: ` h${m[1].length}`, kind: "heading" }),
    },
  ],
  css: [
    {
      // Excluding newlines keeps this to one line per selector. Allowing them
      // made the scan quadratic — every line rescanned to end of file looking
      // for a brace — and merged multi-line selector lists (and stretches of
      // comment) into a single unusable entry.
      re: /^[ \t]*([.#]?[\w-][^{;@\n]*?)\s*\{/gim,
      build: (m) => ({ name: m[1].trim(), detail: "", kind: "rule" }),
    },
  ],
  html: [
    {
      // `\bid=` also matched `data-id=`; requiring whitespace before it fixes
      // that. `[^>\n]` keeps an unclosed `<` from swallowing the whole file,
      // which reported one symbol positioned at the wrong place entirely.
      re: /<(\w+)[^>\n]*?\sid=["']([^"']+)["']/gim,
      build: (m) => ({ name: `#${m[2]}`, detail: ` <${m[1]}>`, kind: "id" }),
    },
  ],
  json: [
    // Only top-level keys; nested ones would drown the list.
    { re: /^[ \t]{0,2}"([^"]+)"[ \t]*:/gim, build: (m) => ({ name: m[1], detail: "", kind: "key" }) },
  ],
};

// Languages that share another's declaration syntax closely enough.
const ALIASES = {
  typescript: "javascript",
  jsx: "javascript",
  tsx: "javascript",
  java: "cpp",
  csharp: "cpp",
  kotlin: "cpp",
  swift: "cpp",
  dart: "cpp",
  c: "cpp",
  yaml: null,
  sql: null,
};

/** Declarations in `text`, in document order, for the given language. */
export function findSymbols(text, languageId) {
  const key = ALIASES[languageId] === undefined ? languageId : ALIASES[languageId];
  const rules = RULES[key];
  if (!rules) return [];

  const found = [];
  for (const { re, build } of rules) {
    re.lastIndex = 0;
    for (const match of text.matchAll(re)) {
      const symbol = build(match);
      if (!symbol || !symbol.name) continue;
      found.push({ ...symbol, pos: match.index });
    }
  }
  found.sort((a, b) => a.pos - b.pos);
  return found;
}

/** True when this language has any extraction rules at all. */
export function supportsSymbols(languageId) {
  const key = ALIASES[languageId] === undefined ? languageId : ALIASES[languageId];
  return Boolean(RULES[key]);
}
