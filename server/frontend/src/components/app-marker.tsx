/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { display: block; width: 100%; }
  .marker { align-items: center; color: var(--muted-foreground); display: flex; font-size: 0.875rem; gap: 8px; min-height: 16px; text-align: left; width: 100%; }
  :host([variant="separator"]) .marker::before, :host([variant="separator"]) .marker::after { background: var(--border); content: ""; flex: 1; height: 1px; min-width: 0; }
  :host([variant="separator"]) .marker::before { margin-right: 4px; }
  :host([variant="separator"]) .marker::after { margin-left: 4px; }
  :host([variant="border"]) .marker { border-bottom: 1px solid var(--border); padding-bottom: 8px; }
  .icon { flex: 0 0 16px; height: 16px; width: 16px; }
  .content { min-width: 0; overflow-wrap: anywhere; }
  ::slotted(a) { color: inherit; text-decoration: underline; text-underline-offset: 3px; }
`;

export function AppMarker(): Component { return <><style>{styles}</style><div class="marker"><span class="icon"><slot name="icon"></slot></span><span class="content"><slot></slot></span></div></>; }
