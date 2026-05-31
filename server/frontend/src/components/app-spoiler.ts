import { LitElement, css, html } from "lit";
import { customElement, property, state } from "lit/decorators.js";

@customElement("app-spoiler")
export class AppSpoiler extends LitElement {
  static styles = css`
    :host {
      display: block;
    }

    button {
      align-items: center;
      background: transparent;
      border: 0;
      color: inherit;
      cursor: pointer;
      display: inline-flex;
      font: inherit;
      gap: 4px;
      padding: 0;
    }

    .chevron {
      align-items: center;
      display: inline-flex;
      flex: 0 0 1em;
      height: 1em;
      justify-content: center;
      width: 1em;
    }

    .chevron > * {
      height: 1em;
      width: 1em;
    }

    .content {
      margin-top: 8px;
      margin-bottom: 16px;
    }

    .title {
      font-weight: bold;
      font-size: 0.95rem;
    }
  `;

  @property()
  title: string = "Spoiler title";

  @property()
  content: string = "";

  @state()
  visible: boolean = false;

  render() {
    return html`
      <button
        type="button"
        aria-expanded=${this.visible ? "true" : "false"}
        @click=${this.toggle}
      >
        <span class="title">${this.title}</span>
        <span class="chevron" aria-hidden="true"
          >${this.visible ? html`<icon-chevron-down></icon-chevron-down>` : html`<icon-chevron-right></icon-chevron-right>`}</span
        >
      </button>
      ${this.visible
        ? html`<div class="content">
            ${this.content || html`<slot></slot>`}
          </div>`
        : null}
    `;
  }

  private toggle() {
    this.visible = !this.visible;
  }
}
