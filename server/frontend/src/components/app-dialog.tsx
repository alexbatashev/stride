/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, onMount } from "@frontiers-labs/argon";
import { IconX } from "./icons/x.js";

function closeOverlay(host: HTMLElement): void {
  if (!host.hasAttribute("open")) return;
  host.removeAttribute("open");
  host.dispatchEvent(new CustomEvent("close", { bubbles: true, composed: true }));
}

const styles = css`
  :host {
    display: contents;
  }

  .overlay {
    align-items: center;
    background: rgb(0 0 0 / 50%);
    display: none;
    inset: 0;
    justify-content: center;
    padding: 16px;
    position: fixed;
    z-index: 50;
  }

  :host([open]) .overlay {
    display: flex;
  }

  .dialog {
    background: var(--background, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 14px;
    box-shadow: 0 10px 38px rgb(0 0 0 / 18%);
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-height: calc(100dvh - 32px);
    max-width: 480px;
    overflow: auto;
    padding: 24px;
    width: 100%;
  }

  .header {
    display: grid;
    gap: 6px;
    padding-right: 28px;
  }

  .title {
    font-size: 1.05rem;
    font-weight: 600;
    line-height: 1.3;
  }

  .title:empty {
    display: none;
  }

  .description {
    color: var(--muted-foreground, #71717a);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .description:empty {
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

  .close .icon {
    height: 16px;
    width: 16px;
  }

  .footer {
    align-items: center;
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .footer:not(:has(::slotted(*))) {
    display: none;
  }
`;

export function AppDialog({
  open = false,
  title = "",
  description = "",
}: {
  open?: boolean;
  title?: string;
  description?: string;
}): Component {
  onMount(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeOverlay(this);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });
  return (
    <>
      <style>{styles}</style>
      <div
        class="overlay"
        onClick={(event: Event) => {
          if (event.target === event.currentTarget) closeOverlay(this);
        }}
      >
        <div class="dialog" role="dialog" aria-modal="true">
          <button class="close" type="button" aria-label="Close" onClick={() => closeOverlay(this)}>
            <span class="icon">
              <IconX />
            </span>
          </button>
          <div class="header">
            <div class="title">{title}</div>
            <div class="description">{description}</div>
          </div>
          <div class="content">
            <slot></slot>
          </div>
          <div class="footer">
            <slot name="footer"></slot>
          </div>
        </div>
      </div>
    </>
  );
}
