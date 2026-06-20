/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";
import { IconX } from "./icons/x.js";

function requestClose(host: HTMLElement): void {
  host.dispatchEvent(new CustomEvent("close", { bubbles: true, composed: true }));
}

const styles = css`
  :host {
    display: contents;
  }

  .overlay {
    background: rgb(0 0 0 / 50%);
    inset: 0;
    position: fixed;
    z-index: 50;
  }

  .panel {
    background: var(--background, #ffffff);
    box-shadow: 0 10px 38px rgb(0 0 0 / 18%);
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    display: flex;
    flex-direction: column;
    gap: 16px;
    overflow: auto;
    padding: 24px;
    position: fixed;
  }

  :host(:not([side])) .panel,
  :host([side="right"]) .panel {
    border-left: 1px solid var(--border, #e4e4e7);
    bottom: 0;
    right: 0;
    top: 0;
    width: min(380px, 100%);
  }

  :host([side="left"]) .panel {
    border-right: 1px solid var(--border, #e4e4e7);
    bottom: 0;
    left: 0;
    top: 0;
    width: min(380px, 100%);
  }

  :host([side="top"]) .panel {
    border-bottom: 1px solid var(--border, #e4e4e7);
    left: 0;
    right: 0;
    top: 0;
  }

  :host([side="bottom"]) .panel {
    border-top: 1px solid var(--border, #e4e4e7);
    bottom: 0;
    left: 0;
    right: 0;
  }

  .title {
    font-size: 1.05rem;
    font-weight: 600;
    padding-right: 28px;
  }

  .title:empty {
    display: none;
  }

  .close {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--muted-foreground, #71717a);
    cursor: pointer;
    display: inline-flex;
    height: 24px;
    justify-content: center;
    padding: 0;
    position: absolute;
    right: 16px;
    top: 16px;
    width: 24px;
  }

  .close:hover {
    background: var(--muted, #f4f4f5);
    color: var(--foreground, #18181b);
  }

  .icon {
    height: 16px;
    width: 16px;
  }
`;

export function AppSheet({ open = false, title = "" }: { open?: boolean; title?: string }): Component {
  return (
    <>
      <style>{styles}</style>
      <div
        class="overlay"
        style={open ? "display:block" : "display:none"}
        onClick={(event: Event) => {
          if (event.target === event.currentTarget) requestClose(this);
        }}
      >
        <div class="panel" role="dialog" aria-modal="true">
          <button class="close" type="button" aria-label="Close" onClick={() => requestClose(this)}>
            <span class="icon">
              <IconX />
            </span>
          </button>
          <div class="title">{title}</div>
          <div class="content">
            <slot></slot>
          </div>
        </div>
      </div>
    </>
  );
}
