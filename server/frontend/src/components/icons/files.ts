import { Component, html } from "@frontiers-labs/argon";

export function IconFiles(): Component<"icon-files"> {
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
      <path d="M15 2H6a2 2 0 0 0-2 2v12" />
      <path
        d="M17 6H8a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2Z"
      />
    </svg>`;
}
