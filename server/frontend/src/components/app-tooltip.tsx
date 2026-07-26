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

  .tip {
    background: var(--primary, #18181b);
    border-radius: 6px;
    bottom: calc(100% + 6px);
    color: var(--primary-foreground, #fafafa);
    font-size: 0.75rem;
    left: 50%;
    line-height: 1.3;
    opacity: 0;
    padding: 6px 12px;
    pointer-events: none;
    position: absolute;
    transform: translateX(-50%) translateY(2px);
    transition:
      opacity 120ms ease,
      transform 120ms ease;
    white-space: nowrap;
    z-index: 40;
  }

  :host(:hover) .tip,
  :host(:focus-within) .tip {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }

  .tip:empty {
    display: none;
  }

  .tip::after { background: var(--primary); border-radius: 2px; bottom: -4px; content: ""; height: 8px; left: 50%; position: absolute; transform: translateX(-50%) rotate(45deg); width: 8px; z-index: -1; }
`;

export function AppTooltip({ text = "" }: { text?: string }): Component {
  return (
    <>
      <style>{styles}</style>
      <span class="trigger">
        <slot></slot>
      </span>
      <span class="tip" role="tooltip">
        {text}
      </span>
    </>
  );
}
