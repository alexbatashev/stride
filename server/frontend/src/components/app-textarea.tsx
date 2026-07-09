/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, ref } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  textarea {
    background: var(--background, transparent);
    border: 1px solid var(--input, #e4e4e7);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground, #09090b);
    font: inherit;
    font-size: 1rem;
    line-height: 1.4;
    min-height: 64px;
    min-width: 0;
    outline: none;
    padding: 8px 10px;
    resize: vertical;
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease,
      opacity 140ms ease;
    width: 100%;
  }

  textarea:focus {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  textarea::placeholder {
    color: var(--muted-foreground, #71717a);
  }

  textarea:disabled {
    background: var(--input-disabled, rgb(244 244 245 / 50%));
    cursor: not-allowed;
    opacity: 0.5;
  }

  @media (min-width: 768px) {
    textarea {
      font-size: 0.875rem;
    }
  }
`;

export function AppTextarea({
  disabled = false,
  name = "",
  placeholder = "",
  required = false,
  rows = "3",
  value = "",
}: {
  disabled?: boolean;
  name?: string;
  placeholder?: string;
  required?: boolean;
  rows?: string;
  value?: string;
}): Component {
  const area = ref<HTMLTextAreaElement>();
  effect(() => {
    const el = area.current;
    if (!el) return;
    el.toggleAttribute("disabled", disabled);
    el.toggleAttribute("required", required);
  });
  return (
    <>
      <style>{styles}</style>
      <textarea
        ref={area}
        name={name}
        placeholder={placeholder}
        rows={rows}
        onInput={(event: Event) => {
          this.value = (event.target as HTMLTextAreaElement).value;
          emit(this, "value-change", { value: this.value });
        }}
      >
        {value}
      </textarea>
    </>
  );
}
