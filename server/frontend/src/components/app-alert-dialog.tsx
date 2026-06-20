/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

function respond(host: HTMLElement, confirmed: boolean): void {
  host.dispatchEvent(
    new CustomEvent("response", { bubbles: true, composed: true, detail: { confirmed: confirmed } }),
  );
}

const styles = css`
  :host {
    display: contents;
  }

  .overlay {
    align-items: center;
    background: rgb(0 0 0 / 50%);
    inset: 0;
    justify-content: center;
    padding: 16px;
    position: fixed;
    z-index: 50;
  }

  .dialog {
    background: var(--background, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 14px;
    box-shadow: 0 10px 38px rgb(0 0 0 / 18%);
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 440px;
    padding: 24px;
    width: 100%;
  }

  .title {
    font-size: 1.05rem;
    font-weight: 600;
    line-height: 1.3;
  }

  .description {
    color: var(--muted-foreground, #71717a);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .footer {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 12px;
  }

  button {
    border-radius: 8px;
    cursor: pointer;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    height: 34px;
    padding: 0 14px;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      opacity 140ms ease;
  }

  .cancel {
    background: var(--background, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    color: var(--foreground, #18181b);
  }

  .cancel:hover {
    background: var(--muted, #f4f4f5);
  }

  .action {
    background: var(--primary, #18181b);
    border: 1px solid transparent;
    color: var(--primary-foreground, #fafafa);
  }

  .action:hover {
    opacity: 0.9;
  }

  :host([variant="destructive"]) .action {
    background: var(--destructive, #dc2626);
  }
`;

export function AppAlertDialog({
  open = false,
  title = "Are you sure?",
  description = "",
  cancelLabel = "Cancel",
  actionLabel = "Continue",
}: {
  open?: boolean;
  title?: string;
  description?: string;
  cancelLabel?: string;
  actionLabel?: string;
}): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="overlay" style={open ? "display:flex" : "display:none"}>
        <div class="dialog" role="alertdialog" aria-modal="true">
          <div class="title">{title}</div>
          <div class="description">{description}</div>
          <div class="footer">
            <button class="cancel" type="button" onClick={() => respond(this, false)}>
              {cancelLabel}
            </button>
            <button class="action" type="button" onClick={() => respond(this, true)}>
              {actionLabel}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}
