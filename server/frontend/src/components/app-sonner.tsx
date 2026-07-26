/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";

interface Toast { id: string; title: string; description?: string; variant?: string; action?: string }

const styles = css`
  :host { bottom: 16px; display: flex; flex-direction: column; gap: 8px; max-width: calc(100vw - 32px); pointer-events: none; position: fixed; right: 16px; width: 356px; z-index: 100; }
  .stack { display: contents; }
  .toast { align-items: flex-start; background: var(--popover); border: 1px solid var(--border); border-radius: var(--radius-lg, 10px); box-shadow: 0 10px 28px rgb(0 0 0 / 14%); color: var(--popover-foreground); display: grid; gap: 2px 12px; grid-template-columns: 1fr auto; padding: 14px 16px; pointer-events: auto; }
  .title { font-size: 0.875rem; font-weight: 600; line-height: 1.3; }
  .description { color: var(--muted-foreground); font-size: 0.8125rem; grid-column: 1; line-height: 1.4; }
  .description:empty, .action:empty { display: none; }
  .action { align-items: center; background: var(--primary); border: 0; border-radius: var(--radius-sm, 6px); color: var(--primary-foreground); cursor: pointer; display: inline-flex; font: inherit; font-size: 0.75rem; font-weight: 500; grid-column: 2; grid-row: 1 / span 2; height: 28px; padding: 0 10px; }
  .toast[data-variant="error"] { border-color: color-mix(in oklab, var(--destructive) 30%, var(--border)); }
  @media (max-width: 480px) { :host { inset: auto 16px 16px; width: auto; } }
`;

export function AppSonner({ toasts = [] }: { toasts?: Toast[] }): Component {
  const toastItems = toasts.map((toast) => <div key={toast.id} class="toast" data-variant={toast.variant ?? "default"} role="status"><div class="title">{toast.title}</div><div class="description">{toast.description ?? ""}</div><button class="action" data-toast-id={toast.id} type="button">{toast.action ?? ""}</button></div>);
  return <><style>{styles}</style><div class="stack" onClick={(event: Event) => { const action = (event.target as Element).closest<HTMLElement>("[data-toast-id]"); if (action) emit(this, "toast-action", { id: action.dataset.toastId ?? "" }); }}>{toastItems}</div></>;
}
