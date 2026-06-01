import { Component, html } from "@frontiers-labs/argon";

export function IconPanelLeftOpen(): Component<"icon-panel-left-open"> {
  return html`<style>
      :host {
        display: inline-flex;
        align-items: center;
        width: 24px;
        height: 24px;
      }
      svg {
        width: 100%;
        height: 100%;
      }</style
    ><svg
      xmlns="http://www.w3.org/2000/svg"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 3v18" />
      <path d="m14 9 3 3-3 3" />
    </svg>`;
}
