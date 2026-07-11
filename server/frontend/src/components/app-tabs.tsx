/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";

interface Tab {
  value: string;
  label: string;
}

const styles = css`
  :host {
    display: block;
  }

  .root { display: flex; flex-direction: column; gap: 8px; }
  .root.vertical { align-items: flex-start; flex-direction: row; }

  .list {
    align-items: center;
    background: var(--muted, #f4f4f5);
    border-radius: var(--radius-lg, 10px);
    display: inline-flex;
    gap: 4px;
    padding: 3px;
    min-height: 36px;
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
    flex: 1;
  }

  .root.vertical .list { align-items: stretch; flex-direction: column; height: auto; }
  .root.vertical .trigger { text-align: left; }
  .root.line .list { background: transparent; border-radius: 0; gap: 4px; padding: 0; }
  .root.line .trigger { border-radius: 0; position: relative; }
  .root.line .trigger[aria-selected="true"] { background: transparent; box-shadow: none; }
  .root.line .trigger[aria-selected="true"]::after { background: var(--foreground); bottom: -5px; content: ""; height: 2px; inset-inline: 0; position: absolute; }
  .root.vertical.line .trigger[aria-selected="true"]::after { bottom: 0; height: auto; inset-block: 0; left: auto; right: -5px; width: 2px; }
`;

export function AppTabs({ tabs = [], value = "", variant = "default", orientation = "horizontal" }: { tabs?: Tab[]; value?: string; variant?: string; orientation?: string }): Component {
  const active = value || (tabs[0]?.value ?? "");
  return (
    <>
      <style>{styles}</style>
      <div class={`root ${variant} ${orientation}`}>
      <div
        class="list"
        role="tablist"
        aria-orientation={orientation}
        onClick={(event: Event) => {
          const trigger = (event.target as Element).closest(".trigger");
          if (!trigger) return;
          const next = trigger.getAttribute("data-value") ?? "";
          const current = value || (tabs[0]?.value ?? "");
          if (current === next) return;
          emit(this, "tab-change", { value: next });
        }}
        onKeyDown={(event: KeyboardEvent) => {
          const horizontal = orientation === "horizontal";
          const previous = event.key === (horizontal ? "ArrowLeft" : "ArrowUp");
          const nextKey = event.key === (horizontal ? "ArrowRight" : "ArrowDown");
          if (!previous && !nextKey && event.key !== "Home" && event.key !== "End") return;
          event.preventDefault();
          const index = Math.max(0, tabs.findIndex((tab) => tab.value === active));
          const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : previous ? (index - 1 + tabs.length) % tabs.length : (index + 1) % tabs.length;
          const next = tabs[nextIndex]?.value ?? active;
          emit(this, "tab-change", { value: next });
        }}
      >
        {tabs
          .map((tab) => (
            <button
              class="trigger"
              type="button"
              role="tab"
              data-value={tab.value}
              aria-selected={active === tab.value ? "true" : "false"}
            >
              {tab.label}
            </button>
          ))
          .join("")}
      </div>
      <div class="panels">
        {tabs
          .map((tab) => (
            <slot
              name={tab.value}
              hidden={active !== tab.value}
              style={active === tab.value ? "" : "display:none"}
            ></slot>
          ))
          .join("")}
      </div>
      </div>
    </>
  );
}
