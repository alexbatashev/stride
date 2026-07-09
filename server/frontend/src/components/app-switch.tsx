/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
  }

  button {
    align-items: center;
    background: var(--secondary-hover, #e4e4e7);
    border: 1px solid transparent;
    border-radius: 999px;
    box-sizing: border-box;
    cursor: pointer;
    display: inline-flex;
    flex: 0 0 auto;
    height: 20px;
    outline: none;
    padding: 2px;
    transition:
      background-color 140ms ease,
      box-shadow 140ms ease;
    width: 36px;
  }

  button:focus-visible {
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  button[aria-checked="true"] {
    background: var(--primary, #18181b);
  }

  .thumb {
    background: var(--background, #ffffff);
    border-radius: 999px;
    height: 14px;
    transition: transform 140ms ease;
    width: 14px;
  }

  button[aria-checked="true"] .thumb {
    transform: translateX(16px);
  }

  :host([disabled]) button {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }
`;

export function AppSwitch({
  checked = false,
  disabled = false,
  name = "",
}: {
  checked?: boolean;
  disabled?: boolean;
  name?: string;
}): Component {
  return (
    <>
      <style>{styles}</style>
      <button
        type="button"
        role="switch"
        aria-checked={checked ? "true" : "false"}
        data-name={name}
        onClick={() => {
          if (disabled) return;
          emit(this, "change", { checked: !checked });
        }}
      >
        <span class="thumb" aria-hidden="true"></span>
      </button>
    </>
  );
}
