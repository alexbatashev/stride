/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, onMount, state } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";

interface SelectOption {
  value: string;
  label: string;
}

const checkIcon =
  '<svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>';

const styles = css`
  :host {
    display: block;
    position: relative;
  }

  .trigger {
    align-items: center;
    background: var(--background, #ffffff);
    border: 1px solid var(--input, #e4e4e7);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 0.875rem;
    gap: 8px;
    height: 32px;
    justify-content: space-between;
    outline: none;
    padding: 0 10px;
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease;
    width: 100%;
  }

  .trigger:focus-visible {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  .placeholder {
    color: var(--muted-foreground, #71717a);
  }

  .chevron {
    color: var(--muted-foreground, #71717a);
    flex: 0 0 auto;
    height: 16px;
    width: 16px;
  }

  .listbox {
    background: var(--popover, #ffffff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 12%);
    box-sizing: border-box;
    display: none;
    margin-top: 4px;
    max-height: 260px;
    overflow: auto;
    padding: 4px;
    position: absolute;
    width: 100%;
    z-index: 40;
  }

  :host([open]) .listbox {
    display: block;
  }

  .option {
    align-items: center;
    border-radius: 6px;
    color: var(--foreground, #18181b);
    cursor: pointer;
    display: flex;
    font-size: 0.875rem;
    gap: 8px;
    justify-content: space-between;
    padding: 6px 8px;
  }

  .option:hover {
    background: var(--accent, #f4f4f5);
  }

  .option .check {
    height: 15px;
    opacity: 0;
    width: 15px;
  }

  .option[aria-selected="true"] .check {
    opacity: 1;
  }

  :host([disabled]) .trigger {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }
`;

export function AppSelect({
  options = [],
  value = "",
  placeholder = "Select an option",
  disabled = false,
}: {
  options?: SelectOption[];
  value?: string;
  placeholder?: string;
  disabled?: boolean;
}): Component {
  let open = state(false);
  const current = options.find((option) => option.value === value);
  onMount(() => {
    const onOutside = (event: Event) => {
      if (open && !event.composedPath().includes(this)) {
        open = false;
        this.removeAttribute("open");
      }
    };
    document.addEventListener("click", onOutside);
    return () => document.removeEventListener("click", onOutside);
  });
  return (
    <>
      <style>{styles}</style>
      <button
        class="trigger"
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open ? "true" : "false"}
        onClick={() => {
          if (this.hasAttribute("disabled")) return;
          open = !open;
          this.toggleAttribute("open", open);
        }}
      >
        {current ? <span>{current.label}</span> : <span class="placeholder">{placeholder}</span>}
        <span class="chevron" aria-hidden="true">
          <IconChevronDown />
        </span>
      </button>
      <div
        class="listbox"
        role="listbox"
        onClick={(event: Event) => {
          const option = (event.target as Element).closest(".option");
          if (!option) return;
          const next = option.getAttribute("data-value") ?? "";
          open = false;
          this.removeAttribute("open");
          this.dispatchEvent(
            new CustomEvent("value-change", { bubbles: true, composed: true, detail: { value: next } }),
          );
        }}
      >
        {options
          .map(
            (option) =>
              `<div class="option" role="option" data-value="${option.value}" aria-selected="${
                value === option.value
              }"><span>${option.label}</span><span class="check" aria-hidden="true">${checkIcon}</span></div>`,
          )
          .join("")}
      </div>
    </>
  );
}
