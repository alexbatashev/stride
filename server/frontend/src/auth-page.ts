import { LitElement, html, css, nothing } from 'lit';
import { ifDefined } from 'lit/directives/if-defined.js';
import '@material/web/tabs/tabs.js';
import '@material/web/tabs/primary-tab.js';
import '@material/web/textfield/outlined-text-field.js';
import '@material/web/button/filled-button.js';
import '@material/web/button/text-button.js';
import '@material/web/progress/linear-progress.js';
import '@material/web/icon/icon.js';
import type { MdOutlinedTextField } from '@material/web/textfield/outlined-text-field.js';
import '../../components/friday-auth-card.js';
import { login, logout, register } from './auth-grpc.js';

type Mode = 'login' | 'register';

export class FridayAuthPage extends LitElement {
  static properties = {
    mode: { state: true },
    loading: { state: true },
    error: { state: true },
    success: { state: true },
    authenticated: { state: true },
  };

  private mode: Mode = 'login';
  private loading = false;
  private error = '';
  private success = false;
  private authenticated = false;

  private get emailField(): MdOutlinedTextField | null {
    return this.renderRoot.querySelector('#email') as MdOutlinedTextField | null;
  }

  private get passwordField(): MdOutlinedTextField | null {
    return this.renderRoot.querySelector('#password') as MdOutlinedTextField | null;
  }

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
    await this.submitAuth();
  }

  private async onPrimaryAction(e: Event) {
    e.preventDefault();
    await this.submitAuth();
  }

  private async submitAuth() {
    if (this.loading) return;

    const emailField = this.emailField;
    const passwordField = this.passwordField;
    if (!emailField || !passwordField) return;

    const email = emailField.value.trim();
    const password = passwordField.value;

    if (!email || !password) return;

    this.loading = true;
    this.error = '';
    this.success = false;

    try {
      if (this.mode === 'login') {
        await login(email, password);
      } else {
        await register(email, password);
      }
      this.success = true;
      this.authenticated = true;
    } catch (err) {
      this.error = err instanceof Error ? err.message : String(err);
    } finally {
      this.loading = false;
    }
  }

  private async onLogout(e: Event) {
    e.preventDefault();
    if (this.loading) return;

    this.loading = true;
    this.error = '';
    this.success = false;
    try {
      await logout();
      this.authenticated = false;
      this.passwordField?.select();
      this.error = '';
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

            <md-filled-button
              type="submit"
              @click=${this.onPrimaryAction}
              ?disabled=${this.loading}
            >
              ${submitLabel}
            </md-filled-button>
          </div>
        </form>

        <div class="footer-links">
          ${this.authenticated
            ? html`
                <md-text-button @click=${this.onLogout} ?disabled=${this.loading}>
                  Sign out
                </md-text-button>
              `
            : nothing}
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

customElements.define('friday-auth-page', FridayAuthPage);

declare global {
  interface HTMLElementTagNameMap {
    'friday-auth-page': FridayAuthPage;
  }
}
