import { Component, css } from "@frontiers-labs/argon";
import { AppMessageActions } from "./app-message-actions.js";
import { AutoMarkdown } from "./auto-markdown.js";

const styles = css`
  :host { display: block; min-width: 0; width: 100%; }
  .row { display: flex; min-width: 0; width: 100%; }
  .row.user { justify-content: flex-end; }
  .message { min-width: 0; overflow-wrap: anywhere; }
  .agent { padding: 2px 4px; width: 100%; }
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
  return <><style>{styles}</style><div class={`row${user ? " user" : ""}`}><div class={`message ${user ? "user-message" : "agent"}`}>{kind === "agent" ? <AutoMarkdown key="body" text={text} format={format} /> : <div class="plain">{text}</div>}</div></div>{!pending && text !== "" && <div class={`actions${user ? " user-actions" : ""}`} style={user ? "margin-left: auto" : ""}><AppMessageActions text={text} align={user ? "end" : "start"} /></div>}</>;
}
