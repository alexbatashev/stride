/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, html } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-block;
  }

  button {
    align-items: center;
    background: var(--primary, #18181b);
    background-clip: padding-box;
    border: 1px solid transparent;
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--primary-foreground, #fafafa);
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    gap: 6px;
    height: 32px;
    justify-content: center;
    line-height: 1;
    outline: none;
    padding: 0 10px;
    position: relative;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease,
      color 140ms ease,
      opacity 140ms ease,
      transform 80ms ease;
    user-select: none;
    white-space: nowrap;
    width: 100%;
  }

  button:hover {
    background: var(--primary-hover, #27272a);
  }

  button:focus-visible {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  button:active {
    transform: translateY(1px);
  }

  :host([variant="outline"]) button {
    background: var(--background, #ffffff);
    border-color: var(--border, #e4e4e7);
    color: var(--foreground, #18181b);
  }

  :host([variant="outline"]) button:hover {
    background: var(--muted, #f4f4f5);
  }

  :host([variant="secondary"]) button {
    background: var(--secondary, #f4f4f5);
    color: var(--secondary-foreground, #18181b);
  }

  :host([variant="secondary"]) button:hover {
    background: var(--secondary-hover, #e4e4e7);
  }

  :host([variant="ghost"]) button {
    background: transparent;
    color: var(--foreground, #18181b);
  }

  :host([variant="ghost"]) button:hover {
    background: var(--muted, #f4f4f5);
  }

  :host([variant="destructive"]) button {
    background: var(--destructive-muted, rgb(220 38 38 / 10%));
    color: var(--destructive, #dc2626);
  }

  :host([variant="destructive"]) button:hover {
    background: var(--destructive-hover, rgb(220 38 38 / 20%));
  }

  :host([variant="destructive"]) button:focus-visible {
    border-color: var(--destructive-ring, rgb(220 38 38 / 40%));
    box-shadow: 0 0 0 3px var(--destructive-shadow, rgb(220 38 38 / 20%));
  }

  :host([variant="link"]) button {
    background: transparent;
    color: var(--primary, #18181b);
    height: auto;
    padding: 0;
    text-underline-offset: 4px;
  }

  :host([variant="link"]) button:hover {
    background: transparent;
    text-decoration: underline;
  }

  :host([size="xs"]) button {
    border-radius: 8px;
    font-size: 0.75rem;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
  }

  :host([size="sm"]) button {
    border-radius: 8px;
    font-size: 0.8rem;
    gap: 4px;
    height: 28px;
    padding: 0 10px;
  }

  :host([size="lg"]) button {
    height: 36px;
    padding: 0 10px;
  }

  :host([size="icon"]) button {
    height: 32px;
    padding: 0;
    width: 32px;
  }

  :host([size="icon-xs"]) button {
    border-radius: 8px;
    height: 24px;
    padding: 0;
    width: 24px;
  }

  :host([size="icon-sm"]) button {
    border-radius: 8px;
    height: 28px;
    padding: 0;
    width: 28px;
  }

  :host([size="icon-lg"]) button {
    height: 36px;
    padding: 0;
    width: 36px;
  }

  :host([disabled]) button,
  :host([loading]) button {
    cursor: default;
    opacity: 0.5;
    pointer-events: none;
  }

  :host([disabled]) button:active,
  :host([loading]) button:active {
    transform: none;
  }

  .spinner {
    animation: spin 800ms linear infinite;
    border: 2px solid currentcolor;
    border-radius: 999px;
    border-right-color: transparent;
    display: none;
    height: 14px;
    width: 14px;
  }

  :host([loading]) .spinner {
    display: inline-block;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
`;

function suppressClickIfDisabled(event: Event): void {
  const root = (event.currentTarget as Element).getRootNode();
  const host = root instanceof ShadowRoot ? root.host : null;

  if (host?.hasAttribute("disabled") || host?.hasAttribute("loading")) {
    event.preventDefault();
    event.stopImmediatePropagation();
  }
}

export function AppButton(): Component<"app-button"> {
  return html`
    <style>${styles}</style>
    <button type="button" @click="${(event: Event) => suppressClickIfDisabled(event)}">
      <span class="spinner" aria-hidden="true"></span>
      <slot></slot>
    </button>
  `;
}
