/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
  }

  .badge {
    align-items: center;
    background: var(--primary, #18181b);
    border: 1px solid transparent;
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--primary-foreground, #fafafa);
    display: inline-flex;
    font-size: 0.75rem;
    font-weight: 500;
    gap: 4px;
    line-height: 1;
    padding: 3px 8px;
    white-space: nowrap;
    width: fit-content;
  }

  :host([variant="secondary"]) .badge {
    background: var(--secondary, #f4f4f5);
    color: var(--secondary-foreground, #18181b);
  }

  :host([variant="outline"]) .badge {
    background: transparent;
    border-color: var(--border, #e4e4e7);
    color: var(--foreground, #18181b);
  }

  :host([variant="destructive"]) .badge {
    background: var(--destructive, #dc2626);
    color: var(--primary-foreground, #fafafa);
  }
`;

export function AppBadge(): Component {
  return (
    <>
      <style>{styles}</style>
      <span class="badge">
        <slot></slot>
      </span>
    </>
  );
}
