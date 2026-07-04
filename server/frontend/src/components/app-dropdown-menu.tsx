/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

type MenuItem = { label: string; action: string; variant?: string };

const styles = css`
  :host {
    position: fixed;
    z-index: 60;
  }

  .menu {
    background: var(--popover, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 12%);
    box-sizing: border-box;
    color: var(--popover-foreground, #18181b);
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 176px;
    padding: 4px;
  }

  .item {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--popover-foreground, #18181b);
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 0.875rem;
    gap: 8px;
    height: 32px;
    outline: none;
    padding: 0 8px;
    text-align: left;
    transition:
      background-color 120ms ease,
      color 120ms ease;
    width: 100%;
  }

  .item:hover,
  .item:focus-visible {
    background: var(--accent, #f4f4f5);
    color: var(--accent-foreground, #18181b);
  }

  .item.destructive {
    color: var(--destructive, #dc2626);
  }

  .item.destructive:hover,
  .item.destructive:focus-visible {
    background: var(--destructive-muted, rgb(220 38 38 / 10%));
    color: var(--destructive, #dc2626);
  }
`;

export function AppDropdownMenu({
  open = false,
  items = [],
}: {
  open?: boolean;
  items?: MenuItem[];
}): Component {
  return (
    <>
      <style>{styles}</style>
      <div
        class="menu"
        role="menu"
        style={open ? "" : "display:none"}
        onClick={(event: Event) => {
          const item = (event.target as HTMLElement).closest<HTMLElement>("[data-action]");
          if (!item?.dataset.action) return;
          this.dispatchEvent(
            new CustomEvent("select", {
              bubbles: true,
              composed: true,
              detail: { action: item.dataset.action },
            }),
          );
        }}
      >
        {items
          .map((item) => (
            <button
              type="button"
              role="menuitem"
              class={`item ${item.variant ?? ""}`}
              data-action={item.action}
            >
              {item.label}
            </button>
          ))
          .join("")}
      </div>
    </>
  );
}
