/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, ref } from "@frontiers-labs/argon";

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
    background: linear-gradient(to right, var(--primary) 0 var(--slider-value, 50%), var(--muted) var(--slider-value, 50%) 100%);
    border-radius: 999px;
    height: 6px;
  }

  input::-moz-range-track {
    background: linear-gradient(to right, var(--primary) 0 var(--slider-value, 50%), var(--muted) var(--slider-value, 50%) 100%);
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

  :host([orientation="vertical"]) { display: inline-block; height: 176px; width: auto; }
  :host([orientation="vertical"]) input { height: 176px; width: 16px; writing-mode: vertical-lr; direction: rtl; }
`;

export function AppSlider({
  min = "0",
  max = "100",
  step = "1",
  value = "50",
  disabled = false,
  orientation = "horizontal",
}: {
  min?: string;
  max?: string;
  step?: string;
  value?: string;
  disabled?: boolean;
  orientation?: string;
}): Component {
  const input = ref<HTMLInputElement>();
  effect(() => {
    const el = input.current;
    if (!el) return;
    el.toggleAttribute("disabled", disabled);
    const low = Number(min);
    const high = Number(max);
    const current = Number(value);
    const percentage = high > low ? Math.max(0, Math.min(100, ((current - low) / (high - low)) * 100)) : 0;
    el.style.setProperty("--slider-value", `${percentage}%`);
    this.setAttribute("orientation", orientation);
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
          emit(this, "value-change", { value: Number(next) });
        }}
      />
    </>
  );
}
