import { Component, css, effect, ref } from "@frontiers-labs/argon";

// Images before links so ![...] is not swallowed by link pattern; bold before italic so ** is matched before *
const INLINE_RE =
  /!\[([^\]]*)\]\(([^\s)]+)\)|\*\*((?:(?!\*\*).)+?)\*\*|\*((?:(?!\*).)+?)\*|\[([^\]]+)\]\(([^\s)]+)\)/g;

function parseInline(text: string): Node[] {
  const nodes: Node[] = [];
  let last = 0;
  // matchAll clones the regex internally, so recursive calls don't clobber lastIndex
  for (const m of text.matchAll(INLINE_RE)) {
    const idx = m.index!;
    if (idx > last) {
      nodes.push(document.createTextNode(text.slice(last, idx)));
    }
    if (m[1] !== undefined) {
      const img = document.createElement("img");
      img.alt = m[1];
      img.src = m[2];
      nodes.push(img);
    } else if (m[3] !== undefined) {
      const strong = document.createElement("strong");
      strong.append(...parseInline(m[3]));
      nodes.push(strong);
    } else if (m[4] !== undefined) {
      const em = document.createElement("em");
      em.append(...parseInline(m[4]));
      nodes.push(em);
    } else {
      const a = document.createElement("a");
      a.href = m[6];
      a.textContent = m[5];
      a.rel = "noopener noreferrer";
      a.target = "_blank";
      nodes.push(a);
    }
    last = idx + m[0].length;
  }
  if (last < text.length) {
    nodes.push(document.createTextNode(text.slice(last)));
  }
  return nodes;
}

function isTableSeparator(line: string): boolean {
  return line.includes("|") && line.includes("-") && /^[\s|:\-]+$/.test(line);
}

function parseTableRow(line: string): string[] {
  const inner = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  return inner.split("|").map((c) => c.trim());
}

function buildTable(tableLines: string[]): HTMLDivElement {
  const wrapper = document.createElement("div");
  wrapper.className = "table-wrap";
  const table = document.createElement("table");
  const thead = document.createElement("thead");
  const tbody = document.createElement("tbody");

  const headerRow = document.createElement("tr");
  for (const cell of parseTableRow(tableLines[0])) {
    const th = document.createElement("th");
    th.append(...parseInline(cell));
    headerRow.append(th);
  }
  thead.append(headerRow);

  for (let i = 2; i < tableLines.length; i++) {
    const tr = document.createElement("tr");
    for (const cell of parseTableRow(tableLines[i])) {
      const td = document.createElement("td");
      td.append(...parseInline(cell));
      tr.append(td);
    }
    tbody.append(tr);
  }

  table.append(thead, tbody);
  wrapper.append(table);
  return wrapper;
}

const HEADING_TAGS = ["h1", "h2", "h3", "h4", "h5", "h6"];

// Info strings whose fenced block is an interactive artifact rendered in a
// sandboxed frame rather than shown as source.
const ARTIFACT_LANGS = new Set(["html"]);

function buildArtifact(source: string): HTMLElement {
  const el = document.createElement("stride-artifact") as HTMLElement & { source: string };
  el.source = source;
  return el;
}

// Shown while an artifact fence is still streaming and has no closing fence yet;
// partial HTML must never reach the sandbox.
function buildArtifactPlaceholder(): HTMLElement {
  const div = document.createElement("div");
  div.className = "artifact-pending";
  div.textContent = "Building interactive view…";
  return div;
}

function buildCodeBlock(lang: string, code: string): HTMLElement {
  const pre = document.createElement("pre");
  pre.className = "code-block";
  const codeEl = document.createElement("code");
  if (lang) {
    codeEl.dataset.lang = lang;
  }
  codeEl.textContent = code;
  pre.append(codeEl);
  return pre;
}

function renderMarkdown(text: string): Node[] {
  const nodes: Node[] = [];
  const lines = text.split("\n");
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    if (line.trim() === "") {
      i++;
      continue;
    }

    // Fenced code block: artifact langs render in a sandboxed frame, everything
    // else as verbatim source. An unterminated fence is still streaming.
    const fence = line.match(/^(\s*)(`{3,})(.*)$/);
    if (fence) {
      const marker = fence[2];
      const lang = fence[3].trim().toLowerCase();
      const bodyLines: string[] = [];
      i++;
      let closed = false;
      while (i < lines.length) {
        const candidate = lines[i].trim();
        if (candidate.startsWith(marker) && candidate.slice(marker.length).trim() === "") {
          closed = true;
          i++;
          break;
        }
        bodyLines.push(lines[i]);
        i++;
      }
      const body = bodyLines.join("\n");
      if (ARTIFACT_LANGS.has(lang)) {
        nodes.push(closed ? buildArtifact(body) : buildArtifactPlaceholder());
      } else {
        nodes.push(buildCodeBlock(lang, body));
      }
      continue;
    }

    // Heading
    const hm = line.match(/^(#{1,6}) (.+)$/);
    if (hm) {
      const el = document.createElement(HEADING_TAGS[hm[1].length - 1]);
      el.append(...parseInline(hm[2]));
      nodes.push(el);
      i++;
      continue;
    }

    // Table: first line has | and second line is a separator
    if (line.includes("|") && i + 1 < lines.length && isTableSeparator(lines[i + 1])) {
      const tableLines = [line];
      i++;
      while (i < lines.length && lines[i].trim() !== "") {
        tableLines.push(lines[i]);
        i++;
      }
      nodes.push(buildTable(tableLines));
      continue;
    }

    // Unordered list
    if (/^\s*[-*+] /.test(line)) {
      const ul = document.createElement("ul");
      while (i < lines.length && /^\s*[-*+] /.test(lines[i])) {
        const m = lines[i].match(/^\s*[-*+] (.+)$/);
        if (!m) break;
        const li = document.createElement("li");
        li.append(...parseInline(m[1]));
        ul.append(li);
        i++;
      }
      nodes.push(ul);
      continue;
    }

    // Ordered list
    if (/^\s*\d+\. /.test(line)) {
      const ol = document.createElement("ol");
      while (i < lines.length && /^\s*\d+\. /.test(lines[i])) {
        const m = lines[i].match(/^\s*\d+\. (.+)$/);
        if (!m) break;
        const li = document.createElement("li");
        li.append(...parseInline(m[1]));
        ol.append(li);
        i++;
      }
      nodes.push(ol);
      continue;
    }

    // Paragraph: accumulate until blank line, heading, list, or table start
    const paraLines = [line];
    i++;
    while (i < lines.length) {
      const next = lines[i];
      if (next.trim() === "") break;
      if (/^#{1,6} /.test(next)) break;
      if (/^\s*[-*+] /.test(next)) break;
      if (/^\s*\d+\. /.test(next)) break;
      if (next.includes("|") && i + 1 < lines.length && isTableSeparator(lines[i + 1])) break;
      paraLines.push(next);
      i++;
    }
    const p = document.createElement("p");
    p.append(...parseInline(paraLines.join(" ")));
    nodes.push(p);
  }

  return nodes;
}

const styles = css`
  :host {
    display: block;
  }

  p {
    margin: 0 0 0.75em;
  }

  p:last-child {
    margin-bottom: 0;
  }

  h1,
  h2,
  h3,
  h4,
  h5,
  h6 {
    margin: 0.5em 0 0.25em;
    font-weight: 600;
    line-height: 1.3;
  }

  h1:first-child,
  h2:first-child,
  h3:first-child,
  h4:first-child,
  h5:first-child,
  h6:first-child {
    margin-top: 0;
  }

  h1 {
    font-size: 1.6em;
  }
  h2 {
    font-size: 1.4em;
  }
  h3 {
    font-size: 1.2em;
  }

  .table-wrap {
    overflow-x: auto;
    max-width: 100%;
    margin: 0.75em 0;
  }

  .table-wrap:last-child {
    margin-bottom: 0;
  }

  table {
    border-collapse: collapse;
    font-size: 0.95em;
    width: max-content;
    max-width: 100%;
  }

  th,
  td {
    border: 1px solid var(--border, #d0d0d0);
    max-width: 32rem;
    overflow-wrap: anywhere;
    padding: 0.4em 0.75em;
    text-align: left;
    vertical-align: top;
    white-space: normal;
  }

  th {
    background: var(--muted, rgba(0, 0, 0, 0.05));
    font-weight: 600;
  }

  ul,
  ol {
    margin: 0 0 0.75em;
    padding-left: 1.5em;
  }

  ul:last-child,
  ol:last-child {
    margin-bottom: 0;
  }

  li {
    margin: 0.2em 0;
  }

  a {
    color: var(--primary, #0066cc);
    text-decoration: none;
  }

  a:hover {
    text-decoration: underline;
  }

  .code-block {
    background: var(--muted, rgba(0, 0, 0, 0.05));
    border-radius: 8px;
    margin: 0.75em 0;
    overflow-x: auto;
    padding: 0.75em 1em;
  }

  .code-block:last-child {
    margin-bottom: 0;
  }

  .code-block code {
    font-family: ui-monospace, monospace;
    font-size: 0.9em;
    white-space: pre;
  }

  .artifact-pending {
    border: 1px dashed var(--border, #d0d0d0);
    border-radius: 12px;
    color: var(--muted-foreground, #666);
    font-size: 0.95em;
    margin: 0.75em 0;
    padding: 12px;
  }
`;

// Argon text bindings insert markup verbatim, so the `text` prop carries
// HTML-escaped content; undo that before parsing markdown into DOM nodes.
function unescapeHtml(text: string): string {
  return text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, "&");
}

export function AutoMarkdown({ text = "" }: { text?: string }): Component {
  const host = ref<HTMLDivElement>();
  effect(() => {
    host.current?.replaceChildren(...renderMarkdown(unescapeHtml(text)));
  });
  // The escaped text paints with SSR; the effect swaps it for parsed markdown.
  return (
    <>
      <style>{styles}</style>
      <div ref={host}>{text}</div>
    </>
  );
}
