import { Component, css } from "@frontiers-labs/argon";
import { AppSpoiler } from "./app-spoiler.js";
import { AutoMarkdown } from "./auto-markdown.js";

const styles = css`
  :host {
    width: 100%;
    display: block;
  }

  .bubble {
    display: block;
  }

  .user {
    border-radius: 24px;
    background: var(--secondary, #fefefe);
    max-width: 800px;
    width: fit-content;
    float: right;
    padding: 24px;
  }

  .tool-call {
    font-size: 0.95rem;
    font-weight: bold;
  }

  .plain {
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  @media print {
    .user {
      float: none;
      max-width: 100%;
      border-radius: 0;
      background: transparent;
      padding: 0 0 0 14px;
      border-left: 3px solid #999;
    }

    app-spoiler,
    .tool-call {
      display: none;
    }
  }
`;

export function AppMessage({
  messageId = "",
  seq = 0,
  role = "user",
  kind = "user",
  text = "",
  thinking = "",
  toolName = "",
}: {
  messageId?: string;
  seq?: number;
  role?: string;
  kind?: string;
  text?: string;
  thinking?: string;
  toolName?: string;
}): Component {
  return (
    <>
      <style>{styles}</style>
      {kind === "tool_output" ? (
        <AppSpoiler title={toolName !== "" ? toolName : "Tool output"} content={text} />
      ) : (
        <div class={kind === "user" ? "bubble user" : "bubble"}>
          {thinking !== "" && <AppSpoiler title="Thinking" content={thinking} />}
          {kind === "agent" ? <AutoMarkdown text={text} /> : <div class="plain">{text}</div>}
          {toolName !== "" && <p class="tool-call">Called tool {toolName}</p>}
        </div>
      )}
    </>
  );
}
