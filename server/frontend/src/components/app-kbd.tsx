/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { display: inline-flex; }
  kbd { align-items: center; background: var(--muted); border-radius: var(--radius-sm, 4px); color: var(--muted-foreground); display: inline-flex; font-family: inherit; font-size: 0.75rem; font-weight: 500; gap: 4px; height: 20px; justify-content: center; min-width: 20px; padding: 0 4px; pointer-events: none; user-select: none; }
`;

export function AppKbd(): Component { return <><style>{styles}</style><kbd><slot></slot></kbd></>; }
