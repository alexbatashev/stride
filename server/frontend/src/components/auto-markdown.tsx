import { Component, css, effect, ref } from "@frontiers-labs/argon";

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
    -webkit-overflow-scrolling: touch;
    max-width: 100%;
    margin: 0.75em 0;
  }

  .table-wrap:last-child {
    margin-bottom: 0;
  }

  table {
    border-collapse: collapse;
    font-size: 0.95em;
    inline-size: max-content;
    max-inline-size: none;
    min-inline-size: 100%;
  }

  th,
  td {
    border: 1px solid var(--border, #d0d0d0);
    max-inline-size: 24rem;
    min-inline-size: 10rem;
    overflow-wrap: break-word;
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

  blockquote {
    border-left: 3px solid var(--border, #d0d0d0);
    color: var(--muted-foreground, #555);
    margin: 0 0 0.75em;
    padding-left: 1em;
  }

  pre {
    background: var(--muted, rgba(0, 0, 0, 0.05));
    border-radius: 6px;
    margin: 0.75em 0;
    max-width: 100%;
    overflow-x: auto;
    padding: 0.8em 1em;
  }

  code {
    background: var(--muted, rgba(0, 0, 0, 0.05));
    border-radius: 4px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.92em;
    padding: 0.1em 0.25em;
  }

  pre code {
    background: transparent;
    border-radius: 0;
    padding: 0;
  }

  hr {
    border: 0;
    border-top: 1px solid var(--border, #d0d0d0);
    margin: 1em 0;
  }

  img,
  video,
  audio,
  iframe {
    display: block;
    height: auto;
    max-width: 100%;
  }

  img,
  video,
  iframe {
    border-radius: 6px;
    margin: 0.75em 0;
  }

  iframe {
    border: 1px solid var(--border, #d0d0d0);
    overflow: hidden;
    min-height: 320px;
    width: 100%;
  }

  @media (max-width: 640px) {
    th,
    td {
      max-inline-size: 18rem;
      min-inline-size: 9rem;
    }
  }

  a {
    color: var(--primary, #0066cc);
    text-decoration: none;
  }

  a:hover {
    text-decoration: underline;
  }
`;

export function AutoMarkdown({
  text = "",
  format = "markdown",
}: {
  text?: string;
  format?: string;
}): Component {
  const host = ref<HTMLDivElement>();
  effect(() => {
    if (host.current) {
      const isHtml = format === "html";
      if (isHtml) {
        host.current.replaceChildren(sanitizeHtmlFragment(text));
        decodeCodeBlockText(host.current);
      } else {
        host.current.innerHTML = renderMarkdown(text);
      }
      wrapTables(host.current);
      return connectWidgetFrames(host.current);
    }
  });
  return (
    <>
      <style>{styles}</style>
      <div ref={host}>{text}</div>
    </>
  );
}

function renderMarkdown(source: string): string {
  const lines = unescapeHtml(source).replace(/\r\n?/g, "\n").split("\n");
  const html: string[] = [];
  let paragraph: string[] = [];
  let list: "ul" | "ol" | null = null;
  let code: string[] | null = null;

  const flushParagraph = () => {
    if (paragraph.length === 0) return;
    html.push(`<p>${renderInline(paragraph.join(" "))}</p>`);
    paragraph = [];
  };

  const closeList = () => {
    if (!list) return;
    html.push(`</${list}>`);
    list = null;
  };

  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (code) {
      if (line.startsWith("```")) {
        html.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
        code = null;
      } else {
        code.push(line);
      }
      continue;
    }

    if (line.startsWith("```")) {
      flushParagraph();
      closeList();
      code = [];
      continue;
    }

    const trimmed = line.trim();
    const next = lines[i + 1]?.trim();
    if (trimmed === "") {
      flushParagraph();
      closeList();
      continue;
    }

    const heading = /^(#{1,6})\s+(.+)$/.exec(trimmed);
    if (heading) {
      flushParagraph();
      closeList();
      const level = heading[1].length;
      html.push(`<h${level}>${renderInline(heading[2])}</h${level}>`);
      continue;
    }

    if (next && isTableHeader(trimmed, next)) {
      flushParagraph();
      closeList();
      const rows = [trimmed];
      i += 2;
      while (i < lines.length && isTableRow(lines[i].trim())) {
        rows.push(lines[i].trim());
        i += 1;
      }
      i -= 1;
      html.push(renderTable(rows));
      continue;
    }

    const unordered = /^[-*]\s+(.+)$/.exec(trimmed);
    if (unordered) {
      flushParagraph();
      if (list !== "ul") {
        closeList();
        list = "ul";
        html.push("<ul>");
      }
      html.push(`<li>${renderInline(unordered[1])}</li>`);
      continue;
    }

    const ordered = /^\d+[.)]\s+(.+)$/.exec(trimmed);
    if (ordered) {
      flushParagraph();
      if (list !== "ol") {
        closeList();
        list = "ol";
        html.push("<ol>");
      }
      html.push(`<li>${renderInline(ordered[1])}</li>`);
      continue;
    }

    closeList();
    paragraph.push(trimmed);
  }

  if (code) {
    html.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
  }
  flushParagraph();
  closeList();

  return html.join("");
}

function isTableHeader(row: string, separator: string): boolean {
  return isTableRow(row) && isTableSeparator(separator) && parseTableRow(row).length >= 2;
}

function isTableRow(row: string): boolean {
  return row.includes("|") && parseTableRow(row).length >= 2;
}

function isTableSeparator(row: string): boolean {
  if (!row.includes("|")) return false;
  const cells = parseTableRow(row);
  return cells.length >= 2 && cells.every((cell) => /^:?-{3,}:?$/.test(cell.trim()));
}

function renderTable(rows: string[]): string {
  const header = parseTableRow(rows[0]);
  const body = rows.slice(1).map(parseTableRow);
  const head = header.map((cell) => `<th>${renderInline(cell)}</th>`).join("");
  const bodyRows = body
    .map((row) => {
      const cells = header.map((_cell, index) => `<td>${renderInline(row[index] ?? "")}</td>`).join("");
      return `<tr>${cells}</tr>`;
    })
    .join("");
  return `<table><thead><tr>${head}</tr></thead><tbody>${bodyRows}</tbody></table>`;
}

function parseTableRow(row: string): string[] {
  const trimmed = row.trim().replace(/^\|/, "").replace(/\|$/, "");
  const cells: string[] = [];
  let cell = "";
  for (let i = 0; i < trimmed.length; i += 1) {
    const char = trimmed[i];
    if (char === "\\" && trimmed[i + 1] === "|") {
      cell += "|";
      i += 1;
      continue;
    }
    if (char === "|") {
      cells.push(cell.trim());
      cell = "";
      continue;
    }
    cell += char;
  }
  cells.push(cell.trim());
  return cells;
}

function renderInline(source: string): string {
  const codeSpans: string[] = [];
  let text = escapeHtml(source);
  text = text.replace(/`([^`]+)`/g, (_match, code) => {
    const index = codeSpans.push(`<code>${code}</code>`) - 1;
    return `\x00CODE${index}\x00`;
  });
  text = text.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_match, label, href) => {
    const safeHref = sanitizeHref(unescapeHtml(href));
    if (!safeHref) return label;
    return `<a href="${escapeAttr(safeHref)}" rel="noopener noreferrer" target="_blank">${label}</a>`;
  });
  text = text.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  text = text.replace(/__([^_]+)__/g, "<strong>$1</strong>");
  text = text.replace(/\*([^*]+)\*/g, "<em>$1</em>");
  text = text.replace(/_([^_]+)_/g, "<em>$1</em>");
  return text.replace(/\x00CODE(\d+)\x00/g, (_match, index) => codeSpans[Number(index)] ?? "");
}

function sanitizeHref(href: string): string | null {
  const trimmed = href.trim();
  if (trimmed === "" || /[\u0000-\u001f\u007f]/.test(trimmed)) return null;
  const lower = trimmed.toLowerCase();
  if (lower.startsWith("/") || lower.startsWith("#") || !lower.includes(":")) {
    return href;
  }
  try {
    const url = new URL(href, window.location.origin);
    if (["http:", "https:", "mailto:"].includes(url.protocol)) {
      return href;
    }
  } catch {
    return null;
  }
  return null;
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function escapeAttr(value: string): string {
  return escapeHtml(value).replace(/"/g, "&quot;");
}

function unescapeHtml(value: string): string {
  const textarea = document.createElement("textarea");
  textarea.innerHTML = value;
  return textarea.value;
}

const allowedHtmlTags = new Set([
  "a",
  "audio",
  "b",
  "blockquote",
  "br",
  "code",
  "del",
  "em",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "hr",
  "i",
  "iframe",
  "img",
  "li",
  "ol",
  "p",
  "pre",
  "s",
  "strong",
  "table",
  "tbody",
  "td",
  "tfoot",
  "th",
  "thead",
  "tr",
  "u",
  "ul",
  "video",
]);

const dangerousHtmlTags = new Set(["script", "style", "object", "embed", "svg", "math"]);

function sanitizeHtmlFragment(source: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = source;
  sanitizeHtmlChildren(template.content);
  return template.content;
}

function sanitizeHtmlChildren(parent: ParentNode): void {
  for (const child of [...parent.childNodes]) {
    if (!(child instanceof HTMLElement)) {
      continue;
    }

    const tag = child.tagName.toLowerCase();
    if (dangerousHtmlTags.has(tag)) {
      child.remove();
      continue;
    }

    sanitizeHtmlChildren(child);

    if (!allowedHtmlTags.has(tag)) {
      child.replaceWith(...child.childNodes);
      continue;
    }

    sanitizeHtmlAttributes(child, tag);
  }
}

function sanitizeHtmlAttributes(element: HTMLElement, tag: string): void {
  const href = element.getAttribute("href");
  const src = element.getAttribute("src");
  const alt = element.getAttribute("alt");
  for (const attr of [...element.attributes]) {
    element.removeAttribute(attr.name);
  }

  if (tag === "a" && href) {
    const safeHref = sanitizeHref(href);
    if (safeHref) {
      element.setAttribute("href", safeHref);
      element.setAttribute("rel", "noopener noreferrer");
      element.setAttribute("target", "_blank");
    }
  }

  if (["audio", "iframe", "img", "video"].includes(tag) && (!src || !sanitizeMediaSrc(src))) {
    element.remove();
    return;
  }

  if (tag === "img" && src) {
    element.setAttribute("src", src);
    if (alt) {
      element.setAttribute("alt", alt);
    }
  }

  if ((tag === "audio" || tag === "video") && src) {
    element.setAttribute("src", src);
    element.setAttribute("controls", "");
  }

  if (tag === "iframe" && src) {
    element.setAttribute("src", src);
    element.setAttribute("sandbox", "allow-scripts");
    element.setAttribute("loading", "lazy");
  }
}

function sanitizeMediaSrc(src: string): boolean {
  const trimmed = src.trim();
  if (trimmed === "" || /[\u0000-\u001f\u007f]/.test(trimmed)) return false;
  const lower = trimmed.toLowerCase();
  if (lower.startsWith("/") || !lower.includes(":")) {
    return true;
  }
  try {
    const url = new URL(src, window.location.origin);
    return ["http:", "https:"].includes(url.protocol) && url.origin === window.location.origin;
  } catch {
    return false;
  }
}

function decodeCodeBlockText(root: HTMLElement): void {
  for (const code of root.querySelectorAll("pre code")) {
    code.textContent = unescapeHtml(code.textContent ?? "");
  }

  for (const pre of root.querySelectorAll("pre")) {
    if (pre.querySelector("code")) {
      continue;
    }
    pre.textContent = unescapeHtml(pre.textContent ?? "");
  }
}

type WidgetHeightMessage = {
  type: "stride-widget-height";
  height: number;
  href?: string;
};

function connectWidgetFrames(root: HTMLElement): () => void {
  const frames = [...root.querySelectorAll("iframe")];
  for (const frame of frames) {
    frame.setAttribute("scrolling", "no");
  }

  const onMessage = (event: MessageEvent<unknown>) => {
    if (!isWidgetHeightMessage(event.data)) {
      return;
    }
    const frame = frames.find((item) => item.contentWindow === event.source);
    if (!frame) {
      return;
    }
    const height = Math.max(320, Math.min(4000, Math.ceil(event.data.height)));
    frame.style.height = `${height}px`;
  };

  window.addEventListener("message", onMessage);
  return () => window.removeEventListener("message", onMessage);
}

function isWidgetHeightMessage(value: unknown): value is WidgetHeightMessage {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as WidgetHeightMessage).type === "stride-widget-height" &&
    Number.isFinite((value as WidgetHeightMessage).height)
  );
}

function wrapTables(root: HTMLElement): void {
  for (const table of root.querySelectorAll("table")) {
    if (table.parentElement?.classList.contains("table-wrap")) {
      continue;
    }
    const wrapper = document.createElement("div");
    wrapper.className = "table-wrap";
    table.before(wrapper);
    wrapper.append(table);
  }
}
