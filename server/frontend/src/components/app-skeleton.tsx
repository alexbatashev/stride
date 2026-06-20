/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  .skeleton {
    animation: pulse 1.6s cubic-bezier(0.4, 0, 0.6, 1) infinite;
    background: var(--muted, #f4f4f5);
    border-radius: 8px;
    height: 100%;
    width: 100%;
  }

  @keyframes pulse {
    50% {
      opacity: 0.5;
    }
  }
`;

export function AppSkeleton(): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="skeleton" aria-hidden="true"></div>
    </>
  );
}
