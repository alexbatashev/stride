/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

interface RadioOption {
  value: string;
  label: string;
}

const styles = css`
  :host {
    display: block;
  }

  .group {
    display: grid;
    gap: 10px;
  }

  .option {
    align-items: center;
    color: var(--foreground, #09090b);
    cursor: pointer;
    display: flex;
    font-size: 0.875rem;
    gap: 8px;
  }

  .radio {
    align-items: center;
    background: var(--background, #ffffff);
    border: 1px solid var(--input, #e4e4e7);
    border-radius: 999px;
    box-sizing: border-box;
    display: inline-flex;
    flex: 0 0 auto;
    height: 16px;
    justify-content: center;
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease;
    width: 16px;
  }

  .option:focus-visible .radio {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
    outline: none;
  }

  .option[aria-checked="true"] .radio {
    border-color: var(--primary, #18181b);
  }

  .dot {
    background: var(--primary, #18181b);
    border-radius: 999px;
    height: 8px;
    transform: scale(0);
    transition: transform 120ms ease;
    width: 8px;
  }

  .option[aria-checked="true"] .dot {
    transform: scale(1);
  }

  :host([disabled]) .group {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }
`;

export function AppRadioGroup({
  options = [],
  value = "",
  name = "",
  disabled = false,
}: {
  options?: RadioOption[];
  value?: string;
  name?: string;
  disabled?: boolean;
}): Component {
  return (
    <>
      <style>{styles}</style>
      <div
        class="group"
        role="radiogroup"
        data-name={name}
        onClick={(event: Event) => {
          if (disabled) return;
          const option = (event.target as Element).closest(".option");
          if (!option) return;
          const next = option.getAttribute("data-value") ?? "";
          if (next === value) return;
          this.dispatchEvent(
            new CustomEvent("value-change", { bubbles: true, composed: true, detail: { value: next } }),
          );
        }}
      >
        {options
          .map((option) => (
            <div
              class="option"
              role="radio"
              tabindex="0"
              data-value={option.value}
              aria-checked={value === option.value ? "true" : "false"}
            >
              <span class="radio" aria-hidden="true"><span class="dot"></span></span>
              <span>{option.label}</span>
            </div>
          ))
          .join("")}
      </div>
    </>
  );
}
