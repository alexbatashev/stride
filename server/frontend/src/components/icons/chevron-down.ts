import { Component, html } from "@frontiers-labs/argon";

export function IconChevronDown(): Component<"icon-chevron-down"> {
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
      <path d="m6 9 6 6 6-6" />
    </svg>`;
}
