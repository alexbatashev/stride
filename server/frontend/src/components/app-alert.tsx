/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  .alert {
    background: var(--background, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 10px;
    color: var(--foreground, #09090b);
    display: grid;
    gap: 4px 12px;
    grid-template-columns: 1fr;
    padding: 14px 16px;
  }

  :host([variant="destructive"]) .alert {
    border-color: var(--destructive-ring, rgb(220 38 38 / 40%));
    color: var(--destructive, #dc2626);
  }

  .alert:has(.icon ::slotted(*)) {
    grid-template-columns: auto 1fr;
  }

  .icon {
    align-items: center;
    display: inline-flex;
    grid-row: span 2;
    height: 18px;
    width: 18px;
  }

  .icon:not(:has(::slotted(*))) {
    display: none;
  }

  .title {
    font-size: 0.9rem;
    font-weight: 600;
    line-height: 1.3;
  }

  .title:empty {
    display: none;
  }

  .description {
    color: var(--muted-foreground, #71717a);
    font-size: 0.875rem;
    line-height: 1.4;
  }

  :host([variant="destructive"]) .description {
    color: inherit;
    opacity: 0.9;
  }
`;

export function AppAlert({ title = "" }: { title?: string }): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="alert" role="alert">
        <span class="icon" aria-hidden="true">
          <slot name="icon"></slot>
        </span>
        <div class="title">{title}</div>
        <div class="description">
          <slot></slot>
        </div>
      </div>
    </>
  );
}
