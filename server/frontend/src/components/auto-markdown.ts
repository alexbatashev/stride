import { LitElement, css, html } from "lit";
import { customElement, property } from "lit/decorators.js";

function parseInline(text: string): Node[] {
  const nodes: Node[] = [];
  const re = /\[([^\]]+)\]\(((?:https?:\/\/|mailto:)[^)\s]+)\)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) {
      nodes.push(document.createTextNode(text.slice(last, m.index)));
    }
    const a = document.createElement("a");
    a.href = m[2];
    a.textContent = m[1];
    a.rel = "noopener noreferrer";
    a.target = "_blank";
    nodes.push(a);
    last = m.index + m[0].length;
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

function buildTable(tableLines: string[]): HTMLTableElement {
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
  return table;
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

    // Paragraph: accumulate until blank line, heading, or table start
    const paraLines = [line];
    i++;
    while (i < lines.length) {
      const next = lines[i];
      if (next.trim() === "") break;
      if (/^#{1,6} /.test(next)) break;
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

    table {
      border-collapse: collapse;
      width: 100%;
      margin: 0.75em 0;
      font-size: 0.95em;
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
