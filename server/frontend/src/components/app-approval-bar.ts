import { Component, css, html } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-block;
    max-width: 960px;
    padding: 8px;
    width: 100%;
  }

  :host([hidden]) {
    display: none;
  }

  .bar {
    align-items: center;
    background: var(--prompt-bg, #212121);
    border: 1px solid var(--prompt-border, #333333);
    border-radius: 18px;
    box-sizing: border-box;
    color: var(--prompt-fg, #d4d4d4);
    display: flex;
    gap: 16px;
    min-height: 64px;
    padding: 12px 14px 12px 18px;
    width: 100%;
  }

  .message {
    flex: 1 1 auto;
    font-size: 0.95rem;
    line-height: 1.35;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
  }

  button {
    border-radius: 999px;
    cursor: pointer;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 600;
    height: 36px;
    min-width: 64px;
    padding: 0 14px;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease,
      color 140ms ease,
      opacity 140ms ease;
  }

  button:focus-visible {
    box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
    outline: none;
  }

  .yes {
    background: var(--prompt-send-ready-bg, #f4f4f5);
    border: 1px solid var(--prompt-send-ready-bg, #f4f4f5);
    color: var(--prompt-send-ready-fg, #18181b);
  }

  .no {
    background: transparent;
    border: 1px solid var(--prompt-control-border, #343434);
    color: var(--prompt-control-fg, #bdbdbd);
  }

  .no:hover {
    background: var(--prompt-control-hover-bg, #2d2d2d);
    color: var(--prompt-control-hover-fg, #e4e4e7);
  }

  .yes:hover {
    opacity: 0.92;
  }

  @media (max-width: 640px) {
    .bar {
      align-items: stretch;
      flex-direction: column;
      gap: 12px;
    }

    .actions {
      justify-content: flex-end;
    }
  }
`;

function answer(event: Event, approved: boolean): void {
  const root = (event.currentTarget as Element).getRootNode();
  const host = root instanceof ShadowRoot ? root.host : null;
  host?.dispatchEvent(
    new CustomEvent("approval-response", {
      bubbles: true,
      composed: true,
      detail: { approved },
    }),
  );
}

export function AppApprovalBar(): Component<"app-approval-bar"> {
  return html`
    <style>${styles}</style>
    <div class="bar" role="group" aria-label="Approval request">
      <div class="message"><slot></slot></div>
      <div class="actions">
        <button class="yes" type="button" @click="${(event: Event) => answer(event, true)}">Yes</button>
        <button class="no" type="button" @click="${(event: Event) => answer(event, false)}">No</button>
      </div>
    </div>
  `;
}
