// A stream mode for Windows batch files (.bat / .cmd).
//
// CodeMirror ships no batch mode, so this is hand-written. Batch syntax is
// small but quirky: `REM` and `::` both start comments, labels begin with `:`,
// variables come as %NAME% or the delayed-expansion !NAME!, and arguments are
// %1..%9.

const KEYWORDS = new Set(
  `assoc break call cd chcp chdir cls cmd color copy date del dir echo endlocal
   erase exit for ftype goto if in md mkdir mklink move path pause popd prompt
   pushd rd ren rename rmdir set setlocal shift start time title type ver verify
   vol xcopy robocopy tasklist taskkill sc net reg findstr find sort more where
   choice timeout attrib`.split(/\s+/),
);

// Words that only mean anything inside `if` / `for` clauses.
const OPERATORS = new Set(
  `not exist defined equ neq lss leq gtr geq errorlevel do else`.split(/\s+/),
);

/** @type {import("@codemirror/language").StreamParser<{continued: boolean}>} */
export const batch = {
  name: "batch",

  startState() {
    return { continued: false };
  },

  token(stream, state) {
    if (stream.eatSpace()) return null;

    // A label at the start of a line — but `::` is the comment idiom.
    if (stream.sol() && stream.peek() === ":") {
      if (stream.match("::")) {
        stream.skipToEnd();
        return "comment";
      }
      stream.next();
      stream.match(/^[^\s]*/);
      return "labelName";
    }

    if (stream.match(/^rem\b/i)) {
      stream.skipToEnd();
      return "comment";
    }

    // %VAR%, %1 and !VAR! (delayed expansion).
    if (stream.match(/^%[^%\s]*%/) || stream.match(/^%~?\d/) || stream.match(/^![^!\s]*!/)) {
      return "variableName.special";
    }

    if (stream.peek() === '"') {
      stream.next();
      while (!stream.eol() && stream.next() !== '"') {
        /* consume to the closing quote */
      }
      return "string";
    }

    // Redirections and pipes.
    if (stream.match(/^(\|\||&&|[|&<>]+)/)) return "operator";

    if (stream.match(/^@/)) return "meta"; // @echo off

    if (stream.match(/^[A-Za-z_][\w.-]*/)) {
      const word = stream.current().toLowerCase();
      if (KEYWORDS.has(word)) return "keyword";
      if (OPERATORS.has(word)) return "operator";
      return "variableName";
    }

    if (stream.match(/^\d+/)) return "number";

    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: "REM" },
  },
};
