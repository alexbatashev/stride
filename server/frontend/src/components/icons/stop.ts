import { component, html } from '@frontiers-labs/argon';

@component('icon-stop')
export function IconStop(): string {
  return html`<style>:host{display:inline-flex;align-items:center;width:16px;height:16px}svg{width:100%;height:100%}</style><svg
    xmlns="http://www.w3.org/2000/svg"
    width="16"
    height="16"
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  ><rect x="4" y="4" width="16" height="16" rx="2" /></svg>`;
}
