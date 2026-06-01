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
    background: var(--prompt-bg, #212121);
    border: 1px solid var(--prompt-border, #333333);
    border-radius: 18px;
    box-sizing: border-box;
    color: var(--prompt-fg, #d4d4d4);
    display: grid;
    gap: 14px;
    min-height: 86px;
    padding: 16px 18px;
    width: 100%;
  }

  .question {
    font-size: 0.98rem;
    font-weight: 600;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .footer {
    align-items: end;
    display: flex;
    gap: 16px;
    justify-content: space-between;
  }

  .answers {
    display: grid;
    gap: 10px;
    min-width: 0;
    width: 100%;
  }

  .options {
    min-width: 0;
  }

  ::slotted(label) {
    align-items: center;
    color: var(--prompt-control-fg, #bdbdbd);
    cursor: pointer;
    display: inline-flex;
    font-size: 0.9rem;
    gap: 8px;
    margin: 0 14px 8px 0;
  }

  input[type="text"] {
    background: transparent;
    border: 1px solid var(--prompt-control-border, #343434);
    border-radius: 12px;
    box-sizing: border-box;
    color: var(--prompt-fg, #d4d4d4);
    font: inherit;
    font-size: 0.9rem;
    height: 38px;
    outline: none;
    padding: 0 12px;
    width: 100%;
  }

  input[type="text"]::placeholder {
    color: var(--prompt-muted, #747474);
  }

  input[type="text"]:focus {
    border-color: var(--prompt-focus-border, #3c3c3c);
    box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
  }

  button {
    background: var(--prompt-send-ready-bg, #f4f4f5);
    border: 1px solid var(--prompt-send-ready-bg, #f4f4f5);
    border-radius: 999px;
    color: var(--prompt-send-ready-fg, #18181b);
    cursor: pointer;
    flex: 0 0 auto;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 600;
    height: 36px;
    min-width: 88px;
    padding: 0 16px;
    transition:
      box-shadow 140ms ease,
      opacity 140ms ease;
  }

  button:focus-visible {
    box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
    outline: none;
  }

  @media (max-width: 640px) {
    .footer {
      align-items: stretch;
      flex-direction: column;
    }

    button {
      align-self: flex-end;
    }
  }
`;

function selectedAnswer(root: ShadowRoot, host: Element): string {
  const custom = root.querySelector<HTMLInputElement>('input[type="text"]')?.value.trim() ?? "";
  if (custom.length > 0) return custom;

  const input = host.querySelector<HTMLInputElement>('input[type="radio"]:checked');
  return input ? input.value : "";
}

function submitAnswer(event: Event): void {
  const root = (event.currentTarget as Element).getRootNode();
  const host = root instanceof ShadowRoot ? root.host : null;
  if (!host || !(root instanceof ShadowRoot)) return;

  const answer = selectedAnswer(root, host);
  if (!answer) return;

  host.dispatchEvent(
    new CustomEvent("quiz-response", {
      bubbles: true,
      composed: true,
      detail: { answer },
    }),
  );
}

export function AppQuizBar(): Component<"app-quiz-bar"> {
  return html`
    <style>${styles}</style>
    <div class="bar" role="group" aria-label="Question">
      <div class="question"><slot name="question"></slot></div>
      <div class="footer">
        <div class="answers">
          <div class="options"><slot name="options"></slot></div>
          <input type="text" placeholder="Custom answer" />
        </div>
        <button type="button" @click="${(event: Event) => submitAnswer(event)}">Continue</button>
      </div>
    </div>
  `;
}
