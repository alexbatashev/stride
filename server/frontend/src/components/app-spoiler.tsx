import { Component, css, state } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";

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
    border-radius: var(--radius-sm, 6px);
    gap: 6px;
    margin-left: -4px;
    padding: 3px 4px;
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
    margin: 6px 0 8px;
  }

  .title {
    font-size: 0.8125rem;
    font-weight: 500;
  }

  button:hover { background: var(--muted); }
  button:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow); outline: none; }
`;

export function AppSpoiler({ title = "Spoiler title", content = "" }: { title?: string; content?: string }): Component {
  let visible = state(false);
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
      {visible && <div class="content">{content}</div>}
    </>
  );
}
