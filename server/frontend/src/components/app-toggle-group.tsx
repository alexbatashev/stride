/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";
import { AppToggle } from "./app-toggle.js";

interface ToggleOption { value: string; label: string; disabled?: boolean }

const styles = css`
  :host { display: inline-flex; }
  .group { align-items: center; display: flex; width: fit-content; }
  app-toggle { flex: 0 0 auto; }
  :host([variant="outline"]) app-toggle + app-toggle { margin-left: -1px; }
`;

export function AppToggleGroup({ options = [], value = [], kind = "multiple", variant = "default", size = "default" }: { options?: ToggleOption[]; value?: string[]; kind?: string; variant?: string; size?: string }): Component {
  const buttons = options.map((option, index) => <AppToggle key={option.value} data-value={option.value} pressed={value.includes(option.value)} disabled={option.disabled ?? false} variant={variant} size={size} position={options.length === 1 ? "only" : index === 0 ? "first" : index === options.length - 1 ? "last" : "middle"}>{option.label}</AppToggle>);
  return <><style>{styles}</style><div class="group" role="group" on:pressed-change={(event: CustomEvent) => { const target = event.target as HTMLElement; const next = target.dataset.value ?? ""; const values = kind === "single" ? (value.includes(next) ? [] : [next]) : (value.includes(next) ? value.filter((item) => item !== next) : [...value, next]); emit(this, "value-change", { value: values }); }}>{buttons}</div></>;
}
