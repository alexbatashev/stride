/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, ref } from "@frontiers-labs/argon";

const styles = css`
  :host { display: block; width: 100%; }
  input { background: transparent; border: 1px solid var(--input); border-radius: var(--radius-md, 8px); box-shadow: 0 1px 2px rgb(0 0 0 / 5%); box-sizing: border-box; color: var(--foreground); font: inherit; font-size: 1rem; height: 36px; min-width: 0; outline: none; padding: 4px 12px; transition: border-color 150ms ease, box-shadow 150ms ease; width: 100%; }
  input::placeholder { color: var(--muted-foreground); }
  input:focus-visible { border-color: var(--ring); box-shadow: 0 0 0 3px var(--ring-shadow); }
  input:disabled { cursor: not-allowed; opacity: 0.5; pointer-events: none; }
  input[aria-invalid="true"] { border-color: var(--destructive); box-shadow: 0 0 0 3px var(--destructive-muted); }
  @media (min-width: 768px) { input { font-size: 0.875rem; } }
  @media (prefers-color-scheme: dark) { input { background: color-mix(in oklab, var(--input) 30%, transparent); } }
  :host([variant="ghost"]) input { background: transparent; border: 0; border-radius: 0; box-shadow: none; height: 34px; }
`;

export function AppInput({ kind = "text", value = "", placeholder = "", name = "", autocomplete = "", disabled = false, invalid = false }: { kind?: string; value?: string; placeholder?: string; name?: string; autocomplete?: string; disabled?: boolean; invalid?: boolean }): Component {
  const input = ref<HTMLInputElement>();
  effect(() => {
    const el = input.current;
    if (!el) return;
    el.toggleAttribute("disabled", disabled);
    if (el.value !== value) el.value = value;
  });
  return <><style>{styles}</style><input ref={input} type={kind} value={value} placeholder={placeholder} name={name} autocomplete={autocomplete} aria-invalid={invalid ? "true" : "false"} onInput={(event: Event) => emit(this, "value-change", { value: (event.target as HTMLInputElement).value })} /></>;
}
