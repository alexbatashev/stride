/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, state } from "@frontiers-labs/argon";

interface Tab {
  value: string;
  label: string;
}

const styles = css`
  :host {
    display: block;
  }

  .list {
    align-items: center;
    background: var(--muted, #f4f4f5);
    border-radius: 10px;
    display: inline-flex;
    gap: 4px;
    padding: 3px;
  }

  .trigger {
    background: transparent;
    border: 0;
    border-radius: 7px;
    color: var(--muted-foreground, #71717a);
    cursor: pointer;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    outline: none;
    padding: 5px 12px;
    transition:
      background-color 140ms ease,
      color 140ms ease,
      box-shadow 140ms ease;
  }

  .trigger:focus-visible {
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  .trigger[aria-selected="true"] {
    background: var(--background, #ffffff);
    box-shadow: 0 1px 2px rgb(0 0 0 / 8%);
    color: var(--foreground, #18181b);
  }

  .panels {
    margin-top: 12px;
  }
`;

export function AppTabs({ tabs = [], value = "" }: { tabs?: Tab[]; value?: string }): Component {
  let active = state(value || (tabs[0]?.value ?? ""));
  return (
    <>
      <style>{styles}</style>
      <div
        class="list"
        role="tablist"
        onClick={(event: Event) => {
          const trigger = (event.target as Element).closest(".trigger");
          if (!trigger) return;
          const next = trigger.getAttribute("data-value") ?? "";
          if (active === next) return;
          active = next;
          this.setAttribute("value", next);
          this.dispatchEvent(
            new CustomEvent("tab-change", { bubbles: true, composed: true, detail: { value: next } }),
          );
        }}
      >
        {tabs
          .map(
            (tab) =>
              `<button class="trigger" type="button" role="tab" data-value="${tab.value}" aria-selected="${
                active === tab.value
              }">${tab.label}</button>`,
          )
          .join("")}
      </div>
      <div class="panels">
        {tabs
          .map((tab) =>
            active === tab.value
              ? `<slot name="${tab.value}"></slot>`
              : `<slot name="${tab.value}" hidden style="display:none"></slot>`,
          )
          .join("")}
      </div>
    </>
  );
}
