import { component, html } from '@frontiers-labs/argon';

@component('icon-arrow-up')
export function IconArrowUp(): string {
  return html`<style>:host{display:inline-flex;align-items:center;width:24px;height:24px}svg{width:100%;height:100%}</style><svg
    xmlns="http://www.w3.org/2000/svg"
    width="24"
    height="24"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
  ><path d="m5 12 7-7 7 7" /><path d="M12 19V5" /></svg>`;
}
