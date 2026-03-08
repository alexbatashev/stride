import { LitElement, html, css, nothing } from 'lit';
import { customElement, state, query } from 'lit/decorators.js';
import { ifDefined } from 'lit/directives/if-defined.js';
import '@material/web/tabs/tabs.js';
import '@material/web/tabs/primary-tab.js';
import '@material/web/textfield/outlined-text-field.js';
import '@material/web/button/filled-button.js';
import '@material/web/button/text-button.js';
import '@material/web/progress/linear-progress.js';
import '@material/web/icon/icon.js';
import type { MdOutlinedTextField } from '@material/web/textfield/outlined-text-field.js';
import 'friday-components/friday-auth-card.js';
import { login, register, type AuthResult } from './auth-grpc.js';

type Mode = 'login' | 'register';

@customElement('friday-auth-page')
export class FridayAuthPage extends LitElement {
  @state() private mode: Mode = 'login';
  @state() private loading = false;
  @state() private error = '';
  @state() private success = false;

  @query('#email') private emailField!: MdOutlinedTextField;
  @query('#password') private passwordField!: MdOutlinedTextField;

  static styles = css`
    :host {
      display: grid;
      place-items: center;
      min-height: 100vh;
      padding: 24px;
    }

    .form-fields {
      display: flex;
      flex-direction: column;
      gap: 16px;
      margin-top: 20px;
    }

    md-outlined-text-field {
      width: 100%;
    }

    .actions {
      margin-top: 8px;
      display: flex;
      flex-direction: column;
      gap: 12px;
    }

    md-filled-button {
      width: 100%;
    }

    md-linear-progress {
      margin-top: 4px;
      border-radius: 4px;
    }

    .status-error {
      font-size: 13px;
      color: var(--md-sys-color-error, #b3261e);
      background: var(--md-sys-color-error-container, #f9dedc);
      border-radius: 8px;
      padding: 10px 12px;
      line-height: 1.4;
    }

    .status-success {
      font-size: 13px;
      color: var(--md-sys-color-on-tertiary-container, #072711);
      background: var(--md-sys-color-tertiary-container, #b6f2c8);
      border-radius: 8px;
      padding: 10px 12px;
      line-height: 1.4;
    }

    .footer-links {
      margin-top: 16px;
      text-align: center;
    }

    md-text-button {
      --md-text-button-label-text-size: 13px;
    }
  `;

  private onTabChange(e: Event) {
    const tabs = e.currentTarget as HTMLElement & { activeTabIndex: number };
    this.mode = tabs.activeTabIndex === 0 ? 'login' : 'register';
    this.error = '';
    this.success = false;
  }

  private async onSubmit(e: Event) {
    e.preventDefault();
    const email = this.emailField.value.trim();
    const password = this.passwordField.value;

    if (!email || !password) return;

    this.loading = true;
    this.error = '';
    this.success = false;

    try {
      let result: AuthResult;
      if (this.mode === 'login') {
        result = await login(email, password);
      } else {
        result = await register(email, password);
      }
      localStorage.setItem('friday.auth.token', result.token);
      this.success = true;
    } catch (err) {
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      this.loading = false;
    }
  }

  private switchMode(mode: Mode) {
    if (this.mode === mode) return;
    this.mode = mode;
    this.error = '';
    this.success = false;
  }

  private renderStatus() {
    if (this.error) {
      return html`<div class="status-error">${this.error}</div>`;
    }
    if (this.success) {
      const verb = this.mode === 'login' ? 'Signed in' : 'Account created';
      return html`<div class="status-success">${verb} successfully.</div>`;
    }
    return nothing;
  }

  render() {
    const isLogin = this.mode === 'login';
    const submitLabel = isLogin ? 'Sign in' : 'Create account';

    return html`
      <friday-auth-card
        headline="Friday"
        subhead="Authentication over gRPC"
      >
        <md-tabs @change=${this.onTabChange}>
          <md-primary-tab ?active=${this.mode === 'login'}>Sign in</md-primary-tab>
          <md-primary-tab ?active=${this.mode === 'register'}>Register</md-primary-tab>
        </md-tabs>

        <form @submit=${this.onSubmit} novalidate>
          <div class="form-fields">
            <md-outlined-text-field
              id="email"
              type="email"
              label="Email"
              autocomplete="email"
              required
              ?disabled=${this.loading}
            >
              <md-icon slot="leading-icon">mail</md-icon>
            </md-outlined-text-field>

            <md-outlined-text-field
              id="password"
              type="password"
              label="Password"
              autocomplete=${isLogin ? 'current-password' : 'new-password'}
              minlength=${ifDefined(isLogin ? undefined : '8')}
              required
              ?disabled=${this.loading}
            >
              <md-icon slot="leading-icon">lock</md-icon>
            </md-outlined-text-field>
          </div>

          <div class="actions">
            ${this.loading
              ? html`<md-linear-progress indeterminate></md-linear-progress>`
              : nothing}

            ${this.renderStatus()}

            <md-filled-button type="submit" ?disabled=${this.loading}>
              ${submitLabel}
            </md-filled-button>
          </div>
        </form>

        <div class="footer-links">
          ${isLogin
            ? html`
                <md-text-button @click=${() => this.switchMode('register')}>
                  No account yet? Register
                </md-text-button>
              `
            : html`
                <md-text-button @click=${() => this.switchMode('login')}>
                  Already have an account? Sign in
                </md-text-button>
              `}
        </div>
      </friday-auth-card>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    'friday-auth-page': FridayAuthPage;
  }
}
