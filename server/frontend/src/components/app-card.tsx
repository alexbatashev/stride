/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  .card {
    background: var(--card, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 12px;
    box-shadow: 0 1px 2px rgb(0 0 0 / 5%);
    color: var(--card-foreground, #18181b);
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px 0;
  }

  .header,
  .content,
  .footer {
    padding: 0 20px;
  }

  .header {
    display: grid;
    gap: 4px;
  }

  .title {
    font-size: 1rem;
    font-weight: 600;
    line-height: 1.2;
  }

  .title:empty {
    display: none;
  }

  .description {
    color: var(--muted-foreground, #71717a);
    font-size: 0.875rem;
    line-height: 1.4;
  }

  .description:empty {
    display: none;
  }

  .header:not(:has(.title:not(:empty))):not(:has(.description:not(:empty))) {
    display: none;
  }

  .footer {
    align-items: center;
    display: flex;
    gap: 8px;
  }

  ::slotted([slot="footer"]) {
    margin-top: 4px;
  }
`;

export function AppCard({ title = "", description = "" }: { title?: string; description?: string }): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="card">
        <div class="header">
          <div class="title">{title}</div>
          <div class="description">{description}</div>
          <slot name="header"></slot>
        </div>
        <div class="content">
          <slot></slot>
        </div>
        <div class="footer">
          <slot name="footer"></slot>
        </div>
      </div>
    </>
  );
}
