/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, onMount } from "@frontiers-labs/argon";
import { IconX } from "./icons/x.js";

function requestClose(host: HTMLElement): void {
  host.dispatchEvent(new CustomEvent("close", { bubbles: true, composed: true }));
}

function isVisible(element: HTMLElement): boolean {
  let current: HTMLElement | null = element;
  while (current) {
    const style = getComputedStyle(current);
    if (current.hidden || current.getAttribute("aria-hidden") === "true" || style.display === "none" || style.visibility === "hidden") return false;
    const root = current.getRootNode();
    current = current.parentElement ?? (root instanceof ShadowRoot ? root.host : null);
  }
  return true;
}

function focusableElements(root: ParentNode): HTMLElement[] {
  const selector = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
  const focusable: HTMLElement[] = [];
  const visit = (parent: ParentNode) => {
    for (const element of Array.from(parent.children)) {
      if (element instanceof HTMLElement && element.matches(selector) && isVisible(element)) {
        focusable.push(element);
      }
      if (element.localName === "slot") {
        for (const assigned of (element as HTMLSlotElement).assignedElements({ flatten: true })) {
          visit(assigned);
          if (assigned.shadowRoot) visit(assigned.shadowRoot);
        }
      } else {
        visit(element);
      }
      if (element.shadowRoot) visit(element.shadowRoot);
    }
  };
  visit(root);
  return focusable;
}

function activeElement(): HTMLElement | null {
  let active = document.activeElement;
  while (active instanceof HTMLElement && active.shadowRoot?.activeElement) active = active.shadowRoot.activeElement;
  return active instanceof HTMLElement ? active : null;
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
    z-index: 200;
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
    gap: 16px;
    max-height: calc(100dvh - 32px);
    max-width: 512px;
    overflow: auto;
    padding: 24px;
    position: relative;
    width: 100%;
  }

  :host([size="wide"]) .dialog {
    max-width: min(980px, calc(100dvw - 32px));
  }

  :host([size="settings"]) .dialog {
    gap: 0;
    height: min(680px, calc(100dvh - 32px));
    max-height: calc(100dvh - 32px);
    max-width: min(1000px, calc(100dvw - 32px));
    overflow: hidden;
    padding: 0;
  }
  :host([size="settings"]) .header {
    align-items: center;
    border-bottom: 1px solid var(--border);
    min-height: 48px;
    padding: 0 52px 0 20px;
  }
  :host([size="settings"]) .title { font-size: 1.125rem; line-height: 1; }
  :host([size="settings"]) .close { top: 12px; }
  :host([size="settings"]) .content { flex: 1; min-height: 0; overflow: hidden; }

  :host([size="fullscreen"]) .overlay { padding: 0; }
  :host([size="fullscreen"]) .dialog {
    border: 0;
    border-radius: 0;
    gap: 0;
    height: 100dvh;
    max-height: none;
    max-width: none;
    padding: 0;
  }
  :host([size="fullscreen"]) .header { border-bottom: 1px solid var(--border); min-height: 48px; padding: 0 48px 0 16px; }
  :host([size="fullscreen"]) .content { flex: 1; min-height: 0; overflow: auto; }

  .header {
    display: grid;
    gap: 6px;
    padding-right: 28px;
  }

  .title {
    font-size: 1.05rem;
    font-weight: 600;
    line-height: 1.3;
  }

  .title:empty {
    display: none;
  }

  .description {
    color: var(--muted-foreground, #71717a);
    font-size: 0.875rem;
    line-height: 1.45;
  }

  .description:empty {
    display: none;
  }

  .close {
    align-items: center;
    background: var(--background, #ffffff);
    border: 0;
    border-radius: 6px;
    color: var(--muted-foreground, #71717a);
    cursor: pointer;
    display: inline-flex;
    height: 24px;
    justify-content: center;
    padding: 0;
    position: absolute;
    right: 16px;
    top: 16px;
    width: 24px;
    z-index: 2;
  }

  .close:hover {
    background: var(--muted, #f4f4f5);
    color: var(--foreground, #18181b);
  }

  .close:focus-visible {
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
    outline: none;
  }

  .close .icon {
    align-items: center;
    display: inline-flex;
    height: 16px;
    justify-content: center;
    width: 16px;
  }

  .close .icon > * {
    height: 16px;
    width: 16px;
  }

  .footer {
    align-items: center;
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .footer:not(:has(::slotted(*))) {
    display: none;
  }

  @media (max-width: 767px) {
    :host([size="settings"]) .overlay { padding: 0; }
    :host([size="settings"]) .dialog {
      border: 0;
      border-radius: 0;
      height: 100dvh;
      max-height: none;
      max-width: none;
    }
    :host([size="settings"]) .header { padding-left: 16px; }
  }
`;

export function AppDialog({
  open = false,
  title = "",
  description = "",
  size = "",
  dialogId = "",
}: {
  open?: boolean;
  title?: string;
  description?: string;
  size?: string;
  dialogId?: string;
}): Component {
  onMount(() => {
    let previouslyFocused: HTMLElement | null = null;
    let previousOverflow = "";
    let locked = false;
    const overlay = this.shadowRoot?.querySelector<HTMLElement>(".overlay");
    const syncOpenState = () => {
      const visible = overlay?.style.display !== "none";
      if (visible && !locked) {
        locked = true;
        previouslyFocused = activeElement();
        previousOverflow = document.body.style.overflow;
        document.body.style.overflow = "hidden";
        queueMicrotask(() => this.shadowRoot?.querySelector<HTMLElement>(".close")?.focus());
      } else if (!visible && locked) {
        locked = false;
        document.body.style.overflow = previousOverflow;
        previouslyFocused?.focus();
        previouslyFocused = null;
      }
    };
    const observer = new MutationObserver(syncOpenState);
    if (overlay) observer.observe(overlay, { attributes: true, attributeFilter: ["style"] });
    syncOpenState();
    const onKey = (event: KeyboardEvent) => {
      if (!open) return;
      if (event.key === "Escape") {
        event.preventDefault();
        requestClose(this);
        return;
      }
      if (event.key !== "Tab") return;
      const dialog = this.shadowRoot?.querySelector<HTMLElement>(".dialog");
      if (!dialog) return;
      const focusable = focusableElements(dialog);
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = activeElement();
      if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      observer.disconnect();
      document.removeEventListener("keydown", onKey);
      if (locked) document.body.style.overflow = previousOverflow;
    };
  });
  effect(() => {
    if (size) {
      this.setAttribute("size", size);
    } else {
      this.removeAttribute("size");
    }
    if (dialogId) {
      this.dataset.dialog = dialogId;
    } else {
      delete this.dataset.dialog;
    }
  });
  return (
    <>
      <style>{styles}</style>
      <div
        class="overlay"
        style={open ? "display:flex" : "display:none"}
        onClick={(event: Event) => {
          if (event.target === event.currentTarget) requestClose(this);
        }}
      >
        <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title" aria-describedby="dialog-description" part="dialog">
          <button class="close" type="button" aria-label="Close" onClick={() => requestClose(this)}>
            <span class="icon">
              <IconX />
            </span>
          </button>
          <div class="header">
            <div class="title" id="dialog-title">{title}</div>
            <div class="description" id="dialog-description">{description}</div>
          </div>
          <div class="content">
            <slot></slot>
          </div>
          <div class="footer">
            <slot name="footer"></slot>
          </div>
        </div>
      </div>
    </>
  );
}
