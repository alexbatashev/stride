/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, onMount, ref } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  label {
    color: var(--foreground, #09090b);
    display: grid;
    font-size: 0.875rem;
    font-weight: 500;
    gap: 8px;
    line-height: 1.35;
  }

  input {
    background: var(--background, transparent);
    border: 1px solid var(--input, #e4e4e7);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    font: inherit;
    font-size: 1rem;
    height: 32px;
    min-width: 0;
    outline: none;
    padding: 4px 10px;
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease,
      background-color 140ms ease,
      opacity 140ms ease;
    width: 100%;
  }

  input:focus {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  input::placeholder {
    color: var(--muted-foreground, #71717a);
  }

  input:disabled {
    background: var(--input-disabled, rgb(244 244 245 / 50%));
    cursor: not-allowed;
    opacity: 0.5;
  }

  @media (min-width: 768px) {
    input {
      font-size: 0.875rem;
    }
  }
`;

export function AppTextInput({
  autocomplete = "",
  disabled = false,
  label = "",
  name = "",
  placeholder = "",
  required = false,
  kind = "text",
  value = "",
}: {
  autocomplete?: string;
  disabled?: boolean;
  label?: string;
  name?: string;
  placeholder?: string;
  required?: boolean;
  kind?: string;
  value?: string;
}): Component {
  const input = ref<HTMLInputElement>();
  onMount(() => {
    (this as HTMLElement & { focusControl: () => void }).focusControl = () => {
      input.current?.focus();
    };
  });
  effect(() => {
    const el = input.current;
    if (!el) return;
    el.toggleAttribute("disabled", disabled);
    el.toggleAttribute("required", required);
  });
  return (
    <>
      <style>{styles}</style>
      <label>
        {label}
        <input
          ref={input}
          autocomplete={autocomplete}
          name={name}
          placeholder={placeholder}
          type={kind}
          value={value}
          onInput={(event: Event) => {
            this.value = (event.target as HTMLInputElement).value;
            emit(this, "value-change", { value: this.value });
          }}
          onKeyDown={(event: KeyboardEvent) => {
            if (event.key !== "Enter") {
              return;
            }
            emit(this, "commit");
          }}
        />
      </label>
    </>
  );
}
