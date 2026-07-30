// Enough Markdown to read an answer by (`T-7`).
//
// A model's reply arrives as Markdown and was shown as the characters it is
// made of, so `### Key fields` and `**time**` sat there literally. What is
// wanted is the rendering; what is not wanted is a way for a reply to become
// part of the page.
//
// So this never produces HTML. It turns text into a small tree of plain data,
// and the panel builds nodes from that with `textContent`. No string of markup
// is assembled from a reply at any point, which means a reply containing
// `<img onerror=...>` is a reply containing those characters — it is displayed,
// not run. (The panel does use `innerHTML` in two places, both for its own
// icons from a fixed set; nothing on this path does.) `marked` is in this
// project and would have been two lines, but it emits markup and does not
// sanitise, and the harness's own rule is that a model's output is data and
// never instruction.
//
// The subset is what answers actually use: fenced code, headings, lists,
// quotes, rules, and inline code, bold, italic and links. Anything unrecognised
// stays as the text it is, which is the right failure: an unrendered heading is
// legible, and a swallowed one is not.
//
// Links are rendered as their text followed by the address, and not as anything
// clickable. A reply is untrusted content, and a one-click path from untrusted
// content to a browser is the shape of every phishing link ever sent.

const FENCE = /^\s*```(\S*)\s*$/;
const HEADING = /^(#{1,6})\s+(.*)$/;
const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBERED = /^\s*(\d+)[.)]\s+(.*)$/;
const QUOTE = /^\s*>\s?(.*)$/;
const RULE = /^\s*(?:---+|\*\*\*+|___+)\s*$/;

/** Split a reply into blocks. Pure data — no markup, no DOM. */
export function blocks(source) {
  const lines = String(source ?? "").split(/\r?\n/);
  const out = [];
  let at = 0;

  while (at < lines.length) {
    const line = lines[at];

    // Fenced code first: nothing inside it is markup, including the things
    // that look like it. A reply explaining Markdown would otherwise be
    // rendered as the thing it was explaining.
    const fence = line.match(FENCE);
    if (fence) {
      const language = fence[1] || "";
      const body = [];
      at += 1;
      while (at < lines.length && !FENCE.test(lines[at])) {
        body.push(lines[at]);
        at += 1;
      }
      at += 1; // the closing fence, or the end of the text
      out.push({ kind: "code", language, text: body.join("\n") });
      continue;
    }

    if (!line.trim()) {
      at += 1;
      continue;
    }

    if (RULE.test(line)) {
      out.push({ kind: "rule" });
      at += 1;
      continue;
    }

    const heading = line.match(HEADING);
    if (heading) {
      out.push({ kind: "heading", level: heading[1].length, spans: spans(heading[2]) });
      at += 1;
      continue;
    }

    if (BULLET.test(line) || NUMBERED.test(line)) {
      const ordered = !BULLET.test(line);
      const items = [];
      while (at < lines.length) {
        const bullet = lines[at].match(BULLET);
        const numbered = lines[at].match(NUMBERED);
        if (ordered && numbered) items.push(spans(numbered[2]));
        else if (!ordered && bullet) items.push(spans(bullet[1]));
        else break;
        at += 1;
      }
      out.push({ kind: "list", ordered, items });
      continue;
    }

    if (QUOTE.test(line)) {
      const said = [];
      while (at < lines.length && QUOTE.test(lines[at])) {
        said.push(lines[at].match(QUOTE)[1]);
        at += 1;
      }
      out.push({ kind: "quote", spans: spans(said.join(" ")) });
      continue;
    }

    // A paragraph runs to the next blank line or the next block that starts
    // one. Its lines are joined with spaces, the way Markdown reads them.
    const said = [];
    while (at < lines.length && lines[at].trim() && !starts(lines[at])) {
      said.push(lines[at].trim());
      at += 1;
    }
    out.push({ kind: "para", spans: spans(said.join(" ")) });
  }
  return out;
}

/** Whether a line begins a block, so a paragraph knows to stop. */
function starts(line) {
  return (
    FENCE.test(line) ||
    HEADING.test(line) ||
    BULLET.test(line) ||
    NUMBERED.test(line) ||
    QUOTE.test(line) ||
    RULE.test(line)
  );
}

// Inline code first, and its content is never looked at again: `**` inside
// backticks is two asterisks, which is the whole point of writing it there.
const INLINE = [
  { kind: "code", pattern: /`([^`]+)`/ },
  // The address may contain a balanced pair of its own — `Foo_(bar)` is an
  // ordinary Wikipedia address, and stopping at the first `)` cut it in half
  // and left the other half sitting in the sentence.
  { kind: "link", pattern: /\[([^\]]*)\]\(((?:[^()\s]|\([^()\s]*\))+)\)/ },
  { kind: "strong", pattern: /\*\*([^*]+)\*\*/ },
  { kind: "strong", pattern: /__([^_]+)__/ },
  { kind: "em", pattern: /\*([^*]+)\*/ },
  { kind: "em", pattern: /_([^_]+)_/ },
];

/** Split one line into runs of text and marked-up spans. */
export function spans(source) {
  const text = String(source ?? "");
  if (!text) return [];

  // The earliest match across every pattern wins, so `a **b** c` splits at the
  // bold rather than wherever the first pattern in the list happens to hit.
  let best = null;
  for (const { kind, pattern } of INLINE) {
    const found = text.match(pattern);
    if (!found) continue;
    if (!best || found.index < best.found.index) best = { kind, found };
  }
  if (!best) return [{ kind: "text", text }];

  const { kind, found } = best;
  const before = text.slice(0, found.index);
  const after = text.slice(found.index + found[0].length);
  const span =
    kind === "link"
      ? { kind: "link", text: found[1] || found[2], href: found[2] }
      : { kind, text: found[1] };

  return [
    ...(before ? spans(before) : []),
    span,
    // Code is opaque: what is inside it was already taken literally, and the
    // rest of the line still gets read.
    ...(after ? spans(after) : []),
  ];
}
