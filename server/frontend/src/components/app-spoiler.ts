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

    .arrow {
      display: inline-block;
      transform-origin: center;
      transition: transform 120ms ease;
    }

    .arrow[data-open="true"] {
      transform: rotate(90deg);
    }

    .content {
      margin-top: 8px;
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
        <span>${this.title}</span>
        <span class="arrow" data-open=${this.visible ? "true" : "false"} aria-hidden="true">></span>
      </button>
      ${this.visible
        ? html`<div class="content">${this.content || html`<slot></slot>`}</div>`
        : null}
    `;
  }

  private toggle() {
    this.visible = !this.visible;
  }
}
