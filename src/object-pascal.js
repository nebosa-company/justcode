// A stream mode for Object Pascal / Delphi.
//
// The legacy Pascal mode that ships with CodeMirror targets standard Pascal: it
// has no `unit`/`interface`/`implementation`/`property`, no `{$...}` compiler
// directives, and no `#13` character literals — all of which are everywhere in
// Delphi source. Its keyword list is closed over inside the module, so this is
// written out rather than extended.

// Reserved words plus the directives that are never used as plain identifiers.
//
// Deliberately absent: name, index, message, read, write, readonly, writeonly,
// stored, default, implements and operator. Those are *contextual* directives —
// reserved only in particular positions — and are extremely common as ordinary
// identifiers (`Name` most of all). A stream tokenizer has no context to tell
// the two apart, so colouring them as keywords is wrong more often than right.
// Delphi's own editor treats them as identifiers too.
const KEYWORDS = new Set(
  `absolute abstract and array as asm assembler automated begin case cdecl class
   const constructor contains deprecated destructor dispid dispinterface div
   do downto dynamic else end except experimental export exports external far file
   final finalization finally for forward function goto helper if implementation
   in inherited initialization inline interface is label library
   mod near nodefault not object of on or out overload override
   package packed pascal platform private procedure program property protected public
   published raise record reference register reintroduce repeat requires
   resident resourcestring safecall sealed set shl shr static stdcall strict
   then threadvar to try type unit until uses var varargs virtual while with
   xor`.split(/\s+/),
);

const TYPES = new Set(
  `ansichar ansistring boolean byte cardinal char comp currency double extended int64
   integer longint longword nativeint nativeuint olevariant pansichar pchar pointer
   pwidechar real real48 shortint shortstring single smallint string tdatetime uint64
   variant widechar widestring word`.split(/\s+/),
);

const ATOMS = new Set(["true", "false", "nil", "self", "result"]);

/** @type {import("@codemirror/language").StreamParser<{comment: string|null}>} */
export const objectPascal = {
  name: "objectpascal",

  startState() {
    return { comment: null };
  },

  token(stream, state) {
    // Continuation of a multi-line comment or directive started earlier. The
    // directive case keeps its own colour across every line, not just the first.
    if (state.comment === "brace" || state.comment === "brace-directive") {
      const kind = state.comment === "brace-directive" ? "meta" : "comment";
      if (stream.skipTo("}")) {
        stream.next();
        state.comment = null;
      } else {
        stream.skipToEnd();
      }
      return kind;
    }
    if (state.comment === "paren") {
      while (!stream.eol()) {
        if (stream.match("*)")) {
          state.comment = null;
          return "comment";
        }
        stream.next();
      }
      return "comment";
    }

    if (stream.eatSpace()) return null;

    if (stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }

    // `{...}` is a comment, `{$...}` a compiler directive worth its own colour.
    if (stream.peek() === "{") {
      stream.next();
      const directive = stream.peek() === "$";
      if (stream.skipTo("}")) {
        stream.next();
      } else {
        stream.skipToEnd();
        state.comment = directive ? "brace-directive" : "brace";
      }
      return directive ? "meta" : "comment";
    }

    if (stream.match("(*")) {
      while (!stream.eol()) {
        if (stream.match("*)")) return "comment";
        stream.next();
      }
      state.comment = "paren";
      return "comment";
    }

    // Single-quoted strings, where '' is an escaped quote.
    if (stream.peek() === "'") {
      stream.next();
      while (!stream.eol()) {
        if (stream.next() === "'") {
          if (stream.peek() === "'") stream.next();
          else break;
        }
      }
      return "string";
    }

    if (stream.match(/^#\d+/) || stream.match(/^#\$[0-9a-fA-F]+/)) return "string";
    if (stream.match(/^\$[0-9a-fA-F]+/)) return "number";
    if (stream.match(/^\d+(\.\d+)?([eE][+-]?\d+)?/)) return "number";

    if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
      const word = stream.current().toLowerCase();
      if (ATOMS.has(word)) return "atom";
      if (KEYWORDS.has(word)) return "keyword";
      if (TYPES.has(word)) return "typeName";
      return "variableName";
    }

    if (stream.match(/^(:=|<>|<=|>=|[-+*/=<>@^.,;:()[\]])/)) return "operator";

    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: "//", block: { open: "{", close: "}" } },
    closeBrackets: { brackets: ["(", "[", "'"] },
  },
};
