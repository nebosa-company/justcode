import { linter, forEachDiagnostic } from "@codemirror/lint";
import { StateEffect } from "@codemirror/state";

/**
 * Signal to re-run the spell check. A linter normally only re-runs when the
 * document changes, so switching spell check on — or finishing the dictionary
 * download — would otherwise show nothing until the next keystroke.
 */
export const refreshSpelling = StateEffect.define();

/** Asks a view to spell-check now. */
export function requestSpellcheck(view) {
  view.dispatch({ effects: refreshSpelling.of(null) });
}

// A dictionary-backed spell checker.
//
// The webview's own checker was doing this before, and had two problems that
// could not be worked around: it never reports what it found, so misspellings
// could not be counted or listed, and it checks the rendered DOM — where syntax
// highlighting splits words across many spans and long files are virtualised —
// so it silently missed words. Checking the document text ourselves fixes both:
// every misspelling becomes an ordinary diagnostic, and so it lands in the
// problems count alongside the syntax linters.

// Words shorter than this are not worth flagging (io, id, fn …).
const MIN_WORD_LENGTH = 3;
const MAX_SPELLING_DIAGNOSTICS = 500;

// Letters plus the apostrophes inside contractions.
const WORD_RE = /[A-Za-z][A-Za-z'’]*/g;

// An English dictionary flags most programming vocabulary, which would bury the
// real misspellings. These are treated as words so code stays quiet and prose
// still gets checked.
const CODE_WORDS = new Set(
  `const let var func fn def elif endif fi esac fmt println printf sprintf stdin
   stdout stderr args argv argc params param init ctor dtor async await goto
   struct enum impl trait mut usize isize bool int uint str chr ptr ref deref
   nullptr namespace typedef typename inline virtual const_cast static_cast
   dict elem elems attr attrs div span href src alt nav ul li td tr th tbody
   thead colspan rowspan img svg css html xml json yaml toml sql db api uri url
   http https localhost www utf ascii regex regexp substr strlen strcmp malloc
   calloc realloc idx len msg err errno req res ctx env cfg config configs util
   utils lib libs repo repos dev prod auth admin usr bin tmp temp dir dirs
   filename filepath pathname basename dirname stringify parse parser lexer
   tokenizer async iterator iterable enumerate zip lambda kwargs cls self this
   nullable serializable deserialize serialization middleware webhook oauth jwt
   uuid guid crud orm sdk cli gui ui ux api's namespace enum boolean charset
   pragma endregion nocase noqa eslint prettier tsconfig webpack vite rollup
   npm node deno bun cargo rustc clippy pyc pyi venv virtualenv conda`
    .split(/\s+/)
    .filter(Boolean),
);

let checker = null;
let loading = null;

// The linter stays installed and reads this flag, rather than being added and
// removed from the extension set. Removing a linter does not retract the
// diagnostics it already published, so switching spell check off used to leave
// its warnings sitting in the gutter; returning an empty result does clear them.
let enabled = false;

export function setSpellcheckEnabled(value) {
  enabled = value;
}

/**
 * Loads the dictionary once, on first use. The word list ships as a static
 * asset rather than inside the bundle — it is half a megabyte, and nothing
 * needs it unless spell checking is switched on.
 */
export function loadDictionary() {
  if (checker) return Promise.resolve(checker);
  if (loading) return loading;
  loading = Promise.all([
    import("nspell"),
    fetch(new URL("dict/en.aff", document.baseURI)).then((r) => r.text()),
    fetch(new URL("dict/en.dic", document.baseURI)).then((r) => r.text()),
  ])
    .then(([nspell, aff, dic]) => {
      const make = nspell.default || nspell;
      checker = make(aff, dic);
      return checker;
    })
    .catch(() => null);
  return loading;
}

/**
 * Splits an identifier the way programmers write them, so `getUserName` and
 * `user_name` are checked as their parts rather than flagged whole.
 */
function* subWords(word, offset) {
  // camelCase / PascalCase / consecutive capitals (HTTPServer -> HTTP, Server).
  // WORD_RE never matches `_` or `-`, so a match is already one bare run of
  // letters and only the case boundaries are left to split on.
  // U+2019 as well as ASCII ': WORD_RE accepts both, and without it here
  // every contraction pasted from Word or a browser was chopped at the
  // apostrophe and its first half reported as a misspelling.
  const parts = word.match(/[A-Z]+(?![a-z])|[A-Z][a-z'\u2019]*|[a-z][a-z'\u2019]*/g) || [];
  let cursor = 0;
  for (const part of parts) {
    const index = word.indexOf(part, cursor);
    yield { text: part, from: offset + index };
    cursor = index + part.length;
  }
}

function isMisspelled(word) {
  if (word.length < MIN_WORD_LENGTH) return false;
  if (CODE_WORDS.has(word.toLowerCase())) return false;
  if (checker.correct(word)) return false;
  // Accept any casing of a known word: NAME, Name, name.
  const lower = word.toLowerCase();
  if (checker.correct(lower)) return false;
  return !checker.correct(lower.charAt(0).toUpperCase() + lower.slice(1));
}

// `suggest()` is by far the most expensive call in the dictionary — around
// 90ms per unseen word — and the same handful of misspellings tend to repeat,
// so results are memoised. The cache is bounded: a very long document with
// thousands of distinct misspellings should not pin them all in memory.
const MAX_CACHED_SUGGESTIONS = 500;
const suggestionCache = new Map();

function suggestionsFor(word) {
  const cached = suggestionCache.get(word);
  if (cached) {
    // Refresh recency so entries in active use survive eviction.
    suggestionCache.delete(word);
    suggestionCache.set(word, cached);
    return cached;
  }
  const suggestions = checker.suggest(word).slice(0, 5);
  suggestionCache.set(word, suggestions);
  if (suggestionCache.size > MAX_CACHED_SUGGESTIONS) {
    suggestionCache.delete(suggestionCache.keys().next().value);
  }
  return suggestions;
}

/**
 * The message shown for one misspelling, in the hover tooltip and in the
 * problems panel.
 *
 * Suggestions are computed on first hover of *this* element, and nowhere else.
 * Neither of the obvious places works: CodeMirror calls `renderMessage` for
 * every diagnostic when the problems panel opens, and enumerates
 * `Diagnostic.actions` for every panel item as well — so putting `suggest()`
 * behind either of them ran it hundreds of times in a row and froze the window
 * for tens of seconds. Hovering is the one signal that means "this one".
 *
 * Applying a fix resolves the diagnostic's *current* range through
 * `forEachDiagnostic` rather than trusting the offsets captured at lint time:
 * lint maps its decoration ranges through document changes but never rewrites
 * the `Diagnostic` objects, so those offsets go stale after an edit and could
 * rewrite the wrong occurrence of a repeated word.
 */
function spellingMessage(view, diagnostic, word) {
  const wrap = document.createElement("span");
  wrap.className = "spell-message";

  const text = document.createElement("span");
  text.textContent = `"${word}" is not in the dictionary`;
  wrap.append(text);

  let filled = false;
  const fill = () => {
    if (filled || !checker) return;
    filled = true;
    const suggestions = suggestionsFor(word);
    if (!suggestions.length) return;

    text.textContent = `"${word}" — did you mean:`;
    const row = document.createElement("span");
    row.className = "spell-suggestions";
    for (const suggestion of suggestions) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "spell-fix";
      button.textContent = suggestion;
      button.addEventListener("mousedown", (event) => {
        // mousedown, not click: the lint tooltip closes on blur.
        event.preventDefault();
        event.stopPropagation();
        const range = currentRangeOf(view, diagnostic);
        if (range) view.dispatch({ changes: { ...range, insert: suggestion } });
      });
      row.append(button);
    }
    wrap.append(row);
  };

  // `mouseenter` does not bubble from a node that is added later, and the panel
  // row is what the pointer actually meets, so listen on both.
  wrap.addEventListener("mouseenter", fill);
  wrap.addEventListener("mousemove", fill);
  return wrap;
}

/** Where a diagnostic sits *now*, found by identity through lint's own state. */
function currentRangeOf(view, diagnostic) {
  let found = null;
  forEachDiagnostic(view.state, (candidate, from, to) => {
    if (candidate === diagnostic) found = { from, to };
  });
  return found;
}

/**
 * The spell-check linter. Off until enabled, and a no-op until the dictionary
 * has loaded — the load kicks off here so nothing blocks on it.
 */
export function spellcheckLinter() {
  return linter(
    (view) => {
      if (!enabled) return [];
      if (!checker) {
        // Kick off the load and ask for another pass once it lands.
        loadDictionary().then((ready) => {
          if (ready) requestSpellcheck(view);
        });
        return [];
      }

      const diagnostics = [];
      const text = view.state.doc.toString();
      for (const match of text.matchAll(WORD_RE)) {
        for (const { text: part, from } of subWords(match[0], match.index)) {
          if (!isMisspelled(part)) continue;
          const to = from + part.length;
          // Built first so `renderMessage` can close over the object itself,
          // which is how its current range is found again when a fix is applied.
          const diagnostic = {
            from,
            to,
            severity: "warning",
            source: "spelling",
            message: `"${part}" is not in the dictionary`,
          };
          diagnostic.renderMessage = (target) => spellingMessage(target, diagnostic, part);
          diagnostics.push(diagnostic);
          if (diagnostics.length >= MAX_SPELLING_DIAGNOSTICS) return diagnostics;
        }
      }
      return diagnostics;
    },
    {
      // Spell checking a whole document is heavier than a parse, so it waits
      // longer after typing stops than the syntax linters do.
      delay: 1200,
      // Re-run on an explicit request as well as on edits.
      needsRefresh: (update) =>
        update.transactions.some((tr) => tr.effects.some((effect) => effect.is(refreshSpelling))),
    },
  );
}
