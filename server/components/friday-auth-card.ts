import { LitElement, html, css } from 'lit';

/**
 * Reusable Material Design 3 card container for auth flows.
 * Renders a centered, elevated surface with a branded header.
 */
export class FridayAuthCard extends LitElement {
  static properties = {
    headline: { type: String },
    subhead: { type: String },
  };

  headline = 'Friday';
  subhead = '';

  static styles = css`
    :host {
      display: block;
    }

    .surface {
      background: var(--md-sys-color-surface-container-low, #f7f2fa);
      border-radius: 28px;
      padding: 32px;
      width: min(420px, 100%);
      box-shadow:
        0 1px 2px rgba(0, 0, 0, 0.1),
        0 4px 12px rgba(0, 0, 0, 0.06);
    }

    .brand {
      display: flex;
      align-items: center;
      gap: 12px;
      margin-bottom: 4px;
    }

    .logo {
      width: 40px;
      height: 40px;
      border-radius: 12px;
      background: var(--md-sys-color-primary, #6750a4);
      color: var(--md-sys-color-on-primary, #fff);
      display: grid;
      place-items: center;
      font-size: 20px;
      font-weight: 700;
      font-family: 'Google Sans', Roboto, sans-serif;
      flex-shrink: 0;
    }

    .headline {
      font-family: 'Google Sans', Roboto, sans-serif;
      font-size: 22px;
      font-weight: 500;
      color: var(--md-sys-color-on-surface, #1c1b1f);
      margin: 0;
    }

    .subhead {
      font-size: 13px;
      color: var(--md-sys-color-on-surface-variant, #49454f);
      margin: 4px 0 0;
      font-family: Roboto, sans-serif;
    }

    .divider {
      height: 1px;
      background: var(--md-sys-color-outline-variant, #cac4d0);
      margin: 20px 0;
    }
  `;

  render() {
    return html`
      <div class="surface">
        <div class="brand">
          <div class="logo">F</div>
          <h1 class="headline">${this.headline}</h1>
        </div>
        ${this.subhead ? html`<p class="subhead">${this.subhead}</p>` : ''}
        <div class="divider"></div>
        <slot></slot>
      </div>
    `;
  }
}

customElements.define('friday-auth-card', FridayAuthCard);

declare global {
  interface HTMLElementTagNameMap {
    'friday-auth-card': FridayAuthCard;
  }
}
