import { Component, css, state } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";
import { AutoMarkdown } from "./auto-markdown.js";

const styles = css`
  :host {
    display: block;
  }

  button {
    align-items: center;
    background: transparent;
    border: 0;
    color: inherit;
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    gap: 4px;
    padding: 0;
  }

  .chevron {
    align-items: center;
    display: inline-flex;
    flex: 0 0 1em;
    height: 1em;
    justify-content: center;
    width: 1em;
  }

  .chevron > * {
    height: 1em;
    width: 1em;
  }

  .content {
    margin-top: 8px;
    margin-bottom: 16px;
  }

  .title {
    font-weight: bold;
    font-size: 0.95rem;
  }
`;

export function AppSpoiler({
  title = "Spoiler title",
  content = "",
  format = "",
}: {
  title?: string;
  content?: string;
  format?: string;
}): Component {
  let visible = state(false);
  const body = format === "markdown" ? <AutoMarkdown text={content} format="markdown" /> : content;
  return (
    <>
      <style>{styles}</style>
      <button
        type="button"
        aria-expanded={visible ? "true" : "false"}
        onClick={() => {
          visible = !visible;
        }}
      >
        <span class="title">{title}</span>
        <span class="chevron" aria-hidden="true">
          {visible ? <IconChevronDown /> : <IconChevronRight />}
        </span>
      </button>
      {visible && <div class="content">{body}</div>}
    </>
  );
}
