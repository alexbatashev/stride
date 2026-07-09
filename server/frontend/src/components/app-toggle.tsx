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
    background: transparent;
    border: 1px solid transparent;
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground, #18181b);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    gap: 6px;
    height: 32px;
    justify-content: center;
    min-width: 32px;
    outline: none;
    padding: 0 10px;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease;
  }

  button:hover {
    background: var(--muted, #f4f4f5);
    color: var(--muted-foreground, #71717a);
  }

  button:focus-visible {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  :host([variant="outline"]) button {
    border-color: var(--border, #e4e4e7);
  }

  button[aria-pressed="true"] {
    background: var(--accent, #f4f4f5);
    color: var(--accent-foreground, #18181b);
  }

  :host([disabled]) button {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }
`;

export function AppToggle({ pressed = false, disabled = false }: { pressed?: boolean; disabled?: boolean }): Component {
  return (
    <>
      <style>{styles}</style>
      <button
        type="button"
        aria-pressed={pressed ? "true" : "false"}
        onClick={() => {
          if (disabled) return;
          emit(this, "pressed-change", { pressed: !pressed });
        }}
      >
        <slot></slot>
      </button>
    </>
  );
}
