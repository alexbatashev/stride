/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, onMount } from "@frontiers-labs/argon";
import { AppButton } from "./app-button.js";

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
    border-radius: var(--radius-lg, 10px);
    box-shadow: 0 10px 38px rgb(0 0 0 / 18%);
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 512px;
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

  app-button { min-width: 72px; }
`;

export function AppAlertDialog({
  open = false,
  title = "Are you sure?",
  description = "",
  cancelLabel = "Cancel",
  actionLabel = "Continue",
  variant = "default",
}: {
  open?: boolean;
  title?: string;
  description?: string;
  cancelLabel?: string;
  actionLabel?: string;
  variant?: string;
}): Component {
  onMount(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && open) respond(this, false);
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });
  return (
    <>
      <style>{styles}</style>
      <div class="overlay" style={open ? "display:flex" : "display:none"}>
        <div class="dialog" role="alertdialog" aria-modal="true">
          <div class="title">{title}</div>
          <div class="description">{description}</div>
          <div class="footer">
            <AppButton class="cancel" variant="outline" onClick={() => respond(this, false)}>
              {cancelLabel}
            </AppButton>
            <AppButton class="action" variant={variant === "destructive" ? "destructive" : "default"} onClick={() => respond(this, true)}>
              {actionLabel}
            </AppButton>
          </div>
        </div>
      </div>
    </>
  );
}
