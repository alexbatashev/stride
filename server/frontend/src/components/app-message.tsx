import { Component, css, unsafeHtml } from "@frontiers-labs/argon";
import { AppMessageActions } from "./app-message-actions.js";
import { AutoMarkdown } from "./auto-markdown.js";

const styles = css`
  :host { display: block; min-width: 0; width: 100%; }
  .row { display: flex; min-width: 0; width: 100%; }
  .row.user { justify-content: flex-end; }
  .message { min-width: 0; overflow-wrap: anywhere; }
  .agent { padding: 2px 0; width: 100%; }
  .rendered p { margin: 0 0 0.75em; }
  .rendered p:last-child { margin-bottom: 0; }
  .rendered h1, .rendered h2, .rendered h3, .rendered h4, .rendered h5, .rendered h6 { font-weight: 600; line-height: 1.3; margin: 0.5em 0 0.25em; }
  .rendered h1:first-child, .rendered h2:first-child, .rendered h3:first-child, .rendered h4:first-child, .rendered h5:first-child, .rendered h6:first-child { margin-top: 0; }
  .rendered h1 { font-size: 1.6em; }
  .rendered h2 { font-size: 1.4em; }
  .rendered h3 { font-size: 1.2em; }
  .rendered ul, .rendered ol { margin: 0 0 0.75em; padding-left: 1.5em; }
  .rendered li { margin: 0.2em 0; }
  .rendered blockquote { border-left: 3px solid var(--border); color: var(--muted-foreground); margin: 0 0 0.75em; padding-left: 1em; }
  .rendered pre { background: var(--muted); border-radius: 6px; margin: 0.75em 0; max-width: 100%; overflow-x: auto; padding: 0.8em 1em; }
  .rendered code { background: var(--muted); border-radius: 4px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 0.92em; padding: 0.1em 0.25em; }
  .rendered pre code { background: transparent; border-radius: 0; padding: 0; }
  .rendered .table-wrap { margin: 0.75em 0; max-width: 100%; overflow-x: auto; }
  .rendered table { border-collapse: collapse; font-size: 0.95em; inline-size: max-content; min-inline-size: 100%; }
  .rendered th, .rendered td { border: 1px solid var(--border); max-inline-size: 24rem; min-inline-size: 10rem; overflow-wrap: break-word; padding: 0.4em 0.75em; text-align: left; vertical-align: top; white-space: normal; }
  .rendered th { background: var(--muted); font-weight: 600; }
  .rendered img, .rendered video, .rendered audio, .rendered iframe { display: block; height: auto; max-width: 100%; }
  .rendered iframe { border: 0; box-sizing: border-box; overflow: hidden; width: 100%; }
  .rendered a { color: var(--primary); text-decoration: none; }
  .rendered a:hover { text-decoration: underline; }
  .user .message { background: var(--message-user-bg); border: 1px solid color-mix(in oklab, var(--message-user-bg) 82%, var(--border)); border-radius: 16px; color: var(--message-user-fg); max-width: min(80%, 640px); padding: 12px; }
  .plain { white-space: pre-wrap; }
  .actions { margin-top: 6px; }
  .user-actions { max-width: min(80%, 640px); width: 100%; }
  @media (max-width: 767px) { .user .message, .user-actions { max-width: 88%; } }
  @media print {
    .user .message { background: transparent; border: 0; border-left: 3px solid #999; border-radius: 0; color: inherit; max-width: 100%; padding: 0 0 0 14px; }
    app-message-actions { display: none; }
  }
`;

export function AppMessage({
  messageId = "",
  seq = 0,
  role = "user",
  kind = "user",
  format = "markdown",
  text = "",
  pending = false,
}: {
  messageId?: string;
  seq?: number;
  role?: string;
  kind?: string;
  format?: string;
  text?: string;
  pending?: boolean;
}): Component {
  const user = kind === "user";
  return <><style>{styles}</style><div class={`row${user ? " user" : ""}`}><div class={`message ${user ? "user-message" : "agent"}`}>{kind === "agent" ? <AutoMarkdown key="body" text={text} format={format}><div class="rendered">{format === "html" ? unsafeHtml(text) : text}</div></AutoMarkdown> : <div class="plain">{text}</div>}</div></div>{!pending && text !== "" && <div class={`actions${user ? " user-actions" : ""}`} style={user ? "margin-left: auto" : ""}><AppMessageActions text={text} align={user ? "end" : "start"} /></div>}</>;
}
