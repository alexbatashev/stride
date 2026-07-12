import { Component, css, emit, state } from "@frontiers-labs/argon";
import type { ModelOption } from "../shared/model-option.js";
import { IconCheck } from "./icons/check.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconEye } from "./icons/eye.js";

const styles = css`
  :host { display: block; min-width: 0; position: relative; }
  .trigger { display: block; min-width: 0; }
  .trigger-button { align-items: center; background: transparent; border: 0; border-radius: 8px; color: var(--prompt-control-fg, #efefef); cursor: pointer; display: flex; font: inherit; font-size: 1rem; font-weight: 500; height: 40px; padding: 0 4px; }
  .trigger-button:hover { background: var(--prompt-control-hover-bg, #303030); }
  .trigger-button:focus-visible { box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%)); outline: none; }
  .trigger-button:disabled { cursor: not-allowed; opacity: 0.5; }
  .trigger-content { align-items: center; display: flex; gap: 10px; min-width: 0; width: 100%; }
  .trigger-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .effort { color: var(--prompt-muted, #a0a0a0); font-weight: 400; }
  .trigger app-button icon-chevron-down { color: var(--prompt-muted, #a0a0a0); height: 18px; margin-left: 2px; width: 18px; }
  .popup { background: var(--popover, #292929); border: 1px solid var(--prompt-control-border, #3a3a3a); border-radius: 12px; box-shadow: 0 16px 36px rgb(0 0 0 / 30%); box-sizing: border-box; display: none; margin-top: 8px; max-height: min(360px, 50vh); min-width: min(340px, calc(100vw - 32px)); overflow-y: auto; padding: 4px; position: absolute; right: 0; width: max-content; z-index: 30; }
  :host([open]) .popup { display: block; }
  .empty { color: var(--muted-foreground); font-size: 0.8125rem; padding: 10px; }
  :host([disabled]) { opacity: 0.5; pointer-events: none; }
  @media (max-width: 640px) { .trigger { max-width: 180px; } .popup { min-width: min(300px, calc(100vw - 32px)); } }
`;

const optionStyles = css`
  :host { display: block; }
  button { align-items: flex-start; background: transparent; border: 0; border-radius: 9px; color: var(--popover-foreground, var(--foreground)); cursor: pointer; display: grid; font: inherit; gap: 3px; grid-template-columns: minmax(0, 1fr) 18px; padding: 10px 11px; text-align: left; width: 100%; }
  button:hover, button:focus-visible { background: var(--accent, #373737); outline: none; }
  .name-line { align-items: center; display: flex; font-size: 0.9375rem; font-weight: 600; gap: 6px; line-height: 1.25; min-width: 0; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .vision { color: var(--muted-foreground, #a3a3a3); display: inline-flex; flex: 0 0 auto; }
  .description { color: var(--muted-foreground, #a3a3a3); font-size: 0.75rem; grid-column: 1 / 2; line-height: 1.35; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .check { align-items: center; align-self: center; color: var(--primary, #fafafa); display: none; grid-column: 2; grid-row: 1 / 3; height: 18px; justify-content: center; width: 18px; }
  button[aria-selected="true"] .check { display: inline-flex; }
  .sr-only { border: 0; clip: rect(0, 0, 0, 0); height: 1px; margin: -1px; overflow: hidden; padding: 0; position: absolute; white-space: nowrap; width: 1px; }
`;

function chooseModel(host: HTMLElement, event: Event): void {
  host.removeAttribute("open");
  host.dispatchEvent(new CustomEvent("value-change", {
    bubbles: true,
    composed: true,
    detail: (event as CustomEvent<{ value: string }>).detail,
  }));
}

function ModelPickerOption({ model, selected }: { model: ModelOption; selected: boolean }): Component {
  return (
    <>
      <style>{optionStyles}</style>
      <button type="button" role="option" aria-selected={selected ? "true" : "false"} onClick={() => emit(this, "model-select", { value: model.value })}>
        <span class="name-line"><span class="name">{model.label}</span>{model.vision && <span class="vision" title="Supports vision"><IconEye /><span class="sr-only">Supports vision</span></span>}</span>
        <span class="description">{model.description || "No description available."}</span>
        <span class="check"><IconCheck /></span>
      </button>
    </>
  );
}

export function AppModelPicker({ models = [], value = "", label = "Choose model", reasoningEffort = "", disabled = false }: { models?: ModelOption[]; value?: string; label?: string; reasoningEffort?: string; disabled?: boolean }): Component {
  let open = state(false);
  const hasReasoningEffort = reasoningEffort !== "";

  return (
    <>
      <style>{styles}</style>
      <div class="trigger">
        <button class="trigger-button" type="button" disabled={disabled} aria-label="Choose model" onClick={() => { open = !open; this.toggleAttribute("open", open); }}>
          <span class="trigger-content"><span class="trigger-label">{label}</span>{hasReasoningEffort && <span class="effort">{reasoningEffort}</span>}<IconChevronDown /></span>
        </button>
      </div>
      <div class="popup" role="listbox" aria-label="Models">
        {models.length === 0
          ? <div class="empty">No models available</div>
          : models.map((model) => (
            <ModelPickerOption key={model.value} model={model} selected={model.value === value} on:model-select={(event: Event) => chooseModel(this, event)} />
          ))}
      </div>
    </>
  );
}
