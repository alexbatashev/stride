/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, onMount, state } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
    position: relative;
  }

  .trigger {
    display: inline-flex;
  }

  .content {
    background: var(--popover, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 10px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 12%);
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    display: none;
    margin-top: 6px;
    min-width: 220px;
    padding: 14px;
    position: absolute;
    top: 100%;
    z-index: 40;
  }

  :host([align="end"]) .content {
    right: 0;
  }

  :host(:not([align="end"])) .content {
    left: 0;
  }

  :host([open]) .content {
    display: block;
  }
`;

export function AppPopover({ open = false }: { open?: boolean }): Component {
  let isOpen = state(open);
  onMount(() => {
    const onOutside = (event: Event) => {
      if (isOpen && !event.composedPath().includes(this)) {
        isOpen = false;
        this.removeAttribute("open");
      }
    };
    document.addEventListener("click", onOutside);
    return () => document.removeEventListener("click", onOutside);
  });
  return (
    <>
      <style>{styles}</style>
      <span
        class="trigger"
        onClick={() => {
          isOpen = !isOpen;
          this.toggleAttribute("open", isOpen);
        }}
      >
        <slot name="trigger"></slot>
      </span>
      <div class="content" role="dialog">
        <slot></slot>
      </div>
    </>
  );
}
