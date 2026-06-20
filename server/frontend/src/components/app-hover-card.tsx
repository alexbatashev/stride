/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

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
    left: 0;
    margin-top: 6px;
    opacity: 0;
    padding: 14px;
    pointer-events: none;
    position: absolute;
    top: 100%;
    transition:
      opacity 140ms ease,
      transform 140ms ease;
    transform: translateY(2px);
    width: 260px;
    z-index: 40;
  }

  :host(:hover) .content,
  :host(:focus-within) .content {
    opacity: 1;
    pointer-events: auto;
    transform: translateY(0);
  }
`;

export function AppHoverCard(): Component {
  return (
    <>
      <style>{styles}</style>
      <span class="trigger">
        <slot name="trigger"></slot>
      </span>
      <div class="content">
        <slot></slot>
      </div>
    </>
  );
}
