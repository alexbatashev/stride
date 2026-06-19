/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, ref } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  input {
    appearance: none;
    background: transparent;
    cursor: pointer;
    display: block;
    height: 16px;
    margin: 0;
    width: 100%;
  }

  input::-webkit-slider-runnable-track {
    background: var(--muted, #f4f4f5);
    border-radius: 999px;
    height: 6px;
  }

  input::-moz-range-track {
    background: var(--muted, #f4f4f5);
    border-radius: 999px;
    height: 6px;
  }

  input::-webkit-slider-thumb {
    appearance: none;
    background: var(--background, #ffffff);
    border: 1px solid var(--primary, #18181b);
    border-radius: 999px;
    height: 16px;
    margin-top: -5px;
    width: 16px;
  }

  input::-moz-range-thumb {
    background: var(--background, #ffffff);
    border: 1px solid var(--primary, #18181b);
    border-radius: 999px;
    height: 16px;
    width: 16px;
  }

  input:focus-visible::-webkit-slider-thumb {
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  input:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
`;

export function AppSlider({
  min = "0",
  max = "100",
  step = "1",
  value = "50",
  disabled = false,
}: {
  min?: string;
  max?: string;
  step?: string;
  value?: string;
  disabled?: boolean;
}): Component {
  const input = ref<HTMLInputElement>();
  effect(() => {
    const el = input.current;
    if (el) el.toggleAttribute("disabled", disabled);
  });
  return (
    <>
      <style>{styles}</style>
      <input
        ref={input}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onInput={(event: Event) => {
          const next = (event.target as HTMLInputElement).value;
          this.setAttribute("value", next);
          this.dispatchEvent(
            new CustomEvent("value-change", {
              bubbles: true,
              composed: true,
              detail: { value: Number(next) },
            }),
          );
        }}
      />
    </>
  );
}
