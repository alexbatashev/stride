/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, state } from "@frontiers-labs/argon";

interface AccordionItem {
  value: string;
  title: string;
  content: string;
}

const chevron =
  '<svg class="chevron" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>';

const styles = css`
  :host {
    display: block;
  }

  .item {
    border-bottom: 1px solid var(--border, #e4e4e7);
  }

  .trigger {
    align-items: center;
    background: transparent;
    border: 0;
    color: var(--foreground, #18181b);
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 0.9rem;
    font-weight: 500;
    gap: 12px;
    justify-content: space-between;
    outline: none;
    padding: 14px 2px;
    text-align: left;
    width: 100%;
  }

  .trigger:hover {
    text-decoration: underline;
  }

  .trigger:focus-visible {
    border-radius: 6px;
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  .chevron {
    color: var(--muted-foreground, #71717a);
    flex: 0 0 auto;
    transition: transform 180ms ease;
  }

  .trigger[aria-expanded="true"] .chevron {
    transform: rotate(180deg);
  }

  .content {
    color: var(--muted-foreground, #71717a);
    font-size: 0.875rem;
    line-height: 1.5;
    padding: 0 2px 14px;
  }
`;

export function AppAccordion({
  items = [],
  type = "single",
}: {
  items?: AccordionItem[];
  type?: string;
}): Component {
  let open = state<string[]>([]);
  return (
    <>
      <style>{styles}</style>
      <div
        class="root"
        onClick={(event: Event) => {
          const trigger = (event.target as Element).closest(".trigger");
          if (!trigger) return;
          const value = trigger.getAttribute("data-value") ?? "";
          if (open.includes(value)) {
            open = open.filter((entry) => entry !== value);
          } else {
            open = type === "multiple" ? [...open, value] : [value];
          }
          this.dispatchEvent(
            new CustomEvent("value-change", { bubbles: true, composed: true, detail: { open: open } }),
          );
        }}
      >
        {items
          .map((item) => {
            const expanded = open.includes(item.value);
            return (
              `<div class="item" data-value="${item.value}">` +
              `<button class="trigger" type="button" data-value="${item.value}" aria-expanded="${expanded}">` +
              `<span>${item.title}</span>${chevron}</button>` +
              (expanded ? `<div class="content">${item.content}</div>` : "") +
              `</div>`
            );
          })
          .join("")}
      </div>
    </>
  );
}
