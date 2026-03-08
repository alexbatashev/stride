import { LitElement, html, css } from 'lit';
import { customElement, property } from 'lit/decorators.js';

@customElement('counter-button')
export class CounterButton extends LitElement {
  static styles = css`
    button {
      padding: 0.5rem 1rem;
      font-size: 1rem;
      cursor: pointer;
    }
  `;

  @property({ type: Number })
  count = 0;

  render() {
    return html`
      <button @click=${this._increment}>
        Clicked ${this.count} time(s)
      </button>
    `;
  }

  private _increment() {
    this.count += 1;
    this.dispatchEvent(new CustomEvent('count-changed', { detail: this.count }));
  }
}
