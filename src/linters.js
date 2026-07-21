import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import { linter } from "@codemirror/lint";
import { parse as acornParse } from "acorn";

const MAX_DIAGNOSTICS = 200;

// How long to let the parser catch up before linting. Nested <script>/<style>
// regions are parsed lazily, so reading the tree without this returns a
// truncated view of the document. Kept small so a huge document cannot stall
// the main thread on every lint pass — the partial tree is linted, and the next
// pass covers the rest.
const PARSE_TIMEOUT_MS = 500;

// Tags that never carry a closing tag in HTML.
const VOID_TAGS = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input",
  "link", "meta", "param", "source", "track", "wbr",
]);

// Tags whose end tag is optional in HTML — the parser closes them implicitly, so
// leaving them "open" is not an error and must not be reported as unclosed.
const OPTIONAL_END_TAGS = new Set([
  "html", "head", "body", "p", "li", "dd", "dt", "td", "th", "tr",
  "thead", "tbody", "tfoot", "caption", "colgroup", "option", "optgroup",
  "rp", "rt", "rtc", "rb",
]);

// <script> bodies worth parsing as JavaScript. Anything else (JSON-LD, an
// x-template, importmap) is data and must not be reported as broken code.
const JS_SCRIPT_TYPES = new Set([
  "module",
  "text/javascript",
  "application/javascript",
  "text/ecmascript",
  "application/ecmascript",
]);

/** A blank buffer is not a mistake — nothing to report until something is typed. */
function isBlank(state) {
  return state.doc.length === 0 || !/\S/.test(state.doc.toString());
}

function fullTree(state) {
  return ensureSyntaxTree(state, state.doc.length, PARSE_TIMEOUT_MS) || syntaxTree(state);
}

/**
 * Reports the error nodes the Lezer parser inserted while recovering.
 * This is what catches unbalanced braces, stray operators and malformed
 * CSS rules, in whichever language the document happens to be.
 */
function syntaxErrors(state, tree) {
  const diagnostics = [];
  let lastEnd = -1;
  tree.iterate({
    enter(node) {
      if (!node.type.isError || diagnostics.length >= MAX_DIAGNOSTICS) return;
      // Zero-width error nodes mark a *missing* token; give them a caret wide
      // enough to be clickable.
      const from = node.from;
      const to = node.to > node.from ? node.to : Math.min(node.from + 1, state.doc.length);
      if (from <= lastEnd) return; // collapse cascades of adjacent errors
      lastEnd = to;
      const found = state.doc.sliceString(from, to).trim();
      diagnostics.push({
        from,
        to,
        severity: "error",
        message: found
          ? `Unexpected syntax near "${truncate(found)}"`
          : "Missing or incomplete syntax here",
      });
    },
  });
  return diagnostics;
}

/**
 * `color: ;` is valid enough for the CSS parser to recover from without
 * flagging anything, but it is always a mistake, so check declarations by hand.
 */
function cssValueErrors(state, tree) {
  const diagnostics = [];
  tree.iterate({
    enter(node) {
      if (node.name !== "Declaration") return;
      const last = node.node.lastChild;
      if (last && last.name !== ":") return;
      const property = node.node.firstChild;
      const name = property ? state.doc.sliceString(property.from, property.to) : "declaration";
      // An empty custom property value (`--x: ;`) is legal CSS, so skip those.
      if (name.startsWith("--")) return;
      diagnostics.push({
        from: node.from,
        to: node.to,
        severity: "error",
        message: `Missing value for "${name}"`,
      });
    },
  });
  return diagnostics;
}

function truncate(text, max = 24) {
  const flat = text.replace(/\s+/g, " ");
  return flat.length > max ? `${flat.slice(0, max)}…` : flat;
}

/**
 * Parses a JavaScript fragment with Acorn and turns a thrown SyntaxError into a
 * diagnostic. `offset` maps fragment positions back into the outer document.
 */
function parseJs(source, offset, docLength) {
  if (!/\S/.test(source)) return [];
  // Try both module and classic-script parsing; the code is only wrong if
  // *neither* accepts it. This avoids flagging module-only syntax (import) in
  // one mode and sloppy-mode syntax (`with`, octal literals) in the other.
  let moduleError = null;
  for (const sourceType of ["module", "script"]) {
    try {
      acornParse(source, {
        ecmaVersion: "latest",
        sourceType,
        allowReturnOutsideFunction: true,
        allowAwaitOutsideFunction: true,
        allowHashBang: true,
      });
      return [];
    } catch (err) {
      if (sourceType === "module") moduleError = err;
    }
  }
  const err = moduleError;
  if (!(err instanceof SyntaxError) || typeof err.pos !== "number") return [];
  const from = Math.min(offset + err.pos, docLength);
  return [
    {
      from,
      to: Math.min(from + 1, docLength),
      severity: "error",
      // Acorn appends "(line:col)"; those numbers are relative to the
      // fragment, so they would be wrong for an embedded <script>.
      message: err.message.replace(/\s*\(\d+:\d+\)$/, ""),
    },
  ];
}

/** Linter for standalone .js files. */
export const jsLinter = linter((view) => {
  const state = view.state;
  if (isBlank(state)) return [];
  const diagnostics = parseJs(state.doc.toString(), 0, state.doc.length);
  // Acorn stops at the first error; the tree walk fills in the rest.
  return diagnostics.length ? diagnostics : syntaxErrors(state, fullTree(state));
});

/**
 * Linter for any Lezer-parsed language without checks of its own (Rust, YAML):
 * whatever the grammar could not make sense of is reported. Stream-based modes
 * and grammars prone to false positives on valid code get no linter at all.
 */
export const treeLinter = linter((view) => {
  const state = view.state;
  if (isBlank(state)) return [];
  return syntaxErrors(state, fullTree(state));
});

/**
 * Linter for .xml files. The XML grammar records structural mistakes as
 * dedicated nodes (`MissingCloseTag`, `MismatchedCloseTag`) rather than generic
 * error nodes, so those are reported by hand, then any remaining error nodes.
 */
export const xmlLinter = linter((view) => {
  const state = view.state;
  if (isBlank(state)) return [];
  const doc = state.doc;
  const diagnostics = [];

  fullTree(state).iterate({
    enter(node) {
      if (node.name === "MissingCloseTag") {
        const element = node.node.parent;
        const openTag = element && element.getChild("OpenTag");
        const tagName = openTag && openTag.getChild("TagName");
        const name = tagName ? doc.sliceString(tagName.from, tagName.to) : "element";
        const from = openTag ? openTag.from : node.from;
        const to = openTag ? openTag.to : Math.min(node.from + 1, doc.length);
        diagnostics.push({ from, to, severity: "error", message: `<${name}> is never closed` });
      } else if (node.name === "MismatchedCloseTag") {
        const match = /^<\/\s*([\w:.-]+)/.exec(doc.sliceString(node.from, node.to));
        diagnostics.push({
          from: node.from,
          to: node.to,
          severity: "error",
          message: match ? `Closing tag </${match[1]}> does not match` : "Mismatched closing tag",
        });
      }
    },
  });

  diagnostics.push(...syntaxErrors(state, fullTree(state)));
  return diagnostics.sort((a, b) => a.from - b.from).slice(0, MAX_DIAGNOSTICS);
});

/** Linter for standalone .css files. */
export const cssLinter = linter((view) => {
  const state = view.state;
  if (isBlank(state)) return [];
  const tree = fullTree(state);
  return [...cssValueErrors(state, tree), ...syntaxErrors(state, tree)]
    .sort((a, b) => a.from - b.from)
    .slice(0, MAX_DIAGNOSTICS);
});

/** Reads the `type` attribute out of a raw `<script ...>` open tag. */
function isJavaScriptTag(openTagText) {
  const match = /\stype\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))/i.exec(openTagText);
  if (!match) return true; // no type attribute means classic JavaScript
  const value = (match[2] ?? match[3] ?? match[4] ?? "").trim().toLowerCase();
  return value === "" || JS_SCRIPT_TYPES.has(value);
}

/**
 * Linter for .html files: checks that every element is closed and in the right
 * order, then runs Acorn over each embedded <script> block.
 */
export const htmlLinter = linter((view) => {
  const state = view.state;
  const doc = state.doc;
  if (isBlank(state)) return [];

  const structural = []; // unclosed / unmatched tags
  const embedded = []; // Acorn results from <script> bodies
  const open = [];
  const tree = fullTree(state);

  tree.iterate({
    enter(node) {
      // The parser names a closer it cannot pair up "MismatchedCloseTag" and
      // keeps it out of the element structure entirely.
      if (node.name === "MismatchedCloseTag") {
        const name = /^<\/\s*([A-Za-z][-\w:.]*)/.exec(doc.sliceString(node.from, node.to));
        structural.push({
          from: node.from,
          to: node.to,
          severity: "error",
          message: name
            ? `Closing tag </${name[1].toLowerCase()}> has no matching opening tag`
            : "Closing tag has no matching opening tag",
        });
        return;
      }
      if (node.name !== "OpenTag" && node.name !== "CloseTag") return;
      const text = doc.sliceString(node.from, node.to);
      const match = /^<\/?\s*([A-Za-z][-\w:.]*)/.exec(text);
      if (!match) return;
      const tag = match[1].toLowerCase();

      if (node.name === "OpenTag") {
        if (!VOID_TAGS.has(tag)) open.push({ tag, from: node.from, to: node.to, text });
        return;
      }

      const index = findLast(open, (entry) => entry.tag === tag);
      if (index === -1) {
        structural.push({
          from: node.from,
          to: node.to,
          severity: "error",
          message: `Closing tag </${tag}> has no matching opening tag`,
        });
        return;
      }
      // Everything opened after the matched tag was never closed — but tags
      // with optional end tags (li, td, p…) are closed implicitly by HTML, so
      // they are not errors.
      for (const orphan of open.splice(index + 1)) {
        if (OPTIONAL_END_TAGS.has(orphan.tag)) continue;
        structural.push({
          from: orphan.from,
          to: orphan.to,
          severity: "error",
          message: `<${orphan.tag}> is never closed`,
        });
      }
      const opener = open.pop();
      // The tree mounts embedded JavaScript lazily and under varying node
      // names, so take the body straight from between the two tags.
      if (tag === "script" && isJavaScriptTag(opener.text)) {
        embedded.push(
          ...parseJs(doc.sliceString(opener.to, node.from), opener.to, doc.length),
        );
      }
    },
  });

  for (const orphan of open) {
    if (OPTIONAL_END_TAGS.has(orphan.tag)) continue;
    structural.push({
      from: orphan.from,
      to: orphan.to,
      severity: "error",
      message: `<${orphan.tag}> is never closed`,
    });
  }

  // Once the tags are unbalanced the rest of the tree is guesswork, so the
  // generic error nodes it produces are symptoms, not separate problems. The
  // same holds for the parser's take on a script Acorn has already rejected.
  const generic = structural.length
    ? []
    : syntaxErrors(state, tree).filter(
        (error) => !embedded.some((known) => known.from < error.to && error.from < known.to),
      );

  return [...structural, ...embedded, ...cssValueErrors(state, tree), ...generic]
    .sort((a, b) => a.from - b.from)
    .slice(0, MAX_DIAGNOSTICS);
});

function findLast(array, predicate) {
  for (let i = array.length - 1; i >= 0; i--) {
    if (predicate(array[i])) return i;
  }
  return -1;
}
