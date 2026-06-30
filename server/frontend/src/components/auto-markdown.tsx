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

export function AutoMarkdown({ text = "" }: { text?: string }): Component {
  const host = ref<HTMLDivElement>();
  effect(() => {
    if (host.current) {
      host.current.innerHTML = text;
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
