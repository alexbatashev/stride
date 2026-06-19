/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
  }

  label {
    align-items: center;
    color: var(--foreground, #09090b);
    cursor: default;
    display: inline-flex;
    font-size: 0.875rem;
    font-weight: 500;
    gap: 6px;
    line-height: 1;
    user-select: none;
  }

  :host([disabled]) label {
    cursor: not-allowed;
    opacity: 0.5;
  }
`;

export function AppLabel({ for: htmlFor = "" }: { for?: string }): Component {
  return (
    <>
      <style>{styles}</style>
      <label for={htmlFor}>
        <slot></slot>
      </label>
    </>
  );
}
