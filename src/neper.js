// A stream mode for neper (.e).
//
// CodeMirror ships no neper grammar, and the language is small enough to
// tokenise in one pass. Token kinds follow the closed registries in the neper
// repository's `docs/grammar.ebnf`: `//` line comments (`///` documents the
// declaration below it and nothing else), one string form plus Rust-style raw
// strings, single-byte character literals, suffixed numeric literals,
// `@attributes`, a fixed 35-word keyword set, and longest-match punctuation.
// There are no block comments and no preprocessor.
//
// Casing is part of the language here rather than a convention — `neper fmt
// --check` rejects a violation the way it rejects bad indentation (spec §3,
// "Naming") — so an identifier that is not a keyword is classified by its
// spelling alone: PascalCase names a type or an error, SCREAMING_SNAKE a
// constant, snake_case a function or a value. That is sound in a way it would
// not be in most languages, because a file that spells them otherwise does not
// build.

const MODULE_KEYWORDS = new Set(["use"]);

const DECLARATION_KEYWORDS = new Set(
  "type const var let fn extern struct union enum error shared".split(" "),
);

const CONTROL_KEYWORDS = new Set(
  "if else while for in switch case default break continue defer try ret when".split(" "),
);

// Keywords that stand where a value would: the two literal pairs, the two
// initialiser forms, and the diverging builtin. They read as literals rather
// than as control flow.
const ATOMS = new Set("true false nil ok zero undef unreachable".split(" "));

const KEYWORDS = new Set(["as"]);

// The primitives of spec §4, plus `str`. `str` is deliberately *not* reserved —
// it is an alias for `[]const u8` that the language itself applies — but it is
// spelled as a type wherever it appears, so it is highlighted as one. The cost
// is that the `e.str` module qualifier picks up the type colour, which is the
// same word for the same reason.
const PRIMITIVE_TYPES = new Set(
  "i8 i16 i32 i64 isize u8 u16 u32 u64 usize f16 bf16 f32 f64 bool void err str".split(" "),
);

// Tried before INTEGER, and only matches when a `.` or an exponent follows the
// leading digits. `0..8` therefore lexes as `0` `..` `8` rather than eating the
// range operator's first dot, and `0x1f` falls through to INTEGER.
const FLOAT =
  /^\d(?:_?\d)*(?:\.\d(?:_?\d)*(?:[eE][+-]?\d(?:_?\d)*)?|[eE][+-]?\d(?:_?\d)*)(?:bf16|f16|f32|f64)?/;

// Radix prefixes come first in the alternation: `0` would otherwise match the
// decimal branch and leave `x1f` behind as an identifier.
const INTEGER =
  /^(?:0x[0-9A-Fa-f](?:_?[0-9A-Fa-f])*|0o[0-7](?:_?[0-7])*|0b[01](?:_?[01])*|\d(?:_?\d)*)(?:i8|i16|i32|i64|isize|u8|u16|u32|u64|usize)?/;

// `'a'`, `'\n'`, `'\x41'`. A character must decode to exactly one byte, which
// is a checker rule rather than a lexical one, so anything single stands here.
const CHARACTER = /^'(?:\\x[0-9A-Fa-f]{2}|\\[ntr\\"'0]|[^\\'\n])'/;

// Longest match first, in the descending ambiguity order the grammar gives:
// the three-character compound assignments before their two-character prefixes,
// and `..` before `.`.
const OPERATOR =
  /^(?:\.\.\.|\+%=|-%=|\*%=|<<=|>>=|\.\.|->|==|!=|<=|>=|<<|>>|\+%|-%|\*%|\+=|-=|\*=|\/=|%=|&=|\^=|\|=|&&|\|\||[-+*/%<>=!~&^|.,:()[\]{}])/;

/** @type {import("@codemirror/language").StreamParser<{rawHashes: number}>} */
export const neper = {
  name: "neper",

  // -1 when not inside a raw string; otherwise the number of `#` in the opening
  // delimiter, which the closing one has to match. Raw strings are the only
  // token that may span lines.
  startState() {
    return { rawHashes: -1 };
  },

  token(stream, state) {
    if (state.rawHashes >= 0) return rawStringBody(stream, state);

    if (stream.eatSpace()) return null;

    // `///` is tested before `//`, as the grammar requires: a doc comment is
    // attached to the declaration below it, an ordinary comment to nothing.
    if (stream.match("///")) {
      stream.skipToEnd();
      return "docComment";
    }
    if (stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }

    // r"…", r#"…"#, up to eight hashes.
    const raw = stream.match(/^r(#{0,8})"/);
    if (raw) {
      state.rawHashes = raw[1].length;
      return rawStringBody(stream, state);
    }

    if (stream.peek() === '"') {
      stream.next();
      // A string cannot contain a newline, so an unterminated one ends with the
      // line rather than colouring the rest of the file.
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === "\\") stream.next();
        else if (ch === '"') break;
      }
      return "string";
    }

    if (stream.peek() === "'") {
      // A half-typed literal still reads as one being written, rather than
      // flashing as an error on every keystroke.
      if (!stream.match(CHARACTER)) stream.next();
      return "string";
    }

    // `@gpu(256)`, `@nocheck`. A bare `@` is left to the operator branch.
    if (stream.match(/^@[A-Za-z_]\w*/)) return "annotation";

    if (stream.match(FLOAT)) return "number";
    if (stream.match(INTEGER)) return "number";

    if (stream.match(/^[A-Za-z_]\w*/)) {
      const word = stream.current();
      if (MODULE_KEYWORDS.has(word)) return "moduleKeyword";
      if (DECLARATION_KEYWORDS.has(word)) return "definitionKeyword";
      if (CONTROL_KEYWORDS.has(word)) return "controlKeyword";
      if (ATOMS.has(word)) return "atom";
      if (KEYWORDS.has(word)) return "keyword";
      if (PRIMITIVE_TYPES.has(word)) return "typeName.standard";
      if (/^[A-Z]/.test(word)) {
        // A lone capital is the one ambiguous case: `T` is a comptime parameter
        // of kind `type`, `N` one of a value kind, and both are spelled with a
        // single letter. Types are much the commoner of the two, so that wins.
        return /[a-z]/.test(word) || word.length === 1 ? "typeName" : "variableName.constant";
      }
      // Neper has no space between a callee and its arguments, so the next
      // character settles a call. `[` is not enough: `y[k]` indexes a slice.
      return stream.peek() === "(" ? "variableName.function" : "variableName";
    }

    if (stream.match(OPERATOR)) return "operator";

    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: "//" },
    closeBrackets: { brackets: ["(", "[", "{", '"', "'"] },
  },
};

/** Consumes raw-string content up to a closing delimiter with matching hashes. */
function rawStringBody(stream, state) {
  const close = `"${"#".repeat(state.rawHashes)}`;
  while (!stream.eol()) {
    if (stream.match(close)) {
      state.rawHashes = -1;
      return "string";
    }
    stream.next();
  }
  return "string";
}
