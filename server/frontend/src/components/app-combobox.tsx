/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit, onMount, state } from "@frontiers-labs/argon";
import { IconCheck } from "./icons/check.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { AppButton } from "./app-button.js";
import { AppInput } from "./app-input.js";

interface ComboboxOption { value: string; label: string; disabled?: boolean }

function selectedLabel(options: ComboboxOption[], value: string): string {
  const selected = options.find((option) => option.value === value);
  if (selected !== undefined) return selected.label;
  return "";
}

const styles = css`
  :host { display: block; position: relative; width: 100%; }
  .control { align-items: center; background: transparent; border: 1px solid var(--input); border-radius: var(--radius-md, 8px); box-shadow: 0 1px 2px rgb(0 0 0 / 5%); display: flex; height: 36px; transition: border-color 150ms ease, box-shadow 150ms ease; }
  .control:focus-within { border-color: var(--ring); box-shadow: 0 0 0 3px var(--ring-shadow); }
  app-input { flex: 1; min-width: 0; }
  .trigger { align-self: center; flex: 0 0 auto; margin-right: 3px; }
  .popup { background: var(--popover); border: 1px solid color-mix(in oklab, var(--foreground) 10%, transparent); border-radius: var(--radius-md, 8px); box-shadow: 0 8px 20px rgb(0 0 0 / 12%); box-sizing: border-box; color: var(--popover-foreground); display: none; margin-top: 6px; max-height: 320px; min-width: 100%; overflow-y: auto; padding: 4px; position: absolute; z-index: 50; }
  :host([open]) .popup { display: block; }
  .option { align-items: center; border-radius: var(--radius-sm, 6px); cursor: default; display: flex; font-size: 0.875rem; gap: 8px; min-height: 32px; outline: none; padding: 6px 32px 6px 8px; position: relative; user-select: none; }
  .option:hover, .option:focus-visible { background: var(--accent); color: var(--accent-foreground); }
  .option[aria-disabled="true"] { opacity: 0.5; pointer-events: none; }
  .check { align-items: center; display: none; height: 16px; justify-content: center; position: absolute; right: 8px; width: 16px; }
  .option[aria-selected="true"] .check { display: inline-flex; }
  .empty { color: var(--muted-foreground); font-size: 0.875rem; padding: 8px; text-align: center; }
  :host([disabled]) { opacity: 0.5; pointer-events: none; }
  @media (prefers-color-scheme: dark) { .control { background: color-mix(in oklab, var(--input) 30%, transparent); } }
`;

export function AppCombobox({ options = [], value = "", placeholder = "Search options", emptyText = "No results found.", disabled = false }: { options?: ComboboxOption[]; value?: string; placeholder?: string; emptyText?: string; disabled?: boolean }): Component {
  let query = state(selectedLabel(options, value));
  let open = state(false);
  const visible = options.filter((option) => option.label.toLowerCase().includes(query.toLowerCase()));
  const optionItems = visible.map((option) => <div key={option.value} class="option" role="option" tabindex="0" data-value={option.value} aria-selected={option.value === value ? "true" : "false"} aria-disabled={option.disabled ?? false}><span>{option.label}</span><span class="check" aria-hidden="true"><IconCheck /></span></div>);
  const optionContent = visible.length === 0 ? <div class="empty">{emptyText}</div> : optionItems;
  onMount(() => {
    const close = (event: Event) => {
      if (open && !event.composedPath().includes(this)) { open = false; this.removeAttribute("open"); }
    };
    document.addEventListener("click", close);
    return () => document.removeEventListener("click", close);
  });
  return <><style>{styles}</style><div class="control" role="combobox" aria-label={placeholder} aria-expanded={open ? "true" : "false"} aria-controls="options" onFocusIn={() => { open = true; this.setAttribute("open", ""); }} on:value-change={(event: CustomEvent) => { event.stopPropagation(); query = event.detail.value; open = true; this.setAttribute("open", ""); emit(this, "search-change", { value: query }); }}><AppInput variant="ghost" kind="text" value={query} placeholder={placeholder}></AppInput><AppButton class="trigger" size="icon-xs" variant="ghost" onClick={() => { if (disabled) return; open = !open; this.toggleAttribute("open", open); }}><IconChevronDown /><span class="sr-only">Toggle options</span></AppButton></div><div id="options" class="popup" role="listbox" onClick={(event: Event) => { const option = (event.target as Element).closest(".option"); if (!option || option.getAttribute("aria-disabled") === "true") return; const next = option.getAttribute("data-value") ?? ""; query = option.textContent?.trim() ?? ""; open = false; this.removeAttribute("open"); emit(this, "value-change", { value: next }); }}>{optionContent}</div></>;
}
