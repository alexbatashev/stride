/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";

interface AccordionItem {
  value: string;
  title: string;
  content: string;
}

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
  value = [],
  type = "single",
}: {
  items?: AccordionItem[];
  value?: string[];
  type?: string;
}): Component {
  return (
    <>
      <style>{styles}</style>
      <div
        class="root"
        onClick={(event: Event) => {
          const trigger = (event.target as Element).closest(".trigger");
          if (!trigger) return;
          const item = trigger.getAttribute("data-value") ?? "";
          const next = value.includes(item)
            ? value.filter((entry) => entry !== item)
            : type === "multiple"
              ? [...value, item]
              : [item];
          emit(this, "value-change", { value: next });
        }}
      >
        {items
          .map((item) => {
            const expanded = value.includes(item.value);
            return (
              <div class="item" data-value={item.value}>
                <button class="trigger" type="button" data-value={item.value} aria-expanded={expanded ? "true" : "false"}>
                  <span>{item.title}</span>
                  <span class="chevron" aria-hidden="true"><IconChevronDown /></span>
                </button>
                {expanded ? <div class="content">{item.content}</div> : ""}
              </div>
            );
          })
          .join("")}
      </div>
    </>
  );
}
