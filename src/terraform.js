// A stream mode for Terraform / HCL (.tf, .tfvars, .hcl).
//
// CodeMirror ships no HCL mode. The syntax is small: blocks with optional
// quoted labels, `key = value` attributes, `#` and `//` line comments, `/* */`
// blocks, heredocs, and `${…}` interpolation inside strings.

const BLOCK_KEYWORDS = new Set(
  "resource data variable output module provider locals terraform provisioner backend connection lifecycle dynamic moved import check removed".split(
    " ",
  ),
);

const KEYWORDS = new Set("for in if else endif endfor can try var local each count".split(" "));

const ATOMS = new Set(["true", "false", "null"]);

/** @type {import("@codemirror/language").StreamParser<{comment: boolean, heredoc: string|null}>} */
export const terraform = {
  name: "terraform",

  startState() {
    return { comment: false, heredoc: null };
  },

  token(stream, state) {
    // A heredoc runs until its terminator appears alone on a line.
    if (state.heredoc) {
      if (stream.sol() && stream.match(new RegExp(`^\\s*${state.heredoc}\\s*$`))) {
        state.heredoc = null;
        return "string";
      }
      stream.skipToEnd();
      return "string";
    }

    if (state.comment) {
      while (!stream.eol()) {
        if (stream.match("*/")) {
          state.comment = false;
          return "comment";
        }
        stream.next();
      }
      return "comment";
    }

    if (stream.eatSpace()) return null;

    if (stream.match("#") || stream.match("//")) {
      stream.skipToEnd();
      return "comment";
    }
    if (stream.match("/*")) {
      state.comment = true;
      return "comment";
    }

    // <<EOT / <<-EOT
    const heredoc = stream.match(/^<<-?([A-Za-z_]\w*)/);
    if (heredoc) {
      state.heredoc = heredoc[1];
      return "string";
    }

    if (stream.peek() === '"') {
      stream.next();
      while (!stream.eol()) {
        const ch = stream.next();
        if (ch === "\\") {
          stream.next();
        } else if (ch === '"') {
          break;
        }
      }
      return "string";
    }

    if (stream.match(/^\$\{/)) return "variableName.special";

    if (stream.match(/^\d+(\.\d+)?/)) return "number";

    if (stream.match(/^[A-Za-z_][\w-]*/)) {
      const word = stream.current();
      if (ATOMS.has(word)) return "atom";
      if (BLOCK_KEYWORDS.has(word)) return "keyword";
      if (KEYWORDS.has(word)) return "controlKeyword";
      // `name =` is an attribute; anything else is a plain reference.
      const rest = stream.string.slice(stream.pos);
      return /^\s*=[^=]/.test(rest) ? "propertyName" : "variableName";
    }

    if (stream.match(/^(==|!=|<=|>=|&&|\|\||[-+*/%<>=!?:.,()[\]{}])/)) return "operator";

    stream.next();
    return null;
  },

  languageData: {
    commentTokens: { line: "#", block: { open: "/*", close: "*/" } },
    closeBrackets: { brackets: ["(", "[", "{", '"'] },
  },
};
