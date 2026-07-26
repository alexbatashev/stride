/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { align-items: center; display: inline-flex; height: 16px; justify-content: center; width: 16px; }
  span { animation: spin 800ms linear infinite; border: 2px solid currentcolor; border-radius: 999px; border-right-color: transparent; box-sizing: border-box; height: 16px; width: 16px; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { span { animation-duration: 1.6s; } }
`;

export function AppSpinner(): Component { return <><style>{styles}</style><span role="status" aria-label="Loading"></span></>; }
