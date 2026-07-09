/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";
import { IconCheck } from "./icons/check.js";

const styles = css`
  :host {
    display: inline-flex;
  }

  button {
    align-items: center;
    background: var(--background, #ffffff);
    border: 1px solid var(--input, #e4e4e7);
    border-radius: 6px;
    box-sizing: border-box;
    color: var(--primary-foreground, #fafafa);
    cursor: pointer;
    display: inline-flex;
    flex: 0 0 auto;
    height: 18px;
    justify-content: center;
    outline: none;
    padding: 0;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease;
    width: 18px;
  }

  button:focus-visible {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  button[aria-checked="true"] {
    background: var(--primary, #18181b);
    border-color: var(--primary, #18181b);
  }

  .check {
    display: none;
    height: 13px;
    width: 13px;
  }

  button[aria-checked="true"] .check {
    display: inline-flex;
  }

  :host([disabled]) button {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }
`;

export function AppCheckbox({
  checked = false,
  disabled = false,
  name = "",
  value = "on",
}: {
  checked?: boolean;
  disabled?: boolean;
  name?: string;
  value?: string;
}): Component {
  return (
    <>
      <style>{styles}</style>
      <button
        type="button"
        role="checkbox"
        aria-checked={checked ? "true" : "false"}
        data-name={name}
        data-value={value}
        onClick={() => {
          if (disabled) return;
          emit(this, "change", { checked: !checked, value: value });
        }}
      >
        <span class="check" aria-hidden="true">
          <IconCheck />
        </span>
      </button>
    </>
  );
}
