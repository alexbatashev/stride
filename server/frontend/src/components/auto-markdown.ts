import { LitElement, css, html } from "lit";
import { customElement, property } from "lit/decorators.js";

// Bold before italic so ** is matched before *
const INLINE_RE =
  /\*\*((?:(?!\*\*).)+?)\*\*|\*((?:(?!\*).)+?)\*|\[([^\]]+)\]\(((?:https?:\/\/|mailto:)[^)\s]+)\)/g;

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
      const strong = document.createElement("strong");
      strong.append(...parseInline(m[1]));
      nodes.push(strong);
    } else if (m[2] !== undefined) {
      const em = document.createElement("em");
      em.append(...parseInline(m[2]));
      nodes.push(em);
    } else {
      const a = document.createElement("a");
      a.href = m[4];
      a.textContent = m[3];
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

const HEADING_TAGS = ["h1", "h2", "h3", "h4", "h5", "h6"] as const;

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

@customElement("auto-markdown")
export class AutoMarkdown extends LitElement {
  @property()
  text: string = "";

  static styles = css`
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
      white-space: nowrap;
    }

    th,
    td {
      border: 1px solid var(--border, #d0d0d0);
      padding: 0.4em 0.75em;
      text-align: left;
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
  `;

  render() {
    return html`<div id="md"></div>`;
  }

  updated(changed: Map<string, unknown>) {
    if (changed.has("text")) {
      this.shadowRoot!.getElementById("md")!.replaceChildren(
        ...renderMarkdown(this.text),
      );
    }
  }
}
