import { marked } from "marked";

// Styling for the generated preview. Kept close to GitHub's rendering so the
// output looks like what most Markdown ends up being read in, and carrying its
// own dark-mode block so the page follows the OS setting once it is open in the
// browser, where JustCode's theme no longer applies.
const PREVIEW_CSS = `
  :root { color-scheme: light dark; }
  body {
    margin: 0 auto;
    padding: 40px 24px 80px;
    max-width: 820px;
    font-family: "Segoe UI", system-ui, sans-serif;
    font-size: 16px;
    line-height: 1.6;
    color: #1f2328;
    background: #ffffff;
  }
  h1, h2 { padding-bottom: .3em; border-bottom: 1px solid #d0d7de; }
  h1, h2, h3, h4 { margin-top: 1.6em; margin-bottom: .6em; line-height: 1.25; }
  a { color: #0969da; }
  code {
    padding: .2em .4em;
    font-size: 85%;
    border-radius: 6px;
    background: rgba(129,139,152,.16);
    font-family: "Cascadia Code", Consolas, monospace;
  }
  pre {
    padding: 16px;
    overflow: auto;
    border-radius: 6px;
    background: #f6f8fa;
  }
  pre code { padding: 0; background: none; }
  blockquote {
    margin: 0 0 16px;
    padding: 0 1em;
    color: #59636e;
    border-left: .25em solid #d0d7de;
  }
  table { border-collapse: collapse; display: block; overflow: auto; }
  th, td { padding: 6px 13px; border: 1px solid #d0d7de; }
  tr:nth-child(2n) { background: #f6f8fa; }
  img { max-width: 100%; }
  hr { height: .25em; border: 0; background: #d0d7de; }
  @media (prefers-color-scheme: dark) {
    body { color: #e6edf3; background: #0d1117; }
    h1, h2 { border-bottom-color: #30363d; }
    a { color: #4493f8; }
    pre { background: #161b22; }
    blockquote { color: #9198a1; border-left-color: #30363d; }
    th, td { border-color: #30363d; }
    tr:nth-child(2n) { background: #161b22; }
    hr { background: #30363d; }
  }
`;

function escapeHtml(text) {
  return text.replace(/[&<>"]/g, (character) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character],
  );
}

/**
 * Renders Markdown source into a complete, standalone HTML document ready to be
 * written to disk and opened in a browser.
 */
export function renderMarkdownDocument(source, title) {
  const body = marked.parse(source, { async: false, gfm: true, breaks: false });
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<style>${PREVIEW_CSS}</style>
</head>
<body>
${body}
</body>
</html>
`;
}
